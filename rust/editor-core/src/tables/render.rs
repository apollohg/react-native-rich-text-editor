use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::model::Node;
use crate::render::incremental::{generate_block, CachedRenderError};
use crate::render::RenderElement;
use crate::schema::Schema;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::commands::{NODE_CLOSING_TOKENS, NODE_OPENING_TOKENS};
use crate::tables::types::TableError;
use crate::tables::TableRole;

const RENDER_STACK_RED_ZONE: usize = 64 * 1024;
const RENDER_STACK_SEGMENT: usize = 1024 * 1024;

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
    pub source_row: u32,
    pub doc_size: u32,
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
pub struct TableSourceRow {
    pub attrs_key: String,
    pub cell_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRenderStructure {
    pub rows: u32,
    pub columns: u32,
    pub column_widths: Vec<Option<u32>>,
    pub direction: Option<String>,
    pub irregular: bool,
    pub read_only_descendants: bool,
    pub attrs_key: String,
    pub doc_size: u32,
    pub source_rows: Vec<TableSourceRow>,
    pub synthetic_regions: Vec<TableRenderSyntheticRegion>,
    pub failure: Option<TableRenderFailure>,
    pub compatibility_diagnostic: Option<TableCompatibilityDiagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRenderRecord {
    pub structure: Arc<TableRenderStructure>,
    pub cells: Vec<Arc<TableRenderCell>>,
}

impl Drop for TableRenderCell {
    fn drop(&mut self) {
        stacker::maybe_grow(RENDER_STACK_RED_ZONE, RENDER_STACK_SEGMENT, || {
            if let Some(elements) = Arc::get_mut(&mut self.elements) {
                for element in elements.iter_mut() {
                    element.drain_json_payloads();
                }
                elements.clear();
            }
        });
    }
}

impl TableRenderRecord {
    pub(crate) fn retained_bytes(&self, element_bytes: impl Fn(&RenderElement) -> usize) -> usize {
        stacker::maybe_grow(RENDER_STACK_RED_ZONE, RENDER_STACK_SEGMENT, || {
            let mut bytes = std::mem::size_of::<Self>()
                .saturating_add(
                    crate::model::arc_allocation_retained_bytes(std::mem::size_of::<
                        TableRenderStructure,
                    >())
                    .unwrap_or(usize::MAX),
                )
                .saturating_add(
                    self.cells
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Arc<TableRenderCell>>()),
                )
                .saturating_add(self.structure.attrs_key.capacity())
                .saturating_add(
                    self.structure
                        .direction
                        .as_ref()
                        .map_or(0, String::capacity),
                )
                .saturating_add(
                    self.structure
                        .column_widths
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Option<u32>>()),
                );
            for row in &self.structure.source_rows {
                bytes = bytes
                    .saturating_add(std::mem::size_of::<TableSourceRow>())
                    .saturating_add(row.attrs_key.capacity());
            }
            for region in &self.structure.synthetic_regions {
                bytes = bytes
                    .saturating_add(std::mem::size_of::<TableRenderSyntheticRegion>())
                    .saturating_add(region.attrs_key.capacity());
            }
            for cell in &self.cells {
                bytes = bytes
                    .saturating_add(
                        crate::model::arc_allocation_retained_bytes(std::mem::size_of::<
                            TableRenderCell,
                        >())
                        .unwrap_or(usize::MAX),
                    )
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
        if let RenderElement::Table { table, .. } = element {
            for cell in table.cells.iter().rev() {
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
            if let RenderElement::Table { table, .. } = element {
                count = count
                    .saturating_add(table.cells.len())
                    .saturating_add(table.structure.source_rows.len())
                    .saturating_add(table.structure.synthetic_regions.len());
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
    pub prior_cells: HashMap<usize, (Node, Arc<TableRenderCell>)>,
    prior_content: HashMap<String, Arc<Vec<RenderElement>>>,
    pub coordinate_origin: u32,
    pub schema_key: String,
    pub attributes: BTreeMap<String, Arc<str>>,
    attribute_nodes: HashMap<(usize, bool), (Node, String)>,
    attribute_keys: HashMap<String, String>,
    attribute_values: HashMap<u64, Vec<(Node, bool, String)>>,
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
                    if let RenderElement::Table { table, .. } = element {
                        keys.insert(table.structure.attrs_key.clone());
                        keys.extend(
                            table
                                .structure
                                .source_rows
                                .iter()
                                .map(|row| row.attrs_key.clone()),
                        );
                        keys.extend(
                            table
                                .structure
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
            prior_content: HashMap::new(),
            coordinate_origin: 0,
            schema_key: crate::schema::schema_fingerprint(schema),
            attributes: BTreeMap::new(),
            attribute_nodes: HashMap::new(),
            attribute_keys: HashMap::new(),
            attribute_values: HashMap::new(),
        }
    }

    fn intern_attributes(&mut self, node: &Node, cell: bool) -> String {
        let identity = (node.attrs() as *const _ as usize, cell);
        if let Some((_, key)) = self.attribute_nodes.get(&identity) {
            return key.clone();
        }
        let attrs = crate::serialize::json_out::filtered_attrs(node, cell);
        let fingerprint = attrs_fingerprint(&attrs);
        if let Some(values) = self.attribute_values.get(&fingerprint) {
            for (prior, prior_cell, json) in values {
                let prior_attrs = crate::serialize::json_out::filtered_attrs(prior, *prior_cell);
                if attrs.len() == prior_attrs.len()
                    && attrs.iter().zip(prior_attrs).all(
                        |((key, value), (prior_key, prior_value))| {
                            key == &prior_key
                                && crate::boundary::json_values_equal_stack_safe(value, prior_value)
                        },
                    )
                {
                    let key = &self.attribute_keys[json];
                    self.attribute_nodes
                        .insert(identity, (node.clone(), key.clone()));
                    return key.clone();
                }
            }
        }
        let mut json = String::new();
        crate::serialize::json_out::write_attrs_json(&mut json, node, cell);
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
        self.attribute_keys.insert(json.clone(), key.clone());
        self.attribute_values.entry(fingerprint).or_default().push((
            node.clone(),
            cell,
            json.clone(),
        ));
        self.attributes
            .entry(key.clone())
            .or_insert_with(|| Arc::from(json));
        self.attribute_nodes
            .insert(identity, (node.clone(), key.clone()));
        key
    }

    fn retain_attributes(&mut self, node: &Node, cell: bool, key: &str) {
        let Some(json) = self.attributes.get(key) else {
            return;
        };
        let identity = (node.attrs() as *const _ as usize, cell);
        self.attribute_nodes
            .insert(identity, (node.clone(), key.to_owned()));
        if !self.attribute_keys.contains_key(json.as_ref()) {
            let fingerprint =
                attrs_fingerprint(&crate::serialize::json_out::filtered_attrs(node, cell));
            self.attribute_keys.insert(json.to_string(), key.to_owned());
            self.attribute_values.entry(fingerprint).or_default().push((
                node.clone(),
                cell,
                json.to_string(),
            ));
        }
    }

    pub(crate) fn retain_cells(
        &mut self,
        elements: &[RenderElement],
        root: &Node,
        start: u32,
        projection: &TableProjectionIndex,
    ) {
        if !elements
            .iter()
            .any(|element| matches!(element, RenderElement::Table { .. }))
        {
            return;
        }
        let mut nodes = HashMap::new();
        let mut pending = vec![(start, root)];
        while let Some((position, node)) = pending.pop() {
            nodes.insert(position, node);
            let mut child_pos = position + NODE_OPENING_TOKENS;
            if let Some(content) = node.content() {
                for child in content.iter() {
                    if child.is_element() {
                        pending.push((child_pos, child));
                    }
                    child_pos += child.node_size();
                }
            }
        }
        let mut pending = vec![(0, elements)];
        while let Some((origin, elements)) = pending.pop() {
            for element in elements {
                if let RenderElement::Table { table, doc_offset } = element {
                    let table_pos = origin + doc_offset;
                    if let Some(node) = nodes.get(&table_pos) {
                        self.retain_attributes(node, false, &table.structure.attrs_key);
                    }
                    for row in absolute_source_rows(table, table_pos) {
                        if let Some(node) = nodes.get(&row.source_pos) {
                            self.retain_attributes(node, false, &row.attrs_key);
                        }
                    }
                    if let Some(projected) = projection.table_at(table_pos) {
                        for (region, rendered) in projected
                            .synthetic
                            .iter()
                            .zip(&table.structure.synthetic_regions)
                        {
                            self.retain_attributes(&region.node, true, &rendered.attrs_key);
                        }
                    }
                    for (cell_pos, cell) in absolute_cell_starts(table, table_pos)
                        .into_iter()
                        .zip(&table.cells)
                    {
                        if let Some(node) = nodes.get(&cell_pos) {
                            self.prior_cells.insert(
                                node.attrs() as *const _ as usize,
                                ((*node).clone(), Arc::clone(cell)),
                            );
                            self.retain_attributes(node, true, &cell.attrs_key);
                        }
                        self.prior_content
                            .insert(cell.content_key.clone(), Arc::clone(&cell.elements));
                        pending.push((cell_pos, cell.elements.as_slice()));
                    }
                }
            }
        }
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

fn attrs_fingerprint(attrs: &[(&String, &serde_json::Value)]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    attrs.len().hash(&mut hash);
    for (key, value) in attrs {
        key.hash(&mut hash);
        let mut pending = vec![*value];
        while let Some(value) = pending.pop() {
            std::mem::discriminant(value).hash(&mut hash);
            match value {
                serde_json::Value::Null => {}
                serde_json::Value::Bool(value) => value.hash(&mut hash),
                serde_json::Value::Number(value) => value.hash(&mut hash),
                serde_json::Value::String(value) => value.hash(&mut hash),
                serde_json::Value::Array(values) => {
                    values.len().hash(&mut hash);
                    pending.extend(values.iter().rev());
                }
                serde_json::Value::Object(values) => {
                    values.len().hash(&mut hash);
                    for key in values.keys() {
                        key.hash(&mut hash);
                    }
                    pending.extend(values.values().rev());
                }
            }
        }
    }
    hash.finish()
}

struct ContentKeySink(Sha256);

impl std::io::Write for ContentKeySink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn content_key(cell: &Node, schema: &Schema, schema_key: &str) -> String {
    #[cfg(test)]
    crate::yrs_engine::observability::record_cell_content_key();
    let mut sink = ContentKeySink(Sha256::new());
    sink.0.update(schema_key.as_bytes());
    for index in 0..cell.child_count() {
        crate::serialize::json_out::write_node_json(&mut sink, cell.child(index).unwrap(), schema)
            .expect("content hash writes are infallible");
    }
    format!("{:x}", sink.0.finalize())
}

pub(crate) fn generate_table(
    node: &Node,
    schema: &Schema,
    table_pos: u32,
    context: &mut TableRenderContext,
    nested: bool,
) -> Result<TableRenderRecord, CachedRenderError> {
    let projection = Arc::clone(&context.index);
    let mut structure = TableRenderStructure {
        doc_size: node.node_size(),
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
        synthetic_regions: Vec::new(),
        failure: projection.exact_failure().map(TableRenderFailure::from),
        compatibility_diagnostic: None,
    };
    let Some(projected) = projection.table_at(table_pos) else {
        structure
            .failure
            .get_or_insert(TableRenderFailure::InvalidStructure);
        return Ok(TableRenderRecord {
            structure: Arc::new(structure),
            cells: Vec::new(),
        });
    };
    structure.rows = projected.rows;
    structure.columns = projected.columns;
    structure.column_widths = projected.widths.clone();
    structure.irregular = projected.irregular;
    structure.compatibility_diagnostic = projected
        .compatibility_diagnostic
        .map(TableCompatibilityDiagnostic::parse)
        .transpose()?;
    let mut real_cells = HashMap::new();
    let mut row_pos = table_pos + NODE_OPENING_TOKENS;
    for row_index in 0..node.child_count() {
        let row = node.child(row_index).unwrap();
        structure.source_rows.push(TableSourceRow {
            cell_count: u32::try_from(row.child_count())
                .map_err(|_| CachedRenderError::PositionOverflow)?,
            attrs_key: context.intern_attributes(row, false),
        });
        let mut cell_pos = row_pos + NODE_OPENING_TOKENS;
        for cell_index in 0..row.child_count() {
            let cell = row.child(cell_index).unwrap();
            real_cells.insert(cell_pos, (row_index, cell));
            cell_pos += cell.node_size();
        }
        row_pos += row.node_size();
    }
    let mut cells = Vec::new();
    for projected_cell in &projected.cells {
        let (source_row, cell) = real_cells
            .get(&projected_cell.source_pos)
            .ok_or(CachedRenderError::CacheInvariantViolation)?;
        if let Some((prior_node, prior)) = context
            .prior_cells
            .get(&(cell.attrs() as *const _ as usize))
        {
            if prior_node.shares_storage_with(cell)
                && prior.source_row as usize == *source_row
                && prior.row == projected_cell.rect.row
                && prior.column == projected_cell.rect.column
                && prior.rowspan == projected_cell.rect.rowspan
                && prior.colspan == projected_cell.rect.colspan
            {
                cells.push(Arc::clone(prior));
                continue;
            }
        }
        let key = content_key(cell, schema, &context.schema_key);
        let elements = if let Some(prior) = context.prior_content.get(&key) {
            Arc::clone(prior)
        } else {
            #[cfg(test)]
            crate::yrs_engine::observability::record_cell_content_generation();
            let mut elements = Vec::new();
            let mut pos = NODE_OPENING_TOKENS;
            let previous_origin =
                std::mem::replace(&mut context.coordinate_origin, projected_cell.source_pos);
            let generated = (|| {
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
                Ok::<_, CachedRenderError>(())
            })();
            context.coordinate_origin = previous_origin;
            generated?;
            Arc::new(elements)
        };
        cells.push(Arc::new(TableRenderCell {
            source_row: u32::try_from(*source_row)
                .map_err(|_| CachedRenderError::PositionOverflow)?,
            doc_size: cell.node_size(),
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
        }));
    }
    for region in &projected.synthetic {
        structure
            .synthetic_regions
            .push(TableRenderSyntheticRegion {
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
    Ok(TableRenderRecord {
        structure: Arc::new(structure),
        cells,
    })
}

pub(crate) fn absolute_cell_starts(table: &TableRenderRecord, table_pos: u32) -> Vec<u32> {
    let mut preceding_size = 0;
    table
        .cells
        .iter()
        .map(|cell| {
            let position = table_pos
                + NODE_OPENING_TOKENS
                + NODE_OPENING_TOKENS
                + (NODE_OPENING_TOKENS + NODE_CLOSING_TOKENS) * cell.source_row
                + preceding_size;
            preceding_size += cell.doc_size;
            position
        })
        .collect()
}

pub(crate) fn absolute_source_rows(
    table: &TableRenderRecord,
    table_pos: u32,
) -> Vec<TableRenderRow> {
    let mut position = table_pos + NODE_OPENING_TOKENS;
    let mut cells = table.cells.iter();
    table
        .structure
        .source_rows
        .iter()
        .map(|row| {
            let source_pos = position;
            position += NODE_OPENING_TOKENS + NODE_CLOSING_TOKENS;
            for cell in cells.by_ref().take(row.cell_count as usize) {
                position += cell.doc_size;
            }
            TableRenderRow {
                source_pos,
                source_end: position,
                attrs_key: row.attrs_key.clone(),
            }
        })
        .collect()
}
