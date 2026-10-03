use std::sync::Arc;

use crate::ffi_v2::types::FfiError;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FfiViewerSourceKind {
    Json,
    Html,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiViewerCompileRequest {
    pub source_kind: FfiViewerSourceKind,
    pub source: String,
    pub config_json: String,
    pub images_enabled: bool,
    pub mention_prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiViewerMark {
    pub mark_type: String,
    pub attrs_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FfiViewerElement {
    Table {
        table_id: String,
    },
    TextRun {
        text: String,
        marks: Vec<FfiViewerMark>,
    },
    InlineAtom {
        node_type: String,
        doc_pos: u32,
        attrs_json: String,
        label: String,
    },
    BlockAtom {
        node_type: String,
        doc_pos: u32,
        attrs_json: String,
        label: String,
    },
    BlockStart {
        node_type: String,
        language: Option<String>,
        depth: u16,
        list_context_json: Option<String>,
    },
    BlockEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiViewerTableCell {
    pub source_pos: u32,
    pub source_end: u32,
    pub row: u32,
    pub column: u32,
    pub rowspan: u32,
    pub colspan: u32,
    pub header: bool,
    pub attrs_key: String,
    pub content_key: String,
    pub elements: Vec<FfiViewerElement>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiViewerTable {
    pub table_pos: u32,
    pub source_end: u32,
    pub rows: u32,
    pub columns: u32,
    pub column_widths: Vec<Option<u32>>,
    pub direction: Option<String>,
    pub irregular: bool,
    pub read_only_descendants: bool,
    pub attrs_key: String,
    pub source_rows: Vec<crate::tables::render::TableRenderRow>,
    pub cells: Vec<FfiViewerTableCell>,
    pub synthetic_regions: Vec<crate::tables::render::TableRenderSyntheticRegion>,
    pub failure: Option<crate::tables::render::TableRenderFailure>,
    pub compatibility_diagnostic: Option<crate::tables::render::TableCompatibilityDiagnostic>,
}

#[derive(uniffi::Object)]
pub struct ViewerCompiledDocument {
    pub(crate) table_attributes: std::collections::HashMap<String, String>,
    pub(crate) semantic_key: String,
    pub(crate) elements: Vec<FfiViewerElement>,
    pub(crate) table_records: Vec<FfiViewerTable>,
    pub(crate) is_empty: bool,
    pub(crate) preferred_text_block_name: String,
    pub(crate) trailing_empty_text_block_count: u32,
    pub(crate) retained_bytes: usize,
}

#[uniffi::export]
impl ViewerCompiledDocument {
    pub fn table_attributes(&self) -> std::collections::HashMap<String, String> {
        self.table_attributes.clone()
    }

    pub fn semantic_key(&self) -> String {
        self.semantic_key.clone()
    }

    pub fn elements(&self) -> Vec<FfiViewerElement> {
        self.elements.clone()
    }

    pub fn table_records(&self) -> Vec<FfiViewerTable> {
        self.table_records.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.is_empty
    }

    pub fn preferred_text_block_name(&self) -> String {
        self.preferred_text_block_name.clone()
    }

    pub fn trailing_empty_text_block_count(&self) -> u32 {
        self.trailing_empty_text_block_count
    }

    pub fn retained_bytes_decimal(&self) -> String {
        self.retained_bytes.to_string()
    }
}

#[derive(uniffi::Record)]
pub struct FfiViewerCompileResult {
    pub value: Option<Arc<ViewerCompiledDocument>>,
    pub error: Option<FfiError>,
}

impl FfiViewerCompileResult {
    pub(crate) fn ok(value: Arc<ViewerCompiledDocument>) -> Self {
        Self {
            value: Some(value),
            error: None,
        }
    }

    pub(crate) fn err(error: FfiError) -> Self {
        Self {
            value: None,
            error: Some(error),
        }
    }
}
