use std::collections::BTreeMap;

use crate::model::Document;
use crate::position::PositionMap;
use crate::render::RenderElement;
use crate::tables::render::TableRenderCell;

struct ScalarExtent {
    scalar_start: u32,
    scalar_end: u32,
}

fn cell_input_blocks(
    document: &Document,
    position_map: &PositionMap,
    cell: &TableRenderCell,
    cell_start: u32,
    block_indices: &[usize],
    cell_scalar_end: Option<u32>,
) -> Result<Vec<super::types::FfiCellInputBlock>, &'static str> {
    let mut next_element = 0;
    block_indices
        .iter()
        .map(|block_index| {
            let block = position_map
                .block(*block_index)
                .ok_or("position block index is invalid")?;
            let doc_start = position_map.effective_doc_start(*block_index);
            let element_index = find_block_element_index(
                document,
                cell,
                block,
                doc_start - cell_start,
                &mut next_element,
            )?;
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
            Ok(super::types::FfiCellInputBlock {
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
    extent_for_blocks(position_map, first, end)
}

fn extent_for_blocks(position_map: &PositionMap, first: usize, end: usize) -> Option<ScalarExtent> {
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

fn scalar_start_at_block_boundary(position_map: &PositionMap, block: usize) -> u32 {
    if block < position_map.block_count() {
        position_map.effective_scalar_start(block)
    } else {
        position_map.total_scalars()
    }
}

pub(crate) fn scalar_range(position_map: &PositionMap, start: u32, end: u32) -> (u32, u32) {
    table_extent(position_map, start, end).map_or_else(
        || {
            let block = lower_bound_doc_start(position_map, start);
            let scalar = scalar_start_at_block_boundary(position_map, block);
            (scalar, scalar)
        },
        |extent| (extent.scalar_start, extent.scalar_end),
    )
}

pub(crate) fn relative_cell_mapping(
    document: &Document,
    position_map: &PositionMap,
    cell: &TableRenderCell,
    cell_start: u32,
    keys: &BTreeMap<u32, String>,
) -> Result<
    (
        u32,
        Vec<super::types::FfiCellInputBlock>,
        Vec<super::types::FfiCellNestedTable>,
    ),
    &'static str,
> {
    let cell_end = cell_start
        .checked_add(cell.doc_size)
        .ok_or("cell end overflow")?;
    let first = lower_bound_doc_start(position_map, cell_start);
    let end = lower_bound_doc_start(position_map, cell_end);
    let extent = extent_for_blocks(position_map, first, end);
    let origin = extent.as_ref().map_or_else(
        || scalar_start_at_block_boundary(position_map, first),
        |extent| extent.scalar_start,
    );
    let mut nested = Vec::new();
    let mut exclusions = Vec::new();
    for (index, element) in cell.elements.iter().enumerate() {
        if let RenderElement::Table { doc_offset, table } = element {
            let start = cell_start + doc_offset;
            let end = start + table.structure.doc_size;
            let extent = table_extent(position_map, start, end);
            nested.push(super::types::FfiCellNestedTable {
                element_index: u32::try_from(index).map_err(|_| "element index overflow")?,
                table_key: keys
                    .get(&start)
                    .ok_or("nested table identity is missing")?
                    .clone(),
                doc_offset: *doc_offset,
                doc_size: table.structure.doc_size,
                scalar_start: extent.as_ref().map(|extent| extent.scalar_start - origin),
                scalar_end: extent.as_ref().map(|extent| extent.scalar_end - origin),
            });
            exclusions.push(start..end);
        }
    }
    let indices: Vec<_> = (first..end)
        .filter(|index| {
            !exclusions
                .iter()
                .any(|range| range.contains(&position_map.effective_doc_start(*index)))
        })
        .collect();
    let blocks = cell_input_blocks(
        document,
        position_map,
        cell,
        cell_start,
        &indices,
        extent.map(|extent| extent.scalar_end),
    )?
    .into_iter()
    .map(|block| super::types::FfiCellInputBlock {
        element_index: block.element_index,
        doc_start: block.doc_start - cell_start,
        doc_end: block.doc_end - cell_start,
        scalar_start: block.scalar_start - origin,
        content_scalar_start: block.content_scalar_start - origin,
        scalar_end: block.scalar_end - origin,
        break_scalar_end: block.break_scalar_end - origin,
        void: block.void,
    })
    .collect();
    Ok((origin, blocks, nested))
}
