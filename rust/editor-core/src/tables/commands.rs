use std::collections::HashMap;

use serde_json::Value;

use crate::boundary::ResourceLimits;
use crate::command_planner::{
    compatible_declared_attrs, default_attrs, empty_text_block_range, resolve_block_insert_pos,
    SemanticCommandHistory, SemanticCommandPlan, SemanticOperation,
};
use crate::model::{Document, Fragment, Node};
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::command_context::{
    CellAnchorPair, TableAction, TableActionCandidate, TableActionOutcome,
};
use crate::tables::projection::{integral_unsigned, span_attribute, ProjectedCell, ProjectedTable};
use crate::tables::roles::{
    TableRoles, MIN_TABLE_CELL_SPAN, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_COLWIDTH_ATTR,
    TABLE_CELL_ROWSPAN_ATTR,
};
use crate::tables::selection::{resolve_cell_rect, CellSelectionRect};
use crate::tables::types::{try_resize, TableActionKind};
use crate::yrs_engine::OperationResult;

pub(crate) mod columns;
pub(crate) mod headers;
pub(crate) mod merge;
pub(crate) mod resize;
pub(crate) mod rows;

pub(crate) const DEFAULT_INSERTED_TABLE_ROWS: u32 = 3;
pub(crate) const DEFAULT_INSERTED_TABLE_COLUMNS: u32 = 3;
pub(crate) const DEFAULT_INSERTED_TABLE_HEADER_ROW: bool = true;
pub(crate) const DEFAULT_TAB_APPENDS_A_ROW: bool = true;
pub(crate) const MIN_INSERTED_TABLE_DIMENSION: u32 = 1;
pub(crate) const MAX_INSERTED_TABLE_DIMENSION: u32 = 1_000;
pub(crate) const MIN_TABLE_COLUMN_WIDTH: u32 = 1;
pub(crate) const UNSPECIFIED_TABLE_COLUMN_WIDTH: u32 = MIN_TABLE_COLUMN_WIDTH;
pub(crate) const MAX_TABLE_COLUMN_WIDTH: u32 = 10_000;
pub(crate) const MINIMUM_SURVIVING_ROWS: u32 = 1;
pub(crate) const MINIMUM_SURVIVING_COLUMNS: u32 = 1;

