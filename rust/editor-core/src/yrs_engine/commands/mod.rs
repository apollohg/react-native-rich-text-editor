//! Serialization-ready editor commands and pure typed-transaction planning.

mod clipboard;
mod format;
mod structural_batch;
mod structure;
mod text;

use std::collections::HashMap;

use crate::model::{Document, Mark};
use crate::position::PositionMap;
use crate::schema::Schema;
use crate::selection::Selection;

use super::{
    OperationError, OperationResult, ResolvedSelection, RevisionedPosition, RevisionedRange,
    SelectionInput, TransactionOrigin, TypedOperation, TypedTransaction,
};

const TABLE_ACTION_ORIGIN_FIELD: &str = "origin";

#[derive(Debug, Clone, PartialEq)]
pub enum TypedCommand {
    InsertText {
        text: String,
    },
    DeleteRange {
        range: RevisionedRange,
    },
    DeleteBackward,
    ReplaceSelectionText {
        text: String,
    },
    Paste {
        fragment: Option<String>,
        html: Option<String>,
        text: Option<String>,
        plain_text: bool,
        allow_base64_images: bool,
        input_filter: Option<String>,
    },
    SplitBlock,
    DeleteAndSplit,
    InsertContentJson {
        json: serde_json::Value,
    },
    InsertContentHtml {
        html: String,
    },
    ToggleMark {
        mark_type: String,
    },
    SetMark {
        mark_type: String,
        attrs: HashMap<String, serde_json::Value>,
    },
    UnsetMark {
        mark_type: String,
    },
    ToggleHeading {
        level: u8,
    },
    ToggleCodeBlock,
    ToggleBlockquote,
    ApplyListType {
        list_type: String,
    },
    WrapInList {
        list_type: String,
        item_type: String,
    },
    UnwrapFromList,
    IndentListItem,
    OutdentListItem,
    ToggleTaskItemChecked,
    InsertNode {
        node_type: String,
    },
    UpdateNodeAttrs {
        doc_pos: u32,
        attrs: HashMap<String, serde_json::Value>,
    },
    ResizeImage {
        at: RevisionedPosition,
        width: u32,
        height: u32,
    },
    MoveSelection {
        range: RevisionedRange,
        at: RevisionedPosition,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandPlan {
    NotApplicable,
    Transaction(TypedTransaction),
    SelectionOnly(TypedTransaction),
}

pub(crate) struct PreparedCommandProof {
    pub document: Document,
    pub selection: Selection,
    pub execution_admission: crate::yrs_engine::prepared_admission::ExecutionSemanticAdmission,
}

#[cfg(test)]
impl PreparedCommandProof {
    pub(crate) fn eager_semantic_admission_mut_for_test(
        &mut self,
    ) -> &mut crate::yrs_engine::compiler::PreparedSemanticAdmission {
        let crate::yrs_engine::prepared_admission::ExecutionSemanticAdmission::Eager(admission) =
            &mut self.execution_admission
        else {
            panic!("test requires an eager prepared semantic admission")
        };
        admission
    }
}

pub(crate) struct PlanningContext<'a> {
    pub request_id: u64,
    pub revision: u64,
    pub state_revision: u64,
    pub document: &'a Document,
    pub position_map: &'a PositionMap,
    pub rendered_text: &'a str,
    pub selection: &'a ResolvedSelection,
    pub initial_selection: Option<&'a SelectionInput>,
    pub origin: TransactionOrigin,
    pub stored_marks: Option<&'a [Mark]>,
    pub schema: &'a Schema,
    pub resource_limits: &'a crate::boundary::ResourceLimits,
    pub editing_limits: &'a crate::yrs_engine::EditingLimits,
    pub max_length: Option<u32>,
    pub yrs_state_epoch: u64,
    pub canonical_schema: &'a crate::yrs_engine::canonical::CanonicalSchemaContext,
    pub canonical_artifact: &'a crate::yrs_engine::canonical::CanonicalArtifact,
    pub allow_deferred_admission: bool,
    pub preparation: Option<&'a std::cell::RefCell<Option<PreparedCommandProof>>>,
}

pub(crate) fn plan(
    context: PlanningContext<'_>,
    command: TypedCommand,
) -> OperationResult<CommandPlan> {
    match command {
        command @ (TypedCommand::InsertText { .. }
        | TypedCommand::DeleteRange { .. }
        | TypedCommand::DeleteBackward
        | TypedCommand::ReplaceSelectionText { .. }
        | TypedCommand::Paste { .. }
        | TypedCommand::SplitBlock
        | TypedCommand::DeleteAndSplit
        | TypedCommand::InsertContentJson { .. }
        | TypedCommand::InsertContentHtml { .. }) => text::plan(context, command),
        command @ (TypedCommand::ToggleMark { .. }
        | TypedCommand::SetMark { .. }
        | TypedCommand::UnsetMark { .. }
        | TypedCommand::ToggleHeading { .. }
        | TypedCommand::ToggleCodeBlock
        | TypedCommand::ToggleBlockquote) => format::plan(context, command),
        command @ (TypedCommand::ApplyListType { .. }
        | TypedCommand::WrapInList { .. }
        | TypedCommand::UnwrapFromList
        | TypedCommand::IndentListItem
        | TypedCommand::OutdentListItem
        | TypedCommand::ToggleTaskItemChecked
        | TypedCommand::InsertNode { .. }
        | TypedCommand::UpdateNodeAttrs { .. }
        | TypedCommand::ResizeImage { .. }
        | TypedCommand::MoveSelection { .. }) => structure::plan(context, command),
    }
}

#[allow(dead_code)]
pub(crate) fn table_action_transaction(
    context: &PlanningContext<'_>,
    prepared: crate::tables::command_context::PreparedTableAction,
) -> OperationResult<CommandPlan> {
    if context.origin != prepared.origin {
        return Err(OperationError::transaction_invalid(
            context.request_id,
            TABLE_ACTION_ORIGIN_FIELD,
            "table normalization belongs to a trusted local command only",
        ));
    }
    if context.revision != prepared.base_document_revision {
        return Err(OperationError::revision_mismatch(
            context.request_id,
            prepared.base_document_revision,
            context.revision,
        ));
    }
    let selection = structure::selection(context);
    let plan = text::admitted_semantic_transaction(context, &selection, prepared.plan)?;
    let CommandPlan::Transaction(transaction) = &plan else {
        return Ok(plan);
    };
    if transaction
        .operations
        .iter()
        .any(|operation| matches!(operation, TypedOperation::ReplaceStructure(_)))
    {
        return Err(OperationError::engine_invariant_failed(
            context.request_id,
            None,
            "a table action cannot be lowered without replacing its whole table",
        ));
    }
    Ok(plan)
}

#[cfg(test)]
pub(crate) struct TableActionTestRequest<'a> {
    pub document: &'a Document,
    pub schema: &'a Schema,
    pub resource_limits: &'a crate::boundary::ResourceLimits,
    pub editing_limits: &'a crate::yrs_engine::EditingLimits,
    pub revision: u64,
    pub state_revision: u64,
    pub yrs_state_epoch: u64,
    pub origin: TransactionOrigin,
}

