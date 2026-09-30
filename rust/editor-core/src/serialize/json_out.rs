use serde_json::{json, Map, Value};

use crate::model::{Document, Node};
use crate::schema::{NodeSpec, Schema};

/// Serialize a document to ProseMirror JSON format using the given schema.
///
/// The output matches the ProseMirror JSON representation:
/// ```json
/// {
///   "type": "doc",
///   "content": [
///     { "type": "paragraph", "content": [{ "type": "text", "text": "Hello" }] }
///   ]
/// }
/// ```
///
/// Node and mark type names are taken verbatim from the document tree (which
/// should already use the naming convention of the schema that created it).
/// Attrs are included only when non-empty, and default-valued attrs (per the
/// schema spec) are omitted.
pub fn to_prosemirror_json(doc: &Document, schema: &Schema) -> Value {
    node_to_json(doc.root(), schema)
}

pub(crate) fn node_to_json(root: &Node, schema: &Schema) -> Value {
    enum Frame<'a> {
        Visit(&'a Node),
        Build(&'a Node, usize),
    }

    let mut frames = vec![Frame::Visit(root)];
    let mut built = Vec::new();
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Visit(node) => {
                let children = node
                    .content()
                    .map(|content| content.children())
                    .unwrap_or(&[]);
                if children.is_empty() || node.node_type() == "__opaque_json" {
                    built.push(node_to_json_shallow(node, schema));
                    continue;
                }
                frames.push(Frame::Build(node, children.len()));
                frames.extend(children.iter().rev().map(Frame::Visit));
            }
            Frame::Build(node, child_count) => {
                let first_child = built
                    .len()
                    .checked_sub(child_count)
                    .expect("JSON projection child stack is balanced");
                let children = built.split_off(first_child);
                let mut value = node_to_json_shallow(node, schema);
                value
                    .as_object_mut()
                    .expect("semantic nodes project to objects")
                    .insert("content".to_string(), Value::Array(children));
                built.push(value);
            }
        }
    }
    built.pop().expect("one projected root")
}

fn node_to_json_shallow(node: &Node, schema: &Schema) -> Value {
    if node.node_type() == "__opaque_json" {
        return node
            .attrs()
            .get("original_json")
            .map(crate::boundary::clone_json_value_stack_safe)
            .unwrap_or(Value::Null);
    }
    let spec = schema.node(node.node_type());
    let mut obj = Map::new();
    obj.insert("type".to_string(), json!(projected_node_type(node, spec)));

    if node.is_text() {
        obj.insert("text".to_string(), json!(node.text_str().unwrap_or("")));

        if !node.marks().is_empty() {
            let marks_json = projected_marks(node);
            obj.insert("marks".to_string(), Value::Array(marks_json));
        }
    } else {
        let attrs_json = build_attrs_json(node, spec);
        if !attrs_json.is_empty() {
            obj.insert("attrs".to_string(), Value::Object(attrs_json));
        }
    }

    Value::Object(obj)
}

fn projected_node_type<'a>(node: &'a Node, spec: Option<&'a NodeSpec>) -> &'a str {
    spec.and_then(|spec| spec.json_projection.as_ref())
        .map_or(node.node_type(), |projection| projection.node_type.as_str())
}

fn projected_marks(node: &Node) -> Vec<Value> {
    node.marks()
        .iter()
        .map(|m| {
            let mut mark_obj = Map::new();
            mark_obj.insert("type".to_string(), json!(m.mark_type()));
            if !m.attrs().is_empty() {
                mark_obj.insert(
                    "attrs".to_string(),
                    Value::Object(
                        m.attrs()
                            .iter()
                            .map(|(key, value)| {
                                (
                                    key.clone(),
                                    crate::boundary::clone_json_value_stack_safe(value),
                                )
                            })
                            .collect(),
                    ),
                );
            }
            Value::Object(mark_obj)
        })
        .collect()
}

/// Build the attrs JSON object for a node, omitting attributes whose values
/// match the schema-defined defaults.
fn build_attrs_json(node: &Node, spec: Option<&NodeSpec>) -> Map<String, Value> {
    let mut attrs_map = Map::new();

    for (key, value) in node.attrs() {
        let is_default = spec
            .and_then(|s| s.attrs.get(key))
            .and_then(|a| a.default.as_ref())
            .map(|default| crate::boundary::json_values_equal_stack_safe(default, value))
            .unwrap_or(false);

        if !is_default {
            attrs_map.insert(
                key.clone(),
                crate::boundary::clone_json_value_stack_safe(value),
            );
        }
    }

    if let Some(projection) = spec.and_then(|spec| spec.json_projection.as_ref()) {
        attrs_map.extend(projection.attrs.iter().map(|(name, value)| {
            (
                name.clone(),
                crate::boundary::clone_json_value_stack_safe(value),
            )
        }));
    }

    attrs_map
}

