use super::position::{
    doc_pos_to_sticky_index_in_sequence, is_void_element, sequence_branch_index_to_doc_pos,
    utf16_offset_to_scalar, xml_out_pm_size, xml_text_plain_string, VOID_NODE_SIZE,
};
use crate::model::Document;
use crate::position::PositionMap;
use crate::schema::Schema;
use crate::tables::commands::{NODE_CLOSING_TOKENS, NODE_OPENING_TOKENS};
use smallvec::SmallVec;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use yrs::branch::{Branch, BranchID, BranchPtr};
use yrs::types::xml::{XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut};
use yrs::{Assoc, Offset, ReadTxn, StickyIndex};

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct BlockBranchIndex {
    blocks: Vec<BlockBranches>,
    by_branch: HashMap<BranchID, usize>,
    table_keys: BTreeMap<Vec<u32>, String>,
    atom_ids: BTreeMap<Vec<u32>, String>,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct BlockBranches {
    pub(crate) element: BranchID,
    pub(crate) texts: SmallVec<[BranchID; 2]>,
}

fn text_branches<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> SmallVec<[BranchID; 2]> {
    element
        .children(txn)
        .filter_map(|child| match child {
            XmlOut::Text(text) => Some(AsRef::<Branch>::as_ref(&text).id()),
            _ => None,
        })
        .collect()
}

fn inline_size<T: ReadTxn>(txn: &T, node: &XmlOut) -> Option<u32> {
    match node {
        XmlOut::Text(text) => u32::try_from(xml_text_plain_string(text, txn)?.chars().count()).ok(),
        XmlOut::Element(_) => Some(NODE_OPENING_TOKENS),
        XmlOut::Fragment(_) => None,
    }
}

fn supports_inline_index(map: &PositionMap, document: &Document, index: usize) -> bool {
    let Some(block) = map.block(index).filter(|block| !block.is_void_block) else {
        return false;
    };
    document
        .node_at(&block.node_path)
        .and_then(|node| node.content())
        .is_some_and(|content| {
            content
                .iter()
                .all(|node| node.is_text() || node.node_size() == NODE_OPENING_TOKENS)
        })
}

struct BlockBranchIndexBuilder {
    starts: HashMap<u32, usize>,
    blocks: Vec<Option<BlockBranches>>,
    table_keys: BTreeMap<Vec<u32>, String>,
    atom_ids: BTreeMap<Vec<u32>, String>,
    unique_table_keys: HashSet<String>,
}

impl BlockBranchIndexBuilder {
    fn new(position_map: &PositionMap) -> Self {
        let starts: HashMap<_, _> = (0..position_map.block_count())
            .map(|index| {
                let block = position_map.block(index).unwrap();
                (
                    position_map.effective_doc_start(index) - u32::from(!block.is_void_block),
                    index,
                )
            })
            .collect();
        Self {
            starts,
            blocks: vec![None; position_map.block_count()],
            table_keys: BTreeMap::new(),
            atom_ids: BTreeMap::new(),
            unique_table_keys: HashSet::new(),
        }
    }

    fn observe_identity(
        &mut self,
        path: &[u32],
        branch: BranchID,
        spec: Option<&crate::schema::NodeSpec>,
    ) -> Option<()> {
        if let BranchID::Nested(id) = branch {
            if spec.is_some_and(|spec| spec.table_role == Some(crate::tables::TableRole::Table)) {
                let key = format!("y{}-{}", id.client, id.clock);
                if !self.unique_table_keys.insert(key.clone()) {
                    return None;
                }
                self.table_keys.entry(path.to_vec()).or_insert(key);
            }
            if spec.is_some_and(|spec| {
                spec.is_void && matches!(spec.role, crate::schema::NodeRole::Block)
            }) {
                self.atom_ids
                    .entry(path.to_vec())
                    .or_insert_with(|| format!("y{}-{}", id.client, id.clock));
            }
        }
        Some(())
    }

    fn finish(self) -> Option<BlockBranchIndex> {
        let blocks: Vec<_> = self.blocks.into_iter().collect::<Option<_>>()?;
        let mut by_branch = HashMap::new();
        for (index, block) in blocks.iter().enumerate() {
            for branch in std::iter::once(&block.element).chain(&block.texts) {
                if by_branch.insert(branch.clone(), index).is_some() {
                    return None;
                }
            }
        }
        Some(BlockBranchIndex {
            blocks,
            by_branch,
            table_keys: self.table_keys,
            atom_ids: self.atom_ids,
        })
    }
}

pub(crate) struct BlockBranchIndexCapture {
    builder: Option<BlockBranchIndexBuilder>,
    position: u32,
    path: Vec<u32>,
    block: Option<(usize, usize)>,
}

impl BlockBranchIndexCapture {
    pub(crate) fn new(map: &PositionMap) -> Self {
        Self {
            builder: Some(BlockBranchIndexBuilder::new(map)),
            position: 0,
            path: Vec::new(),
            block: None,
        }
    }

    pub(crate) fn enter<T: ReadTxn>(
        &mut self,
        ordinal: usize,
        element: &XmlElementRef,
        txn: &T,
        schema: &Schema,
        is_void: bool,
    ) -> bool {
        // The standalone walker inspects void descendants and skips mapped subtrees.
        if is_void || self.block.is_some() {
            return false;
        }
        let Some(builder) = self.builder.as_mut() else {
            return true;
        };
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            self.builder = None;
            return true;
        };
        self.path.push(ordinal);
        let branch = AsRef::<Branch>::as_ref(element).id();
        let spec = super::codec::wire_element_node_spec(element, txn, schema);
        if builder
            .observe_identity(&self.path, branch.clone(), spec)
            .is_none()
        {
            self.builder = None;
            return true;
        }
        if let Some(&index) = builder.starts.get(&self.position) {
            builder.blocks[index].get_or_insert_with(|| BlockBranches {
                element: branch,
                texts: SmallVec::new(),
            });
            self.block = Some((index, self.path.len()));
        }
        self.advance(NODE_OPENING_TOKENS);
        true
    }

    fn advance(&mut self, width: u32) {
        if let Some(position) = self.position.checked_add(width) {
            self.position = position;
        } else {
            self.builder = None;
        }
    }

    pub(crate) fn text(&mut self, branch: BranchID, scalar_len: u32) {
        if let Some(builder) = self.builder.as_mut() {
            if let Some((index, _)) = self.block {
                builder.blocks[index].as_mut().unwrap().texts.push(branch);
            }
            self.advance(scalar_len);
        }
    }

    pub(crate) fn exit(&mut self) {
        if self.builder.is_none() {
            return;
        }
        if self
            .block
            .is_some_and(|(_, depth)| depth == self.path.len())
        {
            self.block = None;
        }
        self.path.pop();
        self.advance(NODE_CLOSING_TOKENS);
    }

    pub(crate) fn finish(self) -> Option<BlockBranchIndex> {
        self.builder?.finish()
    }
}

