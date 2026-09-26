use serde_json::{json, Value};

use crate::model::Document;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::commands::{
    TableCommand, TableEdge, TableHeaderTarget, DEFAULT_INSERTED_TABLE_COLUMNS,
    DEFAULT_INSERTED_TABLE_HEADER_ROW, DEFAULT_INSERTED_TABLE_ROWS, MIN_TABLE_COLUMN_WIDTH,
};
use crate::tables::normalize_tests::{
    cell, cell_with, header_cell, limits, row, schema, seeded_session, table,
};
use crate::tables::projection::{project_table, ProjectedTable, TableGridBudget};
use crate::tables::tests::{
    tabled_schema, tabled_schema_with_second_text_block, PROSEMIRROR_TABLE_NAMES,
    SECOND_TEXT_BLOCK_NODE,
};
use crate::yrs_engine::{
    Affinity, EditingLimits, EditorOffsetKind, HistoryPolicy, InitializationMode, OperationError,
    RevisionedPosition, SelectionInput, SelectionIntent, TransactionOrigin, TypedCommand,
    TypedOperation, TypedTransaction, TypedTransactionResult, YrsDocumentEngine, YrsEngineConfig,
};

const FRAGMENT_NAME: &str = "prosemirror";
const TABLE_NODE: &str = "table";
const CELL_NODE: &str = "table_cell";
const HEADER_CELL_NODE: &str = "table_header";
const PARAGRAPH_NODE: &str = "paragraph";
const TABLE_POSITION: u32 = 0;
const CELL_TEXT_OFFSET: u32 = 2;
const REQUEST_ID: u64 = 7;
const SINGLE_SPAN: u32 = 1;
const NO_DOCUMENT_UPDATES: usize = 0;
const ONE_DOCUMENT_UPDATE: usize = 1;
const CUSTOM_TABLE_NAMES: [&str; 4] = ["grid", "gridRow", "gridCell", "gridHeader"];
const ANCHOR_CELL: usize = 2;
const ANCHOR_FOR_AVAILABILITY: usize = 0;
const LAST_REGULAR_CELL: usize = 3;
const SECOND_CELL_ANCHOR: usize = 1;
const DOCUMENT_INVALID_CODE: &str = "DOCUMENT_INVALID";
const DOCUMENT_LIMIT_EXCEEDED_CODE: &str = "DOCUMENT_LIMIT_EXCEEDED";
const OPERATION_WORK_BUDGET_CODE: &str = "OPERATION_LIMIT_EXCEEDED";
const IDENTITY_FIXTURE_CELLS: usize = 5;
const ONE_CHARACTER: u32 = 1;
const NO_CELLS: usize = 0;
const PROBE_COLUMN_WIDTH: u32 = 140;
const UPDATED_COLUMN_WIDTH: u32 = 160;
const SECOND_LOGICAL_COLUMN: u32 = 1;
const PROSE_PREFIX_TEXT: &str = "before";
const PROSE_PREFIX_CARET: u32 = 2;
const PROSE_PREFIX_TABLE_POSITION: u32 = 8;
const OPERATION_INVALID_CODE: &str = "OPERATION_INVALID";
const OUTSIDE_RESIZE_FIXTURE: u32 = 2;
const SHARED_SURFACE_PROJECTIONS: u64 = 1;
const STAGED_DELETION_PROJECTIONS: u64 = 4;

pub(crate) fn engine_with(schema: Schema, content: Vec<Value>) -> YrsDocumentEngine {
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema,
        fragment_name: FRAGMENT_NAME.into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: limits(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .expect("the tabled engine initializes");
    engine
        .import_json(
            &json!({ "type": "doc", "content": content }).to_string(),
            TransactionOrigin::DocumentImport,
        )
        .expect("the fixture document imports");
    engine
}

pub(crate) fn seeded(content: Vec<Value>) -> YrsDocumentEngine {
    engine_with(schema(), content)
}

pub(crate) fn document_of(engine: &YrsDocumentEngine) -> &Document {
    engine.document().expect("the engine is ready")
}

pub(crate) fn table_of(engine: &YrsDocumentEngine) -> Value {
    engine.document_json().expect("the engine is ready")["content"][0].clone()
}

pub(crate) fn engine_schema(engine: &YrsDocumentEngine) -> Schema {
    engine.schema().clone()
}

pub(crate) fn projection_of(engine: &YrsDocumentEngine) -> ProjectedTable {
    let document = document_of(engine);
    let table = document
        .node_at(&[0])
        .expect("the fixture holds a table")
        .clone();
    project_table(
        &table,
        TABLE_POSITION,
        &engine_schema(engine),
        &mut TableGridBudget::new(limits().max_table_grid_slots),
    )
    .expect("the fixture projects")
}

pub(crate) fn cell_openings(engine: &YrsDocumentEngine) -> Vec<u32> {
    projection_of(engine)
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .collect()
}

fn inside_cell(engine: &YrsDocumentEngine, index: usize) -> RevisionedPosition {
    let opening = cell_openings(engine)[index];
    let interior = crate::tables::interchange::first_editable_position_in_cell(
        document_of(engine),
        &engine_schema(engine),
        opening,
    )
    .expect("the fixture schema resolves its table roles")
    .expect("the fixture anchors on a cell that holds an editable position");
    let map = engine.position_map().expect("the engine is ready");
    RevisionedPosition {
        offset: map.doc_to_scalar(interior, document_of(engine)),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    }
}

fn after_first_character(engine: &YrsDocumentEngine, index: usize) -> RevisionedPosition {
    let opening = cell_openings(engine)[index];
    let map = engine.position_map().expect("the engine is ready");
    RevisionedPosition {
        offset: map.doc_to_scalar(
            opening + CELL_TEXT_OFFSET + ONE_CHARACTER,
            document_of(engine),
        ),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    }
}

pub(crate) fn select_cells(engine: &mut YrsDocumentEngine, anchor: usize, head: usize) {
    let anchor = inside_cell(engine, anchor);
    let head = inside_cell(engine, head);
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Cell {
                anchor: anchor.into(),
                head: head.into(),
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the cell selection applies");
}

fn select_cell(engine: &mut YrsDocumentEngine, index: usize) {
    select_cells(engine, index, index);
}

pub(crate) fn resolved_cells(engine: &YrsDocumentEngine) -> Option<(u32, u32)> {
    match engine.resolved_selection()? {
        crate::yrs_engine::ResolvedSelection::Cell { anchor, head } => {
            Some((anchor.document, head.document))
        }
        crate::yrs_engine::ResolvedSelection::Text { .. }
        | crate::yrs_engine::ResolvedSelection::Node { .. }
        | crate::yrs_engine::ResolvedSelection::All => None,
    }
}

fn run(
    engine: &mut YrsDocumentEngine,
    command: TableCommand,
) -> Result<Option<TypedTransactionResult>, OperationError> {
    engine.apply_command(REQUEST_ID, TypedCommand::Table(command))
}

fn applied(engine: &mut YrsDocumentEngine, command: TableCommand) {
    let result = run(engine, command).expect("the table command plans");
    assert!(
        result.is_some(),
        "the table command produced no transaction"
    );
}

pub(crate) fn row_types(table: &Value, row: usize) -> Vec<String> {
    table["content"][row]["content"]
        .as_array()
        .expect("the row holds cells")
        .iter()
        .map(|cell| {
            cell["type"]
                .as_str()
                .expect("a cell names a type")
                .to_owned()
        })
        .collect()
}

pub(crate) fn row_texts(table: &Value, row: usize) -> Vec<String> {
    table["content"][row]["content"]
        .as_array()
        .expect("the row holds cells")
        .iter()
        .map(|cell| {
            cell["content"][0]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

pub(crate) fn row_count(table: &Value) -> usize {
    table["content"]
        .as_array()
        .expect("the table holds rows")
        .len()
}

fn spans_at(engine: &YrsDocumentEngine, row: u32, column: u32) -> (u32, u32) {
    let projected = projection_of(engine);
    let offset = (row as usize) * (projected.columns as usize) + column as usize;
    let index = projected.slots[offset].expect("the slot holds a real cell");
    let rect = &projected.cells[index].rect;
    (rect.rowspan, rect.colspan)
}

pub(crate) fn geometry(projected: &ProjectedTable) -> (u32, u32, bool) {
    (projected.rows, projected.columns, projected.irregular)
}

pub(crate) fn regular_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell("a0"), cell("a1")]),
        row(vec![cell("b0"), cell("b1")]),
    ])]
}

pub(crate) fn tall_span_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![
            cell_with(SINGLE_SPAN, 2, Value::Null, "tall"),
            cell("a1"),
        ]),
        row(vec![cell("b1")]),
        row(vec![cell("c0"), cell("c1")]),
    ])]
}

pub(crate) fn wide_span_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell_with(2, SINGLE_SPAN, json!([120, 160]), "wide")]),
        row(vec![cell("b0"), cell("b1")]),
    ])]
}

pub(crate) fn header_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![header_cell("h0"), header_cell("h1")]),
        row(vec![cell("b0"), cell("b1")]),
    ])]
}

fn ragged_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell("a0"), cell("a1")]),
        row(vec![cell("b0")]),
    ])]
}

#[test]
fn a_default_insertion_mints_three_rows_of_three_with_a_header_row() {
    let mut engine = seeded(vec![json!({ "type": PARAGRAPH_NODE })]);

    applied(
        &mut engine,
        TableCommand::InsertTable {
            rows: DEFAULT_INSERTED_TABLE_ROWS,
            columns: DEFAULT_INSERTED_TABLE_COLUMNS,
            with_header_row: DEFAULT_INSERTED_TABLE_HEADER_ROW,
        },
    );

    let table = table_of(&engine);
    assert_eq!(table["type"], json!(TABLE_NODE));
    assert_eq!(row_count(&table), DEFAULT_INSERTED_TABLE_ROWS as usize);
    assert_eq!(row_types(&table, 0), vec![HEADER_CELL_NODE.to_owned(); 3]);
    assert_eq!(row_types(&table, 1), vec![CELL_NODE.to_owned(); 3]);
    assert_eq!(row_types(&table, 2), vec![CELL_NODE.to_owned(); 3]);
    assert_eq!(geometry(&projection_of(&engine)), (3, 3, false));
}

