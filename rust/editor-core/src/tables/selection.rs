use crate::selection::Selection;
use crate::tables::admission::{ProjectionFailure, TableProjectionIndex};
use crate::tables::projection::{CellRect, ProjectedTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellAdmission {
    Admitted,
    NotCells,
    ProjectionUnavailable(ProjectionFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellSelectionOrigin {
    Minted,
    Preserved,
}

pub(crate) fn admit_cell_pair(
    index: &TableProjectionIndex,
    anchor: u32,
    head: u32,
) -> CellAdmission {
    if resolve_cell_rect(index, anchor, head).is_some() {
        return CellAdmission::Admitted;
    }
    match index.projection_failure() {
        Some(failure) => CellAdmission::ProjectionUnavailable(failure),
        None => CellAdmission::NotCells,
    }
}

pub(crate) fn cell_pair_is_usable(
    index: &TableProjectionIndex,
    anchor: u32,
    head: u32,
    origin: CellSelectionOrigin,
) -> bool {
    match (admit_cell_pair(index, anchor, head), origin) {
        (CellAdmission::Admitted, _) => true,
        (CellAdmission::ProjectionUnavailable(_), CellSelectionOrigin::Preserved) => true,
        (CellAdmission::ProjectionUnavailable(_), CellSelectionOrigin::Minted)
        | (CellAdmission::NotCells, _) => false,
    }
}

pub(crate) fn admit_cell_opening(
    index: &TableProjectionIndex,
    position: u32,
) -> Result<u32, CellAdmission> {
    match cell_opening_containing(index, position) {
        Some(opening) => Ok(opening),
        None => Err(match index.projection_failure() {
            Some(failure) => CellAdmission::ProjectionUnavailable(failure),
            None => CellAdmission::NotCells,
        }),
    }
}

pub(crate) const CELL_SELECTION_ANCHOR_FIELD: &str = "selection.anchorCell";
pub(crate) const CELL_SELECTION_HEAD_FIELD: &str = "selection.headCell";
pub(crate) const CELL_SELECTION_INVALID: &str =
    "cell selection must target real table cells in one table";
pub(crate) const CELL_SELECTION_PROJECTION_EXHAUSTED: &str =
    "table projection exceeded its resource budget, so a cell selection cannot be admitted";
pub(crate) const CELL_SELECTION_PROJECTION_INVALID: &str =
    "table structure is invalid, so a cell selection cannot be admitted";

const FIRST_ROW: u32 = 0;
const FIRST_COLUMN: u32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellSelectionRect {
    pub table_pos: u32,
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
    pub cells: Vec<u32>,
}

fn cell_index_at(table: &ProjectedTable, position: u32) -> Option<usize> {
    table
        .cells
        .iter()
        .position(|cell| cell.source_pos == position)
}

fn locate<'a>(
    index: &'a TableProjectionIndex,
    position: u32,
) -> Option<(u32, &'a ProjectedTable, usize)> {
    index.positions().find_map(|table_pos| {
        let table = index.table_at(table_pos)?;
        let cell = cell_index_at(table, position)?;
        Some((table_pos, table, cell))
    })
}

pub(crate) fn cell_opening_containing(index: &TableProjectionIndex, position: u32) -> Option<u32> {
    index
        .positions()
        .filter_map(|table_pos| index.table_at(table_pos))
        .flat_map(|table| table.cells.iter())
        .filter(|cell| cell.source_pos <= position && position < cell.source_end)
        .map(|cell| cell.source_pos)
        .max()
}

fn slot_cell(table: &ProjectedTable, row: u32, column: u32) -> Option<usize> {
    let offset = (row as usize)
        .checked_mul(table.columns as usize)?
        .checked_add(column as usize)?;
    (column < table.columns)
        .then(|| table.slots.get(offset).copied().flatten())
        .flatten()
}

fn cover(rect: &CellRect, bounds: &mut (u32, u32, u32, u32)) {
    let (top, left, bottom, right) = *bounds;
    *bounds = (
        top.min(rect.row),
        left.min(rect.column),
        bottom.max(rect.row.saturating_add(rect.rowspan)),
        right.max(rect.column.saturating_add(rect.colspan)),
    );
}

fn close_over_spans(table: &ProjectedTable, bounds: (u32, u32, u32, u32)) -> (u32, u32, u32, u32) {
    let mut bounds = bounds;
    loop {
        let before = bounds;
        for row in before.0..before.2 {
            for column in before.1..before.3 {
                if let Some(cell) = slot_cell(table, row, column).and_then(|i| table.cells.get(i)) {
                    cover(&cell.rect, &mut bounds);
                }
            }
        }
        if bounds == before {
            return bounds;
        }
    }
}

pub(crate) fn resolve_cell_rect(
    index: &TableProjectionIndex,
    anchor: u32,
    head: u32,
) -> Option<CellSelectionRect> {
    let (table_pos, table, anchor_cell) = locate(index, anchor)?;
    let (head_table_pos, _, head_cell) = locate(index, head)?;
    if head_table_pos != table_pos {
        return None;
    }
    let mut bounds = (u32::MAX, u32::MAX, FIRST_ROW, FIRST_COLUMN);
    cover(&table.cells.get(anchor_cell)?.rect, &mut bounds);
    cover(&table.cells.get(head_cell)?.rect, &mut bounds);
    let (top, left, bottom, right) = close_over_spans(table, bounds);

    let mut cells: Vec<u32> = Vec::new();
    for row in top..bottom {
        for column in left..right {
            let Some(cell) = slot_cell(table, row, column).and_then(|i| table.cells.get(i)) else {
                continue;
            };
            if !cells.contains(&cell.source_pos) {
                cells.push(cell.source_pos);
            }
        }
    }
    cells.sort_unstable();
    Some(CellSelectionRect {
        table_pos,
        top,
        left,
        bottom,
        right,
        cells,
    })
}

fn nearest_cell_opening(table: &ProjectedTable, position: u32) -> Option<u32> {
    table
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .find(|source_pos| *source_pos >= position)
        .or_else(|| table.cells.last().map(|cell| cell.source_pos))
}

fn enclosing_table(index: &TableProjectionIndex, position: u32) -> Option<(u32, &ProjectedTable)> {
    index
        .positions()
        .filter(|table_pos| *table_pos <= position)
        .max()
        .and_then(|table_pos| Some((table_pos, index.table_at(table_pos)?)))
}

pub(crate) fn snap_cell_selection(
    index: &TableProjectionIndex,
    anchor: u32,
    head: u32,
) -> Option<Selection> {
    let snap = |position: u32| {
        if locate(index, position).is_some() {
            return Some(position);
        }
        let (_, table) = enclosing_table(index, position)?;
        nearest_cell_opening(table, position)
    };
    let anchor = snap(anchor)?;
    let head = snap(head)?;
    resolve_cell_rect(index, anchor, head)?;
    Some(Selection::Cell { anchor, head })
}
