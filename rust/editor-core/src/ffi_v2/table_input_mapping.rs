use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::model::Document;
use crate::position::PositionMap;
use crate::render::incremental::CachedRenderBlocks;
use crate::render::RenderElement;
use crate::tables::render::{TableRenderCell, TableRenderRecord};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableInputMappings {
    version: u8,
    tables: BTreeMap<String, TableInputTable>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableInputTable {
    extent: Option<ScalarExtent>,
    cells: Vec<TableInputCell>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableInputCell {
    cell_index: u32,
    source_pos: u32,
    source_end: u32,
    blocks: Vec<TableInputBlock>,
    excluded: Vec<TableInputExcluded>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableInputBlock {
    element_index: u32,
    doc_start: u32,
    doc_end: u32,
    scalar_start: u32,
    content_scalar_start: u32,
    scalar_end: u32,
    break_scalar_end: u32,
    void: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TableInputExcluded {
    element_index: u32,
    table_id: String,
    extent: Option<ScalarExtent>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScalarExtent {
    scalar_start: u32,
    scalar_end: u32,
}

struct TableScope<'a> {
    record: &'a TableRenderRecord,
    table_id: String,
}

struct CellScope<'a> {
    start: u32,
    end: u32,
    table_index: usize,
    cell_index: usize,
    cell: &'a TableRenderCell,
}

struct PendingCell {
    block_indices: Vec<usize>,
    excluded: Vec<TableInputExcluded>,
}

pub(crate) fn derive(
    document: &Document,
    position_map: &PositionMap,
    render_blocks: &CachedRenderBlocks,
) -> Result<Option<Value>, &'static str> {
    let mut records = Vec::new();
    render_blocks.visit_table_records(&mut records);
    if records.is_empty() {
        return Ok(None);
    }
    records.sort_by_key(|record| record.table_pos);

    let tables: Vec<_> = records
        .into_iter()
        .map(|record| TableScope {
            record,
            table_id: format!("t{}", record.table_pos),
        })
        .collect();
    let mut cells = Vec::new();
    for (table_index, table) in tables.iter().enumerate() {
        for (cell_index, cell) in table.record.cells.iter().enumerate() {
            cells.push(CellScope {
                start: cell.source_pos,
                end: cell.source_end,
                table_index,
                cell_index,
                cell,
            });
        }
    }
    cells.sort_by_key(|cell| (cell.start, std::cmp::Reverse(cell.end)));

    let mut pending: Vec<Vec<PendingCell>> = tables
        .iter()
        .map(|table| {
            table
                .record
                .cells
                .iter()
                .map(|_| PendingCell {
                    block_indices: Vec::new(),
                    excluded: Vec::new(),
                })
                .collect()
        })
        .collect();
    assign_direct_blocks(position_map, &tables, &cells, &mut pending);
    assign_nested_exclusions(&tables, &cells, position_map, &mut pending)?;

    let mut output = BTreeMap::new();
    for (table_index, table) in tables.iter().enumerate() {
        let mapped_cells = table
            .record
            .cells
            .iter()
            .enumerate()
            .map(|(cell_index, cell)| {
                let pending_cell = &pending[table_index][cell_index];
                let blocks = serialize_cell_blocks(
                    document,
                    position_map,
                    cell,
                    &pending_cell.block_indices,
                )?;
                Ok(TableInputCell {
                    cell_index: u32::try_from(cell_index).map_err(|_| "cell index overflow")?,
                    source_pos: cell.source_pos,
                    source_end: cell.source_end,
                    blocks,
                    excluded: pending_cell.excluded.clone(),
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        output.insert(
            table.table_id.clone(),
            TableInputTable {
                extent: table_extent(
                    position_map,
                    table.record.table_pos,
                    table.record.source_end,
                ),
                cells: mapped_cells,
            },
        );
    }

    serde_json::to_value(TableInputMappings {
        version: 1,
        tables: output,
    })
    .map(Some)
    .map_err(|_| "table input mapping serialization failed")
}

fn assign_direct_blocks(
    position_map: &PositionMap,
    tables: &[TableScope<'_>],
    cells: &[CellScope<'_>],
    pending: &mut [Vec<PendingCell>],
) {
    let mut next_cell = 0;
    let mut active = Vec::new();
    let mut next_table = 0;
    let mut active_tables = Vec::new();
    for block_index in 0..position_map.block_count() {
        let doc_start = position_map.effective_doc_start(block_index);
        while next_table < tables.len() && tables[next_table].record.table_pos <= doc_start {
            active_tables.push(next_table);
            next_table += 1;
        }
        while active_tables
            .last()
            .is_some_and(|index| tables[*index].record.source_end <= doc_start)
        {
            active_tables.pop();
        }
        while next_cell < cells.len() && cells[next_cell].start <= doc_start {
            active.push(next_cell);
            next_cell += 1;
        }
        while active
            .last()
            .is_some_and(|index| cells[*index].end <= doc_start)
        {
            active.pop();
        }
        if let Some(cell_index) = active.last().copied() {
            let cell = &cells[cell_index];
            if active_tables.last() == Some(&cell.table_index) {
                pending[cell.table_index][cell.cell_index]
                    .block_indices
                    .push(block_index);
            }
        }
    }
}

fn assign_nested_exclusions(
    tables: &[TableScope<'_>],
    cells: &[CellScope<'_>],
    position_map: &PositionMap,
    pending: &mut [Vec<PendingCell>],
) -> Result<(), &'static str> {
    let mut next_cell = 0;
    let mut active = Vec::new();
    for (table_index, table) in tables.iter().enumerate() {
        let table_pos = table.record.table_pos;
        while next_cell < cells.len() && cells[next_cell].start <= table_pos {
            active.push(next_cell);
            next_cell += 1;
        }
        while active
            .last()
            .is_some_and(|index| cells[*index].end <= table_pos)
        {
            active.pop();
        }
        let Some(cell_scope_index) = active.last().copied() else {
            continue;
        };
        let cell_scope = &cells[cell_scope_index];
        if cell_scope.table_index == table_index {
            continue;
        }
        let pending_cell = &mut pending[cell_scope.table_index][cell_scope.cell_index];
        let next_element = pending_cell
            .excluded
            .last()
            .map_or(0, |entry| entry.element_index as usize + 1);
        let element_index = cell_scope
            .cell
            .elements
            .iter()
            .enumerate()
            .skip(next_element)
            .find_map(|(index, element)| {
                matches!(element, RenderElement::Table { table: nested } if nested.table_pos == table_pos).then_some(index)
            })
            .ok_or("nested table is missing its cell render element")?;
        pending_cell.excluded.push(TableInputExcluded {
            element_index: u32::try_from(element_index)
                .map_err(|_| "render element index overflow")?,
            table_id: table.table_id.clone(),
            extent: table_extent(position_map, table_pos, table.record.source_end),
        });
    }
    Ok(())
}

fn serialize_cell_blocks(
    document: &Document,
    position_map: &PositionMap,
    cell: &TableRenderCell,
    block_indices: &[usize],
) -> Result<Vec<TableInputBlock>, &'static str> {
    let mut next_element = 0;
    let cell_scalar_end = table_extent(position_map, cell.source_pos, cell.source_end)
        .map(|extent| extent.scalar_end);
    block_indices
        .iter()
        .map(|block_index| {
            let block = position_map
                .block(*block_index)
                .ok_or("position block index is invalid")?;
            let doc_start = position_map.effective_doc_start(*block_index);
            let element_index =
                find_block_element_index(document, cell, block, doc_start, &mut next_element)?;
            let scalar_start = position_map.effective_scalar_start(*block_index);
            let content_scalar_start = scalar_start
                .checked_add(block.scalar_prefix_len)
                .ok_or("scalar prefix overflow")?;
            let scalar_end = content_scalar_start
                .checked_add(block.scalar_len)
                .ok_or("scalar content overflow")?;
            let break_scalar_end = scalar_end
                .checked_add(block.rendered_break_after)
                .ok_or("scalar break overflow")?
                .min(cell_scalar_end.unwrap_or(scalar_end));
            Ok(TableInputBlock {
                element_index: u32::try_from(element_index)
                    .map_err(|_| "render element index overflow")?,
                doc_start,
                doc_end: position_map.effective_doc_end(*block_index),
                scalar_start,
                content_scalar_start,
                scalar_end,
                break_scalar_end,
                void: block.is_void_block,
            })
        })
        .collect()
}

fn find_block_element_index(
    document: &Document,
    cell: &TableRenderCell,
    block: &crate::position::BlockMapping,
    doc_start: u32,
    next_element: &mut usize,
) -> Result<usize, &'static str> {
    let node_type = document
        .node_at(&block.node_path)
        .map(|node| node.node_type());
    let index = cell
        .elements
        .iter()
        .enumerate()
        .skip(*next_element)
        .find_map(|(index, element)| match element {
            RenderElement::BlockStart {
                node_type: rendered,
                ..
            } if !block.is_void_block && Some(rendered.as_str()) == node_type => Some(index),
            RenderElement::VoidBlock { doc_pos, .. }
            | RenderElement::OpaqueBlockAtom { doc_pos, .. }
                if block.is_void_block && *doc_pos == doc_start =>
            {
                Some(index)
            }
            _ => None,
        })
        .ok_or("position block is missing its cell render element")?;
    *next_element = index
        .checked_add(1)
        .ok_or("render element index overflow")?;
    Ok(index)
}

fn table_extent(
    position_map: &PositionMap,
    table_pos: u32,
    table_end: u32,
) -> Option<ScalarExtent> {
    let first = lower_bound_doc_start(position_map, table_pos);
    let end = lower_bound_doc_start(position_map, table_end);
    (first < end).then(|| {
        let last_index = end - 1;
        let last_block = position_map
            .block(last_index)
            .expect("lower bound is in range");
        ScalarExtent {
            scalar_start: position_map.effective_scalar_start(first),
            scalar_end: position_map
                .effective_scalar_start(last_index)
                .saturating_add(last_block.scalar_prefix_len)
                .saturating_add(last_block.scalar_len),
        }
    })
}

fn lower_bound_doc_start(position_map: &PositionMap, target: u32) -> usize {
    let mut low = 0;
    let mut high = position_map.block_count();
    while low < high {
        let middle = low + (high - low) / 2;
        if position_map.effective_doc_start(middle) < target {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}
