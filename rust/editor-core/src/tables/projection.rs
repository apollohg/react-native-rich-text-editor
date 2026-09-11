use serde_json::Value;

use crate::model::Node;
use crate::schema::Schema;
use crate::tables::roles::{
    TableRoles, MIN_TABLE_CELL_SPAN, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_COLWIDTH_ATTR,
    TABLE_CELL_ROWSPAN_ATTR,
};
use crate::tables::types::{try_resize, TableError};
use crate::tables::widths::ColumnWidthResolver;

const PROJECTION_WORK_PER_GRID_SLOT: usize = 16;
const DEFAULT_TABLE_CELL_SPAN: u32 = MIN_TABLE_CELL_SPAN as u32;
const UNSET_COLUMN_WIDTH: u32 = 0;
const NODE_OPENING_TOKENS: u32 = 1;
const NEXT_COLUMN_STEP: u32 = 1;
const SINGLE_WORK_STEP: usize = 1;
const MINIMUM_TABLE_GRID_CHARGE: usize = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellRect {
    pub row: u32,
    pub column: u32,
    pub rowspan: u32,
    pub colspan: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectedCell {
    pub source_pos: u32,
    pub rect: CellRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectedTable {
    pub rows: u32,
    pub columns: u32,
    pub cells: Vec<ProjectedCell>,
    pub slots: Vec<Option<usize>>,
    pub widths: Vec<Option<u32>>,
    pub irregular: bool,
}

pub(crate) struct TableGridBudget {
    limit: usize,
    charged: usize,
    work: usize,
}

impl TableGridBudget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            charged: 0,
            work: limit.saturating_mul(PROJECTION_WORK_PER_GRID_SLOT),
        }
    }

    fn admits(&self, slots: usize) -> Result<(), TableError> {
        if slots > self.limit.saturating_sub(self.charged) {
            return Err(TableError::GridLimit {
                limit: self.limit,
                actual: self.charged.saturating_add(slots),
            });
        }
        Ok(())
    }

    fn charge(&mut self, slots: usize) -> Result<(), TableError> {
        self.admits(slots)?;
        self.charged = self.charged.saturating_add(slots);
        Ok(())
    }

    fn spend(&mut self, steps: usize) -> Result<(), TableError> {
        self.work = self.work.checked_sub(steps).ok_or(TableError::WorkLimit)?;
        Ok(())
    }
}

pub(crate) fn project_table(
    table: &Node,
    table_pos: u32,
    schema: &Schema,
    budget: &mut TableGridBudget,
) -> Result<ProjectedTable, TableError> {
    let roles = TableRoles::resolve(schema)?.ok_or(TableError::InvalidStructure)?;
    if table.node_type() != roles.table {
        return Err(TableError::InvalidStructure);
    }

    let rows = u32::try_from(table.child_count()).map_err(|_| TableError::Allocation)?;
    let raw_columns = raw_column_extent(table, &roles, rows, budget)?;
    let raw_extent = grid_extent(rows, raw_columns)?;
    budget.admits(raw_extent)?;

    let mut placement = Placement::new(rows, raw_extent);
    let mut child_pos = advance(table_pos, NODE_OPENING_TOKENS)?;
    for row in 0..rows {
        let row_node = child_at(table, row)?;
        budget.spend(SINGLE_WORK_STEP)?;
        let next_child_pos = advance(child_pos, row_node.node_size())?;
        if row_node.node_type() == roles.row {
            placement.place_row(
                row_node,
                advance(child_pos, NODE_OPENING_TOKENS)?,
                row,
                &roles,
                budget,
            )?;
        } else {
            placement.irregular = true;
        }
        child_pos = next_child_pos;
    }

    placement.finish(budget)
}

struct Placement {
    rows: u32,
    columns: u32,
    raw_extent: usize,
    occupied_until: Vec<u32>,
    cells: Vec<ProjectedCell>,
    widths: ColumnWidthResolver,
    irregular: bool,
}

impl Placement {
    fn new(rows: u32, raw_extent: usize) -> Self {
        Self {
            rows,
            columns: 0,
            raw_extent,
            occupied_until: Vec::new(),
            cells: Vec::new(),
            widths: ColumnWidthResolver::new(),
            irregular: false,
        }
    }

