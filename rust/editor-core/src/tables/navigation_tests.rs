use serde_json::{json, Value};

use crate::command_planner::SemanticOperation;
use crate::model::Document;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::interchange::{next_outer_cell, CellStep};
use crate::tables::mutation_guard::{admit_local_mutation, LocalMutationRefusal};
use crate::tables::normalize_tests::{cell, cell_with, document_with, limits, row, schema, table};
use crate::yrs_engine::{
    Affinity, EditorOffsetKind, InitializationMode, RevisionedPosition, SelectionInput,
    TransactionOrigin, TypedCommand, YrsDocumentEngine, YrsEngineConfig,
};

const FRAGMENT_NAME: &str = "prosemirror";
const CELL_NODE: &str = "table_cell";
const PARAGRAPH_NODE: &str = "paragraph";
const OUTER_TABLE_POSITION: u32 = 0;
const CELL_TEXT_OFFSET: u32 = 2;
const REQUEST_ID: u64 = 11;
const SINGLE_SPAN: u32 = 1;
const NESTED_TABLE_FIELD: &str = "nestedTable";
const CELL_BOUNDARY_FIELD: &str = "tableCellBoundary";
const NESTED_ROW_AND_TABLE_CLOSING_TOKENS: u32 = 2;

fn cell_holding_a_nested_table() -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [
            { "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "outer" }] },
            table(vec![row(vec![cell("inner")])]),
        ],
    })
}

fn nested_fixture() -> Document {
    document_with(vec![table(vec![
        row(vec![cell("a"), cell("b")]),
        row(vec![cell_holding_a_nested_table(), cell("d")]),
    ])])
}

fn index_of(document: &Document) -> TableProjectionIndex {
    TableProjectionIndex::derive_or_fallback(document, &schema(), &limits())
}

fn openings(index: &TableProjectionIndex, table_pos: u32) -> Vec<u32> {
    index
        .table_at(table_pos)
        .expect("the fixture projects that table")
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .collect()
}

fn nested_table_position(index: &TableProjectionIndex) -> u32 {
    index
        .positions()
        .filter(|position| *position != OUTER_TABLE_POSITION)
        .max()
        .expect("the fixture holds a nested table")
}

fn engine_with(document_json: Value) -> YrsDocumentEngine {
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema: schema(),
        fragment_name: FRAGMENT_NAME.into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: limits(),
        editing_limits: crate::yrs_engine::EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .expect("the tabled engine initializes");
    engine
        .import_json(
            &document_json.to_string(),
            TransactionOrigin::DocumentImport,
        )
        .expect("the fixture document imports");
    engine
}

fn caret_at(engine: &mut YrsDocumentEngine, doc_position: u32) {
    let document = engine.document().expect("the engine is ready");
    let offset = engine
        .position_map()
        .expect("the engine is ready")
        .doc_to_scalar(doc_position, document);
    let point = RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    engine
        .apply_typed_transaction(crate::yrs_engine::TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: crate::yrs_engine::SelectionIntent::Set(SelectionInput::Text {
                anchor: point,
                head: point,
            }),
            history_policy: crate::yrs_engine::HistoryPolicy::Skip,
        })
        .expect("the caret applies");
}

fn refusal_field(error: &crate::yrs_engine::OperationError) -> Option<String> {
    error.details.as_ref()?["field"].as_str().map(str::to_owned)
}

#[test]
fn forward_navigation_walks_every_real_outer_cell_once_and_stops_at_the_end() {
    let document = nested_fixture();
    let index = index_of(&document);
    let outer = openings(&index, OUTER_TABLE_POSITION);
    assert_eq!(outer.len(), 4, "the fixture holds four outer cells");

    let mut walked = vec![outer[0]];
    while let Some(next) = next_outer_cell(
        &index,
        *walked.last().expect("the walk has a cursor") + CELL_TEXT_OFFSET,
        CellStep::Forward,
    ) {
        walked.push(next);
    }
    assert_eq!(
        walked, outer,
        "forward navigation visits every real outer cell in document order and then stops",
    );
    assert_eq!(
        next_outer_cell(&index, outer[0] + CELL_TEXT_OFFSET, CellStep::Backward),
        None,
        "the first real cell has no backward neighbour",
    );
}

