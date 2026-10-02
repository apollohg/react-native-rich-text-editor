use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use yrs::{Assoc, IndexScope, StickyIndex, ID};

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
    pub(crate) anchors: EpochAnchorStorage,
    ancestor: Option<Arc<AncestorNode>>,
    pub(crate) retained_bytes: usize,
}

#[derive(Debug)]
pub(crate) enum EpochAnchorStorage {
    Dense(Vec<BoundaryAnchors>),
    Plain(Box<PlainTextAnchors>),
}

#[derive(Debug)]
pub(crate) struct PlainTextAnchors {
    item: ID,
    offsets: Vec<u32>,
    utf16_length: u32,
    start: BoundaryAnchors,
    end: BoundaryAnchors,
    pins: Vec<Option<CellTextPosition>>,
}

const _: () = assert!(
    std::mem::size_of::<EpochBlockChunk>()
        == std::mem::size_of::<Vec<BoundaryAnchors>>()
            + std::mem::size_of::<Option<Arc<AncestorNode>>>()
            + std::mem::size_of::<usize>()
);

impl PlainTextAnchors {
    fn boundary(&self, index: usize) -> Option<BoundaryAnchors> {
        let offset = *self.offsets.get(index)?;
        let mut anchors = if offset == 0 {
            self.start.clone()
        } else if offset == self.utf16_length {
            self.end.clone()
        } else {
            BoundaryAnchors {
                before: StickyIndex::from_id(
                    ID::new(self.item.client, self.item.clock + offset - 1),
                    Assoc::Before,
                ),
                after: StickyIndex::from_id(
                    ID::new(self.item.client, self.item.clock + offset),
                    Assoc::After,
                ),
                ancestor: self.start.ancestor.clone(),
                pinned_cell: None,
            }
        };
        anchors.pinned_cell = self.pins.get(index).copied().flatten();
        Some(anchors)
    }
}

impl EpochAnchorStorage {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Dense(anchors) => anchors.len(),
            Self::Plain(run) => run.offsets.len(),
        }
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        match self {
            Self::Dense(anchors) => anchors.capacity(),
            Self::Plain(run) => run.offsets.len(),
        }
    }

    pub(crate) fn get(&self, index: usize) -> Option<Cow<'_, BoundaryAnchors>> {
        match self {
            Self::Dense(anchors) => anchors.get(index).map(Cow::Borrowed),
            Self::Plain(run) => run.boundary(index).map(Cow::Owned),
        }
    }

    pub(crate) fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = Cow<'_, BoundaryAnchors>> + ExactSizeIterator {
        (0..self.len()).map(|index| self.get(index).expect("anchor index is in range"))
    }

    pub(crate) fn to_dense(&self) -> Vec<BoundaryAnchors> {
        self.iter().map(Cow::into_owned).collect()
    }

    fn visit_ancestors(&self, mut visit: impl FnMut(Option<&AncestorNode>)) {
        match self {
            Self::Dense(anchors) => {
                for boundary in anchors {
                    visit(boundary.ancestor.as_deref());
                }
            }
            Self::Plain(run) => visit(run.start.ancestor.as_deref()),
        }
    }

    fn attach_cell(&mut self, offset: usize, position: CellTextPosition) -> Option<()> {
        match self {
            Self::Plain(run) => {
                run.offsets.get(offset)?;
                if !run.start.inside_table_cell() {
                    return Some(());
                }
                if run.pins.is_empty() {
                    run.pins = vec![None; run.offsets.len()];
                }
                *run.pins.get_mut(offset)? = Some(position);
            }
            Self::Dense(anchors) => {
                let boundary = anchors.get_mut(offset)?;
                if boundary.inside_table_cell() {
                    boundary.pinned_cell = Some(position);
                }
            }
        }
        Some(())
    }
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

pub(crate) struct EpochSnapshotUpdate {
    yrs_state_epoch: u64,
    document_revision: u64,
    chunks: Vec<Arc<EpochBlockChunk>>,
    chunk_indexes: Vec<usize>,
    cells: Vec<Arc<PinnedCellSpan>>,
    cell_indexes: Vec<usize>,
    scalar_starts: Arc<[u32]>,
    retained_bytes: usize,
    boundary_count: usize,
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
    #[cfg(test)]
    pub(crate) fn stored_anchor_count(&self) -> usize {
        match &self.anchors {
            EpochAnchorStorage::Dense(anchors) => anchors.len(),
            EpochAnchorStorage::Plain(_) => 2,
        }
    }

