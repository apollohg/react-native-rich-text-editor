use crate::block::{Block, Item, ItemContent, ItemPtr, Prelim};
use crate::branch::BranchPtr;
use crate::transaction::{ReadTxn, TransactionMut};
use crate::types::xml::XmlOut;
use crate::types::TypePtr;
use crate::{Out, ID};
use std::convert::TryFrom;

/// Struct used for iterating over the sequence of item's values with respect to a potential
/// [Move] markers that may change their order.
#[derive(Debug, Clone)]
pub(crate) struct BlockIter {
    branch: BranchPtr,
    index: u32,
    rel: u32,
    next_item: Option<ItemPtr>,
    reached_end: bool,
}

impl BlockIter {
    pub fn new(branch: BranchPtr) -> Self {
        let next_item = branch.start;
        let reached_end = branch.start.is_none();
        BlockIter {
            branch,
            next_item,
            reached_end,
            index: 0,
            rel: 0,
        }
    }

    #[inline]
    pub fn rel(&self) -> u32 {
        self.rel
    }

    #[inline]
    pub fn finished(&self) -> bool {
        self.reached_end || self.index == self.branch.content_len
    }

    #[inline]
    pub fn next_item(&self) -> Option<ItemPtr> {
        self.next_item
    }

    pub fn left(&self) -> Option<ItemPtr> {
        if self.reached_end {
            self.next_item
        } else if let Some(item) = self.next_item.as_deref() {
            item.left
        } else {
            None
        }
    }

    pub fn right(&self) -> Option<ItemPtr> {
        if self.reached_end {
            None
        } else {
            self.next_item
        }
    }

    fn can_forward(&self, ptr: Option<ItemPtr>, len: u32) -> bool {
        if !self.reached_end {
            if len > 0 {
                return true;
            } else if let Some(item) = ptr.as_deref() {
                return !item.is_countable() || item.is_deleted() || self.reached_end;
            }
        }

        false
    }

    pub fn forward<T: ReadTxn>(&mut self, txn: &T, len: u32) {
        if !self.try_forward(txn, len) {
            panic!("Length exceeded")
        }
    }

    pub fn try_forward<T: ReadTxn>(&mut self, txn: &T, mut len: u32) -> bool {
        if len == 0 && self.next_item.is_none() {
            return true;
        }

        if self.index + len > self.branch.content_len() || self.next_item.is_none() {
            return false;
        }

        let mut item = self.next_item;
        self.index += len;
        if self.rel != 0 {
            len += self.rel;
            self.rel = 0;
        }

        let encoding = txn.store().offset_kind;
        while self.can_forward(item, len) {
            if item.is_none() {
                return false;
            } else if let Some(i) = item.as_deref() {
                if i.is_countable() && !i.is_deleted() && len > 0 {
                    let item_len = i.content_len(encoding);
                    if item_len > len {
                        self.rel = len;
                        len = 0;
                        break;
                    } else {
                        len -= item_len;
                    }
                }
            }

            if self.reached_end {
                return false;
            }

            match item.as_deref() {
                Some(i) if i.right.is_some() => item = i.right,
                _ => self.reached_end = true, //TODO: we need to ensure to iterate further if this.currMoveEnd === null
            }
        }

        self.index -= len;
        self.next_item = item;
        true
    }