#[test]
fn custom_insertion_dimensions_are_honoured_without_a_header_row() {
    let mut engine = seeded(vec![json!({ "type": PARAGRAPH_NODE })]);

    applied(
        &mut engine,
        TableCommand::InsertTable {
            rows: 2,
            columns: 4,
            with_header_row: false,
        },
    );

    let table = table_of(&engine);
    assert_eq!(row_count(&table), 2);
    assert_eq!(row_types(&table, 0), vec![CELL_NODE.to_owned(); 4]);
    assert_eq!(geometry(&projection_of(&engine)), (2, 4, false));
}

#[test]
fn insertion_uses_the_role_names_the_schema_declares() {
    let schema = tabled_schema(CUSTOM_TABLE_NAMES);
    let mut engine = engine_with(schema, vec![json!({ "type": PARAGRAPH_NODE })]);

    applied(
        &mut engine,
        TableCommand::InsertTable {
            rows: 2,
            columns: 2,
            with_header_row: true,
        },
    );

    let table = table_of(&engine);
    assert_eq!(table["type"], json!(CUSTOM_TABLE_NAMES[0]));
    assert_eq!(table["content"][0]["type"], json!(CUSTOM_TABLE_NAMES[1]));
    assert_eq!(
        row_types(&table, 0),
        vec![CUSTOM_TABLE_NAMES[3].to_owned(); 2]
    );
    assert_eq!(
        row_types(&table, 1),
        vec![CUSTOM_TABLE_NAMES[2].to_owned(); 2]
    );
}

#[test]
fn a_table_is_never_inserted_inside_an_existing_table() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);
    let before = table_of(&engine);

    let outcome = run(
        &mut engine,
        TableCommand::InsertTable {
            rows: DEFAULT_INSERTED_TABLE_ROWS,
            columns: DEFAULT_INSERTED_TABLE_COLUMNS,
            with_header_row: DEFAULT_INSERTED_TABLE_HEADER_ROW,
        },
    )
    .expect("nesting is refused as a plan, not as an error");

    assert!(outcome.is_none(), "a nested table must not be planned");
    assert_eq!(table_of(&engine), before);
}

#[test]
fn a_row_added_below_a_header_row_is_a_body_row() {
    let mut engine = seeded(header_fixture());
    select_cell(&mut engine, 0);

    applied(
        &mut engine,
        TableCommand::AddTableRow {
            side: TableEdge::After,
        },
    );

    let table = table_of(&engine);
    assert_eq!(row_count(&table), 3);
    assert_eq!(row_types(&table, 0), vec![HEADER_CELL_NODE.to_owned(); 2]);
    assert_eq!(row_types(&table, 1), vec![CELL_NODE.to_owned(); 2]);
}

#[test]
fn a_row_added_above_a_header_row_is_a_body_row() {
    let mut engine = seeded(header_fixture());
    select_cell(&mut engine, 0);

    applied(
        &mut engine,
        TableCommand::AddTableRow {
            side: TableEdge::Before,
        },
    );

    let table = table_of(&engine);
    assert_eq!(row_types(&table, 0), vec![CELL_NODE.to_owned(); 2]);
    assert_eq!(row_types(&table, 1), vec![HEADER_CELL_NODE.to_owned(); 2]);
}

#[test]
fn a_row_added_between_body_rows_keeps_their_cell_type() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);

    applied(
        &mut engine,
        TableCommand::AddTableRow {
            side: TableEdge::After,
        },
    );

    let table = table_of(&engine);
    assert_eq!(row_count(&table), 3);
    assert_eq!(row_types(&table, 1), vec![CELL_NODE.to_owned(); 2]);
    assert_eq!(row_texts(&table, 1), vec![String::new(), String::new()]);
    assert_eq!(row_texts(&table, 2), vec!["b0".to_owned(), "b1".to_owned()]);
}

#[test]
fn inserting_a_row_through_a_rowspan_grows_the_spanning_cell_instead_of_filling_it() {
    let mut engine = seeded(tall_span_fixture());
    select_cell(&mut engine, 2);

    applied(
        &mut engine,
        TableCommand::AddTableRow {
            side: TableEdge::Before,
        },
    );

    let table = table_of(&engine);
    assert_eq!(row_count(&table), 4);
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["rowspan"],
        json!(3),
        "the cell the new row passes through grows instead of gaining a neighbour",
    );
    assert_eq!(
        table["content"][1]["content"]
            .as_array()
            .expect("the inserted row holds cells")
            .len(),
        1,
        "only the columns the span does not cover gain a fresh cell",
    );
    assert_eq!(geometry(&projection_of(&engine)), (4, 2, false));
}

#[test]
fn deleting_a_row_a_span_continues_into_shrinks_that_span() {
    let mut engine = seeded(tall_span_fixture());
    select_cell(&mut engine, 2);

    applied(&mut engine, TableCommand::DeleteTableRows);

    let table = table_of(&engine);
    assert_eq!(row_count(&table), 2);
    assert_eq!(spans_at(&engine, 0, 0), (SINGLE_SPAN, SINGLE_SPAN));
    assert_eq!(
        row_texts(&table, 0),
        vec!["tall".to_owned(), "a1".to_owned()]
    );
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, false));
}

#[test]
fn deleting_the_row_a_span_starts_in_carries_the_continuing_cell_content_down() {
    let mut engine = seeded(tall_span_fixture());
    select_cell(&mut engine, 1);

    applied(&mut engine, TableCommand::DeleteTableRows);

    let table = table_of(&engine);
    assert_eq!(row_count(&table), 2);
    assert_eq!(
        row_texts(&table, 0),
        vec!["tall".to_owned(), "b1".to_owned()],
        "the surviving half of the span keeps its content",
    );
    assert_eq!(spans_at(&engine, 0, 0), (SINGLE_SPAN, SINGLE_SPAN));
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, false));
}

#[test]
fn the_last_row_cannot_be_deleted() {
    let mut engine = seeded(vec![table(vec![row(vec![cell("only")])])]);
    select_cell(&mut engine, 0);
    let before = table_of(&engine);

    let outcome = run(&mut engine, TableCommand::DeleteTableRows)
        .expect("emptying a table is refused as a plan, not as an error");

    assert!(outcome.is_none());
    assert_eq!(table_of(&engine), before);
}

#[test]
fn the_last_column_cannot_be_deleted() {
    let mut engine = seeded(vec![table(vec![
        row(vec![cell("a")]),
        row(vec![cell("b")]),
    ])]);
    select_cell(&mut engine, 0);
    let before = table_of(&engine);

    let outcome = run(&mut engine, TableCommand::DeleteTableColumns)
        .expect("emptying a table is refused as a plan, not as an error");

    assert!(outcome.is_none());
    assert_eq!(table_of(&engine), before);
}

#[test]
fn the_whole_table_goes_only_on_an_explicit_table_deletion() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);

    applied(&mut engine, TableCommand::DeleteTable);

    let json = engine.document_json().expect("the engine is ready");
    assert!(
        json["content"]
            .as_array()
            .is_none_or(|content| content.iter().all(|node| node["type"] != json!(TABLE_NODE))),
        "explicit table deletion leaves no table behind: {json}",
    );
}

#[test]
fn inserting_a_column_through_a_colspan_widens_it_and_its_widths() {
    let mut engine = seeded(wide_span_fixture());
    select_cell(&mut engine, 1);

    applied(
        &mut engine,
        TableCommand::AddTableColumn {
            side: TableEdge::After,
        },
    );

    let table = table_of(&engine);
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["colspan"],
        json!(3)
    );
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["colwidth"],
        json!([120, Value::Null, 160]),
        "the widened span gains an unset width at the inserted offset",
    );
    assert_eq!(
        row_texts(&table, 1),
        vec!["b0".to_owned(), String::new(), "b1".to_owned()]
    );
    assert_eq!(geometry(&projection_of(&engine)), (2, 3, false));
}

#[test]
fn deleting_a_column_through_a_colspan_narrows_it_instead_of_removing_the_cell() {
    let mut engine = seeded(wide_span_fixture());
    select_cell(&mut engine, 1);

    applied(&mut engine, TableCommand::DeleteTableColumns);

    let table = table_of(&engine);
    assert_eq!(spans_at(&engine, 0, 0), (SINGLE_SPAN, SINGLE_SPAN));
    assert_eq!(row_texts(&table, 0), vec!["wide".to_owned()]);
    assert_eq!(row_texts(&table, 1), vec!["b1".to_owned()]);
    assert_eq!(geometry(&projection_of(&engine)), (2, 1, false));
}

#[test]
fn toggling_a_header_row_retypes_the_whole_row_and_toggles_back() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);

    applied(
        &mut engine,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Row,
        },
    );
    let headed = table_of(&engine);
    assert_eq!(row_types(&headed, 0), vec![HEADER_CELL_NODE.to_owned(); 2]);
    assert_eq!(row_types(&headed, 1), vec![CELL_NODE.to_owned(); 2]);
    assert_eq!(
        row_texts(&headed, 0),
        vec!["a0".to_owned(), "a1".to_owned()]
    );

    select_cell(&mut engine, 0);
    applied(
        &mut engine,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Row,
        },
    );
    assert_eq!(table_of(&engine), table_of(&seeded(regular_fixture())));
}

#[test]
fn toggling_a_header_carries_compatible_declared_attributes_across() {
    let mut engine = seeded(wide_span_fixture());
    select_cell(&mut engine, 0);

    applied(
        &mut engine,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Cell,
        },
    );

    let table = table_of(&engine);
    let toggled = &table["content"][0]["content"][0];
    assert_eq!(toggled["type"], json!(HEADER_CELL_NODE));
    assert_eq!(toggled["attrs"]["colspan"], json!(2));
    assert_eq!(toggled["attrs"]["colwidth"], json!([120, 160]));
    assert_eq!(row_texts(&table, 0), vec!["wide".to_owned()]);
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, false));
}

