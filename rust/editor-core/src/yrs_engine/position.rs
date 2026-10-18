use yrs::branch::{Branch, BranchID, BranchPtr};
use yrs::types::text::{Text, YChange};
use yrs::types::xml::{XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut, XmlTextRef};
use yrs::types::TypeRef;
use yrs::{Any, Assoc, IndexScope, Offset, ReadTxn, StickyIndex};

use crate::model::Document;
use crate::position::PositionMap;
use crate::position_epoch::{AncestorAnchors, AncestorNode, BoundaryAnchors, EpochBlockChunk};
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::commands::{NODE_CLOSING_TOKENS, NODE_OPENING_TOKENS};
use std::sync::Arc;

use super::block_branch_index::BlockBranchIndex;
use super::{Affinity, EditorOffsetKind, RevisionedPosition};

pub(super) const VOID_NODE_SIZE: u32 = 1;

#[cfg(test)]
std::thread_local! {
    static RELATIVE_FULL_SIZE_PREPASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RELATIVE_FORWARD_TRAVERSALS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RELATIVE_BLOCK_TRAVERSALS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RELATIVE_REVERSE_TRAVERSALS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOUNDARY_WALK_NODE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOUNDARY_TEXT_SCALAR_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOUNDARY_SORT_TARGETS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static BOUNDARY_COMPACTION_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(super) static DENSE_BOUNDARY_STAGING_ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PLAIN_TEXT_DIFF_FALLBACKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static XML_SIZE_NODE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_relative_position_traversal_counts_for_test() {
    RELATIVE_FULL_SIZE_PREPASSES.set(0);
    RELATIVE_FORWARD_TRAVERSALS.set(0);
    RELATIVE_REVERSE_TRAVERSALS.set(0);
}

#[cfg(test)]
pub(crate) fn take_relative_block_traversals_for_test() -> usize {
    RELATIVE_BLOCK_TRAVERSALS.replace(0)
}

#[cfg(test)]
pub(crate) fn take_relative_position_traversal_counts_for_test() -> (usize, usize, usize) {
    (
        RELATIVE_FULL_SIZE_PREPASSES.replace(0),
        RELATIVE_FORWARD_TRAVERSALS.replace(0),
        RELATIVE_REVERSE_TRAVERSALS.replace(0),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativePoint {
    pub sticky: StickyIndex,
    pub affinity: Affinity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelativeSelection {
    Text {
        anchor: RelativePoint,
        head: RelativePoint,
    },
    Node {
        point: RelativePoint,
    },
    Cell {
        anchor: RelativePoint,
        head: RelativePoint,
    },
    All,
}

pub fn doc_pos_to_relative_point<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_pos: u32,
    affinity: Affinity,
    schema: &Schema,
) -> Option<RelativePoint> {
    let sticky = doc_pos_to_sticky_index(txn, fragment, doc_pos, affinity.into(), schema)?;
    Some(RelativePoint { sticky, affinity })
}

/// Materialize a relative point after the caller has admitted the complete
/// ready-document scan budget and resolved `doc_pos` against the certified
/// derived document. The forward traversal itself proves the position exists,
/// so it deliberately avoids the general helper's full-fragment size prepass.
pub(crate) fn admitted_doc_pos_to_relative_point<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_pos: u32,
    affinity: Affinity,
    schema: &Schema,
    indexed_view: Option<(&BlockBranchIndex, &PositionMap, &Document)>,
) -> Option<RelativePoint> {
    let assoc = affinity.into();
    let sticky = indexed_view
        .and_then(|(index, position_map, document)| {
            let sticky = index.sticky_at_doc_pos(txn, doc_pos, assoc, position_map, document)?;
            #[cfg(test)]
            RELATIVE_BLOCK_TRAVERSALS.set(RELATIVE_BLOCK_TRAVERSALS.get().saturating_add(1));
            Some(sticky)
        })
        .or_else(|| forward_doc_pos_to_sticky_index(txn, fragment, doc_pos, assoc, schema))?;
    sticky.get_offset(txn)?;
    (sticky.assoc == assoc).then_some(RelativePoint { sticky, affinity })
}

#[cfg(test)]
pub(crate) fn forward_doc_pos_to_sticky_index_for_test<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_pos: u32,
    assoc: Assoc,
    schema: &Schema,
) -> Option<StickyIndex> {
    forward_doc_pos_to_sticky_index(txn, fragment, doc_pos, assoc, schema)
}

pub fn relative_point_to_doc_pos<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    point: &RelativePoint,
    schema: &Schema,
) -> Option<u32> {
    sticky_index_to_doc_pos(txn, fragment, &point.sticky, schema)
}

pub(crate) fn relative_selection_resolves<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    relative: &RelativeSelection,
    schema: &Schema,
) -> bool {
    let resolves = |point| relative_point_to_doc_pos(txn, fragment, point, schema).is_some();
    match relative {
        RelativeSelection::Text { anchor, head } | RelativeSelection::Cell { anchor, head } => {
            resolves(anchor) && resolves(head)
        }
        RelativeSelection::Node { point } => resolves(point),
        RelativeSelection::All => true,
    }
}

