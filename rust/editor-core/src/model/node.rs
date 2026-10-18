use std::collections::HashMap;
use std::sync::Arc;

use crate::model::attrs::Attrs;
use crate::model::fragment::Fragment;
use crate::model::mark::Mark;

#[cfg(test)]
thread_local! {
    pub(crate) static NODE_HANDLE_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DEEP_NODE_PAYLOAD_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_deep_node_payload_clones_for_test() {
    DEEP_NODE_PAYLOAD_CLONES.set(0);
}

#[cfg(test)]
pub(crate) fn take_deep_node_payload_clones_for_test() -> usize {
    DEEP_NODE_PAYLOAD_CLONES.replace(0)
}

const TEXT_NODE_TYPE: &str = "text";

#[derive(Debug)]
enum NodeData {
    Text {
        text: String,
        marks: Vec<Mark>,
    },
    Void {
        node_type: String,
        attrs: Attrs,
    },
    Element {
        node_type: String,
        attrs: Attrs,
        content: Fragment,
    },
}

const _: () = assert!(
    std::mem::size_of::<NodeData>()
        == std::mem::size_of::<String>()
            + std::mem::size_of::<Attrs>()
            + std::mem::size_of::<Fragment>()
);

impl Clone for NodeData {
    fn clone(&self) -> Self {
        #[cfg(test)]
        DEEP_NODE_PAYLOAD_CLONES.set(DEEP_NODE_PAYLOAD_CLONES.get().saturating_add(1));
        match self {
            Self::Text { text, marks } => Self::Text {
                text: text.clone(),
                marks: marks.clone(),
            },
            Self::Void { node_type, attrs } => Self::Void {
                node_type: node_type.clone(),
                attrs: attrs.clone(),
            },
            Self::Element {
                node_type,
                attrs,
                content,
            } => Self::Element {
                node_type: node_type.clone(),
                attrs: attrs.clone(),
                content: content.clone(),
            },
        }
    }
}

impl NodeData {
    fn take_recursive_payloads(&mut self, pending_nodes: &mut Vec<Node>) {
        match self {
            Self::Text { marks, .. } => marks.clear(),
            Self::Void { .. } => {}
            Self::Element { content, .. } => {
                pending_nodes.extend(content.take_children_for_drop());
            }
        }
    }
}

impl Drop for NodeData {
    fn drop(&mut self) {
        let mut pending_nodes = Vec::new();
        self.take_recursive_payloads(&mut pending_nodes);
        while let Some(mut node) = pending_nodes.pop() {
            if let Some(data) = Arc::get_mut(&mut node.data) {
                data.take_recursive_payloads(&mut pending_nodes);
            }
            // Unique payloads were emptied above before their Arc destructor
            // runs. Shared payloads are drained by whichever owner is last.
            drop(node);
        }
    }
}

#[derive(Debug)]
#[cfg_attr(not(test), derive(Clone))]
pub struct Node {
    data: Arc<NodeData>,
}

#[cfg(test)]
impl Clone for Node {
    fn clone(&self) -> Self {
        NODE_HANDLE_CLONES.set(NODE_HANDLE_CLONES.get() + 1);
        Self {
            data: Arc::clone(&self.data),
        }
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        if self.shares_storage_with(other) {
            return true;
        }
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            if left.shares_storage_with(right) {
                continue;
            }
            if left.node_type() != right.node_type()
                || !crate::boundary::json_objects_equal_stack_safe(left.attrs(), right.attrs())
                || left.marks() != right.marks()
            {
                return false;
            }
            match (left.data.as_ref(), right.data.as_ref()) {
                (NodeData::Text { text: left, .. }, NodeData::Text { text: right, .. })
                    if left == right => {}
                (NodeData::Void { .. }, NodeData::Void { .. }) => {}
                (
                    NodeData::Element { content: left, .. },
                    NodeData::Element { content: right, .. },
                ) if left.child_count() == right.child_count() => {
                    pending.extend(left.iter().zip(right.iter()));
                }
                _ => return false,
            }
        }
        true
    }
}

impl Eq for Node {}

impl Node {
    /// Create a text node with the given content and marks.
    pub fn text(text: String, marks: Vec<Mark>) -> Self {
        Self {
            data: Arc::new(NodeData::Text { text, marks }),
        }
    }

    /// Create a void (atomic) node like hardBreak or horizontalRule.
    pub fn void(node_type: String, attrs: HashMap<String, serde_json::Value>) -> Self {
        Self {
            data: Arc::new(NodeData::Void {
                node_type,
                attrs: Attrs::new(attrs),
            }),
        }
    }

