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
    columns, headers, merge, plan_clear_cells, plan_delete_table, plan_insert_table,
    plan_select_columns, plan_select_rows, resize, rows, DeleteColumnsAction, DeleteRowsAction,
    GridRequirement, InsertColumnAction, InsertRowAction, MergeCellsAction, SetColumnWidthAction,
    SplitCellAction, TableCommand, TableEdge, TableTarget, ToggleHeaderAction,
    CELL_INTERIOR_OFFSET,
};
use crate::tables::interchange::{
    first_editable_position_in_cell, next_outer_cell, outer_cell_containing, CellStep,
};
use crate::tables::selection::{cell_opening_containing, resolve_cell_rect};
use crate::tables::types::TableError;
use crate::yrs_engine::{
    HistoryPolicy, OperationError, OperationResult, SelectionIntent, TypedTransaction,
};

const CLEAR_CELLS_FIELD: &str = "clearTableCells";
const UNREADABLE_GRID_IS_NOT_AVAILABLE: bool = false;
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
    anchor_in(&index, selection)
}

fn anchor_in(index: &TableProjectionIndex, selection: &Selection) -> Option<TableAnchor> {
    let (anchor, head) = match selection {
        Selection::Cell { anchor, head } => (*anchor, *head),
        Selection::Text { anchor, head } => {
            let opening = cell_opening_containing(index, (*anchor).min(*head))?;
            (opening, opening)
        }
        Selection::Node { .. } | Selection::All => return None,
    };
    let rect = resolve_cell_rect(index, anchor, head)?;
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
    requirement: GridRequirement,
) -> Option<TableTarget<'a>> {
    let anchor = anchor?;
    TableTarget::resolve(
        document,
        anchor.table_pos,
        Some(anchor.anchors),
        schema,
        limits,
        requirement,
    )
}

pub(super) fn selection_is_a_cell_rectangle(context: &PlanningContext<'_>) -> bool {
    match context.selection {
        crate::yrs_engine::ResolvedSelection::Cell { .. } => true,
        crate::yrs_engine::ResolvedSelection::Text { .. }
        | crate::yrs_engine::ResolvedSelection::Node { .. }
        | crate::yrs_engine::ResolvedSelection::All => false,
    }
}

pub(super) fn plan_clear_cell_rectangle(
    context: PlanningContext<'_>,
) -> OperationResult<CommandPlan> {
    let selection = super::structure::selection(&context);
    let anchor = anchor_from_selection(
        context.document,
        context.schema,
        context.resource_limits,
        &selection,
    );
    clear_cells(&context, &selection, anchor.as_ref())
}

fn clear_cells(
    context: &PlanningContext<'_>,
    selection: &Selection,
    anchor: Option<&TableAnchor>,
) -> OperationResult<CommandPlan> {
    match anchored_target(
        context.document,
        context.schema,
        context.resource_limits,
        anchor,
        GridRequirement::AsProjected,
    )
    .and_then(|target| plan_clear_cells(&target, context.schema))
    {
        None => Ok(CommandPlan::NotApplicable),
        Some(plan) => admitted_cell_content(context, selection, plan),
    }
}

fn scoped_action(
    context: &PlanningContext<'_>,
    anchor: &TableAnchor,
    selection: &Selection,
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
            selection,
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
            Some(anchor) => scoped_action(&context, &anchor, &selection, &InsertRowAction { side }),
        },
        TableCommand::DeleteTableRows => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &selection, &DeleteRowsAction),
        },
        TableCommand::AddTableColumn { side } => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => {
                scoped_action(&context, &anchor, &selection, &InsertColumnAction { side })
            }
        },
        TableCommand::DeleteTableColumns => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &selection, &DeleteColumnsAction),
        },
        TableCommand::ToggleTableHeader { target } => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(
                &context,
                &anchor,
                &selection,
                &ToggleHeaderAction { target },
            ),
        },
        TableCommand::SelectTableRows => {
            match anchored_target(
                context.document,
                context.schema,
                context.resource_limits,
                anchor.as_ref(),
                GridRequirement::AsProjected,
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
                GridRequirement::AsProjected,
            )
            .and_then(|target| plan_select_columns(&target))
            {
                None => Ok(CommandPlan::NotApplicable),
                Some(expanded) => selection_only(&context, expanded),
            }
        }
        TableCommand::ClearTableCells => clear_cells(&context, &selection, anchor.as_ref()),
        TableCommand::MergeTableCells => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &selection, &MergeCellsAction),
        },
        TableCommand::SplitTableCell => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(&context, &anchor, &selection, &SplitCellAction),
        },
        TableCommand::SetTableColumnWidth { width } => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(
                &context,
                &anchor,
                &selection,
                &SetColumnWidthAction { width },
            ),
        },
        TableCommand::MoveToAdjacentCell { step, append_row } => {
            move_to_adjacent_cell(&context, &selection, step, append_row)
        }
    }
}