pub fn relative_selection_to_selection<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    relative: &RelativeSelection,
    schema: &Schema,
    document: &Document,
    position_map: &PositionMap,
) -> Option<Selection> {
    relative_selection_with_resolver(relative, document, position_map, |point| {
        relative_point_to_doc_pos(txn, fragment, point, schema)
    })
}

pub(crate) fn relative_selection_with_resolver(
    relative: &RelativeSelection,
    document: &Document,
    position_map: &PositionMap,
    resolve: impl Fn(&RelativePoint) -> Option<u32>,
) -> Option<Selection> {
    let selection = match relative {
        RelativeSelection::Text { anchor, head } => {
            Selection::text(resolve(anchor)?, resolve(head)?)
        }
        RelativeSelection::Node { point } => Selection::node(resolve(point)?),
        RelativeSelection::Cell { anchor, head } => {
            Selection::cell(resolve(anchor)?, resolve(head)?)
        }
        RelativeSelection::All => Selection::all(),
    };
    Some(selection.normalized(document, position_map))
}

#[allow(clippy::too_many_arguments)]
pub fn revisioned_position_to_relative_point<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    position: RevisionedPosition,
    rendered_text: &str,
    position_map: &PositionMap,
    document: &Document,
    schema: &Schema,
    block_branch_index: Option<&BlockBranchIndex>,
) -> Option<RelativePoint> {
    let doc_pos = editor_offset_to_doc_pos(
        position.offset,
        position.kind,
        rendered_text,
        position_map,
        document,
    )?;
    let assoc = position.affinity.into();
    block_branch_index
        .and_then(|index| index.sticky_at_doc_pos(txn, doc_pos, assoc, position_map, document))
        .filter(|sticky| sticky.get_offset(txn).is_some() && sticky.assoc == assoc)
        .map(|sticky| RelativePoint {
            sticky,
            affinity: position.affinity,
        })
        .or_else(|| doc_pos_to_relative_point(txn, fragment, doc_pos, position.affinity, schema))
}

pub(crate) fn editor_offset_to_doc_pos(
    offset: u32,
    kind: EditorOffsetKind,
    rendered_text: &str,
    position_map: &PositionMap,
    document: &Document,
) -> Option<u32> {
    let scalar_offset = editor_offset_to_scalar(offset, kind, rendered_text, position_map)?;
    Some(position_map.scalar_to_doc(scalar_offset, document))
}

pub(crate) fn editor_offset_to_scalar(
    offset: u32,
    kind: EditorOffsetKind,
    rendered_text: &str,
    position_map: &PositionMap,
) -> Option<u32> {
    let scalar_offset = match kind {
        EditorOffsetKind::Scalar => offset,
        EditorOffsetKind::Utf16 => utf16_offset_to_scalar(rendered_text, offset)?,
    };
    (scalar_offset <= position_map.total_scalars()).then_some(scalar_offset)
}

pub(crate) fn sticky_index_to_doc_pos<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    sticky_index: &StickyIndex,
    schema: &Schema,
) -> Option<u32> {
    #[cfg(test)]
    RELATIVE_REVERSE_TRAVERSALS.set(RELATIVE_REVERSE_TRAVERSALS.get().saturating_add(1));
    let offset = sticky_index.get_offset(txn)?;
    offset_to_doc_pos(txn, fragment, &offset, schema)
}

pub(crate) fn surviving_relative_point_to_doc_pos<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    point: &RelativePoint,
    schema: &Schema,
) -> Option<u32> {
    let mut offset = point.sticky.get_offset(txn)?;
    let mut climbed = false;
    let mut removed_table_structure = false;
    loop {
        if let Some(position) = offset_to_doc_pos(txn, fragment, &offset, schema) {
            return (!climbed || removed_table_structure).then_some(position);
        }
        let BranchID::Nested(removed_container) = offset.branch.id() else {
            return None;
        };
        removed_table_structure |= is_table_structure_branch(offset.branch, txn, schema);
        climbed = true;
        offset = StickyIndex::new(IndexScope::Relative(removed_container), Assoc::After)
            .get_offset(txn)?;
    }
}

fn is_table_structure_branch<T: ReadTxn>(branch: BranchPtr, txn: &T, schema: &Schema) -> bool {
    matches!(branch.type_ref(), TypeRef::XmlElement(_))
        && super::codec::wire_element_node_spec(&XmlElementRef::from(branch), txn, schema)
            .is_some_and(|spec| spec.table_role.is_some())
}

fn offset_to_doc_pos<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    offset: &Offset,
    schema: &Schema,
) -> Option<u32> {
    #[cfg(test)]
    super::observability::record_yrs_tree_walk();
    let root_branch = BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(fragment));
    if offset.branch == root_branch {
        return sequence_branch_index_to_doc_pos(
            txn,
            fragment.children(txn),
            offset.index,
            &|child| xml_out_pm_size(txn, child, schema),
        );
    }
    let mut child_start = 0u32;
    for child in fragment.children(txn) {
        if let Some(position) = sticky_index_to_doc_pos_in_node(
            txn,
            &child,
            offset.branch,
            offset.index,
            child_start,
            schema,
        ) {
            return Some(position);
        }
        child_start = child_start.checked_add(xml_out_pm_size(txn, &child, schema)?)?;
    }
    None
}

