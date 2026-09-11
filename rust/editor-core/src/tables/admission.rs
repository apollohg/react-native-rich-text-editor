use std::collections::BTreeMap;

use crate::boundary::{BoundaryError, ResourceLimits};
use crate::model::{Document, Node};
use crate::schema::Schema;
use crate::tables::projection::{project_table, ProjectedTable, TableGridBudget};
use crate::tables::roles::TableRoles;
use crate::tables::types::TableError;

const NODE_OPENING_TOKENS: u32 = 1;
const DOCUMENT_CONTENT_START: u32 = 0;
const DOCUMENT_INVALID: &str = "DOCUMENT_INVALID";
const DOCUMENT_LIMIT_EXCEEDED: &str = "DOCUMENT_LIMIT_EXCEEDED";
const TABLE_GRID_PHASE: &str = "tableGrid";
const TABLE_SHAPE_PHASE: &str = "tableShape";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectionFailure {
    ResourceExhausted,
    Structural,
}

impl ProjectionFailure {
    fn of(error: &TableError) -> Self {
        match error {
            TableError::GridLimit { .. } | TableError::WorkLimit | TableError::Allocation => {
                Self::ResourceExhausted
            }
            TableError::InvalidStructure | TableError::InvalidAttributes => Self::Structural,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableProjectionIndex {
    tables: BTreeMap<u32, ProjectedTable>,
    projection_failure: Option<ProjectionFailure>,
}

impl TableProjectionIndex {
    pub(crate) fn empty() -> Self {
        Self {
            tables: BTreeMap::new(),
            projection_failure: None,
        }
    }

    fn fallback(failure: ProjectionFailure) -> Self {
        Self {
            tables: BTreeMap::new(),
            projection_failure: Some(failure),
        }
    }

    pub(crate) fn derive_or_fallback(
        document: &Document,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> Self {
        validate_table_shapes(document, schema, limits)
            .unwrap_or_else(|error| Self::fallback(ProjectionFailure::of(&error)))
    }
}

#[allow(dead_code)]
impl TableProjectionIndex {
    pub(crate) fn table_at(&self, position: u32) -> Option<&ProjectedTable> {
        self.tables.get(&position)
    }

    pub(crate) fn positions(&self) -> impl Iterator<Item = u32> + '_ {
        self.tables.keys().copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.tables.len()
    }

    pub(crate) fn projection_failed(&self) -> bool {
        self.projection_failure.is_some()
    }

    pub(crate) fn projection_failure(&self) -> Option<ProjectionFailure> {
        self.projection_failure
    }

    pub(crate) fn irregular_positions(&self) -> impl Iterator<Item = u32> + '_ {
        self.tables
            .iter()
            .filter(|(_, table)| table.irregular)
            .map(|(position, _)| *position)
    }
}

pub(crate) fn validate_table_shapes(
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<TableProjectionIndex, TableError> {
    let Some(roles) = TableRoles::resolve(schema)? else {
        return Ok(TableProjectionIndex::empty());
    };
    let mut budget = TableGridBudget::new(limits.max_table_grid_slots);
    let mut index = TableProjectionIndex::empty();
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
                return Err(TableError::WorkLimit);
            }
            if child.node_type() == roles.table {
                let projected = project_table(child, position, schema, &mut budget)?;
                if index.tables.insert(position, projected).is_some() {
                    return Err(TableError::InvalidStructure);
                }
            }
            if child.content().is_some() {
                pending.push((
                    child,
                    position
                        .checked_add(NODE_OPENING_TOKENS)
                        .ok_or(TableError::Allocation)?,
                ));
            }
            position = position
                .checked_add(child.node_size())
                .ok_or(TableError::Allocation)?;
        }
    }
    Ok(index)
}

pub(crate) fn table_shape_error(error: TableError) -> BoundaryError {
    match error {
        TableError::GridLimit { limit, actual } => {
            let mut boundary = BoundaryError::limit(DOCUMENT_LIMIT_EXCEEDED, limit, actual);
            boundary.details = Some(serde_json::json!({ "phase": TABLE_GRID_PHASE }));
            boundary
        }
        TableError::WorkLimit => {
            let mut boundary = BoundaryError::new(DOCUMENT_LIMIT_EXCEEDED, error.to_string());
            boundary.details = Some(serde_json::json!({ "phase": TABLE_SHAPE_PHASE }));
            boundary
        }
        TableError::InvalidStructure | TableError::InvalidAttributes | TableError::Allocation => {
            BoundaryError::new(DOCUMENT_INVALID, error.to_string())
        }
    }
}

pub(crate) fn admit_table_shapes(
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<TableProjectionIndex, BoundaryError> {
    validate_table_shapes(document, schema, limits).map_err(table_shape_error)
}