fn navigating_caret(selection: &Selection) -> Option<u32> {
    match selection {
        Selection::Text { head, .. } | Selection::Cell { head, .. } => Some(*head),
        Selection::Node { .. } | Selection::All => None,
    }
}

fn caret_only(context: &PlanningContext<'_>, interior: u32) -> OperationResult<CommandPlan> {
    Ok(CommandPlan::SelectionOnly(TypedTransaction {
        request_id: context.request_id,
        base_document_revision: context.revision,
        origin: context.origin,
        operations: Vec::new(),
        selection_intent: SelectionIntent::Set(crate::yrs_engine::SelectionInput::Text {
            anchor: crate::yrs_engine::RevisionedPosition {
                offset: context
                    .position_map
                    .doc_to_scalar(interior, context.document),
                kind: crate::yrs_engine::EditorOffsetKind::Scalar,
                affinity: crate::yrs_engine::DEFAULT_POSITION_AFFINITY,
            },
            head: crate::yrs_engine::RevisionedPosition {
                offset: context
                    .position_map
                    .doc_to_scalar(interior, context.document),
                kind: crate::yrs_engine::EditorOffsetKind::Scalar,
                affinity: crate::yrs_engine::DEFAULT_POSITION_AFFINITY,
            },
        }),
        history_policy: HistoryPolicy::Skip,
    }))
}

fn outer_cell_anchor(index: &TableProjectionIndex, caret: u32) -> Option<TableAnchor> {
    let located = outer_cell_containing(index, caret)?;
    Some(TableAnchor {
        table_pos: located.table_pos,
        anchors: CellAnchorPair {
            anchor: located.cell_pos,
            head: located.cell_pos,
        },
    })
}

fn appendable_outer_row<'a>(
    document: &'a Document,
    index: &TableProjectionIndex,
    schema: &Schema,
    anchor: &TableAnchor,
) -> Option<TableTarget<'a>> {
    let target = TableTarget::resolve_in(
        document,
        index,
        anchor.table_pos,
        Some(anchor.anchors),
        schema,
        GridRequirement::Regular,
    )?;
    rows::plan_insert_row(&target, TableEdge::After, schema).map(|_| target)
}

fn move_to_adjacent_cell(
    context: &PlanningContext<'_>,
    selection: &Selection,
    step: CellStep,
    append_row: bool,
) -> OperationResult<CommandPlan> {
    let Some(caret) = navigating_caret(selection) else {
        return Ok(CommandPlan::NotApplicable);
    };
    let index = TableProjectionIndex::derive_or_fallback(
        context.document,
        context.schema,
        context.resource_limits,
    );
    let Some(anchor) = outer_cell_anchor(&index, caret) else {
        return Ok(CommandPlan::NotApplicable);
    };
    let mut cursor = caret;
    while let Some(next) = next_outer_cell(&index, cursor, step) {
        if let Some(interior) =
            first_editable_position_in_cell(context.document, context.schema, next)
        {
            return caret_only(context, interior);
        }
        cursor = next;
    }
    match (step, append_row) {
        (CellStep::Forward, true) => {
            if appendable_outer_row(context.document, &index, context.schema, &anchor).is_none() {
                return Ok(CommandPlan::NotApplicable);
            }
            scoped_action(
                context,
                &anchor,
                selection,
                &InsertRowAction {
                    side: TableEdge::After,
                },
            )
        }
        (CellStep::Forward, false) | (CellStep::Backward, _) => Ok(CommandPlan::NotApplicable),
    }
}

pub(crate) struct TableCommandSurface<'a> {
    document: &'a Document,
    schema: &'a Schema,
    limits: &'a ResourceLimits,
    selection: &'a Selection,
    anchor: Option<TableAnchor>,
    target: Option<TableTarget<'a>>,
    index: TableProjectionIndex,
}

