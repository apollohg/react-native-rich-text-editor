use std::collections::HashSet;

use crate::command_planner::SemanticOperation;
use crate::model::Node;
use crate::selection::Selection;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{attrs_with_column_width, TableTarget, FIRST_ROW, ONE_SLOT};
use crate::tables::projection::column_width;
use crate::tables::types::TableError;
use crate::transform::StepMap;

struct CoveringCell<'a> {
    source_pos: u32,
    node: &'a Node,
    slice: u32,
    declared: u32,
}

pub(crate) fn same_column_sources(
    original: &TableTarget<'_>,
    normalized: &TableTarget<'_>,
    column: u32,
    map: &StepMap,
) -> Result<bool, TableError> {
    if original.rows() != normalized.rows()
        || column >= original.columns()
        || column >= normalized.columns()
    {
        return Ok(false);
    }
    let mut original_sources = HashSet::new();
    original_sources
        .try_reserve(original.source_cell_positions().count())
        .map_err(|_| TableError::Allocation)?;
    original_sources.extend(
        original
            .source_cell_positions()
            .map(|source| map.map_pos(source)),
    );
    for row in FIRST_ROW..original.rows() {
        let Some((after, _)) = normalized.cell_at(row, column) else {
            return Ok(false);
        };
        match original.cell_at(row, column) {
            Some((before, _)) => {
                if map.map_pos(before.source_pos) != after.source_pos
                    || column.checked_sub(before.rect.column)
                        != column.checked_sub(after.rect.column)
                {
                    return Ok(false);
                }
            }
            None => {
                let Some(synthetic) = original.synthetic_at(row, column) else {
                    return Ok(false);
                };
                if synthetic.rect != after.rect || original_sources.contains(&after.source_pos) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn covering_cells<'a>(
    target: &TableTarget<'a>,
    column: Option<u32>,
) -> Result<Option<Vec<CoveringCell<'a>>>, TableError> {
    let Some(column) = column.or_else(|| target.rect()?.right.checked_sub(ONE_SLOT)) else {
        return Ok(None);
    };
    if column >= target.columns() {
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
    Ok(covering_cells(target, None)?.is_some())
}

pub(crate) fn plan_set_column_width(
    target: &TableTarget<'_>,
    width: u32,
    column: Option<u32>,
    selection: &Selection,
) -> Result<Option<TableActionOutcome>, TableError> {
    let selection_after = match column {
        Some(_) => selection.clone(),
        None => {
            let Some(rect) = target.rect() else {
                return Ok(None);
            };
            let Some(selected) =
                target.cell_selection_over(rect.top, rect.left, rect.bottom, rect.right)
            else {
                return Ok(None);
            };
            selected
        }
    };
    let Some(covering) = covering_cells(target, column)? else {
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
