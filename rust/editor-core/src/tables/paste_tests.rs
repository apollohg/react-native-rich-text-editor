use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::command_planner::SemanticOperation;
use crate::model::Fragment;
use crate::native_transaction_bridge::{
    NativeBridgeOutcome, NativeTransactionBridge, NATIVE_BRIDGE_ENVELOPE_VERSION,
};
use crate::schema::presets::prosemirror_table_schema;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::command_context::{prepare_table_action, CellAnchorPair, TableActionContext};
use crate::tables::commands::paste::MatrixPasteAction;
use crate::tables::commands_tests::{
    cell_openings, document_of, drain_document_updates, engine_schema, engine_with, geometry,
    header_fixture, place_caret, projection_of, regular_fixture, resolved_cells, row_count,
    row_texts, row_types, seeded, select_cells, session_cell_openings, session_select_rectangle,
    table_of, tall_span_fixture, wide_span_fixture, PROSE_PREFIX_TABLE_POSITION, PROSE_PREFIX_TEXT,
};
use crate::tables::interchange_tests::nesting_cell;
use crate::tables::normalize_tests::{
    cell, cell_identities, cell_with, document_with, header_cell, limits, merged_fixture_table,
    row, schema, seeded_session, table,
};
use crate::tables::paste::{matrix_from_slice, tab_separated_fields, TableMatrix};
use crate::tables::tests::{tabled_schema_json, PROSEMIRROR_TABLE_NAMES};
use crate::tables::TableRoles;
use crate::yrs_engine::{
    structural_edit_batch_for_test, table_action_plan_for_test, Affinity, CommandPlan,
    EditingLimits, EditorOffsetKind, HistoryPolicy, OperationError, ResolvedSelection,
    RevisionedPosition, SelectionInput, SelectionIntent, TableActionTestRequest, TransactionOrigin,
    TypedCommand, TypedOperation, TypedTransaction, TypedTransactionResult, YrsDocumentEngine,
};

const REQUEST_ID: u64 = 43;
const TABLE_POSITION: u32 = 0;
const CELL_TEXT_OFFSET: u32 = 2;
const ONE_CHARACTER: u32 = 1;
const SINGLE_SPAN: u32 = 1;
const DOUBLE_SPAN: u32 = 2;
const CELL_NODE: &str = "table_cell";
const HEADER_CELL_NODE: &str = "table_header";
const PARAGRAPH_NODE: &str = "paragraph";
const MENTION_NODE: &str = "mention";
const OPERATION_INVALID_CODE: &str = "OPERATION_INVALID";
const DOCUMENT_LIMIT_EXCEEDED_CODE: &str = "DOCUMENT_LIMIT_EXCEEDED";
const ONE_DOCUMENT_UPDATE: usize = 1;
const OVERSIZED_MATRIX_SIDE: usize = 65;
const PROSE_CARET: u32 = 3;
const TALL_SOURCE_ROWS: u32 = 4;
const SELECTED_ROWS: u32 = 3;

fn pasted(
    engine: &mut YrsDocumentEngine,
    html: Option<&str>,
    text: Option<&str>,
) -> Result<Option<TypedTransactionResult>, OperationError> {
    engine.apply_command(
        REQUEST_ID,
        TypedCommand::Paste {
            fragment: None,
            html: html.map(str::to_owned),
            text: text.map(str::to_owned),
            plain_text: false,
            allow_base64_images: false,
            input_filter: None,
        },
    )
}

fn paste_text(engine: &mut YrsDocumentEngine, text: &str) {
    let applied = pasted(engine, None, Some(text))
        .unwrap_or_else(|error| panic!("pasting {text:?} plans: {error:?}"));
    assert!(
        applied.is_some(),
        "pasting {text:?} produced no transaction"
    );
}

fn paste_html(engine: &mut YrsDocumentEngine, html: &str) {
    let applied = pasted(engine, Some(html), None)
        .unwrap_or_else(|error| panic!("pasting {html} plans: {error:?}"));
    assert!(applied.is_some(), "pasting {html} produced no transaction");
}

fn declared_colspan(cell: &Value) -> u32 {
    cell["attrs"]["colspan"]
        .as_u64()
        .map_or(SINGLE_SPAN, |span| {
            u32::try_from(span).expect("a span fits u32")
        })
}

fn texts(table: &Value) -> Vec<Vec<String>> {
    (0..row_count(table))
        .map(|index| row_texts(table, index))
        .collect()
}

fn three_by_three() -> Vec<Value> {
    vec![table(vec![
        row(vec![cell("a0"), cell("a1"), cell("a2")]),
        row(vec![cell("b0"), cell("b1"), cell("b2")]),
        row(vec![cell("c0"), cell("c1"), cell("c2")]),
    ])]
}

fn table_roles() -> TableRoles {
    TableRoles::resolve(&schema())
        .expect("the test schema's table roles resolve")
        .expect("the test schema declares table roles")
}