    pub fn backward<T: ReadTxn>(&mut self, txn: &mut T, mut len: u32) {
        if self.index < len {
            panic!("Length exceeded");
        }
        self.index -= len;
        let encoding = txn.store().offset_kind;
        if self.reached_end {
            if let Some(next_item) = self.next_item.as_deref() {
                self.rel = if next_item.is_countable() && !next_item.is_deleted() {
                    next_item.content_len(encoding)
                } else {
                    0
                };
            }
        }
        if self.rel >= len {
            self.rel -= len;
            return;
        }
        let mut item = self.next_item;
        if let Some(i) = item.as_deref() {
            len += if i.is_countable() && !i.is_deleted() {
                i.content_len(encoding)
            } else {
                0
            };
            len -= self.rel;
        }
        self.rel = 0;
        while let Some(i) = item.as_deref() {
            if len == 0 {
                break;
            }

            if i.is_countable() && !i.is_deleted() {
                let item_len = i.content_len(encoding);
                if len < item_len {
                    self.rel = item_len - len;
                    len = 0;
                } else {
                    len -= item_len;
                }
                if len == 0 {
                    break;
                }
            }

            item = if let Some(i) = item.as_deref() {
                i.left
            } else {
                None
            };
        }
        self.next_item = item;
    }

    pub fn delete(&mut self, txn: &mut TransactionMut, mut len: u32) {
        let mut item = self.next_item;
        if self.index + len > self.branch.content_len() {
            panic!("Length exceeded");
        }

        let encoding = txn.store().offset_kind;
        let mut i: &Item;
        while len > 0 {
            while let Some(block) = item.as_deref() {
                i = block;
                if !i.is_deleted() && i.is_countable() && !self.reached_end && len > 0 {
                    if self.rel > 0 {
                        let mut id = i.id.clone();
                        id.clock += self.rel;
                        let store = txn.store_mut();
                        item = store
                            .blocks
                            .get_item_clean_start(&id)
                            .map(|s| store.materialize(s));
                        i = item.as_deref().unwrap();
                        self.rel = 0;
                    }
                    if len < i.content_len(encoding) {
                        let mut id = i.id.clone();
                        id.clock += len;
                        let store = txn.store_mut();
                        store
                            .blocks
                            .get_item_clean_start(&id)
                            .map(|s| store.materialize(s));
                    }
                    len -= i.content_len(encoding);
                    txn.delete(item.unwrap());
                    if i.right.is_some() {
                        item = i.right;
                    } else {
                        self.reached_end = true;
                    }
                } else {
                    break;
                }
            }
            if len > 0 {
                self.next_item = item;
                if self.try_forward(txn, 0) {
                    item = self.next_item;
                } else {
                    panic!("Block iter couldn't move forward");
                }
            }
        }
        self.next_item = item;
    }

    pub(crate) fn slice<T: ReadTxn>(&mut self, txn: &T, buf: &mut [Out]) -> u32 {
        let mut len = buf.len() as u32;
        if self.index + len > self.branch.content_len() {
            return 0;
        }
        self.index += len;
        let mut next_item = self.next_item;
        let encoding = txn.store().offset_kind;
        let mut read = 0u32;
        while len > 0 {
            if !self.reached_end {
                while let Some(item) = next_item {
                    if item.is_countable() && !self.reached_end && len > 0 {
                        if !item.is_deleted() {
                            // we're iterating inside of a block
                            let r = item
                                .content
                                .read(self.rel as usize, &mut buf[read as usize..])
                                as u32;
                            read += r;
                            len -= r;
                            if self.rel + r == item.content_len(encoding) {
                                self.rel = 0;
                            } else {
                                self.rel += r;
                                continue; // do not iterate to item.right
                            }
                        }

                        if item.right.is_some() {
                            next_item = item.right;
                        } else {
                            self.reached_end = true;
                        }
                    } else {
                        break;
                    }
                }
                if !self.reached_end && len > 0 {
                    // always set nextItem before any method call
                    self.next_item = next_item;
                    if !self.try_forward(txn, 0) || self.next_item.is_none() {
                        return read;
                    }
                    next_item = self.next_item;
                }
            } else {
                // reached end and move stack is empty
                next_item = None;
                break;
            }
        }
        self.next_item = next_item;
        self.index -= len;
        read
    }

