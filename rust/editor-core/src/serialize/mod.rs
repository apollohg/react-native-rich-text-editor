pub mod html_in;
pub mod html_out;
pub mod json_in;
pub mod json_out;

#[allow(unused_imports)]
pub use html_in::{from_html, from_html_with_limits, FromHtmlOptions, ParseError};
pub use html_out::to_html;
#[allow(unused_imports)]
pub use json_in::{
    from_prosemirror_json, from_prosemirror_json_with_limits, JsonParseError, UnknownTypeMode,
};
pub(crate) use json_in::{
    normalized_wire_json_node_type, parse_wire_heading_level_str, rehydrate_reserved_html_opaque,
};
pub(crate) use json_out::node_to_json as node_to_prosemirror_json;
pub use json_out::to_prosemirror_json;

const TASK_CHECKED_ATTR: &str = "checked";
const TASK_CONTENT_HTML_TAG: &str = "div";

fn is_tiptap_task_item(spec: &crate::schema::NodeSpec) -> bool {
    matches!(spec.role, crate::schema::NodeRole::ListItem)
        && spec.html_rules.as_ref().is_some_and(|rules| {
            rules.static_attrs.iter().any(|(name, value)| {
                name == "data-type" && value == crate::schema::presets::TASK_ITEM_NODE_NAME
            })
        })
}

fn default_node_attrs(
    spec: &crate::schema::NodeSpec,
) -> std::collections::HashMap<String, serde_json::Value> {
    spec.attrs
        .iter()
        .filter_map(|(name, attr)| {
            attr.default.as_ref().map(|value| {
                (
                    name.clone(),
                    crate::boundary::clone_json_value_stack_safe(value),
                )
            })
        })
        .collect()
}