fn sticky_index_to_doc_pos_in_node<T: ReadTxn>(
    txn: &T,
    node: &XmlOut,
    target_branch: BranchPtr,
    target_index: u32,
    node_start: u32,
    schema: &Schema,
) -> Option<u32> {
    match node {
        XmlOut::Text(text) => {
            let text_branch = BranchPtr::from(<XmlTextRef as AsRef<Branch>>::as_ref(text));
            if text_branch == target_branch {
                let text_value = xml_text_plain_string(text, txn)?;
                let scalar_offset = utf16_offset_to_scalar(&text_value, target_index)?;
                return Some(node_start + scalar_offset);
            }
            None
        }
        XmlOut::Element(element) => {
            if is_void_element(element, txn, schema) {
                return None;
            }
            let element_branch = BranchPtr::from(<XmlElementRef as AsRef<Branch>>::as_ref(element));
            if element_branch == target_branch {
                let content_start = node_start + 1;
                return sequence_branch_index_to_doc_pos(
                    txn,
                    element.children(txn),
                    target_index,
                    &|child| xml_out_pm_size(txn, child, schema),
                )
                .map(|value| content_start + value);
            }

            let mut child_start = node_start + 1;
            for child in element.children(txn) {
                if let Some(position) = sticky_index_to_doc_pos_in_node(
                    txn,
                    &child,
                    target_branch,
                    target_index,
                    child_start,
                    schema,
                ) {
                    return Some(position);
                }
                child_start = child_start.checked_add(xml_out_pm_size(txn, &child, schema)?)?;
            }
            None
        }
        XmlOut::Fragment(fragment) => {
            let fragment_branch =
                BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(fragment));
            if fragment_branch == target_branch {
                return sequence_branch_index_to_doc_pos(
                    txn,
                    fragment.children(txn),
                    target_index,
                    &|child| xml_out_pm_size(txn, child, schema),
                )
                .map(|value| node_start + value);
            }

            let mut child_start = node_start;
            for child in fragment.children(txn) {
                if let Some(position) = sticky_index_to_doc_pos_in_node(
                    txn,
                    &child,
                    target_branch,
                    target_index,
                    child_start,
                    schema,
                ) {
                    return Some(position);
                }
                child_start = child_start.checked_add(xml_out_pm_size(txn, &child, schema)?)?;
            }
            None
        }
    }
}

pub(super) fn sequence_branch_index_to_doc_pos<'a, T: ReadTxn>(
    txn: &T,
    children: impl Iterator<Item = XmlOut> + 'a,
    target_index: u32,
    node_size: &impl Fn(&XmlOut) -> Option<u32>,
) -> Option<u32> {
    let mut branch_index = 0u32;
    let mut doc_pos = 0u32;

    for child in children {
        match &child {
            XmlOut::Text(text) => {
                let text_value = xml_text_plain_string(text, txn)?;
                let text_scalar_len = scalar_len(&text_value);
                if target_index == branch_index {
                    return Some(doc_pos);
                }
                branch_index += 1;
                doc_pos += text_scalar_len;
                if target_index == branch_index {
                    return Some(doc_pos);
                }
            }
            XmlOut::Element(_) | XmlOut::Fragment(_) => {
                if target_index == branch_index {
                    return Some(doc_pos);
                }
                branch_index += 1;
                doc_pos = doc_pos.checked_add(node_size(&child)?)?;
                if target_index == branch_index {
                    return Some(doc_pos);
                }
            }
        }
    }

    if target_index == branch_index {
        Some(doc_pos)
    } else {
        None
    }
}

pub(crate) fn doc_pos_to_sticky_index<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_pos: u32,
    assoc: Assoc,
    schema: &Schema,
) -> Option<StickyIndex> {
    #[cfg(test)]
    RELATIVE_FULL_SIZE_PREPASSES.set(RELATIVE_FULL_SIZE_PREPASSES.get().saturating_add(1));
    #[cfg(test)]
    super::observability::record_yrs_tree_walk();
    let content_size = xml_fragment_pm_content_size(txn, fragment, schema)?;
    if doc_pos > content_size {
        return None;
    }
    forward_doc_pos_to_sticky_index(txn, fragment, doc_pos, assoc, schema)
}

fn forward_doc_pos_to_sticky_index<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_pos: u32,
    assoc: Assoc,
    schema: &Schema,
) -> Option<StickyIndex> {
    #[cfg(test)]
    RELATIVE_FORWARD_TRAVERSALS.set(RELATIVE_FORWARD_TRAVERSALS.get().saturating_add(1));
    doc_pos_to_sticky_index_in_sequence(
        txn,
        fragment.children(txn),
        doc_pos,
        assoc,
        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(fragment)),
        &|child| xml_out_pm_size(txn, child, schema),
    )
}

pub(crate) fn boundary_chunks_at_doc_positions<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_positions: &[Vec<u32>],
    schema: &Schema,
) -> Option<Vec<Arc<EpochBlockChunk>>> {
    #[cfg(test)]
    super::observability::record_yrs_tree_walk();
    boundary_chunks_in_sequence(
        txn,
        fragment.children(txn),
        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(fragment)),
        SequenceStart::FIRST_CHILD,
        0,
        doc_positions,
        schema,
        None,
    )
}