    fn split_rel(&mut self, txn: &mut TransactionMut) {
        if self.rel > 0 {
            if let Some(ptr) = self.next_item {
                let mut item_id = ptr.id().clone();
                item_id.clock += self.rel;
                let store = txn.store_mut();
                self.next_item = store
                    .blocks
                    .get_item_clean_start(&item_id)
                    .map(|s| store.materialize(s));
                self.rel = 0;
            }
        }
    }

    pub(crate) fn read_xml<T: ReadTxn>(&mut self, txn: &T) -> Option<XmlOut> {
        if !self.reached_end && self.rel == 0 && self.index < self.branch.content_len() {
            if let Some(item) = self.next_item {
                if item.is_countable() && !item.is_deleted() {
                    if let ItemContent::Type(branch) = &item.content {
                        if let Ok(value) = XmlOut::try_from(BranchPtr::from(branch)) {
                            self.index += 1;
                            if item.right.is_some() {
                                self.next_item = item.right;
                            } else {
                                self.reached_end = true;
                            }
                            return Some(value);
                        }
                    }
                }
            }
        }
        self.read_value(txn)
            .and_then(|value| XmlOut::try_from(value).ok())
    }

    pub(crate) fn read_value<T: ReadTxn>(&mut self, txn: &T) -> Option<Out> {
        let mut buf = [Out::default()];
        if self.slice(txn, &mut buf) != 0 {
            Some(std::mem::replace(&mut buf[0], Out::default()))
        } else {
            None
        }
    }

    pub fn insert_contents<V: Prelim>(
        &mut self,
        txn: &mut TransactionMut,
        value: V,
    ) -> Option<ItemPtr> {
        self.split_rel(txn);
        let id = {
            let store = txn.store();
            let client_id = store.client_id;
            let clock = store.blocks.get_clock(&client_id);
            ID::new(client_id, clock)
        };
        let parent = TypePtr::Branch(self.branch);
        let right = self.right();
        let left = self.left();
        let (mut content, remainder) = value.into_content(txn);
        let inner_ref = if let ItemContent::Type(inner_ref) = &mut content {
            Some(BranchPtr::from(inner_ref))
        } else {
            None
        };
        let block = Item::new(
            id,
            left,
            left.map(|ptr| ptr.last_id()),
            right,
            right.map(|r| *r.id()),
            parent,
            None,
            content,
        )?;
        let block_ptr = txn.integrate_item(block, 0);

        if let Some(remainder) = remainder {
            remainder.integrate(txn, inner_ref.unwrap().into())
        }

        if let Some(item) = right.as_deref() {
            self.next_item = item.right;
        } else {
            self.next_item = left;
            self.reached_end = true;
        }

        block_ptr
    }

    pub fn values<'a, 'txn, T: ReadTxn>(
        &'a mut self,
        txn: &'txn mut TransactionMut<'txn>,
    ) -> Values<'a, 'txn> {
        Values::new(self, txn)
    }
}

pub struct Values<'a, 'txn> {
    iter: &'a mut BlockIter,
    txn: &'txn mut TransactionMut<'txn>,
}

impl<'a, 'txn> Values<'a, 'txn> {
    fn new(iter: &'a mut BlockIter, txn: &'txn mut TransactionMut<'txn>) -> Self {
        Values { iter, txn }
    }
}

impl<'a, 'txn> Iterator for Values<'a, 'txn> {
    type Item = Out;

