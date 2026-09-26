use crate::boundary::ResourceLimits;
use crate::command_planner::{apply_operations, default_attrs, SemanticOperation};
use crate::model::{Document, Fragment, Node};
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::command_context::{
    table_shape_operation_error, TableAction, TableActionCandidate, TableActionOutcome,
};
use crate::tables::commands::{
    attrs_with_removed_columns, attrs_with_row_span, default_text_block_node, fresh_cell_node,
    GridRequirement, TableTarget, FIRST_COLUMN, FIRST_ROW, FIRST_WIDTH_SLICE, NODE_CLOSING_TOKENS,
    NODE_OPENING_TOKENS, ONE_SLOT,
};
use crate::tables::normalize::UNCORRELATED_REQUEST_ID;
use crate::tables::paste::{clip_matrix, MatrixCell, TableMatrix};
use crate::tables::projection::ProjectedCell;
use crate::tables::types::{TableActionKind, TableError};
use crate::yrs_engine::OperationResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PasteStage {
    Grow,
    IsolateTop,
    IsolateBottom,
    IsolateLeft,
    IsolateRight,
    Write,
}

const PASTE_STAGES: [PasteStage; 6] = [
    PasteStage::Grow,
    PasteStage::IsolateTop,
    PasteStage::IsolateBottom,
    PasteStage::IsolateLeft,
    PasteStage::IsolateRight,
    PasteStage::Write,
];

const NEXT_INDEX: usize = 1;

#[derive(Clone, Copy, Debug)]
struct PasteArea {
    top: u32,
    left: u32,
    bottom: u32,
    right: u32,
}

pub(crate) struct MatrixPasteAction {
    pub matrix: TableMatrix,
}

impl TableAction for MatrixPasteAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::MatrixPaste
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        plan_matrix_paste(candidate, &self.matrix, schema, limits)
            .map_err(|error| table_shape_operation_error(error, UNCORRELATED_REQUEST_ID))
    }
}

fn plan_matrix_paste(
    candidate: &TableActionCandidate<'_>,
    matrix: &TableMatrix,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<Option<TableActionOutcome>, TableError> {
    let Some(target) = TableTarget::resolve(
        candidate.document,
        candidate.table_pos,
        candidate.anchors,
        schema,
        limits,
        GridRequirement::Regular,
    ) else {
        return Ok(None);
    };
    let Some(rect) = target.rect() else {
        return Ok(None);
    };
    let placed = match candidate.selection {
        Selection::Cell { .. } => {
            let width = rect
                .right
                .checked_sub(rect.left)
                .ok_or(TableError::Allocation)?;
            let height = rect
                .bottom
                .checked_sub(rect.top)
                .ok_or(TableError::Allocation)?;
            let Some(clipped) = clip_matrix(matrix, width, height) else {
                return Ok(None);
            };
            clipped
        }
        Selection::Text { .. } => matrix.clone(),
        Selection::Node { .. } | Selection::All => return Ok(None),
    };
    let area = PasteArea {
        top: rect.top,
        left: rect.left,
        bottom: rect
            .top
            .checked_add(placed.height)
            .ok_or(TableError::Allocation)?,
        right: rect
            .left
            .checked_add(placed.width)
            .ok_or(TableError::Allocation)?,
    };
    admit_grown_grid(&target, &area, limits)?;
    let Some(columns) = placed.cell_columns() else {
        return Ok(None);
    };

    let mut document = candidate.document.clone();
    let mut operations = Vec::new();
    for stage in PASTE_STAGES {
        let step = {
            let Some(staged) = staged_table(&document, candidate.table_pos, schema, limits) else {
                return Ok(None);
            };
            match stage {
                PasteStage::Grow => plan_growth(&staged, &area, schema),
                PasteStage::IsolateTop => plan_row_isolation(&staged, area.top, &area, schema),
                PasteStage::IsolateBottom => {
                    plan_row_isolation(&staged, area.bottom, &area, schema)
                }
                PasteStage::IsolateLeft => plan_column_isolation(&staged, area.left, &area, schema),
                PasteStage::IsolateRight => {
                    plan_column_isolation(&staged, area.right, &area, schema)
                }
                PasteStage::Write => plan_write(&staged, &area, &placed, &columns),
            }
        };
        let Some(step) = step else {
            return Ok(None);
        };
        if step.is_empty() {
            continue;
        }
        let Ok(next) = apply_operations(&document, schema, &step) else {
            return Ok(None);
        };
        document = next;
        operations.extend(step);
    }

    let Some(pasted) = staged_table(&document, candidate.table_pos, schema, limits) else {
        return Ok(None);
    };
    let Some(selection_after) =
        pasted.cell_selection_over(area.top, area.left, area.bottom, area.right)
    else {
        return Ok(None);
    };
    Ok(Some(TableActionOutcome {
        operations,
        selection_after,
    }))
}

fn staged_table<'a>(
    document: &'a Document,
    table_pos: u32,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Option<TableTarget<'a>> {
    TableTarget::resolve(
        document,
        table_pos,
        None,
        schema,
        limits,
        GridRequirement::AsProjected,
    )
}

