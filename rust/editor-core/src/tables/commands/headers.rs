use crate::command_planner::SemanticOperation;
use crate::model::Fragment;
use crate::schema::Schema;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{
    retyped_cell, TableHeaderTarget, TableTarget, FIRST_COLUMN, FIRST_ROW,
};

pub(crate) fn plan_toggle_header(
    target: &TableTarget<'_>,
    header: TableHeaderTarget,
    schema: &Schema,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    let (top, left, bottom, right) = match header {
        TableHeaderTarget::Row => (rect.top, FIRST_COLUMN, rect.bottom, target.columns()),
        TableHeaderTarget::Column => (FIRST_ROW, rect.left, target.rows(), rect.right),
        TableHeaderTarget::Cell => (rect.top, rect.left, rect.bottom, rect.right),
    };

    let cells = target.cells_in_rectangle(top, left, bottom, right);
    if cells.is_empty() {
        return None;
    }
    let roles = target.roles();
    let holds_a_header = cells
        .iter()
        .any(|(_, node)| node.node_type() == roles.header_cell);

    let mut operations = Vec::new();
    for (cell, node) in cells {
        let retyped = if holds_a_header {
            if node.node_type() != roles.header_cell {
                continue;
            }
            &roles.cell
        } else {
            &roles.header_cell
        };
        operations.push(SemanticOperation::ReplaceRange {
            from: cell.source_pos,
            to: cell.source_end,
            content: Fragment::from(vec![retyped_cell(schema, node, retyped)?]),
        });
    }
    if operations.is_empty() {
        return None;
    }
    operations.reverse();
    Some(TableActionOutcome {
        operations,
        selection_after: target.cell_selection_over(
            rect.top,
            rect.left,
            rect.bottom,
            rect.right,
        )?,
    })
}
