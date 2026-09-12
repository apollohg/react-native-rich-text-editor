use super::{CommandPlan, PlanningContext, TypedCommand};
use crate::boundary::ResourceLimits;
use crate::command_planner::{simulate_plan, AdmittedSemanticCommandPlan, SemanticCommandPlan};
use crate::model::Document;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::command_context::{
    is_action_unavailable, prepare_table_action, CellAnchorPair, TableAction, TableActionContext,
};
use crate::tables::commands::{
    columns, headers, plan_clear_cells, plan_delete_table, plan_insert_table, plan_select_columns,
    plan_select_rows, rows, DeleteColumnsAction, DeleteRowsAction, InsertColumnAction,
    InsertRowAction, TableCommand, TableTarget, ToggleHeaderAction, CELL_INTERIOR_OFFSET,
};
use crate::tables::selection::{cell_opening_containing, resolve_cell_rect};
use crate::yrs_engine::{
    HistoryPolicy, OperationError, OperationResult, SelectionIntent, TypedTransaction,
};

const CLEAR_CELLS_FIELD: &str = "clearTableCells";
const SELECT_CELLS_FIELD: &str = "selectTableCells";
const TABLE_COMMAND_OPERATION_INDEX: usize = 0;

pub(crate) struct TableAnchor {
    pub table_pos: u32,
    pub anchors: CellAnchorPair,
}

pub(crate) fn anchor_from_selection(
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
    selection: &Selection,
) -> Option<TableAnchor> {
    let index = TableProjectionIndex::derive_or_fallback(document, schema, limits);
    let (anchor, head) = match selection {
        Selection::Cell { anchor, head } => (*anchor, *head),
        Selection::Text { anchor, head } => {
            let opening = cell_opening_containing(&index, (*anchor).min(*head))?;
            (opening, opening)
        }
        Selection::Node { .. } | Selection::All => return None,
    };
    let rect = resolve_cell_rect(&index, anchor, head)?;
    Some(TableAnchor {
        table_pos: rect.table_pos,
        anchors: CellAnchorPair { anchor, head },
    })
}

fn anchored_target<'a>(
    document: &'a Document,
    schema: &Schema,
    limits: &ResourceLimits,
    anchor: Option<&TableAnchor>,
) -> Option<TableTarget<'a>> {
    let anchor = anchor?;
    TableTarget::resolve(
        document,
        anchor.table_pos,
        Some(anchor.anchors),
        schema,
        limits,
    )
}

fn scoped_action(
    context: &PlanningContext<'_>,
    anchor: &TableAnchor,
    action: &dyn TableAction,
) -> OperationResult<CommandPlan> {
    let prepared = prepare_table_action(
        &TableActionContext {
            request_id: context.request_id,
            base_document_revision: context.revision,
            table_pos: anchor.table_pos,
            anchors: Some(anchor.anchors),
            schema: context.schema,
            resource_limits: context.resource_limits,
            editing_limits: context.editing_limits,
            document: context.document,
        },
        action,
    );
    let prepared = match prepared {
        Err(error) if is_action_unavailable(&error) => return Ok(CommandPlan::NotApplicable),
        other => other?,
    };
    super::table_action_transaction(context, prepared)
}

fn semantic(
    context: &PlanningContext<'_>,
    selection: &Selection,
    plan: Option<SemanticCommandPlan>,
) -> OperationResult<CommandPlan> {
    match plan {
        None => Ok(CommandPlan::NotApplicable),
        Some(plan) => super::text::semantic_transaction(context, selection, plan),
    }
}

fn admitted_cell_content(
    context: &PlanningContext<'_>,
    selection: &Selection,
    plan: SemanticCommandPlan,
) -> OperationResult<CommandPlan> {
    let simulated = simulate_plan(
        context.document,
        context.schema,
        selection,
        &plan,
        context.resource_limits,
    )
    .map_err(|()| {
        OperationError::operation_invalid(
            context.request_id,
            TABLE_COMMAND_OPERATION_INDEX,
            CLEAR_CELLS_FIELD,
            "clearing the selected cells did not simulate",
        )
    })?;
    super::text::admitted_semantic_transaction(
        context,
        selection,
        AdmittedSemanticCommandPlan { plan, simulated },
    )
}

fn cell_selection_intent(
    context: &PlanningContext<'_>,
    expanded: &Selection,
) -> Option<crate::yrs_engine::SelectionInput> {
    let Selection::Cell { anchor, head } = expanded else {
        return None;
    };
    let inside = |opening: u32| {
        let interior = opening.checked_add(CELL_INTERIOR_OFFSET)?;
        Some(crate::yrs_engine::RevisionedPosition {
            offset: context
                .position_map
                .doc_to_scalar(interior, context.document),
            kind: crate::yrs_engine::EditorOffsetKind::Scalar,
            affinity: crate::yrs_engine::DEFAULT_POSITION_AFFINITY,
        })
    };
    Some(crate::yrs_engine::SelectionInput::Cell {
        anchor: inside(*anchor)?,
        head: inside(*head)?,
    })
}

