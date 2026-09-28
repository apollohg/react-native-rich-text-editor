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

use super::{Affinity, EditorOffsetKind, RevisionedPosition};

const VOID_NODE_SIZE: u32 = 1;

#[cfg(test)]
std::thread_local! {
    static RELATIVE_FULL_SIZE_PREPASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RELATIVE_FORWARD_TRAVERSALS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RELATIVE_REVERSE_TRAVERSALS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOUNDARY_WALK_NODE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_relative_position_traversal_counts_for_test() {
    RELATIVE_FULL_SIZE_PREPASSES.set(0);
    RELATIVE_FORWARD_TRAVERSALS.set(0);
    RELATIVE_REVERSE_TRAVERSALS.set(0);
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
) -> Option<RelativePoint> {
    let assoc = affinity.into();
    let sticky = forward_doc_pos_to_sticky_index(txn, fragment, doc_pos, assoc, schema)?;
    sticky.get_offset(txn)?;
    (sticky.assoc == assoc).then_some(RelativePoint { sticky, affinity })
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

pub fn revisioned_position_to_relative_point<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    position: RevisionedPosition,
    rendered_text: &str,
    position_map: &PositionMap,
    document: &Document,
    schema: &Schema,
) -> Option<RelativePoint> {
    let doc_pos = editor_offset_to_doc_pos(
        position.offset,
        position.kind,
        rendered_text,
        position_map,
        document,
    )?;
    doc_pos_to_relative_point(txn, fragment, doc_pos, position.affinity, schema)
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
    let mut targets = Vec::new();
    let count = doc_positions.iter().try_fold(0usize, |total, positions| {
        total.checked_add(positions.len())
    })?;
    targets.try_reserve_exact(count).ok()?;
    for (block, positions) in doc_positions.iter().enumerate() {
        targets.extend(
            positions
                .iter()
                .enumerate()
                .map(|(offset, position)| (*position, block, offset)),
        );
    }
    targets.sort_unstable();
    let mut walk = BoundaryAnchorWalk {
        txn,
        schema,
        targets: &targets,
        resolved: 0,
        chunks: doc_positions
            .iter()
            .map(|positions| vec![None; positions.len()])
            .collect(),
        open: Vec::new(),
    };
    walk.walk_sequence(
        fragment.children(txn),
        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(fragment)),
        0,
        None,
    )?;
    if walk.resolved != targets.len() {
        return None;
    }
    walk.chunks
        .into_iter()
        .map(|anchors| {
            let anchors = anchors.into_iter().collect::<Option<Vec<_>>>()?;
            EpochBlockChunk::new(anchors).map(Arc::new)
        })
        .collect()
}

struct BoundaryAnchorWalk<'walk, T> {
    txn: &'walk T,
    schema: &'walk Schema,
    targets: &'walk [(u32, usize, usize)],
    resolved: usize,
    chunks: Vec<Vec<Option<BoundaryAnchors>>>,
    open: Vec<Arc<AncestorNode>>,
}

impl<T: ReadTxn> BoundaryAnchorWalk<'_, T> {
    fn pending(&self, sequence_end: Option<u32>) -> Option<u32> {
        self.targets
            .get(self.resolved)
            .map(|target| target.0)
            .filter(|target| sequence_end.is_none_or(|end| *target < end))
    }

    fn resolve(&mut self, mut leaf: BoundaryAnchors) {
        leaf.ancestor = self.open.last().cloned();
        let position = self.targets[self.resolved].0;
        while self
            .targets
            .get(self.resolved + 1)
            .is_some_and(|target| target.0 == position)
        {
            let (_, block, offset) = self.targets[self.resolved];
            self.chunks[block][offset] = Some(leaf.clone());
            self.resolved += 1;
        }
        let (_, block, offset) = self.targets[self.resolved];
        self.chunks[block][offset] = Some(leaf);
        self.resolved += 1;
    }

    fn resolve_at_child(
        &mut self,
        branch: BranchPtr,
        index: u32,
        position: u32,
        sequence_end: Option<u32>,
    ) -> Option<()> {
        if self.pending(sequence_end) == Some(position) {
            self.resolve(boundary_anchors_at(self.txn, branch, index)?);
        }
        Some(())
    }

    fn enter(&mut self, branch: BranchPtr, index: u32, table_cell: bool) -> Option<()> {
        let entered = Arc::new(AncestorNode {
            anchors: AncestorAnchors {
                before: sticky_at(self.txn, branch, index, Assoc::Before)?,
                after: sticky_at(self.txn, branch, index.checked_add(1)?, Assoc::After)?,
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
        start: u32,
        sequence_end: Option<u32>,
    ) -> Option<u32> {
        let mut index = 0u32;
        let mut position = start;
        let mut children = children.peekable();
        while let Some(child) = children.next() {
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
                        )?;
                        let content_end = self.walk_sequence(
                            element.children(self.txn),
                            BranchPtr::from(<XmlElementRef as AsRef<Branch>>::as_ref(element)),
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
                    self.enter(branch, index, false)?;
                    self.walk_sequence(
                        nested.children(self.txn),
                        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(nested)),
                        position,
                        Some(fragment_end),
                    )?;
                    self.leave();
                    fragment_end
                }
            };
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
        let value = xml_text_plain_string(text, self.txn)?;
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
                utf16_offset = utf16_offset.checked_add(width)?;
                scalar_offset += 1;
            }
            let before = sticky_at(self.txn, text_branch, utf16_offset, Assoc::Before)
                .or_else(|| sticky_at(self.txn, branch, index, Assoc::Before));
            let after = sticky_at(self.txn, text_branch, utf16_offset, Assoc::After)
                .or_else(|| sticky_at(self.txn, branch, index.checked_add(1)?, Assoc::After));
            match before.zip(after) {
                Some((before, after)) => self.resolve(BoundaryAnchors {
                    before,
                    after,
                    ancestor: None,
                    pinned_cell: None,
                }),
                None if target == text_end && followed_by_text => break,
                None => return None,
            }
        }
        Some(text_end)
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

fn is_void_element<T: ReadTxn>(element: &XmlElementRef, txn: &T, schema: &Schema) -> bool {
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

pub fn scalar_offset_to_utf16(value: &str, scalar_offset: u32) -> Option<u32> {
    let mut scalar_count = 0u32;
    let mut utf16_count = 0u32;
    if scalar_offset == 0 {
        return Some(0);
    }
    for character in value.chars() {
        scalar_count += 1;
        utf16_count += character.len_utf16() as u32;
        if scalar_count == scalar_offset {
            return Some(utf16_count);
        }
    }
    None
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
