use crate::boundary::ResourceLimits;
use crate::model::{Document, Fragment, Node};
use crate::position::PositionMap;
use crate::schema::{NodeRole, Schema};
use crate::selection::Selection;
use crate::transform::Transaction;

use super::{SemanticCommandHistory, SemanticCommandPlan, SemanticOperation};

const NODE_BOUNDARY_TOKEN: u32 = 1;

fn insertion_marks(
    document: &Document,
    schema: &Schema,
    position: u32,
    stored_marks: Option<&[crate::model::Mark]>,
    end: u32,
) -> Vec<crate::model::Mark> {
    let marks = if position == end {
        stored_marks
            .map(<[_]>::to_vec)
            .unwrap_or_else(|| crate::editor_state::marks_at_position(document, schema, position))
    } else {
        crate::editor_state::marks_across_range(document, schema, position, end)
    };
    super::canonical_marks(&marks, schema)
}

pub(crate) fn plan_insert_text(
    document: &Document,
    schema: &Schema,
    selection: &Selection,
    stored_marks: Option<&[crate::model::Mark]>,
    text: &str,
) -> Option<SemanticCommandPlan> {
    let Selection::Text { anchor, head } = selection else {
        return None;
    };
    if anchor != head || text.is_empty() {
        return None;
    }
    if is_block_void_at_position(document, schema, *anchor) {
        return None;
    }
    let marks = insertion_marks(document, schema, *anchor, stored_marks, *anchor);
    Some(SemanticCommandPlan::one(SemanticOperation::InsertText {
        pos: *anchor,
        text: text.to_string(),
        marks,
    }))
}

#[derive(Debug)]
pub(crate) struct TextReplacementOperationLimit {
    pub limit: usize,
    pub actual: usize,
}

enum TextReplacementPlanError {
    OperationLimit(TextReplacementOperationLimit),
    Inapplicable,
}

fn multiline_replacement(
    document: &Document,
    schema: &Schema,
    position: u32,
    marks: &[crate::model::Mark],
    text: &str,
    limits: &ResourceLimits,
) -> Result<Option<SemanticCommandPlan>, TextReplacementPlanError> {
    if text.split(crate::clipboard::LINE_BREAK).count() > limits.max_document_nodes {
        return Ok(None);
    }
    let parts: Vec<&str> = text.split(crate::clipboard::LINE_BREAK).collect();
    let resolved = document
        .resolve(position)
        .map_err(|_| TextReplacementPlanError::Inapplicable)?;
    let block = resolved.parent(document);
    if !schema
        .node(block.node_type())
        .is_some_and(|spec| matches!(spec.role, crate::schema::NodeRole::TextBlock))
    {
        return Ok(None);
    }
    let Some(&block_index) = resolved.node_path.last() else {
        return Ok(None);
    };
    let parent = document
        .node_at(&resolved.node_path[..resolved.node_path.len() - 1])
        .ok_or(TextReplacementPlanError::Inapplicable)?;
    let parent_spec = schema
        .node(parent.node_type())
        .ok_or(TextReplacementPlanError::Inapplicable)?;
    let siblings = parent
        .content()
        .ok_or(TextReplacementPlanError::Inapplicable)?;
    let index = block_index as usize;
    let prefix = siblings.iter().take(index + 1).collect::<Vec<_>>();
    let suffix = siblings.iter().skip(index + 1).collect::<Vec<_>>();
    let budget = crate::schema::content_rule::WorkBudget::new(
        crate::transform::apply::document_validation_work_limit(limits),
    );
    let candidates =
        super::preferred_text_blocks_for_parent(schema, parent_spec, &prefix, &suffix, &budget)
            .map_err(|()| TextReplacementPlanError::Inapplicable)?;
    for name in candidates {
        let Some(attrs) = super::default_attrs(schema, &name) else {
            continue;
        };
        let blocks = parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let content = Fragment::from(if part.is_empty() {
                    Vec::new()
                } else {
                    vec![Node::text((*part).to_owned(), marks.to_vec())]
                });
                if index == 0 {
                    Node::element(block.node_type().into(), block.attrs().clone(), content)
                } else {
                    Node::element(name.clone(), attrs.clone(), content)
                }
            })
            .collect();
        let slice = crate::clipboard::ClipboardSlice {
            document: Document::new(Node::element(
                document.root().node_type().into(),
                document.root().attrs().clone(),
                Fragment::from(blocks),
            )),
            open_start: crate::clipboard::OPEN_TEXTBLOCK_DEPTH,
            open_end: crate::clipboard::OPEN_TEXTBLOCK_DEPTH,
        };
        if let Some(plan) = crate::clipboard::replacement(
            document,
            &Selection::cursor(position),
            &slice,
            schema,
            limits,
        ) {
            return Ok(Some(plan));
        }
    }
    Ok(None)
}

