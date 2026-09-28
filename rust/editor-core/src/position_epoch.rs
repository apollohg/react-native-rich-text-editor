use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use yrs::{IndexScope, StickyIndex};

use crate::session::{ErrorDomain, SessionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundaryAnchors {
    pub(crate) before: StickyIndex,
    pub(crate) after: StickyIndex,
    pub(crate) ancestor: Option<Arc<AncestorNode>>,
    pub(crate) pinned_cell: Option<CellTextPosition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AncestorAnchors {
    pub(crate) before: StickyIndex,
    pub(crate) after: StickyIndex,
    pub(crate) table_cell: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AncestorNode {
    pub(crate) anchors: AncestorAnchors,
    pub(crate) parent: Option<Arc<AncestorNode>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PinnedCellSpan {
    pub(crate) node_path: Vec<u32>,
    pub(crate) block_range: std::ops::Range<usize>,
    pub(crate) cell: PinnedTableCell,
    pub(crate) points: Vec<(u32, CellTextPoint)>,
}

#[derive(Debug)]
pub(crate) struct EpochBlockChunk {
    pub(crate) anchors: Vec<BoundaryAnchors>,
    pub(crate) ancestor: Option<Arc<AncestorNode>>,
    pub(crate) retained_bytes: usize,
}

#[derive(Debug)]
pub(crate) struct EpochSnapshot {
    pub(crate) yrs_state_epoch: u64,
    pub(crate) document_revision: u64,
    pub(crate) chunks: Arc<[Arc<EpochBlockChunk>]>,
    pub(crate) scalar_starts: Arc<[u32]>,
    pub(crate) cells: Arc<[Arc<PinnedCellSpan>]>,
    pub(crate) retained_bytes: usize,
}

impl BoundaryAnchors {
    pub(crate) fn ancestor_chain(&self) -> impl Iterator<Item = &AncestorAnchors> + Clone {
        std::iter::successors(self.ancestor.as_deref(), |node| node.parent.as_deref())
            .map(|node| &node.anchors)
    }

    pub(crate) fn inside_table_cell(&self) -> bool {
        self.ancestor_chain().any(|ancestor| ancestor.table_cell)
    }
}

impl EpochBlockChunk {
    pub(crate) fn new(anchors: Vec<BoundaryAnchors>) -> Option<Self> {
        let retained_bytes = anchors.iter().try_fold(
            anchors
                .capacity()
                .checked_mul(std::mem::size_of::<BoundaryAnchors>())?,
            |total, boundary| {
                total
                    .checked_add(sticky_heap_bytes(&boundary.before))?
                    .checked_add(sticky_heap_bytes(&boundary.after))
            },
        )?;
        Some(Self {
            ancestor: anchors
                .first()
                .and_then(|boundary| boundary.ancestor.clone()),
            anchors,
            retained_bytes,
        })
    }
}

impl PinnedCellSpan {
    fn retained_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>()
            .checked_add(
                self.node_path
                    .capacity()
                    .checked_mul(std::mem::size_of::<u32>())?,
            )?
            .checked_add(
                self.points
                    .capacity()
                    .checked_mul(std::mem::size_of::<(u32, CellTextPoint)>())?,
            )
    }
}

impl EpochSnapshot {
    pub(crate) fn new(
        yrs_state_epoch: u64,
        document_revision: u64,
        chunks: Vec<Arc<EpochBlockChunk>>,
        cells: Vec<Arc<PinnedCellSpan>>,
    ) -> Option<Self> {
        let mut scalar_starts = Vec::with_capacity(chunks.len());
        let mut start = 0u32;
        let mut retained_bytes = chunks
            .len()
            .checked_mul(std::mem::size_of::<Arc<EpochBlockChunk>>() + std::mem::size_of::<u32>())?
            .checked_add(
                cells
                    .len()
                    .checked_mul(std::mem::size_of::<Arc<PinnedCellSpan>>())?,
            )?;
        let mut ancestors = HashSet::new();
        for chunk in &chunks {
            scalar_starts.push(start);
            start = start.checked_add(u32::try_from(chunk.anchors.len()).ok()?)?;
            retained_bytes = retained_bytes
                .checked_add(chunk.retained_bytes)?
                .checked_add(std::mem::size_of::<EpochBlockChunk>())?;
            let mut previous = None;
            for innermost in std::iter::once(chunk.ancestor.as_deref()).chain(
                chunk
                    .anchors
                    .iter()
                    .map(|boundary| boundary.ancestor.as_deref()),
            ) {
                let identity = innermost.map(|node| node as *const AncestorNode);
                if identity == previous {
                    continue;
                }
                previous = identity;
                let mut current = innermost;
                while let Some(node) = current {
                    if !ancestors.insert(node as *const AncestorNode) {
                        break;
                    }
                    retained_bytes = retained_bytes
                        .checked_add(std::mem::size_of::<AncestorNode>())?
                        .checked_add(sticky_heap_bytes(&node.anchors.before))?
                        .checked_add(sticky_heap_bytes(&node.anchors.after))?;
                    current = node.parent.as_deref();
                }
            }
        }
        for span in &cells {
            retained_bytes = retained_bytes.checked_add(span.retained_bytes()?)?;
        }
        Some(Self {
            yrs_state_epoch,
            document_revision,
            chunks: chunks.into(),
            scalar_starts: scalar_starts.into(),
            cells: cells.into(),
            retained_bytes,
        })
    }

    pub(crate) fn scalar_starts(chunks: &[Arc<EpochBlockChunk>]) -> Option<Vec<u32>> {
        let mut starts = Vec::with_capacity(chunks.len());
        let mut start = 0u32;
        for chunk in chunks {
            starts.push(start);
            start = start.checked_add(u32::try_from(chunk.anchors.len()).ok()?)?;
        }
        Some(starts)
    }

    pub(crate) fn attach_cells<'a>(
        chunks: &mut [Arc<EpochBlockChunk>],
        starts: &[u32],
        cells: impl Iterator<Item = (usize, &'a Arc<PinnedCellSpan>)>,
    ) -> Option<()> {
        for (cell, span) in cells {
            if span.points.is_empty() {
                continue;
            }
            let origin = *starts.get(span.block_range.start)?;
            let mut points = span.points.iter().peekable();
            while let Some((relative, _)) = points.peek() {
                let scalar = relative.checked_add(origin)?;
                let block = starts
                    .partition_point(|start| *start <= scalar)
                    .checked_sub(1)?;
                let end = starts.get(block + 1).copied().unwrap_or(u32::MAX);
                let chunk = Arc::get_mut(&mut chunks[block])?;
                while points.peek().is_some_and(|(relative, _)| {
                    relative
                        .checked_add(origin)
                        .is_some_and(|scalar| scalar < end)
                }) {
                    let (relative, point) = points.next()?;
                    let offset =
                        usize::try_from(relative.checked_add(origin)?.checked_sub(starts[block])?)
                            .ok()?;
                    let boundary = chunk.anchors.get_mut(offset)?;
                    if boundary.inside_table_cell() {
                        boundary.pinned_cell = Some(CellTextPosition {
                            cell,
                            point: *point,
                        });
                    }
                }
            }
        }
        Some(())
    }

    pub(crate) fn with_rebuilt_chunks(
        &self,
        yrs_state_epoch: u64,
        document_revision: u64,
        mut chunks: Vec<Arc<EpochBlockChunk>>,
        cells: Vec<Arc<PinnedCellSpan>>,
        replaced_chunks: &[usize],
        replaced_cells: &[usize],
    ) -> Option<Self> {
        if chunks.len() != self.chunks.len() || cells.len() != self.cells.len() {
            return None;
        }
        let mut starts = self.scalar_starts.to_vec();
        let mut delta = 0i64;
        for (replacement, &index) in replaced_chunks.iter().enumerate() {
            delta = delta
                .checked_add(i64::try_from(chunks[index].anchors.len()).ok()?)?
                .checked_sub(i64::try_from(self.chunks[index].anchors.len()).ok()?)?;
            let end = replaced_chunks
                .get(replacement + 1)
                .map_or(starts.len(), |next| next + 1);
            if delta != 0 {
                for start in &mut starts[index + 1..end] {
                    *start = u32::try_from(i64::from(*start).checked_add(delta)?).ok()?;
                }
            }
        }
        Self::attach_cells(
            &mut chunks,
            &starts,
            replaced_cells.iter().map(|index| (*index, &cells[*index])),
        )?;
        let mut retained_bytes = self.retained_bytes;
        for &index in replaced_chunks {
            retained_bytes = retained_bytes
                .checked_sub(self.chunks.get(index)?.retained_bytes)?
                .checked_add(chunks.get(index)?.retained_bytes)?;
        }
        for &index in replaced_cells {
            retained_bytes = retained_bytes
                .checked_sub(self.cells.get(index)?.retained_bytes()?)?
                .checked_add(cells.get(index)?.retained_bytes()?)?;
        }
        Some(Self {
            yrs_state_epoch,
            document_revision,
            chunks: chunks.into(),
            scalar_starts: starts.into(),
            cells: cells.into(),
            retained_bytes,
        })
    }

    pub(crate) fn boundary_count(&self) -> usize {
        self.scalar_starts
            .last()
            .zip(self.chunks.last())
            .map_or(0, |(start, chunk)| *start as usize + chunk.anchors.len())
    }

    pub(crate) fn boundary(&self, index: u32) -> Option<EpochBoundary<'_>> {
        let block = self
            .scalar_starts
            .partition_point(|start| *start <= index)
            .checked_sub(1)?;
        let anchors = self
            .chunks
            .get(block)?
            .anchors
            .get(usize::try_from(index.checked_sub(self.scalar_starts[block])?).ok()?)?;
        Some(EpochBoundary {
            anchors,
            pinned_cell: anchors.pinned_cell.and_then(|position| {
                Some(PinnedCellBoundary {
                    cell: &self.cells.get(position.cell)?.cell,
                    point: position.point,
                })
            }),
            document_revision: self.document_revision,
        })
    }

    #[cfg(test)]
    pub(crate) fn ancestor_count(&self) -> usize {
        let mut ancestors = HashSet::new();
        for boundary in self.chunks.iter().flat_map(|chunk| &chunk.anchors) {
            let mut current = boundary.ancestor.as_deref();
            while let Some(node) = current {
                if !ancestors.insert(node as *const AncestorNode) {
                    break;
                }
                current = node.parent.as_deref();
            }
        }
        ancestors.len()
    }
}

pub(crate) struct EpochBoundary<'epoch> {
    pub(crate) anchors: &'epoch BoundaryAnchors,
    pub(crate) pinned_cell: Option<PinnedCellBoundary<'epoch>>,
    pub(crate) document_revision: u64,
}

impl<'epoch> EpochBoundary<'epoch> {
    pub(crate) fn ancestor_chain(
        &self,
    ) -> impl Iterator<Item = &'epoch AncestorAnchors> + Clone + 'epoch {
        self.anchors.ancestor_chain()
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
    snapshot: Arc<EpochSnapshot>,
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
        snapshot: Arc<EpochSnapshot>,
    ) -> Result<u64, SessionError> {
        self.admit_boundary_count(snapshot.boundary_count())?;
        let replacing = self.owner_pins.get(&owner_id).copied();
        if replacing.is_none() && self.owner_pins.len() >= self.limits.max_owners {
            return Err(limit_error(
                "maxPositionEpochOwners",
                self.limits.max_owners,
                self.owner_pins.len().saturating_add(1),
            ));
        }

        let retained_bytes = snapshot.retained_bytes;
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
                snapshot,
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
        epoch.snapshot.boundary(index).ok_or_else(|| {
            SessionError::new(
                ErrorDomain::Boundary,
                "POSITION_INVALID",
                "position epoch offset is outside the rendered document",
            )
        })
    }

    pub(crate) fn is_snapshot_pinned(&self, snapshot: &Arc<EpochSnapshot>) -> bool {
        self.epochs
            .values()
            .any(|epoch| Arc::ptr_eq(&epoch.snapshot, snapshot))
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
