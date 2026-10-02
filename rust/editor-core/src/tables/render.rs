mod row_key;
pub use row_key::TableRowAttributeKey;

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

const ATTRIBUTE_DIGEST_BYTES: usize = 32;
const ATTRIBUTE_KEY_BYTES: usize = ATTRIBUTE_DIGEST_BYTES * 2;
const ATTRIBUTE_HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

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
    pub attrs_key: TableRowAttributeKey,
    pub cell_count: u32,
}

const _: () = {
    assert!(std::mem::size_of::<TableSourceRow>() == std::mem::size_of::<(String, u32)>());
    assert!(std::mem::align_of::<TableSourceRow>() == std::mem::align_of::<(String, u32)>());
};

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

#[derive(Debug)]
pub struct TableRenderRecord {
    data: Arc<TableRenderData>,
    pub(crate) source_fallback: Option<Arc<Vec<RenderElement>>>,
    cell_capacity: usize,
}

#[derive(Debug, PartialEq)]
pub struct TableRenderData {
    pub structure: TableRenderStructure,
    pub cells: Box<[Arc<TableRenderCell>]>,
}

impl std::ops::Deref for TableRenderRecord {
    type Target = TableRenderData;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl Clone for TableRenderRecord {
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
            source_fallback: self.source_fallback.clone(),
            // Vec::clone charged only its length, even when the source had spare capacity.
            cell_capacity: self.cells.len(),
        }
    }
}

impl PartialEq for TableRenderRecord {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data && self.source_fallback == other.source_fallback
    }
}

impl Drop for TableRenderRecord {
    fn drop(&mut self) {
        if let Some(elements) = self.source_fallback.as_mut().and_then(Arc::get_mut) {
            for element in elements.iter_mut() {
                element.drain_json_payloads();
            }
        }
    }
}

