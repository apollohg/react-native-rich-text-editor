use std::hash::{DefaultHasher, Hash, Hasher};

use crate::model::Document;
use crate::position::PositionMap;
use crate::position_epoch::PinnedTableCell;
use crate::schema::Schema;
use crate::serialize::node_to_prosemirror_json;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::commands::node_starting_at;
use crate::tables::projection::{ProjectedCell, ProjectedTable};

const CELL_CONTENT_OFFSET: u32 = 1;

pub(super) struct PinnedCellSpan {
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) cell: PinnedTableCell,
}

pub(super) struct CellPinning<'state> {
    pub(super) document: &'state Document,
    pub(super) schema: &'state Schema,
    pub(super) index: &'state TableProjectionIndex,
    pub(super) position_map: &'state PositionMap,
}

impl CellPinning<'_> {
    pub(super) fn spans(&self) -> Vec<PinnedCellSpan> {
        let mut spans: Vec<PinnedCellSpan> = self
            .tables()
            .flat_map(|table| {
                table.cells.iter().filter_map(move |cell| {
                    Some(PinnedCellSpan {
                        start: cell.source_pos,
                        end: cell.source_end,
                        cell: self.pin(table, cell)?,
                    })
                })
            })
            .collect();
        spans.sort_by_key(|span| span.start);
        spans
    }

    pub(super) fn reanchor_in_retyped_cell(
        &self,
        row_position: u32,
        pinned: &PinnedTableCell,
        epoch_offset: u32,
    ) -> Option<u32> {
        let table = self.tables().find(|table| {
            table
                .cells
                .iter()
                .any(|cell| cell.source_pos == row_position || cell.source_end == row_position)
        })?;
        let cell = table
            .cells
            .iter()
            .find(|cell| cell.rect.row == pinned.row && cell.rect.column == pinned.column)?;
        let retyped = self.pin(table, cell)?;
        let same_logical_cell = PinnedTableCell {
            content_scalar_start: pinned.content_scalar_start,
            ..retyped
        } == *pinned;
        if !same_logical_cell {
            return None;
        }
        retyped
            .content_scalar_start
            .checked_add(epoch_offset.checked_sub(pinned.content_scalar_start)?)
    }

    fn tables(&self) -> impl Iterator<Item = &ProjectedTable> + '_ {
        self.index
            .positions()
            .filter_map(|position| self.index.table_at(position))
    }

    fn pin(&self, table: &ProjectedTable, cell: &ProjectedCell) -> Option<PinnedTableCell> {
        let node = node_starting_at(self.document, cell.source_pos)?;
        let mut fingerprint = DefaultHasher::new();
        for child in node.content()?.iter() {
            node_to_prosemirror_json(child, self.schema)
                .to_string()
                .hash(&mut fingerprint);
        }
        Some(PinnedTableCell {
            row: cell.rect.row,
            column: cell.rect.column,
            rowspan: cell.rect.rowspan,
            colspan: cell.rect.colspan,
            table_rows: table.rows,
            table_columns: table.columns,
            content_scalar_start: self.position_map.doc_to_scalar(
                cell.source_pos.checked_add(CELL_CONTENT_OFFSET)?,
                self.document,
            ),
            content_fingerprint: fingerprint.finish(),
        })
    }
}