    fn place_row(
        &mut self,
        row_node: &Node,
        row_content_pos: u32,
        row: u32,
        roles: &TableRoles,
        budget: &mut TableGridBudget,
    ) -> Result<(), TableError> {
        let mut cell_pos = row_content_pos;
        let mut cursor = 0;
        for index in 0..row_node.child_count() {
            let cell = row_node.child(index).ok_or(TableError::InvalidStructure)?;
            budget.spend(SINGLE_WORK_STEP)?;
            let next_cell_pos = advance(cell_pos, cell.node_size())?;
            if cell.node_type() == roles.cell || cell.node_type() == roles.header_cell {
                cursor = self.place_cell(cell, cell_pos, row, cursor, budget)?;
            } else {
                self.irregular = true;
            }
            cell_pos = next_cell_pos;
        }
        Ok(())
    }

    fn place_cell(
        &mut self,
        cell: &Node,
        source_pos: u32,
        row: u32,
        cursor: u32,
        budget: &mut TableGridBudget,
    ) -> Result<u32, TableError> {
        let colspan = span_attribute(cell, TABLE_CELL_COLSPAN_ATTR)?;
        let rowspan = span_attribute(cell, TABLE_CELL_ROWSPAN_ATTR)?;
        let effective_rowspan = rowspan.min(self.rows.saturating_sub(row));
        if effective_rowspan != rowspan {
            self.irregular = true;
        }

        let mut column = cursor;
        while !self.rectangle_is_free(column, advance(column, NEXT_COLUMN_STEP)?, row) {
            budget.spend(SINGLE_WORK_STEP)?;
            column = advance(column, NEXT_COLUMN_STEP)?;
        }

        let end = loop {
            budget.spend(SINGLE_WORK_STEP)?;
            let end = advance(column, colspan)?;
            if self.rectangle_is_free(column, end, row) {
                break end;
            }
            self.irregular = true;
            column = advance(column, NEXT_COLUMN_STEP)?;
        };

        if end > self.columns {
            let extent = grid_extent(self.rows, end)?;
            budget.admits(extent.max(self.raw_extent))?;
            try_resize(&mut self.occupied_until, end as usize, 0)?;
            self.columns = end;
        }

        for column_index in column..end {
            if let Some(occupied) = self.occupied_until.get_mut(column_index as usize) {
                *occupied = advance(row, effective_rowspan)?;
            }
        }

        for _ in 0..effective_rowspan {
            for offset in 0..colspan {
                budget.spend(SINGLE_WORK_STEP)?;
                let width = column_width(cell, offset)?;
                self.widths
                    .contribute_at(advance(column, offset)? as usize, width)?;
            }
        }

        self.cells.push(ProjectedCell {
            source_pos,
            rect: CellRect {
                row,
                column,
                rowspan: effective_rowspan,
                colspan,
            },
        });
        Ok(end)
    }

    fn rectangle_is_free(&self, column: u32, end: u32, row: u32) -> bool {
        (column..end).all(|index| {
            self.occupied_until
                .get(index as usize)
                .is_none_or(|occupied| *occupied <= row)
        })
    }

    fn finish(self, budget: &mut TableGridBudget) -> Result<ProjectedTable, TableError> {
        let extent = grid_extent(self.rows, self.columns)?;
        budget.charge(extent.max(self.raw_extent).max(MINIMUM_TABLE_GRID_CHARGE))?;

        let mut slots: Vec<Option<usize>> = Vec::new();
        try_resize(&mut slots, extent, None)?;
        for (index, cell) in self.cells.iter().enumerate() {
            let rect = &cell.rect;
            for row in rect.row..advance(rect.row, rect.rowspan)? {
                for column in rect.column..advance(rect.column, rect.colspan)? {
                    let slot_index = (row as usize)
                        .checked_mul(self.columns as usize)
                        .and_then(|offset| offset.checked_add(column as usize))
                        .ok_or(TableError::Allocation)?;
                    let slot = slots.get_mut(slot_index).ok_or(TableError::Allocation)?;
                    *slot = Some(index);
                }
            }
        }

        let irregular = self.irregular
            || self.rows == 0
            || self.columns == 0
            || slots.iter().any(Option::is_none);

        Ok(ProjectedTable {
            rows: self.rows,
            columns: self.columns,
            cells: self.cells,
            slots,
            widths: self.widths.finish(self.columns as usize)?,
            irregular,
        })
    }
}