    pub(crate) fn ancestor(&self) -> Option<Arc<AncestorNode>> {
        self.ancestor.clone()
    }

    pub(crate) fn plain_text(
        item: ID,
        offsets: Vec<u32>,
        utf16_length: u32,
        start: BoundaryAnchors,
        end: BoundaryAnchors,
    ) -> Option<Self> {
        if offsets.is_empty()
            || !offsets.is_sorted()
            || *offsets.last()? > utf16_length
            || item.clock.checked_add(utf16_length).is_none()
            || start.pinned_cell.is_some()
            || end.pinned_cell.is_some()
        {
            return None;
        }
        match (&start.ancestor, &end.ancestor) {
            (Some(a), Some(b)) if Arc::ptr_eq(a, b) => {}
            (None, None) => {}
            _ => return None,
        }
        let start_heap =
            sticky_heap_bytes(&start.before).checked_add(sticky_heap_bytes(&start.after))?;
        let end_heap = sticky_heap_bytes(&end.before).checked_add(sticky_heap_bytes(&end.after))?;
        let retained_bytes = offsets.iter().try_fold(
            offsets
                .len()
                .checked_mul(std::mem::size_of::<BoundaryAnchors>())?,
            |total, offset| {
                total.checked_add(if *offset == 0 {
                    start_heap
                } else if *offset == utf16_length {
                    end_heap
                } else {
                    0
                })
            },
        )?;
        let physical_bytes = std::mem::size_of::<PlainTextAnchors>()
            .checked_add(offsets.capacity().checked_mul(std::mem::size_of::<u32>())?)?
            .checked_add(
                offsets
                    .len()
                    .checked_mul(std::mem::size_of::<Option<CellTextPosition>>())?,
            )?
            .checked_add(start_heap)?
            .checked_add(end_heap)?;
        if physical_bytes > retained_bytes {
            return None;
        }
        Some(Self {
            ancestor: start.ancestor.clone(),
            anchors: EpochAnchorStorage::Plain(Box::new(PlainTextAnchors {
                item,
                offsets,
                utf16_length,
                start,
                end,
                pins: Vec::new(),
            })),
            retained_bytes,
        })
    }

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
            anchors: EpochAnchorStorage::Dense(anchors),
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
            let mut ancestor_bytes = Some(0usize);
            chunk.anchors.visit_ancestors(|innermost| {
                let identity = innermost.map(|node| node as *const AncestorNode);
                if identity == previous {
                    return;
                }
                previous = identity;
                let mut current = innermost;
                while let Some(node) = current {
                    if !ancestors.insert(node as *const AncestorNode) {
                        break;
                    }
                    ancestor_bytes = ancestor_bytes.and_then(|bytes| {
                        bytes
                            .checked_add(std::mem::size_of::<AncestorNode>())?
                            .checked_add(sticky_heap_bytes(&node.anchors.before))?
                            .checked_add(sticky_heap_bytes(&node.anchors.after))
                    });
                    current = node.parent.as_deref();
                }
            });
            retained_bytes = retained_bytes.checked_add(ancestor_bytes?)?;
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
        Self::attach_cells_to_chunks(chunks, starts, cells, Some)
    }

    fn attach_cells_to_chunks<'a>(
        chunks: &mut [Arc<EpochBlockChunk>],
        starts: &[u32],
        cells: impl Iterator<Item = (usize, &'a Arc<PinnedCellSpan>)>,
        chunk_index: impl Fn(usize) -> Option<usize>,
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
                let chunk = Arc::get_mut(chunks.get_mut(chunk_index(block)?)?)?;
                while points.peek().is_some_and(|(relative, _)| {
                    relative
                        .checked_add(origin)
                        .is_some_and(|scalar| scalar < end)
                }) {
                    let (relative, point) = points.next()?;
                    let offset =
                        usize::try_from(relative.checked_add(origin)?.checked_sub(starts[block])?)
                            .ok()?;
                    chunk.anchors.attach_cell(
                        offset,
                        CellTextPosition {
                            cell,
                            point: *point,
                        },
                    )?;
                }
            }
        }
        Some(())
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
        let pinned_cell = anchors.pinned_cell.and_then(|position| {
            Some(PinnedCellBoundary {
                cell: &self.cells.get(position.cell)?.cell,
                point: position.point,
            })
        });
        Some(EpochBoundary {
            anchors,
            pinned_cell,
            document_revision: self.document_revision,
        })
    }

    #[cfg(test)]
    pub(crate) fn ancestor_count(&self) -> usize {
        let mut ancestors = HashSet::new();
        for chunk in self.chunks.iter() {
            chunk.anchors.visit_ancestors(|ancestor| {
                let mut current = ancestor;
                while let Some(node) = current {
                    if !ancestors.insert(node as *const AncestorNode) {
                        break;
                    }
                    current = node.parent.as_deref();
                }
            });
        }
        ancestors.len()
    }
}