    /// Create an element (container) node like doc, paragraph, list, etc.
    pub fn element(
        node_type: String,
        attrs: HashMap<String, serde_json::Value>,
        content: Fragment,
    ) -> Self {
        Self {
            data: Arc::new(NodeData::Element {
                node_type,
                attrs: Attrs::new(attrs),
                content,
            }),
        }
    }

    /// The node type name (e.g. "paragraph", "text", "hardBreak").
    pub fn node_type(&self) -> &str {
        match self.data.as_ref() {
            NodeData::Text { .. } => TEXT_NODE_TYPE,
            NodeData::Void { node_type, .. } | NodeData::Element { node_type, .. } => node_type,
        }
    }

    /// The node's attributes.
    pub fn attrs(&self) -> &HashMap<String, serde_json::Value> {
        match self.data.as_ref() {
            NodeData::Text { .. } => Attrs::empty(),
            NodeData::Void { attrs, .. } | NodeData::Element { attrs, .. } => attrs.get(),
        }
    }

    /// The marks applied to this node (only meaningful for text nodes).
    pub fn marks(&self) -> &[Mark] {
        match self.data.as_ref() {
            NodeData::Text { marks, .. } => marks,
            NodeData::Void { .. } | NodeData::Element { .. } => &[],
        }
    }

    /// Whether this is a text node.
    pub fn is_text(&self) -> bool {
        matches!(self.data.as_ref(), NodeData::Text { .. })
    }

    /// Whether this is a void (atomic) node.
    pub fn is_void(&self) -> bool {
        matches!(self.data.as_ref(), NodeData::Void { .. })
    }

    /// Whether this is an element (container) node.
    pub fn is_element(&self) -> bool {
        matches!(self.data.as_ref(), NodeData::Element { .. })
    }

    /// The token size of this node in the ProseMirror position model.
    ///
    /// - Text nodes: number of Unicode scalar values (chars)
    /// - Void nodes: always 1
    /// - Element nodes: 1 (open) + content size + 1 (close)
    pub fn node_size(&self) -> u32 {
        match self.data.as_ref() {
            NodeData::Text { text, .. } => text.chars().count() as u32,
            NodeData::Void { .. } => 1,
            NodeData::Element { content, .. } => 1 + content.size() + 1,
        }
    }

    /// The size of the node's content (excluding open/close tokens).
    /// For text nodes this equals `node_size()`. For void nodes this is 0.
    /// For element nodes this is the fragment size.
    pub fn content_size(&self) -> u32 {
        match self.data.as_ref() {
            NodeData::Text { text, .. } => text.chars().count() as u32,
            NodeData::Void { .. } => 0,
            NodeData::Element { content, .. } => content.size(),
        }
    }

