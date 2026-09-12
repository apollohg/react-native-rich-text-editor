use crate::command_planner::SemanticOperation;
use crate::model::{Fragment, Node};
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{
    attrs_with_merged_span, attrs_with_unit_span, cell_holds_no_content, default_text_block_node,
    TableTarget, FIRST_WIDTH_SLICE, NODE_CLOSING_TOKENS, NODE_OPENING_TOKENS, ONE_SLOT,
};

const MERGE_SOURCE_MINIMUM: usize = 2;
const SPLITTABLE_CELLS: usize = 1;

pub(crate) fn plan_merge_cells(
    target: &TableTarget<'_>,
    schema: &Schema,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    if rect.cuts_a_span {
        return None;
    }
    let sources = target.cells_in_rectangle(rect.top, rect.left, rect.bottom, rect.right);
    if sources.len() < MERGE_SOURCE_MINIMUM {
        return None;
    }
    let (surviving, surviving_node) = target.cell_at(rect.top, rect.left)?;

    let mut operations = vec![SemanticOperation::UpdateNodeAttrs {
        pos: surviving.source_pos,
        attrs: attrs_with_merged_span(
            surviving_node,
            rect.right.checked_sub(rect.left)?,
            rect.bottom.checked_sub(rect.top)?,
        )?,
    }];
    let mut consumed = Vec::new();
    let mut carried: Vec<Node> = Vec::new();
    for (cell, node) in sources {
        if cell.source_pos == surviving.source_pos {
            continue;
        }
        if !cell_holds_no_content(node, schema) {
            carried.extend(node.content()?.children().iter().cloned());
        }
        consumed.push(SemanticOperation::ReplaceRange {
            from: cell.source_pos,
            to: cell.source_end,
            content: Fragment::empty(),
        });
    }

    if !carried.is_empty() {
        let content_end = surviving.source_end.checked_sub(NODE_CLOSING_TOKENS)?;
        let content_start = if cell_holds_no_content(surviving_node, schema) {
            surviving.source_pos.checked_add(NODE_OPENING_TOKENS)?
        } else {
            content_end
        };
        operations.push(SemanticOperation::ReplaceRange {
            from: content_start,
            to: content_end,
            content: Fragment::from(carried),
        });
    }
    operations.extend(consumed);
    operations.reverse();

    Some(TableActionOutcome {
        operations,
        selection_after: Selection::cell(surviving.source_pos, surviving.source_pos),
    })
}

pub(crate) fn plan_split_cell(
    target: &TableTarget<'_>,
    schema: &Schema,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    if target
        .cells_in_rectangle(rect.top, rect.left, rect.bottom, rect.right)
        .len()
        != SPLITTABLE_CELLS
    {
        return None;
    }
    let (cell, node) = target.cell_at(rect.top, rect.left)?;
    if cell.rect.colspan <= ONE_SLOT && cell.rect.rowspan <= ONE_SLOT {
        return None;
    }

    let slices = (FIRST_WIDTH_SLICE..cell.rect.colspan)
        .map(|offset| attrs_with_unit_span(node, offset))
        .collect::<Vec<_>>();
    let block = default_text_block_node(schema)?;
    let cell_type = node.node_type().to_owned();

    let mut operations = vec![SemanticOperation::UpdateNodeAttrs {
        pos: cell.source_pos,
        attrs: slices.first()?.clone(),
    }];
    for row in rect.top..rect.bottom {
        let position = if row == rect.top {
            cell.source_end
        } else {
            target.position_at(row, rect.left)?
        };
        let mut fresh = Vec::new();
        for (offset, attrs) in slices.iter().enumerate() {
            if row == rect.top && offset == FIRST_WIDTH_SLICE as usize {
                continue;
            }
            fresh.push(Node::element(
                cell_type.clone(),
                attrs.clone(),
                Fragment::from(vec![block.clone()]),
            ));
        }
        if fresh.is_empty() {
            continue;
        }
        operations.push(SemanticOperation::ReplaceRange {
            from: position,
            to: position,
            content: Fragment::from(fresh),
        });
    }
    operations.reverse();

    Some(TableActionOutcome {
        operations,
        selection_after: Selection::cell(cell.source_pos, cell.source_pos),
    })
}
