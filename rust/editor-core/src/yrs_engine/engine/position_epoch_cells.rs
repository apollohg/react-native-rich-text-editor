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

#[cfg(test)]
thread_local! {
    static PINNED_CELL_SERIALIZATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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
    pub(super) render_blocks: &'state crate::render::incremental::CachedRenderBlocks,
    pub(super) schema_fingerprint: &'state str,
}

struct RenderedCellFingerprints<'state> {
    tables: Vec<(
        usize,
        &'state ProjectedTable,
        &'state crate::tables::render::TableRenderRecord,
    )>,
}

impl RenderedCellFingerprints<'_> {
    fn fingerprint(
        &self,
        table: &ProjectedTable,
        cell: &ProjectedCell,
        node: &Node,
    ) -> Option<u64> {
        let key = table as *const ProjectedTable as usize;
        let entry = self
            .tables
            .binary_search_by_key(&key, |entry| entry.0)
            .ok()?;
        let (_, projected, rendered) = self.tables[entry];
        if table.rows != projected.rows || table.columns != projected.columns {
            return None;
        }
        let index = projected
            .cells
            .binary_search_by_key(&cell.source_pos, |cell| cell.source_pos)
            .ok()?;
        if projected.cells[index] != *cell {
            return None;
        }
        let rendered = rendered.cells.get(index)?;
        (rendered.doc_size == node.node_size()
            && rendered.row == cell.rect.row
            && rendered.column == cell.rect.column
            && rendered.rowspan == cell.rect.rowspan
            && rendered.colspan == cell.rect.colspan)
            .then(|| rendered.content_key.cell_fingerprint())
            .flatten()
    }
}

