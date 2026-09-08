use crate::boundary::ResourceLimits;
use crate::command_planner::{SemanticCommandHistory, SemanticCommandPlan, SemanticOperation};
use crate::model::{Document, Fragment, Node};
use crate::schema::{NodeRole, Schema};
use crate::selection::Selection;
use crate::serialize::{
    from_prosemirror_json_with_limits, to_html, to_prosemirror_json, UnknownTypeMode,
};
use serde_json::{json, Value};

pub(crate) struct ClipboardSlice {
    pub document: Document,
    pub open_start: usize,
    pub open_end: usize,
}

pub(crate) fn selection_range(document: &Document, selection: &Selection) -> (u32, u32) {
    match selection {
        Selection::Node { pos } => {
            let size = document
                .resolve(*pos)
                .ok()
                .and_then(|resolved| {
                    let mut offset = 0;
                    resolved
                        .parent(document)
                        .content()?
                        .iter()
                        .find_map(|node| {
                            let found =
                                (offset == resolved.parent_offset).then_some(node.node_size());
                            offset += node.node_size();
                            found
                        })
                })
                .unwrap_or(1);
            (*pos, pos.saturating_add(size))
        }
        _ => (selection.from(document), selection.to(document)),
    }
}

fn copy_element(node: &Node, children: Vec<Node>) -> Node {
    Node::element(
        node.node_type().into(),
        node.attrs().clone(),
        Fragment::from(children),
    )
}

fn clipped_children(node: &Node, start: u32, from: u32, to: u32) -> Vec<Node> {
    let mut children = Vec::new();
    let mut position = start;
    for child in node.content().into_iter().flat_map(Fragment::iter) {
        let end = position + child.node_size();
        if from < end && to > position {
            if from <= position && to >= end {
                children.push(child.clone());
            } else if let Some(text) = child.text_str() {
                let text = text
                    .chars()
                    .skip(from.saturating_sub(position) as usize)
                    .take((to.min(end) - from.max(position)) as usize)
                    .collect();
                children.push(Node::text(text, child.marks().to_vec()));
            } else if child.is_element() {
                children.push(copy_element(
                    child,
                    clipped_children(child, position + 1, from, to),
                ));
            }
        }
        position = end;
    }
    children
}

fn boundary_depth(document: &Document, position: u32) -> usize {
    document
        .resolve(position)
        .map(|resolved| resolved.node_path.len())
        .unwrap_or(0)
}

pub(crate) fn export(document: &Document, selection: &Selection, schema: &Schema) -> Option<Value> {
    let (from, to) = selection_range(document, selection);
    if from >= to || to > document.content_size() {
        return None;
    }
    let selected = Document::new(Node::element(
        document.root().node_type().into(),
        Default::default(),
        Fragment::from(clipped_children(document.root(), 0, from, to)),
    ));
    Some(json!({
        "fragment": json!({"version":1,"schema":crate::schema::schema_fingerprint(schema),"openStart":boundary_depth(document, from),"openEnd":boundary_depth(document, to),"document":to_prosemirror_json(&selected, schema),"text":readable_text(&selected, schema)}).to_string(),
        "html": to_html(&selected, schema),
        "text": readable_text(&selected, schema)
    }))
}

