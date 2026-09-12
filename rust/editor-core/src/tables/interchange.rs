use crate::model::{Document, Fragment, Node};
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::command_context::CellAnchorPair;
use crate::tables::commands::{fresh_cell_node, GridRequirement, TableTarget};
use crate::tables::selection::resolve_cell_rect;

const ONE_CELL: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InterchangeFailure {
    NotACellRectangle,
    UnreadableGrid,
}

pub(crate) fn table_clipboard_fragment(
    document: &Document,
    selection: &Selection,
    projection_index: &TableProjectionIndex,
    schema: &Schema,
) -> Result<Fragment, InterchangeFailure> {
    let (anchor, head) = match selection {
        Selection::Cell { anchor, head } => (*anchor, *head),
        Selection::Text { .. } | Selection::Node { .. } | Selection::All => {
            return Err(InterchangeFailure::NotACellRectangle)
        }
    };
    let rect = resolve_cell_rect(projection_index, anchor, head)
        .ok_or(InterchangeFailure::NotACellRectangle)?;
    let target = TableTarget::resolve_in(
        document,
        projection_index,
        rect.table_pos,
        Some(CellAnchorPair { anchor, head }),
        schema,
        GridRequirement::AsProjected,
    )
    .ok_or(InterchangeFailure::UnreadableGrid)?;
    let roles = target.roles().clone();

    let mut rows = Vec::new();
    for row in rect.top..rect.bottom {
        let mut cells = Vec::new();
        for column in rect.left..rect.right {
            match target.cell_at(row, column) {
                Some((cell, node)) if cell.rect.row == row && cell.rect.column == column => {
                    cells.push(node.clone());
                }
                Some(_) => continue,
                None => cells.push(
                    fresh_cell_node(schema, &roles.cell)
                        .ok_or(InterchangeFailure::UnreadableGrid)?,
                ),
            }
        }
        let row_node = target
            .row_node(row)
            .ok_or(InterchangeFailure::UnreadableGrid)?;
        rows.push(Node::element(
            roles.row.clone(),
            row_node.attrs().clone(),
            Fragment::from(cells),
        ));
    }
    let table = target.table_node();
    Ok(Fragment::from(vec![Node::element(
        roles.table.clone(),
        table.attrs().clone(),
        Fragment::from(rows),
    )]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellStep {
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OuterCell {
    pub table_pos: u32,
    pub cell_pos: u32,
    pub order: usize,
}

pub(crate) fn outer_cell_containing(
    projection_index: &TableProjectionIndex,
    position: u32,
) -> Option<OuterCell> {
    let table_pos = projection_index
        .positions()
        .filter(|table_pos| {
            projection_index
                .table_at(*table_pos)
                .is_some_and(|table| holds(table, position))
        })
        .min()?;
    let table = projection_index.table_at(table_pos)?;
    let order = table
        .cells
        .iter()
        .position(|cell| cell.source_pos <= position && position < cell.source_end)?;
    Some(OuterCell {
        table_pos,
        cell_pos: table.cells.get(order)?.source_pos,
        order,
    })
}

pub(crate) fn next_outer_cell(
    projection_index: &TableProjectionIndex,
    anchor: u32,
    direction: CellStep,
) -> Option<u32> {
    let located = outer_cell_containing(projection_index, anchor)?;
    let next = match direction {
        CellStep::Forward => located.order.checked_add(ONE_CELL)?,
        CellStep::Backward => located.order.checked_sub(ONE_CELL)?,
    };
    projection_index
        .table_at(located.table_pos)?
        .cells
        .get(next)
        .map(|cell| cell.source_pos)
}

fn holds(table: &crate::tables::projection::ProjectedTable, position: u32) -> bool {
    table
        .cells
        .iter()
        .any(|cell| cell.source_pos <= position && position < cell.source_end)
}
