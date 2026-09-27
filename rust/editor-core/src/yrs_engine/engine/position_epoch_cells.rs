use std::hash::{DefaultHasher, Hash, Hasher};

use crate::model::{Document, Node};
use crate::position::PositionMap;
use crate::position_epoch::{CellTextPoint, PinnedCellBoundary, PinnedTableCell};
use crate::schema::Schema;
use crate::serialize::node_to_prosemirror_json;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::commands::node_starting_at;
use crate::tables::projection::{ProjectedCell, ProjectedTable};
use crate::yrs_engine::Affinity;

const CELL_CONTENT_OFFSET: u32 = 1;
const ADJACENT_TEXT_POSITION: u32 = 1;

pub(super) struct PinnedCellSpan {
    pub(super) cell: PinnedTableCell,
    pub(super) points: Vec<(u32, CellTextPoint)>,
}

pub(super) struct CellPinning<'state> {
    pub(super) document: &'state Document,
    pub(super) schema: &'state Schema,
    pub(super) index: &'state TableProjectionIndex,
    pub(super) position_map: &'state PositionMap,
}

impl CellPinning<'_> {
    pub(super) fn spans(&self) -> Vec<PinnedCellSpan> {
        self.tables()
            .flat_map(|table| {
                table.cells.iter().filter_map(move |cell| {
                    let points = self.text_points(cell)?;
                    Some(PinnedCellSpan {
                        cell: self.pin(table, cell, &points)?,
                        points,
                    })
                })
            })
            .collect()
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
        let node = node_starting_at(self.document, cell.source_pos)?;
        if text_fingerprint_of(node) != pinned.cell.text_fingerprint {
            return None;
        }
        let points = self.text_points(cell)?;
        if run_structure_of(&points) == pinned.cell.run_structure {
            return scalar_at_point(&points, pinned.point);
        }
        let mut matching = points
            .iter()
            .filter(|(_, point)| point.text_offset == pinned.point.text_offset)
            .map(|(scalar, _)| *scalar);
        match affinity {
            Affinity::Before => matching.next(),
            Affinity::After => matching.last(),
        }
    }

    pub(super) fn reanchor_in_retyped_cell(
        &self,
        row_position: u32,
        pinned: PinnedCellBoundary<'_>,
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
        let points = self.text_points(cell)?;
        if self.pin(table, cell, &points)? != *pinned.cell {
            return None;
        }
        scalar_at_point(&points, pinned.point)
    }

    fn text_points(&self, cell: &ProjectedCell) -> Option<Vec<(u32, CellTextPoint)>> {
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
        let mut points = Vec::new();
        let mut previous: Option<(u32, CellTextPoint)> = None;
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
            let point = match previous {
                None => CellTextPoint {
                    text_offset: 0,
                    run: 0,
                    run_offset: 0,
                },
                Some((previous_doc_pos, point)) if previous_doc_pos == doc_pos => point,
                Some((previous_doc_pos, point))
                    if previous_doc_pos.checked_add(ADJACENT_TEXT_POSITION) == Some(doc_pos) =>
                {
                    CellTextPoint {
                        text_offset: point.text_offset + 1,
                        run: point.run,
                        run_offset: point.run_offset + 1,
                    }
                }
                Some((_, point)) => CellTextPoint {
                    text_offset: point.text_offset,
                    run: point.run + 1,
                    run_offset: 0,
                },
            };
            previous = Some((doc_pos, point));
            points.push((scalar, point));
        }
        Some(points)
    }

    fn tables(&self) -> impl Iterator<Item = &ProjectedTable> + '_ {
        self.index
            .positions()
            .filter_map(|position| self.index.table_at(position))
    }

    fn pin(
        &self,
        table: &ProjectedTable,
        cell: &ProjectedCell,
        points: &[(u32, CellTextPoint)],
    ) -> Option<PinnedTableCell> {
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
            run_structure: run_structure_of(points),
        })
    }
}

fn scalar_at_point(points: &[(u32, CellTextPoint)], target: CellTextPoint) -> Option<u32> {
    points
        .iter()
        .find(|(_, point)| point.run == target.run && point.run_offset == target.run_offset)
        .map(|(scalar, _)| *scalar)
}

fn run_structure_of(points: &[(u32, CellTextPoint)]) -> u64 {
    let mut structure = DefaultHasher::new();
    for (_, point) in points {
        (point.run, point.run_offset).hash(&mut structure);
    }
    structure.finish()
}

fn text_fingerprint_of(node: &Node) -> u64 {
    let mut fingerprint = DefaultHasher::new();
    node.text_content().hash(&mut fingerprint);
    fingerprint.finish()
}
