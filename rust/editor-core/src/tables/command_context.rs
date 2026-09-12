use crate::boundary::ResourceLimits;
use crate::command_planner::{
    AdmittedSemanticCommandPlan, SemanticCommandHistory, SemanticCommandPlan, SemanticOperation,
    SimulatedCommandPlan,
};
use crate::model::Document;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::normalize::{normalize_outer_table, outer_table_grid};
use crate::tables::selection::resolve_cell_rect;
use crate::tables::types::{TableActionKind, TableWorkCounters};
use crate::transform::{apply_step_canonical_marks, StepMap};
use crate::yrs_engine::{EditingLimits, OperationError, OperationResult, TransactionOrigin};

const TABLE_ACTION_FIELD: &str = "tableAction";
const TABLE_ACTION_UNAVAILABLE_FIELD: &str = "tableAction.unavailable";
const TABLE_ACTION_ANCHOR_FIELD: &str = "tableAction.anchors";
const TABLE_ACTION_GRID_FIELD: &str = "tableAction.grid";
const TABLE_ACTION_OPERATIONS_FIELD: &str = "maxOperationsPerTransaction";
const SINGLE_PASS: u32 = 1;
const TRUSTED_TABLE_ACTION_ORIGIN: TransactionOrigin = TransactionOrigin::LocalCommand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellAnchorPair {
    pub anchor: u32,
    pub head: u32,
}

pub(crate) struct TableActionContext<'a> {
    pub request_id: u64,
    pub base_document_revision: u64,
    pub table_pos: u32,
    pub anchors: Option<CellAnchorPair>,
    pub schema: &'a Schema,
    pub resource_limits: &'a ResourceLimits,
    pub editing_limits: &'a EditingLimits,
    pub document: &'a Document,
}

pub(crate) struct TableActionCandidate<'a> {
    pub document: &'a Document,
    pub table_pos: u32,
    pub anchors: Option<CellAnchorPair>,
}

pub(crate) struct TableActionOutcome {
    pub operations: Vec<SemanticOperation>,
    pub selection_after: Selection,
}

pub(crate) trait TableAction {
    fn kind(&self) -> TableActionKind;

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> Option<TableActionOutcome>;
}

pub(crate) struct PreparedTableAction {
    pub request_id: u64,
    pub kind: TableActionKind,
    pub origin: TransactionOrigin,
    pub base_document_revision: u64,
    pub plan: AdmittedSemanticCommandPlan,
    pub counters: TableWorkCounters,
}

