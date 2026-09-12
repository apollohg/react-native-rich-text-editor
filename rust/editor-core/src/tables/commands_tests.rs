use serde_json::{json, Value};

use crate::model::Document;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::commands::{
    TableCommand, TableEdge, TableHeaderTarget, DEFAULT_INSERTED_TABLE_COLUMNS,
    DEFAULT_INSERTED_TABLE_HEADER_ROW, DEFAULT_INSERTED_TABLE_ROWS,
};
use crate::tables::normalize_tests::{
    cell, cell_with, header_cell, limits, row, schema, seeded_session, table,
};
use crate::tables::projection::{project_table, ProjectedTable, TableGridBudget};
use crate::tables::tests::tabled_schema;
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
fn clearing_cells_leaves_a_synthetic_gap_synthetic() {
    let mut engine = seeded(ragged_fixture());
    let before = table_of(&engine);
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, true));
    select_cell(&mut engine, 0);

    let outcome = run(&mut engine, TableCommand::ClearTableCells)
        .expect("clearing declines on an irregular grid rather than erroring");

    assert!(outcome.is_none());
    assert_eq!(
        table_of(&engine),
        before,
        "clearing must never write the cell a synthetic slot stands in for",
    );
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, true));
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
fn availability_reports_no_proof_on_an_irregular_grid_and_normalizes_nothing() {
    let engine = seeded(ragged_fixture());
    let document = document_of(&engine).clone();
    let schema = engine_schema(&engine);
    let openings = cell_openings(&engine);
    let selection = Selection::cell(openings[0], openings[0]);
    crate::tables::normalize::reset_planned_normalization_passes();

    let commands =
        crate::editor_state::command_applicability(&document, &schema, &selection, &limits());

    assert_eq!(commands.get("addTableRowAfter"), Some(&false));
    assert_eq!(commands.get("clearTableCells"), Some(&false));
    assert_eq!(
        crate::tables::normalize::planned_normalization_passes(),
        0,
        "querying availability must plan no normalization of its own",
    );
    assert_eq!(document, *document_of(&engine));
}

#[test]
fn availability_matches_the_planner_on_a_regular_grid() {
    let mut engine = seeded(regular_fixture());
    select_cell(&mut engine, 0);
    let openings = cell_openings(&engine);
    let selection = Selection::cell(openings[0], openings[0]);

    let commands = crate::editor_state::command_applicability(
        document_of(&engine),
        &engine_schema(&engine),
        &selection,
        &limits(),
    );

    for name in [
        "addTableRowBefore",
        "addTableRowAfter",
        "deleteTableRows",
        "addTableColumnBefore",
        "addTableColumnAfter",
        "deleteTableColumns",
        "toggleTableHeaderRow",
        "toggleTableHeaderColumn",
        "toggleTableHeaderCell",
        "deleteTable",
        "clearTableCells",
    ] {
        assert_eq!(commands.get(name), Some(&true), "{name} must be available");
    }
    assert_eq!(
        commands.get("insertTable"),
        Some(&false),
        "a table cannot be inserted inside a table",
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

fn session_cell_identities(session: &crate::session::EditorSession) -> Vec<String> {
    crate::tables::normalize_tests::cell_identities(
        &session.engine.encoded_state().expect("the state encodes"),
    )
}

fn session_cell_openings(session: &crate::session::EditorSession) -> Vec<u32> {
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

fn session_select_cell(session: &mut crate::session::EditorSession, index: usize) {
    let opening = session_cell_openings(session)[index];
    let document = session
        .engine
        .document()
        .expect("the engine is ready")
        .clone();
    let map = session.engine.position_map().expect("the engine is ready");
    let point = RevisionedPosition {
        offset: map.doc_to_scalar(opening + CELL_TEXT_OFFSET, &document),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    let revision = session.engine.revision();
    session
        .engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: revision,
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Cell {
                anchor: point,
                head: point,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the cell selection applies");
}

fn drain_document_updates(session: &mut crate::session::EditorSession) -> usize {
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

#[test]
fn every_table_command_keeps_the_cell_identities_it_does_not_touch() {
    for (command, untouched) in [
        (
            TableCommand::AddTableRow {
                side: TableEdge::After,
            },
            vec!["tall", "a1", "b1", "c0", "c1"],
        ),
        (
            TableCommand::AddTableColumn {
                side: TableEdge::After,
            },
            vec!["tall", "a1", "b1", "c0", "c1"],
        ),
        (
            TableCommand::ClearTableCells,
            vec!["tall", "a1", "b1", "c0", "c1"],
        ),
    ] {
        let mut session = identity_session();
        let before = session_cell_identities(&session);
        assert_eq!(before.len(), untouched.len());
        session_select_cell(&mut session, 2);

        let request_id = REQUEST_ID;
        session
            .engine
            .apply_command(request_id, TypedCommand::Table(command))
            .unwrap_or_else(|error| panic!("{command:?} applies: {error:?}"))
            .unwrap_or_else(|| panic!("{command:?} produced a transaction"));

        let after = session_cell_identities(&session);
        for identity in &before {
            assert!(
                after.contains(identity),
                "{command:?} replaced the cell with identity {identity}",
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
        session_select_cell(&mut session, 2);
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
        assert!(
            crate::native_transaction_bridge::table_command_envelope_for_test(
                &json!({ "type": "insertTable", "rows": dimension }).to_string(),
            )
            .is_none(),
            "an out-of-range dimension {dimension} must not parse",
        );
    }
}

#[test]
fn every_table_command_discriminant_has_an_envelope() {
    for (payload, expected) in [
        (json!({ "type": "deleteTable" }), TableCommand::DeleteTable),
        (
            json!({ "type": "addTableRow", "side": "before" }),
            TableCommand::AddTableRow {
                side: TableEdge::Before,
            },
        ),
        (
            json!({ "type": "addTableRow", "side": "after" }),
            TableCommand::AddTableRow {
                side: TableEdge::After,
            },
        ),
        (
            json!({ "type": "deleteTableRows" }),
            TableCommand::DeleteTableRows,
        ),
        (
            json!({ "type": "addTableColumn", "side": "before" }),
            TableCommand::AddTableColumn {
                side: TableEdge::Before,
            },
        ),
        (
            json!({ "type": "deleteTableColumns" }),
            TableCommand::DeleteTableColumns,
        ),
        (
            json!({ "type": "toggleTableHeader", "target": "row" }),
            TableCommand::ToggleTableHeader {
                target: TableHeaderTarget::Row,
            },
        ),
        (
            json!({ "type": "toggleTableHeader", "target": "column" }),
            TableCommand::ToggleTableHeader {
                target: TableHeaderTarget::Column,
            },
        ),
        (
            json!({ "type": "toggleTableHeader", "target": "cell" }),
            TableCommand::ToggleTableHeader {
                target: TableHeaderTarget::Cell,
            },
        ),
        (
            json!({ "type": "selectTableRows" }),
            TableCommand::SelectTableRows,
        ),
        (
            json!({ "type": "selectTableColumns" }),
            TableCommand::SelectTableColumns,
        ),
        (
            json!({ "type": "clearTableCells" }),
            TableCommand::ClearTableCells,
        ),
    ] {
        assert_eq!(
            crate::native_transaction_bridge::table_command_envelope_for_test(&payload.to_string()),
            Some(TypedCommand::Table(expected)),
            "{payload} must parse",
        );
    }
}