#[test]
fn selecting_table_rows_grows_the_rectangle_to_every_column() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);
    let openings = cell_openings(&engine);

    applied(&mut engine, TableCommand::SelectTableRows);

    assert_eq!(resolved_cells(&engine), Some((openings[0], openings[1])),);
    assert_eq!(table_of(&engine), table_of(&seeded(regular_fixture())));
}

#[test]
fn selecting_table_columns_grows_the_rectangle_to_every_row() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);
    let openings = cell_openings(&engine);

    applied(&mut engine, TableCommand::SelectTableColumns);

    assert_eq!(resolved_cells(&engine), Some((openings[0], openings[2])),);
}

#[test]
fn selecting_rows_can_end_on_an_outer_cell_with_only_nested_content() {
    let mut engine = seeded(vec![table(vec![row(vec![
        cell("first"),
        nested_only_cell(),
    ])])]);
    select_cell(&mut engine, 0);
    let openings = cell_openings(&engine);
    let before = engine.document_json();
    let before_revision = engine.revision();

    applied(&mut engine, TableCommand::SelectTableRows);

    assert_eq!(resolved_cells(&engine), Some((openings[0], openings[1])));
    assert_eq!(engine.document_json(), before);
    assert_eq!(engine.revision(), before_revision);
}

#[test]
fn clearing_a_merged_selection_keeps_its_spans_and_its_grid() {
    let mut engine = seeded(wide_span_fixture());
    let before = geometry(&projection_of(&engine));
    select_cells(&mut engine, 0, 2);

    applied(&mut engine, TableCommand::ClearTableCells);

    let table = table_of(&engine);
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["colspan"],
        json!(2)
    );
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["colwidth"],
        json!([120, 160]),
    );
    assert_eq!(row_texts(&table, 0), vec![String::new()]);
    assert_eq!(row_texts(&table, 1), vec![String::new(), String::new()]);
    assert_eq!(geometry(&projection_of(&engine)), before);
}

#[test]
fn clearing_over_a_synthetic_gap_clears_the_real_cells_and_leaves_the_gap() {
    let mut engine = seeded(ragged_fixture());
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, true));
    select_cells(&mut engine, 1, 2);

    applied(&mut engine, TableCommand::ClearTableCells);

    let table = table_of(&engine);
    assert_eq!(row_texts(&table, 0), vec![String::new(), String::new()]);
    assert_eq!(
        row_texts(&table, 1),
        vec![String::new()],
        "the short row keeps exactly one real cell",
    );
    assert_eq!(
        geometry(&projection_of(&engine)),
        (2, 2, true),
        "clearing must never mint the cell a synthetic slot stands in for",
    );
}

#[test]
fn backspace_over_a_cell_rectangle_clears_it_instead_of_deleting_a_text_span() {
    let mut engine = seeded(wide_span_fixture());
    let before = geometry(&projection_of(&engine));
    select_cells(&mut engine, 0, 2);

    let result = engine
        .apply_command(REQUEST_ID, TypedCommand::DeleteBackward)
        .expect("a backspace over a cell rectangle plans");

    assert!(
        result.is_some(),
        "a backspace over a cell rectangle must not be a silent no-op",
    );
    let table = table_of(&engine);
    assert_eq!(row_texts(&table, 0), vec![String::new()]);
    assert_eq!(row_texts(&table, 1), vec![String::new(), String::new()]);
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["colspan"],
        json!(2),
        "clearing by backspace keeps every span",
    );
    assert_eq!(geometry(&projection_of(&engine)), before);
}

#[test]
fn backspace_outside_a_cell_rectangle_still_deletes_text() {
    let mut engine = seeded(regular_fixture());
    let caret = after_first_character(&engine, 0);
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: caret,
                head: caret,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the text selection applies");

    engine
        .apply_command(REQUEST_ID, TypedCommand::DeleteBackward)
        .expect("a backspace inside a cell plans")
        .expect("a backspace inside a cell mutates");

    assert_eq!(
        row_texts(&table_of(&engine), 0),
        vec!["0".to_owned(), "a1".to_owned()],
        "a caret inside a cell still deletes one character",
    );
}

#[test]
fn no_table_command_takes_the_single_operation_prepared_path() {
    for command in [
        TableCommand::AddTableRow {
            side: TableEdge::After,
        },
        TableCommand::AddTableColumn {
            side: TableEdge::After,
        },
        TableCommand::DeleteTableRows,
        TableCommand::DeleteTableColumns,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Row,
        },
        TableCommand::ClearTableCells,
    ] {
        let mut engine = seeded(tall_span_fixture());
        select_cell(&mut engine, ANCHOR_CELL);
        let plan = engine
            .plan_command(REQUEST_ID, TypedCommand::Table(command))
            .unwrap_or_else(|error| panic!("{command:?} plans: {error:?}"));
        let crate::yrs_engine::CommandPlan::Transaction(transaction) = plan else {
            panic!("{command:?} must lower to a document transaction");
        };
        assert!(
            transaction
                .operations
                .iter()
                .all(|operation| matches!(operation, TypedOperation::EditStructure(_))),
            "{command:?} must stay outside the single-operation prepared admission, which \
             only ever carries InsertText, AddMark, RemoveMark or WrapInList",
        );
    }
}

#[test]
fn a_row_command_normalizes_a_ragged_table_before_it_inserts() {
    let mut engine = seeded(ragged_fixture());
    select_cell(&mut engine, 0);

    applied(
        &mut engine,
        TableCommand::AddTableRow {
            side: TableEdge::After,
        },
    );

    assert_eq!(geometry(&projection_of(&engine)), (3, 2, false));
}

#[test]
fn availability_separates_geometry_from_content_on_an_irregular_grid() {
    let engine = seeded(ragged_fixture());
    let document = document_of(&engine).clone();
    let schema = engine_schema(&engine);
    let openings = cell_openings(&engine);
    let selection = Selection::cell(openings[0], openings[0]);
    crate::tables::normalize::reset_planned_normalization_passes();

    let commands =
        crate::editor_state::command_applicability(&document, &schema, &selection, &limits());

    for geometry_command in [
        "addTableRowAfter",
        "addTableRowBefore",
        "deleteTableRows",
        "addTableColumnAfter",
        "deleteTableColumns",
        "toggleTableHeaderRow",
    ] {
        assert_eq!(
            commands.get(geometry_command),
            Some(&false),
            "{geometry_command} needs a regular grid it cannot prove here",
        );
    }
    for content_command in ["clearTableCells", "selectTableRows", "selectTableColumns"] {
        assert_eq!(
            commands.get(content_command),
            Some(&true),
            "{content_command} works on the projection as it is",
        );
    }
    assert_eq!(
        crate::tables::normalize::planned_normalization_passes(),
        0,
        "querying availability must plan no normalization of its own",
    );
    assert_eq!(document, *document_of(&engine));
}

#[test]
fn availability_projects_the_document_once_for_the_whole_command_surface() {
    let mut engine = seeded(tall_span_fixture());
    select_cell(&mut engine, ANCHOR_CELL);
    let openings = cell_openings(&engine);
    let selection = Selection::cell(openings[ANCHOR_CELL], openings[ANCHOR_CELL]);
    crate::tables::admission::reset_projection_derivations();

    let commands = crate::editor_state::command_applicability(
        document_of(&engine),
        &engine_schema(&engine),
        &selection,
        &limits(),
    );

    assert_eq!(
        commands.len(),
        crate::editor_state::ACTIVE_COMMAND_ENTRIES,
        "the surface must answer every command it advertises",
    );
    assert_eq!(
        crate::tables::admission::projection_derivations(),
        SHARED_SURFACE_PROJECTIONS + STAGED_DELETION_PROJECTIONS,
        "the surface must project once and pay only for the documents deletion stages",
    );
}

fn planner_accepts(fixture: Vec<Value>, anchor: usize, command: TableCommand) -> bool {
    let mut engine = seeded(fixture);
    select_cell(&mut engine, anchor);
    run(&mut engine, command)
        .expect("the table command plans without erroring on a regular grid")
        .is_some()
}

fn advertised(fixture: Vec<Value>, anchor: usize, command: TableCommand) -> bool {
    let mut engine = seeded(fixture);
    select_cell(&mut engine, anchor);
    let openings = cell_openings(&engine);
    crate::yrs_engine::TableCommandSurface::resolve(
        document_of(&engine),
        &engine_schema(&engine),
        &Selection::cell(openings[anchor], openings[anchor]),
        &limits(),
    )
    .is_available(command)
}

fn nested_only_cell() -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [table(vec![row(vec![cell("inner")])])],
    })
}

fn availability_fixtures() -> Vec<(&'static str, Vec<Value>, usize)> {
    vec![
        ("regular", regular_fixture(), ANCHOR_FOR_AVAILABILITY),
        ("tall span", tall_span_fixture(), ANCHOR_FOR_AVAILABILITY),
        (
            "header row",
            vec![table(vec![
                row(vec![header_cell("h0"), header_cell("h1")]),
                row(vec![cell("b0"), cell("b1")]),
            ])],
            ANCHOR_FOR_AVAILABILITY,
        ),
        ("last cell", regular_fixture(), LAST_REGULAR_CELL),
        (
            "nested only neighbour",
            vec![table(vec![row(vec![
                cell("a"),
                nested_only_cell(),
                cell("c"),
            ])])],
            ANCHOR_FOR_AVAILABILITY,
        ),
        (
            "nested only last cell",
            vec![table(vec![row(vec![cell("a"), nested_only_cell()])])],
            ANCHOR_FOR_AVAILABILITY,
        ),
        (
            "nested only below",
            vec![table(vec![
                row(vec![cell("a"), cell("b")]),
                row(vec![nested_only_cell(), cell("d")]),
            ])],
            ANCHOR_FOR_AVAILABILITY,
        ),
        (
            "nested only before",
            vec![table(vec![row(vec![nested_only_cell(), cell("c")])])],
            SECOND_CELL_ANCHOR,
        ),
        (
            "declared width",
            vec![table(vec![
                row(vec![
                    cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([PROBE_COLUMN_WIDTH]), "a0"),
                    cell("a1"),
                ]),
                row(vec![
                    cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([PROBE_COLUMN_WIDTH]), "b0"),
                    cell("b1"),
                ]),
            ])],
            ANCHOR_FOR_AVAILABILITY,
        ),
    ]
}

