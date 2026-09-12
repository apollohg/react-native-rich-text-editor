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
const IDENTITY_FIXTURE_CELLS: usize = 5;
const ONE_CHARACTER: u32 = 1;
const NO_CELLS: usize = 0;
const PROBE_COLUMN_WIDTH: u32 = 140;
const SHARED_SURFACE_PROJECTIONS: u64 = 1;
const STAGED_DELETION_PROJECTIONS: u64 = 4;

fn engine_with(schema: Schema, content: Vec<Value>) -> YrsDocumentEngine {
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

fn seeded(content: Vec<Value>) -> YrsDocumentEngine {
    engine_with(schema(), content)
}

fn document_of(engine: &YrsDocumentEngine) -> &Document {
    engine.document().expect("the engine is ready")
}

fn table_of(engine: &YrsDocumentEngine) -> Value {
    engine.document_json().expect("the engine is ready")["content"][0].clone()
}

fn engine_schema(_engine: &YrsDocumentEngine) -> Schema {
    schema()
}

fn projection_of(engine: &YrsDocumentEngine) -> ProjectedTable {
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

fn cell_openings(engine: &YrsDocumentEngine) -> Vec<u32> {
    projection_of(engine)
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .collect()
}

fn inside_cell(engine: &YrsDocumentEngine, index: usize) -> RevisionedPosition {
    let opening = cell_openings(engine)[index];
    let map = engine.position_map().expect("the engine is ready");
    RevisionedPosition {
        offset: map.doc_to_scalar(opening + CELL_TEXT_OFFSET, document_of(engine)),
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

fn select_cells(engine: &mut YrsDocumentEngine, anchor: usize, head: usize) {
    let anchor = inside_cell(engine, anchor);
    let head = inside_cell(engine, head);
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Cell { anchor, head }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the cell selection applies");
}

fn select_cell(engine: &mut YrsDocumentEngine, index: usize) {
    select_cells(engine, index, index);
}

fn resolved_cells(engine: &YrsDocumentEngine) -> Option<(u32, u32)> {
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

fn row_types(table: &Value, row: usize) -> Vec<String> {
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

fn row_texts(table: &Value, row: usize) -> Vec<String> {
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

fn row_count(table: &Value) -> usize {
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

fn geometry(projected: &ProjectedTable) -> (u32, u32, bool) {
    (projected.rows, projected.columns, projected.irregular)
}

fn regular_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell("a0"), cell("a1")]),
        row(vec![cell("b0"), cell("b1")]),
    ])]
}

fn tall_span_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![
            cell_with(SINGLE_SPAN, 2, Value::Null, "tall"),
            cell("a1"),
        ]),
        row(vec![cell("b1")]),
        row(vec![cell("c0"), cell("c1")]),
    ])]
}

fn wide_span_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell_with(2, SINGLE_SPAN, json!([120, 160]), "wide")]),
        row(vec![cell("b0"), cell("b1")]),
    ])]
}

fn header_fixture() -> Vec<Value> {
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
    ]
}

#[test]
fn availability_matches_the_planner_for_every_table_command() {
    for (name, fixture, anchor) in availability_fixtures() {
        for command in every_table_command() {
            assert_eq!(
                advertised(fixture.clone(), anchor, command),
                planner_accepts(fixture.clone(), anchor, command),
                "on the {name} fixture at cell {anchor}, {command:?} must advertise \
                 exactly what the planner will do",
            );
        }
    }
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
    let resize = |width| TableCommand::SetTableColumnWidth { width };

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
            selection_intent: SelectionIntent::Set(SelectionInput::Cell { anchor, head }),
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
        TableCommand::SetTableColumnWidth { width } => {
            json!({ "type": "setTableColumnWidth", "width": width })
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
fn resizing_declines_when_every_covering_cell_already_carries_the_width() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 1);
    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: PROBE_COLUMN_WIDTH,
        },
    );

    assert_eq!(
        run(
            &mut engine,
            TableCommand::SetTableColumnWidth {
                width: PROBE_COLUMN_WIDTH,
            },
        ),
        Ok(None),
        "an unchanged width must not mint a transaction",
    );
}

#[test]
fn resizing_stays_available_on_a_column_that_already_carries_the_requested_width() {
    let mut engine = seeded(resize_fixture());
    select_cell(&mut engine, 1);
    applied(
        &mut engine,
        TableCommand::SetTableColumnWidth {
            width: MIN_TABLE_COLUMN_WIDTH,
        },
    );
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
        Some(&true),
        "a resizable column stays advertised even when the advertised width is a no-op",
    );
    assert_eq!(
        run(
            &mut engine,
            TableCommand::SetTableColumnWidth {
                width: MIN_TABLE_COLUMN_WIDTH,
            },
        ),
        Ok(None),
        "the planner still declines the width the column already carries",
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

fn place_caret(engine: &mut YrsDocumentEngine, index: usize) {
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