pub(crate) const NODE_OPENING_TOKENS: u32 = 1;
pub(crate) const NODE_CLOSING_TOKENS: u32 = 1;
pub(crate) const ONE_SLOT: u32 = 1;
pub(crate) const FIRST_ROW: u32 = 0;
pub(crate) const FIRST_COLUMN: u32 = 0;
pub(crate) const FIRST_WIDTH_SLICE: u32 = 0;
const UNSET_COLUMN_WIDTH: u64 = 0;
const ONLY_CHILD: usize = 1;
const NO_CHILDREN: usize = 0;
pub(crate) const CELL_INTERIOR_OFFSET: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableEdge {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableHeaderTarget {
    Row,
    Column,
    Cell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCommand {
    InsertTable {
        rows: u32,
        columns: u32,
        with_header_row: bool,
    },
    DeleteTable,
    AddTableRow {
        side: TableEdge,
    },
    DeleteTableRows,
    AddTableColumn {
        side: TableEdge,
    },
    DeleteTableColumns,
    ToggleTableHeader {
        target: TableHeaderTarget,
    },
    SelectTableRows,
    SelectTableColumns,
    ClearTableCells,
    MergeTableCells,
    SplitTableCell,
    SetTableColumnWidth {
        width: u32,
    },
    MoveToAdjacentCell {
        step: crate::tables::interchange::CellStep,
        append_row: bool,
    },
}

pub(crate) struct TableTarget<'a> {
    table_pos: u32,
    table_node: &'a Node,
    projected: ProjectedTable,
    cell_nodes: Vec<&'a Node>,
    row_nodes: Vec<&'a Node>,
    row_starts: Vec<u32>,
    roles: TableRoles,
    rect: Option<CellSelectionRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GridRequirement {
    Regular,
    AsProjected,
}

impl<'a> TableTarget<'a> {
    pub(crate) fn resolve(
        document: &'a Document,
        table_pos: u32,
        anchors: Option<CellAnchorPair>,
        schema: &Schema,
        limits: &ResourceLimits,
        requirement: GridRequirement,
    ) -> Option<Self> {
        let index = TableProjectionIndex::derive_or_fallback(document, schema, limits);
        Self::resolve_in(document, &index, table_pos, anchors, schema, requirement)
    }

    pub(crate) fn resolve_in(
        document: &'a Document,
        index: &TableProjectionIndex,
        table_pos: u32,
        anchors: Option<CellAnchorPair>,
        schema: &Schema,
        requirement: GridRequirement,
    ) -> Option<Self> {
        let Ok(Some(roles)) = TableRoles::resolve(schema) else {
            return None;
        };
        let projected = index.table_at(table_pos)?.clone();
        match requirement {
            GridRequirement::Regular if projected.irregular => return None,
            GridRequirement::Regular | GridRequirement::AsProjected => {}
        }
        let rect = match anchors {
            None => None,
            Some(anchors) => Some(
                resolve_cell_rect(index, anchors.anchor, anchors.head)
                    .filter(|rect| rect.table_pos == table_pos)?,
            ),
        };
        let table = node_starting_at(document, table_pos)?;
        if table.node_type() != roles.table {
            return None;
        }

        let mut row_nodes = Vec::new();
        let mut row_starts = Vec::new();
        let mut cell_nodes = Vec::new();
        let mut row_pos = table_pos.checked_add(NODE_OPENING_TOKENS)?;
        for row in table.content()?.iter() {
            if row.node_type() != roles.row {
                return None;
            }
            row_starts.push(row_pos);
            row_nodes.push(row);
            for cell in row.content()?.iter() {
                if cell.node_type() != roles.cell && cell.node_type() != roles.header_cell {
                    return None;
                }
                cell_nodes.push(cell);
            }
            row_pos = row_pos.checked_add(row.node_size())?;
        }
        row_starts.push(row_pos);
        if cell_nodes.len() != projected.cells.len() {
            return None;
        }

        Some(Self {
            table_pos,
            table_node: table,
            projected,
            cell_nodes,
            row_nodes,
            row_starts,
            roles,
            rect,
        })
    }

    pub(crate) fn table_pos(&self) -> u32 {
        self.table_pos
    }

    pub(crate) fn table_node(&self) -> &'a Node {
        self.table_node
    }

    pub(crate) fn is_regular(&self) -> bool {
        !self.projected.irregular
    }

    pub(crate) fn rows(&self) -> u32 {
        self.projected.rows
    }

    pub(crate) fn columns(&self) -> u32 {
        self.projected.columns
    }

    pub(crate) fn rect(&self) -> Option<&CellSelectionRect> {
        self.rect.as_ref()
    }

    pub(crate) fn roles(&self) -> &TableRoles {
        &self.roles
    }

    fn slot(&self, row: u32, column: u32) -> Option<usize> {
        if row >= self.projected.rows || column >= self.projected.columns {
            return None;
        }
        let offset = (row as usize)
            .checked_mul(self.projected.columns as usize)?
            .checked_add(column as usize)?;
        self.projected.slots.get(offset).copied().flatten()
    }

    pub(crate) fn cell_at(&self, row: u32, column: u32) -> Option<(&ProjectedCell, &'a Node)> {
        let index = self.slot(row, column)?;
        Some((
            self.projected.cells.get(index)?,
            *self.cell_nodes.get(index)?,
        ))
    }

    pub(crate) fn covers_same_cell(&self, left: (u32, u32), right: (u32, u32)) -> bool {
        match (self.slot(left.0, left.1), self.slot(right.0, right.1)) {
            (Some(left), Some(right)) => left == right,
            (Some(_), None) | (None, Some(_)) | (None, None) => false,
        }
    }

    pub(crate) fn row_start(&self, row: u32) -> Option<u32> {
        self.row_starts.get(row as usize).copied()
    }

    pub(crate) fn row_node(&self, row: u32) -> Option<&'a Node> {
        self.row_nodes.get(row as usize).copied()
    }

    pub(crate) fn is_header_row(&self, row: u32) -> bool {
        self.projected.columns > 0
            && (FIRST_COLUMN..self.projected.columns).all(|column| self.is_header_cell(row, column))
    }

    pub(crate) fn is_header_column(&self, column: u32) -> bool {
        self.projected.rows > 0
            && (FIRST_ROW..self.projected.rows).all(|row| self.is_header_cell(row, column))
    }

    fn is_header_cell(&self, row: u32, column: u32) -> bool {
        self.cell_at(row, column)
            .is_some_and(|(_, node)| node.node_type() == self.roles.header_cell)
    }

    pub(crate) fn cell_type_at(&self, row: u32, column: u32) -> String {
        self.cell_at(row, column)
            .map(|(_, node)| node.node_type().to_owned())
            .unwrap_or_else(|| self.roles.cell.clone())
    }

    pub(crate) fn position_at(&self, row: u32, column: u32) -> Option<u32> {
        let row_start = self.row_start(row)?;
        for candidate in column..self.projected.columns {
            let Some((cell, _)) = self.cell_at(row, candidate) else {
                continue;
            };
            if cell.source_pos > row_start {
                return Some(cell.source_pos);
            }
        }
        row_start
            .checked_add(self.row_node(row)?.node_size())?
            .checked_sub(NODE_CLOSING_TOKENS)
    }

    pub(crate) fn cell_starting_at(&self, source_pos: u32) -> Option<&ProjectedCell> {
        self.projected
            .cells
            .iter()
            .find(|cell| cell.source_pos == source_pos)
    }

    pub(crate) fn cells_in_rectangle(
        &self,
        top: u32,
        left: u32,
        bottom: u32,
        right: u32,
    ) -> Vec<(&ProjectedCell, &'a Node)> {
        let mut seen: Vec<usize> = Vec::new();
        let mut cells = Vec::new();
        for row in top..bottom.min(self.projected.rows) {
            for column in left..right.min(self.projected.columns) {
                let Some(index) = self.slot(row, column) else {
                    continue;
                };
                if seen.contains(&index) {
                    continue;
                }
                seen.push(index);
                let Some(cell) = self.projected.cells.get(index) else {
                    continue;
                };
                let Some(node) = self.cell_nodes.get(index) else {
                    continue;
                };
                cells.push((cell, *node));
            }
        }
        cells.sort_by_key(|(cell, _)| cell.source_pos);
        cells
    }

    pub(crate) fn cell_selection_over(
        &self,
        top: u32,
        left: u32,
        bottom: u32,
        right: u32,
    ) -> Option<Selection> {
        let (anchor, _) = self.cell_at(top, left)?;
        let (head, _) =
            self.cell_at(bottom.checked_sub(ONE_SLOT)?, right.checked_sub(ONE_SLOT)?)?;
        Some(Selection::cell(anchor.source_pos, head.source_pos))
    }

    pub(crate) fn cell_selection_at(&self, row: u32, column: u32) -> Option<Selection> {
        let (cell, _) = self.cell_at(row, column)?;
        Some(Selection::cell(cell.source_pos, cell.source_pos))
    }
}

