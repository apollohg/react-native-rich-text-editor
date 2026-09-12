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

const ONE_CELL: usize = 1;
const NODE_OPENING_TOKENS: u32 = 1;

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

fn effective_cell(node: &Node, rect: &CellRect) -> Result<Node, InterchangeFailure> {
    let declared_colspan = span_attribute(node, TABLE_CELL_COLSPAN_ATTR)
        .map_err(|_| InterchangeFailure::UnreadableGrid)?;
    let declared_rowspan = span_attribute(node, TABLE_CELL_ROWSPAN_ATTR)
        .map_err(|_| InterchangeFailure::UnreadableGrid)?;
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

pub(crate) fn first_editable_position_in_cell(
    document: &Document,
    schema: &Schema,
    cell_pos: u32,
) -> Option<u32> {
    let roles = match TableRoles::resolve(schema) {
        Ok(Some(roles)) => roles,
        Ok(None) | Err(_) => return None,
    };
    let cell = node_starting_at(document, cell_pos)?;
    let mut child_pos = cell_pos.checked_add(NODE_OPENING_TOKENS)?;
    for child in cell.content()?.iter() {
        if child.node_type() != roles.table {
            return child_pos.checked_add(NODE_OPENING_TOKENS);
        }
        child_pos = child_pos.checked_add(child.node_size())?;
    }
    None
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