impl EpochSnapshotUpdate {
    pub(crate) fn new(
        previous: &EpochSnapshot,
        yrs_state_epoch: u64,
        document_revision: u64,
        mut chunks: Vec<Arc<EpochBlockChunk>>,
        cells: Vec<Arc<PinnedCellSpan>>,
        chunk_indexes: Vec<usize>,
        cell_indexes: Vec<usize>,
    ) -> Option<Self> {
        if chunks.len() != chunk_indexes.len() || cells.len() != cell_indexes.len() {
            return None;
        }
        let mut starts = previous.scalar_starts.to_vec();
        let mut delta = 0i64;
        for (replacement, (&index, chunk)) in chunk_indexes.iter().zip(&chunks).enumerate() {
            delta = delta
                .checked_add(i64::try_from(chunk.anchors.len()).ok()?)?
                .checked_sub(i64::try_from(previous.chunks.get(index)?.anchors.len()).ok()?)?;
            let end = chunk_indexes
                .get(replacement + 1)
                .map_or(starts.len(), |next| next + 1);
            if delta != 0 {
                for start in starts.get_mut(index + 1..end)? {
                    *start = u32::try_from(i64::from(*start).checked_add(delta)?).ok()?;
                }
            }
        }
        EpochSnapshot::attach_cells_to_chunks(
            &mut chunks,
            &starts,
            cell_indexes.iter().copied().zip(&cells),
            |block| chunk_indexes.binary_search(&block).ok(),
        )?;
        let mut retained_bytes = previous.retained_bytes;
        for (&index, chunk) in chunk_indexes.iter().zip(&chunks) {
            retained_bytes = retained_bytes
                .checked_sub(previous.chunks.get(index)?.retained_bytes)?
                .checked_add(chunk.retained_bytes)?;
        }
        for (&index, cell) in cell_indexes.iter().zip(&cells) {
            retained_bytes = retained_bytes
                .checked_sub(previous.cells.get(index)?.retained_bytes()?)?
                .checked_add(cell.retained_bytes()?)?;
        }
        let boundary_count = if let Some(last) = starts.len().checked_sub(1) {
            let chunk = chunk_indexes
                .binary_search(&last)
                .ok()
                .map(|index| &chunks[index])
                .or_else(|| previous.chunks.get(last))?;
            (starts[last] as usize).checked_add(chunk.anchors.len())?
        } else {
            0
        };
        Some(Self {
            yrs_state_epoch,
            document_revision,
            chunks,
            chunk_indexes,
            cells,
            cell_indexes,
            scalar_starts: starts.into(),
            retained_bytes,
            boundary_count,
        })
    }

    fn into_snapshot(mut self, previous: &EpochSnapshot) -> EpochSnapshot {
        let mut chunks = previous.chunks.to_vec();
        let mut cells = previous.cells.to_vec();
        self.replace_arrays(&mut chunks, &mut cells);
        EpochSnapshot {
            yrs_state_epoch: self.yrs_state_epoch,
            document_revision: self.document_revision,
            chunks: chunks.into(),
            scalar_starts: self.scalar_starts,
            cells: cells.into(),
            retained_bytes: self.retained_bytes,
        }
    }

    fn replace_arrays(
        &mut self,
        chunks: &mut [Arc<EpochBlockChunk>],
        cells: &mut [Arc<PinnedCellSpan>],
    ) {
        for (index, chunk) in self
            .chunk_indexes
            .iter()
            .copied()
            .zip(self.chunks.drain(..))
        {
            chunks[index] = chunk;
        }
        for (index, cell) in self.cell_indexes.iter().copied().zip(self.cells.drain(..)) {
            cells[index] = cell;
        }
    }

