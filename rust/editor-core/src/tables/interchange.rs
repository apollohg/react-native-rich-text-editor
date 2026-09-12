use crate::model::{Document, Fragment, Node};
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::command_context::CellAnchorPair;
use crate::tables::commands::{fresh_cell_node, node_starting_at, GridRequirement, TableTarget};
use crate::tables::projection::{span_attribute, CellRect};
use crate::tables::roles::{
    TableRoles, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_COLWIDTH_ATTR, TABLE_CELL_ROWSPAN_ATTR,
};
use crate::tables::selection::resolve_cell_rect;
use crate::tables::types::TableError;
use crate::yrs_engine::OperationError;

const ONE_CELL: usize = 1;
const NODE_OPENING_TOKENS: u32 = 1;
const TABLE_INTERCHANGE_OPERATION_INDEX: usize = 0;
pub(crate) const TABLE_CLIPBOARD_SELECTION_FIELD: &str = "tableClipboard.selection";
pub(crate) const TABLE_CLIPBOARD_GRID_FIELD: &str = "tableClipboard.grid";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InterchangeFailure {
    NotACellRectangle,
    UnreadableGrid,
}

impl InterchangeFailure {
    pub(crate) fn into_operation_error(self, request_id: u64) -> OperationError {
        match self {
            Self::NotACellRectangle => OperationError::operation_invalid(
                request_id,
                TABLE_INTERCHANGE_OPERATION_INDEX,
                TABLE_CLIPBOARD_SELECTION_FIELD,
                "a table clipboard fragment requires a cell rectangle",
            ),
            Self::UnreadableGrid => OperationError::document_invalid(
                request_id,
                None,
                TABLE_CLIPBOARD_GRID_FIELD,
                "the table grid cannot be read",
            ),
        }
    }
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
                    cells.push(effective_cell(node, &cell.rect)?);
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

fn unreadable_grid(error: TableError) -> InterchangeFailure {
    match error {
        TableError::GridLimit { .. }
        | TableError::WorkLimit
        | TableError::Allocation
        | TableError::InvalidStructure
        | TableError::InvalidAttributes => InterchangeFailure::UnreadableGrid,
    }
}

fn effective_cell(node: &Node, rect: &CellRect) -> Result<Node, InterchangeFailure> {
    let declared_colspan =
        span_attribute(node, TABLE_CELL_COLSPAN_ATTR).map_err(unreadable_grid)?;
    let declared_rowspan =
        span_attribute(node, TABLE_CELL_ROWSPAN_ATTR).map_err(unreadable_grid)?;
    if declared_colspan == rect.colspan && declared_rowspan == rect.rowspan {
        return Ok(node.clone());
    }
    let mut attrs = node.attrs().clone();
    attrs.insert(
        TABLE_CELL_COLSPAN_ATTR.to_string(),
        serde_json::Value::from(rect.colspan),
    );
    attrs.insert(
        TABLE_CELL_ROWSPAN_ATTR.to_string(),
        serde_json::Value::from(rect.rowspan),
    );
    if let Some(serde_json::Value::Array(widths)) = node.attrs().get(TABLE_CELL_COLWIDTH_ATTR) {
        let mut clipped = widths.clone();
        clipped.resize(rect.colspan as usize, serde_json::Value::Null);
        attrs.insert(
            TABLE_CELL_COLWIDTH_ATTR.to_string(),
            serde_json::Value::Array(clipped),
        );
    }
    Ok(Node::element(
        node.node_type().into(),
        attrs,
        node.content()
            .cloned()
            .ok_or(InterchangeFailure::UnreadableGrid)?,
    ))
}

fn encloses_a_table(node: &Node, roles: &TableRoles) -> bool {
    if node.node_type() == roles.table {
        return true;
    }
    node.content().is_some_and(|content| {
        content
            .iter()
            .any(|descendant| encloses_a_table(descendant, roles))
    })
}

fn first_editable_position_in(
    content: &Fragment,
    interior_pos: u32,
    roles: &TableRoles,
) -> Result<Option<u32>, InterchangeFailure> {
    let mut child_pos = interior_pos;
    for child in content.iter() {
        if !encloses_a_table(child, roles) {
            return match child_pos.checked_add(NODE_OPENING_TOKENS) {
                Some(interior) => Ok(Some(interior)),
                None => Err(InterchangeFailure::UnreadableGrid),
            };
        }
        if child.node_type() != roles.table {
            let child_interior = match child_pos.checked_add(NODE_OPENING_TOKENS) {
                Some(child_interior) => child_interior,
                None => return Err(InterchangeFailure::UnreadableGrid),
            };
            if let Some(nested) = child.content() {
                if let Some(found) = first_editable_position_in(nested, child_interior, roles)? {
                    return Ok(Some(found));
                }
            }
        }
        child_pos = match child_pos.checked_add(child.node_size()) {
            Some(child_pos) => child_pos,
            None => return Err(InterchangeFailure::UnreadableGrid),
        };
    }
    Ok(None)
}

pub(crate) fn first_editable_position_in_cell(
    document: &Document,
    schema: &Schema,
    cell_pos: u32,
) -> Result<Option<u32>, InterchangeFailure> {
    let Some(roles) = TableRoles::resolve(schema).map_err(unreadable_grid)? else {
        return Ok(None);
    };
    let Some(cell) = node_starting_at(document, cell_pos) else {
        return Ok(None);
    };
    let Some(content) = cell.content() else {
        return Ok(None);
    };
    let interior_pos = match cell_pos.checked_add(NODE_OPENING_TOKENS) {
        Some(interior_pos) => interior_pos,
        None => return Err(InterchangeFailure::UnreadableGrid),
    };
    first_editable_position_in(content, interior_pos, &roles)
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