impl CellPinning<'_> {
    fn rendered_fingerprints(&self) -> RenderedCellFingerprints<'_> {
        let mut tables = Vec::new();
        if self
            .render_blocks
            .matches_identity(self.document, self.schema_fingerprint)
        {
            let mut records = Vec::new();
            self.render_blocks.visit_table_records(&mut records);
            for (position, record) in records {
                let Some(source) = self.index.table_at(position) else {
                    continue;
                };
                let Some(projected) = self.render_blocks.table_projection_index.table_at(position)
                else {
                    continue;
                };
                if record.source_fallback.is_none()
                    && record.cells.len() == projected.cells.len()
                    && record.structure.rows == projected.rows
                    && record.structure.columns == projected.columns
                {
                    tables.push((source as *const ProjectedTable as usize, projected, record));
                }
            }
            tables.sort_unstable_by_key(|entry| entry.0);
        }
        RenderedCellFingerprints { tables }
    }

    pub(super) fn spans(&self, doc_positions: &[Vec<u32>]) -> Vec<Arc<PinnedCellSpan>> {
        let cells = self.cells_in_document_order();
        let points = self.text_points(
            &cells,
            doc_positions
                .iter()
                .flatten()
                .enumerate()
                .map(|(scalar, position)| {
                    (
                        u32::try_from(scalar).expect("admitted scalar fits u32"),
                        *position,
                    )
                }),
        );
        let starts: Vec<u32> = cells.iter().map(|(_, cell)| cell.source_pos).collect();
        let nodes = nodes_starting_at(self.document, &starts);
        let fingerprints = self.rendered_fingerprints();
        cells
            .into_iter()
            .zip(nodes)
            .zip(points)
            .filter_map(|(((table, cell), node), points)| {
                let (node_path, node) = node?;
                self.span(
                    table,
                    cell,
                    node,
                    node_path,
                    points,
                    fingerprints.fingerprint(table, cell, node),
                )
                .map(Arc::new)
            })
            .collect()
    }

    pub(super) fn rebuild_spans(
        &self,
        previous: &[&PinnedCellSpan],
        positions: &[(usize, Vec<u32>)],
    ) -> Option<Vec<Arc<PinnedCellSpan>>> {
        let fingerprints = self.rendered_fingerprints();
        let mut cells = Vec::with_capacity(previous.len());
        for span in previous {
            let table_path = span.node_path.get(..span.node_path.len().checked_sub(2)?)?;
            let table_position = crate::yrs_engine::compiler::node_boundary_position(
                self.document.root(),
                table_path,
            )?;
            let table = self.index.table_at(table_position)?;
            let cell_position = crate::yrs_engine::compiler::node_boundary_position(
                self.document.root(),
                &span.node_path,
            )?;
            let index = table
                .cells
                .binary_search_by_key(&cell_position, |cell| cell.source_pos)
                .ok()?;
            cells.push((table, &table.cells[index]));
        }
        let points = self.text_points(
            &cells,
            positions.iter().flat_map(|(block, positions)| {
                let start = self.position_map.effective_scalar_start(*block);
                positions.iter().enumerate().map(move |(offset, position)| {
                    (
                        start + u32::try_from(offset).expect("block offset fits u32"),
                        *position,
                    )
                })
            }),
        );
        previous
            .iter()
            .zip(cells)
            .zip(points)
            .map(|((previous, (table, cell)), points)| {
                let node = self.document.node_at(&previous.node_path)?;
                self.span(
                    table,
                    cell,
                    node,
                    previous.node_path.clone(),
                    points,
                    fingerprints.fingerprint(table, cell, node),
                )
                .map(Arc::new)
            })
            .collect()
    }

    fn span(
        &self,
        table: &ProjectedTable,
        cell: &ProjectedCell,
        node: &Node,
        node_path: Vec<u32>,
        mut points: Vec<(u32, CellTextPoint)>,
        content_fingerprint: Option<u64>,
    ) -> Option<PinnedCellSpan> {
        let pinned = self.pin(table, cell, node, &points, content_fingerprint)?;
        let block_range = self.position_map.block_range_for_path(&node_path);
        if !points.is_empty() {
            if block_range.is_empty() {
                return None;
            }
            let start = self.position_map.effective_scalar_start(block_range.start);
            for (scalar, _) in &mut points {
                *scalar = scalar.checked_sub(start)?;
            }
        }
        Some(PinnedCellSpan {
            node_path,
            block_range,
            cell: pinned,
            points,
        })
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
        if self.pin(table, cell, node, &points, None)? != *pinned.cell {
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
            (0..=self.position_map.total_scalars()).map(|scalar| {
                (
                    scalar,
                    self.position_map.scalar_to_doc(scalar, self.document),
                )
            }),
        )
        .into_iter()
        .nth(target)
    }

    fn text_points(
        &self,
        cells: &[(&ProjectedTable, &ProjectedCell)],
        doc_positions: impl Iterator<Item = (u32, u32)>,
    ) -> Vec<Vec<(u32, CellTextPoint)>> {
        let mut points: Vec<Vec<(u32, CellTextPoint)>> = vec![Vec::new(); cells.len()];
        let mut previous: Vec<Option<(u32, CellTextPoint)>> = vec![None; cells.len()];
        let mut open: Vec<usize> = Vec::new();
        let mut next_cell = 0;
        for (scalar, doc_pos) in doc_positions {
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
        content_fingerprint: Option<u64>,
    ) -> Option<PinnedTableCell> {
        let content = node.content()?;
        let content_fingerprint = content_fingerprint.unwrap_or_else(|| {
            let mut fingerprint = CellFingerprintSink(DefaultHasher::new());
            for child in content.iter() {
                #[cfg(test)]
                PINNED_CELL_SERIALIZATIONS.set(PINNED_CELL_SERIALIZATIONS.get() + 1);
                crate::serialize::json_out::write_node_json(&mut fingerprint, child, self.schema)
                    .expect("cell fingerprint writes are infallible");
                fingerprint.0.write_u8(crate::tables::render::CELL_FINGERPRINT_CHILD_TERMINATOR);
            }
            fingerprint.0.finish()
        });
        Some(PinnedTableCell {
            row: cell.rect.row,
            column: cell.rect.column,
            rowspan: cell.rect.rowspan,
            colspan: cell.rect.colspan,
            table_rows: table.rows,
            table_columns: table.columns,
            content_fingerprint,
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

fn nodes_starting_at<'doc>(
    document: &'doc Document,
    positions: &[u32],
) -> Vec<Option<(Vec<u32>, &'doc Node)>> {
    let mut found = Vec::with_capacity(positions.len());
    collect_nodes_starting_at(document.root(), 0, positions, &mut Vec::new(), &mut found);
    found.resize(positions.len(), None);
    found
}

fn collect_nodes_starting_at<'doc>(
    parent: &'doc Node,
    content_start: u32,
    positions: &[u32],
    path: &mut Vec<u32>,
    found: &mut Vec<Option<(Vec<u32>, &'doc Node)>>,
) {
    let Some(content) = parent.content() else {
        return;
    };
    let mut position = content_start;
    for (index, child) in content.iter().enumerate() {
        path.push(u32::try_from(index).expect("admitted child index fits u32"));
        let end = position.saturating_add(child.node_size());
        while positions
            .get(found.len())
            .is_some_and(|&wanted| wanted < position)
        {
            found.push(None);
        }
        if positions.get(found.len()) == Some(&position) {
            found.push(Some((path.clone(), child)));
        }
        if positions
            .get(found.len())
            .is_some_and(|&wanted| wanted < end)
        {
            collect_nodes_starting_at(
                child,
                position.saturating_add(NODE_OPENING_TOKENS),
                positions,
                path,
                found,
            );
        }
        position = end;
        path.pop();
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
