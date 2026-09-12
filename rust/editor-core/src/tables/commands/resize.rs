use crate::command_planner::SemanticOperation;
use crate::model::Node;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{attrs_with_column_width, TableTarget, FIRST_ROW, ONE_SLOT};
use crate::tables::projection::column_width;

struct CoveringCell<'a> {
    source_pos: u32,
    node: &'a Node,
    slice: u32,
}

fn covering_cells<'a>(target: &TableTarget<'a>) -> Option<Vec<CoveringCell<'a>>> {
    let rect = target.rect()?;
    let column = rect.right.checked_sub(ONE_SLOT)?;
    let mut cells = Vec::new();
    let mut row = FIRST_ROW;
    while row < target.rows() {
        let (cell, node) = target.cell_at(row, column)?;
        let slice = column.checked_sub(cell.rect.column)?;
        if column_width(node, slice).is_err() {
            return None;
        }
        cells.push(CoveringCell {
            source_pos: cell.source_pos,
            node,
            slice,
        });
        row = row.checked_add(cell.rect.rowspan)?;
    }
    (!cells.is_empty()).then_some(cells)
}

pub(crate) fn can_set_column_width(target: &TableTarget<'_>) -> bool {
    covering_cells(target).is_some()
}

pub(crate) fn plan_set_column_width(
    target: &TableTarget<'_>,
    width: u32,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    let selection_after =
        target.cell_selection_over(rect.top, rect.left, rect.bottom, rect.right)?;

    let mut operations = Vec::new();
    for cell in covering_cells(target)? {
        let Ok(declared) = column_width(cell.node, cell.slice) else {
            return None;
        };
        if declared == width {
            continue;
        }
        operations.push(SemanticOperation::UpdateNodeAttrs {
            pos: cell.source_pos,
            attrs: attrs_with_column_width(cell.node, cell.slice, width)?,
        });
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