pub(crate) fn boundary_chunks_in_root_window<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    children: std::ops::Range<u32>,
    start: u32,
    doc_positions: &[Vec<u32>],
    schema: &Schema,
) -> Option<Vec<Arc<EpochBlockChunk>>> {
    let mut siblings = fragment.children(txn);
    let previous_id = match children.start.checked_sub(1) {
        Some(previous) => Some(integrated_xml_child_id(
            &siblings.by_ref().nth(usize::try_from(previous).ok()?)?,
        )?),
        None => None,
    };
    let lookahead = usize::try_from(children.end - children.start)
        .ok()?
        .checked_add(1)?;
    boundary_chunks_in_sequence(
        txn,
        siblings.take(lookahead),
        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(fragment)),
        SequenceStart {
            index: children.start,
            previous_id,
        },
        start,
        doc_positions,
        schema,
        None,
    )
}

pub(crate) fn boundary_chunk_for_block<T: ReadTxn>(
    txn: &T,
    element: &XmlElementRef,
    doc_start: u32,
    positions: Vec<u32>,
    schema: &Schema,
    ancestor: Option<Arc<AncestorNode>>,
) -> Option<Arc<EpochBlockChunk>> {
    boundary_chunks_in_sequence(
        txn,
        element.children(txn),
        BranchPtr::from(<XmlElementRef as AsRef<Branch>>::as_ref(element)),
        SequenceStart::FIRST_CHILD,
        doc_start,
        &[positions],
        schema,
        ancestor,
    )?
    .into_iter()
    .next()
}

enum BoundaryTargets<'positions> {
    Ordered {
        positions: &'positions [Vec<u32>],
        block: usize,
        offset: usize,
    },
    Sorted {
        targets: Vec<(u32, usize, usize)>,
        index: usize,
    },
}

impl<'positions> BoundaryTargets<'positions> {
    fn new(positions: &'positions [Vec<u32>]) -> Option<Self> {
        let count = positions
            .iter()
            .try_fold(0usize, |total, block| total.checked_add(block.len()))?;
        if positions.iter().flatten().is_sorted() {
            return Some(Self::Ordered {
                positions,
                block: positions
                    .iter()
                    .position(|block| !block.is_empty())
                    .unwrap_or(positions.len()),
                offset: 0,
            });
        }
        let mut targets = Vec::new();
        targets.try_reserve_exact(count).ok()?;
        for (block, positions) in positions.iter().enumerate() {
            targets.extend(
                positions
                    .iter()
                    .enumerate()
                    .map(|(offset, position)| (*position, block, offset)),
            );
        }
        targets.sort_unstable();
        #[cfg(test)]
        BOUNDARY_SORT_TARGETS.set(BOUNDARY_SORT_TARGETS.get().saturating_add(targets.len()));
        Some(Self::Sorted { targets, index: 0 })
    }

    fn peek(&self) -> Option<(u32, usize, usize)> {
        match self {
            Self::Ordered {
                positions,
                block,
                offset,
            } => positions
                .get(*block)?
                .get(*offset)
                .map(|position| (*position, *block, *offset)),
            Self::Sorted { targets, index } => targets.get(*index).copied(),
        }
    }

    fn advance(&mut self) {
        match self {
            Self::Ordered {
                positions,
                block,
                offset,
            } => {
                *offset += 1;
                while *block < positions.len() && *offset >= positions[*block].len() {
                    *block += 1;
                    *offset = 0;
                }
            }
            Self::Sorted { index, .. } => *index += 1,
        }
    }
}

enum PendingBoundaryChunk {
    Empty(usize),
    Dense {
        anchors: Vec<Option<BoundaryAnchors>>,
        unresolved: usize,
    },
    Ready(EpochBlockChunk),
}

#[derive(Default)]
struct BoundaryStaging {
    pending: Vec<Option<BoundaryAnchors>>,
    resolved: Vec<BoundaryAnchors>,
}

impl PendingBoundaryChunk {
    fn insert(
        &mut self,
        offset: usize,
        boundary: BoundaryAnchors,
        staging: &mut BoundaryStaging,
    ) -> Option<()> {
        if let Self::Empty(length) = self {
            let mut anchors = std::mem::take(&mut staging.pending);
            #[cfg(test)]
            if anchors.capacity() < *length {
                DENSE_BOUNDARY_STAGING_ALLOCATIONS
                    .set(DENSE_BOUNDARY_STAGING_ALLOCATIONS.get().saturating_add(1));
            }
            anchors.resize(*length, None);
            *self = Self::Dense {
                anchors,
                unresolved: *length,
            };
        }
        let Self::Dense {
            anchors,
            unresolved,
        } = self
        else {
            unreachable!("completed chunk has no pending targets")
        };
        if anchors.get_mut(offset)?.replace(boundary).is_none() {
            *unresolved -= 1;
        }
        if *unresolved == 0 {
            let mut pending = std::mem::take(anchors);
            staging.resolved.clear();
            for anchor in pending.drain(..) {
                staging.resolved.push(anchor?);
            }
            *self = Self::Ready(completed_boundary_chunk(&staging.resolved)?);
            staging.resolved.clear();
            if pending.capacity() > staging.pending.capacity() {
                staging.pending = pending;
            }
        }
        Some(())
    }

