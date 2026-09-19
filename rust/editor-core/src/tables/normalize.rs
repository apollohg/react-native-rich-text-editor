#[cfg(any(test, feature = "table-interop"))]
use std::cell::Cell;

use crate::boundary::ResourceLimits;
use crate::command_planner::{
    apply_operations, prove_structural_diff, structural_diff_bounded, structural_diff_range,
    SemanticOperation, StructuralDiff,
};
use crate::model::{Document, Fragment, Node};
use crate::schema::Schema;
use crate::tables::projection::{raw_table_grid, ProjectedTable, TableGridBudget};
use crate::tables::reference_grid::{reference_normalization, ReferenceFailure};
use crate::tables::roles::TableRoles;
use crate::tables::types::TableError;
use crate::yrs_engine::{OperationError, OperationResult};

pub(crate) const UNCORRELATED_REQUEST_ID: u64 = 0;
#[cfg(any(test, feature = "table-interop"))]
const ONE_PLANNED_PASS: u64 = 1;
#[cfg(any(test, feature = "table-interop"))]
const NO_PLANNED_PASSES: u64 = 0;

#[cfg(any(test, feature = "table-interop"))]
std::thread_local! {
    static PLANNED_NORMALIZATION_PASSES: Cell<u64> = const { Cell::new(NO_PLANNED_PASSES) };
}

#[cfg(any(test, feature = "table-interop"))]
fn record_planned_normalization_pass() {
    PLANNED_NORMALIZATION_PASSES
        .with(|passes| passes.set(passes.get().saturating_add(ONE_PLANNED_PASS)));
}
const TABLE_NORMALIZATION_FIELD: &str = "tableNormalization";
const TABLE_POSITION_FIELD: &str = "tablePos";
const NODE_OPENING_TOKENS: u32 = 1;
const NODE_CLOSING_TOKENS: u32 = 1;
const DOCUMENT_CONTENT_START: u32 = 0;
const FIRST_ROW: u32 = 0;
const NO_MISSING_SLOTS: u32 = 0;
const ADJACENT_ROW_DISTANCE: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NormalizationFailure {
    Shape(TableError),
    NestedTarget,
    MissingTarget,
    Unplannable,
}

impl NormalizationFailure {
    pub(crate) fn into_operation_error(self, request_id: u64) -> OperationError {
        match self {
            Self::Shape(TableError::GridLimit { limit, actual }) => {
                OperationError::document_limit_exceeded(
                    request_id,
                    None,
                    TABLE_NORMALIZATION_FIELD,
                    limit as u64,
                    actual as u64,
                )
            }
            Self::Shape(error @ (TableError::WorkLimit | TableError::Allocation)) => {
                OperationError::operation_work_budget_exceeded(
                    request_id,
                    TABLE_NORMALIZATION_FIELD,
                    error.to_string(),
                )
            }
            Self::Shape(error @ (TableError::InvalidStructure | TableError::InvalidAttributes)) => {
                OperationError::document_invalid(
                    request_id,
                    None,
                    TABLE_NORMALIZATION_FIELD,
                    error.to_string(),
                )
            }
            Self::NestedTarget => OperationError::document_invalid(
                request_id,
                None,
                TABLE_POSITION_FIELD,
                "a nested table is never a normalization target",
            ),
            Self::MissingTarget => OperationError::document_invalid(
                request_id,
                None,
                TABLE_POSITION_FIELD,
                "no outer table begins at the requested position",
            ),
            Self::Unplannable => OperationError::engine_invariant_failed(
                request_id,
                None,
                "table normalization could not be expressed as a scoped child-window change",
            ),
        }
    }
}

#[cfg(any(test, feature = "table-interop"))]
pub(crate) fn planned_normalization_passes() -> u64 {
    PLANNED_NORMALIZATION_PASSES.with(Cell::get)
}

#[cfg(any(test, feature = "table-interop"))]
pub(crate) fn reset_planned_normalization_passes() {
    PLANNED_NORMALIZATION_PASSES.with(|passes| passes.set(NO_PLANNED_PASSES));
}

pub(crate) fn normalize_outer_table(
    document: &Document,
    table_pos: u32,
    schema: &Schema,
    limits: &ResourceLimits,
) -> OperationResult<Vec<SemanticOperation>> {
    #[cfg(any(test, feature = "table-interop"))]
    record_planned_normalization_pass();
    plan_normalization(document, table_pos, schema, limits)
        .map_err(|failure| failure.into_operation_error(UNCORRELATED_REQUEST_ID))
}

pub(crate) fn outer_table_positions(
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
) -> OperationResult<Vec<u32>> {
    collect_outer_table_positions(document, schema, limits)
        .map_err(|failure| failure.into_operation_error(UNCORRELATED_REQUEST_ID))
}