fn resize_idempotence_carve_out(command: TableCommand) -> bool {
    matches!(command, TableCommand::SetTableColumnWidth { .. })
}

#[test]
fn availability_matches_the_planner_for_every_table_command() {
    let mut carve_out_was_exercised = false;
    for (name, fixture, anchor) in availability_fixtures() {
        for command in every_table_command() {
            let advertised = advertised(fixture.clone(), anchor, command);
            let planned = planner_accepts(fixture.clone(), anchor, command);
            assert!(
                advertised || !planned,
                "on the {name} fixture at cell {anchor}, {command:?} plans but is not advertised, \
                 which hides an action the host could take",
            );
            if resize_idempotence_carve_out(command) {
                carve_out_was_exercised |= advertised && !planned;
                continue;
            }
            assert_eq!(
                advertised, planned,
                "on the {name} fixture at cell {anchor}, {command:?} must advertise \
                 exactly what the planner will do",
            );
        }
    }
    assert!(
        carve_out_was_exercised,
        "a fixture must actually generate the advertised-but-idempotent resize, or the carve out \
         is untested and the guard is weaker than it claims",
    );
}

#[test]
fn column_width_availability_answers_whether_a_resize_is_possible_at_all() {
    let already = PROBE_COLUMN_WIDTH;
    let fixture = vec![table(vec![
        row(vec![
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([already]), "a0"),
            cell("a1"),
        ]),
        row(vec![
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([already]), "b0"),
            cell("b1"),
        ]),
    ])];
    let resize = |width| TableCommand::SetTableColumnWidth {
        width,
        column: None,
        table_pos: None,
    };

    assert!(
        advertised(fixture.clone(), ANCHOR_FOR_AVAILABILITY, resize(already)),
        "a column that already carries the width is still resizable",
    );
    assert_eq!(
        advertised(fixture.clone(), ANCHOR_FOR_AVAILABILITY, resize(already)),
        advertised(
            fixture.clone(),
            ANCHOR_FOR_AVAILABILITY,
            resize(already + MIN_TABLE_COLUMN_WIDTH)
        ),
        "availability is width independent, so the advertised width carries no claim",
    );
    assert!(
        !planner_accepts(fixture.clone(), ANCHOR_FOR_AVAILABILITY, resize(already)),
        "re-applying the width the column already carries is a no-op, not an edit",
    );
    assert!(
        planner_accepts(
            fixture,
            ANCHOR_FOR_AVAILABILITY,
            resize(already + MIN_TABLE_COLUMN_WIDTH)
        ),
        "a different width is a real edit",
    );
}

#[test]
fn no_table_command_lowers_to_a_whole_table_replacement() {
    for command in [
        TableCommand::AddTableRow {
            side: TableEdge::After,
        },
        TableCommand::AddTableColumn {
            side: TableEdge::After,
        },
        TableCommand::DeleteTableRows,
        TableCommand::DeleteTableColumns,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Row,
        },
        TableCommand::ClearTableCells,
    ] {
        let mut engine = seeded(tall_span_fixture());
        select_cell(&mut engine, 2);
        let plan = engine
            .plan_command(REQUEST_ID, TypedCommand::Table(command))
            .unwrap_or_else(|error| panic!("{command:?} plans: {error:?}"));
        let crate::yrs_engine::CommandPlan::Transaction(transaction) = plan else {
            panic!("{command:?} must lower to a document transaction");
        };
        assert!(
            transaction
                .operations
                .iter()
                .all(|operation| !matches!(operation, TypedOperation::ReplaceStructure(_))),
            "{command:?} lowered to a whole-table replacement",
        );
        assert!(
            transaction
                .operations
                .iter()
                .all(|operation| matches!(operation, TypedOperation::EditStructure(_))),
            "{command:?} must lower to sealed structural edit batches only",
        );
    }
}

fn identity_fixture_json() -> String {
    json!({
        "type": "doc",
        "content": tall_span_fixture(),
    })
    .to_string()
}

pub(crate) fn session_cell_identities(session: &crate::session::EditorSession) -> Vec<String> {
    crate::tables::normalize_tests::cell_identities(
        &session.engine.encoded_state().expect("the state encodes"),
    )
}

pub(crate) fn session_cell_openings(session: &crate::session::EditorSession) -> Vec<u32> {
    session
        .engine
        .table_projection_index()
        .expect("the engine is ready")
        .table_at(TABLE_POSITION)
        .expect("the fixture holds a table")
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .collect()
}

pub(crate) fn session_select_cell(session: &mut crate::session::EditorSession, index: usize) {
    session_select_rectangle(session, index, index);
}

pub(crate) fn session_select_rectangle(
    session: &mut crate::session::EditorSession,
    anchor_index: usize,
    head_index: usize,
) {
    let openings = session_cell_openings(session);
    let anchor_opening = openings[anchor_index];
    let head_opening = openings[head_index];
    let document = session
        .engine
        .document()
        .expect("the engine is ready")
        .clone();
    let map = session.engine.position_map().expect("the engine is ready");
    let point = |opening: u32| RevisionedPosition {
        offset: map.doc_to_scalar(opening + CELL_TEXT_OFFSET, &document),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    let anchor = point(anchor_opening);
    let head = point(head_opening);
    let revision = session.engine.revision();
    session
        .engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: revision,
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Cell {
                anchor: anchor.into(),
                head: head.into(),
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the cell selection applies");
}

pub(crate) fn drain_document_updates(session: &mut crate::session::EditorSession) -> usize {
    let (_, outbox) = session.engine_and_outbox();
    let Some(outbox) = outbox else {
        return NO_DOCUMENT_UPDATES;
    };
    let mut documents = NO_DOCUMENT_UPDATES;
    while let Some(lease) = outbox.lease_next().expect("the outbox leases") {
        if matches!(
            lease.payload,
            crate::collaboration_runtime::outbox::OutboundLeasePayload::DocumentUpdate(_)
        ) {
            documents += 1;
        }
        outbox.ack_lease(lease.lease_id).expect("the lease acks");
    }
    documents
}

fn identity_session() -> crate::session::EditorSession {
    let mut session = seeded_session(identity_fixture_json());
    session.attach_collaboration_runtime();
    session
}

#[derive(Clone, Copy, Debug)]
enum IdentitySelection {
    Cell(usize),
    Rectangle(usize, usize),
}

struct IdentityExpectation {
    command: TableCommand,
    selection: IdentitySelection,
    surviving: &'static [usize],
    cells_after: usize,
}

const IDENTITY_EXPECTATIONS: [IdentityExpectation; 10] = [
    IdentityExpectation {
        command: TableCommand::AddTableRow {
            side: TableEdge::After,
        },
        selection: IdentitySelection::Cell(ANCHOR_CELL),
        surviving: &[0, 1, 2, 3, 4],
        cells_after: 7,
    },
    IdentityExpectation {
        command: TableCommand::AddTableColumn {
            side: TableEdge::After,
        },
        selection: IdentitySelection::Cell(ANCHOR_CELL),
        surviving: &[0, 1, 2, 3, 4],
        cells_after: 8,
    },
    IdentityExpectation {
        command: TableCommand::DeleteTableRows,
        selection: IdentitySelection::Cell(ANCHOR_CELL),
        surviving: &[0, 1, 3, 4],
        cells_after: 4,
    },
    IdentityExpectation {
        command: TableCommand::DeleteTableColumns,
        selection: IdentitySelection::Cell(ANCHOR_CELL),
        surviving: &[0, 3],
        cells_after: 2,
    },
    IdentityExpectation {
        command: TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Row,
        },
        selection: IdentitySelection::Cell(ANCHOR_CELL),
        surviving: &[1, 3, 4],
        cells_after: 5,
    },
    IdentityExpectation {
        command: TableCommand::ClearTableCells,
        selection: IdentitySelection::Cell(ANCHOR_CELL),
        surviving: &[0, 1, 2, 3, 4],
        cells_after: 5,
    },
    IdentityExpectation {
        command: TableCommand::MergeTableCells,
        selection: IdentitySelection::Rectangle(1, 4),
        surviving: &[0, 1, 3],
        cells_after: 3,
    },
    IdentityExpectation {
        command: TableCommand::MergeTableCells,
        selection: IdentitySelection::Rectangle(0, 2),
        surviving: &[0, 3, 4],
        cells_after: 3,
    },
    IdentityExpectation {
        command: TableCommand::SplitTableCell,
        selection: IdentitySelection::Cell(0),
        surviving: &[0, 1, 2, 3, 4],
        cells_after: 6,
    },
    IdentityExpectation {
        command: TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: None,
            table_pos: None,
        },
        selection: IdentitySelection::Cell(ANCHOR_CELL),
        surviving: &[0, 1, 2, 3, 4],
        cells_after: 5,
    },
];

#[test]
fn every_mutating_table_command_keeps_exactly_the_cell_identities_it_does_not_touch() {
    for expectation in IDENTITY_EXPECTATIONS {
        let command = expectation.command;
        let selection = expectation.selection;
        let mut session = identity_session();
        let before = session_cell_identities(&session);
        assert_eq!(before.len(), IDENTITY_FIXTURE_CELLS);
        match expectation.selection {
            IdentitySelection::Cell(index) => session_select_cell(&mut session, index),
            IdentitySelection::Rectangle(anchor, head) => {
                session_select_rectangle(&mut session, anchor, head)
            }
        }

        session
            .engine
            .apply_command(REQUEST_ID, TypedCommand::Table(command))
            .unwrap_or_else(|error| panic!("{command:?} over {selection:?} applies: {error:?}"))
            .unwrap_or_else(|| panic!("{command:?} over {selection:?} produced a transaction"));

        let after = session_cell_identities(&session);
        assert_eq!(
            after.len(),
            expectation.cells_after,
            "{command:?} over {selection:?} left an unexpected cell count",
        );
        for (index, identity) in before.iter().enumerate() {
            let kept = after.contains(identity);
            assert_eq!(
                kept,
                expectation.surviving.contains(&index),
                "{command:?} over {selection:?} disagrees about cell {index}: kept = {kept}",
            );
        }
    }
}

