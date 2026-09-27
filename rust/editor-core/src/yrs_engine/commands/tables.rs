use super::{CommandPlan, PlanningContext, TypedCommand};
use crate::boundary::ResourceLimits;
use crate::command_planner::{
    apply_operations_mapped, simulate_plan, AdmittedSemanticCommandPlan, SemanticCommandPlan,
};
use crate::model::Document;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::command_context::{
    is_action_unavailable, prepare_table_action, CellAnchorPair, PreparedTableAction, TableAction,
    TableActionContext,
};
use crate::tables::commands::paste::MatrixPasteAction;
use crate::tables::commands::{
    cells_are_cleared, columns, headers, merge, plan_clear_cells, plan_delete_table,
    plan_insert_table, plan_select_columns, plan_select_rows, resize, rows, DeleteColumnsAction,
    DeleteRowsAction, GridRequirement, InsertColumnAction, InsertRowAction, MergeCellsAction,
    SetColumnWidthAction, SplitCellAction, TableCommand, TableEdge, TableTarget,
    ToggleHeaderAction,
};
use crate::tables::interchange::{
    first_editable_position_in_cell, next_outer_cell, outer_cell_containing, CellStep,
    InterchangeFailure,
};
use crate::tables::normalize::{
    collect_outer_table_positions, outer_table_positions, NormalizationFailure,
};
use crate::tables::paste::TableMatrix;
use crate::tables::selection::{cell_opening_containing, resolve_cell_rect};
use crate::tables::types::TableError;
use crate::yrs_engine::derived_state::resolved_from_legacy_with_view;
use crate::yrs_engine::{
    HistoryPolicy, MovedTableCells, OperationError, OperationResult, ResolvedSelection,
    SelectionIntent, TypedTransaction,
};

const CLEAR_CELLS_FIELD: &str = "clearTableCells";
const EXPLICIT_TABLE_COLUMN_FIELD: &str = "column";
const UNREADABLE_GRID_IS_NOT_AVAILABLE: bool = false;
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
    prepared_action(
        context,
        anchor.table_pos,
        Some(anchor.anchors),
        selection,
        action,
    )
}

fn prepared_action(
    context: &PlanningContext<'_>,
    table_pos: u32,
    anchors: Option<CellAnchorPair>,
    selection: &Selection,
    action: &dyn TableAction,
) -> OperationResult<CommandPlan> {
    match prepared_action_on(
        context,
        context.document,
        table_pos,
        anchors,
        selection,
        action,
    )? {
        None => Ok(CommandPlan::NotApplicable),
        Some(prepared) => super::table_action_transaction(context, prepared),
    }
}

fn prepared_action_on(
    context: &PlanningContext<'_>,
    document: &Document,
    table_pos: u32,
    anchors: Option<CellAnchorPair>,
    selection: &Selection,
    action: &dyn TableAction,
) -> OperationResult<Option<PreparedTableAction>> {
    let prepared = prepare_table_action(
        &TableActionContext {
            request_id: context.request_id,
            base_document_revision: context.revision,
            table_pos,
            anchors,
            schema: context.schema,
            resource_limits: context.resource_limits,
            editing_limits: context.editing_limits,
            document,
            selection,
        },
        action,
    );
    match prepared {
        Err(error) if is_action_unavailable(&error) => Ok(None),
        other => other.map(Some),
    }
}

pub(super) fn outer_paste_anchor(
    context: &PlanningContext<'_>,
    selection: &Selection,
) -> OperationResult<Option<TableAnchor>> {
    let Some(anchor) = anchor_from_selection(
        context.document,
        context.schema,
        context.resource_limits,
        selection,
    ) else {
        return Ok(None);
    };
    let outer = outer_table_positions(context.document, context.schema, context.resource_limits)?
        .contains(&anchor.table_pos);
    Ok(outer.then_some(anchor))
}

pub(super) fn paste_matrix(
    context: &PlanningContext<'_>,
    anchor: &TableAnchor,
    selection: &Selection,
    matrix: TableMatrix,
) -> OperationResult<CommandPlan> {
    scoped_action(context, anchor, selection, &MatrixPasteAction { matrix })
}

pub(super) fn cell_drop_selection(
    context: &PlanningContext<'_>,
    target_cell: u32,
) -> OperationResult<Option<ResolvedSelection>> {
    let index = TableProjectionIndex::derive_or_fallback(
        context.document,
        context.schema,
        context.resource_limits,
    );
    let Some(rect) = resolve_cell_rect(&index, target_cell, target_cell) else {
        return Ok(None);
    };
    if !outer_table_positions(context.document, context.schema, context.resource_limits)?
        .contains(&rect.table_pos)
    {
        return Ok(None);
    }
    let Some(caret) =
        first_editable_position_in_cell(context.document, context.schema, target_cell)
            .map_err(|failure| failure.into_operation_error(context.request_id))?
    else {
        return Ok(None);
    };
    if cell_opening_containing(&index, caret) != Some(target_cell) {
        return Ok(None);
    }
    Ok(resolved_from_legacy_with_view(
        context.document,
        &Selection::text(caret, caret),
        context.schema,
        context.position_map,
        context.rendered_text,
        &index,
    ))
}