fn admit_grown_grid(
    target: &TableTarget<'_>,
    area: &PasteArea,
    limits: &ResourceLimits,
) -> Result<(), TableError> {
    let slots = (target.rows().max(area.bottom) as usize)
        .checked_mul(target.columns().max(area.right) as usize)
        .ok_or(TableError::Allocation)?;
    if slots > limits.max_table_grid_slots {
        return Err(TableError::GridLimit {
            limit: limits.max_table_grid_slots,
            actual: slots,
        });
    }
    Ok(())
}

fn fresh_cells(schema: &Schema, cell_type: &str, count: u32) -> Option<Vec<Node>> {
    let cell = fresh_cell_node(schema, cell_type)?;
    Some((FIRST_COLUMN..count).map(|_| cell.clone()).collect())
}

fn plan_growth(
    target: &TableTarget<'_>,
    area: &PasteArea,
    schema: &Schema,
) -> Option<Vec<SemanticOperation>> {
    let roles = target.roles();
    let rows = target.rows();
    let columns = target.columns();
    let mut operations = Vec::new();

    if area.bottom > rows {
        let last_row = rows.checked_sub(ONE_SLOT)?;
        let mut cells = Vec::new();
        for column in FIRST_COLUMN..columns.max(area.right) {
            let header =
                column < columns && target.cell_type_at(last_row, column) == roles.header_cell;
            let cell_type = if header {
                &roles.header_cell
            } else {
                &roles.cell
            };
            cells.push(fresh_cell_node(schema, cell_type)?);
        }
        let row = Node::element(
            roles.row.clone(),
            default_attrs(schema, &roles.row)?,
            Fragment::from(cells),
        );
        let appended = (rows..area.bottom).map(|_| row.clone()).collect::<Vec<_>>();
        let table_end = target.row_start(rows)?;
        operations.push(SemanticOperation::ReplaceRange {
            from: table_end,
            to: table_end,
            content: Fragment::from(appended),
        });
    }

    if area.right > columns {
        let added = area.right.checked_sub(columns)?;
        for row in (FIRST_ROW..rows).rev() {
            let row_node = target.row_node(row)?;
            let header = row_node
                .content()?
                .children()
                .last()
                .is_some_and(|last| last.node_type() == roles.header_cell);
            let cell_type = if header {
                &roles.header_cell
            } else {
                &roles.cell
            };
            let row_end = target
                .row_start(row)?
                .checked_add(row_node.node_size())?
                .checked_sub(NODE_CLOSING_TOKENS)?;
            operations.push(SemanticOperation::ReplaceRange {
                from: row_end,
                to: row_end,
                content: Fragment::from(fresh_cells(schema, cell_type, added)?),
            });
        }
    }
    Some(operations)
}

fn remainder_cell(
    schema: &Schema,
    node: &Node,
    attrs: std::collections::HashMap<String, serde_json::Value>,
) -> Option<Node> {
    Some(Node::element(
        node.node_type().to_owned(),
        attrs,
        Fragment::from(vec![default_text_block_node(schema)?]),
    ))
}

fn plan_row_isolation(
    target: &TableTarget<'_>,
    edge: u32,
    area: &PasteArea,
    schema: &Schema,
) -> Option<Vec<SemanticOperation>> {
    if edge == FIRST_ROW || edge >= target.rows() {
        return Some(Vec::new());
    }
    let mut shrinks = Vec::new();
    let mut remainders = Vec::new();
    let mut column = area.left;
    while column < area.right {
        let (cell, node) = target.cell_at(edge, column)?;
        if cell.rect.row >= edge {
            column = column.checked_add(ONE_SLOT)?;
            continue;
        }
        let above = edge.checked_sub(cell.rect.row)?;
        let below = cell.rect.rowspan.checked_sub(above)?;
        shrinks.push(SemanticOperation::UpdateNodeAttrs {
            pos: cell.source_pos,
            attrs: attrs_with_row_span(node, above),
        });
        let at = target.position_at(edge, cell.rect.column)?;
        remainders.push(SemanticOperation::ReplaceRange {
            from: at,
            to: at,
            content: Fragment::from(vec![remainder_cell(
                schema,
                node,
                attrs_with_row_span(node, below),
            )?]),
        });
        column = cell.rect.column.checked_add(cell.rect.colspan)?;
    }
    remainders.reverse();
    shrinks.extend(remainders);
    Some(shrinks)
}