#[test]
fn a_table_command_emits_exactly_one_document_update_and_a_selection_none() {
    for (command, expected) in [
        (
            TableCommand::AddTableRow {
                side: TableEdge::After,
            },
            ONE_DOCUMENT_UPDATE,
        ),
        (
            TableCommand::AddTableColumn {
                side: TableEdge::After,
            },
            ONE_DOCUMENT_UPDATE,
        ),
        (TableCommand::DeleteTableRows, ONE_DOCUMENT_UPDATE),
        (TableCommand::DeleteTableColumns, ONE_DOCUMENT_UPDATE),
        (
            TableCommand::ToggleTableHeader {
                target: TableHeaderTarget::Row,
            },
            ONE_DOCUMENT_UPDATE,
        ),
        (TableCommand::ClearTableCells, ONE_DOCUMENT_UPDATE),
        (TableCommand::SelectTableRows, NO_DOCUMENT_UPDATES),
    ] {
        let mut session = identity_session();
        session_select_cell(&mut session, ANCHOR_CELL);
        assert_eq!(
            drain_document_updates(&mut session),
            NO_DOCUMENT_UPDATES,
            "the fixture and its selection leave nothing pending",
        );

        let (engine, outbox) = session.engine_and_outbox();
        engine
            .apply_command_with_outbox(REQUEST_ID, TypedCommand::Table(command), outbox)
            .unwrap_or_else(|error| panic!("{command:?} applies: {error:?}"));

        assert_eq!(
            drain_document_updates(&mut session),
            expected,
            "{command:?} must emit exactly {expected} native document update(s)",
        );
    }
}

#[test]
fn named_overlap_row_insertion_refuses_after_one_pass_without_publishing() {
    let mut session = seeded_session(
        json!({"type": "doc", "content": [table(vec![
            row(vec![cell("a"), cell_with(1, 2, Value::Null, "b")]),
            row(vec![cell_with(2, 3, Value::Null, "c")]), row(vec![]),
        ])]})
        .to_string(),
    );
    session_select_cell(&mut session, 0);
    assert_eq!(drain_document_updates(&mut session), 0);
    let before_document = session.engine.document_json().unwrap();
    let before_state = session.engine.encoded_state().unwrap();
    let before_revision = (session.engine.revision(), session.engine.state_revision());
    let before_history = (session.engine.can_undo(), session.engine.can_redo());
    crate::tables::normalize::reset_planned_normalization_passes();
    let (engine, outbox) = session.engine_and_outbox();
    assert!(engine
        .apply_command_with_outbox(
            REQUEST_ID,
            TypedCommand::Table(TableCommand::AddTableRow {
                side: TableEdge::After
            }),
            outbox
        )
        .unwrap()
        .is_none());
    assert_eq!(crate::tables::normalize::planned_normalization_passes(), 1);
    assert_eq!(session.engine.document_json().unwrap(), before_document);
    assert_eq!(session.engine.encoded_state().unwrap(), before_state);
    assert_eq!(
        (session.engine.revision(), session.engine.state_revision()),
        before_revision
    );
    assert_eq!(
        (session.engine.can_undo(), session.engine.can_redo()),
        before_history
    );
    assert_eq!(drain_document_updates(&mut session), 0);
}

#[test]
fn an_insertion_envelope_defaults_to_three_by_three_with_a_header_row() {
    let command = crate::native_transaction_bridge::table_command_envelope_for_test(
        &json!({ "type": "insertTable" }).to_string(),
    )
    .expect("a bare insertion envelope parses");

    assert_eq!(
        command,
        TypedCommand::Table(TableCommand::InsertTable {
            rows: DEFAULT_INSERTED_TABLE_ROWS,
            columns: DEFAULT_INSERTED_TABLE_COLUMNS,
            with_header_row: DEFAULT_INSERTED_TABLE_HEADER_ROW,
        }),
    );
}

#[test]
fn an_insertion_envelope_refuses_an_unbounded_dimension() {
    for dimension in [json!(0), json!(100_000)] {
        let refusal = crate::native_transaction_bridge::table_command_envelope_for_test(
            &json!({ "type": "insertTable", "rows": dimension }).to_string(),
        )
        .expect_err("an out-of-range dimension must not parse");

        assert!(
            refusal.contains("table dimension"),
            "the refusal must name the dimension bound, got {refusal}",
        );
    }
}

fn envelope_payload(command: TableCommand) -> Value {
    match command {
        TableCommand::InsertTable {
            rows,
            columns,
            with_header_row,
        } => json!({
            "type": "insertTable",
            "rows": rows,
            "columns": columns,
            "withHeaderRow": with_header_row,
        }),
        TableCommand::DeleteTable => json!({ "type": "deleteTable" }),
        TableCommand::AddTableRow { side } => {
            json!({ "type": "addTableRow", "side": edge_payload(side) })
        }
        TableCommand::DeleteTableRows => json!({ "type": "deleteTableRows" }),
        TableCommand::AddTableColumn { side } => {
            json!({ "type": "addTableColumn", "side": edge_payload(side) })
        }
        TableCommand::DeleteTableColumns => json!({ "type": "deleteTableColumns" }),
        TableCommand::ToggleTableHeader { target } => {
            json!({ "type": "toggleTableHeader", "target": header_payload(target) })
        }
        TableCommand::SelectTableRows => json!({ "type": "selectTableRows" }),
        TableCommand::SelectTableColumns => json!({ "type": "selectTableColumns" }),
        TableCommand::ClearTableCells => json!({ "type": "clearTableCells" }),
        TableCommand::MergeTableCells => json!({ "type": "mergeTableCells" }),
        TableCommand::SplitTableCell => json!({ "type": "splitTableCell" }),
        TableCommand::SetTableColumnWidth {
            width,
            column,
            table_pos,
        } => {
            let mut payload = json!({ "type": "setTableColumnWidth", "width": width });
            if let Some(column) = column {
                payload["column"] = json!(column);
            }
            if let Some(table_pos) = table_pos {
                payload["tablePos"] = json!(table_pos);
            }
            payload
        }
        TableCommand::MoveToAdjacentCell { step, append_row } => json!({
            "type": "moveToAdjacentCell",
            "step": step_payload(step),
            "appendRow": append_row,
        }),
    }
}

fn step_payload(step: crate::tables::interchange::CellStep) -> &'static str {
    match step {
        crate::tables::interchange::CellStep::Forward => "forward",
        crate::tables::interchange::CellStep::Backward => "backward",
    }
}

fn edge_payload(side: TableEdge) -> &'static str {
    match side {
        TableEdge::Before => "before",
        TableEdge::After => "after",
    }
}

fn header_payload(target: TableHeaderTarget) -> &'static str {
    match target {
        TableHeaderTarget::Row => "row",
        TableHeaderTarget::Column => "column",
        TableHeaderTarget::Cell => "cell",
    }
}

const EVERY_TABLE_EDGE: [TableEdge; 2] = [TableEdge::Before, TableEdge::After];
const EVERY_TABLE_HEADER_TARGET: [TableHeaderTarget; 3] = [
    TableHeaderTarget::Row,
    TableHeaderTarget::Column,
    TableHeaderTarget::Cell,
];
const EVERY_CELL_STEP: [crate::tables::interchange::CellStep; 2] = [
    crate::tables::interchange::CellStep::Forward,
    crate::tables::interchange::CellStep::Backward,
];
const EVERY_TAB_ROW_APPEND: [bool; 2] = [true, false];
const TABLE_COMMAND_ENVELOPE_CASES: usize = 21;

fn every_table_command() -> Vec<TableCommand> {
    let mut commands = vec![
        TableCommand::InsertTable {
            rows: DEFAULT_INSERTED_TABLE_ROWS,
            columns: DEFAULT_INSERTED_TABLE_COLUMNS,
            with_header_row: DEFAULT_INSERTED_TABLE_HEADER_ROW,
        },
        TableCommand::DeleteTable,
        TableCommand::DeleteTableRows,
        TableCommand::DeleteTableColumns,
        TableCommand::SelectTableRows,
        TableCommand::SelectTableColumns,
        TableCommand::ClearTableCells,
        TableCommand::MergeTableCells,
        TableCommand::SplitTableCell,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: None,
            table_pos: None,
        },
    ];
    for side in EVERY_TABLE_EDGE {
        commands.push(TableCommand::AddTableRow { side });
        commands.push(TableCommand::AddTableColumn { side });
    }
    for target in EVERY_TABLE_HEADER_TARGET {
        commands.push(TableCommand::ToggleTableHeader { target });
    }
    for step in EVERY_CELL_STEP {
        for append_row in EVERY_TAB_ROW_APPEND {
            commands.push(TableCommand::MoveToAdjacentCell { step, append_row });
        }
    }
    commands
}

#[test]
fn every_table_command_discriminant_round_trips_through_its_envelope() {
    let commands = every_table_command();
    assert_eq!(
        commands.len(),
        TABLE_COMMAND_ENVELOPE_CASES,
        "every edge and header target must be round tripped, not one instance per variant",
    );
    let payloads: std::collections::BTreeSet<String> = commands
        .iter()
        .map(|command| envelope_payload(*command).to_string())
        .collect();
    assert_eq!(
        payloads.len(),
        TABLE_COMMAND_ENVELOPE_CASES,
        "each case must address a distinct envelope",
    );
    for command in commands {
        let payload = envelope_payload(command);
        assert_eq!(
            crate::native_transaction_bridge::table_command_envelope_for_test(&payload.to_string()),
            Ok(TypedCommand::Table(command)),
            "{payload} must parse back to the command that produced it",
        );
    }
}