impl Drop for TableRenderCell {
    fn drop(&mut self) {
        #[cfg(test)]
        CELL_PAYLOAD_CLEANUP_VISITS.set(CELL_PAYLOAD_CLEANUP_VISITS.get() + 1);
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

#[cfg(test)]
std::thread_local! {
    pub(crate) static CELL_START_VALUE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static CELL_PAYLOAD_CLEANUP_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static CELL_OUTPUT_METER_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl TableRenderCell {
    pub(crate) fn retained_bytes(&self, element_bytes: impl Fn(&RenderElement) -> usize) -> usize {
        stacker::maybe_grow(RENDER_STACK_RED_ZONE, RENDER_STACK_SEGMENT, || {
            #[cfg(test)]
            CELL_OUTPUT_METER_VISITS.set(CELL_OUTPUT_METER_VISITS.get() + 1);
            let bytes = crate::model::arc_allocation_retained_bytes(std::mem::size_of::<Self>())
                .unwrap_or(usize::MAX)
                .saturating_add(self.attrs_key.capacity())
                .saturating_add(self.content_key.capacity())
                .saturating_add(
                    self.elements
                        .capacity()
                        .saturating_mul(std::mem::size_of::<RenderElement>()),
                );
            self.elements.iter().fold(bytes, |bytes, element| {
                bytes.saturating_add(element_bytes(element))
            })
        })
    }
}

#[inline(always)]
fn extend_cell_references(output: &mut Vec<Arc<TableRenderCell>>, input: &[Arc<TableRenderCell>]) {
    output.extend(input.iter().cloned());
}

#[cfg(all(target_os = "android", target_arch = "aarch64"))]
#[target_feature(enable = "lse")]
unsafe fn extend_cell_references_lse(
    output: &mut Vec<Arc<TableRenderCell>>,
    input: &[Arc<TableRenderCell>],
) {
    extend_cell_references(output, input);
}

impl TableRenderRecord {
    pub(crate) fn new(
        structure: TableRenderStructure,
        cells: Vec<Arc<TableRenderCell>>,
        source_fallback: Option<Arc<Vec<RenderElement>>>,
    ) -> Self {
        let cell_capacity = cells.capacity();
        Self {
            data: Arc::new(TableRenderData {
                structure,
                cells: cells.into_boxed_slice(),
            }),
            source_fallback,
            cell_capacity,
        }
    }

    pub(crate) fn cell_capacity(&self) -> usize {
        self.cell_capacity
    }

    pub(crate) fn try_clone_cells(&self) -> Result<Vec<Arc<TableRenderCell>>, CachedRenderError> {
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(self.cell_capacity())
            .map_err(|_| CachedRenderError::AllocationFailed)?;
        #[cfg(all(target_os = "android", target_arch = "aarch64"))]
        if std::arch::is_aarch64_feature_detected!("lse") {
            // Runtime detection keeps the baseline Android CPU requirement unchanged.
            unsafe { extend_cell_references_lse(&mut cells, &self.cells) };
            return Ok(cells);
        }
        extend_cell_references(&mut cells, &self.cells);
        Ok(cells)
    }

    pub(crate) fn shares_data_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    #[cfg(test)]
    pub(crate) fn edit_parts_for_testing(
        &mut self,
        edit: impl FnOnce(&mut TableRenderStructure, &mut Vec<Arc<TableRenderCell>>),
    ) {
        let mut structure = self.structure.clone();
        let mut cells = Vec::with_capacity(self.cell_capacity);
        extend_cell_references(&mut cells, &self.cells);
        edit(&mut structure, &mut cells);
        *self = Self::new(structure, cells, self.source_fallback.clone());
    }

    pub(crate) fn retained_bytes(&self, element_bytes: impl Fn(&RenderElement) -> usize) -> usize {
        self.retained_bytes_with_cell_bytes(element_bytes, None)
    }

    pub(crate) fn retained_bytes_with_cell_bytes(
        &self,
        element_bytes: impl Fn(&RenderElement) -> usize,
        cell_bytes: Option<usize>,
    ) -> usize {
        stacker::maybe_grow(RENDER_STACK_RED_ZONE, RENDER_STACK_SEGMENT, || {
            let mut bytes = std::mem::size_of::<Self>()
                .saturating_add(
                    crate::model::arc_allocation_retained_bytes(
                        std::mem::size_of::<TableRenderData>(),
                    )
                    .unwrap_or(usize::MAX),
                )
                .saturating_add(
                    self.cell_capacity
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
                    .saturating_add(row.attrs_key.retained_string_capacity());
            }
            for region in &self.structure.synthetic_regions {
                bytes = bytes
                    .saturating_add(std::mem::size_of::<TableRenderSyntheticRegion>())
                    .saturating_add(region.attrs_key.capacity());
            }
            bytes = bytes.saturating_add(cell_bytes.unwrap_or_else(|| {
                self.cells.iter().fold(0usize, |total, cell| {
                    total.saturating_add(cell.retained_bytes(&element_bytes))
                })
            }));
            if let Some(elements) = &self.source_fallback {
                bytes = bytes
                    .saturating_add(
                        crate::model::arc_allocation_retained_bytes(std::mem::size_of::<
                            Vec<RenderElement>,
                        >())
                        .unwrap_or(usize::MAX),
                    )
                    .saturating_add(
                        elements
                            .capacity()
                            .saturating_mul(std::mem::size_of::<RenderElement>()),
                    );
                for element in elements.iter() {
                    bytes = bytes.saturating_add(element_bytes(element));
                }
            }
            bytes
        })
    }
}

pub(crate) fn all_elements(elements: &[RenderElement]) -> impl Iterator<Item = &RenderElement> {
    let mut pending = vec![elements.iter()];
    std::iter::from_fn(move || loop {
        let elements = pending.last_mut()?;
        if let Some(element) = elements.next() {
            if let RenderElement::Table { table, .. } = element {
                if let Some(elements) = &table.source_fallback {
                    pending.push(elements.iter());
                } else {
                    for cell in table.cells.iter().rev() {
                        pending.push(cell.elements.iter());
                    }
                }
            }
            return Some(element);
        }
        pending.pop();
    })
}

pub(crate) fn source_elements(elements: &[RenderElement]) -> impl Iterator<Item = &RenderElement> {
    all_elements(elements).filter(|element| !matches!(element, RenderElement::Table { .. }))
}

pub(crate) fn element_count(elements: &[RenderElement]) -> usize {
    all_elements(elements).fold(0usize, |count, element| {
        count.saturating_add(element_shallow_count(element))
    })
}

pub(crate) fn element_shallow_count(element: &RenderElement) -> usize {
    if let RenderElement::Table { table, .. } = element {
        table
            .cells
            .len()
            .saturating_add(table.structure.source_rows.len())
            .saturating_add(table.structure.synthetic_regions.len())
            .saturating_add(1)
    } else {
        1
    }
}

pub(crate) struct TableRenderContext {
    pub index: Arc<TableProjectionIndex>,
    pub prior_cells: HashMap<usize, (Node, Arc<TableRenderCell>)>,
    prior_content: HashMap<String, Arc<Vec<RenderElement>>>,
    pub coordinate_origin: u32,
    pub source_only: bool,
    pub schema_key: String,
    pub attributes: BTreeMap<String, Arc<str>>,
    attribute_nodes: HashMap<(usize, bool), (Node, String)>,
    attribute_keys: HashMap<String, String>,
    attribute_values: HashMap<u64, Vec<(Node, bool, String)>>,
}

impl TableRenderContext {
    #[cfg(test)]
    pub(crate) fn attribute_identity_count_for_test(&self) -> usize {
        self.attribute_nodes.len()
    }

    #[cfg(test)]
    pub(crate) fn has_attribute_identity_for_test(&self, node: &Node, cell: bool) -> bool {
        self.attribute_nodes
            .contains_key(&(node.attrs() as *const _ as usize, cell))
    }

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
                                .map(|row| row.attrs_key.to_owned_string()),
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

    pub(crate) fn new(index: Arc<TableProjectionIndex>, schema_key: &str) -> Self {
        Self {
            index,
            prior_cells: HashMap::new(),
            prior_content: HashMap::new(),
            coordinate_origin: 0,
            source_only: false,
            schema_key: schema_key.to_owned(),
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
                    if !attrs.is_empty() {
                        self.attribute_nodes
                            .insert(identity, (node.clone(), key.clone()));
                    }
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

fn attribute_key(digest: &[u8; ATTRIBUTE_DIGEST_BYTES]) -> String {
    let mut key = String::with_capacity(ATTRIBUTE_KEY_BYTES);
    for byte in digest {
        key.push(ATTRIBUTE_HEX_DIGITS[usize::from(byte >> 4)] as char);
        key.push(ATTRIBUTE_HEX_DIGITS[usize::from(byte & 0x0f)] as char);
    }
    key
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

pub(crate) fn render_cell_content(
    cell: &Node,
    schema: &Schema,
    cell_pos: u32,
    context: &mut TableRenderContext,
) -> Result<(String, Arc<Vec<RenderElement>>), CachedRenderError> {
    let key = content_key(cell, schema, &context.schema_key);
    let elements = if let Some(prior) = context.prior_content.get(&key) {
        Arc::clone(prior)
    } else {
        #[cfg(test)]
        crate::yrs_engine::observability::record_cell_content_generation();
        let mut elements = Vec::new();
        let mut pos = NODE_OPENING_TOKENS;
        let previous_origin = std::mem::replace(&mut context.coordinate_origin, cell_pos);
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
    Ok((key, elements))
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
    let representable = (0..node.child_count()).all(|index| {
        let row = node.child(index).expect("source row index is in bounds");
        schema
            .node(row.node_type())
            .is_some_and(|spec| spec.table_role == Some(TableRole::Row))
            && (0..row.child_count()).all(|index| {
                let cell = row.child(index).expect("source cell index is in bounds");
                schema.node(cell.node_type()).is_some_and(|spec| {
                    matches!(
                        spec.table_role,
                        Some(TableRole::Cell | TableRole::HeaderCell)
                    )
                })
            })
    });
    let Some(projected) = projection.table_at(table_pos).filter(|_| representable) else {
        structure
            .failure
            .get_or_insert(TableRenderFailure::InvalidStructure);
        let mut elements = Vec::new();
        let previous_source_only = context.source_only;
        context.source_only = true;
        let result = generate_block(
            node,
            schema,
            &mut elements,
            &mut 0,
            0,
            None,
            0,
            context,
            nested,
        );
        context.source_only = previous_source_only;
        result?;
        return Ok(TableRenderRecord::new(
            structure,
            Vec::new(),
            Some(Arc::new(elements)),
        ));
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
            attrs_key: context.intern_attributes(row, false).into(),
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
        let (key, elements) =
            render_cell_content(cell, schema, projected_cell.source_pos, context)?;
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
    Ok(TableRenderRecord::new(structure, cells, None))
}

struct AbsoluteCellStarts<'a> {
    cells: std::slice::Iter<'a, Arc<TableRenderCell>>,
    table_pos: u32,
    preceding_size: u32,
}

impl Iterator for AbsoluteCellStarts<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        let cell = self.cells.next()?;
        #[cfg(test)]
        CELL_START_VALUE_VISITS.set(CELL_START_VALUE_VISITS.get() + 1);
        let position = self.table_pos
            + NODE_OPENING_TOKENS
            + NODE_OPENING_TOKENS
            + (NODE_OPENING_TOKENS + NODE_CLOSING_TOKENS) * cell.source_row
            + self.preceding_size;
        self.preceding_size += cell.doc_size;
        Some(position)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.cells.size_hint()
    }
}

impl ExactSizeIterator for AbsoluteCellStarts<'_> {}

pub(crate) fn absolute_cell_starts(
    table: &TableRenderRecord,
    table_pos: u32,
) -> impl ExactSizeIterator<Item = u32> + '_ {
    AbsoluteCellStarts {
        cells: table.cells.iter(),
        table_pos,
        preceding_size: 0,
    }
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
                attrs_key: row.attrs_key.to_owned_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod output_meter_tests {
    use super::*;

    #[test]
    fn shared_table_headers_preserve_the_legacy_render_charge() {
        type PreviousRecord = (
            Arc<TableRenderStructure>,
            Vec<Arc<TableRenderCell>>,
            Option<Arc<Vec<RenderElement>>>,
        );
        #[allow(dead_code)]
        enum PreviousElementLayout {
            Table {
                table: PreviousRecord,
                doc_offset: u32,
            },
            OpaqueInlineAtom {
                node_type: String,
                label: String,
                doc_pos: u32,
                attrs: std::collections::HashMap<String, serde_json::Value>,
                mention_theme: Option<std::collections::HashMap<String, serde_json::Value>>,
            },
        }
        const _: () = {
            assert!(
                std::mem::size_of::<TableRenderRecord>() + std::mem::size_of::<TableRenderData>()
                    == std::mem::size_of::<PreviousRecord>()
                        + std::mem::size_of::<TableRenderStructure>()
            );
            assert!(std::mem::size_of::<RenderElement>() == std::mem::size_of::<PreviousElementLayout>());
        };
        let allocation = crate::model::arc_allocation_retained_bytes;
        assert_eq!(
            std::mem::size_of::<TableRenderRecord>()
                + allocation(std::mem::size_of::<TableRenderData>()).unwrap(),
            std::mem::size_of::<PreviousRecord>()
                + allocation(std::mem::size_of::<TableRenderStructure>()).unwrap(),
            "sharing must not change fixed render/history admission charges",
        );
        assert_eq!(
            std::mem::size_of::<RenderElement>(),
            std::mem::size_of::<PreviousElementLayout>(),
            "the table variant must not alter enclosing vector charges"
        );
    }

    #[test]
    fn table_snapshot_clones_share_cells_and_preserve_capacity_charges() {
        use crate::render::output_bytes::render_element_bytes;
        use crate::test_support::large_table_fixture::{
            plain_table_document, session_with_document,
        };
        const ROWS: usize = 3;
        const COLUMNS: usize = 3;
        let session = session_with_document(&plain_table_document(ROWS, COLUMNS));
        let cache = crate::render::incremental::CachedRenderBlocks::build(
            session.engine.document().unwrap(),
            session.engine.schema(),
            &crate::boundary::ResourceLimits::default(),
        )
        .unwrap();
        let mut records = Vec::new();
        cache.visit_table_records(&mut records);
        let table = records[0].1;
        let charged = table.retained_bytes(render_element_bytes);
        let capacity = table.cell_capacity();
        assert!(
            capacity > table.cells.len(),
            "fixture must exercise spare vector capacity"
        );
        let snapshot = table.clone();
        assert_eq!(
            table.cells.as_ptr(),
            snapshot.cells.as_ptr(),
            "materializing a snapshot must not copy every cell reference"
        );
        assert_eq!(
            snapshot.retained_bytes(render_element_bytes),
            charged - (capacity - table.cells.len()) * std::mem::size_of::<Arc<TableRenderCell>>(),
            "a cloned snapshot keeps the prior Vec clone's exact charge"
        );
        let next = snapshot.clone();
        assert_eq!(
            snapshot.retained_bytes(render_element_bytes),
            next.retained_bytes(render_element_bytes)
        );
        drop(records);
        drop(cache);
        assert_eq!(
            snapshot, next,
            "snapshots remain valid after the source cache is released"
        );
        assert_eq!(snapshot.cells.len(), ROWS * COLUMNS);
    }

    #[test]
    fn editable_table_structure_clones_share_row_key_payloads() {
        use crate::test_support::large_table_fixture::{
            plain_table_document, session_with_document,
        };
        const ROWS: usize = 17;
        const COLUMNS: usize = 3;
        let session = session_with_document(&plain_table_document(ROWS, COLUMNS));
        let cache = crate::render::incremental::CachedRenderBlocks::build(
            session.engine.document().unwrap(),
            session.engine.schema(),
            &crate::boundary::ResourceLimits::default(),
        )
        .unwrap();
        let mut records = Vec::new();
        cache.visit_table_records(&mut records);
        let source = records[0].1;
        let cloned = source.structure.clone();
        assert_eq!(source.structure, cloned);
        for (index, (before, after)) in source
            .structure
            .source_rows
            .iter()
            .zip(&cloned.source_rows)
            .enumerate()
        {
            assert_eq!(
                before.attrs_key.as_ptr(),
                after.attrs_key.as_ptr(),
                "row={index}: localized table copies must share canonical attribute keys"
            );
        }
        drop(records);
        drop(cache);
        assert_eq!(cloned.source_rows.len(), ROWS);
        assert!(cloned
            .source_rows
            .iter()
            .all(|row| row.cell_count == COLUMNS as u32));
    }

    #[test]
    fn editable_cell_clones_preserve_capacity_identity_and_final_release() {
        use crate::test_support::large_table_fixture::{
            plain_table_document, session_with_document,
        };
        const ROWS: usize = 3;
        const COLUMNS: usize = 3;
        const EXTRA_CAPACITY: usize = 11;
        let session = session_with_document(&plain_table_document(ROWS, COLUMNS));
        let cache = crate::render::incremental::CachedRenderBlocks::build(
            session.engine.document().unwrap(),
            session.engine.schema(),
            &crate::boundary::ResourceLimits::default(),
        )
        .unwrap();
        let mut records = Vec::new();
        cache.visit_table_records(&mut records);
        let source = records[0].1;
        for count in [0, 1, ROWS * COLUMNS] {
            for spare in [0, EXTRA_CAPACITY] {
                let mut cells = Vec::with_capacity(count + spare);
                cells.extend(
                    source
                        .cells
                        .iter()
                        .take(count)
                        .map(|cell| Arc::new((**cell).clone())),
                );
                let table = TableRenderRecord::new(source.structure.clone(), cells, None);
                let weak: Vec<_> = table.cells.iter().map(Arc::downgrade).collect();
                let mut baseline = Vec::new();
                baseline.try_reserve_exact(table.cell_capacity()).unwrap();
                baseline.extend(table.cells.iter().cloned());
                let candidate = table.try_clone_cells().unwrap();
                assert_eq!(
                    candidate.capacity(),
                    baseline.capacity(),
                    "count={count} spare={spare}"
                );
                assert_eq!(candidate.len(), baseline.len());
                for (old, new) in baseline.iter().zip(&candidate) {
                    assert!(Arc::ptr_eq(old, new));
                    assert_eq!(Arc::strong_count(new), 3);
                }
                drop(baseline);
                drop(table);
                assert!(candidate.iter().all(|cell| Arc::strong_count(cell) == 1));
                assert!(weak.iter().all(|cell| cell.strong_count() == 1));
                drop(candidate);
                assert!(weak.iter().all(|cell| cell.upgrade().is_none()));
            }
        }
    }

    #[test]
    fn standalone_cell_output_meter_preserves_the_render_stack_guard() {
        use crate::test_support::large_table_fixture::{
            plain_table_document, session_with_document,
        };
        let session = session_with_document(&plain_table_document(1, 1));
        let cache = crate::render::incremental::CachedRenderBlocks::build(
            session.engine.document().unwrap(),
            session.engine.schema(),
            &crate::boundary::ResourceLimits::default(),
        )
        .unwrap();
        let output = cache.materialize();
        let RenderElement::Table { table, .. } = &output[0][0] else {
            panic!("table fixture");
        };
        stacker::grow(RENDER_STACK_RED_ZONE / 2, || {
            table.cells[0].retained_bytes(|_| {
                let remaining = stacker::remaining_stack().expect("known grown stack bounds");
                assert!(
                    remaining >= RENDER_STACK_RED_ZONE,
                    "recursive payload meter reached with only {remaining} stack bytes"
                );
                0
            });
        });
    }
}