pub(crate) fn prepare_table_action(
    context: &TableActionContext<'_>,
    action: &dyn TableAction,
) -> OperationResult<PreparedTableAction> {
    let mut counters = TableWorkCounters::default();
    let mut operations: Vec<SemanticOperation> = Vec::new();

    let pre_pass = normalization_pass(
        context,
        context.document,
        NormalizationPhase::Pre,
        &mut counters,
    )?;
    let (candidate, pre_map) = advance_candidate(context, context.document, &pre_pass)?;
    operations.extend(pre_pass);

    let anchors = remap_anchors(context, &candidate, &pre_map)?;
    let outcome = action
        .plan(
            &TableActionCandidate {
                document: &candidate,
                table_pos: context.table_pos,
                anchors,
            },
            context.schema,
            context.resource_limits,
        )
        .ok_or_else(|| action_unavailable(context, action.kind()))?;
    let (acted, _) = advance_candidate(context, &candidate, &outcome.operations)?;
    operations.extend(outcome.operations);

    let prepared_document = match outer_table_grid(
        &acted,
        context.table_pos,
        context.schema,
        context.resource_limits,
    )
    .map_err(|error| recorrelate(error, context.request_id))?
    {
        None => acted,
        Some(_) => {
            let post_pass =
                normalization_pass(context, &acted, NormalizationPhase::Post, &mut counters)?;
            let (normalized, _) = advance_candidate(context, &acted, &post_pass)?;
            operations.extend(post_pass);
            require_valid_outer_grid(context, &normalized)?;
            normalized
        }
    };

    if operations.len() > context.editing_limits.max_operations_per_transaction {
        return Err(OperationError::operation_limit_exceeded(
            context.request_id,
            None,
            TABLE_ACTION_OPERATIONS_FIELD,
            context.editing_limits.max_operations_per_transaction as u64,
            operations.len() as u64,
        ));
    }

    Ok(PreparedTableAction {
        request_id: context.request_id,
        kind: action.kind(),
        origin: TRUSTED_TABLE_ACTION_ORIGIN,
        base_document_revision: context.base_document_revision,
        plan: AdmittedSemanticCommandPlan {
            plan: SemanticCommandPlan {
                operations,
                selection_after: Some(outcome.selection_after.clone()),
                history: SemanticCommandHistory::InputBoundary,
            },
            simulated: SimulatedCommandPlan {
                document: prepared_document,
                selection: outcome.selection_after,
            },
        },
        counters,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NormalizationPhase {
    Pre,
    Post,
}

fn normalization_pass(
    context: &TableActionContext<'_>,
    document: &Document,
    phase: NormalizationPhase,
    counters: &mut TableWorkCounters,
) -> OperationResult<Vec<SemanticOperation>> {
    let counted = match phase {
        NormalizationPhase::Pre => &mut counters.pre_normalization_passes,
        NormalizationPhase::Post => &mut counters.post_normalization_passes,
    };
    *counted = counted.saturating_add(SINGLE_PASS);
    let operations = normalize_outer_table(
        document,
        context.table_pos,
        context.schema,
        context.resource_limits,
    )
    .map_err(|error| recorrelate(error, context.request_id))?;
    counters.normalization_operations = counters
        .normalization_operations
        .saturating_add(u32::try_from(operations.len()).unwrap_or(u32::MAX));
    Ok(operations)
}

fn advance_candidate(
    context: &TableActionContext<'_>,
    document: &Document,
    operations: &[SemanticOperation],
) -> OperationResult<(Document, StepMap)> {
    let mut candidate = document.clone();
    let mut composed = StepMap::empty();
    for operation in operations {
        let (next, step_map) =
            apply_step_canonical_marks(&candidate, &operation.as_step(), context.schema).map_err(
                |error| {
                    OperationError::operation_invalid(
                        context.request_id,
                        operations.len(),
                        TABLE_ACTION_FIELD,
                        error.to_string(),
                    )
                },
            )?;
        composed = composed.compose(&step_map);
        candidate = next;
    }
    Ok((candidate, composed))
}

fn remap_anchors(
    context: &TableActionContext<'_>,
    candidate: &Document,
    map: &StepMap,
) -> OperationResult<Option<CellAnchorPair>> {
    let Some(anchors) = context.anchors else {
        return Ok(None);
    };
    let before = TableProjectionIndex::derive_or_fallback(
        context.document,
        context.schema,
        context.resource_limits,
    );
    let before_rect = resolve_cell_rect(&before, anchors.anchor, anchors.head)
        .filter(|rect| rect.table_pos == context.table_pos)
        .ok_or_else(|| anchors_unusable(context))?;
    let mapped = CellAnchorPair {
        anchor: map.map_pos(anchors.anchor),
        head: map.map_pos(anchors.head),
    };
    let after = TableProjectionIndex::derive_or_fallback(
        candidate,
        context.schema,
        context.resource_limits,
    );
    let after_rect = resolve_cell_rect(&after, mapped.anchor, mapped.head)
        .filter(|rect| rect.table_pos == context.table_pos)
        .ok_or_else(|| anchors_unusable(context))?;
    let expected: Vec<u32> = before_rect
        .cells
        .iter()
        .map(|cell| map.map_pos(*cell))
        .collect();
    if after_rect.cells != expected {
        return Err(anchors_unusable(context));
    }
    Ok(Some(mapped))
}

fn require_valid_outer_grid(
    context: &TableActionContext<'_>,
    document: &Document,
) -> OperationResult<()> {
    let grid = outer_table_grid(
        document,
        context.table_pos,
        context.schema,
        context.resource_limits,
    )
    .map_err(|error| recorrelate(error, context.request_id))?;
    match grid {
        Some(grid) if !grid.irregular => Ok(()),
        Some(_) | None => Err(OperationError::operation_invalid(
            context.request_id,
            0,
            TABLE_ACTION_GRID_FIELD,
            "the requested table action does not leave its target grid valid",
        )),
    }
}

pub(crate) fn is_action_unavailable(error: &OperationError) -> bool {
    error.details == Some(serde_json::json!({ "field": TABLE_ACTION_UNAVAILABLE_FIELD }))
}

fn action_unavailable(context: &TableActionContext<'_>, kind: TableActionKind) -> OperationError {
    OperationError::operation_invalid(
        context.request_id,
        0,
        TABLE_ACTION_UNAVAILABLE_FIELD,
        format!("table action {} is unavailable here", kind.as_str()),
    )
}

fn anchors_unusable(context: &TableActionContext<'_>) -> OperationError {
    OperationError::operation_invalid(
        context.request_id,
        0,
        TABLE_ACTION_ANCHOR_FIELD,
        "normalization changed the real cells the table action targets",
    )
}

fn recorrelate(mut error: OperationError, request_id: u64) -> OperationError {
    error.request_id = request_id;
    error
}
