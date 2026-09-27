use std::hash::{DefaultHasher, Hash, Hasher};

use crate::model::{Document, Node};
use crate::position::PositionMap;
use crate::position_epoch::{PinnedCellBoundary, PinnedTableCell};
use crate::schema::Schema;
use crate::serialize::node_to_prosemirror_json;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::commands::node_starting_at;
use crate::tables::projection::{ProjectedCell, ProjectedTable};
use crate::yrs_engine::Affinity;

const CELL_CONTENT_OFFSET: u32 = 1;
const ADJACENT_TEXT_POSITION: u32 = 1;

pub(super) struct PinnedCellSpan {
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) cell: PinnedTableCell,
}

pub(super) struct CellTextCounter {
    offsets: Vec<u32>,
    previous: Option<(usize, u32)>,
}

impl CellTextCounter {
    pub(super) fn new(cells: usize) -> Self {
        Self {
            offsets: vec![0; cells],
            previous: None,
        }
    }

    pub(super) fn advance(&mut self, cell: Option<usize>, doc_pos: u32) -> Option<u32> {
        let previous = self.previous.take();
        let cell = cell?;
        if previous.is_some_and(|(previous_cell, previous_doc_pos)| {
            previous_cell == cell && is_adjacent_text_step(previous_doc_pos, doc_pos)
        }) {
            self.offsets[cell] += 1;
        }
        self.previous = Some((cell, doc_pos));
        Some(self.offsets[cell])
    }
}

fn text_fingerprint_of(node: &Node) -> u64 {
    let mut fingerprint = DefaultHasher::new();
    node.text_content().hash(&mut fingerprint);
    fingerprint.finish()
}

fn is_adjacent_text_step(previous_doc_pos: u32, doc_pos: u32) -> bool {
    previous_doc_pos.checked_add(ADJACENT_TEXT_POSITION) == Some(doc_pos)
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

    pub(super) fn reanchor_in_surviving_cell(
        &self,
        doc_pos: u32,
        pinned: PinnedCellBoundary<'_>,
        affinity: Affinity,
    ) -> Option<u32> {
        let cell = self
            .tables()
            .flat_map(|table| table.cells.iter())
            .filter(|cell| cell.source_pos < doc_pos && doc_pos < cell.source_end)
            .min_by_key(|cell| cell.source_end - cell.source_pos)?;
        if self.text_fingerprint(cell)? != pinned.cell.text_fingerprint {
            return None;
        }
        self.scalar_at_text_offset(cell, pinned.text_offset, affinity)
    }

    pub(super) fn reanchor_in_retyped_cell(
        &self,
        row_position: u32,
        pinned: PinnedCellBoundary<'_>,
        affinity: Affinity,
    ) -> Option<u32> {
        let table = self.tables().find(|table| {
            table
                .cells
                .iter()
                .any(|cell| cell.source_pos == row_position || cell.source_end == row_position)
        })?;
        let cell = table.cells.iter().find(|cell| {
            cell.rect.row == pinned.cell.row && cell.rect.column == pinned.cell.column
        })?;
        if self.pin(table, cell)? != *pinned.cell {
            return None;
        }
        self.scalar_at_text_offset(cell, pinned.text_offset, affinity)
    }

    fn scalar_at_text_offset(
        &self,
        cell: &ProjectedCell,
        text_offset: u32,
        affinity: Affinity,
    ) -> Option<u32> {
        let nested: Vec<&ProjectedCell> = self
            .tables()
            .flat_map(|table| table.cells.iter())
            .filter(|nested| {
                cell.source_pos < nested.source_pos && nested.source_end <= cell.source_end
            })
            .collect();
        let start = self.position_map.doc_to_scalar(
            cell.source_pos.checked_add(CELL_CONTENT_OFFSET)?,
            self.document,
        );
        let mut count = 0u32;
        let mut previous_doc_pos: Option<u32> = None;
        let mut found = None;
        for scalar in start..=self.position_map.total_scalars() {
            let doc_pos = self.position_map.scalar_to_doc(scalar, self.document);
            if doc_pos >= cell.source_end {
                break;
            }
            if doc_pos <= cell.source_pos
                || nested
                    .iter()
                    .any(|nested| nested.source_pos < doc_pos && doc_pos < nested.source_end)
            {
                continue;
            }
            if previous_doc_pos.is_some_and(|previous| is_adjacent_text_step(previous, doc_pos)) {
                count += 1;
            }
            previous_doc_pos = Some(doc_pos);
            if count > text_offset {
                break;
            }
            if count == text_offset {
                found = Some(scalar);
                if affinity == Affinity::Before {
                    break;
                }
            }
        }
        found
    }

    fn tables(&self) -> impl Iterator<Item = &ProjectedTable> + '_ {
        self.index
            .positions()
            .filter_map(|position| self.index.table_at(position))
    }

    fn text_fingerprint(&self, cell: &ProjectedCell) -> Option<u64> {
        node_starting_at(self.document, cell.source_pos).map(text_fingerprint_of)
    }

    fn pin(&self, table: &ProjectedTable, cell: &ProjectedCell) -> Option<PinnedTableCell> {
        let node = node_starting_at(self.document, cell.source_pos)?;
        let mut content_fingerprint = DefaultHasher::new();
        for child in node.content()?.iter() {
            node_to_prosemirror_json(child, self.schema)
                .to_string()
                .hash(&mut content_fingerprint);
        }
        Some(PinnedTableCell {
            row: cell.rect.row,
            column: cell.rect.column,
            rowspan: cell.rect.rowspan,
            colspan: cell.rect.colspan,
            table_rows: table.rows,
            table_columns: table.columns,
            content_fingerprint: content_fingerprint.finish(),
            text_fingerprint: text_fingerprint_of(node),
        })
    }
}