impl BlockBranchIndex {
    #[cfg(test)]
    pub(crate) fn assert_same_allocations_for_test(&self, other: &Self) {
        assert_eq!(self, other);
        assert_eq!(self.blocks.capacity(), other.blocks.capacity());
        assert_eq!(self.by_branch.capacity(), other.by_branch.capacity());
        for (left, right) in self.blocks.iter().zip(&other.blocks) {
            assert_eq!(left.texts.capacity(), right.texts.capacity());
        }
        for (left, right) in self
            .table_keys
            .iter()
            .zip(&other.table_keys)
            .chain(self.atom_ids.iter().zip(&other.atom_ids))
        {
            assert_eq!(left.0.capacity(), right.0.capacity());
            assert_eq!(left.1.capacity(), right.1.capacity());
        }
    }

    pub(crate) fn build<T: ReadTxn>(
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
        position_map: &PositionMap,
    ) -> Option<Self> {
        #[cfg(test)]
        super::observability::record_yrs_tree_walk();
        let mut builder = BlockBranchIndexBuilder::new(position_map);
        enum Frame {
            Visit { path: Vec<u32>, node: XmlOut },
            FinishElement { void_end: Option<u32> },
        }
        fn push_children(
            pending: &mut Vec<Frame>,
            children: impl Iterator<Item = XmlOut>,
            path: &[u32],
            flatten: bool,
        ) -> Option<()> {
            let first = pending.len();
            for (index, node) in children.enumerate() {
                let index = u32::try_from(index).ok()?;
                let mut path = path.to_vec();
                if flatten {
                    let last = path.last_mut()?;
                    *last = last.checked_add(index)?;
                } else {
                    path.push(index);
                }
                pending.push(Frame::Visit { path, node });
            }
            pending[first..].reverse();
            Some(())
        }
        let mut pending = Vec::new();
        push_children(&mut pending, fragment.children(txn), &[], false)?;
        let mut position = 0u32;
        while let Some(frame) = pending.pop() {
            let (path, node) = match frame {
                Frame::FinishElement { void_end } => {
                    position = match void_end {
                        Some(end) => end,
                        None => position.checked_add(NODE_CLOSING_TOKENS)?,
                    };
                    continue;
                }
                Frame::Visit { path, node } => (path, node),
            };
            match &node {
                XmlOut::Element(element) => {
                    let spec = super::codec::wire_element_node_spec(element, txn, schema);
                    builder.observe_identity(&path, AsRef::<Branch>::as_ref(element).id(), spec)?;
                    if let Some(index) = builder.starts.get(&position) {
                        builder.blocks[*index].get_or_insert_with(|| BlockBranches {
                            element: AsRef::<Branch>::as_ref(element).id(),
                            texts: text_branches(txn, element),
                        });
                        position = position.checked_add(xml_out_pm_size(txn, &node, schema)?)?;
                        continue;
                    }
                    // Void children are still inspected, but do not contribute to their parent's size.
                    let void_end = if is_void_element(element, txn, schema) {
                        Some(position.checked_add(VOID_NODE_SIZE)?)
                    } else {
                        None
                    };
                    position = position.checked_add(NODE_OPENING_TOKENS)?;
                    pending.push(Frame::FinishElement { void_end });
                    push_children(&mut pending, element.children(txn), &path, false)?;
                }
                XmlOut::Fragment(fragment) => {
                    push_children(&mut pending, fragment.children(txn), &path, true)?;
                }
                XmlOut::Text(_) => {
                    position = position.checked_add(xml_out_pm_size(txn, &node, schema)?)?;
                }
            }
        }
        builder.finish()
    }