fn empty_cell() -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [{ "type": PARAGRAPH_NODE }],
    })
}

fn rich_cell(blocks: [&str; 2]) -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": blocks
            .iter()
            .map(|text| json!({
                "type": PARAGRAPH_NODE,
                "content": [{ "type": "text", "text": text }],
            }))
            .collect::<Vec<Value>>(),
    })
}

fn cell_blocks(table: &Value, row: usize, column: usize) -> Vec<String> {
    table["content"][row]["content"][column]["content"]
        .as_array()
        .expect("the cell holds blocks")
        .iter()
        .map(|block| {
            block["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

fn cell_attrs(table: &Value, row: usize, column: usize) -> Value {
    table["content"][row]["content"][column]["attrs"].clone()
}

fn cell_count(table: &Value, row: usize) -> usize {
    table["content"][row]["content"]
        .as_array()
        .map_or(NO_CELLS, Vec::len)
}

#[test]
fn merging_carries_every_source_block_into_the_top_left_cell() {
    let mut engine = seeded(vec![table(vec![
        row(vec![rich_cell(["a0", "a1"]), cell("b")]),
        row(vec![cell("c"), empty_cell()]),
    ])]);
    select_cells(&mut engine, 0, 3);

    applied(&mut engine, TableCommand::MergeTableCells);

    let table = table_of(&engine);
    assert_eq!(
        cell_count(&table, 0),
        1,
        "the rectangle collapses to one cell"
    );
    assert_eq!(cell_count(&table, 1), 0, "the consumed row keeps no cells");
    assert_eq!(
        cell_blocks(&table, 0, 0),
        vec!["a0", "a1", "b", "c"],
        "the surviving cell keeps every source block in document order",
    );
    assert_eq!(spans_at(&engine, 0, 0), (2, 2));
}

fn cell_with_block(block_type: &str, text: Option<&str>) -> Value {
    let content = match text {
        None => json!({ "type": block_type }),
        Some(text) => json!({
            "type": block_type,
            "content": [{ "type": "text", "text": text }],
        }),
    };
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [content],
    })
}

#[test]
fn merging_treats_any_empty_text_block_as_an_empty_source() {
    let mut engine = engine_with(
        tabled_schema_with_second_text_block(PROSEMIRROR_TABLE_NAMES),
        vec![table(vec![row(vec![
            cell_with_block(PARAGRAPH_NODE, Some("a")),
            cell_with_block(SECOND_TEXT_BLOCK_NODE, None),
        ])])],
    );
    select_cells(&mut engine, 0, 1);

    applied(&mut engine, TableCommand::MergeTableCells);

    let table = table_of(&engine);
    assert_eq!(
        cell_blocks(&table, 0, 0),
        vec!["a"],
        "an empty non paragraph text block contributes nothing to the merge",
    );
}

#[test]
fn merging_into_an_empty_survivor_replaces_its_placeholder_block() {
    let mut engine = seeded(vec![table(vec![row(vec![empty_cell(), cell("b")])])]);
    select_cells(&mut engine, 0, 1);

    applied(&mut engine, TableCommand::MergeTableCells);

    let table = table_of(&engine);
    assert_eq!(
        cell_blocks(&table, 0, 0),
        vec!["b"],
        "an empty survivor drops its placeholder instead of keeping a blank block",
    );
    assert_eq!(spans_at(&engine, 0, 0), (1, 2));
}

#[test]
fn merging_declines_when_the_selection_cuts_an_existing_span() {
    let mut engine = seeded(tall_span_fixture());
    select_cells(&mut engine, 2, 3);
    let before = table_of(&engine);

    assert_eq!(
        run(&mut engine, TableCommand::MergeTableCells),
        Ok(None),
        "a rectangle an existing span sticks out of is not mergeable",
    );
    assert_eq!(
        table_of(&engine),
        before,
        "a declined merge must leave the table exactly as it was",
    );
}

#[test]
fn merging_admits_a_selection_that_contains_a_whole_span() {
    let mut engine = seeded(tall_span_fixture());
    select_cells(&mut engine, 0, 2);

    applied(&mut engine, TableCommand::MergeTableCells);

    let table = table_of(&engine);
    assert_eq!(cell_blocks(&table, 0, 0), vec!["tall", "a1", "b1"]);
    assert_eq!(spans_at(&engine, 0, 0), (2, 2));
}

#[test]
fn merging_mixed_cell_types_keeps_the_surviving_cell_type() {
    let mut engine = seeded(header_fixture());
    select_cells(&mut engine, 0, 1);

    applied(&mut engine, TableCommand::MergeTableCells);

    let table = table_of(&engine);
    assert_eq!(row_types(&table, 0), vec![HEADER_CELL_NODE.to_owned()]);
    assert_eq!(cell_blocks(&table, 0, 0), vec!["h0", "h1"]);
}

#[test]
fn merging_declines_when_the_selection_covers_one_cell() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);

    assert_eq!(
        run(&mut engine, TableCommand::MergeTableCells),
        Ok(None),
        "a single cell has nothing to merge with",
    );
}

#[test]
fn splitting_a_horizontal_span_slices_its_widths_and_keeps_its_content() {
    let mut engine = seeded(wide_span_fixture());
    select_cell(&mut engine, 0);

    applied(&mut engine, TableCommand::SplitTableCell);

    let table = table_of(&engine);
    assert_eq!(row_texts(&table, 0), vec!["wide".to_owned(), String::new()]);
    assert_eq!(cell_attrs(&table, 0, 0)["colwidth"], json!([120]));
    assert_eq!(cell_attrs(&table, 0, 1)["colwidth"], json!([160]));
    assert_eq!(spans_at(&engine, 0, 0), (1, 1));
}

#[test]
fn splitting_a_vertical_span_mints_an_empty_cell_in_every_covered_row() {
    let mut engine = seeded(tall_span_fixture());
    select_cell(&mut engine, 0);

    applied(&mut engine, TableCommand::SplitTableCell);

    let table = table_of(&engine);
    assert_eq!(
        row_texts(&table, 0),
        vec!["tall".to_owned(), "a1".to_owned()]
    );
    assert_eq!(row_texts(&table, 1), vec![String::new(), "b1".to_owned()]);
    assert_eq!(spans_at(&engine, 0, 0), (1, 1));
}

#[test]
fn splitting_declines_on_a_cell_that_spans_nothing() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);

    assert_eq!(
        run(&mut engine, TableCommand::SplitTableCell),
        Ok(None),
        "an unspanned cell has nothing to split",
    );
}

fn resize_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell_with(2, SINGLE_SPAN, Value::Null, "wide")]),
        row(vec![cell("b0"), cell("b1")]),
    ])]
}

#[test]
fn resizing_writes_one_logical_column_across_every_covering_cell() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 1);

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: None,
            table_pos: None,
        },
    );

    let table = table_of(&engine);
    assert_eq!(
        cell_attrs(&table, 0, 0)["colwidth"],
        json!([PROBE_COLUMN_WIDTH, 0]),
        "a spanning cell keeps its per-span indexing and only writes its own slice",
    );
    assert_eq!(
        cell_attrs(&table, 1, 0)["colwidth"],
        json!([PROBE_COLUMN_WIDTH]),
    );
    assert_eq!(
        cell_attrs(&table, 1, 1)["colwidth"],
        Value::Null,
        "the untargeted column keeps its unset width",
    );
}

#[test]
fn resizing_targets_the_right_edge_of_the_selected_rectangle() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 2);

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: None,
            table_pos: None,
        },
    );

    let table = table_of(&engine);
    assert_eq!(
        cell_attrs(&table, 0, 0)["colwidth"],
        json!([0, PROBE_COLUMN_WIDTH]),
    );
    assert_eq!(cell_attrs(&table, 1, 0)["colwidth"], Value::Null);
    assert_eq!(
        cell_attrs(&table, 1, 1)["colwidth"],
        json!([PROBE_COLUMN_WIDTH]),
    );
}

#[test]
fn explicit_resize_targets_the_requested_column_and_preserves_the_text_caret() {
    let mut engine = seeded(resize_fixture());
    place_caret(&mut engine, 1);
    let before_selection = engine.resolved_selection().cloned();

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(SECOND_LOGICAL_COLUMN),
            table_pos: None,
        },
    );

    let table = table_of(&engine);
    assert_eq!(
        cell_attrs(&table, 0, 0)["colwidth"],
        json!([0, PROBE_COLUMN_WIDTH])
    );
    assert_eq!(cell_attrs(&table, 1, 0)["colwidth"], Value::Null);
    assert_eq!(
        cell_attrs(&table, 1, 1)["colwidth"],
        json!([PROBE_COLUMN_WIDTH])
    );
    assert_eq!(engine.resolved_selection().cloned(), before_selection);
}

#[test]
fn explicit_resize_preserves_a_cell_rectangle_while_targeting_another_column() {
    let mut engine = seeded(resize_fixture());
    select_cells(&mut engine, 1, 2);
    let before_selection = engine.resolved_selection().cloned();

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(0),
            table_pos: None,
        },
    );

    let table = table_of(&engine);
    assert_eq!(
        cell_attrs(&table, 0, 0)["colwidth"],
        json!([PROBE_COLUMN_WIDTH, 0])
    );
    assert_eq!(
        cell_attrs(&table, 1, 0)["colwidth"],
        json!([PROBE_COLUMN_WIDTH])
    );
    assert_eq!(cell_attrs(&table, 1, 1)["colwidth"], Value::Null);
    assert_eq!(engine.resolved_selection().cloned(), before_selection);
}

