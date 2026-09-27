use crate::selection::Selection;
use crate::tables::admission::{ProjectionFailure, TableProjectionIndex};
use crate::tables::projection::{CellRect, ProjectedCell, ProjectedTable};
use crate::transform::StepMap;

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

pub(crate) fn admit_exact_cell_opening(
    index: &TableProjectionIndex,
    opening: u32,
) -> Result<u32, CellAdmission> {
    if locate(index, opening).is_some() {
        Ok(opening)
    } else {
        Err(match index.projection_failure() {
            Some(failure) => CellAdmission::ProjectionUnavailable(failure),
            None => CellAdmission::NotCells,
        })
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
    pub cuts_a_span: bool,
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
    let closed = close_over_spans(table, bounds);
    let cuts_a_span = closed != bounds;
    let (top, left, bottom, right) = closed;

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
        cuts_a_span,
        cells,
    })
}

fn located_cell(
    index: &TableProjectionIndex,
    position: u32,
) -> Option<(u32, &ProjectedTable, &ProjectedCell)> {
    let (table_pos, table, cell) = locate(index, position)?;
    Some((table_pos, table, table.cells.get(cell)?))
}

fn opening_at(table: &ProjectedTable, row: u32, column: u32) -> Option<u32> {
    slot_cell(table, row, column).and_then(|cell| Some(table.cells.get(cell)?.source_pos))
}

fn row_end(rect: &CellRect) -> u32 {
    rect.row.saturating_add(rect.rowspan)
}

fn column_end(rect: &CellRect) -> u32 {
    rect.column.saturating_add(rect.colspan)
}

fn spans_every_column(table: &ProjectedTable, anchor: &CellRect, head: &CellRect) -> bool {
    anchor.column.min(head.column) == FIRST_COLUMN
        && column_end(anchor).max(column_end(head)) == table.columns
}

fn spans_every_row(table: &ProjectedTable, anchor: &CellRect, head: &CellRect) -> bool {
    anchor.row.min(head.row) == FIRST_ROW && row_end(anchor).max(row_end(head)) == table.rows
}

fn row_selection(
    table: &ProjectedTable,
    anchor: &ProjectedCell,
    head: &ProjectedCell,
) -> Option<Selection> {
    let last_column = table.columns.checked_sub(1)?;
    let row_start = |cell: &ProjectedCell| opening_at(table, cell.rect.row, FIRST_COLUMN);
    let row_finish = |cell: &ProjectedCell| opening_at(table, cell.rect.row, last_column);
    let (anchor_pos, head_pos) = if anchor.rect.column <= head.rect.column {
        (
            if anchor.rect.column > FIRST_COLUMN {
                row_start(anchor)?
            } else {
                anchor.source_pos
            },
            if column_end(&head.rect) < table.columns {
                row_finish(head)?
            } else {
                head.source_pos
            },
        )
    } else {
        (
            if column_end(&anchor.rect) < table.columns {
                row_finish(anchor)?
            } else {
                anchor.source_pos
            },
            if head.rect.column > FIRST_COLUMN {
                row_start(head)?
            } else {
                head.source_pos
            },
        )
    };
    Some(Selection::cell(anchor_pos, head_pos))
}

fn column_selection(
    table: &ProjectedTable,
    anchor: &ProjectedCell,
    head: &ProjectedCell,
) -> Option<Selection> {
    let last_row = table.rows.checked_sub(1)?;
    let column_top = |cell: &ProjectedCell| opening_at(table, FIRST_ROW, cell.rect.column);
    let column_bottom =
        |cell: &ProjectedCell| opening_at(table, last_row, column_end(&cell.rect).checked_sub(1)?);
    let (anchor_pos, head_pos) = if anchor.rect.row <= head.rect.row {
        (
            if anchor.rect.row > FIRST_ROW {
                column_top(anchor)?
            } else {
                anchor.source_pos
            },
            if row_end(&head.rect) < table.rows {
                column_bottom(head)?
            } else {
                head.source_pos
            },
        )
    } else {
        (
            if row_end(&anchor.rect) < table.rows {
                column_bottom(anchor)?
            } else {
                anchor.source_pos
            },
            if head.rect.row > FIRST_ROW {
                column_top(head)?
            } else {
                head.source_pos
            },
        )
    };
    Some(Selection::cell(anchor_pos, head_pos))
}

pub(crate) fn map_cell_selection(
    before: &TableProjectionIndex,
    after: &TableProjectionIndex,
    anchor: u32,
    head: u32,
    map: &StepMap,
) -> Selection {
    let mapped_anchor = map.map_pos(anchor);
    let mapped_head = map.map_pos(head);
    let mapped = Selection::cell(mapped_anchor, mapped_head);
    let (Some((table_pos, table, anchor_cell)), Some((head_table_pos, _, head_cell))) = (
        located_cell(after, mapped_anchor),
        located_cell(after, mapped_head),
    ) else {
        return Selection::text(mapped_anchor, mapped_head);
    };
    if head_table_pos != table_pos {
        return Selection::text(mapped_anchor, mapped_head);
    }
    let (Some((_, before_table, before_anchor)), Some((_, _, before_head))) =
        (located_cell(before, anchor), located_cell(before, head))
    else {
        return mapped;
    };
    let stretched = if spans_every_column(before_table, &before_anchor.rect, &before_head.rect) {
        row_selection(table, anchor_cell, head_cell)
    } else if spans_every_row(before_table, &before_anchor.rect, &before_head.rect) {
        column_selection(table, anchor_cell, head_cell)
    } else {
        None
    };
    stretched.unwrap_or(mapped)
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