    fn finish(self) -> Option<EpochBlockChunk> {
        match self {
            Self::Empty(0) => EpochBlockChunk::new(Vec::new()),
            Self::Empty(_) | Self::Dense { .. } => None,
            Self::Ready(chunk) => Some(chunk),
        }
    }
}

fn completed_boundary_chunk(anchors: &[BoundaryAnchors]) -> Option<EpochBlockChunk> {
    #[cfg(test)]
    if BOUNDARY_COMPACTION_DISABLED.get() {
        return EpochBlockChunk::new(anchors.to_vec());
    }
    EpochBlockChunk::compacted(anchors)
}

fn boundary_chunks_in_sequence<T: ReadTxn>(
    txn: &T,
    children: impl Iterator<Item = XmlOut>,
    branch: BranchPtr,
    sequence_start: SequenceStart,
    start: u32,
    doc_positions: &[Vec<u32>],
    schema: &Schema,
    ancestor: Option<Arc<AncestorNode>>,
) -> Option<Vec<Arc<EpochBlockChunk>>> {
    let mut walk = BoundaryAnchorWalk {
        txn,
        schema,
        targets: BoundaryTargets::new(doc_positions)?,
        chunks: doc_positions
            .iter()
            .map(|positions| PendingBoundaryChunk::Empty(positions.len()))
            .collect(),
        staging: BoundaryStaging::default(),
        open: ancestor.into_iter().collect(),
    };
    walk.walk_sequence(children, branch, sequence_start, start, None)?;
    if walk.targets.peek().is_some() {
        return None;
    }
    walk.chunks
        .into_iter()
        .map(|chunk| chunk.finish().map(Arc::new))
        .collect()
}

#[derive(Clone, Copy)]
struct SequenceStart {
    index: u32,
    previous_id: Option<yrs::ID>,
}

impl SequenceStart {
    const FIRST_CHILD: Self = Self {
        index: 0,
        previous_id: None,
    };
}

struct BoundaryAnchorWalk<'walk, T> {
    txn: &'walk T,
    schema: &'walk Schema,
    targets: BoundaryTargets<'walk>,
    chunks: Vec<PendingBoundaryChunk>,
    staging: BoundaryStaging,
    open: Vec<Arc<AncestorNode>>,
}

