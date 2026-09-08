use super::{text::semantic_transaction, CommandPlan, PlanningContext, TypedCommand};
use crate::clipboard::{self, ClipboardSlice};
use crate::model::{Document, Fragment, Node};
use crate::selection::Selection;
use crate::yrs_engine::{OperationError, OperationResult};

fn filter_node(node: &Node, filter: Option<&regex::Regex>, allow_base64: bool) -> Option<Node> {
    let safe_url = |value: &serde_json::Value| {
        let Some(url) = value.as_str() else {
            return true;
        };
        let compact: String = url
            .chars()
            .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
            .flat_map(char::to_lowercase)
            .collect();
        !compact.starts_with("javascript:")
            && !compact.starts_with("vbscript:")
            && (!compact.starts_with("data:")
                || (allow_base64 && compact.starts_with("data:image/")))
    };
    if node
        .attrs()
        .iter()
        .any(|(name, value)| matches!(name.as_str(), "src" | "href") && !safe_url(value))
    {
        return None;
    }
    if let Some(text) = node.text_str() {
        let text = text
            .chars()
            .filter(|c| filter.is_none_or(|filter| filter.is_match(&c.to_string())))
            .collect::<String>();
        let marks = node
            .marks()
            .iter()
            .filter(|mark| {
                !mark
                    .attrs()
                    .iter()
                    .any(|(name, value)| name == "href" && !safe_url(value))
            })
            .cloned()
            .collect();
        return (!text.is_empty()).then(|| Node::text(text, marks));
    }
    if node.is_void() {
        return Some(node.clone());
    }
    Some(Node::element(
        node.node_type().into(),
        node.attrs().clone(),
        Fragment::from(
            node.content()?
                .iter()
                .filter_map(|node| filter_node(node, filter, allow_base64))
                .collect(),
        ),
    ))
}

fn has_payload(root: &Node) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.is_void() || node.text_str().is_some_and(|text| !text.is_empty()) {
            return true;
        }
        if let Some(content) = node.content() {
            pending.extend(content.iter());
        }
    }
    false
}