fn matrix_of(rows: Vec<Value>) -> TableMatrix {
    let source = document_with(vec![table(rows)]);
    let content = source
        .root()
        .content()
        .expect("the source document holds content");
    matrix_from_slice(content, 0, 0, &table_roles(), &schema(), &limits())
        .expect("the source rows are readable")
        .expect("the source table is a cell matrix")
}

fn legacy_selection(engine: &YrsDocumentEngine) -> Selection {
    match engine.resolved_selection().expect("the engine is ready") {
        ResolvedSelection::Text { anchor, head } => Selection::text(anchor.document, head.document),
        ResolvedSelection::Cell { anchor, head } => Selection::cell(anchor.document, head.document),
        ResolvedSelection::Node { at } => Selection::node(at.document),
        ResolvedSelection::All => Selection::all(),
    }
}

#[derive(Clone, Copy, Debug)]
enum Destination {
    Caret(usize),
    Rectangle(usize, usize),
}

fn destinations(cells: usize) -> Vec<Destination> {
    let mut destinations: Vec<Destination> = (0..cells).map(Destination::Caret).collect();
    destinations.push(Destination::Rectangle(0, cells - 1));
    destinations.extend((1..cells).map(|head| Destination::Rectangle(head - 1, head)));
    destinations
}

fn prose_prefixed(fixture: Vec<Value>) -> Vec<Value> {
    let mut content = vec![json!({
        "type": PARAGRAPH_NODE,
        "content": [{ "type": "text", "text": PROSE_PREFIX_TEXT }],
    })];
    content.extend(fixture);
    content
}

fn header_column_fixture() -> Vec<Value> {
    vec![table(vec![
        row(vec![header_cell("h0"), cell("a1"), cell("a2")]),
        row(vec![header_cell("h1"), cell("b1"), cell("b2")]),
    ])]
}

fn generated_fixtures() -> Vec<(&'static str, u32, Vec<Value>)> {
    vec![
        ("regular", TABLE_POSITION, regular_fixture()),
        ("tall span", TABLE_POSITION, tall_span_fixture()),
        ("wide span", TABLE_POSITION, wide_span_fixture()),
        ("header row", TABLE_POSITION, header_fixture()),
        ("header column", TABLE_POSITION, header_column_fixture()),
        ("merged", TABLE_POSITION, vec![merged_fixture_table()]),
        (
            "prose then tall span",
            PROSE_PREFIX_TABLE_POSITION,
            prose_prefixed(tall_span_fixture()),
        ),
        (
            "prose then merged",
            PROSE_PREFIX_TABLE_POSITION,
            prose_prefixed(vec![merged_fixture_table()]),
        ),
    ]
}

fn lone_merged_source() -> Vec<Value> {
    vec![
        row(vec![cell_with(DOUBLE_SPAN, DOUBLE_SPAN, Value::Null, "m")]),
        row(Vec::new()),
    ]
}

fn generated_matrices() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        ("1x1", vec![row(vec![cell("x")])]),
        ("1x2", vec![row(vec![cell("x"), cell("y")])]),
        ("2x1", vec![row(vec![cell("x")]), row(vec![cell("y")])]),
        (
            "3x3",
            vec![
                row(vec![cell("p"), cell("q"), cell("r")]),
                row(vec![cell("s"), cell("t"), cell("u")]),
                row(vec![cell("v"), cell("w"), cell("z")]),
            ],
        ),
        (
            "source colspan",
            vec![
                row(vec![cell_with(DOUBLE_SPAN, SINGLE_SPAN, Value::Null, "w")]),
                row(vec![cell("p"), cell("q")]),
            ],
        ),
        (
            "source rowspan",
            vec![
                row(vec![
                    cell_with(SINGLE_SPAN, DOUBLE_SPAN, Value::Null, "t"),
                    cell("u"),
                ]),
                row(vec![cell("v")]),
            ],
        ),
        (
            "header source",
            vec![row(vec![header_cell("h"), header_cell("i")])],
        ),
    ]
}

fn select(engine: &mut YrsDocumentEngine, destination: Destination) -> CellAnchorPair {
    let openings = cell_openings(engine);
    match destination {
        Destination::Caret(index) => {
            place_caret(engine, index);
            CellAnchorPair {
                anchor: openings[index],
                head: openings[index],
            }
        }
        Destination::Rectangle(anchor, head) => {
            select_cells(engine, anchor, head);
            CellAnchorPair {
                anchor: openings[anchor],
                head: openings[head],
            }
        }
    }
}