impl<T: ReadTxn> BoundaryAnchorWalk<'_, T> {
    fn pending(&self, sequence_end: Option<u32>) -> Option<u32> {
        self.targets
            .peek()
            .map(|target| target.0)
            .filter(|target| sequence_end.is_none_or(|end| *target < end))
    }

    fn resolve(&mut self, mut leaf: BoundaryAnchors) -> Option<()> {
        leaf.ancestor = self.open.last().cloned();
        let position = self.targets.peek().expect("pending boundary target").0;
        loop {
            let (_, block, offset) = self.targets.peek().expect("pending boundary target");
            self.targets.advance();
            if self
                .targets
                .peek()
                .is_some_and(|target| target.0 == position)
            {
                self.chunks[block].insert(offset, leaf.clone(), &mut self.staging)?;
            } else {
                return self.chunks[block].insert(offset, leaf, &mut self.staging);
            }
        }
    }

    fn resolve_at_child(
        &mut self,
        branch: BranchPtr,
        index: u32,
        position: u32,
        sequence_end: Option<u32>,
    ) -> Option<()> {
        if self.pending(sequence_end) == Some(position) {
            self.resolve(boundary_anchors_at(self.txn, branch, index)?)?;
        }
        Some(())
    }

    fn enter(
        &mut self,
        branch: BranchPtr,
        index: u32,
        table_cell: bool,
        before: Option<StickyIndex>,
        after: Option<StickyIndex>,
    ) -> Option<()> {
        let entered = Arc::new(AncestorNode {
            anchors: AncestorAnchors {
                before: before.or_else(|| sticky_at(self.txn, branch, index, Assoc::Before))?,
                after: after
                    .or_else(|| sticky_at(self.txn, branch, index.checked_add(1)?, Assoc::After))?,
                table_cell,
            },
            parent: self.open.last().cloned(),
        });
        self.open.push(entered);
        Some(())
    }

    fn leave(&mut self) {
        self.open.pop();
    }

    fn walk_sequence(
        &mut self,
        children: impl Iterator<Item = XmlOut>,
        branch: BranchPtr,
        sequence_start: SequenceStart,
        start: u32,
        sequence_end: Option<u32>,
    ) -> Option<u32> {
        let mut index = sequence_start.index;
        let mut position = start;
        let mut children = children.peekable();
        let mut previous_id = sequence_start.previous_id;
        while let Some(child) = children.next() {
            let current_id = integrated_xml_child_id(&child);
            let before = if index == 0 {
                Some(StickyIndex::new(
                    IndexScope::from_branch(branch),
                    Assoc::Before,
                ))
            } else {
                previous_id.map(|id| StickyIndex::from_id(id, Assoc::Before))
            };
            let after = match children.peek() {
                Some(next) => {
                    integrated_xml_child_id(next).map(|id| StickyIndex::from_id(id, Assoc::After))
                }
                None if index.checked_add(1) == Some(branch.content_len()) => {
                    current_id.map(|id| StickyIndex::from_id(id, Assoc::Before))
                }
                None => None,
            };
            #[cfg(test)]
            BOUNDARY_WALK_NODE_VISITS.set(BOUNDARY_WALK_NODE_VISITS.get().saturating_add(1));
            position = match &child {
                XmlOut::Text(text) => {
                    let followed_by_text = matches!(children.peek(), Some(XmlOut::Text(_)));
                    self.walk_text(
                        text,
                        branch,
                        index,
                        position,
                        sequence_end,
                        followed_by_text,
                    )?
                }
                XmlOut::Element(element) => {
                    self.resolve_at_child(branch, index, position, sequence_end)?;
                    if is_void_element(element, self.txn, self.schema) {
                        position.checked_add(VOID_NODE_SIZE)?
                    } else {
                        self.enter(
                            branch,
                            index,
                            is_table_cell_element(element, self.txn, self.schema),
                            before,
                            after,
                        )?;
                        let content_end = self.walk_sequence(
                            element.children(self.txn),
                            BranchPtr::from(<XmlElementRef as AsRef<Branch>>::as_ref(element)),
                            SequenceStart::FIRST_CHILD,
                            position.checked_add(NODE_OPENING_TOKENS)?,
                            None,
                        )?;
                        self.leave();
                        content_end.checked_add(NODE_CLOSING_TOKENS)?
                    }
                }
                XmlOut::Fragment(nested) => {
                    self.resolve_at_child(branch, index, position, sequence_end)?;
                    let fragment_end =
                        position.checked_add(xml_out_pm_size(self.txn, &child, self.schema)?)?;
                    self.enter(branch, index, false, before, after)?;
                    self.walk_sequence(
                        nested.children(self.txn),
                        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(nested)),
                        SequenceStart::FIRST_CHILD,
                        position,
                        Some(fragment_end),
                    )?;
                    self.leave();
                    fragment_end
                }
            };
            previous_id = current_id;
            index = index.checked_add(1)?;
        }
        self.resolve_at_child(branch, index, position, sequence_end)?;
        Some(position)
    }

    fn walk_text(
        &mut self,
        text: &XmlTextRef,
        branch: BranchPtr,
        index: u32,
        start: u32,
        sequence_end: Option<u32>,
        followed_by_text: bool,
    ) -> Option<u32> {
        let (single_item, value) = match text.try_single_text_item(self.txn) {
            Some((id, value)) => (Some(id), value),
            None => (None, xml_text_plain_string(text, self.txn)?),
        };
        let text_end = start.checked_add(scalar_len(&value))?;
        let text_branch = BranchPtr::from(<XmlTextRef as AsRef<Branch>>::as_ref(text));
        let mut characters = value.chars();
        let mut scalar_offset = 0u32;
        let mut utf16_offset = 0u32;
        while let Some(target) = self
            .pending(sequence_end)
            .filter(|target| *target <= text_end)
        {
            let target_offset = target.checked_sub(start)?;
            while scalar_offset < target_offset {
                let width = u32::try_from(characters.next()?.len_utf16()).ok()?;
                #[cfg(test)]
                BOUNDARY_TEXT_SCALAR_VISITS
                    .set(BOUNDARY_TEXT_SCALAR_VISITS.get().saturating_add(1));
                utf16_offset = utf16_offset.checked_add(width)?;
                scalar_offset += 1;
            }
            let anchors = text_boundary_anchors(
                self.txn,
                text_branch,
                branch,
                index,
                utf16_offset,
                target == text_end,
                single_item,
            );
            match anchors {
                Some(anchors) => self.resolve(anchors)?,
                None if target == text_end && followed_by_text => break,
                None => return None,
            }
        }
        Some(text_end)
    }
}

fn text_boundary_anchors<T: ReadTxn>(
    txn: &T,
    text_branch: BranchPtr,
    branch: BranchPtr,
    index: u32,
    utf16_offset: u32,
    at_end: bool,
    single_item: Option<yrs::ID>,
) -> Option<BoundaryAnchors> {
    let before = single_item
        .filter(|_| utf16_offset > 0)
        .and_then(|id| {
            Some(StickyIndex::from_id(
                yrs::ID::new(id.client, id.clock.checked_add(utf16_offset - 1)?),
                Assoc::Before,
            ))
        })
        .or_else(|| sticky_at(txn, text_branch, utf16_offset, Assoc::Before))
        .or_else(|| sticky_at(txn, branch, index, Assoc::Before));
    let after = single_item
        .filter(|_| !at_end)
        .and_then(|id| {
            Some(StickyIndex::from_id(
                yrs::ID::new(id.client, id.clock.checked_add(utf16_offset)?),
                Assoc::After,
            ))
        })
        .or_else(|| sticky_at(txn, text_branch, utf16_offset, Assoc::After))
        .or_else(|| sticky_at(txn, branch, index.checked_add(1)?, Assoc::After));
    Some(BoundaryAnchors {
        before: before?,
        after: after?,
        ancestor: None,
        pinned_cell: None,
    })
}

fn integrated_xml_child_id(child: &XmlOut) -> Option<yrs::ID> {
    match child.id() {
        BranchID::Nested(id) => Some(id),
        BranchID::Root(_) => None,
    }
}

