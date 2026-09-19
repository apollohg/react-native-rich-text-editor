use crate::block::ItemContent;
use crate::store::Store;
use crate::types::{TypePtr, TypeRef};
use crate::Any;

struct Budget {
    bytes: usize,
    items: usize,
}

impl Budget {
    fn charge(&mut self, bytes: usize) -> Option<()> {
        self.bytes = self.bytes.checked_sub(bytes)?;
        Some(())
    }

    fn item(&mut self, bytes: usize) -> Option<()> {
        self.items = self.items.checked_sub(1)?;
        self.charge(bytes)
    }

    fn payload(&mut self, len: usize) -> Option<()> {
        // Covers JSON escaping, temporary strings, Vec growth and wire copies.
        self.charge(len.checked_mul(32)?)
    }

    fn any(&mut self, value: &Any, depth: usize) -> Option<()> {
        if depth > 64 {
            return None;
        }
        self.item(256)?;
        match value {
            Any::String(value) => self.payload(value.len())?,
            Any::Buffer(value) => self.payload(value.len())?,
            Any::Array(values) => {
                if values.len() > self.items {
                    return None;
                }
                for value in values.iter() {
                    self.any(value, depth + 1)?;
                }
            }
            Any::Map(values) => {
                if values.len() > self.items {
                    return None;
                }
                for (key, value) in values.iter() {
                    self.payload(key.len())?;
                    self.any(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Some(())
    }
}

impl Store {
    pub(crate) fn preflight_history_encoding(
        &self,
        max_bytes: usize,
        max_items: usize,
    ) -> Option<()> {
        if self.pending.is_some() || self.pending_ds.is_some() || !self.subdocs.is_empty() {
            return None;
        }
        let mut budget = Budget {
            bytes: max_bytes,
            items: max_items,
        };
        // Covers encoder growth, state-vector/sort storage and worst-case deletion ranges.
        budget.charge(4096)?;
        for (_, blocks) in self.blocks.iter() {
            budget.item(2048)?;
            if blocks.len() > budget.items {
                return None;
            }
            for block in blocks.iter() {
                budget.item(2048)?;
                let Some(item) = block.as_item() else {
                    continue;
                };
                match &item.parent {
                    TypePtr::Branch(branch) => {
                        if branch.item.is_none() {
                            budget.payload(branch.name.as_ref()?.len())?;
                        }
                    }
                    TypePtr::Named(name) => budget.payload(name.len())?,
                    TypePtr::ID(_) => {}
                    TypePtr::Unknown => return None,
                }
                if let Some(key) = &item.parent_sub {
                    budget.payload(key.len())?;
                }
                match &item.content {
                    ItemContent::Any(values) => {
                        if values.len() > budget.items {
                            return None;
                        }
                        for value in values {
                            budget.any(value, 0)?;
                        }
                    }
                    ItemContent::Binary(value) => budget.payload(value.len())?,
                    ItemContent::String(value) => budget.payload(value.as_str().len())?,
                    ItemContent::JSON(values) => {
                        if values.len() > budget.items {
                            return None;
                        }
                        for value in values {
                            budget.item(256)?;
                            budget.payload(value.len())?;
                        }
                    }
                    ItemContent::Embed(value) => budget.any(value, 0)?,
                    ItemContent::Format(key, value) => {
                        budget.payload(key.len())?;
                        budget.any(value, 0)?;
                    }
                    ItemContent::Type(branch) => match &branch.type_ref {
                        TypeRef::XmlElement(name) => budget.payload(name.len())?,
                        TypeRef::Array
                        | TypeRef::Map
                        | TypeRef::Text
                        | TypeRef::XmlFragment
                        | TypeRef::XmlHook
                        | TypeRef::XmlText
                        | TypeRef::Undefined => {}
                        _ => return None,
                    },
                    ItemContent::Deleted(_) => {}
                    ItemContent::Doc(_, _) => return None,
                }
            }
        }
        Some(())
    }
}
