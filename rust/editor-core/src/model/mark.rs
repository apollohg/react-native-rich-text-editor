use std::collections::HashMap;

use crate::model::attrs::Attrs;

/// A mark represents inline formatting (bold, italic, link, etc.) applied to
/// a text node. Marks don't occupy positions in the document's token stream —
/// they're metadata attached to text nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    mark_type: String,
    attrs: Attrs,
}

const _: () = assert!(
    std::mem::size_of::<Mark>() == std::mem::size_of::<String>() + std::mem::size_of::<Attrs>()
);

impl Mark {
    /// Create a new mark with the given type name and attributes.
    pub fn new(mark_type: String, attrs: HashMap<String, serde_json::Value>) -> Self {
        Self {
            mark_type,
            attrs: Attrs::new(attrs),
        }
    }

    /// The mark type name (e.g. "bold", "italic", "link").
    pub fn mark_type(&self) -> &str {
        &self.mark_type
    }

    /// The mark's attributes (e.g. `{"href": "https://..."}` for a link mark).
    pub fn attrs(&self) -> &HashMap<String, serde_json::Value> {
        self.attrs.get()
    }

    pub(crate) fn history_snapshot_clone_retained_bytes(&self) -> Option<usize> {
        self.mark_type
            .capacity()
            .checked_add(self.attrs.retained_bytes()?)
    }
}