#[test]
fn every_generated_matrix_paste_plan_absorbs_into_one_sealed_batch() {
    let schema = schema();
    let limits = limits();
    let editing_limits = EditingLimits::default();
    let mut checked = 0usize;
    for (fixture_name, table_pos, fixture) in generated_fixtures() {
        let cells = cell_openings(&seeded(fixture.clone())).len();
        for destination in destinations(cells) {
            for (matrix_name, matrix_rows) in generated_matrices() {
                let label = format!("{fixture_name} / {destination:?} / {matrix_name}");
                let mut engine = seeded(fixture.clone());
                let anchors = select(&mut engine, destination);
                let selection = legacy_selection(&engine);
                let document = document_of(&engine).clone();
                let prepared = prepare_table_action(
                    &TableActionContext {
                        request_id: REQUEST_ID,
                        base_document_revision: engine.revision(),
                        table_pos,
                        anchors: Some(anchors),
                        schema: &schema,
                        resource_limits: &limits,
                        editing_limits: &editing_limits,
                        document: &document,
                        selection: &selection,
                    },
                    &MatrixPasteAction {
                        matrix: matrix_of(matrix_rows),
                    },
                )
                .unwrap_or_else(|error| panic!("{label}: the paste prepares: {error:?}"));
                let operations = prepared.plan.plan.operations.clone();
                let batch = structural_edit_batch_for_test(
                    REQUEST_ID,
                    &document,
                    &schema,
                    &operations,
                    &prepared.plan.simulated.selection,
                )
                .unwrap_or_else(|error| panic!("{label}: absorbing errored: {error:?}"));
                assert!(
                    batch.is_some(),
                    "{label}: the emitted vector does not absorb into a sealed batch: {operations:#?}",
                );
                let simulated = TableProjectionIndex::derive_or_fallback(
                    &prepared.plan.simulated.document,
                    &schema,
                    &limits,
                );
                assert!(
                    !simulated
                        .table_at(table_pos)
                        .unwrap_or_else(|| panic!("{label}: the pasted table survives"))
                        .irregular,
                    "{label}: the pasted table is irregular: {:?}",
                    prepared.plan.simulated.document,
                );
                let plan = table_action_plan_for_test(
                    TableActionTestRequest {
                        document: &document,
                        schema: &schema,
                        resource_limits: &limits,
                        editing_limits: &editing_limits,
                        revision: engine.revision(),
                        state_revision: engine.state_revision(),
                        yrs_state_epoch: engine.yrs_state_epoch(),
                        origin: TransactionOrigin::LocalCommand,
                    },
                    prepared,
                )
                .unwrap_or_else(|error| panic!("{label}: the paste lowers: {error:?}"));
                let CommandPlan::Transaction(transaction) = plan else {
                    panic!("{label}: the paste lowers to a document transaction");
                };
                assert!(
                    matches!(
                        transaction.operations.as_slice(),
                        [TypedOperation::EditStructure(_)]
                    ),
                    "{label}: the paste must lower to exactly one sealed batch, got {:?}",
                    transaction.operations,
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "the generator produced no plans");
}

#[test]
fn the_absorption_check_refuses_a_row_replacement_that_crosses_cell_boundaries() {
    let engine = seeded(regular_fixture());
    let document = document_of(&engine).clone();
    let openings = cell_openings(&engine);
    let crossing = vec![SemanticOperation::ReplaceRange {
        from: openings[0] + CELL_TEXT_OFFSET + ONE_CHARACTER,
        to: openings[1] + CELL_TEXT_OFFSET + ONE_CHARACTER,
        content: Fragment::empty(),
    }];

    let batch = structural_edit_batch_for_test(
        REQUEST_ID,
        &document,
        &engine_schema(&engine),
        &crossing,
        &Selection::cursor(openings[0] + CELL_TEXT_OFFSET),
    )
    .expect("the crossing range resolves");

    assert!(
        batch.is_none(),
        "a range with endpoints inside two different cells must not seal: {batch:?}",
    );
}

#[test]
fn a_cell_selection_repeats_a_smaller_matrix_cyclically_without_growing() {
    let mut engine = seeded(three_by_three());
    select_cells(&mut engine, 0, 8);

    paste_text(&mut engine, "1\t2\n3\t4");

    let table = table_of(&engine);
    assert_eq!(
        texts(&table),
        vec![
            vec!["1", "2", "1"],
            vec!["3", "4", "3"],
            vec!["1", "2", "1"],
        ],
        "the 2x2 source must tile the 3x3 selection: {table}",
    );
    assert_eq!(geometry(&projection_of(&engine)), (3, 3, false));
    let openings = cell_openings(&engine);
    assert_eq!(resolved_cells(&engine), Some((openings[0], openings[8])));
}

#[test]
fn a_cell_selection_truncates_a_larger_matrix_to_its_rectangle() {
    let mut engine = seeded(three_by_three());
    select_cells(&mut engine, 4, 5);

    paste_text(&mut engine, "p\tq\tr\ns\tt\tu");

    let table = table_of(&engine);
    assert_eq!(
        texts(&table),
        vec![
            vec!["a0", "a1", "a2"],
            vec!["b0", "p", "q"],
            vec!["c0", "c1", "c2"],
        ],
        "only the selected 1x2 rectangle may change: {table}",
    );
    assert_eq!(geometry(&projection_of(&engine)), (3, 3, false));
}

#[test]
fn a_caret_grows_the_table_to_hold_the_whole_matrix_with_reference_header_roles() {
    let mut engine = seeded(header_fixture());
    place_caret(&mut engine, 3);

    paste_text(&mut engine, "x\ty\nz\tw");

    let table = table_of(&engine);
    assert_eq!(
        texts(&table),
        vec![
            vec!["h0", "h1", ""],
            vec!["b0", "x", "y"],
            vec!["", "z", "w"]
        ],
        "the matrix lands at the caret cell and the table grows around it: {table}",
    );
    assert_eq!(row_types(&table, 0), vec![HEADER_CELL_NODE; 3]);
    assert_eq!(row_types(&table, 1), vec![CELL_NODE; 3]);
    assert_eq!(row_types(&table, 2), vec![CELL_NODE; 3]);
    assert_eq!(geometry(&projection_of(&engine)), (3, 3, false));
    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_cells(&engine),
        Some((openings[4], openings[8])),
        "the pasted rectangle becomes the cell selection",
    );
}

#[test]
fn a_ragged_tab_separated_matrix_writes_empty_cells_into_its_short_rows() {
    let mut engine = seeded(regular_fixture());
    place_caret(&mut engine, 0);

    paste_text(&mut engine, "a\tb\r\nc\r\n");

    let table = table_of(&engine);
    assert_eq!(
        texts(&table),
        vec![vec!["a", "b"], vec!["c", ""]],
        "a short row is padded, so the destination cell beside it is emptied: {table}",
    );
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, false));
}

