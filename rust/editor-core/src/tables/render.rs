use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::model::Node;
use crate::render::incremental::{generate_block, CachedRenderError};
use crate::render::RenderElement;
use crate::schema::Schema;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::types::TableError;
use crate::tables::TableRole;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum TableRenderFailure {
    GridLimit,
    WorkLimit,
    Allocation,
    InvalidStructure,
    InvalidAttributes,
}

impl From<&TableError> for TableRenderFailure {
    fn from(error: &TableError) -> Self {
        match error {
            TableError::GridLimit { .. } => Self::GridLimit,
            TableError::WorkLimit => Self::WorkLimit,
            TableError::Allocation => Self::Allocation,
            TableError::InvalidStructure => Self::InvalidStructure,
            TableError::InvalidAttributes => Self::InvalidAttributes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, uniffi::Enum)]
#[serde(rename_all = "kebab-case")]
pub enum TableCompatibilityDiagnostic {
    VirtualGridLimit,
    EmptyReferenceSurface,
    UnsupportedRowRole,
    UnsupportedCellRole,
    AmbiguousSourceMap,
    UnsupportedGapDefault,
    OverlappingReferenceCells,
    UnmappedReferenceCell,
    NonrectangularReferenceCell,
    ZeroSpanAfterReferencePass,
}

impl TableCompatibilityDiagnostic {
    fn parse(value: &str) -> Result<Self, CachedRenderError> {
        Ok(match value {
            "virtual-grid-limit" => Self::VirtualGridLimit,
            "empty-reference-surface" => Self::EmptyReferenceSurface,
            "unsupported-row-role" => Self::UnsupportedRowRole,
            "unsupported-cell-role" => Self::UnsupportedCellRole,
            "ambiguous-source-map" => Self::AmbiguousSourceMap,
            "unsupported-gap-default" => Self::UnsupportedGapDefault,
            "overlapping-reference-cells" => Self::OverlappingReferenceCells,
            "unmapped-reference-cell" => Self::UnmappedReferenceCell,
            "nonrectangular-reference-cell" => Self::NonrectangularReferenceCell,
            "zero-span-after-reference-pass" => Self::ZeroSpanAfterReferencePass,
            _ => return Err(CachedRenderError::CacheInvariantViolation),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRenderCell {
    pub source_pos: u32,
    pub source_end: u32,
    pub row: u32,
    pub column: u32,
    pub rowspan: u32,
    pub colspan: u32,
    pub header: bool,
    pub attrs_key: String,
    pub content_key: String,
    pub elements: Arc<Vec<RenderElement>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct TableRenderRow {
    pub source_pos: u32,
    pub source_end: u32,
    pub attrs_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct TableRenderSyntheticRegion {
    pub row: u32,
    pub column: u32,
    pub rowspan: u32,
    pub colspan: u32,
    pub header: bool,
    pub attrs_key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRenderRecord {
    pub table_pos: u32,
    pub source_end: u32,
    pub rows: u32,
    pub columns: u32,
    pub column_widths: Vec<Option<u32>>,
    pub direction: Option<String>,
    pub irregular: bool,
    pub read_only_descendants: bool,
    pub attrs_key: String,
    pub source_rows: Vec<TableRenderRow>,
    pub cells: Vec<TableRenderCell>,
    pub synthetic_regions: Vec<TableRenderSyntheticRegion>,
    pub failure: Option<TableRenderFailure>,
    pub compatibility_diagnostic: Option<TableCompatibilityDiagnostic>,
}

impl Drop for TableRenderRecord {
    fn drop(&mut self) {
        stacker::maybe_grow(64 * 1024, 1024 * 1024, || {
            for cell in &mut self.cells {
                if let Some(elements) = Arc::get_mut(&mut cell.elements) {
                    for element in elements.iter_mut() {
                        element.drain_json_payloads();
                    }
                    elements.clear();
                }
            }
        });
    }
}

impl TableRenderRecord {
    pub(crate) fn retained_bytes(&self, element_bytes: impl Fn(&RenderElement) -> usize) -> usize {
        stacker::maybe_grow(64 * 1024, 1024 * 1024, || {
            let mut bytes = std::mem::size_of::<Self>()
                .saturating_add(self.attrs_key.capacity())
                .saturating_add(self.direction.as_ref().map_or(0, String::capacity))
                .saturating_add(
                    self.column_widths
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Option<u32>>()),
                );
            for row in &self.source_rows {
                bytes = bytes
                    .saturating_add(std::mem::size_of::<TableRenderRow>())
                    .saturating_add(row.attrs_key.capacity());
            }
            for region in &self.synthetic_regions {
                bytes = bytes
                    .saturating_add(std::mem::size_of::<TableRenderSyntheticRegion>())
                    .saturating_add(region.attrs_key.capacity());
            }
            for cell in &self.cells {
                bytes = bytes
                    .saturating_add(std::mem::size_of::<TableRenderCell>())
                    .saturating_add(cell.attrs_key.capacity())
                    .saturating_add(cell.content_key.capacity())
                    .saturating_add(
                        cell.elements
                            .capacity()
                            .saturating_mul(std::mem::size_of::<RenderElement>()),
                    );
                for element in cell.elements.iter() {
                    bytes = bytes.saturating_add(element_bytes(element));
                }
            }
            bytes
        })
    }
}

pub(crate) fn source_elements(elements: &[RenderElement]) -> Vec<&RenderElement> {
    let mut pending: Vec<_> = elements.iter().rev().collect();
    let mut output = Vec::new();
    while let Some(element) = pending.pop() {
        if let RenderElement::Table { table } = element {
            let mut cells: Vec<_> = table.cells.iter().collect();
            cells.sort_by_key(|cell| cell.source_pos);
            for cell in cells.into_iter().rev() {
                pending.extend(cell.elements.iter().rev());
            }
        } else {
            output.push(element);
        }
    }
    output
}

pub(crate) fn element_count(elements: &[RenderElement]) -> usize {
    let mut count = 0usize;
    let mut pending = vec![elements];
    while let Some(elements) = pending.pop() {
        count = count.saturating_add(elements.len());
        for element in elements {
            if let RenderElement::Table { table } = element {
                count = count
                    .saturating_add(table.cells.len())
                    .saturating_add(table.source_rows.len())
                    .saturating_add(table.synthetic_regions.len());
                for cell in &table.cells {
                    pending.push(&cell.elements);
                }
            }
        }
    }
    count
}

pub(crate) struct TableRenderContext {
    pub index: Arc<TableProjectionIndex>,
    pub prior_cells: HashMap<String, (u32, Arc<Vec<RenderElement>>)>,
    pub schema_key: String,
    pub attributes: BTreeMap<String, Arc<str>>,
    attribute_nodes: HashMap<(usize, bool), (Node, String)>,
}

impl TableRenderContext {
    pub(crate) fn retain_referenced_attributes(
        &mut self,
        roots: impl Iterator<Item = impl AsRef<[RenderElement]>>,
    ) {
        let mut keys = std::collections::HashSet::new();
        for root in roots {
            let mut pending = vec![root.as_ref()];
            while let Some(elements) = pending.pop() {
                for element in elements {
                    if let RenderElement::Table { table } = element {
                        keys.insert(table.attrs_key.clone());
                        keys.extend(table.source_rows.iter().map(|row| row.attrs_key.clone()));
                        keys.extend(
                            table
                                .synthetic_regions
                                .iter()
                                .map(|region| region.attrs_key.clone()),
                        );
                        for cell in &table.cells {
                            keys.insert(cell.attrs_key.clone());
                            pending.push(cell.elements.as_slice());
                        }
                    }
                }
            }
        }
        self.attributes.retain(|key, _| keys.contains(key));
    }

    pub(crate) fn new(index: Arc<TableProjectionIndex>, schema: &Schema) -> Self {
        Self {
            index,
            prior_cells: HashMap::new(),
            schema_key: crate::schema::schema_fingerprint(schema),
            attributes: BTreeMap::new(),
            attribute_nodes: HashMap::new(),
        }
    }

    fn intern_attributes(&mut self, node: &Node, cell: bool) -> String {
        let identity = (node.attrs() as *const _ as usize, cell);
        if let Some((_, key)) = self.attribute_nodes.get(&identity) {
            return key.clone();
        }
        let json = attrs_json(node, cell);
        #[cfg(test)]
        ATTRIBUTE_SERIALIZED_BYTES.set(ATTRIBUTE_SERIALIZED_BYTES.get() + json.len());
        let mut digest: [u8; 32] = Sha256::digest(json.as_bytes()).into();
        let mut key = attribute_key(&digest);
        let mut collision = 0usize;
        while let Some(existing) = self.attributes.get(&key) {
            if existing.as_ref() == json {
                break;
            }
            collision += 1;
            debug_assert!(collision <= self.attributes.len());
            increment_attribute_key(&mut digest);
            key = attribute_key(&digest);
        }
        self.attributes
            .entry(key.clone())
            .or_insert_with(|| Arc::from(json));
        self.attribute_nodes
            .insert(identity, (node.clone(), key.clone()));
        key
    }

    pub(crate) fn retain_cells(&mut self, elements: &[RenderElement]) {
        let mut pending = vec![elements];
        while let Some(elements) = pending.pop() {
            for element in elements {
                if let RenderElement::Table { table } = element {
                    for cell in &table.cells {
                        self.prior_cells.insert(
                            cell.content_key.clone(),
                            (cell.source_pos, Arc::clone(&cell.elements)),
                        );
                        pending.push(&cell.elements);
                    }
                }
            }
        }
    }

    fn reusable(&mut self, elements: &[RenderElement], delta: i64) -> bool {
        let index = Arc::clone(&self.index);
        let mut pending = vec![elements];
        while let Some(elements) = pending.pop() {
            for element in elements {
                let RenderElement::Table { table } = element else {
                    continue;
                };
                let Ok(pos) = u32::try_from(i64::from(table.table_pos) + delta) else {
                    return false;
                };
                let Some(projected) = index.table_at(pos) else {
                    return false;
                };
                if table.rows != projected.rows
                    || table.columns != projected.columns
                    || table.column_widths != projected.widths
                    || table.irregular != projected.irregular
                    || table.failure.is_some()
                    || projected
                        .compatibility_diagnostic
                        .map(TableCompatibilityDiagnostic::parse)
                        .transpose()
                        .ok()
                        != Some(table.compatibility_diagnostic)
                    || table.cells.len() != projected.cells.len()
                {
                    return false;
                }
                for (cell, current) in table.cells.iter().zip(&projected.cells) {
                    if i64::from(cell.source_pos) + delta != i64::from(current.source_pos)
                        || i64::from(cell.source_end) + delta != i64::from(current.source_end)
                        || cell.row != current.rect.row
                        || cell.column != current.rect.column
                        || cell.rowspan != current.rect.rowspan
                        || cell.colspan != current.rect.colspan
                    {
                        return false;
                    }
                    pending.push(&cell.elements);
                }
                if table.synthetic_regions.len() != projected.synthetic.len() {
                    return false;
                }
                for (region, current) in table.synthetic_regions.iter().zip(&projected.synthetic) {
                    if region.row != current.rect.row
                        || region.column != current.rect.column
                        || region.rowspan != current.rect.rowspan
                        || region.colspan != current.rect.colspan
                        || region.attrs_key != self.intern_attributes(&current.node, true)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }
}

fn attribute_key(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn increment_attribute_key(digest: &mut [u8; 32]) {
    for byte in digest.iter_mut().rev() {
        let (next, overflow) = byte.overflowing_add(1);
        *byte = next;
        if !overflow {
            break;
        }
    }
}

fn attrs_json(node: &Node, cell: bool) -> String {
    let value = serde_json::Value::Object(
        node.attrs()
            .iter()
            .filter(|(key, _)| !cell || !matches!(key.as_str(), "colspan" | "rowspan" | "colwidth"))
            .map(|(key, value)| {
                (
                    key.clone(),
                    crate::boundary::clone_json_value_stack_safe(value),
                )
            })
            .collect(),
    );
    let bytes = crate::boundary::serialize_json_value_stack_safe(&value, 0);
    crate::boundary::drop_json_value_stack_safe(value);
    String::from_utf8(bytes).expect("JSON is UTF-8")
}

fn content_key(cell: &Node, schema: &Schema, schema_key: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(schema_key.as_bytes());
    for index in 0..cell.child_count() {
        let value = crate::serialize::json_out::node_to_json(cell.child(index).unwrap(), schema);
        let bytes = crate::boundary::serialize_json_value_stack_safe(&value, 0);
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
        crate::boundary::drop_json_value_stack_safe(value);
    }
    format!("{:x}", hash.finalize())
}

#[cfg(test)]
std::thread_local! {
    pub(crate) static CELL_CONTENT_GENERATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static ATTRIBUTE_SERIALIZED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn generate_table(
    node: &Node,
    schema: &Schema,
    table_pos: u32,
    context: &mut TableRenderContext,
    nested: bool,
) -> Result<TableRenderRecord, CachedRenderError> {
    let source_end = table_pos
        .checked_add(node.node_size())
        .ok_or(CachedRenderError::PositionOverflow)?;
    let projection = Arc::clone(&context.index);
    let mut record = TableRenderRecord {
        table_pos,
        source_end,
        rows: 0,
        columns: 0,
        column_widths: Vec::new(),
        direction: schema
            .node(node.node_type())
            .filter(|spec| spec.attrs.contains_key("dir"))
            .and_then(|_| node.attrs().get("dir"))
            .and_then(serde_json::Value::as_str)
            .filter(|value| matches!(*value, "ltr" | "rtl"))
            .map(str::to_owned),
        irregular: true,
        read_only_descendants: nested,
        attrs_key: context.intern_attributes(node, false),
        source_rows: Vec::new(),
        cells: Vec::new(),
        synthetic_regions: Vec::new(),
        failure: projection.exact_failure().map(TableRenderFailure::from),
        compatibility_diagnostic: None,
    };
    let Some(projected) = projection.table_at(table_pos) else {
        record
            .failure
            .get_or_insert(TableRenderFailure::InvalidStructure);
        return Ok(record);
    };
    record.rows = projected.rows;
    record.columns = projected.columns;
    record.column_widths = projected.widths.clone();
    record.irregular = projected.irregular;
    record.compatibility_diagnostic = projected
        .compatibility_diagnostic
        .map(TableCompatibilityDiagnostic::parse)
        .transpose()?;
    let mut real_cells = HashMap::new();
    let mut row_pos = table_pos + 1;
    for row_index in 0..node.child_count() {
        let row = node.child(row_index).unwrap();
        record.source_rows.push(TableRenderRow {
            source_pos: row_pos,
            source_end: row_pos + row.node_size(),
            attrs_key: context.intern_attributes(row, false),
        });
        let mut cell_pos = row_pos + 1;
        for cell_index in 0..row.child_count() {
            let cell = row.child(cell_index).unwrap();
            real_cells.insert(cell_pos, cell);
            cell_pos += cell.node_size();
        }
        row_pos += row.node_size();
    }
    for projected_cell in &projected.cells {
        let cell = real_cells
            .get(&projected_cell.source_pos)
            .ok_or(CachedRenderError::CacheInvariantViolation)?;
        let key = content_key(cell, schema, &context.schema_key);
        let prior = context
            .prior_cells
            .get(&key)
            .cloned()
            .filter(|(prior_pos, elements)| {
                context.reusable(
                    elements,
                    i64::from(projected_cell.source_pos) - i64::from(*prior_pos),
                )
            });
        let elements = if let Some((prior_pos, prior)) = prior.as_ref() {
            if *prior_pos == projected_cell.source_pos {
                Arc::clone(prior)
            } else {
                let mut elements = prior.as_ref().clone();
                rebase_elements(
                    &mut elements,
                    i64::from(projected_cell.source_pos) - i64::from(*prior_pos),
                )?;
                Arc::new(elements)
            }
        } else {
            #[cfg(test)]
            CELL_CONTENT_GENERATIONS.set(CELL_CONTENT_GENERATIONS.get() + 1);
            let mut elements = Vec::new();
            let mut pos = projected_cell.source_pos + 1;
            for index in 0..cell.child_count() {
                generate_block(
                    cell.child(index).unwrap(),
                    schema,
                    &mut elements,
                    &mut pos,
                    0,
                    None,
                    index,
                    context,
                    true,
                )?;
            }
            Arc::new(elements)
        };
        record.cells.push(TableRenderCell {
            source_pos: projected_cell.source_pos,
            source_end: projected_cell.source_end,
            row: projected_cell.rect.row,
            column: projected_cell.rect.column,
            rowspan: projected_cell.rect.rowspan,
            colspan: projected_cell.rect.colspan,
            header: schema
                .node(cell.node_type())
                .is_some_and(|spec| spec.table_role == Some(TableRole::HeaderCell)),
            attrs_key: context.intern_attributes(cell, true),
            content_key: key,
            elements,
        });
    }
    for region in &projected.synthetic {
        record.synthetic_regions.push(TableRenderSyntheticRegion {
            row: region.rect.row,
            column: region.rect.column,
            rowspan: region.rect.rowspan,
            colspan: region.rect.colspan,
            header: schema
                .node(region.node.node_type())
                .is_some_and(|spec| spec.table_role == Some(TableRole::HeaderCell)),
            attrs_key: context.intern_attributes(&region.node, true),
        });
    }
    Ok(record)
}

pub(crate) fn rebase_elements(
    elements: &mut [RenderElement],
    delta: i64,
) -> Result<(), CachedRenderError> {
    let shift = |pos: &mut u32| -> Result<(), CachedRenderError> {
        *pos = u32::try_from(i64::from(*pos) + delta)
            .map_err(|_| CachedRenderError::PositionOverflow)?;
        Ok(())
    };
    stacker::maybe_grow(64 * 1024, 1024 * 1024, || {
        for element in elements {
            match element {
                RenderElement::Table { table } => {
                    shift(&mut table.table_pos)?;
                    shift(&mut table.source_end)?;
                    for row in &mut table.source_rows {
                        shift(&mut row.source_pos)?;
                        shift(&mut row.source_end)?;
                    }
                    for cell in &mut table.cells {
                        shift(&mut cell.source_pos)?;
                        shift(&mut cell.source_end)?;
                        rebase_elements(Arc::make_mut(&mut cell.elements).as_mut_slice(), delta)?;
                    }
                }
                RenderElement::VoidInline { doc_pos, .. }
                | RenderElement::VoidBlock { doc_pos, .. }
                | RenderElement::OpaqueInlineAtom { doc_pos, .. }
                | RenderElement::OpaqueBlockAtom { doc_pos, .. } => shift(doc_pos)?,
                _ => {}
            }
        }
        Ok(())
    })
}