fn boundary_anchors_at<T: ReadTxn>(
    txn: &T,
    branch: BranchPtr,
    index: u32,
) -> Option<BoundaryAnchors> {
    Some(BoundaryAnchors {
        before: sticky_at(txn, branch, index, Assoc::Before)?,
        after: sticky_at(txn, branch, index, Assoc::After)?,
        ancestor: None,
        pinned_cell: None,
    })
}

fn sticky_at<T: ReadTxn>(
    txn: &T,
    branch: BranchPtr,
    index: u32,
    assoc: Assoc,
) -> Option<StickyIndex> {
    StickyIndex::at(txn, branch, index, assoc).or_else(|| {
        let opposite = match assoc {
            Assoc::Before => Assoc::After,
            Assoc::After => Assoc::Before,
        };
        StickyIndex::at(txn, branch, index, opposite)
    })
}

pub(crate) fn cursor_sticky_index_from_doc_pos<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_pos: u32,
    collapsed: bool,
    schema: &Schema,
) -> Option<StickyIndex> {
    if !collapsed {
        return doc_pos_to_sticky_index(txn, fragment, doc_pos, Assoc::Before, schema);
    }

    doc_pos_to_sticky_index(txn, fragment, doc_pos, Assoc::After, schema)
        .or_else(|| doc_pos_to_sticky_index(txn, fragment, doc_pos, Assoc::Before, schema))
}

pub(super) fn doc_pos_to_sticky_index_in_sequence<'a, T: ReadTxn>(
    txn: &T,
    children: impl Iterator<Item = XmlOut> + 'a,
    doc_pos: u32,
    assoc: Assoc,
    branch: BranchPtr,
    node_size: &impl Fn(&XmlOut) -> Option<u32>,
) -> Option<StickyIndex> {
    let mut branch_index = 0u32;
    let mut consumed_pm = 0u32;
    let mut children = children.peekable();

    while let Some(child) = children.next() {
        match &child {
            XmlOut::Text(text) => {
                let text_value = xml_text_plain_string(text, txn)?;
                let text_scalar_len = scalar_len(&text_value);
                let mut retry_adjacent_text = false;
                if doc_pos <= consumed_pm + text_scalar_len {
                    let utf16_offset = scalar_offset_to_utf16(&text_value, doc_pos - consumed_pm)?;
                    if let Some(sticky) = StickyIndex::at(
                        txn,
                        BranchPtr::from(<XmlTextRef as AsRef<Branch>>::as_ref(text)),
                        utf16_offset,
                        assoc,
                    ) {
                        return Some(sticky);
                    }
                    if doc_pos < consumed_pm + text_scalar_len {
                        return None;
                    }
                    retry_adjacent_text = true;
                }
                branch_index += 1;
                consumed_pm += text_scalar_len;
                if retry_adjacent_text && !matches!(children.peek(), Some(XmlOut::Text(_))) {
                    return None;
                }
            }
            XmlOut::Element(element) => {
                let child_size = node_size(&child)?;
                if doc_pos == consumed_pm {
                    return StickyIndex::at(txn, branch, branch_index, assoc);
                }
                if doc_pos < consumed_pm + child_size {
                    return doc_pos_to_sticky_index_in_sequence(
                        txn,
                        element.children(txn),
                        doc_pos - consumed_pm - 1,
                        assoc,
                        BranchPtr::from(<XmlElementRef as AsRef<Branch>>::as_ref(element)),
                        node_size,
                    );
                }
                branch_index += 1;
                consumed_pm += child_size;
            }
            XmlOut::Fragment(nested) => {
                let child_size = node_size(&child)?;
                if doc_pos == consumed_pm {
                    return StickyIndex::at(txn, branch, branch_index, assoc);
                }
                if doc_pos < consumed_pm + child_size {
                    return doc_pos_to_sticky_index_in_sequence(
                        txn,
                        nested.children(txn),
                        doc_pos - consumed_pm,
                        assoc,
                        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(nested)),
                        node_size,
                    );
                }
                branch_index += 1;
                consumed_pm += child_size;
            }
        }
    }

    if doc_pos == consumed_pm {
        StickyIndex::at(txn, branch, branch_index, assoc)
    } else {
        None
    }
}

fn xml_fragment_pm_content_size<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    schema: &Schema,
) -> Option<u32> {
    fragment.children(txn).try_fold(0u32, |size, child| {
        size.checked_add(xml_out_pm_size(txn, &child, schema)?)
    })
}

fn is_table_cell_element<T: ReadTxn>(element: &XmlElementRef, txn: &T, schema: &Schema) -> bool {
    super::codec::wire_element_node_spec(element, txn, schema).is_some_and(|spec| {
        matches!(
            spec.table_role,
            Some(crate::tables::TableRole::Cell | crate::tables::TableRole::HeaderCell)
        )
    })
}

pub(super) fn is_void_element<T: ReadTxn>(
    element: &XmlElementRef,
    txn: &T,
    schema: &Schema,
) -> bool {
    if matches!(
        element.tag().as_ref(),
        "__opaque" | "__opaque_json" | "__skip"
    ) {
        return true;
    }
    if let Some(spec) = super::codec::wire_element_node_spec(element, txn, schema) {
        return spec.is_void;
    }
    true
}

