use crate::ffi_v2::types::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MirrorRejection {
    BaseRevisionMismatch,
    UnknownTable(String),
    CellIndexOutOfRange(String, u32),
    CellStructureChanged(String, u32),
    DocSizeMismatch(String),
    ScalarSizeMismatch(String),
    InputBlockOutOfStride(String, usize),
    MissingAttribute(String),
    DuplicateTableKey(String),
    HostMissing(String),
    ExtentsIncomplete,
    InvalidSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TableFrameMirror {
    pub(crate) revision: Option<String>,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) tables: BTreeMap<String, FfiTableRecord>,
    pub(crate) extents: BTreeMap<String, FfiTableExtent>,
    pub(crate) root_blocks: Vec<serde_json::Value>,
}

impl TableFrameMirror {
    pub(crate) fn apply(&mut self, frame: &FfiNativeRenderFrame) -> Result<(), MirrorRejection> {
        let snapshot: serde_json::Value = serde_json::from_str(&frame.snapshot_json)
            .map_err(|_| MirrorRejection::InvalidSnapshot)?;
        let revision = snapshot["documentVersion"]
            .as_str()
            .filter(|value| parse_canonical_u64(value).is_some())
            .ok_or(MirrorRejection::InvalidSnapshot)?
            .to_owned();
        for field in ["stateRevision"] {
            if snapshot[field]
                .as_str()
                .and_then(parse_canonical_u64)
                .is_none()
            {
                return Err(MirrorRejection::InvalidSnapshot);
            }
        }
        if snapshot["scalarLength"].as_u64().is_none()
            || !snapshot["selection"].is_object()
            || !snapshot["activeState"].is_object()
            || !snapshot["historyState"].is_object()
        {
            return Err(MirrorRejection::InvalidSnapshot);
        }
        let update = &frame.tables;
        if update.kind == FfiTableFrameKind::Delta
            && (self.revision.is_none() || update.base_document_revision != self.revision)
        {
            return Err(MirrorRejection::BaseRevisionMismatch);
        }
        let mut next = if update.kind == FfiTableFrameKind::Full {
            Self::default()
        } else {
            self.clone()
        };
        if let Some(blocks) = snapshot["renderBlocks"].as_array() {
            next.root_blocks = blocks.clone();
        } else {
            let patch = &snapshot["renderPatch"];
            if patch["baseDocumentVersion"].as_str() != self.revision.as_deref() {
                return Err(MirrorRejection::InvalidSnapshot);
            }
            let start = patch["startIndex"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or(MirrorRejection::InvalidSnapshot)?;
            let count = patch["deleteCount"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or(MirrorRejection::InvalidSnapshot)?;
            let end = start
                .checked_add(count)
                .filter(|end| *end <= next.root_blocks.len())
                .ok_or(MirrorRejection::InvalidSnapshot)?;
            let blocks = patch["renderBlocks"]
                .as_array()
                .ok_or(MirrorRejection::InvalidSnapshot)?;
            next.root_blocks.splice(start..end, blocks.clone());
        }
        for key in &update.removed_attribute_keys {
            next.attributes.remove(key);
        }
        for attribute in &update.attributes {
            if serde_json::from_str::<serde_json::Value>(&attribute.json).is_err() {
                return Err(MirrorRejection::MissingAttribute(attribute.key.clone()));
            }
            next.attributes
                .insert(attribute.key.clone(), attribute.json.clone());
        }
        for key in &update.removed_table_keys {
            if next.tables.remove(key).is_none() {
                return Err(MirrorRejection::UnknownTable(key.clone()));
            }
            next.extents.remove(key);
        }
        let mut replaced = BTreeSet::new();
        for table in &update.tables {
            if !replaced.insert(table.table_key.clone()) {
                return Err(MirrorRejection::DuplicateTableKey(table.table_key.clone()));
            }
            next.tables.insert(table.table_key.clone(), table.clone());
        }
        let mut changed = BTreeSet::new();
        for change in &update.cell_updates {
            let table = next
                .tables
                .get_mut(&change.table_key)
                .ok_or_else(|| MirrorRejection::UnknownTable(change.table_key.clone()))?;
            let old = table
                .cells
                .get_mut(change.cell_index as usize)
                .ok_or_else(|| {
                    MirrorRejection::CellIndexOutOfRange(
                        change.table_key.clone(),
                        change.cell_index,
                    )
                })?;
            let new = &change.cell;
            if (
                old.source_row,
                old.row,
                old.column,
                old.rowspan,
                old.colspan,
                old.header,
            ) != (
                new.source_row,
                new.row,
                new.column,
                new.rowspan,
                new.colspan,
                new.header,
            ) {
                return Err(MirrorRejection::CellStructureChanged(
                    change.table_key.clone(),
                    change.cell_index,
                ));
            }
            if !changed.insert((change.table_key.clone(), change.cell_index)) {
                return Err(MirrorRejection::CellStructureChanged(
                    change.table_key.clone(),
                    change.cell_index,
                ));
            }
            table.doc_size = table
                .doc_size
                .checked_sub(old.doc_size)
                .and_then(|size| size.checked_add(new.doc_size))
                .ok_or_else(|| MirrorRejection::DocSizeMismatch(change.table_key.clone()))?;
            *old = new.clone();
        }
        let same_revision =
            self.revision.as_deref() == Some(&revision) && update.kind == FfiTableFrameKind::Delta;
        if !same_revision || !update.extents.is_empty() {
            next.extents.clear();
            for extent in &update.extents {
                if next
                    .extents
                    .insert(extent.table_key.clone(), extent.clone())
                    .is_some()
                {
                    return Err(MirrorRejection::DuplicateTableKey(extent.table_key.clone()));
                }
            }
        }
        for (key, table) in &next.tables {
            let attribute = |key: &String| {
                if next.attributes.contains_key(key) {
                    Ok(())
                } else {
                    Err(MirrorRejection::MissingAttribute(key.clone()))
                }
            };
            attribute(&table.attrs_key)?;
            for row in &table.source_rows {
                attribute(&row.attrs_key)?;
            }
            for region in &table.synthetic_regions {
                attribute(&region.attrs_key)?;
            }
            let mut doc_size = 2u64 + 2 * table.source_rows.len() as u64;
            let mut stride = 0u64;
            for (index, cell) in table.cells.iter().enumerate() {
                attribute(&cell.attrs_key)?;
                doc_size += u64::from(cell.doc_size);
                stride += u64::from(cell.scalar_stride);
                let mut previous = (0, 0);
                for block in &cell.input_blocks {
                    if block.doc_start < previous.0
                        || block.scalar_start < previous.1
                        || block.doc_start > block.doc_end
                        || block.doc_end > cell.doc_size
                        || block.scalar_start > block.content_scalar_start
                        || block.content_scalar_start > block.scalar_end
                        || block.scalar_end > block.break_scalar_end
                        || block.break_scalar_end > cell.scalar_stride
                        || block.element_index as usize >= cell.elements.len()
                    {
                        return Err(MirrorRejection::InputBlockOutOfStride(key.clone(), index));
                    }
                    previous = (block.doc_end, block.break_scalar_end);
                }
                for nested in &cell.nested_tables {
                    let child = next
                        .tables
                        .get(&nested.table_key)
                        .ok_or_else(|| MirrorRejection::UnknownTable(nested.table_key.clone()))?;
                    if child.host.as_ref()
                        != Some(&FfiTableHost {
                            table_key: key.clone(),
                            cell_index: index as u32,
                        })
                        || nested
                            .doc_offset
                            .checked_add(nested.doc_size)
                            .is_none_or(|end| end > cell.doc_size)
                    {
                        return Err(MirrorRejection::HostMissing(nested.table_key.clone()));
                    }
                    if child.doc_size != nested.doc_size {
                        return Err(MirrorRejection::DocSizeMismatch(child.table_key.clone()));
                    }
                    let width = match (nested.scalar_start, nested.scalar_end) {
                        (Some(start), Some(end)) if start <= end && end <= cell.scalar_stride => {
                            end - start
                        }
                        (None, None) => 0,
                        _ => {
                            return Err(MirrorRejection::ScalarSizeMismatch(
                                child.table_key.clone(),
                            ))
                        }
                    };
                    if child.failure.is_none()
                        && child
                            .cells
                            .iter()
                            .map(|cell| u64::from(cell.scalar_stride))
                            .sum::<u64>()
                            != u64::from(width)
                    {
                        return Err(MirrorRejection::ScalarSizeMismatch(child.table_key.clone()));
                    }
                }
            }
            if table.failure.is_none() && doc_size != u64::from(table.doc_size) {
                return Err(MirrorRejection::DocSizeMismatch(key.clone()));
            }
            if table.failure.is_some() && !table.cells.is_empty() {
                return Err(MirrorRejection::DocSizeMismatch(key.clone()));
            }
            if let Some(host) = &table.host {
                let cell = next
                    .tables
                    .get(&host.table_key)
                    .and_then(|parent| parent.cells.get(host.cell_index as usize))
                    .ok_or_else(|| MirrorRejection::HostMissing(key.clone()))?;
                if !cell
                    .nested_tables
                    .iter()
                    .any(|nested| nested.table_key == *key)
                {
                    return Err(MirrorRejection::HostMissing(key.clone()));
                }
            } else {
                let extent = next
                    .extents
                    .get(key)
                    .ok_or(MirrorRejection::ExtentsIncomplete)?;
                if extent.doc_size != table.doc_size {
                    return Err(MirrorRejection::DocSizeMismatch(key.clone()));
                }
                if extent.scalar_end < extent.scalar_start
                    || (table.failure.is_none()
                        && stride != u64::from(extent.scalar_end - extent.scalar_start))
                {
                    return Err(MirrorRejection::ScalarSizeMismatch(key.clone()));
                }
            }
        }
        if next.extents.len()
            != next
                .tables
                .values()
                .filter(|table| table.host.is_none())
                .count()
        {
            return Err(MirrorRejection::ExtentsIncomplete);
        }
        let root_keys: BTreeSet<_> = next
            .root_blocks
            .iter()
            .flat_map(|block| block.as_array().into_iter().flatten())
            .filter(|element| element["type"] == "table")
            .map(|element| {
                element["tableId"]
                    .as_str()
                    .ok_or(MirrorRejection::InvalidSnapshot)
            })
            .collect::<Result<_, _>>()?;
        if root_keys != next.extents.keys().map(String::as_str).collect() {
            return Err(MirrorRejection::ExtentsIncomplete);
        }
        next.revision = Some(revision);
        *self = next;
        Ok(())
    }

    fn table_start(&self, key: &str) -> Result<(u32, u32), MirrorRejection> {
        let mut key = key;
        let mut doc = 0u32;
        let mut scalar = 0u32;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(key) {
                return Err(MirrorRejection::HostMissing(key.into()));
            }
            let table = self
                .tables
                .get(key)
                .ok_or_else(|| MirrorRejection::UnknownTable(key.into()))?;
            let Some(host) = &table.host else {
                let extent = self
                    .extents
                    .get(key)
                    .ok_or(MirrorRejection::ExtentsIncomplete)?;
                return Ok((doc + extent.doc_start, scalar + extent.scalar_start));
            };
            let parent = &self.tables[&host.table_key];
            let index = host.cell_index as usize;
            let cell = &parent.cells[index];
            let nested = cell
                .nested_tables
                .iter()
                .find(|nested| nested.table_key == key)
                .ok_or_else(|| MirrorRejection::HostMissing(key.into()))?;
            doc += 2
                + 2 * cell.source_row
                + parent.cells[..index]
                    .iter()
                    .map(|cell| cell.doc_size)
                    .sum::<u32>()
                + nested.doc_offset;
            scalar += parent.cells[..index]
                .iter()
                .map(|cell| cell.scalar_stride)
                .sum::<u32>()
                + nested.scalar_start.unwrap_or(0);
            key = &host.table_key;
        }
    }

    pub(crate) fn absolute_input_blocks(
        &self,
        key: &str,
    ) -> Result<Vec<Vec<FfiCellInputBlock>>, MirrorRejection> {
        let table = self
            .tables
            .get(key)
            .ok_or_else(|| MirrorRejection::UnknownTable(key.into()))?;
        let (table_doc, mut scalar) = self.table_start(key)?;
        let mut preceding = 0;
        let mut output = Vec::new();
        for cell in &table.cells {
            let doc = table_doc + 2 + 2 * cell.source_row + preceding;
            output.push(
                cell.input_blocks
                    .iter()
                    .map(|block| FfiCellInputBlock {
                        element_index: block.element_index,
                        doc_start: doc + block.doc_start,
                        doc_end: doc + block.doc_end,
                        scalar_start: scalar + block.scalar_start,
                        content_scalar_start: scalar + block.content_scalar_start,
                        scalar_end: scalar + block.scalar_end,
                        break_scalar_end: scalar + block.break_scalar_end,
                        void: block.void,
                    })
                    .collect(),
            );
            preceding += cell.doc_size;
            scalar += cell.scalar_stride;
        }
        Ok(output)
    }
}