#[test]
fn a_caret_in_a_tall_cell_splits_its_overhang_and_keeps_cell_identities() {
    let mut engine = seeded(tall_span_fixture());
    let before = cell_identities(&engine.encoded_state().expect("the state encodes"));
    place_caret(&mut engine, 0);

    paste_text(&mut engine, "x\ty");

    let table = table_of(&engine);
    assert_eq!(
        texts(&table),
        vec![vec!["x", "y"], vec!["", "b1"], vec!["c0", "c1"]],
        "the tall cell is cut at the matrix's bottom edge: {table}",
    );
    assert_eq!(geometry(&projection_of(&engine)), (3, 2, false));
    let after = cell_identities(&engine.encoded_state().expect("the state encodes"));
    assert_eq!(
        after.len(),
        before.len() + 1,
        "one remainder cell is minted"
    );
    assert_eq!(after[0], before[0], "the tall cell keeps its identity");
    assert_eq!(
        after[1], before[1],
        "the written neighbour keeps its identity"
    );
    assert_eq!(
        after[3..],
        before[2..],
        "untouched cells keep their identities"
    );
}

#[test]
fn a_destination_span_the_source_does_not_share_is_replaced_by_the_source_cells() {
    let mut engine = seeded(wide_span_fixture());
    place_caret(&mut engine, 0);

    paste_text(&mut engine, "p\tq");

    let table = table_of(&engine);
    assert_eq!(texts(&table), vec![vec!["p", "q"], vec!["b0", "b1"]]);
    for column in 0..2 {
        assert_eq!(
            declared_colspan(&table["content"][0]["content"][column]),
            SINGLE_SPAN,
            "the wide destination is replaced by unit source cells: {table}",
        );
    }
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, false));
}

#[test]
fn a_pasted_html_table_brings_its_spans_and_header_roles() {
    let mut engine = engine_with(prosemirror_table_schema(), regular_fixture());
    place_caret(&mut engine, 0);

    paste_html(
        &mut engine,
        "<table><tr><th colspan=\"2\">w</th></tr><tr><td>1</td><td>2</td></tr></table>",
    );

    let table = table_of(&engine);
    assert_eq!(texts(&table), vec![vec!["w"], vec!["1", "2"]], "{table}");
    assert_eq!(row_types(&table, 0), vec![HEADER_CELL_NODE]);
    assert_eq!(
        declared_colspan(&table["content"][0]["content"][0]),
        DOUBLE_SPAN,
        "{table}"
    );
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, false));
}

