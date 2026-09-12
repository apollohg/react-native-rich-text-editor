use crate::command_planner::SemanticOperation;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{attrs_with_column_width, TableTarget, FIRST_ROW, ONE_SLOT};
use crate::tables::projection::column_width;

pub(crate) fn plan_set_column_width(
    target: &TableTarget<'_>,
    width: u32,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    let column = rect.right.checked_sub(ONE_SLOT)?;
    let selection_after =
        target.cell_selection_over(rect.top, rect.left, rect.bottom, rect.right)?;

    let mut operations = Vec::new();
    let mut row = FIRST_ROW;
    while row < target.rows() {
        let (cell, node) = target.cell_at(row, column)?;
        let offset = column.checked_sub(cell.rect.column)?;
        let Ok(declared) = column_width(node, offset) else {
            return None;
        };
        if declared != width {
            operations.push(SemanticOperation::UpdateNodeAttrs {
                pos: cell.source_pos,
                attrs: attrs_with_column_width(node, offset, width)?,
            });
        }
        row = row.checked_add(cell.rect.rowspan)?;
    }
    if operations.is_empty() {
        return None;
    }
    operations.reverse();

    Some(TableActionOutcome {
        operations,
        selection_after,
    })
}