impl<'a> TableCommandSurface<'a> {
    pub(crate) fn resolve(
        document: &'a Document,
        schema: &'a Schema,
        selection: &'a Selection,
        limits: &'a ResourceLimits,
    ) -> Self {
        let index = TableProjectionIndex::derive_or_fallback(document, schema, limits);
        let anchor = anchor_in(&index, selection);
        let target = anchor.as_ref().and_then(|anchor| {
            TableTarget::resolve_in(
                document,
                &index,
                anchor.table_pos,
                Some(anchor.anchors),
                schema,
                GridRequirement::AsProjected,
            )
        });
        Self {
            document,
            schema,
            limits,
            selection,
            anchor,
            target,
            index,
        }
    }

    fn regular_target(&self) -> Option<&TableTarget<'a>> {
        self.target.as_ref().filter(|target| target.is_regular())
    }

    pub(crate) fn is_available(&self, command: TableCommand) -> bool {
        match command {
            TableCommand::InsertTable {
                rows,
                columns,
                with_header_row,
            } => {
                self.anchor.is_none()
                    && plan_insert_table(
                        self.document,
                        self.schema,
                        self.selection,
                        self.limits,
                        rows,
                        columns,
                        with_header_row,
                    )
                    .is_some()
            }
            TableCommand::DeleteTable => self.anchor.as_ref().is_some_and(|anchor| {
                plan_delete_table(self.document, self.schema, anchor.table_pos).is_some()
            }),
            TableCommand::AddTableRow { side } => self
                .regular_target()
                .is_some_and(|target| rows::plan_insert_row(target, side, self.schema).is_some()),
            TableCommand::DeleteTableRows => self.regular_target().is_some_and(|target| {
                rows::plan_delete_rows(self.document, target, self.schema, self.limits).is_some()
            }),
            TableCommand::AddTableColumn { side } => self.regular_target().is_some_and(|target| {
                columns::plan_insert_column(target, side, self.schema).is_some()
            }),
            TableCommand::DeleteTableColumns => self.regular_target().is_some_and(|target| {
                columns::plan_delete_columns(self.document, target, self.schema, self.limits)
                    .is_some()
            }),
            TableCommand::ToggleTableHeader { target: header } => {
                self.regular_target().is_some_and(|target| {
                    headers::plan_toggle_header(target, header, self.schema, self.selection)
                        .is_some()
                })
            }
            TableCommand::SelectTableRows => self
                .target
                .as_ref()
                .is_some_and(|target| plan_select_rows(target).is_some()),
            TableCommand::SelectTableColumns => self
                .target
                .as_ref()
                .is_some_and(|target| plan_select_columns(target).is_some()),
            TableCommand::ClearTableCells => self
                .target
                .as_ref()
                .is_some_and(|target| plan_clear_cells(target, self.schema).is_some()),
            TableCommand::MergeTableCells => self
                .regular_target()
                .is_some_and(|target| merge::plan_merge_cells(target, self.schema).is_some()),
            TableCommand::SplitTableCell => self
                .regular_target()
                .is_some_and(|target| merge::plan_split_cell(target, self.schema).is_some()),
            TableCommand::SetTableColumnWidth { .. } => {
                self.regular_target().is_some_and(|target| {
                    match resize::can_set_column_width(target) {
                        Ok(resizable) => resizable,
                        Err(
                            TableError::GridLimit { .. }
                            | TableError::WorkLimit
                            | TableError::Allocation
                            | TableError::InvalidStructure
                            | TableError::InvalidAttributes,
                        ) => UNREADABLE_GRID_IS_NOT_AVAILABLE,
                    }
                })
            }
            TableCommand::MoveToAdjacentCell { step, append_row } => {
                let Some(caret) = navigating_caret(self.selection) else {
                    return false;
                };
                let Some(anchor) = outer_cell_anchor(&self.index, caret) else {
                    return false;
                };
                if next_outer_cell(&self.index, caret, step).is_some() {
                    return true;
                }
                match (step, append_row) {
                    (CellStep::Forward, true) => {
                        appendable_outer_row(self.document, &self.index, self.schema, &anchor)
                            .is_some()
                    }
                    (CellStep::Forward, false) | (CellStep::Backward, _) => false,
                }
            }
        }
    }
}
