use crate::command_planner::SemanticOperation;
use crate::model::Node;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{attrs_with_column_width, TableTarget, FIRST_ROW, ONE_SLOT};
use crate::tables::projection::column_width;
use crate::tables::types::TableError;

struct CoveringCell<'a> {
    source_pos: u32,
    node: &'a Node,
    slice: u32,
    declared: u32,
}

fn covering_cells<'a>(
    target: &TableTarget<'a>,
) -> Result<Option<Vec<CoveringCell<'a>>>, TableError> {
    let Some(rect) = target.rect() else {
        return Ok(None);
    };
    let Some(column) = rect.right.checked_sub(ONE_SLOT) else {
        return Ok(None);
    };
    let mut cells = Vec::new();
    let mut row = FIRST_ROW;
    while row < target.rows() {
        let Some((cell, node)) = target.cell_at(row, column) else {
            return Ok(None);
        };
        let Some(slice) = column.checked_sub(cell.rect.column) else {
            return Ok(None);
        };
        let declared = column_width(node, slice)?;
        cells.push(CoveringCell {
            source_pos: cell.source_pos,
            node,
            slice,
            declared,
        });
        let Some(next_row) = row.checked_add(cell.rect.rowspan) else {
            return Ok(None);
        };
        row = next_row;
    }
    Ok((!cells.is_empty()).then_some(cells))
}

pub(crate) fn can_set_column_width(target: &TableTarget<'_>) -> Result<bool, TableError> {
    Ok(covering_cells(target)?.is_some())
}

pub(crate) fn plan_set_column_width(
    target: &TableTarget<'_>,
    width: u32,
) -> Result<Option<TableActionOutcome>, TableError> {
    let Some(rect) = target.rect() else {
        return Ok(None);
    };
    let Some(selection_after) =
        target.cell_selection_over(rect.top, rect.left, rect.bottom, rect.right)
    else {
        return Ok(None);
    };
    let Some(covering) = covering_cells(target)? else {
        return Ok(None);
    };

    let mut operations = Vec::new();
    for cell in covering {
        if cell.declared == width {
            continue;
        }
        let Some(attrs) = attrs_with_column_width(cell.node, cell.slice, width) else {
            return Ok(None);
        };
        operations.push(SemanticOperation::UpdateNodeAttrs {
            pos: cell.source_pos,
            attrs,
        });
    }
    if operations.is_empty() {
        return Ok(None);
    }
    operations.reverse();

    Ok(Some(TableActionOutcome {
        operations,
        selection_after,
    }))
}