    fn try_apply_exclusive(mut self, previous: &mut Arc<EpochSnapshot>) -> Result<(), Self> {
        let Some(previous) = Arc::get_mut(previous) else {
            return Err(self);
        };
        let Some(chunks) = Arc::get_mut(&mut previous.chunks) else {
            return Err(self);
        };
        let Some(cells) = Arc::get_mut(&mut previous.cells) else {
            return Err(self);
        };
        self.replace_arrays(chunks, cells);
        previous.yrs_state_epoch = self.yrs_state_epoch;
        previous.document_revision = self.document_revision;
        previous.scalar_starts = self.scalar_starts;
        previous.retained_bytes = self.retained_bytes;
        Ok(())
    }
}

pub(crate) struct EpochBoundary<'epoch> {
    pub(crate) anchors: Cow<'epoch, BoundaryAnchors>,
    pub(crate) pinned_cell: Option<PinnedCellBoundary<'epoch>>,
    pub(crate) document_revision: u64,
}

impl<'epoch> EpochBoundary<'epoch> {
    pub(crate) fn ancestor_chain(&self) -> impl Iterator<Item = &AncestorAnchors> + Clone + '_ {
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

struct PositionEpochInstallation {
    replacing: Option<u64>,
    epoch_id: u64,
    next_epoch_id: u64,
    next_retained: usize,
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
        let installation =
            self.prepare_install(owner_id, snapshot.boundary_count(), snapshot.retained_bytes)?;
        Ok(self.commit_install(owner_id, editor_lineage, snapshot, installation))
    }

    pub(crate) fn install_update(
        &mut self,
        owner_id: u64,
        editor_lineage: u64,
        latest: &mut Arc<EpochSnapshot>,
        update: EpochSnapshotUpdate,
    ) -> Result<u64, SessionError> {
        let replacing_latest = self
            .owner_pins
            .get(&owner_id)
            .and_then(|epoch| self.epochs.get(epoch))
            .is_some_and(|epoch| Arc::ptr_eq(&epoch.snapshot, latest));
        let installation =
            self.prepare_install(owner_id, update.boundary_count, update.retained_bytes)?;
        let update = if replacing_latest {
            self.epochs
                .remove(&installation.replacing.expect("replaced owner epoch"));
            update.try_apply_exclusive(latest).err()
        } else {
            Some(update)
        };
        if let Some(update) = update {
            *latest = Arc::new(update.into_snapshot(latest));
        }
        Ok(self.commit_install(owner_id, editor_lineage, latest.clone(), installation))
    }

    fn prepare_install(
        &self,
        owner_id: u64,
        boundary_count: usize,
        retained_bytes: usize,
    ) -> Result<PositionEpochInstallation, SessionError> {
        self.admit_boundary_count(boundary_count)?;
        let replacing = self.owner_pins.get(&owner_id).copied();
        if replacing.is_none() && self.owner_pins.len() >= self.limits.max_owners {
            return Err(limit_error(
                "maxPositionEpochOwners",
                self.limits.max_owners,
                self.owner_pins.len().saturating_add(1),
            ));
        }

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
        let next_epoch_id = self.next_epoch_id.checked_add(1).ok_or_else(|| {
            SessionError::new(
                ErrorDomain::Boundary,
                "POSITION_EPOCH_EXHAUSTED",
                "position epoch identifier space is exhausted",
            )
        })?;

        Ok(PositionEpochInstallation {
            replacing,
            epoch_id,
            next_epoch_id,
            next_retained,
        })
    }

    fn commit_install(
        &mut self,
        owner_id: u64,
        editor_lineage: u64,
        snapshot: Arc<EpochSnapshot>,
        installation: PositionEpochInstallation,
    ) -> u64 {
        let PositionEpochInstallation {
            replacing,
            epoch_id,
            next_epoch_id,
            next_retained,
        } = installation;
        let retained_bytes = snapshot.retained_bytes;
        self.next_epoch_id = next_epoch_id;
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
        epoch_id
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

#[cfg(test)]
#[path = "position_epoch_tests.rs"]
mod tests;
