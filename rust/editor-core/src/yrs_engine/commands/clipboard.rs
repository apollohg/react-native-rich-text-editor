use super::{text::semantic_transaction, CommandPlan, PlanningContext, TypedCommand};
use crate::clipboard::{self, ClipboardSlice};
use crate::model::{Document, Fragment, Node};
use crate::selection::Selection;
use crate::tables::command_context::table_shape_operation_error;
use crate::tables::commands::cell_node;
use crate::tables::paste::{
    matrix_from_rows, matrix_from_slice, tab_separated_fields, TableMatrix,
};
use crate::tables::types::TableError;
use crate::tables::TableRoles;
use crate::yrs_engine::{OperationError, OperationResult};

const MAX_INPUT_BYTES_FIELD: &str = "maxInputBytes";
const PARAGRAPH_HTML_TAG: &str = "p";
const PARAGRAPH_NODE: &str = "paragraph";

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

fn usable_slice(
    slice: ClipboardSlice,
    filter: Option<&regex::Regex>,
    allow_base64_images: bool,
) -> Option<ClipboardSlice> {
    let had_payload = has_payload(slice.document.root());
    let filtered = filter_node(slice.document.root(), filter, allow_base64_images)?;
    let slice = ClipboardSlice {
        document: Document::new(filtered),
        ..slice
    };
    if slice.document.root().child_count() == 0 {
        return None;
    }
    if had_payload && !has_payload(slice.document.root()) {
        return None;
    }
    Some(slice)
}

fn filtered_text(
    context: &PlanningContext<'_>,
    text: Option<&str>,
    filter: Option<&regex::Regex>,
) -> OperationResult<Option<String>> {
    let Some(text) = text else {
        return Ok(None);
    };
    if text.len() > context.resource_limits.max_input_bytes {
        return Err(OperationError::document_limit_exceeded(
            context.request_id,
            None,
            MAX_INPUT_BYTES_FIELD,
            context.resource_limits.max_input_bytes as u64,
            text.len() as u64,
        ));
    }
    let text = text
        .chars()
        .filter(|c| filter.is_none_or(|filter| filter.is_match(&c.to_string())))
        .collect::<String>();
    Ok((!text.is_empty()).then_some(text))
}

fn text_blocks(context: &PlanningContext<'_>, text: &str) -> Option<Vec<Node>> {
    let paragraph = context
        .schema
        .node_by_html_tag(PARAGRAPH_HTML_TAG)
        .or_else(|| context.schema.node(PARAGRAPH_NODE))?;
    let attrs: std::collections::HashMap<String, serde_json::Value> = paragraph
        .attrs
        .iter()
        .filter_map(|(key, attr)| attr.default.clone().map(|value| (key.clone(), value)))
        .collect();
    Some(
        clipboard::normalized_line_breaks(text)
            .split(clipboard::LINE_BREAK)
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
            .collect(),
    )
}

fn pasted_matrix(
    context: &PlanningContext<'_>,
    selection: &Selection,
    slices: &[ClipboardSlice],
    text: Option<&str>,
    filter: Option<&regex::Regex>,
) -> OperationResult<Option<TableMatrix>> {
    let shaped = |error: TableError| table_shape_operation_error(error, context.request_id);
    let Some(roles) = TableRoles::resolve(context.schema).map_err(shaped)? else {
        return Ok(None);
    };
    let fills_cell_selection = match selection {
        Selection::Cell { .. } => true,
        Selection::Text { .. } | Selection::Node { .. } | Selection::All => false,
    };
    let single_cell = |blocks: Vec<Node>| -> OperationResult<Option<TableMatrix>> {
        let Some(cell) = cell_node(context.schema, &roles.cell, blocks) else {
            return Ok(None);
        };
        matrix_from_rows(
            vec![vec![cell]],
            &roles,
            context.schema,
            context.resource_limits,
        )
        .map_err(shaped)
    };
    if let Some(slice) = slices.first() {
        let Some(content) = slice.document.root().content() else {
            return Ok(None);
        };
        let matrix = matrix_from_slice(
            content,
            slice.open_start,
            slice.open_end,
            &roles,
            context.schema,
            context.resource_limits,
        )
        .map_err(shaped)?;
        if matrix.is_some() || !fills_cell_selection {
            return Ok(matrix);
        }
        return single_cell(content.children().to_vec());
    }
    let Some(text) = filtered_text(context, text, filter)? else {
        return Ok(None);
    };
    if let Some(fields) = tab_separated_fields(&text) {
        let mut rows = Vec::with_capacity(fields.len());
        for row in fields {
            let mut cells = Vec::with_capacity(row.len());
            for field in row {
                let Some(cell) = text_blocks(context, &field)
                    .and_then(|blocks| cell_node(context.schema, &roles.cell, blocks))
                else {
                    return Ok(None);
                };
                cells.push(cell);
            }
            rows.push(cells);
        }
        return matrix_from_rows(rows, &roles, context.schema, context.resource_limits)
            .map_err(shaped);
    }
    if !fills_cell_selection {
        return Ok(None);
    }
    let Some(blocks) = text_blocks(context, &text) else {
        return Ok(None);
    };
    single_cell(blocks)
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
        let Some((from, to)) = clipboard::selection_range(context.document, &selection) else {
            return Ok(CommandPlan::NotApplicable);
        };
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
                        .is_some_and(|spec| spec.html_tag.as_deref() == Some(PARAGRAPH_HTML_TAG))
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
    let slices: Vec<ClipboardSlice> = if plain_text {
        Vec::new()
    } else {
        representations
            .into_iter()
            .filter_map(|slice| usable_slice(slice, filter.as_ref(), allow_base64_images))
            .collect()
    };
    let pasted_text = text.or(fragment_text).or(derived_text);
    if let Some(anchor) = super::tables::outer_paste_anchor(&context, &selection)? {
        if let Some(matrix) = pasted_matrix(
            &context,
            &selection,
            &slices,
            pasted_text.as_deref(),
            filter.as_ref(),
        )? {
            match super::tables::paste_matrix(&context, &anchor, &selection, matrix)? {
                CommandPlan::NotApplicable => {}
                plan @ (CommandPlan::Transaction(_) | CommandPlan::SelectionOnly(_)) => {
                    return Ok(plan)
                }
            }
        }
    }
    for slice in slices {
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
    let Some(text) = filtered_text(&context, pasted_text.as_deref(), filter.as_ref())? else {
        return Ok(CommandPlan::NotApplicable);
    };
    if matches!(selection, Selection::Text { .. }) {
        return super::text::plan(context, TypedCommand::ReplaceSelectionText { text });
    }
    let Some(blocks) = text_blocks(&context, &text) else {
        return Ok(CommandPlan::NotApplicable);
    };
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