#[test]
fn a_merged_cell_is_offered_once_rather_than_once_per_covered_slot() {
    let document = document_with(vec![table(vec![
        row(vec![cell_with(2, SINGLE_SPAN, Value::Null, "wide")]),
        row(vec![cell("c"), cell("d")]),
    ])]);
    let index = index_of(&document);
    let outer = openings(&index, OUTER_TABLE_POSITION);
    assert_eq!(outer.len(), 3, "the fixture holds three real cells");
    assert_eq!(
        next_outer_cell(&index, outer[0] + CELL_TEXT_OFFSET, CellStep::Forward),
        Some(outer[1]),
        "a colspan-2 cell is left once, not once per covered column",
    );
}

#[test]
fn a_caret_inside_a_nested_table_navigates_the_enclosing_outer_table() {
    let document = nested_fixture();
    let index = index_of(&document);
    let outer = openings(&index, OUTER_TABLE_POSITION);
    let nested = openings(&index, nested_table_position(&index));
    assert_eq!(nested.len(), 1, "the fixture holds one nested cell");

    let inside_nested = nested[0] + CELL_TEXT_OFFSET;
    assert_eq!(
        next_outer_cell(&index, inside_nested, CellStep::Forward),
        Some(outer[3]),
        "navigation from inside a nested table steps to the next outer cell, never an inner one",
    );
    assert_eq!(
        next_outer_cell(&index, inside_nested, CellStep::Backward),
        Some(outer[1]),
        "backward navigation from inside a nested table also leaves through the outer grid",
    );
}

#[test]
fn native_input_inside_a_nested_table_is_refused_while_the_outer_cell_accepts_it() {
    let mut engine = engine_with(json!({ "type": "doc", "content": [table(vec![
        row(vec![cell_holding_a_nested_table(), cell("d")]),
    ])] }));
    let index = index_of(engine.document().expect("the engine is ready"));
    let nested = openings(&index, nested_table_position(&index));
    let outer = openings(&index, OUTER_TABLE_POSITION);

    caret_at(&mut engine, nested[0] + CELL_TEXT_OFFSET);
    let refused = engine
        .apply_command(
            REQUEST_ID,
            TypedCommand::InsertText {
                text: "x".to_string(),
            },
        )
        .expect_err("typing inside a nested table must be refused");
    assert_eq!(
        refusal_field(&refused).as_deref(),
        Some(NESTED_TABLE_FIELD),
        "the refusal must name the nested table, not a generic command failure: {refused:?}",
    );

    caret_at(&mut engine, outer[1] + CELL_TEXT_OFFSET);
    assert!(
        engine
            .apply_command(
                REQUEST_ID,
                TypedCommand::InsertText {
                    text: "x".to_string(),
                },
            )
            .expect("typing in an outer cell still plans")
            .is_some(),
        "the guard must only refuse nested descendants",
    );
}

#[test]
fn a_programmatic_attribute_write_into_a_nested_table_is_refused() {
    let document = nested_fixture();
    let index = index_of(&document);
    let nested_table = nested_table_position(&index);
    let nested_cell = openings(&index, nested_table)[0];
    for target in [nested_table, nested_cell] {
        assert_eq!(
            admit_local_mutation(
                &document,
                &schema(),
                &limits(),
                TransactionOrigin::LocalApi,
                &[SemanticOperation::UpdateNodeAttrs {
                    pos: target,
                    attrs: std::collections::HashMap::new(),
                }],
            ),
            Err(LocalMutationRefusal::NestedTableDescendant),
            "a bridge attribute write at {target} must be refused, not merely hidden in the UI",
        );
    }
}