pub(crate) fn node_starting_at(document: &Document, position: u32) -> Option<&Node> {
    let Ok(resolved) = document.resolve(position) else {
        return None;
    };
    let parent = resolved.parent(document);
    let mut cursor = 0u32;
    for child in parent.content()?.iter() {
        if cursor == resolved.parent_offset {
            return Some(child);
        }
        cursor = cursor.checked_add(child.node_size())?;
    }
    None
}

pub(crate) fn default_text_block_node(schema: &Schema) -> Option<Node> {
    let spec = schema.preferred_text_block()?;
    Some(Node::element(
        spec.name.clone(),
        default_attrs(schema, &spec.name)?,
        Fragment::empty(),
    ))
}

pub(crate) fn fresh_cell_node(schema: &Schema, cell_type: &str) -> Option<Node> {
    Some(Node::element(
        cell_type.to_owned(),
        default_attrs(schema, cell_type)?,
        Fragment::from(vec![default_text_block_node(schema)?]),
    ))
}

pub(crate) fn caret_in_cell(cell_pos: u32) -> Option<Selection> {
    Some(Selection::cursor(
        cell_pos.checked_add(CELL_INTERIOR_OFFSET)?,
    ))
}

pub(crate) fn attrs_with_row_span(cell: &Node, rowspan: u32) -> HashMap<String, Value> {
    let mut attrs = cell.attrs().clone();
    attrs.insert(TABLE_CELL_ROWSPAN_ATTR.to_string(), Value::from(rowspan));
    attrs
}