#[test]
fn explicit_logical_column_identity_is_independent_of_caret_side() {
    let mut left_caret = seeded(resize_fixture());
    let mut right_caret = seeded(resize_fixture());
    place_caret(&mut left_caret, 1);
    place_caret(&mut right_caret, 2);

    for engine in [&mut left_caret, &mut right_caret] {
        applied(
            engine,
            TableCommand::SetTableColumnWidth {
                width: PROBE_COLUMN_WIDTH,
                column: Some(SECOND_LOGICAL_COLUMN),
                table_pos: None,
            },
        );
    }

    assert_eq!(table_of(&left_caret), table_of(&right_caret));
}

#[test]
fn explicit_resize_of_the_same_width_keeps_the_original_selection_and_state() {
    let mut engine = seeded(resize_fixture());
    place_caret(&mut engine, 1);
    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(SECOND_LOGICAL_COLUMN),
            table_pos: None,
        },
    );
    let before_selection = engine.resolved_selection().cloned();
    let before_state = engine.encoded_state().unwrap();
    let before_revision = (engine.revision(), engine.state_revision());
    let before_history = (engine.can_undo(), engine.can_redo());

    assert_eq!(
        run(
            &mut engine,
            TableCommand::SetTableColumnWidth {
                width: PROBE_COLUMN_WIDTH,
                column: Some(SECOND_LOGICAL_COLUMN),
                table_pos: None,
            },
        ),
        Ok(None),
    );
    assert_eq!(engine.resolved_selection().cloned(), before_selection);
    assert_eq!(engine.encoded_state().unwrap(), before_state);
    assert_eq!(
        (engine.revision(), engine.state_revision()),
        before_revision
    );
    assert_eq!((engine.can_undo(), engine.can_redo()), before_history);
}

#[test]
fn explicit_resize_refuses_an_out_of_bounds_column_without_mutation() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 1);
    let before_document = engine.document_json();
    let before_state = engine.encoded_state().unwrap();
    let before_revision = (engine.revision(), engine.state_revision());
    let before_history = (engine.can_undo(), engine.can_redo());

    assert_eq!(
        run(
            &mut engine,
            TableCommand::SetTableColumnWidth {
                width: PROBE_COLUMN_WIDTH,
                column: Some(OUTSIDE_RESIZE_FIXTURE),
                table_pos: None,
            },
        ),
        Ok(None),
    );
    assert_eq!(engine.document_json(), before_document);
    assert_eq!(engine.encoded_state().unwrap(), before_state);
    assert_eq!(
        (engine.revision(), engine.state_revision()),
        before_revision
    );
    assert_eq!((engine.can_undo(), engine.can_redo()), before_history);
}

#[test]
fn explicit_resize_updates_a_newly_filled_synthetic_target_column() {
    let mut engine = seeded(short_first_row_fixture());
    select_cell(&mut engine, 1);
    assert!(projection_of(&engine).irregular);

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(SECOND_LOGICAL_COLUMN),
            table_pos: None,
        },
    );

    let table = table_of(&engine);
    assert_eq!(cell_attrs(&table, 0, 0)["colwidth"], Value::Null);
    assert_eq!(
        cell_attrs(&table, 0, 1)["colwidth"],
        json!([PROBE_COLUMN_WIDTH])
    );
    assert_eq!(cell_attrs(&table, 1, 0)["colwidth"], Value::Null);
    assert_eq!(
        cell_attrs(&table, 1, 1)["colwidth"],
        json!([PROBE_COLUMN_WIDTH])
    );
}

#[test]
fn explicit_resize_accepts_gap_fill_when_source_slices_keep_their_identity() {
    let mut engine = seeded(short_first_row_fixture());
    select_cell(&mut engine, 1);
    assert!(projection_of(&engine).irregular);

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(0),
            table_pos: None,
        },
    );

    let table = table_of(&engine);
    assert_eq!(
        cell_attrs(&table, 0, 0)["colwidth"],
        json!([PROBE_COLUMN_WIDTH])
    );
    assert_eq!(
        cell_attrs(&table, 1, 0)["colwidth"],
        json!([PROBE_COLUMN_WIDTH])
    );
    assert_eq!(cell_attrs(&table, 1, 1)["colwidth"], Value::Null);
    let openings = cell_openings(&engine);
    assert_eq!(resolved_cells(&engine), Some((openings[2], openings[2])));
}

#[test]
fn explicit_resize_accepts_width_only_normalization_with_stable_source_cells() {
    let mut engine = seeded(vec![table(vec![
        row(vec![cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([100]), "a")]),
        row(vec![cell_with(
            SINGLE_SPAN,
            SINGLE_SPAN,
            json!([PROBE_COLUMN_WIDTH]),
            "b",
        )]),
    ])]);
    select_cell(&mut engine, 0);

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: UPDATED_COLUMN_WIDTH,
            column: Some(0),
            table_pos: None,
        },
    );

    let table = table_of(&engine);
    assert_eq!(
        cell_attrs(&table, 0, 0)["colwidth"],
        json!([UPDATED_COLUMN_WIDTH])
    );
    assert_eq!(
        cell_attrs(&table, 1, 0)["colwidth"],
        json!([UPDATED_COLUMN_WIDTH])
    );
}

#[test]
fn explicit_resize_refuses_a_colliding_fallback_column_without_mutation() {
    let mut engine = seeded(vec![table(vec![
        row(vec![cell("a"), cell_with(1, 2, Value::Null, "b")]),
        row(vec![cell_with(2, 3, Value::Null, "c")]),
        row(vec![]),
    ])]);
    select_cell(&mut engine, 0);
    assert!(projection_of(&engine).irregular);
    assert!(projection_of(&engine).columns > OUTSIDE_RESIZE_FIXTURE);
    let before_document = engine.document_json();
    let before_state = engine.encoded_state().unwrap();
    let before_revision = (engine.revision(), engine.state_revision());
    let before_history = (engine.can_undo(), engine.can_redo());

    assert_eq!(
        run(
            &mut engine,
            TableCommand::SetTableColumnWidth {
                width: PROBE_COLUMN_WIDTH,
                column: Some(OUTSIDE_RESIZE_FIXTURE),
                table_pos: None,
            },
        ),
        Ok(None),
    );
    assert_eq!(engine.document_json(), before_document);
    assert_eq!(engine.encoded_state().unwrap(), before_state);
    assert_eq!(
        (engine.revision(), engine.state_revision()),
        before_revision
    );
    assert_eq!((engine.can_undo(), engine.can_redo()), before_history);
}

#[test]
fn explicit_resize_is_one_undoable_action_across_covering_cells() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 1);
    assert!(!engine.can_undo());

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(SECOND_LOGICAL_COLUMN),
            table_pos: None,
        },
    );
    assert!(engine.can_undo());
    engine
        .undo(REQUEST_ID + 1)
        .unwrap()
        .expect("one undo applies");
    let undone = table_of(&engine);
    assert_eq!(cell_attrs(&undone, 0, 0)["colwidth"], Value::Null);
    assert_eq!(cell_attrs(&undone, 1, 1)["colwidth"], Value::Null);
    assert!(
        !engine.can_undo(),
        "one history entry contains all width slices"
    );
    engine
        .redo(REQUEST_ID + 2)
        .unwrap()
        .expect("one redo applies");
    let redone = table_of(&engine);
    assert_eq!(cell_attrs(&redone, 0, 0)["colwidth"][0].as_f64(), Some(0.0));
    assert_eq!(
        cell_attrs(&redone, 0, 0)["colwidth"][1].as_f64(),
        Some(PROBE_COLUMN_WIDTH as f64),
    );
    assert_eq!(
        cell_attrs(&redone, 1, 1)["colwidth"][0].as_f64(),
        Some(PROBE_COLUMN_WIDTH as f64),
    );
    assert!(!engine.can_redo());
}

fn prose_then_resize_fixture() -> Vec<Value> {
    let mut content = vec![json!({
        "type": PARAGRAPH_NODE,
        "content": [{ "type": "text", "text": PROSE_PREFIX_TEXT }],
    })];
    content.extend(resize_fixture());
    content
}

fn second_block_table(engine: &YrsDocumentEngine) -> Value {
    engine.document_json().expect("the engine is ready")["content"][1].clone()
}

fn caret_in_prose_prefix(engine: &mut YrsDocumentEngine) {
    let map = engine.position_map().expect("the engine is ready");
    let point = RevisionedPosition {
        offset: map.doc_to_scalar(PROSE_PREFIX_CARET, document_of(engine)),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point,
                head: point,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the prose caret applies");
}

#[test]
fn explicit_table_resize_leaves_a_prose_caret_alone_and_undo_restores_it() {
    let mut engine = seeded(prose_then_resize_fixture());
    caret_in_prose_prefix(&mut engine);
    let table_pos = crate::tables::normalize::outer_table_positions(
        document_of(&engine),
        &engine_schema(&engine),
        &limits(),
    )
    .expect("outer tables resolve")[0];
    assert_eq!(table_pos, PROSE_PREFIX_TABLE_POSITION);
    let before_selection = engine.resolved_selection().cloned();
    assert!(matches!(
        before_selection,
        Some(crate::yrs_engine::ResolvedSelection::Text { .. })
    ));
    assert!(!engine.can_undo());

    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(SECOND_LOGICAL_COLUMN),
            table_pos: Some(table_pos),
        },
    );

    let table = second_block_table(&engine);
    assert_eq!(
        cell_attrs(&table, 0, 0)["colwidth"],
        json!([0, PROBE_COLUMN_WIDTH])
    );
    assert_eq!(
        cell_attrs(&table, 1, 1)["colwidth"],
        json!([PROBE_COLUMN_WIDTH])
    );
    assert_eq!(
        engine.resolved_selection().cloned(),
        before_selection,
        "an explicit table target never moves the caret out of the prose",
    );
    assert!(engine.can_undo());
    engine
        .undo(REQUEST_ID + 1)
        .unwrap()
        .expect("one undo applies");
    let undone = second_block_table(&engine);
    assert_eq!(cell_attrs(&undone, 0, 0)["colwidth"], Value::Null);
    assert_eq!(cell_attrs(&undone, 1, 1)["colwidth"], Value::Null);
    assert_eq!(
        engine.resolved_selection().cloned(),
        before_selection,
        "undo restores the prose caret, not a parked table selection",
    );
    assert!(!engine.can_undo());
}