    /// Collect all text content from this node and its descendants.
    pub fn text_content(&self) -> String {
        let mut output = String::new();
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match node.data.as_ref() {
                NodeData::Text { text, .. } => output.push_str(text),
                NodeData::Void { .. } => {}
                NodeData::Element { content, .. } => {
                    pending.extend(content.iter().rev());
                }
            }
        }
        output
    }

    /// Number of direct children. Text and void nodes have 0 children.
    pub fn child_count(&self) -> usize {
        match self.data.as_ref() {
            NodeData::Element { content, .. } => content.child_count(),
            _ => 0,
        }
    }

    /// Access a direct child by index.
    pub fn child(&self, index: usize) -> Option<&Node> {
        match self.data.as_ref() {
            NodeData::Element { content, .. } => content.child(index),
            _ => None,
        }
    }

    /// Access the content fragment. Returns `None` for text and void nodes.
    pub fn content(&self) -> Option<&Fragment> {
        match self.data.as_ref() {
            NodeData::Element { content, .. } => Some(content),
            _ => None,
        }
    }

    /// The raw text of a text node. Returns `None` for non-text nodes.
    pub fn text_str(&self) -> Option<&str> {
        match self.data.as_ref() {
            NodeData::Text { text, .. } => Some(text.as_str()),
            _ => None,
        }
    }

    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    pub(super) fn replace_node_at_path(&mut self, path: &[u32], replacement: Node) {
        if path.is_empty() {
            *self = replacement;
            return;
        }
        let shared = Arc::get_mut(&mut self.data).is_none();
        if shared {
            self.data = Arc::new(self.data.as_ref().clone());
        }
        let data = Arc::get_mut(&mut self.data).expect("replacement owns its ancestor payload");
        let NodeData::Element {
            node_type,
            attrs,
            content,
        } = data
        else {
            panic!("non-leaf node in path must be an element");
        };
        if !shared {
            // Rebuilding normalizes metadata capacity even on an owned ancestor.
            *node_type = node_type.as_str().to_owned();
            *attrs = attrs.clone();
        }
        content.replace_node_at_path(path, replacement);
    }

    pub(crate) fn history_snapshot_retained_bytes_beyond(&self, base: &Self) -> Option<usize> {
        if self.shares_storage_with(base) {
            return Some(0);
        }
        let (Some(content), Some(base_content)) = (self.content(), base.content()) else {
            return self.history_snapshot_retained_bytes();
        };
        let window = crate::model::unshared_window(
            content.children(),
            base_content.children(),
            Self::shares_storage_with,
        );
        content.children()[window].iter().try_fold(
            self.history_snapshot_own_retained_bytes()?,
            |total, child| total.checked_add(child.history_snapshot_retained_bytes()?),
        )
    }

    pub(crate) fn history_snapshot_retained_bytes(&self) -> Option<usize> {
        let mut total = 0usize;
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            total = total.checked_add(node.history_snapshot_own_retained_bytes()?)?;
            if let NodeData::Element { content, .. } = node.data.as_ref() {
                pending.extend(content.iter());
            }
        }
        Some(total)
    }

    pub(crate) fn history_snapshot_own_retained_bytes(&self) -> Option<usize> {
        let total = crate::model::arc_allocation_retained_bytes(std::mem::size_of::<NodeData>())?;
        match self.data.as_ref() {
            NodeData::Text { text, marks } => marks.iter().try_fold(
                total
                    .checked_add(text.capacity())?
                    .checked_add(marks.capacity().checked_mul(std::mem::size_of::<Mark>())?)?,
                |total, mark| total.checked_add(mark.history_snapshot_clone_retained_bytes()?),
            ),
            NodeData::Void { node_type, attrs } => total
                .checked_add(node_type.capacity())?
                .checked_add(attrs.retained_bytes()?),
            NodeData::Element {
                node_type,
                attrs,
                content,
            } => total
                .checked_add(node_type.capacity())?
                .checked_add(attrs.retained_bytes()?)?
                .checked_add(
                    content
                        .children_capacity()
                        .checked_mul(std::mem::size_of::<Node>())?,
                ),
        }
    }
}

#[cfg(test)]
mod shared_storage_tests {
    use super::*;
    use crate::model::Document;
    use crate::schema::presets::tiptap_schema;
    use crate::serialize::to_prosemirror_json;
    use crate::transform::{apply_step, Step};

    fn document() -> Document {
        Document::new(Node::element(
            "doc".into(),
            HashMap::new(),
            Fragment::from(vec![Node::element(
                "paragraph".into(),
                HashMap::from([("class".into(), serde_json::json!("lead"))]),
                Fragment::from(vec![Node::text("abc".into(), Vec::new())]),
            )]),
        ))
    }

    #[test]
    fn node_and_document_clones_share_immutable_payload_and_preserve_equality() {
        let original = document();
        let cloned = original.clone();

        assert!(original.root().shares_storage_with(cloned.root()));
        assert_eq!(original, cloned);
        assert_eq!(cloned.root().node_type(), "doc");
        assert_eq!(
            cloned.root().child(0).unwrap().attrs()["class"],
            serde_json::json!("lead")
        );
        assert_eq!(
            to_prosemirror_json(&original, &tiptap_schema()),
            to_prosemirror_json(&cloned, &tiptap_schema())
        );
    }

    #[test]
    fn transform_rebuilds_new_path_without_mutating_shared_original() {
        let schema = tiptap_schema();
        let original = document();
        let shared_original = original.clone();
        let (changed, _) = apply_step(
            &original,
            &Step::InsertText {
                pos: 2,
                text: "x".into(),
                marks: Vec::new(),
            },
            &schema,
        )
        .unwrap();

        assert!(original.root().shares_storage_with(shared_original.root()));
        assert_eq!(original, shared_original);
        assert_ne!(changed, original);
        assert_eq!(
            original
                .root()
                .child(0)
                .unwrap()
                .child(0)
                .unwrap()
                .text_str(),
            Some("abc")
        );
        assert_eq!(
            changed
                .root()
                .child(0)
                .unwrap()
                .child(0)
                .unwrap()
                .text_str(),
            Some("axbc")
        );
    }
}