    fn next(&mut self) -> Option<Self::Item> {
        if self.iter.reached_end || self.iter.index == self.iter.branch.content_len() {
            None
        } else {
            let mut buf = [Out::default()];
            if self.iter.slice(self.txn, &mut buf) != 0 {
                Some(std::mem::replace(&mut buf[0], Out::default()))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::exchange_updates;
    use crate::{
        Array, ArrayPrelim, ArrayRef, Doc, MapPrelim, Transact, XmlElementPrelim,
        XmlFragmentPrelim, XmlTextPrelim,
    };

    fn assert_xml_cursor_equivalence(doc: &Doc, label: &str) {
        let fragment = doc.get_or_insert_xml_fragment("xml");
        let branch = BranchPtr::from(AsRef::<crate::branch::Branch>::as_ref(&fragment));
        let txn = doc.transact();
        let len = branch.content_len();
        const END_PROBES: u32 = 3;
        for offset in 0..=len {
            let mut oracle = BlockIter::new(branch);
            if offset != 0 {
                oracle.forward(&txn, offset);
            }
            let mut candidate = oracle.clone();
            for call in 0..len + END_PROBES {
                let expected = oracle
                    .read_value(&txn)
                    .and_then(|v| XmlOut::try_from(v).ok());
                let actual = candidate.read_xml(&txn);
                let identity = |node: &XmlOut| (std::mem::discriminant(node), node.id());
                let context = format!("{label}, offset={offset}, call={call}");
                assert_eq!(
                    actual.as_ref().map(identity),
                    expected.as_ref().map(identity),
                    "{context}"
                );
                assert_eq!(candidate.branch, oracle.branch, "{context}: branch");
                assert_eq!(candidate.index, oracle.index, "{context}: index");
                assert_eq!(candidate.rel, oracle.rel, "{context}: relative offset");
                assert_eq!(
                    candidate.next_item, oracle.next_item,
                    "{context}: next item"
                );
                assert_eq!(candidate.reached_end, oracle.reached_end, "{context}: end");
            }
        }
    }

    #[test]
    fn xml_reads_preserve_values_and_cursor_across_mixed_and_merged_items() {
        let doc = Doc::with_client_id(1);
        let fragment = doc.get_or_insert_xml_fragment("xml");
        let array = ArrayRef::from(BranchPtr::from(AsRef::<crate::branch::Branch>::as_ref(
            &fragment,
        )));
        assert_xml_cursor_equivalence(&doc, "empty");
        array.push_back(&mut doc.transact_mut(), XmlElementPrelim::empty("head"));
        assert_xml_cursor_equivalence(&doc, "single element");
        {
            let mut txn = doc.transact_mut();
            array.push_back(&mut txn, XmlTextPrelim::new("text😀"));
            array.push_back(
                &mut txn,
                XmlFragmentPrelim::new::<_, ()>([XmlElementPrelim::empty("nested").into()]),
            );
            array.push_back(&mut txn, XmlElementPrelim::empty("tail"));
        }
        assert_xml_cursor_equivalence(&doc, "all XML kinds");
        {
            let mut txn = doc.transact_mut();
            array.insert_range(&mut txn, 1, [10, 20, 30]);
            array.insert(&mut txn, 2, MapPrelim::default());
            array.insert(&mut txn, 3, ArrayPrelim::default());
        }
        assert_xml_cursor_equivalence(&doc, "primitives and non-XML shared types");
        {
            let mut txn = doc.transact_mut();
            array.remove(&mut txn, 0);
            array.remove(&mut txn, 2);
            let last = array.len(&txn) - 1;
            array.remove(&mut txn, last);
        }
        assert_xml_cursor_equivalence(&doc, "deleted head middle tail");
        let peer = Doc::with_client_id(2);
        let peer_fragment = peer.get_or_insert_xml_fragment("xml");
        let peer_array = ArrayRef::from(BranchPtr::from(AsRef::<crate::branch::Branch>::as_ref(
            &peer_fragment,
        )));
        exchange_updates(&[&doc, &peer]);
        assert_xml_cursor_equivalence(&peer, "decoded update");
        array.insert(&mut doc.transact_mut(), 0, XmlTextPrelim::new("local"));
        peer_array.insert(
            &mut peer.transact_mut(),
            0,
            XmlElementPrelim::empty("remote"),
        );
        exchange_updates(&[&doc, &peer]);
        assert_xml_cursor_equivalence(&doc, "merged local");
        assert_xml_cursor_equivalence(&peer, "merged peer");
    }
}