pub(super) fn move_matrix(
    context: &PlanningContext<'_>,
    moved: MovedTableCells,
    anchor: &TableAnchor,
    selection: &Selection,
    matrix: TableMatrix,
) -> OperationResult<CommandPlan> {
    let Some(source) = outer_paste_anchor(
        context,
        &Selection::cell(moved.anchor_cell, moved.head_cell),
    )?
    else {
        return Ok(CommandPlan::NotApplicable);
    };
    let Some(source_target) = anchored_target(
        context.document,
        context.schema,
        context.resource_limits,
        Some(&source),
        GridRequirement::AsProjected,
    ) else {
        return Ok(CommandPlan::NotApplicable);
    };
    let cleared = match plan_clear_cells(&source_target, context.schema) {
        Some(plan) => plan.operations,
        None if cells_are_cleared(&source_target, context.schema) => Vec::new(),
        None => return Ok(CommandPlan::NotApplicable),
    };
    let Ok((document, map)) = apply_operations_mapped(context.document, context.schema, &cleared)
    else {
        return Ok(CommandPlan::NotApplicable);
    };
    let Some(mut prepared) = prepared_action_on(
        context,
        &document,
        map.map_pos(anchor.table_pos),
        Some(CellAnchorPair {
            anchor: map.map_pos(anchor.anchors.anchor),
            head: map.map_pos(anchor.anchors.head),
        }),
        &selection.map(&map),
        &MatrixPasteAction { matrix },
    )?
    else {
        return Ok(CommandPlan::NotApplicable);
    };
    let pasted = std::mem::take(&mut prepared.plan.plan.operations);
    prepared.plan.plan.operations = cleared.into_iter().chain(pasted).collect();
    super::table_action_transaction(context, prepared)
}

fn explicit_table_resize(
    context: &PlanningContext<'_>,
    table_pos: u32,
    column: Option<u32>,
    width: u32,
    selection: &Selection,
) -> OperationResult<CommandPlan> {
    let Some(column) = column else {
        return Err(OperationError::operation_invalid(
            context.request_id,
            TABLE_COMMAND_OPERATION_INDEX,
            EXPLICIT_TABLE_COLUMN_FIELD,
            "an explicit table position requires an explicit column",
        ));
    };
    if !outer_table_positions(context.document, context.schema, context.resource_limits)?
        .contains(&table_pos)
    {
        return Ok(CommandPlan::NotApplicable);
    }
    prepared_action(
        context,
        table_pos,
        None,
        selection,
        &SetColumnWidthAction {
            width,
            column: Some(column),
        },
    )
}

fn explicit_table_deletion(
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
    table_pos: u32,
    selection: &Selection,
) -> Result<Option<SemanticCommandPlan>, TableError> {
    if !collect_outer_table_positions(document, schema, limits)?.contains(&table_pos) {
        return Ok(None);
    }
    Ok(plan_delete_table(document, schema, table_pos, selection))
}