    pub(crate) fn with_block_replaced<T: ReadTxn>(
        self: &Arc<Self>,
        txn: &T,
        block_index: usize,
        schema: &Schema,
    ) -> Option<Arc<Self>> {
        let block = self.blocks.get(block_index)?;
        let element = XmlElementRef::from(block.element.get_branch(txn)?);
        if !super::codec::wire_element_node_spec(&element, txn, schema).is_some_and(|spec| {
            !spec.is_void && matches!(spec.role, crate::schema::NodeRole::TextBlock)
        }) {
            return None;
        }
        let texts = text_branches(txn, &element);
        if texts == block.texts {
            return Some(Arc::clone(self));
        }
        let mut next = self.as_ref().clone();
        for branch in &block.texts {
            next.by_branch.remove(branch);
        }
        for branch in &texts {
            if next
                .by_branch
                .insert(branch.clone(), block_index)
                .is_some_and(|index| index != block_index)
            {
                return None;
            }
        }
        next.blocks[block_index].texts = texts;
        Some(Arc::new(next))
    }

    pub(crate) fn table_key(&self, path: &[u32]) -> Option<&str> {
        self.table_keys.get(path).map(String::as_str)
    }

    pub(crate) fn atom_id(&self, path: &[u32]) -> Option<&str> {
        self.atom_ids.get(path).map(String::as_str)
    }

    pub(crate) fn block_branches(&self, block_index: usize) -> Option<&BlockBranches> {
        self.blocks.get(block_index)
    }

    pub(crate) fn doc_pos_of_offset<T: ReadTxn>(
        &self,
        txn: &T,
        offset: &Offset,
        position_map: &PositionMap,
        document: &Document,
    ) -> Option<u32> {
        let block_index = *self.by_branch.get(&offset.branch.id())?;
        if !supports_inline_index(position_map, document, block_index) {
            return None;
        }
        let block = self.blocks.get(block_index)?;
        let element = XmlElementRef::from(block.element.get_branch(txn)?);
        let start = position_map.effective_doc_start(block_index);
        if offset.branch.id() == block.element {
            let inner = sequence_branch_index_to_doc_pos(
                txn,
                element.children(txn),
                offset.index,
                &|child| inline_size(txn, child),
            )?;
            return start.checked_add(inner);
        }
        let mut position = start;
        for child in element.children(txn) {
            if let XmlOut::Text(text) = &child {
                if AsRef::<Branch>::as_ref(text).id() == offset.branch.id() {
                    return position.checked_add(utf16_offset_to_scalar(
                        &xml_text_plain_string(text, txn)?,
                        offset.index,
                    )?);
                }
            }
            position = position.checked_add(inline_size(txn, &child)?)?;
        }
        None
    }

    pub(crate) fn sticky_at_doc_pos<T: ReadTxn>(
        &self,
        txn: &T,
        doc_pos: u32,
        assoc: Assoc,
        position_map: &PositionMap,
        document: &Document,
    ) -> Option<StickyIndex> {
        let (mut low, mut high) = (0, position_map.block_count());
        while low < high {
            let middle = low + (high - low) / 2;
            if position_map.effective_doc_start(middle) <= doc_pos {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let index = low.checked_sub(1)?;
        if !supports_inline_index(position_map, document, index)
            || doc_pos > position_map.effective_doc_end(index)
        {
            return None;
        }
        let block = self.blocks.get(index)?;
        let element = XmlElementRef::from(block.element.get_branch(txn)?);
        doc_pos_to_sticky_index_in_sequence(
            txn,
            element.children(txn),
            doc_pos - position_map.effective_doc_start(index),
            assoc,
            BranchPtr::from(AsRef::<Branch>::as_ref(&element)),
            &|child| inline_size(txn, child),
        )
    }
}