pub(crate) fn attrs_with_added_column(cell: &Node, offset: u32) -> Option<HashMap<String, Value>> {
    let Ok(colspan) = span_attribute(cell, TABLE_CELL_COLSPAN_ATTR) else {
        return None;
    };
    let mut attrs = cell.attrs().clone();
    attrs.insert(
        TABLE_CELL_COLSPAN_ATTR.to_string(),
        Value::from(colspan.checked_add(ONE_SLOT)?),
    );
    if let Some(Value::Array(widths)) = cell.attrs().get(TABLE_CELL_COLWIDTH_ATTR) {
        let mut widened = widths.clone();
        widened.insert((offset as usize).min(widened.len()), Value::Null);
        attrs.insert(TABLE_CELL_COLWIDTH_ATTR.to_string(), Value::Array(widened));
    }
    Some(attrs)
}

pub(crate) fn attrs_with_removed_column(
    cell: &Node,
    offset: u32,
) -> Option<HashMap<String, Value>> {
    let Ok(colspan) = span_attribute(cell, TABLE_CELL_COLSPAN_ATTR) else {
        return None;
    };
    let narrowed = colspan
        .checked_sub(ONE_SLOT)
        .filter(|span| u64::from(*span) >= MIN_TABLE_CELL_SPAN)?;
    let mut attrs = cell.attrs().clone();
    attrs.insert(TABLE_CELL_COLSPAN_ATTR.to_string(), Value::from(narrowed));
    if let Some(Value::Array(widths)) = cell.attrs().get(TABLE_CELL_COLWIDTH_ATTR) {
        let mut remaining = widths.clone();
        if (offset as usize) < remaining.len() {
            remaining.remove(offset as usize);
        }
        let keeps_a_width = remaining
            .iter()
            .any(|width| integral_unsigned(width).is_some_and(|value| value > UNSET_COLUMN_WIDTH));
        attrs.insert(
            TABLE_CELL_COLWIDTH_ATTR.to_string(),
            if keeps_a_width {
                Value::Array(remaining)
            } else {
                Value::Null
            },
        );
    }
    Some(attrs)
}

pub(crate) fn attrs_with_merged_span(
    cell: &Node,
    colspan: u32,
    rowspan: u32,
) -> Option<HashMap<String, Value>> {
    let Ok(declared) = span_attribute(cell, TABLE_CELL_COLSPAN_ATTR) else {
        return None;
    };
    let widened_by = colspan.checked_sub(declared)?;
    let mut attrs = cell.attrs().clone();
    attrs.insert(TABLE_CELL_COLSPAN_ATTR.to_string(), Value::from(colspan));
    attrs.insert(TABLE_CELL_ROWSPAN_ATTR.to_string(), Value::from(rowspan));
    if let Some(Value::Array(widths)) = cell.attrs().get(TABLE_CELL_COLWIDTH_ATTR) {
        let mut widened = widths.clone();
        let slice = (declared as usize).min(widened.len());
        for _ in FIRST_WIDTH_SLICE..widened_by {
            widened.insert(slice, Value::from(UNSET_COLUMN_WIDTH));
        }
        attrs.insert(TABLE_CELL_COLWIDTH_ATTR.to_string(), Value::Array(widened));
    }
    Some(attrs)
}

pub(crate) fn attrs_with_unit_span(cell: &Node, offset: u32) -> HashMap<String, Value> {
    let mut attrs = cell.attrs().clone();
    attrs.insert(
        TABLE_CELL_COLSPAN_ATTR.to_string(),
        Value::from(MIN_TABLE_CELL_SPAN),
    );
    attrs.insert(
        TABLE_CELL_ROWSPAN_ATTR.to_string(),
        Value::from(MIN_TABLE_CELL_SPAN),
    );
    if let Some(Value::Array(widths)) = cell.attrs().get(TABLE_CELL_COLWIDTH_ATTR) {
        let slice = widths
            .get(offset as usize)
            .and_then(integral_unsigned)
            .filter(|width| *width > UNSET_COLUMN_WIDTH);
        attrs.insert(
            TABLE_CELL_COLWIDTH_ATTR.to_string(),
            match slice {
                Some(width) => Value::Array(vec![Value::from(width)]),
                None => Value::Null,
            },
        );
    }
    attrs
}

