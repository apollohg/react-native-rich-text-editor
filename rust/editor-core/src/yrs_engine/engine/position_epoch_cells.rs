use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use crate::model::{Document, Node};
use crate::position::PositionMap;
use crate::position_epoch::{
    CellTextAttachment, CellTextPoint, PinnedCellBoundary, PinnedCellSpan, PinnedTableCell,
};
use crate::schema::Schema;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::commands::{node_starting_at, NODE_OPENING_TOKENS};
use crate::tables::projection::{ProjectedCell, ProjectedTable};
use crate::yrs_engine::Affinity;

const ADJACENT_TEXT_POSITION: u32 = 1;
const TEXT_STEP: u32 = 1;
const RUN_STEP: u32 = 1;

struct CellFingerprintSink(DefaultHasher);

impl std::io::Write for CellFingerprintSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) struct CellPinning<'state> {
    pub(super) document: &'state Document,
    pub(super) schema: &'state Schema,
    pub(super) index: &'state TableProjectionIndex,
    pub(super) position_map: &'state PositionMap,
}

impl CellPinning<'_> {
    pub(super) fn spans(&self, doc_positions: &[Vec<u32>]) -> Vec<Arc<PinnedCellSpan>> {
        let cells = self.cells_in_document_order();
        let points = self.text_points(&cells, doc_positions.iter().flatten().copied());
        let starts: Vec<u32> = cells.iter().map(|(_, cell)| cell.source_pos).collect();
        let nodes = nodes_starting_at(self.document, &starts);
        cells
            .into_iter()
            .zip(nodes)
            .zip(points)
            .filter_map(|(((table, cell), node), points)| {
                Some(Arc::new(PinnedCellSpan {
                    cell: self.pin(table, cell, node?, &points)?,
                    points,
                }))
            })
            .collect()
    }

    pub(super) fn reanchor_in_surviving_cell(
        &self,
        doc_pos: u32,
        pinned: PinnedCellBoundary<'_>,
        affinity: Affinity,
    ) -> Option<u32> {
        let cells = self.cells_in_document_order();
        let (target, (_, cell)) = cells
            .iter()
            .enumerate()
            .filter(|(_, (_, cell))| cell.source_pos < doc_pos && doc_pos < cell.source_end)
            .min_by_key(|(_, (_, cell))| cell.source_end - cell.source_pos)?;
        let node = node_starting_at(self.document, cell.source_pos)?;
        if text_fingerprint_of(node) != pinned.cell.text_fingerprint {
            return None;
        }
        let points = self.cell_text_points(&cells, target)?;
        if run_structure_of(&points) == pinned.cell.run_structure {
            return scalar_at_point(&points, pinned.point);
        }
        scalar_at_text_offset(&points, pinned.point, affinity)
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
        let cells = self.cells_in_document_order();
        let target = cells
            .binary_search_by_key(&cell.source_pos, |(_, cell)| cell.source_pos)
            .ok()?;
        let node = node_starting_at(self.document, cell.source_pos)?;
        let points = self.cell_text_points(&cells, target)?;
        if self.pin(table, cell, node, &points)? != *pinned.cell {
            return None;
        }
        scalar_at_point(&points, pinned.point)
    }

    fn tables(&self) -> impl Iterator<Item = &ProjectedTable> + '_ {
        self.index
            .positions()
            .filter_map(|position| self.index.table_at(position))
    }

    fn cells_in_document_order(&self) -> Vec<(&ProjectedTable, &ProjectedCell)> {
        let mut cells: Vec<(&ProjectedTable, &ProjectedCell)> = self
            .tables()
            .flat_map(|table| table.cells.iter().map(move |cell| (table, cell)))
            .collect();
        cells.sort_by_key(|(_, cell)| cell.source_pos);
        cells
    }

    fn cell_text_points(
        &self,
        cells: &[(&ProjectedTable, &ProjectedCell)],
        target: usize,
    ) -> Option<Vec<(u32, CellTextPoint)>> {
        self.text_points(
            cells,
            (0..=self.position_map.total_scalars())
                .map(|scalar| self.position_map.scalar_to_doc(scalar, self.document)),
        )
        .into_iter()
        .nth(target)
    }

    fn text_points(
        &self,
        cells: &[(&ProjectedTable, &ProjectedCell)],
        doc_positions: impl Iterator<Item = u32>,
    ) -> Vec<Vec<(u32, CellTextPoint)>> {
        let mut points: Vec<Vec<(u32, CellTextPoint)>> = vec![Vec::new(); cells.len()];
        let mut previous: Vec<Option<(u32, CellTextPoint)>> = vec![None; cells.len()];
        let mut open: Vec<usize> = Vec::new();
        let mut next_cell = 0;
        for (scalar, doc_pos) in doc_positions.enumerate() {
            let scalar = u32::try_from(scalar).expect("position map scalar bounds fit u32");
            while open
                .last()
                .is_some_and(|&innermost| cells[innermost].1.source_end <= doc_pos)
            {
                open.pop();
            }
            while let Some((_, cell)) = cells.get(next_cell) {
                if cell.source_pos >= doc_pos {
                    break;
                }
                if doc_pos < cell.source_end {
                    open.push(next_cell);
                }
                next_cell += 1;
            }
            let Some(&innermost) = open.last() else {
                continue;
            };
            let point = next_text_point(previous[innermost], doc_pos);
            previous[innermost] = Some((doc_pos, point));
            points[innermost].push((scalar, point));
        }
        for cell_points in &mut points {
            attach_run_starts(cell_points);
        }
        points
    }

    fn pin(
        &self,
        table: &ProjectedTable,
        cell: &ProjectedCell,
        node: &Node,
        points: &[(u32, CellTextPoint)],
    ) -> Option<PinnedTableCell> {
        let mut content_fingerprint = CellFingerprintSink(DefaultHasher::new());
        for child in node.content()?.iter() {
            crate::serialize::json_out::write_node_json(
                &mut content_fingerprint,
                child,
                self.schema,
            )
            .expect("cell fingerprint writes are infallible");
            const STRING_HASH_TERMINATOR: u8 = 0xff;
            content_fingerprint.0.write_u8(STRING_HASH_TERMINATOR);
        }
        Some(PinnedTableCell {
            row: cell.rect.row,
            column: cell.rect.column,
            rowspan: cell.rect.rowspan,
            colspan: cell.rect.colspan,
            table_rows: table.rows,
            table_columns: table.columns,
            content_fingerprint: content_fingerprint.0.finish(),
            text_fingerprint: text_fingerprint_of(node),
            run_structure: run_structure_of(points),
        })
    }
}

