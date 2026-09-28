use std::collections::BTreeMap;
use std::sync::Arc;

use super::types::*;
use crate::render::RenderElement;
use crate::session::{EditorSession, SessionError};
use crate::tables::render::{TableRenderCell, TableRenderRecord, TableRenderStructure};
use crate::yrs_engine::YrsEngineError;

fn invariant(message: &'static str) -> SessionError {
    SessionError::from(YrsEngineError::new("ENGINE_INVARIANT_FAILED", message))
}

fn same_layout(a: &TableRenderStructure, b: &TableRenderStructure) -> bool {
    a.rows == b.rows
        && a.columns == b.columns
        && a.column_widths == b.column_widths
        && a.direction == b.direction
        && a.irregular == b.irregular
        && a.read_only_descendants == b.read_only_descendants
        && a.attrs_key == b.attrs_key
        && a.source_rows == b.source_rows
        && a.synthetic_regions == b.synthetic_regions
        && a.failure == b.failure
        && a.compatibility_diagnostic == b.compatibility_diagnostic
}

fn same_cell_layout(a: &TableRenderCell, b: &TableRenderCell) -> bool {
    (
        a.source_row,
        a.row,
        a.column,
        a.rowspan,
        a.colspan,
        a.header,
    ) == (
        b.source_row,
        b.row,
        b.column,
        b.rowspan,
        b.colspan,
        b.header,
    )
}

pub(crate) fn same_cell(a: &Arc<TableRenderCell>, b: &Arc<TableRenderCell>) -> bool {
    Arc::ptr_eq(a, b)
        || (a.content_key == b.content_key
            && a.attrs_key == b.attrs_key
            && a.doc_size == b.doc_size)
}

pub(super) struct TableContext<'a> {
    position: u32,
    key: String,
    record: &'a TableRenderRecord,
    starts: Vec<u32>,
    host: Option<FfiTableHost>,
}

pub(super) fn contexts<'a>(
    records: &[(u32, &'a TableRenderRecord)],
    keys: &BTreeMap<u32, String>,
) -> Result<Vec<TableContext<'a>>, SessionError> {
    let mut output: Vec<TableContext<'a>> = Vec::with_capacity(records.len());
    let mut ancestors: Vec<usize> = Vec::new();
    for &(position, record) in records {
        while ancestors.last().is_some_and(|index| {
            position >= output[*index].position + output[*index].record.structure.doc_size
        }) {
            ancestors.pop();
        }
        let host = ancestors.last().and_then(|parent| {
            let parent = &output[*parent];
            let index = parent
                .starts
                .partition_point(|start| *start <= position)
                .checked_sub(1)?;
            (position < parent.starts[index] + parent.record.cells[index].doc_size).then(|| {
                FfiTableHost {
                    table_key: parent.key.clone(),
                    cell_index: index as u32,
                }
            })
        });
        ancestors.push(output.len());
        output.push(TableContext {
            position,
            key: keys
                .get(&position)
                .ok_or_else(|| invariant("table identity missing"))?
                .clone(),
            record,
            starts: crate::tables::render::absolute_cell_starts(record, position),
            host,
        });
    }
    Ok(output)
}

