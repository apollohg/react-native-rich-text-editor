use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::ffi_v2::editor as v2;
use crate::ffi_v2::types::FfiJsonResult;
use crate::session::{
    CollaborationLimits, DocumentState, EditorSession, EditorSessionConfig, SessionPolicy,
};
use crate::tables::tests::{tabled_schema_json, PROSEMIRROR_TABLE_NAMES};
use crate::yrs_engine::{
    EditingLimits, InitializationMode, ReplacementHistory, YrsDocumentEngine, YrsEngineConfig,
};

pub(crate) const GRID_BOUNDARY_FIXTURE_SLOTS: usize = 24_990;
const GRID_BOUNDARY_FIXTURE_COLUMNS: usize = 10;
const HEADER_ROW: usize = 0;
const MULTI_PARAGRAPH_FIXTURE_ROWS: usize = 4;
const MULTI_PARAGRAPH_FIXTURE_COLUMNS: usize = 3;
const MULTI_PARAGRAPH_CELL: (usize, usize) = (1, 1);
const MULTI_PARAGRAPH_CELL_PARAGRAPHS: usize = 3;
const NESTED_TABLE_CELL: (usize, usize) = (2, 2);
const NESTED_TABLE_SIZE: usize = 2;
const NESTED_CELL_TEXT_PREFIX: &str = "N";
const INLINE_ATOM_NODE: &str = "mention";
const INLINE_ATOM_ID: &str = "fixture-atom";
const TWO_TABLE_FIXTURE_SIZE: usize = 3;
const TWO_TABLE_FIXTURE_PROSE: [&str; 2] = ["before tables", "between tables"];
const FRAGMENT_NAME: &str = "prosemirror";
const FIXTURE_REQUEST_ID: u64 = 1;

pub(crate) fn fixture_cell_text(row: usize, column: usize) -> String {
    format!("R{row:04}C{column:04}XY")
}

pub(crate) fn keystroke_cell(rows: usize, columns: usize) -> usize {
    rows * columns / 2
}

fn text_paragraph(text: &str) -> Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
}

fn table_of(rows: usize, columns: usize, content: impl Fn(usize, usize) -> Vec<Value>) -> Value {
    let table_rows: Vec<Value> = (0..rows)
        .map(|row| {
            let kind = if row == HEADER_ROW {
                "table_header"
            } else {
                "table_cell"
            };
            let cells: Vec<Value> = (0..columns)
                .map(|column| json!({"type": kind, "content": content(row, column)}))
                .collect();
            json!({"type": "table_row", "content": cells})
        })
        .collect();
    json!({"type": "table", "content": table_rows})
}

fn plain_table(rows: usize, columns: usize) -> Value {
    table_of(rows, columns, |row, column| {
        vec![text_paragraph(&fixture_cell_text(row, column))]
    })
}

fn document_of(blocks: Vec<Value>) -> Value {
    json!({"type": "doc", "content": blocks})
}

pub(crate) fn plain_table_document(rows: usize, columns: usize) -> Value {
    document_of(vec![plain_table(rows, columns)])
}

pub(crate) fn multi_paragraph_cell_document() -> Value {
    let nested = table_of(NESTED_TABLE_SIZE, NESTED_TABLE_SIZE, |row, column| {
        vec![text_paragraph(&format!(
            "{NESTED_CELL_TEXT_PREFIX}{}",
            fixture_cell_text(row, column)
        ))]
    });
    let table = table_of(
        MULTI_PARAGRAPH_FIXTURE_ROWS,
        MULTI_PARAGRAPH_FIXTURE_COLUMNS,
        |row, column| match (row, column) {
            MULTI_PARAGRAPH_CELL => (0..MULTI_PARAGRAPH_CELL_PARAGRAPHS)
                .map(|paragraph| {
                    let mut content = vec![json!({
                        "type": "text",
                        "text": format!("{}P{paragraph}", fixture_cell_text(row, column)),
                    })];
                    if paragraph == 0 {
                        content.push(
                            json!({"type": INLINE_ATOM_NODE, "attrs": {"id": INLINE_ATOM_ID}}),
                        );
                    }
                    json!({"type": "paragraph", "content": content})
                })
                .collect(),
            NESTED_TABLE_CELL => vec![nested.clone()],
            _ => vec![text_paragraph(&fixture_cell_text(row, column))],
        },
    );
    document_of(vec![table])
}

pub(crate) fn two_table_document() -> Value {
    let [before, between] = TWO_TABLE_FIXTURE_PROSE;
    document_of(vec![
        text_paragraph(before),
        plain_table(TWO_TABLE_FIXTURE_SIZE, TWO_TABLE_FIXTURE_SIZE),
        text_paragraph(between),
        plain_table(TWO_TABLE_FIXTURE_SIZE, TWO_TABLE_FIXTURE_SIZE),
    ])
}

