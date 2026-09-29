use super::position::{
    doc_pos_to_sticky_index_in_sequence, sequence_branch_index_to_doc_pos, utf16_offset_to_scalar,
    xml_out_pm_size, xml_text_plain_string,
};
use crate::model::Document;
use crate::position::PositionMap;
use crate::schema::Schema;
use crate::tables::commands::NODE_OPENING_TOKENS;
use smallvec::SmallVec;
use std::collections::{BTreeMap, HashMap, HashSet};
use yrs::branch::{Branch, BranchID, BranchPtr};
use yrs::types::xml::{XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut};
use yrs::{Assoc, Offset, ReadTxn, StickyIndex};

#[derive(Debug, Clone)]
pub(crate) struct BlockBranchIndex {
    blocks: Vec<BlockBranches>,
    by_branch: HashMap<BranchID, usize>,
    table_keys: BTreeMap<Vec<u32>, String>,
    atom_ids: BTreeMap<Vec<u32>, String>,
}

#[derive(Debug, Clone)]
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

impl BlockBranchIndex {
    pub(crate) fn build<T: ReadTxn>(
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
        position_map: &PositionMap,
    ) -> Option<Self> {
        #[cfg(test)]
        super::observability::record_yrs_tree_walk();
        let starts: HashMap<_, _> = (0..position_map.block_count())
            .map(|index| {
                let block = position_map.block(index).unwrap();
                (
                    position_map.effective_doc_start(index) - u32::from(!block.is_void_block),
                    index,
                )
            })
            .collect();
        let mut blocks = vec![None; position_map.block_count()];
        let mut pending = Vec::new();
        let mut position = 0u32;
        for (child_index, child) in fragment.children(txn).enumerate() {
            let size = xml_out_pm_size(txn, &child, schema)?;
            pending.push((position, vec![u32::try_from(child_index).ok()?], child));
            position = position.checked_add(size)?;
        }
        let mut table_keys = BTreeMap::new();
        let mut atom_ids = BTreeMap::new();
        let mut unique_table_keys = HashSet::new();
        while let Some((position, path, node)) = pending.pop() {
            match node {
                XmlOut::Element(element) => {
                    let spec = super::codec::wire_element_node_spec(&element, txn, schema);
                    if let BranchID::Nested(id) = AsRef::<Branch>::as_ref(&element).id() {
                        if spec.is_some_and(|spec| {
                            spec.table_role == Some(crate::tables::TableRole::Table)
                        }) {
                            let key = format!("y{}-{}", id.client, id.clock);
                            if !unique_table_keys.insert(key.clone()) {
                                return None;
                            }
                            table_keys.insert(path.clone(), key);
                        }
                        if spec.is_some_and(|spec| {
                            spec.is_void && matches!(spec.role, crate::schema::NodeRole::Block)
                        }) {
                            atom_ids.insert(path.clone(), format!("y{}-{}", id.client, id.clock));
                        }
                    }
                    if let Some(index) = starts.get(&position) {
                        blocks[*index] = Some(BlockBranches {
                            element: AsRef::<Branch>::as_ref(&element).id(),
                            texts: text_branches(txn, &element),
                        });
                        continue;
                    }
                    let mut child_pos = position.checked_add(NODE_OPENING_TOKENS)?;
                    for (child_index, child) in element.children(txn).enumerate() {
                        let size = xml_out_pm_size(txn, &child, schema)?;
                        let mut child_path = path.clone();
                        child_path.push(u32::try_from(child_index).ok()?);
                        pending.push((child_pos, child_path, child));
                        child_pos = child_pos.checked_add(size)?;
                    }
                }
                XmlOut::Fragment(fragment) => {
                    let mut child_pos = position;
                    for (child_index, child) in fragment.children(txn).enumerate() {
                        let size = xml_out_pm_size(txn, &child, schema)?;
                        let mut child_path = path.clone();
                        *child_path.last_mut()? += u32::try_from(child_index).ok()?;
                        pending.push((child_pos, child_path, child));
                        child_pos = child_pos.checked_add(size)?;
                    }
                }
                XmlOut::Text(_) => {}
            }
        }
        let blocks: Vec<_> = blocks.into_iter().collect::<Option<_>>()?;
        let mut by_branch = HashMap::new();
        for (index, block) in blocks.iter().enumerate() {
            for branch in std::iter::once(&block.element).chain(&block.texts) {
                if by_branch.insert(branch.clone(), index).is_some() {
                    return None;
                }
            }
        }
        Some(Self {
            blocks,
            by_branch,
            table_keys,
            atom_ids,
        })
    }

    pub(crate) fn with_block_replaced<T: ReadTxn>(
        &self,
        txn: &T,
        block_index: usize,
        schema: &Schema,
    ) -> Option<Self> {
        let block = self.blocks.get(block_index)?;
        let element = XmlElementRef::from(block.element.get_branch(txn)?);
        if !super::codec::wire_element_node_spec(&element, txn, schema).is_some_and(|spec| {
            !spec.is_void && matches!(spec.role, crate::schema::NodeRole::TextBlock)
        }) {
            return None;
        }
        let mut next = self.clone();
        for branch in &block.texts {
            next.by_branch.remove(branch);
        }
        let texts = text_branches(txn, &element);
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
        Some(next)
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