#[test]
fn a_left_edge_cut_splits_the_straddling_cell_and_its_column_widths() {
    let mut engine = engine_with(
        prosemirror_table_schema(),
        vec![table(vec![
            row(vec![cell("c00"), cell("c01"), cell("c02")]),
            row(vec![
                cell_with(DOUBLE_SPAN, SINGLE_SPAN, json!([100, 200]), "W"),
                cell("c12"),
            ]),
        ])],
    );
    place_caret(&mut engine, 1);

    paste_html(
        &mut engine,
        "<table><tr><td>p</td></tr><tr><td>q</td></tr></table>",
    );

    let table = table_of(&engine);
    assert_eq!(
        texts(&table),
        vec![vec!["c00", "p", "c02"], vec!["W", "q", "c12"]],
        "{table}",
    );
    let lower = &table["content"][1]["content"];
    assert_eq!(declared_colspan(&lower[0]), SINGLE_SPAN, "{table}");
    assert_eq!(
        lower[0]["attrs"]["colwidth"],
        json!([100]),
        "the straddling cell keeps the width of the column it still covers: {table}",
    );
    assert_eq!(declared_colspan(&lower[1]), SINGLE_SPAN, "{table}");
    assert_eq!(
        lower[1]["attrs"]["colwidth"],
        Value::Null,
        "the written cell takes the pasted cell's attributes, not the split-off width: {table}",
    );
    assert_eq!(geometry(&projection_of(&engine)), (2, 3, false));
}

#[test]
fn pasting_inside_a_nested_table_is_refused_without_mutation() {
    let mut engine = seeded(vec![table(vec![row(vec![nesting_cell(), cell("x")])])]);
    let nested_cell = {
        let index = engine
            .table_projection_index()
            .expect("the engine is ready");
        let nested = index
            .positions()
            .find(|position| *position != TABLE_POSITION)
            .expect("the fixture holds a nested table");
        index
            .table_at(nested)
            .expect("the nested table projects")
            .cells[0]
            .source_pos
    };
    let caret = {
        let map = engine.position_map().expect("the engine is ready");
        RevisionedPosition {
            offset: map.doc_to_scalar(
                nested_cell + CELL_TEXT_OFFSET + ONE_CHARACTER,
                document_of(&engine),
            ),
            kind: EditorOffsetKind::Scalar,
            affinity: Affinity::Before,
        }
    };
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
        .expect("the caret lands inside the nested cell");
    let before = engine.document_json().expect("the engine is ready");
    let revision = engine.revision();

    let error = pasted(&mut engine, None, Some("a\tb"))
        .expect_err("a nested table's cells never accept a paste");

    assert_eq!(error.code, OPERATION_INVALID_CODE, "{error:?}");
    assert_eq!(engine.document_json().expect("the engine is ready"), before);
    assert_eq!(engine.revision(), revision);
}

fn atom_schema() -> Schema {
    let mut definition = tabled_schema_json(PROSEMIRROR_TABLE_NAMES);
    definition["nodes"]
        .as_array_mut()
        .expect("the tabled schema lists nodes")
        .push(json!({
            "name": MENTION_NODE,
            "role": "inline",
            "group": "inline",
            "isVoid": true,
            "attrs": { "id": {}, "label": { "default": "" } },
            "allowUndeclaredAttrs": true,
        }));
    Schema::from_json(&definition).expect("the atom schema is valid")
}

fn atom_cell() -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [{
            "type": PARAGRAPH_NODE,
            "content": [
                { "type": "text", "text": "a" },
                {
                    "type": MENTION_NODE,
                    "attrs": { "id": "m1", "label": "Sam", "metadata": { "kind": "person" } },
                },
            ],
        }],
    })
}

#[test]
fn copied_atom_cells_paste_with_their_payload_intact() {
    let schema = atom_schema();
    let mut engine = engine_with(
        schema.clone(),
        vec![table(vec![
            row(vec![atom_cell(), cell("b")]),
            row(vec![cell("c"), cell("d")]),
        ])],
    );
    let openings = cell_openings(&engine);
    let document = document_of(&engine).clone();
    let index = TableProjectionIndex::derive_or_fallback(&document, &schema, &limits());
    let copied = crate::clipboard::export_cells(
        &document,
        &Selection::cell(openings[0], openings[1]),
        &index,
        &schema,
    )
    .expect("the atom row copies");
    place_caret(&mut engine, 2);

    let applied = engine
        .apply_command(
            REQUEST_ID,
            TypedCommand::Paste {
                fragment: copied["fragment"].as_str().map(str::to_owned),
                html: copied["html"].as_str().map(str::to_owned),
                text: copied["text"].as_str().map(str::to_owned),
                plain_text: false,
                allow_base64_images: false,
                input_filter: None,
            },
        )
        .expect("the copied row pastes");

    assert!(applied.is_some());
    let table = table_of(&engine);
    assert_eq!(
        table["content"][1]["content"], table["content"][0]["content"],
        "the pasted row must reproduce the copied atom and its undeclared payload: {table}",
    );
}