pub(crate) fn grid_boundary_document(slots: usize) -> Value {
    assert_eq!(
        slots % GRID_BOUNDARY_FIXTURE_COLUMNS,
        0,
        "grid boundary fixtures fill whole rows of {GRID_BOUNDARY_FIXTURE_COLUMNS} columns"
    );
    plain_table_document(
        slots / GRID_BOUNDARY_FIXTURE_COLUMNS,
        GRID_BOUNDARY_FIXTURE_COLUMNS,
    )
}

pub(crate) fn session_with_document(document: &Value) -> EditorSession {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        YrsDocumentEngine::new(YrsEngineConfig {
            schema: crate::schema::presets::prosemirror_table_schema(),
            fragment_name: FRAGMENT_NAME.into(),
            initialization_mode: InitializationMode::LocalEmpty,
            resource_limits: ResourceLimits::default(),
            editing_limits: EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .expect("the fixture engine initializes"),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .expect("the fixture session initializes");
    session
        .replace_document_json(
            FIXTURE_REQUEST_ID,
            &document.to_string(),
            ReplacementHistory::ResetAndClear,
        )
        .expect("the fixture document imports");
    session
}

pub(crate) fn ffi_value(result: &FfiJsonResult) -> Value {
    serde_json::from_str(
        result
            .value
            .as_deref()
            .unwrap_or_else(|| panic!("the FFI call succeeds: {:?}", result.error)),
    )
    .expect("the FFI value is JSON")
}

pub(crate) fn ffi_empty_editor() -> String {
    ffi_value(&v2::editor_v2_create(
        json!({
            "schema": tabled_schema_json(PROSEMIRROR_TABLE_NAMES),
            "initialization": {"type": "localEmpty"},
        })
        .to_string(),
        None,
    ))["editorId"]
        .as_str()
        .expect("create returns an editor id")
        .to_owned()
}

pub(crate) fn ffi_replace_request(document: &Value) -> String {
    json!({
        "version": 1,
        "requestId": FIXTURE_REQUEST_ID.to_string(),
        "setJson": document,
        "history": "resetAndClear",
    })
    .to_string()
}

pub(crate) fn ffi_editor_with_document(document: &Value) -> String {
    let editor_id = ffi_empty_editor();
    ffi_value(&v2::editor_v2_replace_document(
        editor_id.clone(),
        ffi_replace_request(document),
    ));
    editor_id
}

fn count_nodes_of_type(value: &Value, node_type: &str) -> usize {
    usize::from(value["type"] == node_type)
        + value["content"].as_array().map_or(0, |children| {
            children
                .iter()
                .map(|child| count_nodes_of_type(child, node_type))
                .sum()
        })
}

#[test]
fn shared_table_fixtures_import_with_their_declared_shapes() {
    let multi_paragraph_slots = MULTI_PARAGRAPH_FIXTURE_ROWS * MULTI_PARAGRAPH_FIXTURE_COLUMNS
        + NESTED_TABLE_SIZE * NESTED_TABLE_SIZE;
    let two_table_slots = 2 * TWO_TABLE_FIXTURE_SIZE * TWO_TABLE_FIXTURE_SIZE;
    for (name, document, tables, slots, atoms) in [
        (
            "multi-paragraph cell",
            multi_paragraph_cell_document(),
            2,
            multi_paragraph_slots,
            1,
        ),
        ("two tables", two_table_document(), 2, two_table_slots, 0),
        (
            "grid boundary",
            grid_boundary_document(GRID_BOUNDARY_FIXTURE_SLOTS),
            1,
            GRID_BOUNDARY_FIXTURE_SLOTS,
            0,
        ),
    ] {
        let session = session_with_document(&document);
        let index = session
            .engine
            .table_projection_index()
            .expect("the fixture engine is ready");
        let projected: Vec<_> = index
            .positions()
            .map(|position| index.table_at(position).expect("listed tables project"))
            .collect();
        let projected_slots: usize = projected
            .iter()
            .map(|table| (table.rows * table.columns) as usize)
            .sum();
        let imported = session
            .engine
            .document_json()
            .expect("the fixture engine is ready");
        eprintln!(
            "{name}: {} tables, {projected_slots} slots",
            projected.len()
        );
        assert_eq!(projected.len(), tables, "{name}: projected table count");
        assert_eq!(projected_slots, slots, "{name}: projected grid slots");
        assert_eq!(
            count_nodes_of_type(&imported, INLINE_ATOM_NODE),
            atoms,
            "{name}: inline atoms survive import"
        );

        let editor_id = ffi_editor_with_document(&document);
        assert_eq!(
            ffi_value(&v2::editor_v2_get_document_json(editor_id.clone())),
            imported,
            "{name}: the FFI import matches the session import"
        );
        assert!(
            v2::editor_v2_destroy(editor_id).error.is_none(),
            "{name}: the fixture editor is destroyed"
        );
    }
}
