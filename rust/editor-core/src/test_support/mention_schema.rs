use serde_json::json;

use crate::schema::presets::tiptap_table_schema;
use crate::schema::Schema;

pub(crate) const MENTION_NODE: &str = "mention";

pub(crate) fn mention_table_schema() -> Schema {
    let base = tiptap_table_schema();
    let mention_spec = Schema::from_json(&json!({
        "nodes": [
            { "name": "doc", "content": "paragraph+", "role": "doc" },
            { "name": "paragraph", "content": "inline*", "group": "block", "role": "textBlock" },
            { "name": "text", "content": "", "group": "inline", "role": "text" },
            {
                "name": MENTION_NODE,
                "content": "",
                "group": "inline",
                "role": "inline",
                "isVoid": true,
                "allowUndeclaredAttrs": true,
                "attrs": { "label": { "default": null } }
            }
        ],
        "marks": []
    }))
    .expect("the mention schema is valid")
    .node(MENTION_NODE)
    .cloned()
    .expect("the mention schema declares the mention node");
    Schema::new(
        base.all_nodes().cloned().chain([mention_spec]).collect(),
        base.all_marks().cloned().collect(),
    )
}