fn raw_column_extent(
    table: &Node,
    roles: &TableRoles,
    rows: u32,
    budget: &mut TableGridBudget,
) -> Result<u32, TableError> {
    let row_count = rows as usize;
    budget.spend(row_count)?;

    let mut row_widths: Vec<u64> = Vec::new();
    let mut entering: Vec<u64> = Vec::new();
    let mut leaving: Vec<u64> = Vec::new();
    try_resize(&mut row_widths, row_count, 0)?;
    try_resize(&mut entering, row_count.saturating_add(1), 0)?;
    try_resize(&mut leaving, row_count.saturating_add(1), 0)?;

    for row in 0..rows {
        let row_node = child_at(table, row)?;
        if row_node.node_type() != roles.row {
            continue;
        }
        let mut occupied: u64 = 0;
        for index in 0..row_node.child_count() {
            let cell = row_node.child(index).ok_or(TableError::InvalidStructure)?;
            budget.spend(SINGLE_WORK_STEP)?;
            if cell.node_type() != roles.cell && cell.node_type() != roles.header_cell {
                continue;
            }
            let colspan = u64::from(span_attribute(cell, TABLE_CELL_COLSPAN_ATTR)?);
            let rowspan = span_attribute(cell, TABLE_CELL_ROWSPAN_ATTR)?;
            occupied = occupied.saturating_add(colspan);
            let covered_until = row.saturating_add(rowspan).min(rows);
            let carried_from = row.saturating_add(1);
            if covered_until > carried_from {
                add_at(&mut entering, carried_from as usize, colspan)?;
                add_at(&mut leaving, covered_until as usize, colspan)?;
            }
        }
        add_at(&mut row_widths, row as usize, occupied)?;
    }

    let mut carried: u64 = 0;
    let mut widest: u64 = 0;
    for row in 0..row_count {
        carried = carried
            .saturating_add(*entering.get(row).ok_or(TableError::Allocation)?)
            .saturating_sub(*leaving.get(row).ok_or(TableError::Allocation)?);
        let occupied = carried.saturating_add(*row_widths.get(row).ok_or(TableError::Allocation)?);
        widest = widest.max(occupied);
    }
    u32::try_from(widest).map_err(|_| TableError::Allocation)
}

fn add_at(values: &mut [u64], index: usize, amount: u64) -> Result<(), TableError> {
    let slot = values.get_mut(index).ok_or(TableError::Allocation)?;
    *slot = slot.saturating_add(amount);
    Ok(())
}

fn child_at(node: &Node, index: u32) -> Result<&Node, TableError> {
    node.child(index as usize)
        .ok_or(TableError::InvalidStructure)
}

fn advance(position: u32, amount: u32) -> Result<u32, TableError> {
    position.checked_add(amount).ok_or(TableError::Allocation)
}

fn grid_extent(rows: u32, columns: u32) -> Result<usize, TableError> {
    (rows as usize)
        .checked_mul(columns as usize)
        .ok_or(TableError::Allocation)
}

fn span_attribute(cell: &Node, name: &str) -> Result<u32, TableError> {
    match cell.attrs().get(name) {
        None | Some(Value::Null) => Ok(DEFAULT_TABLE_CELL_SPAN),
        Some(value) => {
            let span = value.as_u64().ok_or(TableError::InvalidAttributes)?;
            if span < MIN_TABLE_CELL_SPAN {
                return Err(TableError::InvalidAttributes);
            }
            u32::try_from(span).map_err(|_| TableError::InvalidAttributes)
        }
    }
}

fn column_width(cell: &Node, offset: u32) -> Result<u32, TableError> {
    let widths = match cell.attrs().get(TABLE_CELL_COLWIDTH_ATTR) {
        None | Some(Value::Null) => return Ok(UNSET_COLUMN_WIDTH),
        Some(Value::Array(widths)) => widths,
        Some(_) => return Err(TableError::InvalidAttributes),
    };
    match widths.get(offset as usize) {
        None | Some(Value::Null) => Ok(UNSET_COLUMN_WIDTH),
        Some(value) => {
            let width = value.as_u64().ok_or(TableError::InvalidAttributes)?;
            u32::try_from(width).map_err(|_| TableError::InvalidAttributes)
        }
    }
}