#[test]
fn explicit_table_resize_declines_a_position_without_an_outer_table() {
    let mut engine = seeded(prose_then_resize_fixture());
    caret_in_prose_prefix(&mut engine);
    let revision = engine.revision();

    for table_pos in [
        PROSE_PREFIX_CARET,
        PROSE_PREFIX_TABLE_POSITION + ONE_CHARACTER,
    ] {
        assert_eq!(
            run(
                &mut engine,
                TableCommand::SetTableColumnWidth {
                    width: PROBE_COLUMN_WIDTH,
                    column: Some(SECOND_LOGICAL_COLUMN),
                    table_pos: Some(table_pos),
                },
            ),
            Ok(None),
            "position {table_pos} is not an outer table opening",
        );
    }
    assert_eq!(engine.revision(), revision);
    assert_eq!(
        cell_attrs(&second_block_table(&engine), 1, 1)["colwidth"],
        Value::Null
    );
    assert!(!engine.can_undo());
}

#[test]
fn an_explicit_table_position_requires_an_explicit_column() {
    let mut engine = seeded(prose_then_resize_fixture());
    caret_in_prose_prefix(&mut engine);
    let revision = engine.revision();

    let error = run(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: None,
            table_pos: Some(PROSE_PREFIX_TABLE_POSITION),
        },
    )
    .expect_err("a table position without a column is a malformed request");
    assert_eq!(error.code, OPERATION_INVALID_CODE);
    assert!(
        error.message.contains("explicit column"),
        "the refusal must name the missing column, got {}",
        error.message
    );
    assert_eq!(engine.revision(), revision);
    assert!(!engine.can_undo());
}

#[test]
fn resizing_declines_when_every_covering_cell_already_carries_the_width() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 1);
    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: None,
            table_pos: None,
        },
    );

    assert_eq!(
        run(
            &mut engine,
            TableCommand::SetTableColumnWidth {
                width: PROBE_COLUMN_WIDTH,
                column: None,
                table_pos: None,
            },
        ),
        Ok(None),
        "an unchanged width must not mint a transaction",
    );
}

#[test]
fn the_published_surface_makes_no_width_free_resize_claim() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 1);
    let openings = cell_openings(&engine);
    let selection = Selection::cell(openings[1], openings[1]);

    let commands = crate::editor_state::command_applicability(
        document_of(&engine),
        &engine_schema(&engine),
        &selection,
        &limits(),
    );

    assert_eq!(
        commands.get("setTableColumnWidth"),
        None,
        "a resize targets an explicit table, column and width, so the selection cannot vouch for it",
    );
}

#[test]
fn a_column_width_envelope_refuses_an_unbounded_width() {
    for width in [json!(0), json!(100_000)] {
        let refusal = crate::native_transaction_bridge::table_command_envelope_for_test(
            &json!({ "type": "setTableColumnWidth", "width": width }).to_string(),
        )
        .expect_err("an out-of-range width must not parse");

        assert!(
            refusal.contains("table column width"),
            "the refusal must name the width bound, got {refusal}",
        );
    }
}

#[test]
fn a_column_width_envelope_accepts_an_explicit_logical_column() {
    let command = crate::native_transaction_bridge::table_command_envelope_for_test(
        &json!({
            "type": "setTableColumnWidth",
            "width": PROBE_COLUMN_WIDTH,
            "column": 1,
        })
        .to_string(),
    )
    .expect("an explicit logical column must parse");

    assert!(matches!(
        command,
        TypedCommand::Table(TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(SECOND_LOGICAL_COLUMN),
            table_pos: None,
        })
    ));
}

#[test]
fn a_column_width_envelope_accepts_an_explicit_table_position() {
    let command = crate::native_transaction_bridge::table_command_envelope_for_test(
        &json!({
            "type": "setTableColumnWidth",
            "width": PROBE_COLUMN_WIDTH,
            "column": SECOND_LOGICAL_COLUMN,
            "tablePos": PROSE_PREFIX_TABLE_POSITION,
        })
        .to_string(),
    )
    .expect("an explicit table position must parse");

    assert!(matches!(
        command,
        TypedCommand::Table(TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: Some(SECOND_LOGICAL_COLUMN),
            table_pos: Some(PROSE_PREFIX_TABLE_POSITION),
        })
    ));

    for table_pos in [
        json!(null),
        json!(-1),
        json!(1.5),
        json!("8"),
        json!(4294967296u64),
    ] {
        let refusal = crate::native_transaction_bridge::table_command_envelope_for_test(
            &json!({
                "type": "setTableColumnWidth",
                "width": PROBE_COLUMN_WIDTH,
                "column": SECOND_LOGICAL_COLUMN,
                "tablePos": table_pos,
            })
            .to_string(),
        );
        assert!(
            refusal.is_err(),
            "malformed table position {table_pos} must not parse"
        );
    }
}

#[test]
fn a_column_width_envelope_rejects_malformed_explicit_columns() {
    for column in [
        json!(null),
        json!(-1),
        json!(1.5),
        json!("1"),
        json!(4294967296u64),
    ] {
        let refusal = crate::native_transaction_bridge::table_command_envelope_for_test(
            &json!({
                "type": "setTableColumnWidth",
                "width": PROBE_COLUMN_WIDTH,
                "column": column,
            })
            .to_string(),
        );
        assert!(refusal.is_err(), "malformed column {column} must not parse");
    }

    let omitted = crate::native_transaction_bridge::table_command_envelope_for_test(
        &json!({ "type": "setTableColumnWidth", "width": PROBE_COLUMN_WIDTH }).to_string(),
    )
    .expect("omitting the optional column retains the legacy command");
    assert!(matches!(
        omitted,
        TypedCommand::Table(TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
            column: None,
            table_pos: None,
        })
    ));
}

fn short_first_row_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell("a0")]),
        row(vec![cell("b0"), cell("b1")]),
    ])]
}

#[test]
fn a_header_toggle_maps_its_selection_through_the_normalization_it_triggers() {
    let mut engine = seeded(short_first_row_fixture());
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, true));
    select_cell(&mut engine, 1);

    applied(
        &mut engine,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Cell,
        },
    );

    let table = table_of(&engine);
    assert_eq!(row_types(&table, 0), vec![CELL_NODE.to_owned(); 2]);
    assert_eq!(
        row_types(&table, 1),
        vec![HEADER_CELL_NODE.to_owned(), CELL_NODE.to_owned()],
        "the toggle must land on the cell the caret named after the gap was filled",
    );
    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_cells(&engine),
        Some((openings[2], openings[2])),
        "the surviving selection must name the toggled cell in post normalization positions",
    );
}

pub(crate) fn place_caret(engine: &mut YrsDocumentEngine, index: usize) {
    let point = after_first_character(engine, index);
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point,
                head: point,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the caret applies");
}

fn resolved_caret(engine: &YrsDocumentEngine) -> Option<(u32, u32)> {
    match engine.resolved_selection()? {
        crate::yrs_engine::ResolvedSelection::Text { anchor, head } => {
            Some((anchor.document, head.document))
        }
        crate::yrs_engine::ResolvedSelection::Cell { .. }
        | crate::yrs_engine::ResolvedSelection::Node { .. }
        | crate::yrs_engine::ResolvedSelection::All => None,
    }
}

#[test]
fn a_header_toggle_keeps_a_text_caret_and_maps_it_through_normalization() {
    let mut engine = seeded(short_first_row_fixture());
    place_caret(&mut engine, 1);

    applied(
        &mut engine,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Cell,
        },
    );

    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_caret(&engine),
        Some((
            openings[2] + CELL_TEXT_OFFSET + ONE_CHARACTER,
            openings[2] + CELL_TEXT_OFFSET + ONE_CHARACTER,
        )),
        "a non destructive toggle must leave the caret where it was, in mapped positions",
    );
}

#[test]
fn a_malformed_column_width_is_refused_before_any_table_action_sees_it() {
    for malformed in [json!(["abc"]), json!("140"), json!([-1])] {
        let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
            schema: schema(),
            fragment_name: FRAGMENT_NAME.into(),
            initialization_mode: InitializationMode::LocalEmpty,
            resource_limits: limits(),
            editing_limits: EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .expect("the tabled engine initializes");
        let error = engine
            .import_json(
                &json!({ "type": "doc", "content": vec![table(vec![row(vec![
                    cell_with(SINGLE_SPAN, SINGLE_SPAN, malformed.clone(), "a"),
                ])])] })
                .to_string(),
                TransactionOrigin::DocumentImport,
            )
            .expect_err("a malformed column width must not be admitted");
        assert_eq!(
            error.code, DOCUMENT_INVALID_CODE,
            "{malformed} must be refused as an invalid document, not admitted: {error:?}",
        );
    }
}

#[test]
fn a_table_shape_failure_reaches_the_host_as_its_own_error_class() {
    for (failure, code) in [
        (
            crate::tables::types::TableError::InvalidAttributes,
            DOCUMENT_INVALID_CODE,
        ),
        (
            crate::tables::types::TableError::InvalidStructure,
            DOCUMENT_INVALID_CODE,
        ),
        (
            crate::tables::types::TableError::GridLimit {
                limit: 1,
                actual: 2,
            },
            DOCUMENT_LIMIT_EXCEEDED_CODE,
        ),
        (
            crate::tables::types::TableError::WorkLimit,
            OPERATION_WORK_BUDGET_CODE,
        ),
        (
            crate::tables::types::TableError::Allocation,
            OPERATION_WORK_BUDGET_CODE,
        ),
    ] {
        let error = crate::tables::command_context::table_shape_operation_error(
            failure.clone(),
            REQUEST_ID,
        );
        assert_eq!(
            error.code, code,
            "{failure:?} must keep its own error class instead of collapsing into unavailability",
        );
        assert!(
            !crate::tables::command_context::is_action_unavailable(&error),
            "{failure:?} must never look like a table action that simply does not apply",
        );
    }
}