fn cell_record(
    session: &EditorSession,
    context: &TableContext<'_>,
    index: usize,
    keys: &BTreeMap<u32, String>,
) -> Result<FfiTableCellRecord, SessionError> {
    let cell = &context.record.cells[index];
    let start = context.starts[index];
    let map = session
        .engine
        .position_map()
        .ok_or_else(|| invariant("position map missing"))?;
    let document = session
        .engine
        .document()
        .ok_or_else(|| invariant("document missing"))?;
    let (origin, input_blocks, nested_tables) =
        super::table_input_mapping::relative_cell_mapping(document, map, cell, start, keys)
            .map_err(invariant)?;
    let scalar_end = if let Some(next) = context.starts.get(index + 1) {
        super::table_input_mapping::scalar_range(
            map,
            *next,
            *next + context.record.cells[index + 1].doc_size,
        )
        .0
    } else {
        super::table_input_mapping::scalar_range(
            map,
            context.position,
            context.position + context.record.structure.doc_size,
        )
        .1
    };
    let elements = cell
        .elements
        .iter()
        .map(|element| match element {
            RenderElement::Table { doc_offset, .. } => keys
                .get(&(start + doc_offset))
                .map(|key| FfiViewerElement::Table {
                    table_id: key.clone(),
                })
                .ok_or_else(|| invariant("nested table identity missing")),
            _ => Ok(crate::viewer::viewer_leaf_element(element.clone(), None, 0)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FfiTableCellRecord {
        source_row: cell.source_row,
        row: cell.row,
        column: cell.column,
        rowspan: cell.rowspan,
        colspan: cell.colspan,
        header: cell.header,
        attrs_key: cell.attrs_key.clone(),
        content_key: cell.content_key.clone(),
        doc_size: cell.doc_size,
        scalar_stride: scalar_end
            .checked_sub(origin)
            .ok_or_else(|| invariant("cell scalar stride is reversed"))?,
        elements,
        input_blocks,
        nested_tables,
    })
}

pub(super) fn table_record(
    session: &EditorSession,
    context: &TableContext<'_>,
    keys: &BTreeMap<u32, String>,
) -> Result<FfiTableRecord, SessionError> {
    let structure = &context.record.structure;
    Ok(FfiTableRecord {
        table_key: context.key.clone(),
        host: context.host.clone(),
        doc_size: structure.doc_size,
        rows: structure.rows,
        columns: structure.columns,
        column_widths: structure.column_widths.clone(),
        direction: structure.direction.clone(),
        irregular: structure.irregular,
        read_only_descendants: structure.read_only_descendants,
        attrs_key: structure.attrs_key.clone(),
        source_rows: structure
            .source_rows
            .iter()
            .map(|row| FfiTableSourceRow {
                attrs_key: row.attrs_key.clone(),
                cell_count: row.cell_count,
            })
            .collect(),
        cells: (0..context.record.cells.len())
            .map(|index| cell_record(session, context, index, keys))
            .collect::<Result<Vec<_>, _>>()?,
        synthetic_regions: structure.synthetic_regions.clone(),
        failure: structure.failure,
        compatibility_diagnostic: structure.compatibility_diagnostic,
    })
}

pub(crate) fn build_native_frame(
    session: &mut EditorSession,
    editor_id: &str,
    owner_id: Option<u64>,
    mirror: Option<(u32, u32)>,
) -> Result<FfiNativeRenderFrame, SessionError> {
    let current = session
        .engine
        .cached_render_blocks()
        .ok_or_else(|| invariant("render cache missing"))?;
    let previous = owner_id
        .and_then(|owner| session.native_render_cursor(owner))
        .filter(|cursor| cursor.schema_fingerprint == session.engine.schema_fingerprint());
    let revision = session.engine.revision();
    let mut tables = FfiTableFrame {
        kind: if previous.is_some() {
            FfiTableFrameKind::Delta
        } else {
            FfiTableFrameKind::Full
        },
        base_document_revision: previous
            .as_ref()
            .map(|cursor| cursor.document_revision.to_string()),
        attributes: Vec::new(),
        removed_attribute_keys: Vec::new(),
        tables: Vec::new(),
        removed_table_keys: Vec::new(),
        cell_updates: Vec::new(),
        extents: Vec::new(),
    };
    if !previous
        .as_ref()
        .is_some_and(|cursor| cursor.document_revision == revision)
    {
        let keys = super::render::table_keys(&session.engine)?;
        let mut records = Vec::new();
        current.visit_table_records(&mut records);
        records.sort_by_key(|(position, _)| *position);
        let current_tables = contexts(&records, &keys)?;
        let mut previous_records = Vec::new();
        if let Some(cursor) = &previous {
            cursor
                .render_blocks
                .visit_table_records(&mut previous_records);
        }
        previous_records.sort_by_key(|(position, _)| *position);
        let old_hosts: BTreeMap<_, _> = if let Some(cursor) = &previous {
            contexts(&previous_records, &cursor.table_keys)?
                .into_iter()
                .map(|context| (context.key, context.host))
                .collect()
        } else {
            BTreeMap::new()
        };
        let old: BTreeMap<_, _> = previous_records
            .into_iter()
            .map(|(position, record)| {
                let key = previous
                    .as_ref()
                    .and_then(|cursor| cursor.table_keys.get(&position))
                    .ok_or_else(|| invariant("previous table identity missing"))?;
                Ok((key.as_str(), record))
            })
            .collect::<Result<_, SessionError>>()?;
        for (key, json) in &current.table_attributes {
            if !previous
                .as_ref()
                .is_some_and(|cursor| cursor.render_blocks.table_attributes.contains_key(key))
            {
                tables.attributes.push(FfiTableAttribute {
                    key: key.clone(),
                    json: json.to_string(),
                });
            }
        }
        if let Some(cursor) = &previous {
            tables.removed_attribute_keys = cursor
                .render_blocks
                .table_attributes
                .keys()
                .filter(|key| !current.table_attributes.contains_key(*key))
                .cloned()
                .collect();
            let live: std::collections::BTreeSet<_> = keys.values().map(String::as_str).collect();
            tables.removed_table_keys = old
                .keys()
                .filter(|key| !live.contains(**key))
                .map(|key| (*key).to_owned())
                .collect();
        }
        let map = session
            .engine
            .position_map()
            .ok_or_else(|| invariant("position map missing"))?;
        for context in &current_tables {
            if context.host.is_none() {
                let (scalar_start, scalar_end) = super::table_input_mapping::scalar_range(
                    map,
                    context.position,
                    context.position + context.record.structure.doc_size,
                );
                tables.extents.push(FfiTableExtent {
                    table_key: context.key.clone(),
                    doc_start: context.position,
                    doc_size: context.record.structure.doc_size,
                    scalar_start,
                    scalar_end,
                });
            }
            match old.get(context.key.as_str()).filter(|old| {
                old_hosts.get(&context.key) == Some(&context.host)
                    && old.cells.len() == context.record.cells.len()
                    && old
                        .cells
                        .iter()
                        .zip(&context.record.cells)
                        .all(|(a, b)| Arc::ptr_eq(a, b) || same_cell_layout(a, b))
                    && (Arc::ptr_eq(&old.structure, &context.record.structure)
                        || same_layout(&old.structure, &context.record.structure))
            }) {
                Some(old) => {
                    for (index, (before, after)) in
                        old.cells.iter().zip(&context.record.cells).enumerate()
                    {
                        if !same_cell(before, after) {
                            tables.cell_updates.push(FfiTableCellUpdate {
                                table_key: context.key.clone(),
                                cell_index: index as u32,
                                cell: cell_record(session, context, index, &keys)?,
                            });
                        }
                    }
                }
                None => tables.tables.push(table_record(session, context, &keys)?),
            }
        }
    }
    let snapshot_json = super::render::root_snapshot_json(session, editor_id, owner_id, mirror)?;
    if let Some(owner) = owner_id.filter(|_| {
        !previous
            .as_ref()
            .is_some_and(|cursor| cursor.document_revision == revision)
    }) {
        session.retain_native_render_cursor(owner, revision, current);
    }
    Ok(FfiNativeRenderFrame {
        snapshot_json,
        tables,
    })
}