pub(super) fn plan(
    context: PlanningContext<'_>,
    fragment: Option<String>,
    html: Option<String>,
    text: Option<String>,
    plain_text: bool,
    allow_base64_images: bool,
    input_filter: Option<String>,
) -> OperationResult<CommandPlan> {
    let filter = input_filter
        .map(|filter| regex::Regex::new(&filter))
        .transpose()
        .map_err(|error| {
            OperationError::operation_invalid(
                context.request_id,
                0,
                "inputFilter",
                error.to_string(),
            )
        })?;
    let selection = crate::yrs_engine::derived_state::resolved_to_legacy(context.selection);
    if plain_text && text.as_deref() == Some("") && fragment.is_none() && html.is_none() {
        let (from, to) = clipboard::selection_range(context.document, &selection);
        if from == to {
            return Ok(CommandPlan::NotApplicable);
        }
        let slice = ClipboardSlice {
            document: Document::new(Node::element(
                context.document.root().node_type().into(),
                Default::default(),
                Fragment::empty(),
            )),
            open_start: 0,
            open_end: 0,
        };
        let Some(plan) = clipboard::replacement(
            context.document,
            &selection,
            &slice,
            context.schema,
            context.resource_limits,
        ) else {
            return Ok(CommandPlan::NotApplicable);
        };
        return semantic_transaction(&context, &selection, plan);
    }
    let fragment_text = fragment
        .as_deref()
        .and_then(|source| clipboard::fragment_text(source, context.resource_limits));
    let mut representations = Vec::new();
    if let Some(fragment) = fragment {
        if let Some(slice) = clipboard::decode(&fragment, context.schema, context.resource_limits) {
            representations.push(slice);
        }
    }
    if let Some(html) = html {
        if let Ok(document) = crate::serialize::html_in::from_clipboard_html_with_limits(
            &html,
            context.schema,
            &crate::serialize::FromHtmlOptions {
                strict: false,
                allow_base64_images,
            },
            context.resource_limits,
        ) {
            // Ordinary paragraphs join the destination text at their open edges.
            let is_paragraph = |node: Option<&Node>| {
                node.is_some_and(|node| {
                    context
                        .schema
                        .node(node.node_type())
                        .is_some_and(|spec| spec.html_tag.as_deref() == Some("p"))
                })
            };
            let open_start = usize::from(is_paragraph(document.root().child(0)));
            let open_end = usize::from(is_paragraph(
                document
                    .root()
                    .child(document.root().child_count().saturating_sub(1)),
            ));
            if document.root().content_size() > 2
                || !clipboard::readable_text(&document, context.schema).is_empty()
                || document.root().child(0).is_some_and(Node::is_void)
            {
                representations.push(ClipboardSlice {
                    document,
                    open_start,
                    open_end,
                });
            }
        }
    }
    let derived_text = representations
        .first()
        .map(|slice| clipboard::readable_text(&slice.document, context.schema));
    if !plain_text {
        for slice in representations {
            let had_payload = has_payload(slice.document.root());
            let Some(filtered) =
                filter_node(slice.document.root(), filter.as_ref(), allow_base64_images)
            else {
                continue;
            };
            let slice = ClipboardSlice {
                document: Document::new(filtered),
                ..slice
            };
            if slice.document.root().child_count() == 0 {
                continue;
            }
            if had_payload && !has_payload(slice.document.root()) {
                continue;
            }
            let Some(plan) = clipboard::replacement(
                context.document,
                &selection,
                &slice,
                context.schema,
                context.resource_limits,
            ) else {
                continue;
            };
            if let Ok(transaction) = semantic_transaction(&context, &selection, plan) {
                return Ok(transaction);
            }
        }
    }
    let Some(text) = text.or(fragment_text).or(derived_text) else {
        return Ok(CommandPlan::NotApplicable);
    };
    if text.len() > context.resource_limits.max_input_bytes {
        return Err(OperationError::document_limit_exceeded(
            context.request_id,
            None,
            "maxInputBytes",
            context.resource_limits.max_input_bytes as u64,
            text.len() as u64,
        ));
    }
    let text = text
        .chars()
        .filter(|c| {
            filter
                .as_ref()
                .is_none_or(|filter| filter.is_match(&c.to_string()))
        })
        .collect::<String>();
    if text.is_empty() {
        return Ok(CommandPlan::NotApplicable);
    }
    if matches!(selection, Selection::Text { .. }) {
        return super::text::plan(context, TypedCommand::ReplaceSelectionText { text });
    }
    let paragraph = context
        .schema
        .node_by_html_tag("p")
        .or_else(|| context.schema.node("paragraph"));
    let Some(paragraph) = paragraph else {
        return Ok(CommandPlan::NotApplicable);
    };
    let attrs: std::collections::HashMap<String, serde_json::Value> = paragraph
        .attrs
        .iter()
        .filter_map(|(key, attr)| attr.default.clone().map(|value| (key.clone(), value)))
        .collect();
    let blocks = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|part| {
            Node::element(
                paragraph.name.clone(),
                attrs.clone(),
                Fragment::from(if part.is_empty() {
                    vec![]
                } else {
                    vec![Node::text(part.into(), vec![])]
                }),
            )
        })
        .collect();
    let slice = ClipboardSlice {
        document: Document::new(Node::element(
            context.document.root().node_type().into(),
            Default::default(),
            Fragment::from(blocks),
        )),
        open_start: 0,
        open_end: 0,
    };
    let Some(plan) = clipboard::replacement(
        context.document,
        &selection,
        &slice,
        context.schema,
        context.resource_limits,
    ) else {
        return Ok(CommandPlan::NotApplicable);
    };
    semantic_transaction(&context, &selection, plan)
}