#[test]
fn an_enclosing_deletion_that_covers_a_nested_table_whole_is_admitted() {
    let document = nested_fixture();
    let index = index_of(&document);
    let nested_table = nested_table_position(&index);
    let nested_end = index
        .table_at(nested_table)
        .expect("the nested table projects")
        .cells
        .iter()
        .map(|cell| cell.source_end)
        .max()
        .expect("the nested table holds a cell")
        + NESTED_ROW_AND_TABLE_CLOSING_TOKENS;
    assert_eq!(
        admit_local_mutation(
            &document,
            &schema(),
            &limits(),
            TransactionOrigin::LocalCommand,
            &[SemanticOperation::DeleteRange {
                from: nested_table,
                to: nested_end,
            }],
        ),
        Ok(()),
        "deleting a nested table as a whole is structural editing of its owner, not of the nest",
    );
    assert_eq!(
        admit_local_mutation(
            &document,
            &schema(),
            &limits(),
            TransactionOrigin::LocalCommand,
            &[SemanticOperation::DeleteRange {
                from: nested_table + 1,
                to: nested_end,
            }],
        ),
        Err(LocalMutationRefusal::NestedTableDescendant),
        "a deletion that opens the nested table without removing it must be refused",
    );
}

#[test]
fn replay_traffic_is_never_held_to_the_local_nested_table_refusal() {
    let document = nested_fixture();
    let index = index_of(&document);
    let nested_cell = openings(&index, nested_table_position(&index))[0];
    let operations = [SemanticOperation::InsertText {
        pos: nested_cell + CELL_TEXT_OFFSET,
        text: "x".to_string(),
        marks: Vec::new(),
    }];
    assert_eq!(
        admit_local_mutation(
            &document,
            &schema(),
            &limits(),
            TransactionOrigin::LocalInput,
            &operations,
        ),
        Err(LocalMutationRefusal::NestedTableDescendant),
        "local input inside a nested table is refused",
    );
    for origin in [
        TransactionOrigin::UndoRedo,
        TransactionOrigin::RemoteSync,
        TransactionOrigin::SnapshotRestore,
        TransactionOrigin::DocumentImport,
    ] {
        assert_eq!(
            admit_local_mutation(&document, &schema(), &limits(), origin, &operations),
            Ok(()),
            "{origin:?} replays geometry this engine did not author and must not be refused",
        );
    }
}

#[test]
fn a_join_across_a_cell_boundary_is_refused_while_an_intra_cell_join_is_admitted() {
    let document = document_with(vec![table(vec![row(vec![cell("a"), cell("b")])])]);
    let index = index_of(&document);
    let outer = openings(&index, OUTER_TABLE_POSITION);
    let first_block_start = outer[0] + 1;
    assert_eq!(
        admit_local_mutation(
            &document,
            &schema(),
            &limits(),
            TransactionOrigin::LocalInput,
            &[SemanticOperation::JoinBlocks {
                pos: first_block_start
            }],
        ),
        Err(LocalMutationRefusal::CellBoundaryJoin),
        "backspace at the start of a cell must never join the cell into its predecessor",
    );

    let two_block_cell = json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [
            { "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "one" }] },
            { "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "two" }] },
        ],
    });
    let document = document_with(vec![table(vec![row(vec![two_block_cell])])]);
    let index = index_of(&document);
    let opening = openings(&index, OUTER_TABLE_POSITION)[0];
    assert_eq!(
        admit_local_mutation(
            &document,
            &schema(),
            &limits(),
            TransactionOrigin::LocalInput,
            &[SemanticOperation::JoinBlocks {
                pos: opening + 1 + 5
            }],
        ),
        Ok(()),
        "joining two blocks inside one cell stays ordinary intra-cell block editing",
    );
}

#[test]
fn a_refusal_names_its_own_boundary_field() {
    assert_eq!(
        refusal_field(
            &LocalMutationRefusal::NestedTableDescendant.into_operation_error(REQUEST_ID)
        )
        .as_deref(),
        Some(NESTED_TABLE_FIELD),
    );
    assert_eq!(
        refusal_field(&LocalMutationRefusal::CellBoundaryJoin.into_operation_error(REQUEST_ID))
            .as_deref(),
        Some(CELL_BOUNDARY_FIELD),
    );
}