#[test]
fn tab_separated_text_parses_quotes_line_breaks_and_ragged_rows() {
    let cases: [(&str, Option<Vec<Vec<&str>>>); 14] = [
        ("a\tb", Some(vec![vec!["a", "b"]])),
        ("a\tb\n", Some(vec![vec!["a", "b"]])),
        (
            "a\tb\r\nc\td\r\n",
            Some(vec![vec!["a", "b"], vec!["c", "d"]]),
        ),
        ("a\tb\rc\td", Some(vec![vec!["a", "b"], vec!["c", "d"]])),
        ("\"x\ty\"\tz", Some(vec![vec!["x\ty", "z"]])),
        ("\"one\r\ntwo\"\tz", Some(vec![vec!["one\ntwo", "z"]])),
        ("\"say \"\"hi\"\"\"\tz", Some(vec![vec!["say \"hi\"", "z"]])),
        ("a\"b\tc", Some(vec![vec!["a\"b", "c"]])),
        ("\"open\tz", Some(vec![vec!["\"open", "z"]])),
        ("\"x\"y\tz", Some(vec![vec!["\"x\"y", "z"]])),
        ("a\t\tb", Some(vec![vec!["a", "", "b"]])),
        ("a\tb\nc", Some(vec![vec!["a", "b"], vec!["c"]])),
        ("\t", Some(vec![vec!["", ""]])),
        ("no tabs\nhere", None),
    ];
    for (input, expected) in cases {
        let expected = expected.map(|rows| {
            rows.into_iter()
                .map(|fields| fields.into_iter().map(str::to_owned).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        });
        assert_eq!(tab_separated_fields(input), expected, "parsing {input:?}");
    }
}

#[test]
fn plain_text_without_tabs_edits_the_active_cell() {
    let mut engine = seeded(regular_fixture());
    place_caret(&mut engine, 0);

    paste_text(&mut engine, "zz");

    assert_eq!(
        texts(&table_of(&engine)),
        vec![vec!["azz0", "a1"], vec!["b0", "b1"]],
    );
}

#[test]
fn plain_text_without_tabs_fills_every_selected_cell() {
    let mut engine = seeded(regular_fixture());
    select_cells(&mut engine, 0, 3);

    paste_text(&mut engine, "k");

    assert_eq!(
        texts(&table_of(&engine)),
        vec![vec!["k", "k"], vec!["k", "k"]],
    );
    assert_eq!(geometry(&projection_of(&engine)), (2, 2, false));
}

#[test]
fn tab_separated_text_outside_a_table_keeps_the_plain_text_paste() {
    let mut engine = seeded(vec![
        json!({ "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "before" }] }),
        table(vec![row(vec![cell("a0")])]),
    ]);
    let before_table = engine.document_json().expect("the engine is ready")["content"][1].clone();
    let caret = {
        let map = engine.position_map().expect("the engine is ready");
        RevisionedPosition {
            offset: map.doc_to_scalar(PROSE_CARET, document_of(&engine)),
            kind: EditorOffsetKind::Scalar,
            affinity: Affinity::Before,
        }
    };
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
        .expect("the caret lands in the paragraph");

    paste_text(&mut engine, "a\tb");

    let document = engine.document_json().expect("the engine is ready");
    assert_eq!(
        document["content"][0]["content"][0]["text"],
        json!("bea\tbfore"),
        "prose receives the text literally: {document}",
    );
    assert_eq!(document["content"][1], before_table);
}

#[test]
fn a_matrix_that_would_exceed_the_grid_budget_is_refused_without_mutation() {
    let mut engine = seeded(regular_fixture());
    place_caret(&mut engine, 0);
    let before = engine.document_json().expect("the engine is ready");
    let revision = engine.revision();
    let line = vec!["x"; OVERSIZED_MATRIX_SIDE].join("\t");
    let oversized = vec![line; OVERSIZED_MATRIX_SIDE].join("\n");
    assert!(
        OVERSIZED_MATRIX_SIDE * OVERSIZED_MATRIX_SIDE > limits().max_table_grid_slots,
        "the fixture must exceed the configured grid budget",
    );

    let error =
        pasted(&mut engine, None, Some(&oversized)).expect_err("an over-budget matrix is refused");

    assert_eq!(error.code, DOCUMENT_LIMIT_EXCEEDED_CODE, "{error:?}");
    assert_eq!(engine.document_json().expect("the engine is ready"), before);
    assert_eq!(engine.revision(), revision);
}

fn bridge_paste(session: &mut crate::session::EditorSession, text: &str) -> NativeBridgeOutcome {
    let envelope = json!({
        "version": NATIVE_BRIDGE_ENVELOPE_VERSION,
        "requestId": REQUEST_ID.to_string(),
        "baseDocumentRevision": session.engine.revision().to_string(),
        "command": { "type": "paste", "text": text },
    })
    .to_string();
    NativeTransactionBridge::new(session)
        .submit_command(&envelope)
        .unwrap_or_else(|error| panic!("the bridged paste applies: {error:?}"))
}

#[test]
fn a_bridged_matrix_paste_is_one_update_and_one_undoable_history_entry() {
    let mut session =
        seeded_session(json!({ "type": "doc", "content": regular_fixture() }).to_string());
    session.attach_collaboration_runtime();
    let before = session.engine.document_json().expect("the engine is ready");
    let openings = session_cell_openings(&session);
    assert_eq!(openings.len(), 4);
    session_select_rectangle(&mut session, 0, 3);
    drain_document_updates(&mut session);

    let outcome = bridge_paste(&mut session, "1\t2\n3\t4\n5\t6");

    assert!(
        matches!(outcome, NativeBridgeOutcome::Transaction(_)),
        "{outcome:?}",
    );
    assert_eq!(
        drain_document_updates(&mut session),
        ONE_DOCUMENT_UPDATE,
        "the matrix paste must publish exactly one document update",
    );
    let after = session.engine.document_json().expect("the engine is ready");
    assert_eq!(
        texts(&after["content"][0]),
        vec![vec!["1", "2"], vec!["3", "4"]],
        "a cell selection clips the taller matrix: {after}",
    );

    let mut bridge = NativeTransactionBridge::new(&mut session);
    assert!(bridge.undo(REQUEST_ID).expect("the undo applies"));
    assert_eq!(
        session.engine.document_json().expect("the engine is ready"),
        before,
        "one undo restores the pre-paste table",
    );
    let mut bridge = NativeTransactionBridge::new(&mut session);
    assert!(
        !bridge
            .undo(REQUEST_ID)
            .expect("the second undo is answered"),
        "the paste must be a single history entry",
    );
    let mut bridge = NativeTransactionBridge::new(&mut session);
    assert!(bridge.redo(REQUEST_ID).expect("the redo applies"));
    assert_eq!(
        session.engine.document_json().expect("the engine is ready"),
        after,
    );
}

#[test]
fn a_clipped_rowspan_is_cut_at_the_selection_bottom_not_by_its_own_span() {
    let mut engine = seeded(three_by_three());
    select_cells(&mut engine, 0, 6);
    let schema = engine_schema(&engine);
    let source = crate::serialize::from_prosemirror_json(
        &json!({
            "type": "doc",
            "content": [table(vec![
                row(vec![cell_with(SINGLE_SPAN, TALL_SOURCE_ROWS, Value::Null, "t"), cell("a")]),
                row(vec![cell("b")]),
                row(vec![cell("c")]),
                row(vec![cell("d")]),
            ])],
        }),
        &schema,
        crate::serialize::UnknownTypeMode::Error,
    )
    .expect("the tall source parses");
    let index =
        TableProjectionIndex::derive_or_fallback(&source, &schema, &ResourceLimits::default());
    let tall = index
        .table_at(TABLE_POSITION)
        .expect("the source projects")
        .cells[0]
        .source_pos;
    let copied =
        crate::clipboard::export_cells(&source, &Selection::cell(tall, tall), &index, &schema)
            .expect("the tall cell copies");

    engine
        .apply_command(
            REQUEST_ID,
            TypedCommand::Paste {
                fragment: copied["fragment"].as_str().map(str::to_owned),
                html: None,
                text: None,
                plain_text: false,
                allow_base64_images: false,
                input_filter: None,
            },
        )
        .expect("the tall source pastes")
        .expect("the tall source produced a transaction");

    let table = table_of(&engine);
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["rowspan"],
        json!(SELECTED_ROWS),
        "a 4-row cell clipped into a 3-row selection spans all 3 rows, where the pinned \
         reference would write max(1, 3 - 4) = 1: {table}",
    );
    assert_eq!(
        texts(&table),
        vec![vec!["t", "a1", "a2"], vec!["b1", "b2"], vec!["c1", "c2"]],
    );
    assert_eq!(geometry(&projection_of(&engine)), (3, 3, false));
}

#[test]
fn pasting_the_cells_already_there_rewrites_nothing() {
    let mut engine = seeded(regular_fixture());
    select_cells(&mut engine, 0, 3);
    let before = engine.encoded_state().expect("the state encodes");
    let revision = engine.revision();

    paste_text(&mut engine, "a0\ta1\nb0\tb1");

    assert_eq!(
        engine.revision(),
        revision,
        "an identical matrix is not a document change"
    );
    assert_eq!(engine.encoded_state().expect("the state encodes"), before);
}

fn pasted_plain_text(engine: &mut YrsDocumentEngine, html: Option<&str>, text: &str) {
    engine
        .apply_command(
            REQUEST_ID,
            TypedCommand::Paste {
                fragment: None,
                html: html.map(str::to_owned),
                text: Some(text.to_owned()),
                plain_text: true,
                allow_base64_images: false,
                input_filter: None,
            },
        )
        .unwrap_or_else(|error| panic!("pasting {text:?} as plain text plans: {error:?}"))
        .unwrap_or_else(|| panic!("pasting {text:?} as plain text produced no transaction"));
}

#[test]
fn a_plain_text_paste_into_a_cell_selection_ignores_the_rich_table() {
    let mut engine = engine_with(prosemirror_table_schema(), three_by_three());
    select_cells(&mut engine, 0, 4);

    pasted_plain_text(
        &mut engine,
        Some("<table><tr><td>rich</td></tr></table>"),
        "p\tq",
    );

    assert_eq!(
        texts(&table_of(&engine)),
        vec![
            vec!["p", "q", "a2"],
            vec!["p", "q", "b2"],
            vec!["c0", "c1", "c2"],
        ],
        "plain-text mode must tile the text matrix, not the rich table",
    );
}

#[test]
fn a_lone_merged_source_tiles_its_covered_row_instead_of_doing_nothing() {
    let mut engine = seeded(three_by_three());
    select_cells(&mut engine, 0, 8);
    let schema = engine_schema(&engine);
    let source = crate::serialize::from_prosemirror_json(
        &json!({ "type": "doc", "content": [table(lone_merged_source())] }),
        &schema,
        crate::serialize::UnknownTypeMode::Error,
    )
    .expect("the merged source parses");
    let index =
        TableProjectionIndex::derive_or_fallback(&source, &schema, &ResourceLimits::default());
    let merged = index
        .table_at(TABLE_POSITION)
        .expect("the source projects")
        .cells[0]
        .source_pos;
    let copied =
        crate::clipboard::export_cells(&source, &Selection::cell(merged, merged), &index, &schema)
            .expect("the merged cell copies");
    let revision = engine.revision();

    engine
        .apply_command(
            REQUEST_ID,
            TypedCommand::Paste {
                fragment: copied["fragment"].as_str().map(str::to_owned),
                html: None,
                text: None,
                plain_text: false,
                allow_base64_images: false,
                input_filter: None,
            },
        )
        .expect("the merged source pastes")
        .expect("the merged source produced a transaction");

    assert_ne!(
        engine.revision(),
        revision,
        "the paste must change the table"
    );
    let table = table_of(&engine);
    assert_eq!(row_texts(&table, 0), vec!["m", "m"], "{table}");
    assert!(
        table["content"][1]["content"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "the middle row stays fully covered by the repeated spans: {table}",
    );
    assert_eq!(
        row_texts(&table, 2),
        vec!["m", "m"],
        "the 2x2 cell repeats down the selection and is cut at its bottom: {table}",
    );
    assert_eq!(
        declared_colspan(&table["content"][0]["content"][0]),
        DOUBLE_SPAN
    );
    assert_eq!(
        declared_colspan(&table["content"][0]["content"][1]),
        SINGLE_SPAN
    );
    assert_eq!(
        table["content"][0]["content"][0]["attrs"]["rowspan"],
        json!(DOUBLE_SPAN)
    );
    assert_eq!(geometry(&projection_of(&engine)), (3, 3, false));
}

fn multi_paragraph_cell() -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [
            { "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "one" }] },
            { "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "say \"two\"" }] },
        ],
    })
}

