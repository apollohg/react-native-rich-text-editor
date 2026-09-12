use crate::boundary::ResourceLimits;
use crate::command_planner::{apply_operations, SemanticOperation};
use crate::model::{Document, Node};
use crate::schema::Schema;
use crate::tables::roles::TableRoles;
use crate::tables::types::TableError;
use crate::yrs_engine::{OperationError, TransactionOrigin};

const NESTED_TABLE_FIELD: &str = "nestedTable";
const CELL_BOUNDARY_FIELD: &str = "tableCellBoundary";
const LOCAL_MUTATION_WORK_FIELD: &str = "nestedTableAdmissionWork";
const NODE_OPENING_TOKENS: u32 = 1;
const NODE_CLOSING_TOKENS: u32 = 1;
const DOCUMENT_CONTENT_START: u32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LocalMutationRefusal {
    NestedTableDescendant,
    CellBoundaryJoin,
    Unreplayable,
    Unreadable(TableError),
}

impl LocalMutationRefusal {
    pub(crate) fn into_operation_error(self, request_id: u64) -> OperationError {
        match self {
            Self::NestedTableDescendant => OperationError::operation_invalid(
                request_id,
                0,
                NESTED_TABLE_FIELD,
                "a nested table's content is never locally editable",
            ),
            Self::CellBoundaryJoin => OperationError::operation_invalid(
                request_id,
                0,
                CELL_BOUNDARY_FIELD,
                "blocks on opposite sides of a table cell boundary are never joined",
            ),
            Self::Unreplayable => OperationError::engine_invariant_failed(
                request_id,
                None,
                "a local candidate could not be replayed for nested table admission",
            ),
            Self::Unreadable(error) => OperationError::operation_work_budget_exceeded(
                request_id,
                LOCAL_MUTATION_WORK_FIELD,
                error.to_string(),
            ),
        }
    }
}

pub(crate) fn admit_local_mutation(
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
    origin: TransactionOrigin,
    operations: &[SemanticOperation],
) -> Result<(), LocalMutationRefusal> {
    match origin {
        TransactionOrigin::LocalInput
        | TransactionOrigin::LocalCommand
        | TransactionOrigin::LocalApi => {}
        TransactionOrigin::UndoRedo
        | TransactionOrigin::RemoteSync
        | TransactionOrigin::SnapshotRestore
        | TransactionOrigin::DocumentImport => return Ok(()),
    }
    let roles = match TableRoles::resolve(schema) {
        Ok(Some(roles)) => roles,
        Ok(None) => return Ok(()),
        Err(error) => return Err(LocalMutationRefusal::Unreadable(error)),
    };
    let mut replayed: Option<Document> = None;
    let mut remaining = operations;
    while let Some((operation, rest)) = remaining.split_first() {
        let candidate = replayed.as_ref().unwrap_or(document);
        let geography = TableGeography::survey(candidate, &roles, limits)
            .map_err(LocalMutationRefusal::Unreadable)?;
        geography.admit(operation)?;
        if rest.is_empty() {
            return Ok(());
        }
        replayed = Some(
            apply_operations(candidate, schema, std::slice::from_ref(operation))
                .map_err(|()| LocalMutationRefusal::Unreplayable)?,
        );
        remaining = rest;
    }
    Ok(())
}

#[derive(Default)]
struct TableGeography {
    nested_tables: Vec<(u32, u32)>,
    cell_contents: Vec<(u32, u32)>,
}

enum OperationExtent {
    Point(u32),
    NodeTarget(u32),
    Join(u32),
    Range(u32, u32),
}

impl TableGeography {
    fn survey(
        document: &Document,
        roles: &TableRoles,
        limits: &ResourceLimits,
    ) -> Result<Self, TableError> {
        let mut geography = Self::default();
        let mut visited = 0usize;
        let mut pending: Vec<(&Node, u32, bool)> =
            vec![(document.root(), DOCUMENT_CONTENT_START, false)];
        while let Some((node, content_start, inside_table)) = pending.pop() {
            let Some(content) = node.content() else {
                continue;
            };
            let mut position = content_start;
            for child in content.iter() {
                visited = visited.saturating_add(1);
                if visited > limits.max_document_nodes {
                    return Err(TableError::WorkLimit);
                }
                let end = position
                    .checked_add(child.node_size())
                    .ok_or(TableError::Allocation)?;
                let child_is_a_table = child.node_type() == roles.table;
                if child_is_a_table && inside_table {
                    geography.nested_tables.push((position, end));
                }
                if child.node_type() == roles.cell || child.node_type() == roles.header_cell {
                    geography.cell_contents.push((
                        position
                            .checked_add(NODE_OPENING_TOKENS)
                            .ok_or(TableError::Allocation)?,
                        end.checked_sub(NODE_CLOSING_TOKENS)
                            .ok_or(TableError::Allocation)?,
                    ));
                }
                if child.content().is_some() {
                    pending.push((
                        child,
                        position
                            .checked_add(NODE_OPENING_TOKENS)
                            .ok_or(TableError::Allocation)?,
                        inside_table || child_is_a_table,
                    ));
                }
                position = end;
            }
        }
        Ok(geography)
    }

    fn admit(&self, operation: &SemanticOperation) -> Result<(), LocalMutationRefusal> {
        let extent = extent_of(operation);
        for (start, end) in self.nested_tables.iter().copied() {
            let refused = match extent {
                OperationExtent::Point(pos) | OperationExtent::Join(pos) => {
                    start < pos && pos < end
                }
                OperationExtent::NodeTarget(pos) => start <= pos && pos < end,
                OperationExtent::Range(from, to) => {
                    from < end && to > start && !(from <= start && to >= end)
                }
            };
            if refused {
                return Err(LocalMutationRefusal::NestedTableDescendant);
            }
        }
        for (start, end) in self.cell_contents.iter().copied() {
            let refused = match extent {
                OperationExtent::Join(pos) => pos == start || pos == end,
                OperationExtent::Range(from, to) => {
                    within_cell_content(from, start, end) != within_cell_content(to, start, end)
                }
                OperationExtent::Point(_) | OperationExtent::NodeTarget(_) => false,
            };
            if refused {
                return Err(LocalMutationRefusal::CellBoundaryJoin);
            }
        }
        Ok(())
    }
}

fn within_cell_content(position: u32, start: u32, end: u32) -> bool {
    start <= position && position <= end
}

fn extent_of(operation: &SemanticOperation) -> OperationExtent {
    match operation {
        SemanticOperation::InsertText { pos, .. }
        | SemanticOperation::SplitBlock { pos, .. }
        | SemanticOperation::UnwrapFromList { pos }
        | SemanticOperation::OutdentListItem { pos }
        | SemanticOperation::IndentListItem { pos }
        | SemanticOperation::InsertNode { pos, .. } => OperationExtent::Point(*pos),
        SemanticOperation::UpdateNodeAttrs { pos, .. } => OperationExtent::NodeTarget(*pos),
        SemanticOperation::JoinBlocks { pos } => OperationExtent::Join(*pos),
        SemanticOperation::DeleteRange { from, to }
        | SemanticOperation::AddMark { from, to, .. }
        | SemanticOperation::RemoveMark { from, to, .. }
        | SemanticOperation::ReplaceMark { from, to, .. }
        | SemanticOperation::ReplaceRange { from, to, .. }
        | SemanticOperation::WrapInList { from, to, .. } => OperationExtent::Range(*from, *to),
    }
}