#[cfg(test)]
pub(crate) fn table_action_plan_for_test(
    request: TableActionTestRequest<'_>,
    prepared: crate::tables::command_context::PreparedTableAction,
) -> OperationResult<CommandPlan> {
    let position_map = PositionMap::build(request.document, request.schema);
    let rendered_text = crate::render::rendered_text(request.document, request.schema);
    let canonical_schema =
        crate::yrs_engine::canonical::CanonicalSchemaContext::new(request.schema);
    let canonical_artifact = canonical_schema
        .derive(request.document)
        .expect("the table action fixture is canonical");
    let point = crate::yrs_engine::ResolvedPoint {
        document: 0,
        scalar: 0,
        utf16: 0,
    };
    let selection = ResolvedSelection::Text {
        anchor: point,
        head: point,
    };
    let context = PlanningContext {
        request_id: prepared.request_id,
        revision: request.revision,
        state_revision: request.state_revision,
        document: request.document,
        position_map: &position_map,
        rendered_text: &rendered_text,
        selection: &selection,
        initial_selection: None,
        origin: request.origin,
        stored_marks: None,
        schema: request.schema,
        resource_limits: request.resource_limits,
        editing_limits: request.editing_limits,
        max_length: None,
        yrs_state_epoch: request.yrs_state_epoch,
        canonical_schema: &canonical_schema,
        canonical_artifact: &canonical_artifact,
        allow_deferred_admission: false,
        preparation: None,
    };
    table_action_transaction(&context, prepared)
}