#[test]
fn copied_cells_round_trip_through_their_tab_separated_text() {
    let source = seeded(vec![table(vec![
        row(vec![
            cell_with(DOUBLE_SPAN, SINGLE_SPAN, Value::Null, "wide"),
            cell("x"),
        ]),
        row(vec![multi_paragraph_cell(), cell("tab\there"), cell("")]),
    ])]);
    let schema = engine_schema(&source);
    let document = document_of(&source).clone();
    let openings = cell_openings(&source);
    let index = TableProjectionIndex::derive_or_fallback(&document, &schema, &limits());
    let copied = crate::clipboard::export_cells(
        &document,
        &Selection::cell(openings[0], openings[4]),
        &index,
        &schema,
    )
    .expect("the rectangle copies");
    let expected = "wide\t\tx\n\"one\nsay \"\"two\"\"\"\t\"tab\there\"\t";
    assert_eq!(copied["text"], json!(expected), "the text flavour is TSV");

    let mut destination = seeded(three_by_three());
    place_caret(&mut destination, 0);
    pasted_plain_text(
        &mut destination,
        None,
        copied["text"]
            .as_str()
            .expect("the text flavour is a string"),
    );

    let table = table_of(&destination);
    assert_eq!(
        texts(&table),
        vec![
            vec!["wide", "", "x"],
            vec!["one", "tab\there", ""],
            vec!["c0", "c1", "c2"]
        ],
        "a span flattens to its first slot and leaves covered slots empty: {table}",
    );
    let paragraphs: Vec<&str> = table["content"][1]["content"][0]["content"]
        .as_array()
        .expect("the cell holds blocks")
        .iter()
        .map(|block| block["content"][0]["text"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        paragraphs,
        vec!["one", "say \"two\""],
        "quoted in-cell line breaks come back as paragraphs: {table}",
    );
}