pub(crate) fn plan_replace_selection_text(
    document: &Document,
    schema: &Schema,
    selection: &Selection,
    stored_marks: Option<&[crate::model::Mark]>,
    text: &str,
    limits: &ResourceLimits,
    max_operations: usize,
) -> Result<Option<SemanticCommandPlan>, TextReplacementOperationLimit> {
    let Selection::Text { anchor, head } = selection else {
        return Ok(None);
    };
    let from = (*anchor).min(*head);
    let to = (*anchor).max(*head);
    if from < to
        && crate::tables::mutation_guard::range_crosses_cell_boundary(document, schema, from, to)
    {
        return plan_replacement_after_deletion(
            document,
            schema,
            from,
            to,
            limits,
            |preview, cursor| {
                plan_replace_selection_text(
                    preview,
                    schema,
                    cursor,
                    None,
                    text,
                    limits,
                    max_operations,
                )
            },
        );
    }
    let is_code = document.resolve(from).ok().is_some_and(|resolved| {
        Some(resolved.parent(document).node_type()) == super::code_block_node_name(schema)
    });
    let normalized;
    let text = if !is_code && text.contains('\r') {
        normalized = crate::clipboard::normalized_line_breaks(text);
        normalized.as_str()
    } else {
        text
    };
    let multiline = !is_code && text.contains('\n');
    let marks = insertion_marks(document, schema, from, stored_marks, to);
    match plan_textblock_text_replacement(
        document,
        schema,
        from,
        to,
        text,
        multiline,
        &marks,
        limits,
        max_operations,
    ) {
        Ok(plan) => Ok(plan),
        Err(TextReplacementPlanError::OperationLimit(limit)) => Err(limit),
        Err(TextReplacementPlanError::Inapplicable) if from == to => Ok(None),
        Err(TextReplacementPlanError::Inapplicable) => {
            let lines = if multiline {
                text.split(crate::clipboard::LINE_BREAK).collect()
            } else {
                vec![text]
            };
            Ok(root_text_replacement(
                document, schema, from, to, &lines, &marks, limits,
            ))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_textblock_text_replacement(
    document: &Document,
    schema: &Schema,
    from: u32,
    to: u32,
    text: &str,
    multiline: bool,
    marks: &[crate::model::Mark],
    limits: &ResourceLimits,
    max_operations: usize,
) -> Result<Option<SemanticCommandPlan>, TextReplacementPlanError> {
    let count = usize::from(from < to)
        + if multiline {
            text.split('\n').filter(|part| !part.is_empty()).count()
                + text.bytes().filter(|byte| *byte == b'\n').count()
        } else {
            usize::from(!text.is_empty())
        };
    if multiline {
        let mut operations = Vec::new();
        if from < to {
            operations.push(SemanticOperation::DeleteRange { from, to });
        }
        let preview = apply_operations(document, schema, &operations)
            .map_err(|()| TextReplacementPlanError::Inapplicable)?;
        if let Some(mut plan) = multiline_replacement(&preview, schema, from, marks, text, limits)?
        {
            operations.append(&mut plan.operations);
            if operations.len() > max_operations {
                return Err(TextReplacementPlanError::OperationLimit(
                    TextReplacementOperationLimit {
                        limit: max_operations,
                        actual: operations.len(),
                    },
                ));
            }
            plan.operations = operations;
            return Ok(Some(plan));
        }
    }
    let limit = max_operations;
    if count > limit {
        return Err(TextReplacementPlanError::OperationLimit(
            TextReplacementOperationLimit {
                limit,
                actual: count,
            },
        ));
    }
    let mut operations = Vec::with_capacity(count);
    if from < to {
        operations.push(SemanticOperation::DeleteRange { from, to });
    }
    if !multiline {
        if !text.is_empty() {
            operations.push(SemanticOperation::InsertText {
                pos: from,
                text: text.to_string(),
                marks: marks.to_vec(),
            });
        }
        if !within_one_textblock(document, schema, from, to)
            && !produces_valid_document(document, schema, &operations, limits)
        {
            return Err(TextReplacementPlanError::Inapplicable);
        }
        return Ok((!operations.is_empty()).then_some(SemanticCommandPlan {
            operations,
            selection_after: None,
            history: SemanticCommandHistory::InputBoundary,
        }));
    }
    let mut preview = apply_operations(document, schema, &operations)
        .map_err(|()| TextReplacementPlanError::Inapplicable)?;
    let mut cursor = from;
    for (index, part) in text.split('\n').enumerate() {
        if index > 0 {
            let split = super::preferred_split_operation(&preview, schema, cursor, limits)
                .map_err(|()| TextReplacementPlanError::Inapplicable)?
                .ok_or(TextReplacementPlanError::Inapplicable)?;
            preview = apply_operations(&preview, schema, std::slice::from_ref(&split))
                .map_err(|()| TextReplacementPlanError::Inapplicable)?;
            operations.push(split);
            cursor = cursor
                .checked_add(2)
                .ok_or(TextReplacementPlanError::Inapplicable)?;
        }
        if !part.is_empty() {
            let insert = SemanticOperation::InsertText {
                pos: cursor,
                text: part.to_string(),
                marks: marks.to_vec(),
            };
            preview = apply_operations(&preview, schema, std::slice::from_ref(&insert))
                .map_err(|()| TextReplacementPlanError::Inapplicable)?;
            operations.push(insert);
            cursor = cursor
                .checked_add(
                    u32::try_from(part.chars().count())
                        .map_err(|_| TextReplacementPlanError::Inapplicable)?,
                )
                .ok_or(TextReplacementPlanError::Inapplicable)?;
        }
    }
    Ok(Some(SemanticCommandPlan {
        operations,
        selection_after: Some(Selection::cursor(cursor)),
        history: SemanticCommandHistory::InputBoundary,
    }))
}

pub(super) fn inline_runs_wrapped_in_text_blocks(
    schema: &Schema,
    nodes: &[Node],
) -> Option<Vec<Node>> {
    let mut blocks = Vec::with_capacity(nodes.len());
    let mut run = Vec::new();
    for node in nodes {
        let is_inline = node.is_text()
            || schema.node(node.node_type()).is_some_and(|spec| {
                matches!(
                    spec.role,
                    NodeRole::Text | NodeRole::Inline | NodeRole::HardBreak
                )
            });
        if is_inline {
            run.push(node.clone());
            continue;
        }
        if !run.is_empty() {
            blocks.push(default_text_block_with_content(
                schema,
                Fragment::from(std::mem::take(&mut run)),
            )?);
        }
        blocks.push(node.clone());
    }
    if !run.is_empty() {
        blocks.push(default_text_block_with_content(
            schema,
            Fragment::from(run),
        )?);
    }
    Some(blocks)
}

pub(super) fn block_insertion_points(
    document: &Document,
    schema: &Schema,
    cursor: u32,
) -> Vec<u32> {
    let mut points = Vec::new();
    let mut point = crate::editor_state::block_insert_position(document, schema, cursor);
    while let Some(position) = point {
        points.push(position);
        point = document.resolve(position).ok().and_then(|resolved| {
            let parent = resolved.parent(document);
            let before_parent = node_delete_start(document, &resolved.node_path)?;
            if resolved.node_path.is_empty() {
                None
            } else if resolved.parent_offset == 0 {
                Some(before_parent)
            } else if resolved.parent_offset == parent.content_size() {
                before_parent.checked_add(parent.node_size())
            } else {
                None
            }
        });
    }
    points
}

pub(super) fn produces_valid_document(
    document: &Document,
    schema: &Schema,
    operations: &[SemanticOperation],
    limits: &ResourceLimits,
) -> bool {
    valid_candidate(document, schema, operations, limits).is_some()
}

fn valid_candidate(
    document: &Document,
    schema: &Schema,
    operations: &[SemanticOperation],
    limits: &ResourceLimits,
) -> Option<Document> {
    apply_operations(document, schema, operations)
        .ok()
        .filter(|candidate| {
            crate::transform::DocumentValidator::validate(candidate, schema, limits).is_ok()
        })
}

fn covering_container_boundary(
    document: &Document,
    schema: &Schema,
    position: u32,
    at_end: bool,
) -> Option<u32> {
    let resolved = document.resolve(position).ok()?;
    let parent = resolved.parent(document);
    if resolved.node_path.is_empty() || schema.is_text_block(parent.node_type()) {
        return None;
    }
    if at_end && resolved.parent_offset == parent.content_size() {
        position.checked_add(NODE_BOUNDARY_TOKEN)
    } else if !at_end && resolved.parent_offset == 0 {
        position.checked_sub(NODE_BOUNDARY_TOKEN)
    } else {
        None
    }
}

fn plan_covering_deletion(
    document: &Document,
    schema: &Schema,
    from: u32,
    to: u32,
    limits: &ResourceLimits,
) -> Option<SemanticCommandPlan> {
    let (mut from, mut to) = (from, to);
    loop {
        let start = covering_container_boundary(document, schema, from, false);
        let end = covering_container_boundary(document, schema, to, true);
        if start.is_none() && end.is_none() {
            return None;
        }
        let (next_from, next_to) = (start.unwrap_or(from), end.unwrap_or(to));
        if crate::tables::mutation_guard::range_crosses_cell_boundary(
            document, schema, next_from, next_to,
        ) {
            return None;
        }
        (from, to) = (next_from, next_to);
        let delete = SemanticOperation::DeleteRange { from, to };
        if let Some(candidate) =
            valid_candidate(document, schema, std::slice::from_ref(&delete), limits)
        {
            return (candidate != *document).then(|| SemanticCommandPlan::one(delete));
        }
    }
}

pub(super) fn within_one_textblock(
    document: &Document,
    schema: &Schema,
    from: u32,
    to: u32,
) -> bool {
    let (Ok(start), Ok(end)) = (document.resolve(from), document.resolve(to)) else {
        return false;
    };
    start.node_path == end.node_path && schema.is_text_block(start.parent(document).node_type())
}

fn root_text_replacement(
    document: &Document,
    schema: &Schema,
    from: u32,
    to: u32,
    lines: &[&str],
    marks: &[crate::model::Mark],
    limits: &ResourceLimits,
) -> Option<SemanticCommandPlan> {
    if crate::tables::mutation_guard::range_crosses_cell_boundary(document, schema, from, to) {
        return None;
    }
    let blocks = lines
        .iter()
        .map(|line| {
            default_text_block_with_content(
                schema,
                Fragment::from(if line.is_empty() {
                    Vec::new()
                } else {
                    vec![Node::text((*line).to_owned(), marks.to_vec())]
                }),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let open_depth = if blocks.is_empty() {
        0
    } else {
        crate::clipboard::OPEN_TEXTBLOCK_DEPTH
    };
    let slice = crate::clipboard::ClipboardSlice {
        document: Document::new(Node::element(
            document.root().node_type().into(),
            document.root().attrs().clone(),
            Fragment::from(blocks),
        )),
        open_start: open_depth,
        open_end: open_depth,
    };
    let plan = crate::clipboard::replacement(
        document,
        &Selection::text(from, to),
        &slice,
        schema,
        limits,
    )?;
    let unchanged = matches!(
        plan.operations.as_slice(),
        [SemanticOperation::ReplaceRange { from, to, content }] if from == to && content.size() == 0
    );
    (!unchanged).then_some(plan)
}

fn plan_range_deletion(
    document: &Document,
    schema: &Schema,
    from: u32,
    to: u32,
    limits: &ResourceLimits,
) -> Option<SemanticCommandPlan> {
    let start = document.resolve(from).ok()?;
    let end = document.resolve(to).ok()?;
    let delete = SemanticOperation::DeleteRange { from, to };
    if start.node_path == end.node_path {
        let parent = start.parent(document);
        if schema.is_text_block(parent.node_type()) {
            return Some(SemanticCommandPlan::one(delete));
        }
        let mut offset = 0u32;
        let (mut before, mut after) = (Vec::new(), Vec::new());
        for child in parent.content()?.iter() {
            if offset < start.parent_offset {
                before.push(child.node_type());
            } else if offset >= end.parent_offset {
                after.push(child.node_type());
            }
            offset += child.node_size();
        }
        let rule = &schema.node(parent.node_type())?.content;
        let accepts = |children: &[&str]| {
            rule.matches(children, |child, symbol| {
                schema.node_matches_symbol(child, symbol)
            })
        };
        let filler = default_text_block(schema)?;
        let remaining = [before.as_slice(), after.as_slice()].concat();
        let filled = [before.as_slice(), &[filler.node_type()], after.as_slice()].concat();
        return Some(SemanticCommandPlan::one(
            if accepts(&remaining) || !accepts(&filled) {
                delete
            } else {
                SemanticOperation::ReplaceRange {
                    from,
                    to,
                    content: Fragment::from(vec![filler]),
                }
            },
        ));
    }
    if let Some(candidate) =
        valid_candidate(document, schema, std::slice::from_ref(&delete), limits)
    {
        return (candidate != *document).then(|| SemanticCommandPlan::one(delete));
    }
    root_text_replacement(document, schema, from, to, &[], &[], limits)
        .or_else(|| plan_covering_deletion(document, schema, from, to, limits))
}

pub(crate) fn plan_replacement_after_deletion<E>(
    document: &Document,
    schema: &Schema,
    from: u32,
    to: u32,
    limits: &ResourceLimits,
    insertion: impl FnOnce(&Document, &Selection) -> Result<Option<SemanticCommandPlan>, E>,
) -> Result<Option<SemanticCommandPlan>, E> {
    let mut preview = document.clone();
    let mut operations = Vec::new();
    for (start, end) in
        crate::tables::mutation_guard::cell_region_pieces(document, schema, from, to)
            .into_iter()
            .rev()
    {
        let Some(deletion) = plan_range_deletion(&preview, schema, start, end, limits) else {
            continue;
        };
        let Ok(next) = apply_operations(&preview, schema, &deletion.operations) else {
            return Ok(None);
        };
        preview = next;
        operations.extend(deletion.operations);
    }
    let cursor = Selection::cursor(from);
    let Some(inserted) = insertion(&preview, &cursor)? else {
        return Ok((!operations.is_empty()).then(|| SemanticCommandPlan {
            operations,
            selection_after: Some(cursor),
            history: SemanticCommandHistory::InputBoundary,
        }));
    };
    let selection_after = match inserted.selection_after {
        Some(selection) => selection,
        None => {
            let Ok((_, step_map)) = apply_operations_mapped(&preview, schema, &inserted.operations)
            else {
                return Ok(None);
            };
            cursor.map(&step_map)
        }
    };
    operations.extend(inserted.operations);
    Ok(Some(SemanticCommandPlan {
        operations,
        selection_after: Some(selection_after),
        history: inserted.history,
    }))
}

pub(crate) fn plan_insert_content(
    document: &Document,
    schema: &Schema,
    selection: &Selection,
    content: &Fragment,
    limits: &ResourceLimits,
) -> Option<SemanticCommandPlan> {
    let insertion_at = |document: &Document, selection: &Selection| {
        let content = match selection.from(document) {
            Some(position)
                if crate::tables::mutation_guard::position_is_inside_cell(
                    document, schema, position,
                ) =>
            {
                Fragment::from(crate::tables::mutation_guard::tables_flattened_into_blocks(
                    content.children(),
                    schema,
                ))
            }
            _ => content.clone(),
        };
        let planned = |selection: &Selection, content: &Fragment| {
            crate::editor_state::plan_content_insertion(document, schema, selection, content).map(
                |replacement| SemanticCommandPlan {
                    operations: vec![SemanticOperation::ReplaceRange {
                        from: replacement.from,
                        to: replacement.to,
                        content: replacement.content,
                    }],
                    selection_after: Some(replacement.selection_after),
                    history: SemanticCommandHistory::InputBoundary,
                },
            )
        };
        let valid = |plan: &SemanticCommandPlan| {
            produces_valid_document(document, schema, &plan.operations, limits)
        };
        if let Some(plan) = planned(selection, &content).filter(valid) {
            return Some(plan);
        }
        let blocks = Fragment::from(inline_runs_wrapped_in_text_blocks(
            schema,
            content.children(),
        )?);
        block_insertion_points(document, schema, selection.from(document)?)
            .into_iter()
            .filter_map(|point| planned(&Selection::cursor(point), &blocks))
            .find(valid)
    };
    let (from, to) = (selection.from(document)?, selection.to(document)?);
    if from == to {
        return insertion_at(document, selection);
    }
    let replaces_selection = |plan: &SemanticCommandPlan| match plan.operations.as_slice() {
        [SemanticOperation::ReplaceRange {
            from: replace_from,
            to: replace_to,
            ..
        }] => *replace_from == from && *replace_to == to,
        _ => false,
    };
    if !crate::tables::mutation_guard::range_crosses_cell_boundary(document, schema, from, to) {
        if let Some(plan) = insertion_at(document, selection).filter(replaces_selection) {
            return Some(plan);
        }
    }
    let Ok(plan) =
        plan_replacement_after_deletion(document, schema, from, to, limits, |preview, cursor| {
            Ok::<_, std::convert::Infallible>(insertion_at(preview, cursor))
        });
    plan
}

fn default_text_block_with_content(schema: &Schema, content: Fragment) -> Option<Node> {
    let spec = schema.preferred_text_block()?;
    let attrs = spec
        .attrs
        .iter()
        .filter_map(|(name, attr)| attr.default.clone().map(|value| (name.clone(), value)))
        .collect();
    Some(Node::element(spec.name.clone(), attrs, content))
}

pub(super) fn default_text_block(schema: &Schema) -> Option<Node> {
    default_text_block_with_content(schema, Fragment::empty())
}

fn is_block_void_at_position(document: &Document, schema: &Schema, position: u32) -> bool {
    let Ok(resolved) = document.resolve(position) else {
        return false;
    };
    let Some(content) = resolved.parent(document).content() else {
        return false;
    };
    let mut offset = 0;
    for child in content.iter() {
        if offset == resolved.parent_offset {
            return child.is_void()
                && schema
                    .node(child.node_type())
                    .is_some_and(|spec| matches!(spec.role, crate::schema::NodeRole::Block));
        }
        let Some(next) = offset.checked_add(child.node_size()) else {
            return false;
        };
        offset = next;
    }
    false
}

pub(super) fn node_delete_start(document: &Document, path: &[u32]) -> Option<u32> {
    let mut node = document.root();
    let mut open = 0u32;
    for index in path.iter().copied() {
        let content = node.content()?;
        let mut child_open = open.checked_add(1)?;
        for sibling in content.iter().take(index as usize) {
            child_open = child_open.checked_add(sibling.node_size())?;
        }
        node = content.child(index as usize)?;
        open = child_open;
    }
    open.checked_sub(1)
}

pub(crate) fn plan_delete_backward(
    document: &Document,
    position_map: &PositionMap,
    schema: &Schema,
    selection: &Selection,
    limits: &ResourceLimits,
) -> Result<Option<SemanticCommandPlan>, ()> {
    let Selection::Text { anchor, head } = selection else {
        return Ok(None);
    };
    let scalar_anchor = position_map.doc_to_scalar(*anchor, document);
    let scalar_head = position_map.doc_to_scalar(*head, document);
    let from = scalar_anchor.min(scalar_head);
    let to = scalar_anchor.max(scalar_head);
    if from < to {
        return plan_delete_scalar_range_impl(
            document,
            position_map,
            schema,
            from,
            to,
            true,
            false,
            limits,
        );
    }
    let Some(cursor) = selection.from(document) else {
        return Ok(None);
    };
    if let Some(plan) = super::plan_empty_split_action(document, schema, cursor) {
        return Ok(Some(plan));
    }
    if let Some(plan) = super::plan_empty_non_default_text_block(document, schema, cursor, limits)?
    {
        return Ok(Some(plan));
    }
    // Must precede the `to == 0` bail: a quote at the very start of the
    // document leaves the caret there, and giving up would make the keystroke
    // do nothing at all with no way out of the quote.
    if let Some(plan) = super::plan_blockquote_lift_at_start(document, schema, cursor) {
        return Ok(Some(plan));
    }
    if to == 0 {
        return Ok(None);
    }
    plan_delete_scalar_range_impl(
        document,
        position_map,
        schema,
        to - 1,
        to,
        true,
        true,
        limits,
    )
}

pub(crate) fn plan_delete_scalar_range(
    document: &Document,
    position_map: &PositionMap,
    schema: &Schema,
    scalar_from: u32,
    scalar_to: u32,
    limits: &ResourceLimits,
) -> Result<Option<SemanticCommandPlan>, ()> {
    plan_delete_scalar_range_impl(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        false,
        false,
        limits,
    )
}

#[allow(clippy::too_many_arguments)]
fn plan_delete_scalar_range_impl(
    document: &Document,
    position_map: &PositionMap,
    schema: &Schema,
    scalar_from: u32,
    scalar_to: u32,
    is_backward_delete: bool,
    is_collapsed_backward_delete: bool,
    limits: &ResourceLimits,
) -> Result<Option<SemanticCommandPlan>, ()> {
    let doc_from = position_map.scalar_to_doc(scalar_from, document);
    let doc_to = position_map.scalar_to_doc(scalar_to, document);
    if doc_from < doc_to
        && crate::tables::mutation_guard::range_crosses_cell_boundary(
            document, schema, doc_from, doc_to,
        )
    {
        return plan_replacement_after_deletion(
            document,
            schema,
            doc_from,
            doc_to,
            limits,
            |_, _| Ok(None),
        );
    }
    if scalar_from < scalar_to {
        if let Some(plan) = super::clear_selected_block_content(document, schema, doc_from, doc_to)
        {
            return Ok(Some(plan));
        }
    }
    if let Some(pos) = super::empty_list_unwrap_pos(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_from,
        doc_to,
    ) {
        return Ok(Some(SemanticCommandPlan::one(
            SemanticOperation::UnwrapFromList { pos },
        )));
    }
    if scalar_from < scalar_to
        && doc_from == doc_to
        && position_map.doc_to_scalar(doc_to, document) == scalar_to
    {
        if let Some(plan) = super::plan_empty_blockquote_exit(document, schema, doc_to) {
            return Ok(Some(plan));
        }
    }
    if let Some(plan) = super::marker_backspace_action(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_from,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    if let Some(plan) = super::lift_trailing_empty_list_block(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_from,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    let empty_block_plan = super::empty_block_delete_action(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_from,
        doc_to,
        is_collapsed_backward_delete,
    );
    if is_collapsed_backward_delete && empty_block_plan.is_some() {
        return Ok(empty_block_plan);
    }
    if let Some(plan) = super::replace_void_and_empty_block(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    if let Some(plan) = super::replace_only_void_block(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_from,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    if let Some(plan) = empty_block_plan {
        return Ok(Some(plan));
    }
    if let Some(plan) = super::merge_into_previous_list_action(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    if let Some(plan) = super::move_into_previous_blockquote_action(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    if let Some(plan) = super::delete_selection_through_previous_void_block_action(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_from,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    if let Some(plan) = super::delete_previous_void_block_action(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    if is_backward_delete
        && super::previous_void_block_at_text_head(
            document,
            position_map,
            schema,
            scalar_from,
            scalar_to,
            doc_to,
        )
        .is_some()
    {
        return Ok(None);
    }
    // Must precede the DeleteRange fallback: at the head of a non-empty text
    // block that range would straddle the block boundary, which the engine
    // rejects as a cross-parent structural deletion.
    if let Some(plan) = super::join_with_previous_block_action(
        document,
        position_map,
        schema,
        scalar_from,
        scalar_to,
        doc_to,
    ) {
        return Ok(Some(plan));
    }
    Ok(plan_range_deletion(
        document, schema, doc_from, doc_to, limits,
    ))
}

pub(crate) fn plan_split(
    document: &Document,
    position_map: &PositionMap,
    schema: &Schema,
    selection: &Selection,
    delete_selection: bool,
    limits: &ResourceLimits,
) -> Result<Option<SemanticCommandPlan>, ()> {
    let Selection::Text { anchor, head } = selection else {
        return Ok(None);
    };
    let scalar_anchor = position_map.doc_to_scalar(*anchor, document);
    let scalar_head = position_map.doc_to_scalar(*head, document);
    let scalar_from = scalar_anchor.min(scalar_head);
    let scalar_to = scalar_anchor.max(scalar_head);
    let doc_from = position_map.scalar_to_doc(scalar_from, document);
    let doc_to = position_map.scalar_to_doc(scalar_to, document);

    if doc_from == doc_to && is_block_void_at_position(document, schema, doc_from) {
        return Ok(None);
    }

    if doc_from == doc_to {
        if let Some(plan) = plan_code_split(document, schema, doc_from, limits)? {
            return Ok(Some(plan));
        }
    }
    if delete_selection && doc_from < doc_to {
        return plan_replacement_after_deletion(
            document,
            schema,
            doc_from,
            doc_to,
            limits,
            |preview, cursor| {
                plan_split(
                    preview,
                    &PositionMap::build(preview, schema),
                    schema,
                    cursor,
                    false,
                    limits,
                )
            },
        );
    }

    if let Some(action) = super::plan_empty_split_action(document, schema, doc_from) {
        return Ok(Some(action));
    }
    let Some(split) = super::preferred_split_operation(document, schema, doc_from, limits)? else {
        return Ok(None);
    };
    Ok(Some(SemanticCommandPlan {
        operations: vec![split],
        selection_after: None,
        history: SemanticCommandHistory::InputBoundary,
    }))
}

fn plan_code_split(
    document: &Document,
    schema: &Schema,
    position: u32,
    limits: &ResourceLimits,
) -> Result<Option<SemanticCommandPlan>, ()> {
    let resolved = match document.resolve(position) {
        Ok(resolved) => resolved,
        Err(_) => return Ok(None),
    };
    let block = resolved.parent(document);
    if Some(block.node_type()) != super::code_block_node_name(schema) {
        return Ok(None);
    }
    if resolved.parent_offset == block.content_size() && block.text_content().ends_with('\n') {
        let delete = SemanticOperation::DeleteRange {
            from: position.checked_sub(1).ok_or(())?,
            to: position,
        };
        let preview = apply_operations(document, schema, std::slice::from_ref(&delete))?;
        let Some(split) = super::preferred_split_operation(&preview, schema, position - 1, limits)?
        else {
            return Err(());
        };
        let candidate = apply_operations(&preview, schema, std::slice::from_ref(&split))?;
        let block_path = resolved.node_path.as_slice();
        let &block_index = block_path.last().ok_or(())?;
        let parent_path = &block_path[..block_path.len() - 1];
        let parent = candidate.node_at(parent_path).ok_or(())?;
        let index = usize::try_from(block_index).map_err(|_| ())?;
        let left = parent.child(index).ok_or(())?.clone();
        let right = parent
            .child(index.checked_add(1).ok_or(())?)
            .ok_or(())?
            .clone();
        let from = node_delete_start(document, block_path).ok_or(())?;
        let cursor = from
            .checked_add(left.node_size())
            .and_then(|value| value.checked_add(1))
            .ok_or(())?;
        return Ok(Some(SemanticCommandPlan {
            operations: vec![SemanticOperation::ReplaceRange {
                from,
                to: from.checked_add(block.node_size()).ok_or(())?,
                content: Fragment::from(vec![left, right]),
            }],
            selection_after: Some(Selection::cursor(cursor)),
            history: SemanticCommandHistory::InputBoundary,
        }));
    }
    Ok(Some(SemanticCommandPlan::one(
        SemanticOperation::InsertText {
            pos: position,
            text: "\n".into(),
            marks: Vec::new(),
        },
    )))
}

pub(crate) fn apply_operations(
    document: &Document,
    schema: &Schema,
    operations: &[SemanticOperation],
) -> Result<Document, ()> {
    apply_operations_mapped(document, schema, operations).map(|(document, _)| document)
}

pub(crate) fn apply_operations_mapped(
    document: &Document,
    schema: &Schema,
    operations: &[SemanticOperation],
) -> Result<(Document, crate::transform::StepMap), ()> {
    let mut transaction = Transaction::new();
    for operation in operations {
        transaction.add_step(operation.as_step());
    }
    transaction
        .apply_steps_unchecked(document, schema)
        .map_err(|_| ())
}