pub(crate) fn readable_text(document: &Document, schema: &Schema) -> String {
    fn text(node: &Node, schema: &Schema) -> String {
        if let Some(text) = node.text_str() {
            return text.into();
        }
        if node.is_void() {
            if schema
                .node(node.node_type())
                .is_some_and(|spec| spec.html_tag.as_deref() == Some("br"))
            {
                return "\n".into();
            }
            return ["label", "alt", "name", "title"]
                .iter()
                .find_map(|key| node.attrs().get(*key).and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
        }
        let children = node.content().map(Fragment::children).unwrap_or(&[]);
        let block_children = children.iter().any(|child| {
            schema.node(child.node_type()).is_some_and(|spec| {
                !matches!(
                    spec.role,
                    NodeRole::Inline | NodeRole::HardBreak | NodeRole::Text
                )
            })
        });
        children
            .iter()
            .map(|child| text(child, schema))
            .collect::<Vec<_>>()
            .join(if block_children { "\n" } else { "" })
    }
    text(document.root(), schema)
}

pub(crate) fn decode(
    source: &str,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Option<ClipboardSlice> {
    if source.len() > limits.max_input_bytes {
        return None;
    }
    let value = crate::boundary::parse_json_value_stack_safe(
        source,
        limits
            .max_document_depth
            .saturating_mul(4)
            .saturating_add(16),
        limits.max_document_depth,
        "DOCUMENT_LIMIT_EXCEEDED",
        "DOCUMENT_INVALID",
    )
    .ok()?;
    let value = value.as_value();
    if value.get("version")?.as_u64()? != 1 {
        return None;
    }
    let open_start = usize::try_from(value.get("openStart")?.as_u64()?).ok()?;
    let open_end = usize::try_from(value.get("openEnd")?.as_u64()?).ok()?;
    let wire_document = value.get("document")?;
    let mode = if value.get("schema").and_then(Value::as_str)
        == Some(crate::schema::schema_fingerprint(schema).as_str())
    {
        UnknownTypeMode::Preserve
    } else {
        UnknownTypeMode::Error
    };
    let document = from_prosemirror_json_with_limits(wire_document, schema, mode, limits).ok()?;
    if to_prosemirror_json(&document, schema) != *wire_document {
        return None;
    }
    for (depth, first) in [(open_start, true), (open_end, false)] {
        let mut node = document.root();
        for _ in 0..depth {
            node = node.child(if first {
                0
            } else {
                node.child_count().checked_sub(1)?
            })?;
            if !node.is_element() {
                return None;
            }
        }
    }
    Some(ClipboardSlice {
        document,
        open_start,
        open_end,
    })
}

pub(crate) fn fragment_text(source: &str, limits: &ResourceLimits) -> Option<String> {
    if source.len() > limits.max_input_bytes {
        return None;
    }
    let value = crate::boundary::parse_json_value_stack_safe(
        source,
        limits
            .max_document_depth
            .saturating_mul(4)
            .saturating_add(16),
        limits.max_document_depth,
        "DOCUMENT_LIMIT_EXCEEDED",
        "DOCUMENT_INVALID",
    )
    .ok()?;
    let value = value.as_value();
    (value.get("version")?.as_u64()? == 1).then_some(())?;
    value.get("text")?.as_str().map(str::to_owned)
}

fn join_edges(
    mut left: Vec<Node>,
    mut right: Vec<Node>,
    depth: usize,
    schema: &Schema,
) -> (Vec<Node>, usize) {
    if depth == 0 || left.is_empty() || right.is_empty() {
        left.extend(right);
        return (left, 0);
    }
    let a = left.last().unwrap();
    let b = &right[0];
    let compatible = a.node_type() == b.node_type()
        || [a, b].iter().all(|node| {
            schema
                .node(node.node_type())
                .is_some_and(|spec| matches!(spec.role, NodeRole::TextBlock))
        });
    if !compatible || !a.is_element() || !b.is_element() {
        left.extend(right);
        return (left, 0);
    }
    let a = left.pop().unwrap();
    let b = right.remove(0);
    let (children, merged) = join_edges(
        a.content().unwrap().children().to_vec(),
        b.content().unwrap().children().to_vec(),
        depth - 1,
        schema,
    );
    left.push(copy_element(&a, children));
    left.extend(right);
    (left, merged + 1)
}

pub(crate) fn replacement(
    document: &Document,
    selection: &Selection,
    slice: &ClipboardSlice,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Option<SemanticCommandPlan> {
    let (from, to) = selection_range(document, selection);
    if to > document.content_size() {
        return None;
    }
    let mut content = slice.document.root().content()?.children().to_vec();
    let mut open_start = slice.open_start;
    let mut open_end = slice.open_end;
    while content.len() == 1
        && open_start > boundary_depth(document, from)
        && open_end > boundary_depth(document, to)
        && open_start > 1
        && open_end > 1
    {
        content = content[0].content()?.children().to_vec();
        open_start -= 1;
        open_end -= 1;
    }

    let mut left = clipped_children(document.root(), 0, 0, from);
    let mut right = clipped_children(document.root(), 0, to, document.content_size());
    let empty_text_block = |node: &Node| {
        node.content_size() == 0
            && schema
                .node(node.node_type())
                .is_some_and(|spec| matches!(spec.role, NodeRole::TextBlock))
    };
    if !content.is_empty()
        && (open_start == 0
            || content.first().is_some_and(|node| {
                schema
                    .node(node.node_type())
                    .is_some_and(|spec| matches!(spec.role, NodeRole::TextBlock))
            }))
        && boundary_depth(document, from) == 1
        && left.last().is_some_and(empty_text_block)
    {
        left.pop();
        open_start = 0;
    }
    if !content.is_empty()
        && open_end == 0
        && boundary_depth(document, to) == 1
        && right.first().is_some_and(empty_text_block)
    {
        right.remove(0);
    }
    let deleting = content.is_empty();
    let (inserted, _) = join_edges(
        left,
        content,
        open_start.min(boundary_depth(document, from)),
        schema,
    );
    let before_right_size: u32 = inserted.iter().map(Node::node_size).sum();
    let join_depth = if deleting {
        boundary_depth(document, from).min(boundary_depth(document, to))
    } else {
        open_end.min(boundary_depth(document, to))
    };
    let (mut result, joined) = join_edges(inserted, right, join_depth, schema);
    if result.is_empty() {
        let spec = schema
            .node_by_html_tag("p")
            .or_else(|| schema.node("paragraph"))?;
        result.push(Node::element(
            spec.name.clone(),
            spec.attrs
                .iter()
                .filter_map(|(key, attr)| attr.default.clone().map(|value| (key.clone(), value)))
                .collect(),
            Fragment::empty(),
        ));
    }
    let candidate = Document::new(copy_element(document.root(), result));
    crate::transform::DocumentValidator::validate(&candidate, schema, limits).ok()?;
    let old = document.root().content()?.children();
    let new = candidate.root().content()?.children();
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let replace_from = old[..prefix].iter().map(Node::node_size).sum();
    let replace_to = document.content_size()
        - old[old.len() - suffix..]
            .iter()
            .map(Node::node_size)
            .sum::<u32>();
    if candidate == *document && matches!(selection, Selection::Node { .. }) {
        return Some(SemanticCommandPlan {
            operations: vec![],
            selection_after: Some(selection.clone()),
            history: SemanticCommandHistory::InputBoundary,
        });
    }
    let cursor = before_right_size
        .saturating_sub(joined as u32)
        .min(candidate.content_size());
    let mut selection_after = Selection::cursor(cursor);
    let mut position = 0;
    for node in candidate.root().content()?.iter() {
        let end = position + node.node_size();
        if end == cursor && node.is_void() {
            selection_after = Selection::node(position);
            break;
        }
        position = end;
    }
    Some(SemanticCommandPlan {
        operations: vec![SemanticOperation::ReplaceRange {
            from: replace_from,
            to: replace_to,
            content: Fragment::from(new[prefix..new.len() - suffix].to_vec()),
        }],
        selection_after: Some(selection_after),
        history: SemanticCommandHistory::InputBoundary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::presets::tiptap_schema;
    use crate::serialize::{from_prosemirror_json, UnknownTypeMode};
    use serde_json::json;

    #[test]
    fn clipboard_partial_unicode_marks_replace_selection() {
        let schema = tiptap_schema();
        let source = from_prosemirror_json(&json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a😀bc","marks":[{"type":"bold"}]}]}]}), &schema, UnknownTypeMode::Error).unwrap();
        let data = export(&source, &Selection::text(4, 2), &schema).unwrap();
        assert_eq!(data["text"], "😀b");
        let slice = decode(
            data["fragment"].as_str().unwrap(),
            &schema,
            &ResourceLimits::default(),
        )
        .unwrap();
        let plan = replacement(
            &source,
            &Selection::text(2, 4),
            &slice,
            &schema,
            &ResourceLimits::default(),
        )
        .unwrap();
        let after =
            crate::command_planner::apply_operations(&source, &schema, &plan.operations).unwrap();
        assert_eq!(after.root().text_content(), "a😀bc");
        assert_eq!(
            after.root().child(0).unwrap().child(0).unwrap().marks()[0].mark_type(),
            "bold"
        );
    }
    #[test]
    fn clipboard_custom_marks_atoms_and_schema_mismatch_are_lossless_or_rejected() {
        let definition = json!({"nodes":[
            {"name":"doc","content":"block+","role":"doc"},
            {"name":"paragraph","content":"inline*","group":"block","role":"textBlock","htmlTag":"p"},
            {"name":"text","role":"text","group":"inline"},
            {"name":"mention","role":"inline","group":"inline","isVoid":true,"attrs":{"id":{},"label":{"default":""}},"allowUndeclaredAttrs":true}
        ],"marks":[{"name":"annotation","htmlTag":"span","attrs":{"data":{"default":null}},"allowUndeclaredAttrs":true}]});
        let schema = Schema::from_json(&definition).unwrap();
        let source = from_prosemirror_json(&json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"secret"},{"type":"mention","attrs":{"id":"123","label":"Sam","metadata":{"kind":"person"}}},{"type":"text","text":"note","marks":[{"type":"annotation","attrs":{"data":{"id":7}}}]}]}]}), &schema, UnknownTypeMode::Error).unwrap();
        let copied = export(&source, &Selection::text(7, 12), &schema).unwrap();
        assert_eq!(copied["text"], "Samnote");
        assert!(!copied["fragment"].as_str().unwrap().contains("secret"));
        let decoded = decode(
            copied["fragment"].as_str().unwrap(),
            &schema,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(
            decoded
                .document
                .root()
                .child(0)
                .unwrap()
                .child(0)
                .unwrap()
                .attrs()["metadata"],
            json!({"kind":"person"})
        );
        assert_eq!(
            decoded
                .document
                .root()
                .child(0)
                .unwrap()
                .child(1)
                .unwrap()
                .marks()[0]
                .attrs()["data"],
            json!({"id":7})
        );
        assert!(decode(
            copied["fragment"].as_str().unwrap(),
            &tiptap_schema(),
            &ResourceLimits::default()
        )
        .is_none());
        let mut malformed: Value =
            serde_json::from_str(copied["fragment"].as_str().unwrap()).unwrap();
        malformed["version"] = json!(2);
        assert!(decode(&malformed.to_string(), &schema, &ResourceLimits::default()).is_none());
        malformed["version"] = json!(1);
        malformed["openStart"] = json!(99);
        assert!(decode(&malformed.to_string(), &schema, &ResourceLimits::default()).is_none());
        let mut limits = ResourceLimits::default();
        limits.max_input_bytes = 8;
        assert!(decode(copied["fragment"].as_str().unwrap(), &schema, &limits).is_none());
    }

    #[test]
    fn clipboard_node_selection_exports_full_container() {
        let schema = tiptap_schema();
        let document = from_prosemirror_json(&json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"selected"}]},{"type":"paragraph","content":[{"type":"text","text":"private"}]}]}), &schema, UnknownTypeMode::Error).unwrap();
        let data = export(&document, &Selection::node(0), &schema).unwrap();
        assert_eq!(data["text"], "selected");
        assert!(!data["fragment"].as_str().unwrap().contains("private"));
    }
    #[test]
    fn clipboard_root_metadata_is_not_copied_or_required_by_fragment() {
        let schema = Schema::from_json(&json!({"nodes":[
            {"name":"doc","content":"paragraph+","role":"doc","attrs":{"id":{"default":""},"locale":{"default":"en"}}},
            {"name":"paragraph","content":"text*","role":"textBlock","htmlTag":"p"},
            {"name":"text","role":"text"}
        ],"marks":[]})).unwrap();
        let document = from_prosemirror_json(&json!({"type":"doc","attrs":{"id":"private","locale":"fr"},"content":[{"type":"paragraph","content":[{"type":"text","text":"copy"}]}]}), &schema, UnknownTypeMode::Error).unwrap();
        let data = export(&document, &Selection::All, &schema).unwrap();
        assert!(!data["fragment"].as_str().unwrap().contains("private"));
        assert!(decode(
            data["fragment"].as_str().unwrap(),
            &schema,
            &ResourceLimits::default()
        )
        .is_some());
    }
    #[test]
    fn clipboard_same_schema_preserves_existing_opaque_content() {
        let schema = tiptap_schema();
        let source = from_prosemirror_json(&json!({"type":"doc","content":[{"type":"foreignCard","attrs":{"id":"semantic-id","label":"Card"},"content":[{"type":"text","text":"opaque text"}]}]}), &schema, UnknownTypeMode::Preserve).unwrap();
        let data = export(&source, &Selection::All, &schema).unwrap();
        let decoded = decode(
            data["fragment"].as_str().unwrap(),
            &schema,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.document, source);
    }
}