fn next_text_point(previous: Option<(u32, CellTextPoint)>, doc_pos: u32) -> CellTextPoint {
    match previous {
        None => CellTextPoint {
            text_offset: 0,
            run: 0,
            run_offset: 0,
            attachment: CellTextAttachment::PrecedingText,
        },
        Some((previous_doc_pos, point)) if previous_doc_pos == doc_pos => point,
        Some((previous_doc_pos, point))
            if previous_doc_pos.checked_add(ADJACENT_TEXT_POSITION) == Some(doc_pos) =>
        {
            CellTextPoint {
                text_offset: point.text_offset + TEXT_STEP,
                run_offset: point.run_offset + TEXT_STEP,
                ..point
            }
        }
        Some((_, point)) => CellTextPoint {
            run: point.run + RUN_STEP,
            run_offset: 0,
            ..point
        },
    }
}

fn attach_run_starts(points: &mut [(u32, CellTextPoint)]) {
    let mut run_with_text = None;
    for (_, point) in points.iter_mut().rev() {
        if point.run_offset > 0 {
            run_with_text = Some(point.run);
        } else if run_with_text == Some(point.run) {
            point.attachment = CellTextAttachment::FollowingText;
        }
    }
}

fn nodes_starting_at<'doc>(document: &'doc Document, positions: &[u32]) -> Vec<Option<&'doc Node>> {
    let mut found = Vec::with_capacity(positions.len());
    collect_nodes_starting_at(document.root(), 0, positions, &mut found);
    found.resize(positions.len(), None);
    found
}

fn collect_nodes_starting_at<'doc>(
    parent: &'doc Node,
    content_start: u32,
    positions: &[u32],
    found: &mut Vec<Option<&'doc Node>>,
) {
    let Some(content) = parent.content() else {
        return;
    };
    let mut position = content_start;
    for child in content.iter() {
        let end = position.saturating_add(child.node_size());
        while positions
            .get(found.len())
            .is_some_and(|&wanted| wanted < position)
        {
            found.push(None);
        }
        if positions.get(found.len()) == Some(&position) {
            found.push(Some(child));
        }
        if positions
            .get(found.len())
            .is_some_and(|&wanted| wanted < end)
        {
            collect_nodes_starting_at(
                child,
                position.saturating_add(NODE_OPENING_TOKENS),
                positions,
                found,
            );
        }
        position = end;
    }
}

fn scalar_at_point(points: &[(u32, CellTextPoint)], target: CellTextPoint) -> Option<u32> {
    points
        .iter()
        .find(|(_, point)| point.run == target.run && point.run_offset == target.run_offset)
        .map(|(scalar, _)| *scalar)
}

fn scalar_at_text_offset(
    points: &[(u32, CellTextPoint)],
    target: CellTextPoint,
    affinity: Affinity,
) -> Option<u32> {
    let at_offset = || {
        points
            .iter()
            .filter(move |(_, point)| point.text_offset == target.text_offset)
    };
    let same_side: Vec<u32> = at_offset()
        .filter(|(_, point)| point.attachment == target.attachment)
        .map(|(scalar, _)| *scalar)
        .collect();
    let candidates = if same_side.is_empty() {
        at_offset().map(|(scalar, _)| *scalar).collect()
    } else {
        same_side
    };
    match affinity {
        Affinity::Before => candidates.first().copied(),
        Affinity::After => candidates.last().copied(),
    }
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

#[cfg(test)]
#[path = "position_epoch_cells_tests.rs"]
mod tests;