pub(super) fn xml_out_pm_size<T: ReadTxn>(txn: &T, node: &XmlOut, schema: &Schema) -> Option<u32> {
    #[cfg(test)]
    XML_SIZE_NODE_VISITS.set(XML_SIZE_NODE_VISITS.get().saturating_add(1));
    match node {
        XmlOut::Text(text) => Some(scalar_len(&xml_text_plain_string(text, txn)?)),
        XmlOut::Element(element) => {
            if is_void_element(element, txn, schema) {
                Some(VOID_NODE_SIZE)
            } else {
                element
                    .children(txn)
                    .try_fold(NODE_OPENING_TOKENS + NODE_CLOSING_TOKENS, |size, child| {
                        size.checked_add(xml_out_pm_size(txn, &child, schema)?)
                    })
            }
        }
        XmlOut::Fragment(fragment) => fragment.children(txn).try_fold(0u32, |size, child| {
            size.checked_add(xml_out_pm_size(txn, &child, schema)?)
        }),
    }
}

pub(super) fn xml_text_plain_string<T: ReadTxn>(text: &XmlTextRef, txn: &T) -> Option<String> {
    if let Some(value) = text.try_plain_string(txn) {
        return Some(value);
    }
    #[cfg(test)]
    PLAIN_TEXT_DIFF_FALLBACKS.set(PLAIN_TEXT_DIFF_FALLBACKS.get().saturating_add(1));
    let mut value = String::new();
    for diff in text.diff(txn, YChange::identity) {
        let yrs::Out::Any(Any::String(run)) = diff.insert else {
            return None;
        };
        value.push_str(&run);
    }
    Some(value)
}

fn scalar_len(value: &str) -> u32 {
    value.chars().count() as u32
}

const UTF8_WORD_BYTES: usize = std::mem::size_of::<u64>();
const UTF8_WORD_HIGH_BITS: u64 = 0x8080_8080_8080_8080;
const UTF8_FOUR_BYTE_LEAD: u8 = 0xF0;
const UTF8_CONTINUATION_MASK: u8 = 0xC0;
const UTF8_CONTINUATION_TAG: u8 = 0x80;

fn utf8_word_scalars_and_four_byte_leads(word: u64) -> (u32, u32) {
    let continuations = (word & !(word << 1) & UTF8_WORD_HIGH_BITS).count_ones();
    let four_byte_leads =
        (word & (word << 1) & (word << 2) & (word << 3) & UTF8_WORD_HIGH_BITS).count_ones();
    (UTF8_WORD_BYTES as u32 - continuations, four_byte_leads)
}

fn scalar_prefix(value: &str, scalar_offset: u32) -> Option<(usize, u32)> {
    let bytes = value.as_bytes();
    let mut scalars = 0u32;
    let mut surrogate_pairs = 0u32;
    let mut consumed = 0usize;
    for chunk in bytes.chunks_exact(UTF8_WORD_BYTES) {
        let word = u64::from_le_bytes(chunk.try_into().expect("exact UTF-8 word chunk"));
        let (word_scalars, word_pairs) = utf8_word_scalars_and_four_byte_leads(word);
        if scalars + word_scalars > scalar_offset {
            break;
        }
        scalars += word_scalars;
        surrogate_pairs += word_pairs;
        consumed += UTF8_WORD_BYTES;
    }
    let mut byte_offset = bytes.len();
    for (index, byte) in bytes[consumed..].iter().enumerate() {
        if byte & UTF8_CONTINUATION_MASK == UTF8_CONTINUATION_TAG {
            continue;
        }
        if scalars == scalar_offset {
            byte_offset = consumed + index;
            break;
        }
        scalars += 1;
        surrogate_pairs += u32::from(*byte >= UTF8_FOUR_BYTE_LEAD);
    }
    (scalars == scalar_offset).then_some((byte_offset, surrogate_pairs))
}

pub fn scalar_offset_to_utf16(value: &str, scalar_offset: u32) -> Option<u32> {
    scalar_prefix(value, scalar_offset).map(|(_, surrogate_pairs)| scalar_offset + surrogate_pairs)
}

pub(crate) fn scalar_offset_to_byte(value: &str, scalar_offset: u32) -> Option<usize> {
    scalar_prefix(value, scalar_offset).map(|(byte_offset, _)| byte_offset)
}

pub(crate) fn scalar_offset_to_byte_and_utf16(
    value: &str,
    scalar_offset: u32,
) -> Option<(usize, u32)> {
    scalar_prefix(value, scalar_offset)
        .map(|(byte_offset, surrogate_pairs)| (byte_offset, scalar_offset + surrogate_pairs))
}

pub fn utf16_offset_to_scalar(value: &str, utf16_offset: u32) -> Option<u32> {
    let mut scalar_count = 0u32;
    let mut utf16_count = 0u32;
    if utf16_offset == 0 {
        return Some(0);
    }
    for character in value.chars() {
        scalar_count += 1;
        utf16_count += character.len_utf16() as u32;
        if utf16_count == utf16_offset {
            return Some(scalar_count);
        }
        if utf16_count > utf16_offset {
            return None;
        }
    }
    None
}

impl From<Affinity> for Assoc {
    fn from(value: Affinity) -> Self {
        match value {
            Affinity::Before => Self::Before,
            Affinity::After => Self::After,
        }
    }
}

#[cfg(test)]
#[path = "position_tests.rs"]
mod tests;
