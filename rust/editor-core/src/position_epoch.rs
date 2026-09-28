use std::collections::BTreeMap;

use yrs::{IndexScope, StickyIndex};

use crate::session::{ErrorDomain, SessionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundaryAnchors {
    pub(crate) before: StickyIndex,
    pub(crate) after: StickyIndex,
    pub(crate) ancestor: Option<u32>,
    pub(crate) pinned_cell: Option<CellTextPosition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AncestorAnchors {
    pub(crate) before: StickyIndex,
    pub(crate) after: StickyIndex,
    pub(crate) table_cell: bool,
    pub(crate) parent: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellTextPosition {
    pub(crate) cell: usize,
    pub(crate) point: CellTextPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellTextPoint {
    pub(crate) text_offset: u32,
    pub(crate) run: u32,
    pub(crate) run_offset: u32,
    pub(crate) attachment: CellTextAttachment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellTextAttachment {
    PrecedingText,
    FollowingText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PinnedTableCell {
    pub(crate) row: u32,
    pub(crate) column: u32,
    pub(crate) rowspan: u32,
    pub(crate) colspan: u32,
    pub(crate) table_rows: u32,
    pub(crate) table_columns: u32,
    pub(crate) content_fingerprint: u64,
    pub(crate) text_fingerprint: u64,
    pub(crate) run_structure: u64,
}

#[derive(Debug)]
pub(crate) struct EpochBoundaries {
    pub(crate) anchors: Vec<BoundaryAnchors>,
    pub(crate) ancestors: Vec<AncestorAnchors>,
    pub(crate) cells: Vec<PinnedTableCell>,
}

impl EpochBoundaries {
    pub(crate) fn ancestor_chain<'epoch>(
        &'epoch self,
        boundary: &BoundaryAnchors,
    ) -> impl Iterator<Item = &'epoch AncestorAnchors> + Clone + 'epoch {
        ancestor_chain(&self.ancestors, boundary.ancestor)
    }

    pub(crate) fn inside_table_cell(&self, boundary: &BoundaryAnchors) -> bool {
        self.ancestor_chain(boundary)
            .any(|ancestor| ancestor.table_cell)
    }
}

fn ancestor_chain(
    ancestors: &[AncestorAnchors],
    innermost: Option<u32>,
) -> impl Iterator<Item = &AncestorAnchors> + Clone {
    std::iter::successors(
        innermost.and_then(|index| ancestors.get(usize::try_from(index).ok()?)),
        move |ancestor| {
            ancestor
                .parent
                .and_then(|index| ancestors.get(usize::try_from(index).ok()?))
        },
    )
}

pub(crate) struct EpochBoundary<'epoch> {
    pub(crate) anchors: &'epoch BoundaryAnchors,
    pub(crate) ancestors: &'epoch [AncestorAnchors],
    pub(crate) pinned_cell: Option<PinnedCellBoundary<'epoch>>,
    pub(crate) document_revision: u64,
}

impl<'epoch> EpochBoundary<'epoch> {
    pub(crate) fn ancestor_chain(
        &self,
    ) -> impl Iterator<Item = &'epoch AncestorAnchors> + Clone + 'epoch {
        ancestor_chain(self.ancestors, self.anchors.ancestor)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PinnedCellBoundary<'epoch> {
    pub(crate) cell: &'epoch PinnedTableCell,
    pub(crate) point: CellTextPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedBoundary {
    pub(crate) offset: u32,
    pub(crate) fallback: bool,
    pub(crate) left_table_cell: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedEpochRange {
    pub(crate) anchor: u32,
    pub(crate) head: u32,
    pub(crate) fallback: bool,
    pub(crate) left_table_cell: bool,
}

#[derive(Debug)]
struct PositionEpoch {
    editor_lineage: u64,
    document_revision: u64,
    boundaries: EpochBoundaries,
    retained_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PositionEpochLimits {
    pub(crate) max_owners: usize,
    pub(crate) max_boundaries: usize,
    pub(crate) max_retained_bytes: usize,
}

impl Default for PositionEpochLimits {
    fn default() -> Self {
        Self {
            max_owners: 64,
            max_boundaries: 1_000_001,
            max_retained_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PositionEpochStore {
    next_epoch_id: u64,
    epochs: BTreeMap<u64, PositionEpoch>,
    owner_pins: BTreeMap<u64, u64>,
    retained_bytes: usize,
    limits: PositionEpochLimits,
}

impl PositionEpochStore {
    pub(crate) fn new(limits: PositionEpochLimits) -> Self {
        Self {
            next_epoch_id: 1,
            epochs: BTreeMap::new(),
            owner_pins: BTreeMap::new(),
            retained_bytes: 0,
            limits,
        }
    }

    pub(crate) fn admit_boundary_count(&self, count: usize) -> Result<(), SessionError> {
        if count > self.limits.max_boundaries {
            return Err(limit_error(
                "maxPositionEpochBoundaries",
                self.limits.max_boundaries,
                count,
            ));
        }
        Ok(())
    }

    pub(crate) fn install(
        &mut self,
        owner_id: u64,
        editor_lineage: u64,
        document_revision: u64,
        boundaries: EpochBoundaries,
    ) -> Result<u64, SessionError> {
        self.admit_boundary_count(boundaries.anchors.len())?;
        let replacing = self.owner_pins.get(&owner_id).copied();
        if replacing.is_none() && self.owner_pins.len() >= self.limits.max_owners {
            return Err(limit_error(
                "maxPositionEpochOwners",
                self.limits.max_owners,
                self.owner_pins.len().saturating_add(1),
            ));
        }

        let retained_bytes = retained_bytes(&boundaries)?;
        let replaced_bytes = replacing
            .and_then(|epoch_id| self.epochs.get(&epoch_id))
            .map_or(0, |epoch| epoch.retained_bytes);
        let next_retained = self
            .retained_bytes
            .saturating_sub(replaced_bytes)
            .checked_add(retained_bytes)
            .ok_or_else(|| {
                limit_error(
                    "maxPositionEpochRetainedBytes",
                    self.limits.max_retained_bytes,
                    usize::MAX,
                )
            })?;
        if next_retained > self.limits.max_retained_bytes {
            return Err(limit_error(
                "maxPositionEpochRetainedBytes",
                self.limits.max_retained_bytes,
                next_retained,
            ));
        }

        let epoch_id = self.next_epoch_id;
        self.next_epoch_id = self.next_epoch_id.checked_add(1).ok_or_else(|| {
            SessionError::new(
                ErrorDomain::Boundary,
                "POSITION_EPOCH_EXHAUSTED",
                "position epoch identifier space is exhausted",
            )
        })?;

        if let Some(replaced) = replacing {
            self.epochs.remove(&replaced);
        }
        self.owner_pins.insert(owner_id, epoch_id);
        self.epochs.insert(
            epoch_id,
            PositionEpoch {
                editor_lineage,
                document_revision,
                boundaries,
                retained_bytes,
            },
        );
        self.retained_bytes = next_retained;
        Ok(epoch_id)
    }

    pub(crate) fn boundary(
        &self,
        owner_id: u64,
        epoch_id: u64,
        editor_lineage: u64,
        index: u32,
    ) -> Result<EpochBoundary<'_>, SessionError> {
        if self.owner_pins.get(&owner_id).copied() != Some(epoch_id) {
            return Err(invalid_epoch());
        }
        let epoch = self.epochs.get(&epoch_id).ok_or_else(invalid_epoch)?;
        if epoch.editor_lineage != editor_lineage {
            return Err(invalid_epoch());
        }
        let anchors = epoch
            .boundaries
            .anchors
            .get(usize::try_from(index).map_err(|_| invalid_epoch())?)
            .ok_or_else(|| {
                SessionError::new(
                    ErrorDomain::Boundary,
                    "POSITION_INVALID",
                    "position epoch offset is outside the rendered document",
                )
            })?;
        Ok(EpochBoundary {
            anchors,
            ancestors: &epoch.boundaries.ancestors,
            pinned_cell: anchors.pinned_cell.and_then(|position| {
                Some(PinnedCellBoundary {
                    cell: epoch.boundaries.cells.get(position.cell)?,
                    point: position.point,
                })
            }),
            document_revision: epoch.document_revision,
        })
    }

    pub(crate) fn release_owner(&mut self, owner_id: u64) {
        let Some(epoch_id) = self.owner_pins.remove(&owner_id) else {
            return;
        };
        if let Some(epoch) = self.epochs.remove(&epoch_id) {
            self.retained_bytes = self.retained_bytes.saturating_sub(epoch.retained_bytes);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.epochs.clear();
        self.owner_pins.clear();
        self.retained_bytes = 0;
    }
}

fn retained_bytes(boundaries: &EpochBoundaries) -> Result<usize, SessionError> {
    let overflow = || limit_error("maxPositionEpochRetainedBytes", usize::MAX, usize::MAX);
    fn allocated<T>(values: &Vec<T>) -> Option<usize> {
        values.capacity().checked_mul(std::mem::size_of::<T>())
    }
    let fixed = allocated(&boundaries.anchors)
        .zip(allocated(&boundaries.ancestors))
        .zip(allocated(&boundaries.cells))
        .and_then(|((anchors, ancestors), cells)| {
            anchors.checked_add(ancestors)?.checked_add(cells)
        })
        .ok_or_else(overflow)?;
    boundaries
        .anchors
        .iter()
        .flat_map(|boundary| [&boundary.before, &boundary.after])
        .chain(
            boundaries
                .ancestors
                .iter()
                .flat_map(|ancestor| [&ancestor.before, &ancestor.after]),
        )
        .try_fold(fixed, |total, sticky| {
            total.checked_add(sticky_heap_bytes(sticky))
        })
        .ok_or_else(overflow)
}

fn sticky_heap_bytes(sticky: &StickyIndex) -> usize {
    match sticky.scope() {
        IndexScope::Root(name) => name.len(),
        IndexScope::Relative(_) | IndexScope::Nested(_) => 0,
    }
}

pub(crate) fn table_cell_removed(request_id: u64) -> SessionError {
    let mut error = SessionError::new(
        ErrorDomain::Boundary,
        "POSITION_EPOCH_CELL_REMOVED",
        "the table cell addressed by this position epoch no longer exists",
    );
    error.request_id = Some(request_id);
    error
}

fn invalid_epoch() -> SessionError {
    SessionError::new(
        ErrorDomain::Boundary,
        "POSITION_EPOCH_INVALID",
        "position epoch is not pinned by this native owner",
    )
}

fn limit_error(field: &'static str, limit: usize, actual: usize) -> SessionError {
    let mut error = SessionError::new(
        ErrorDomain::Boundary,
        "POSITION_EPOCH_LIMIT_EXCEEDED",
        format!("position epoch exceeds {field}"),
    );
    error.limit = u64::try_from(limit).ok();
    error.actual = u64::try_from(actual).ok();
    error.details = Some(serde_json::json!({"field": field}));
    error
}