pub(crate) fn attrs_with_column_width(
    cell: &Node,
    offset: u32,
    width: u32,
) -> Option<HashMap<String, Value>> {
    let Ok(colspan) = span_attribute(cell, TABLE_CELL_COLSPAN_ATTR) else {
        return None;
    };
    let mut widths = match cell.attrs().get(TABLE_CELL_COLWIDTH_ATTR) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(declared)) => declared.clone(),
        Some(_) => return None,
    };
    if widths.len() < colspan as usize
        && try_resize(
            &mut widths,
            colspan as usize,
            Value::from(UNSET_COLUMN_WIDTH),
        )
        .is_err()
    {
        return None;
    }
    let slot = widths.get_mut(offset as usize)?;
    *slot = Value::from(width);
    let mut attrs = cell.attrs().clone();
    attrs.insert(TABLE_CELL_COLWIDTH_ATTR.to_string(), Value::Array(widths));
    Some(attrs)
}

pub(crate) fn cell_holds_only(cell: &Node, block: &Node) -> bool {
    cell.content()
        .is_some_and(|content| content.children() == std::slice::from_ref(block))
}

pub(crate) fn cell_holds_no_content(cell: &Node, schema: &Schema) -> bool {
    let Some(content) = cell.content() else {
        return false;
    };
    let [block] = content.children() else {
        return false;
    };
    schema
        .node(block.node_type())
        .is_some_and(|spec| matches!(spec.role, crate::schema::NodeRole::TextBlock))
        && block
            .content()
            .is_none_or(|blocks| blocks.child_count() == NO_CHILDREN)
}

pub(crate) fn retyped_cell(schema: &Schema, cell: &Node, cell_type: &str) -> Option<Node> {
    Some(Node::element(
        cell_type.to_owned(),
        compatible_declared_attrs(schema, cell_type, cell.attrs())?,
        cell.content().cloned()?,
    ))
}

fn regular_target<'a>(
    candidate: &TableActionCandidate<'a>,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Option<TableTarget<'a>> {
    TableTarget::resolve(
        candidate.document,
        candidate.table_pos,
        candidate.anchors,
        schema,
        limits,
        GridRequirement::Regular,
    )
}

pub(crate) struct InsertRowAction {
    pub side: TableEdge,
}

impl TableAction for InsertRowAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::InsertRow
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        Ok(rows::plan_insert_row(&target, self.side, schema))
    }
}

pub(crate) struct DeleteRowsAction;

impl TableAction for DeleteRowsAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::DeleteRow
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        Ok(rows::plan_delete_rows(
            candidate.document,
            &target,
            schema,
            limits,
        ))
    }
}

pub(crate) struct InsertColumnAction {
    pub side: TableEdge,
}

impl TableAction for InsertColumnAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::InsertColumn
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        Ok(columns::plan_insert_column(&target, self.side, schema))
    }
}

pub(crate) struct DeleteColumnsAction;

impl TableAction for DeleteColumnsAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::DeleteColumn
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        Ok(columns::plan_delete_columns(
            candidate.document,
            &target,
            schema,
            limits,
        ))
    }
}

pub(crate) struct ToggleHeaderAction {
    pub target: TableHeaderTarget,
}

impl TableAction for ToggleHeaderAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::Header
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        Ok(headers::plan_toggle_header(
            &target,
            self.target,
            schema,
            &candidate.selection,
        ))
    }
}

pub(crate) struct MergeCellsAction;

impl TableAction for MergeCellsAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::Merge
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        Ok(merge::plan_merge_cells(&target, schema))
    }
}

pub(crate) struct SplitCellAction;

impl TableAction for SplitCellAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::Split
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        Ok(merge::plan_split_cell(&target, schema))
    }
}

pub(crate) struct SetColumnWidthAction {
    pub width: u32,
}