fn available_when_readable(answer: Result<bool, TableError>) -> bool {
    match answer {
        Ok(available) => available,
        Err(
            TableError::GridLimit { .. }
            | TableError::WorkLimit
            | TableError::Allocation
            | TableError::InvalidStructure
            | TableError::InvalidAttributes,
        ) => UNREADABLE_GRID_IS_NOT_AVAILABLE,
    }
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

fn cell_selection_intent(expanded: &Selection) -> Option<crate::yrs_engine::SelectionInput> {
    let Selection::Cell { anchor, head } = expanded else {
        return None;
    };
    Some(crate::yrs_engine::SelectionInput::Cell {
        anchor: crate::yrs_engine::CellSelectionPoint::Document {
            opening: *anchor,
            affinity: crate::yrs_engine::DEFAULT_POSITION_AFFINITY,
        },
        head: crate::yrs_engine::CellSelectionPoint::Document {
            opening: *head,
            affinity: crate::yrs_engine::DEFAULT_POSITION_AFFINITY,
        },
    })
}

fn selection_only(
    context: &PlanningContext<'_>,
    expanded: Selection,
) -> OperationResult<CommandPlan> {
    let Some(intent) = cell_selection_intent(&expanded) else {
        return Ok(CommandPlan::NotApplicable);
    };
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
        TableCommand::DeleteTable { table_pos } => {
            let plan = match (table_pos, anchor) {
                (Some(table_pos), _) => explicit_table_deletion(
                    context.document,
                    context.schema,
                    context.resource_limits,
                    table_pos,
                    &selection,
                )
                .map_err(|error| {
                    NormalizationFailure::from(error).into_operation_error(context.request_id)
                })?,
                (None, None) => None,
                (None, Some(anchor)) => plan_delete_table(
                    context.document,
                    context.schema,
                    anchor.table_pos,
                    &selection,
                ),
            };
            semantic(&context, &selection, plan)
        }
        TableCommand::AddTableRow { side } => match anchor {
            None => Ok(CommandPlan::NotApplicable),
            Some(anchor) => scoped_action(
                &context,
                &anchor,
                &selection,
                &InsertRowAction {
                    side,
                    enters_inserted_row: false,
                },
            ),
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
        TableCommand::SetTableColumnWidth {
            width,
            column,
            table_pos,
        } => match (table_pos, anchor) {
            (Some(table_pos), _) => {
                explicit_table_resize(&context, table_pos, column, width, &selection)
            }
            (None, None) => Ok(CommandPlan::NotApplicable),
            (None, Some(anchor)) => scoped_action(
                &context,
                &anchor,
                &selection,
                &SetColumnWidthAction { width, column },
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

fn next_editable_outer_cell(
    document: &Document,
    index: &TableProjectionIndex,
    schema: &Schema,
    caret: u32,
    step: CellStep,
) -> Result<Option<u32>, InterchangeFailure> {
    let mut cursor = caret;
    while let Some(next) = next_outer_cell(index, cursor, step) {
        if let Some(interior) = first_editable_position_in_cell(document, schema, next)? {
            return Ok(Some(interior));
        }
        cursor = next;
    }
    Ok(None)
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

fn last_row_anchor(index: &TableProjectionIndex, table_pos: u32) -> Option<TableAnchor> {
    let table = index.table_at(table_pos)?;
    let cell_pos = table
        .cells
        .iter()
        .find(|cell| cell.rect.row.checked_add(cell.rect.rowspan) == Some(table.rows))
        .map(|cell| cell.source_pos)?;
    Some(TableAnchor {
        table_pos,
        anchors: CellAnchorPair {
            anchor: cell_pos,
            head: cell_pos,
        },
    })
}

fn appendable_outer_row(
    document: &Document,
    index: &TableProjectionIndex,
    schema: &Schema,
    anchor: &TableAnchor,
) -> Option<TableAnchor> {
    let trailing = last_row_anchor(index, anchor.table_pos)?;
    let target = TableTarget::resolve_in(
        document,
        index,
        trailing.table_pos,
        Some(trailing.anchors),
        schema,
        GridRequirement::Regular,
    )?;
    rows::plan_insert_row(&target, TableEdge::After, schema).map(|_| trailing)
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
    let landing = next_editable_outer_cell(context.document, &index, context.schema, caret, step)
        .map_err(|failure| failure.into_operation_error(context.request_id))?;
    if let Some(interior) = landing {
        return caret_only(context, interior);
    }
    match (step, append_row) {
        (CellStep::Forward, true) => {
            let Some(trailing) =
                appendable_outer_row(context.document, &index, context.schema, &anchor)
            else {
                return Ok(CommandPlan::NotApplicable);
            };
            scoped_action(
                context,
                &trailing,
                selection,
                &InsertRowAction {
                    side: TableEdge::After,
                    enters_inserted_row: true,
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
            TableCommand::DeleteTable { table_pos } => match (table_pos, &self.anchor) {
                (Some(table_pos), _) => available_when_readable(
                    explicit_table_deletion(
                        self.document,
                        self.schema,
                        self.limits,
                        table_pos,
                        self.selection,
                    )
                    .map(|plan| plan.is_some()),
                ),
                (None, None) => false,
                (None, Some(anchor)) => {
                    plan_delete_table(self.document, self.schema, anchor.table_pos, self.selection)
                        .is_some()
                }
            },
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
            TableCommand::SplitTableCell => self.regular_target().is_some_and(|target| {
                merge::plan_split_cell(target, self.schema, self.selection).is_some()
            }),
            TableCommand::SetTableColumnWidth { .. } => {
                self.regular_target().is_some_and(|target| {
                    available_when_readable(resize::can_set_column_width(target))
                })
            }
            TableCommand::MoveToAdjacentCell { step, append_row } => {
                let Some(caret) = navigating_caret(self.selection) else {
                    return false;
                };
                let Some(anchor) = outer_cell_anchor(&self.index, caret) else {
                    return false;
                };
                match next_editable_outer_cell(self.document, &self.index, self.schema, caret, step)
                {
                    Ok(Some(_)) => return true,
                    Ok(None) => {}
                    Err(
                        InterchangeFailure::NotACellRectangle | InterchangeFailure::UnreadableGrid,
                    ) => return UNREADABLE_GRID_IS_NOT_AVAILABLE,
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
