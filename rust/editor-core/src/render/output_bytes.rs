use std::collections::HashMap;

use super::RenderElement;

pub(crate) fn attrs_bytes(attrs: &HashMap<String, serde_json::Value>) -> usize {
    let serialized_len = if attrs.is_empty() {
        b"{}".len()
    } else {
        serde_json::to_vec(attrs).map_or(usize::MAX, |value| value.len())
    };
    8usize.saturating_add(serialized_len)
}

pub(crate) fn json_bytes(value: &serde_json::Value) -> usize {
    8usize.saturating_add(serde_json::to_vec(value).map_or(usize::MAX, |value| value.len()))
}

pub(crate) fn string_bytes(value: &str) -> usize {
    8usize.saturating_add(value.len())
}

pub(crate) fn render_element_bytes(element: &RenderElement) -> usize {
    let payload = match element {
        RenderElement::Table { table, .. } => table.retained_bytes(render_element_bytes),
        RenderElement::TextRun { text, marks } => {
            marks
                .iter()
                .fold(string_bytes(text).saturating_add(8), |bytes, mark| {
                    bytes
                        .saturating_add(string_bytes(&mark.mark_type))
                        .saturating_add(attrs_bytes(&mark.attrs))
                })
        }
        RenderElement::VoidInline {
            node_type, attrs, ..
        }
        | RenderElement::VoidBlock {
            node_type, attrs, ..
        } => string_bytes(node_type)
            .saturating_add(4)
            .saturating_add(attrs_bytes(attrs)),
        RenderElement::OpaqueInlineAtom {
            node_type,
            label,
            attrs,
            mention_theme,
            ..
        } => string_bytes(node_type)
            .saturating_add(string_bytes(label))
            .saturating_add(4)
            .saturating_add(1)
            .saturating_add(attrs_bytes(attrs))
            .saturating_add(mention_theme.as_ref().map_or(0, attrs_bytes)),
        RenderElement::OpaqueBlockAtom {
            node_type,
            label,
            attrs,
            ..
        } => string_bytes(node_type)
            .saturating_add(string_bytes(label))
            .saturating_add(4)
            .saturating_add(attrs_bytes(attrs)),
        RenderElement::BlockStart {
            node_type,
            list_context,
            ..
        } => string_bytes(node_type)
            .saturating_add(2)
            .saturating_add(1)
            .saturating_add(list_context.as_ref().map_or(0, |context| {
                18usize.saturating_add(context.kind.as_ref().map_or(0, |kind| string_bytes(kind)))
            })),
        RenderElement::BlockEnd => 0,
    };
    1usize.saturating_add(payload)
}