pub(crate) fn write_node_json<'a>(
    sink: &mut impl std::io::Write,
    node: &'a Node,
    schema: &'a Schema,
) -> std::io::Result<()> {
    use crate::boundary::{JsonWriteFrame as Frame, StackSafeJsonValue};
    crate::boundary::write_json_frames(
        sink,
        smallvec::smallvec![Frame::Expand(node)],
        |node, frames| {
            if node.node_type() == "__opaque_json" {
                frames.push(match node.attrs().get("original_json") {
                    Some(value) => Frame::Value(value),
                    None => Frame::Raw(b"null"),
                });
                return;
            }
            let spec = schema.node(node.node_type());
            frames.push(Frame::Raw(b"}"));
            frames.push(Frame::String(projected_node_type(node, spec)));
            frames.push(Frame::Raw(b"\"type\":"));
            if node.is_text() {
                frames.push(Frame::Raw(b","));
                frames.push(Frame::String(node.text_str().unwrap_or("")));
                frames.push(Frame::Raw(b"\"text\":"));
                if !node.marks().is_empty() {
                    frames.push(Frame::Raw(b","));
                    frames.push(Frame::OwnedValue(StackSafeJsonValue::new(Value::Array(
                        projected_marks(node),
                    ))));
                    frames.push(Frame::Raw(b"\"marks\":"));
                }
            }
            let children = node
                .content()
                .map(|content| content.children())
                .unwrap_or(&[]);
            if !children.is_empty() {
                frames.push(Frame::Raw(b","));
                frames.push(Frame::Raw(b"]"));
                for (index, child) in children.iter().enumerate().rev() {
                    if index + 1 < children.len() {
                        frames.push(Frame::Raw(b","));
                    }
                    frames.push(Frame::Expand(child));
                }
                frames.push(Frame::Raw(b"\"content\":["));
            }
            if !node.is_text() {
                let attrs = build_attrs_json(node, spec);
                if !attrs.is_empty() {
                    frames.push(Frame::Raw(b","));
                    frames.push(Frame::OwnedValue(StackSafeJsonValue::new(Value::Object(
                        attrs,
                    ))));
                    frames.push(Frame::Raw(b"\"attrs\":"));
                }
            }
            frames.push(Frame::Raw(b"{"));
        },
    )
}

pub(crate) fn filtered_attrs(node: &Node, cell: bool) -> Vec<(&String, &Value)> {
    let mut attrs: Vec<_> = node
        .attrs()
        .iter()
        .filter(|(key, _)| !cell || !matches!(key.as_str(), "colspan" | "rowspan" | "colwidth"))
        .collect();
    attrs.sort_unstable_by_key(|(key, _)| *key);
    attrs
}

pub(crate) fn write_attrs_json(output: &mut String, node: &Node, cell: bool) {
    #[cfg(test)]
    crate::yrs_engine::observability::record_attribute_serialization();
    use crate::boundary::JsonWriteFrame as Frame;
    let attrs = filtered_attrs(node, cell);
    let mut frames = smallvec::smallvec![Frame::Raw(b"}")];
    for (index, (key, value)) in attrs.iter().enumerate().rev() {
        if index + 1 < attrs.len() {
            frames.push(Frame::Raw(b","));
        }
        frames.push(Frame::Value(value));
        frames.push(Frame::Raw(b":"));
        frames.push(Frame::String(key));
    }
    frames.push(Frame::Raw(b"{"));
    let mut bytes = Vec::new();
    crate::boundary::write_json_frames(&mut bytes, frames, |(): (), _| {})
        .expect("JSON attributes serialize to memory");
    output.push_str(std::str::from_utf8(&bytes).expect("JSON is UTF-8"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Fragment, Mark};
    use std::collections::HashMap;

    #[test]
    fn streamed_nodes_match_projection_defaults_marks_and_opaque_values() {
        let marked = Node::text(
            "quote=\" newline=\n 雪🙂".into(),
            vec![
                Mark::new(
                    "link".into(),
                    HashMap::from([("href".into(), json!("/a?b=\"c"))]),
                ),
                Mark::new("bold".into(), HashMap::new()),
            ],
        );
        let nodes = [
            Node::element(
                "h2".into(),
                HashMap::from([("level".into(), json!(99))]),
                Fragment::from(vec![marked.clone()]),
            ),
            Node::element(
                "paragraph".into(),
                HashMap::new(),
                Fragment::from(Vec::new()),
            ),
            Node::element(
                "orderedList".into(),
                HashMap::from([("start".into(), json!(1))]),
                Fragment::from(Vec::new()),
            ),
            Node::void(
                "image".into(),
                HashMap::from([
                    ("src".into(), json!("/雪.png")),
                    ("width".into(), json!(1.5)),
                    (
                        "metadata".into(),
                        json!({"large":u64::MAX,"nested":[true,null]}),
                    ),
                ]),
            ),
            Node::element(
                "__opaque_json".into(),
                HashMap::from([(
                    "original_json".into(),
                    json!([{"type":"text","content":[],"text":"opaque"},null]),
                )]),
                Fragment::from(vec![marked.clone()]),
            ),
            Node::element(
                "__opaque_json".into(),
                HashMap::new(),
                Fragment::from(vec![marked.clone()]),
            ),
            marked,
        ];
        for schema in [
            crate::schema::presets::tiptap_schema(),
            crate::schema::presets::prosemirror_schema(),
        ] {
            for node in &nodes {
                let expected = crate::boundary::serialize_json_value_stack_safe(
                    &node_to_json(node, &schema),
                    0,
                );
                let mut actual = Vec::new();
                write_node_json(&mut actual, node, &schema).unwrap();
                assert_eq!(
                    actual,
                    expected,
                    "exact canonical bytes for {}",
                    node.node_type()
                );
            }
        }
    }
}