fn selection_only(
    context: &PlanningContext<'_>,
    expanded: Selection,
) -> OperationResult<CommandPlan> {
    let intent = cell_selection_intent(context, &expanded).ok_or_else(|| {
        OperationError::selection_position_invalid(
            context.request_id,
            SELECT_CELLS_FIELD,
            "the expanded cell rectangle is not representable as an editor selection",
        )
    })?;
    Ok(CommandPlan::SelectionOnly(TypedTransaction {
        request_id: context.request_id,
        base_document_revision: context.revision,
        origin: context.origin,
        operations: Vec::new(),
        selection_intent: SelectionIntent::Set(intent),
        history_policy: HistoryPolicy::Skip,
    }))
}

pub(super) fn plan(
    context: PlanningContext<'_>,
    command: TypedCommand,
) -> OperationResult<CommandPlan> {
    let TypedCommand::Table(command) = command else {
        return Err(OperationError::engine_invariant_failed(
            context.request_id,
            None,
            "the table planner received a non-table command",
        ));
    };
    let selection = super::structure::selection(&context);
    let anchor = anchor_from_selection(
        context.document,
        context.schema,
        context.resource_limits,
        &selection,
    );

    match command {
        TableCommand::InsertTable {
            rows,
            columns,
            with_header_row,
        } => {
            if anchor.is_some() {
                return Ok(CommandPlan::NotApplicable);
            }
            let plan = plan_insert_table(
                context.document,
                context.schema,
                &selection,
                context.resource_limits,
                rows,
                columns,
                with_header_row,
            );
            semantic(&context, &selection, plan)
        }
        TableCommand::DeleteTable => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => {
                let plan = plan_delete_table(context.document, context.schema, anchor.table_pos);
                semantic(&context, &selection, plan)
            }
        },
        TableCommand::AddTableRow { side } => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &InsertRowAction { side }),
        },
        TableCommand::DeleteTableRows => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &DeleteRowsAction),
        },
        TableCommand::AddTableColumn { side } => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &InsertColumnAction { side }),
        },
        TableCommand::DeleteTableColumns => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &DeleteColumnsAction),
        },
        TableCommand::ToggleTableHeader { target } => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(
                &context,
                &anchor,
                &ToggleHeaderAction {
                    target,
                    selection_before: selection.clone(),
                },
            ),
        },
        TableCommand::SelectTableRows => {
            match anchored_target(
                context.document,
                context.schema,
                context.resource_limits,
                anchor.as_ref(),
            )
            .and_then(|target| plan_select_rows(&target))
            {
                None => Ok(CommandPlan::NotApplicable),
                Some(expanded) => selection_only(&context, expanded),
            }
        }
        TableCommand::SelectTableColumns => {
            match anchored_target(
                context.document,
                context.schema,
                context.resource_limits,
                anchor.as_ref(),
            )
            .and_then(|target| plan_select_columns(&target))
            {
                None => Ok(CommandPlan::NotApplicable),
                Some(expanded) => selection_only(&context, expanded),
            }
        }
        TableCommand::ClearTableCells => {
            match anchored_target(
                context.document,
                context.schema,
                context.resource_limits,
                anchor.as_ref(),
            )
            .and_then(|target| plan_clear_cells(&target, context.schema))
            {
                None => Ok(CommandPlan::NotApplicable),
                Some(plan) => admitted_cell_content(&context, &selection, plan),
            }
        }
    }
}

pub(crate) fn table_command_is_available(
    document: &Document,
    schema: &Schema,
    selection: &Selection,
    limits: &ResourceLimits,
    command: TableCommand,
) -> bool {
    let anchor = anchor_from_selection(document, schema, limits, selection);
    let target = anchored_target(document, schema, limits, anchor.as_ref());
    match command {
        TableCommand::InsertTable {
            rows,
            columns,
            with_header_row,
        } => {
            anchor.is_none()
                && plan_insert_table(
                    document,
                    schema,
                    selection,
                    limits,
                    rows,
                    columns,
                    with_header_row,
                )
                .is_some()
        }
        TableCommand::DeleteTable => anchor
            .is_some_and(|anchor| plan_delete_table(document, schema, anchor.table_pos).is_some()),
        TableCommand::AddTableRow { side } => {
            target.is_some_and(|target| rows::plan_insert_row(&target, side, schema).is_some())
        }
        TableCommand::DeleteTableRows => target.is_some_and(|target| {
            rows::plan_delete_rows(document, &target, schema, limits).is_some()
        }),
        TableCommand::AddTableColumn { side } => target
            .is_some_and(|target| columns::plan_insert_column(&target, side, schema).is_some()),
        TableCommand::DeleteTableColumns => target.is_some_and(|target| {
            columns::plan_delete_columns(document, &target, schema, limits).is_some()
        }),
        TableCommand::ToggleTableHeader { target: header } => target.is_some_and(|target| {
            headers::plan_toggle_header(&target, header, schema, selection).is_some()
        }),
        TableCommand::SelectTableRows => {
            target.is_some_and(|target| plan_select_rows(&target).is_some())
        }
        TableCommand::SelectTableColumns => {
            target.is_some_and(|target| plan_select_columns(&target).is_some())
        }
        TableCommand::ClearTableCells => {
            target.is_some_and(|target| plan_clear_cells(&target, schema).is_some())
        }
    }
}