fn collect_outer_table_positions(
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<Vec<u32>, NormalizationFailure> {
    let Some(roles) = TableRoles::resolve(schema).map_err(NormalizationFailure::Shape)? else {
        return Ok(Vec::new());
    };
    let mut positions = Vec::new();
    let mut visited = 0usize;
    let mut pending: Vec<(&Node, u32)> = vec![(document.root(), DOCUMENT_CONTENT_START)];
    while let Some((node, content_start)) = pending.pop() {
        let Some(content) = node.content() else {
            continue;
        };
        let mut position = content_start;
        for child in content.iter() {
            visited = visited.saturating_add(1);
            if visited > limits.max_document_nodes {
                return Err(NormalizationFailure::Shape(TableError::WorkLimit));
            }
            if child.node_type() == roles.table {
                positions.push(position);
            } else if child.content().is_some() {
                pending.push((child, advance(position, NODE_OPENING_TOKENS)?));
            }
            position = advance(position, child.node_size())?;
        }
    }
    positions.sort_unstable();
    Ok(positions)
}

pub(crate) fn outer_table_grid(
    document: &Document,
    table_pos: u32,
    schema: &Schema,
    limits: &ResourceLimits,
) -> OperationResult<Option<ProjectedTable>> {
    locate_outer_table(document, table_pos, schema, limits)
        .map(|located| located.map(|located| located.projected))
        .map_err(|failure| failure.into_operation_error(UNCORRELATED_REQUEST_ID))
}

struct LocatedTable {
    path: Vec<u32>,
    projected: ProjectedTable,
    budget: TableGridBudget,
}

fn locate_outer_table(
    document: &Document,
    table_pos: u32,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<Option<LocatedTable>, NormalizationFailure> {
    let Some(roles) = TableRoles::resolve(schema).map_err(NormalizationFailure::Shape)? else {
        return Ok(None);
    };
    let Some(path) = outer_table_path(document, table_pos, &roles, limits)? else {
        return Ok(None);
    };
    let table = document
        .node_at(&path)
        .ok_or(NormalizationFailure::MissingTarget)?;
    let mut budget = TableGridBudget::new(limits.max_table_grid_slots);
    let projected = raw_table_grid(table, table_pos, schema, &mut budget)
        .map_err(NormalizationFailure::Shape)?;
    Ok(Some(LocatedTable {
        path,
        projected,
        budget,
    }))
}

fn outer_table_path(
    document: &Document,
    table_pos: u32,
    roles: &TableRoles,
    limits: &ResourceLimits,
) -> Result<Option<Vec<u32>>, NormalizationFailure> {
    let mut node = document.root();
    let mut path: Vec<u32> = Vec::new();
    let mut content_start = DOCUMENT_CONTENT_START;
    let mut visited = 0usize;
    loop {
        let Some(content) = node.content() else {
            return Ok(None);
        };
        let mut position = content_start;
        let mut descent = None;
        for (index, child) in content.iter().enumerate() {
            visited = visited.saturating_add(1);
            if visited > limits.max_document_nodes {
                return Err(NormalizationFailure::Shape(TableError::WorkLimit));
            }
            let index = u32::try_from(index)
                .map_err(|_| NormalizationFailure::Shape(TableError::Allocation))?;
            let end = position
                .checked_add(child.node_size())
                .ok_or(NormalizationFailure::Shape(TableError::Allocation))?;
            if position == table_pos {
                if child.node_type() != roles.table {
                    return Ok(None);
                }
                path.push(index);
                return Ok(Some(path));
            }
            if position < table_pos && table_pos < end && child.content().is_some() {
                if child.node_type() == roles.table {
                    return Err(NormalizationFailure::NestedTarget);
                }
                descent = Some((
                    index,
                    child,
                    position
                        .checked_add(NODE_OPENING_TOKENS)
                        .ok_or(NormalizationFailure::Shape(TableError::Allocation))?,
                ));
                break;
            }
            position = end;
        }
        let Some((index, child, child_content_start)) = descent else {
            return Ok(None);
        };
        if path.len() >= limits.max_document_depth {
            return Err(NormalizationFailure::Shape(TableError::WorkLimit));
        }
        path.push(index);
        node = child;
        content_start = child_content_start;
    }
}

fn plan_normalization(
    document: &Document,
    table_pos: u32,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<Vec<SemanticOperation>, NormalizationFailure> {
    let mut located = locate_outer_table(document, table_pos, schema, limits)?
        .ok_or(NormalizationFailure::MissingTarget)?;
    let table = document
        .node_at(&located.path)
        .ok_or(NormalizationFailure::MissingTarget)?;
    if located.projected.cells.is_empty() {
        return Ok(Vec::new());
    }
    let raw_charge = located.budget.charged_slots();
    let analysis = reference_normalization(
        table,
        schema,
        &located.projected,
        &mut located.budget,
        raw_charge,
    )
    .map_err(|failure| match failure {
        ReferenceFailure::Unsafe(error) => NormalizationFailure::Shape(error),
        ReferenceFailure::Unsupported(_) => NormalizationFailure::Unplannable,
    })?;
    let mut operations = Vec::new();
    for (cell, attrs) in located.projected.cells.iter().zip(analysis.attrs) {
        if let Some(attrs) = attrs {
            operations.push(SemanticOperation::UpdateNodeAttrs {
                pos: cell.source_pos,
                attrs,
            });
        }
    }
    let mut candidate = apply_planned(document, schema, &operations)?;
    let additions = analysis.additions;
    for (row_index, missing) in additions.iter().copied().enumerate().rev() {
        if missing == NO_MISSING_SLOTS {
            continue;
        }
        let row_index = u32::try_from(row_index)
            .map_err(|_| NormalizationFailure::Shape(TableError::Allocation))?;
        let (operation, next) = insertion_operation(
            &candidate,
            &located.path,
            table_pos,
            row_index,
            missing,
            &additions,
            schema,
            limits,
        )?;
        operations.push(operation);
        candidate = next;
    }
    Ok(operations)
}

fn apply_planned(
    document: &Document,
    schema: &Schema,
    operations: &[SemanticOperation],
) -> Result<Document, NormalizationFailure> {
    apply_operations(document, schema, operations).map_err(|()| NormalizationFailure::Unplannable)
}

#[allow(clippy::too_many_arguments)]
fn insertion_operation(
    candidate: &Document,
    path: &[u32],
    table_pos: u32,
    row_index: u32,
    missing: u32,
    additions: &[u32],
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<(SemanticOperation, Document), NormalizationFailure> {
    let table = candidate
        .node_at(path)
        .ok_or(NormalizationFailure::MissingTarget)?;
    let row_start = row_start_position(table, table_pos, row_index)?;
    let row = table
        .child(row_index as usize)
        .ok_or(NormalizationFailure::Shape(TableError::InvalidStructure))?;
    let position = if inserts_at_row_start(additions, row_index) {
        advance(row_start, NODE_OPENING_TOKENS)?
    } else {
        advance(row_start, row.node_size())?
            .checked_sub(NODE_CLOSING_TOKENS)
            .ok_or(NormalizationFailure::Shape(TableError::Allocation))?
    };
    let cell = filler_cell(row, schema)?;
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(missing as usize)
        .map_err(|_| NormalizationFailure::Shape(TableError::Allocation))?;
    for _ in 0..missing {
        cells.push(cell.clone());
    }
    let operation = SemanticOperation::ReplaceRange {
        from: position,
        to: position,
        content: Fragment::from(cells),
    };
    let after = apply_planned(candidate, schema, std::slice::from_ref(&operation))?;
    prove_scoped_row_insertion(candidate, &after, path, row_index, position, schema, limits)?;
    Ok((operation, after))
}

fn inserts_at_row_start(additions: &[u32], row_index: u32) -> bool {
    let first = additions
        .iter()
        .position(|missing| *missing != NO_MISSING_SLOTS);
    let last = additions
        .iter()
        .rposition(|missing| *missing != NO_MISSING_SLOTS);
    let Some(last) = last.and_then(|last| u32::try_from(last).ok()) else {
        return false;
    };
    let follows_first = first
        .and_then(|first| u32::try_from(first).ok())
        .and_then(|first| row_index.checked_sub(first))
        .is_some_and(|distance| distance == ADJACENT_ROW_DISTANCE);
    (row_index == FIRST_ROW || follows_first) && last == row_index
}

#[allow(clippy::too_many_arguments)]
fn prove_scoped_row_insertion(
    before: &Document,
    after: &Document,
    path: &[u32],
    row_index: u32,
    position: u32,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<(), NormalizationFailure> {
    let diff = structural_diff_bounded(before, after, limits)
        .map_err(|()| NormalizationFailure::Unplannable)?
        .ok_or(NormalizationFailure::Unplannable)?;
    let expected_parent: Vec<u32> = path
        .iter()
        .copied()
        .chain(std::iter::once(row_index))
        .collect();
    if diff.parent_path != expected_parent || diff.from_child != diff.to_child {
        return Err(NormalizationFailure::Unplannable);
    }
    if replaced_range(before, &diff, limits)? != (position, position) {
        return Err(NormalizationFailure::Unplannable);
    }
    if !prove_structural_diff(before, after, &diff, schema, limits)
        .map_err(|()| NormalizationFailure::Unplannable)?
    {
        return Err(NormalizationFailure::Unplannable);
    }
    Ok(())
}

fn replaced_range(
    document: &Document,
    diff: &StructuralDiff,
    limits: &ResourceLimits,
) -> Result<(u32, u32), NormalizationFailure> {
    structural_diff_range(document, diff, limits).map_err(|()| NormalizationFailure::Unplannable)
}

fn row_start_position(
    table: &Node,
    table_pos: u32,
    row_index: u32,
) -> Result<u32, NormalizationFailure> {
    let content = table
        .content()
        .ok_or(NormalizationFailure::Shape(TableError::InvalidStructure))?;
    let mut position = advance(table_pos, NODE_OPENING_TOKENS)?;
    for row in content.iter().take(row_index as usize) {
        position = advance(position, row.node_size())?;
    }
    Ok(position)
}

fn filler_cell(row: &Node, schema: &Schema) -> Result<Node, NormalizationFailure> {
    crate::tables::reference_grid::filler_cell(row, schema).ok_or(NormalizationFailure::Unplannable)
}

fn advance(position: u32, amount: u32) -> Result<u32, NormalizationFailure> {
    position
        .checked_add(amount)
        .ok_or(NormalizationFailure::Shape(TableError::Allocation))
}
