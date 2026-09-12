use crate::boundary::ResourceLimits;
use crate::command_planner::{apply_operations, default_attrs, SemanticOperation};
use crate::model::{Document, Fragment, Node};
use crate::schema::Schema;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{
    attrs_with_row_span, caret_in_cell, fresh_cell_node, GridRequirement, TableEdge, TableTarget,
    FIRST_COLUMN, FIRST_ROW, MINIMUM_SURVIVING_ROWS, NODE_OPENING_TOKENS, ONE_SLOT,
};

fn reference_row(target: &TableTarget<'_>, row: u32) -> Option<u32> {
    let reference = row.checked_sub(ONE_SLOT).unwrap_or(FIRST_ROW);
    if !target.is_header_row(reference) {
        return Some(reference);
    }
    if row == FIRST_ROW || row == target.rows() {
        return None;
    }
    Some(row)
}

pub(crate) fn plan_insert_row(
    target: &TableTarget<'_>,
    side: TableEdge,
    schema: &Schema,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    let row = match side {
        TableEdge::Before => rect.top,
        TableEdge::After => rect.bottom,
    };
    if row > target.rows() {
        return None;
    }
    let reference = reference_row(target, row);

    let mut operations = Vec::new();
    let mut cells = Vec::new();
    let mut column = FIRST_COLUMN;
    while column < target.columns() {
        let spans_across = row > FIRST_ROW
            && row < target.rows()
            && target.covers_same_cell((row.checked_sub(ONE_SLOT)?, column), (row, column));
        if spans_across {
            let (cell, node) = target.cell_at(row, column)?;
            operations.push(SemanticOperation::UpdateNodeAttrs {
                pos: cell.source_pos,
                attrs: attrs_with_row_span(node, cell.rect.rowspan.checked_add(ONE_SLOT)?),
            });
            column = column.checked_add(cell.rect.colspan)?;
            continue;
        }
        let cell_type = match reference {
            Some(reference) => target.cell_type_at(reference, column),
            None => target.roles().cell.clone(),
        };
        cells.push(fresh_cell_node(schema, &cell_type)?);
        column = column.checked_add(ONE_SLOT)?;
    }
    if cells.is_empty() {
        return None;
    }

    let position = target.row_start(row)?;
    let row_type = target.roles().row.clone();
    operations.push(SemanticOperation::ReplaceRange {
        from: position,
        to: position,
        content: Fragment::from(vec![Node::element(
            row_type.clone(),
            default_attrs(schema, &row_type)?,
            Fragment::from(cells),
        )]),
    });
    Some(TableActionOutcome {
        operations,
        selection_after: caret_in_cell(position.checked_add(NODE_OPENING_TOKENS)?)?,
    })
}

pub(crate) fn plan_delete_rows(
    document: &Document,
    target: &TableTarget<'_>,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    let removed = rect.bottom.checked_sub(rect.top)?;
    if target.rows().checked_sub(removed)? < MINIMUM_SURVIVING_ROWS {
        return None;
    }

    let mut candidate = document.clone();
    let mut operations = Vec::new();
    for row in (rect.top..rect.bottom).rev() {
        let step = {
            let stage = TableTarget::resolve(
                &candidate,
                target.table_pos(),
                None,
                schema,
                limits,
                GridRequirement::Regular,
            )?;
            plan_delete_one_row(&stage, row)?
        };
        let Ok(next) = apply_operations(&candidate, schema, &step) else {
            return None;
        };
        candidate = next;
        operations.extend(step);
    }

    let surviving = TableTarget::resolve(
        &candidate,
        target.table_pos(),
        None,
        schema,
        limits,
        GridRequirement::Regular,
    )?;
    let row = rect.top.min(surviving.rows().checked_sub(ONE_SLOT)?);
    Some(TableActionOutcome {
        operations,
        selection_after: surviving.cell_selection_at(row, FIRST_COLUMN)?,
    })
}

fn plan_delete_one_row(target: &TableTarget<'_>, row: u32) -> Option<Vec<SemanticOperation>> {
    let row_start = target.row_start(row)?;
    let row_end = row_start.checked_add(target.row_node(row)?.node_size())?;

    let mut shrinks = Vec::new();
    let mut carried = Vec::new();
    let mut column = FIRST_COLUMN;
    while column < target.columns() {
        let (cell, node) = target.cell_at(row, column)?;
        let continues_from_above = row > FIRST_ROW
            && target.covers_same_cell((row.checked_sub(ONE_SLOT)?, column), (row, column));
        let continues_below = row.checked_add(ONE_SLOT)? < target.rows()
            && target.covers_same_cell((row, column), (row.checked_add(ONE_SLOT)?, column));
        if continues_from_above {
            shrinks.push(SemanticOperation::UpdateNodeAttrs {
                pos: cell.source_pos,
                attrs: attrs_with_row_span(node, cell.rect.rowspan.checked_sub(ONE_SLOT)?),
            });
        } else if continues_below {
            let destination = target.position_at(row.checked_add(ONE_SLOT)?, column)?;
            carried.push(SemanticOperation::ReplaceRange {
                from: destination,
                to: destination,
                content: Fragment::from(vec![Node::element(
                    node.node_type().to_owned(),
                    attrs_with_row_span(node, cell.rect.rowspan.checked_sub(ONE_SLOT)?),
                    node.content().cloned()?,
                )]),
            });
        }
        column = column.checked_add(cell.rect.colspan)?;
    }

    carried.reverse();
    let mut operations = shrinks;
    operations.extend(carried);
    operations.push(SemanticOperation::ReplaceRange {
        from: row_start,
        to: row_end,
        content: Fragment::empty(),
    });
    Some(operations)
}