fn plan_column_isolation(
    target: &TableTarget<'_>,
    edge: u32,
    area: &PasteArea,
    schema: &Schema,
) -> Option<Vec<SemanticOperation>> {
    if edge == FIRST_COLUMN || edge >= target.columns() {
        return Some(Vec::new());
    }
    let mut operations = Vec::new();
    let mut row = area.top;
    while row < area.bottom {
        let (cell, node) = target.cell_at(row, edge)?;
        if cell.rect.column >= edge {
            row = row.checked_add(ONE_SLOT)?;
            continue;
        }
        let kept = edge.checked_sub(cell.rect.column)?;
        let moved = cell.rect.colspan.checked_sub(kept)?;
        operations.push(SemanticOperation::UpdateNodeAttrs {
            pos: cell.source_pos,
            attrs: attrs_with_removed_columns(node, kept, moved)?,
        });
        operations.push(SemanticOperation::ReplaceRange {
            from: cell.source_end,
            to: cell.source_end,
            content: Fragment::from(vec![remainder_cell(
                schema,
                node,
                attrs_with_removed_columns(node, FIRST_WIDTH_SLICE, kept)?,
            )?]),
        });
        row = cell.rect.row.checked_add(cell.rect.rowspan)?;
    }
    operations.reverse();
    Some(operations)
}

fn keeps_destination(destination: &ProjectedCell, node: &Node, source: &MatrixCell) -> bool {
    node.node_type() == source.node.node_type()
        && destination.rect.colspan == source.colspan
        && destination.rect.rowspan == source.rowspan
}

fn rewrite_cell(
    destination: &ProjectedCell,
    node: &Node,
    source: &MatrixCell,
    operations: &mut Vec<SemanticOperation>,
) -> Option<()> {
    if node.attrs() != source.node.attrs() {
        operations.push(SemanticOperation::UpdateNodeAttrs {
            pos: destination.source_pos,
            attrs: source.node.attrs().clone(),
        });
    }
    let content = source.node.content()?;
    if node.content()? != content {
        operations.push(SemanticOperation::ReplaceRange {
            from: destination.source_pos.checked_add(NODE_OPENING_TOKENS)?,
            to: destination.source_end.checked_sub(NODE_CLOSING_TOKENS)?,
            content: content.clone(),
        });
    }
    Some(())
}

fn replace_run(
    from: u32,
    to: u32,
    replaced: bool,
    inserted: &mut Vec<Node>,
    operations: &mut Vec<SemanticOperation>,
) {
    if !replaced && inserted.is_empty() {
        return;
    }
    operations.push(SemanticOperation::ReplaceRange {
        from,
        to,
        content: Fragment::from(std::mem::take(inserted)),
    });
}

fn plan_row_write(
    target: &TableTarget<'_>,
    row: u32,
    area: &PasteArea,
    sources: &[MatrixCell],
    source_columns: &[u32],
    operations: &mut Vec<SemanticOperation>,
) -> Option<()> {
    let mut destinations = Vec::new();
    for column in area.left..area.right {
        let (cell, node) = target.cell_at(row, column)?;
        if cell.rect.row == row && cell.rect.column == column {
            destinations.push((cell, node));
        }
    }
    let mut placed = Vec::with_capacity(sources.len());
    for (source, column) in sources.iter().zip(source_columns) {
        placed.push((area.left.checked_add(*column)?, source));
    }

    let mut run_start = target.position_at(row, area.left)?;
    let mut run_replaces = false;
    let mut inserted: Vec<Node> = Vec::new();
    let mut destination_index = 0usize;
    let mut source_index = 0usize;
    loop {
        match (
            destinations.get(destination_index),
            placed.get(source_index),
        ) {
            (Some((cell, node)), Some((column, source)))
                if cell.rect.column == *column && keeps_destination(cell, node, source) =>
            {
                replace_run(
                    run_start,
                    cell.source_pos,
                    run_replaces,
                    &mut inserted,
                    operations,
                );
                rewrite_cell(cell, node, source, operations)?;
                run_start = cell.source_end;
                run_replaces = false;
                destination_index = destination_index.checked_add(NEXT_INDEX)?;
                source_index = source_index.checked_add(NEXT_INDEX)?;
            }
            (Some((cell, _)), Some((column, source))) => {
                let destination_column = cell.rect.column;
                if destination_column <= *column {
                    run_replaces = true;
                    destination_index = destination_index.checked_add(NEXT_INDEX)?;
                }
                if *column <= destination_column {
                    inserted.push(source.node.clone());
                    source_index = source_index.checked_add(NEXT_INDEX)?;
                }
            }
            (Some(_), None) => {
                run_replaces = true;
                destination_index = destination_index.checked_add(NEXT_INDEX)?;
            }
            (None, Some((_, source))) => {
                inserted.push(source.node.clone());
                source_index = source_index.checked_add(NEXT_INDEX)?;
            }
            (None, None) => break,
        }
    }
    let run_end = target.position_at(row, area.right)?;
    replace_run(run_start, run_end, run_replaces, &mut inserted, operations);
    Some(())
}

fn plan_write(
    target: &TableTarget<'_>,
    area: &PasteArea,
    matrix: &TableMatrix,
    columns: &[Vec<u32>],
) -> Option<Vec<SemanticOperation>> {
    let mut operations = Vec::new();
    for ((row, sources), source_columns) in
        (area.top..area.bottom).zip(matrix.rows.iter()).zip(columns)
    {
        plan_row_write(target, row, area, sources, source_columns, &mut operations)?;
    }
    operations.reverse();
    Some(operations)
}
