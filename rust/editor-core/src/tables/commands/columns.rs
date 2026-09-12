use crate::boundary::ResourceLimits;
use crate::command_planner::{apply_operations, SemanticOperation};
use crate::model::{Document, Fragment};
use crate::schema::Schema;
use crate::tables::command_context::TableActionOutcome;
use crate::tables::commands::{
    attrs_with_added_column, attrs_with_removed_column, caret_in_cell, fresh_cell_node,
    GridRequirement, TableEdge, TableTarget, FIRST_COLUMN, FIRST_ROW, MINIMUM_SURVIVING_COLUMNS,
    ONE_SLOT,
};

fn reference_column(target: &TableTarget<'_>, column: u32) -> Option<u32> {
    let reference = column.checked_sub(ONE_SLOT).unwrap_or(FIRST_COLUMN);
    if !target.is_header_column(reference) {
        return Some(reference);
    }
    if column == FIRST_COLUMN || column == target.columns() {
        return None;
    }
    Some(column)
}

pub(crate) fn plan_insert_column(
    target: &TableTarget<'_>,
    side: TableEdge,
    schema: &Schema,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    let column = match side {
        TableEdge::Before => rect.left,
        TableEdge::After => rect.right,
    };
    if column > target.columns() {
        return None;
    }
    let reference = reference_column(target, column);

    let mut widenings = Vec::new();
    let mut insertions = Vec::new();
    let mut first_inserted: Option<u32> = None;
    let mut row = FIRST_ROW;
    while row < target.rows() {
        let spans_across = column > FIRST_COLUMN
            && column < target.columns()
            && target.covers_same_cell((row, column.checked_sub(ONE_SLOT)?), (row, column));
        if spans_across {
            let (cell, node) = target.cell_at(row, column)?;
            widenings.push(SemanticOperation::UpdateNodeAttrs {
                pos: cell.source_pos,
                attrs: attrs_with_added_column(node, column.checked_sub(cell.rect.column)?)?,
            });
            row = row.checked_add(cell.rect.rowspan)?;
            continue;
        }
        let cell_type = match reference {
            Some(reference) => target.cell_type_at(row, reference),
            None => target.roles().cell.clone(),
        };
        let position = target.position_at(row, column)?;
        first_inserted.get_or_insert(position);
        insertions.push(SemanticOperation::ReplaceRange {
            from: position,
            to: position,
            content: Fragment::from(vec![fresh_cell_node(schema, &cell_type)?]),
        });
        row = row.checked_add(ONE_SLOT)?;
    }

    let first_inserted = first_inserted?;
    insertions.reverse();
    let mut operations = widenings;
    operations.extend(insertions);
    Some(TableActionOutcome {
        operations,
        selection_after: caret_in_cell(first_inserted)?,
    })
}

pub(crate) fn plan_delete_columns(
    document: &Document,
    target: &TableTarget<'_>,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Option<TableActionOutcome> {
    let rect = target.rect()?;
    let removed = rect.right.checked_sub(rect.left)?;
    if target.columns().checked_sub(removed)? < MINIMUM_SURVIVING_COLUMNS {
        return None;
    }

    let mut candidate = document.clone();
    let mut operations = Vec::new();
    for column in (rect.left..rect.right).rev() {
        let step = {
            let stage = TableTarget::resolve(
                &candidate,
                target.table_pos(),
                None,
                schema,
                limits,
                GridRequirement::Regular,
            )?;
            plan_delete_one_column(&stage, column)?
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
    let column = rect.left.min(surviving.columns().checked_sub(ONE_SLOT)?);
    let row = rect.top.min(surviving.rows().checked_sub(ONE_SLOT)?);
    Some(TableActionOutcome {
        operations,
        selection_after: surviving.cell_selection_at(row, column)?,
    })
}

fn plan_delete_one_column(target: &TableTarget<'_>, column: u32) -> Option<Vec<SemanticOperation>> {
    let mut operations = Vec::new();
    let mut row = FIRST_ROW;
    while row < target.rows() {
        let (cell, node) = target.cell_at(row, column)?;
        let spans_left = column > FIRST_COLUMN
            && target.covers_same_cell((row, column.checked_sub(ONE_SLOT)?), (row, column));
        let spans_right = column.checked_add(ONE_SLOT)? < target.columns()
            && target.covers_same_cell((row, column), (row, column.checked_add(ONE_SLOT)?));
        if spans_left || spans_right {
            operations.push(SemanticOperation::UpdateNodeAttrs {
                pos: cell.source_pos,
                attrs: attrs_with_removed_column(node, column.checked_sub(cell.rect.column)?)?,
            });
        } else {
            operations.push(SemanticOperation::ReplaceRange {
                from: cell.source_pos,
                to: cell.source_end,
                content: Fragment::empty(),
            });
        }
        row = row.checked_add(cell.rect.rowspan)?;
    }
    operations.reverse();
    Some(operations)
}