impl TableAction for SetColumnWidthAction {
    fn kind(&self) -> TableActionKind {
        TableActionKind::Resize
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<Option<TableActionOutcome>> {
        let Some(target) = regular_target(candidate, schema, limits) else {
            return Ok(None);
        };
        resize::plan_set_column_width(&target, self.width).map_err(|error| {
            crate::tables::command_context::table_shape_operation_error(
                error,
                crate::tables::normalize::UNCORRELATED_REQUEST_ID,
            )
        })
    }
}

pub(crate) fn plan_insert_table(
    document: &Document,
    schema: &Schema,
    selection: &Selection,
    limits: &ResourceLimits,
    rows: u32,
    columns: u32,
    with_header_row: bool,
) -> Option<SemanticCommandPlan> {
    let Ok(Some(roles)) = TableRoles::resolve(schema) else {
        return None;
    };
    if rows < MIN_INSERTED_TABLE_DIMENSION || columns < MIN_INSERTED_TABLE_DIMENSION {
        return None;
    }
    if (rows as usize).checked_mul(columns as usize)? > limits.max_table_grid_slots {
        return None;
    }
    let from = selection.from(document)?;
    let to = selection.to(document)?;
    if from != to {
        return None;
    }

    let body_cell = fresh_cell_node(schema, &roles.cell)?;
    let header_cell = fresh_cell_node(schema, &roles.header_cell)?;
    let mut row_nodes = Vec::new();
    for row in FIRST_ROW..rows {
        let template = if with_header_row && row == FIRST_ROW {
            &header_cell
        } else {
            &body_cell
        };
        row_nodes.push(Node::element(
            roles.row.clone(),
            default_attrs(schema, &roles.row)?,
            Fragment::from(
                (FIRST_COLUMN..columns)
                    .map(|_| template.clone())
                    .collect::<Vec<_>>(),
            ),
        ));
    }
    let table = Node::element(
        roles.table.clone(),
        default_attrs(schema, &roles.table)?,
        Fragment::from(row_nodes),
    );

    let (replace_from, replace_to) =
        empty_text_block_range(document, schema, from).unwrap_or_else(|| {
            let insert = resolve_block_insert_pos(document, schema, from);
            (insert, insert)
        });
    let first_cell = replace_from
        .checked_add(NODE_OPENING_TOKENS)?
        .checked_add(NODE_OPENING_TOKENS)?;
    Some(SemanticCommandPlan {
        operations: vec![SemanticOperation::ReplaceRange {
            from: replace_from,
            to: replace_to,
            content: Fragment::from(vec![table]),
        }],
        selection_after: caret_in_cell(first_cell),
        history: SemanticCommandHistory::InputBoundary,
    })
}

pub(crate) fn plan_delete_table(
    document: &Document,
    schema: &Schema,
    table_pos: u32,
) -> Option<SemanticCommandPlan> {
    let Ok(Some(roles)) = TableRoles::resolve(schema) else {
        return None;
    };
    let table = node_starting_at(document, table_pos)?;
    if table.node_type() != roles.table {
        return None;
    }
    let Ok(resolved) = document.resolve(table_pos) else {
        return None;
    };
    let alone = resolved.parent(document).content()?.child_count() == ONLY_CHILD;
    let replacement = if alone {
        vec![default_text_block_node(schema)?]
    } else {
        Vec::new()
    };
    let caret = if alone {
        table_pos.checked_add(NODE_OPENING_TOKENS)?
    } else {
        table_pos
    };
    Some(SemanticCommandPlan {
        operations: vec![SemanticOperation::ReplaceRange {
            from: table_pos,
            to: table_pos.checked_add(table.node_size())?,
            content: Fragment::from(replacement),
        }],
        selection_after: Some(Selection::cursor(caret)),
        history: SemanticCommandHistory::InputBoundary,
    })
}

pub(crate) fn plan_select_rows(target: &TableTarget<'_>) -> Option<Selection> {
    let rect = target.rect()?;
    target.cell_selection_over(rect.top, FIRST_COLUMN, rect.bottom, target.columns())
}

pub(crate) fn plan_select_columns(target: &TableTarget<'_>) -> Option<Selection> {
    let rect = target.rect()?;
    target.cell_selection_over(FIRST_ROW, rect.left, target.rows(), rect.right)
}

pub(crate) fn plan_clear_cells(
    target: &TableTarget<'_>,
    schema: &Schema,
) -> Option<SemanticCommandPlan> {
    let rect = target.rect()?;
    let default_block = default_text_block_node(schema)?;
    let mut operations = Vec::new();
    for (cell, node) in target.cells_in_rectangle(rect.top, rect.left, rect.bottom, rect.right) {
        if cell_holds_only(node, &default_block) {
            continue;
        }
        operations.push(SemanticOperation::ReplaceRange {
            from: cell.source_pos.checked_add(NODE_OPENING_TOKENS)?,
            to: cell.source_end.checked_sub(NODE_CLOSING_TOKENS)?,
            content: Fragment::from(vec![default_block.clone()]),
        });
    }
    if operations.is_empty() {
        return None;
    }
    operations.reverse();
    Some(SemanticCommandPlan {
        operations,
        selection_after: None,
        history: SemanticCommandHistory::InputBoundary,
    })
}
