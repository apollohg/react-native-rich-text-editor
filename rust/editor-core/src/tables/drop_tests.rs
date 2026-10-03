use serde_json::{json, Value};

use crate::native_transaction_bridge::{
    NativeBridgeOutcome, NativeTransactionBridge, NATIVE_BRIDGE_ENVELOPE_VERSION,
};
use crate::selection::Selection;
use crate::session::EditorSession;
use crate::tables::commands_tests::{drain_document_updates, row_count, row_texts};
use crate::tables::interchange_tests::nesting_cell;
use crate::tables::normalize_tests::{cell, row, schema, seeded_session, table};

const REQUEST_ID: u64 = 71;
const ONE_DOCUMENT_UPDATE: usize = 1;
const NO_DOCUMENT_UPDATES: usize = 0;
const PARAGRAPH_NODE: &str = "paragraph";
const FIRST_TABLE: usize = 0;
const SECOND_TABLE: usize = 1;
const INSIDE_OPENING: u32 = 1;

fn three_by_three() -> Value {
    table(vec![
        row(vec![cell("a0"), cell("a1"), cell("a2")]),
        row(vec![cell("b0"), cell("b1"), cell("b2")]),
        row(vec![cell("c0"), cell("c1"), cell("c2")]),
    ])
}

fn paragraph(text: &str) -> Value {
    json!({ "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": text }] })
}

fn session_of(content: Vec<Value>) -> EditorSession {
    let mut session = seeded_session(json!({ "type": "doc", "content": content }).to_string());
    session.attach_collaboration_runtime();
    drain_document_updates(&mut session);
    session
}

fn table_openings(session: &EditorSession, ordinal: usize) -> Vec<u32> {
    let index = session
        .engine
        .table_projection_index()
        .expect("the engine is ready");
    let mut positions: Vec<u32> = index.positions().collect();
    positions.sort_unstable();
    index
        .table_at(positions[ordinal])
        .expect("the requested table projects")
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .collect()
}

fn copied_fragment(session: &EditorSession, anchor: u32, head: u32) -> String {
    let document = session.engine.document().expect("the engine is ready");
    let index = session
        .engine
        .table_projection_index()
        .expect("the engine is ready");
    let copied =
        crate::clipboard::export_cells(document, &Selection::cell(anchor, head), &index, &schema())
            .expect("the source rectangle copies");
    copied["fragment"]
        .as_str()
        .expect("a copied rectangle carries a fragment")
        .to_owned()
}

fn document(session: &EditorSession) -> Value {
    session.engine.document_json().expect("the engine is ready")
}

fn texts(table: &Value) -> Vec<Vec<String>> {
    (0..row_count(table))
        .map(|index| row_texts(table, index))
        .collect()
}

fn submit_drop(session: &mut EditorSession, command: Value) -> NativeBridgeOutcome {
    let envelope = json!({
        "version": NATIVE_BRIDGE_ENVELOPE_VERSION,
        "requestId": REQUEST_ID.to_string(),
        "baseDocumentRevision": session.engine.revision().to_string(),
        "command": command,
    })
    .to_string();
    NativeTransactionBridge::new(session)
        .submit_command(&envelope)
        .unwrap_or_else(|error| panic!("the drop {envelope} is answered: {error:?}"))
}

fn move_command(fragment: &str, target: u32, anchor: u32, head: u32) -> Value {
    json!({
        "type": "paste",
        "fragment": fragment,
        "cellDrop": {
            "targetCell": target,
            "movedCells": { "anchorCell": anchor, "headCell": head },
        },
    })
}

fn assert_transaction(outcome: &NativeBridgeOutcome) {
    assert!(
        matches!(outcome, NativeBridgeOutcome::Transaction(_)),
        "the drop must apply as one transaction: {outcome:?}",
    );
}

fn assert_single_undo_restores(session: &mut EditorSession, before: &Value, after: &Value) {
    assert!(
        NativeTransactionBridge::new(session)
            .undo(REQUEST_ID)
            .expect("the undo is answered"),
        "the move must be undoable",
    );
    assert_eq!(
        &document(session),
        before,
        "one undo restores both the source and the target",
    );
    assert!(
        !NativeTransactionBridge::new(session)
            .undo(REQUEST_ID)
            .expect("the second undo is answered"),
        "the move must be exactly one history entry",
    );
    assert!(NativeTransactionBridge::new(session)
        .redo(REQUEST_ID)
        .expect("the redo is answered"));
    assert_eq!(&document(session), after, "redo reapplies the whole move");
}

#[test]
fn a_same_table_move_pastes_at_the_drop_cell_and_clears_the_source_as_one_history_entry() {
    let mut session = session_of(vec![three_by_three()]);
    let openings = table_openings(&session, FIRST_TABLE);
    let fragment = copied_fragment(&session, openings[0], openings[1]);
    let before = document(&session);

    let outcome = submit_drop(
        &mut session,
        move_command(&fragment, openings[7], openings[0], openings[1]),
    );

    assert_transaction(&outcome);
    assert_eq!(
        drain_document_updates(&mut session),
        ONE_DOCUMENT_UPDATE,
        "the paste and the clear publish one document update",
    );
    let after = document(&session);
    assert_eq!(
        texts(&after["content"][0]),
        vec![
            vec!["", "", "a2"],
            vec!["b0", "b1", "b2"],
            vec!["c0", "a0", "a1"],
        ],
        "a0 a1 move from the top row to c1 c2: {after}",
    );
    assert_single_undo_restores(&mut session, &before, &after);
}

#[test]
fn an_overlapping_move_writes_the_source_over_its_own_cleared_cells() {
    let mut session = session_of(vec![three_by_three()]);
    let openings = table_openings(&session, FIRST_TABLE);
    let fragment = copied_fragment(&session, openings[0], openings[4]);

    let outcome = submit_drop(
        &mut session,
        move_command(&fragment, openings[4], openings[0], openings[4]),
    );

    assert_transaction(&outcome);
    let after = document(&session);
    assert_eq!(
        texts(&after["content"][0]),
        vec![
            vec!["", "", "a2"],
            vec!["", "a0", "a1"],
            vec!["c0", "b0", "b1"],
        ],
        "moving the top-left 2x2 one cell down-right keeps the whole source: {after}",
    );
}

#[test]
fn a_move_into_an_earlier_table_clears_the_source_in_the_same_transaction() {
    let mut session = session_of(vec![
        table(vec![row(vec![cell("x0"), cell("x1")])]),
        paragraph("between"),
        three_by_three(),
    ]);
    let target = table_openings(&session, FIRST_TABLE);
    let source = table_openings(&session, SECOND_TABLE);
    let fragment = copied_fragment(&session, source[3], source[4]);
    let before = document(&session);

    let outcome = submit_drop(
        &mut session,
        move_command(&fragment, target[0], source[3], source[4]),
    );

    assert_transaction(&outcome);
    assert_eq!(drain_document_updates(&mut session), ONE_DOCUMENT_UPDATE);
    let after = document(&session);
    assert_eq!(texts(&after["content"][0]), vec![vec!["b0", "b1"]]);
    assert_eq!(
        texts(&after["content"][2]),
        vec![
            vec!["a0", "a1", "a2"],
            vec!["", "", "b2"],
            vec!["c0", "c1", "c2"],
        ],
        "the source row of the second table is cleared in the same transaction: {after}",
    );
    assert_single_undo_restores(&mut session, &before, &after);
}

#[test]
fn a_move_into_a_later_table_leaves_no_partial_state_between_the_two_tables() {
    let mut session = session_of(vec![
        three_by_three(),
        paragraph("between"),
        table(vec![row(vec![cell("x0"), cell("x1"), cell("x2")])]),
    ]);
    let source = table_openings(&session, FIRST_TABLE);
    let target = table_openings(&session, SECOND_TABLE);
    let fragment = copied_fragment(&session, source[6], source[8]);
    let before = document(&session);

    let outcome = submit_drop(
        &mut session,
        move_command(&fragment, target[0], source[6], source[8]),
    );

    assert_transaction(&outcome);
    let after = document(&session);
    assert_eq!(
        texts(&after["content"][0])[2],
        vec!["", "", ""],
        "the moved source row is cleared: {after}",
    );
    assert_eq!(texts(&after["content"][2]), vec![vec!["c0", "c1", "c2"]]);
    assert_single_undo_restores(&mut session, &before, &after);
}

#[test]
fn a_copy_drop_pastes_the_matrix_at_the_drop_cell_and_grows_the_table() {
    let mut session = session_of(vec![three_by_three()]);
    let openings = table_openings(&session, FIRST_TABLE);

    let outcome = submit_drop(
        &mut session,
        json!({
            "type": "paste",
            "text": "1\t2\n3\t4",
            "cellDrop": { "targetCell": openings[8] },
        }),
    );

    assert_transaction(&outcome);
    let after = document(&session);
    assert_eq!(
        texts(&after["content"][0]),
        vec![
            vec!["a0", "a1", "a2", ""],
            vec!["b0", "b1", "b2", ""],
            vec!["c0", "c1", "1", "2"],
            vec!["", "", "3", "4"],
        ],
        "a caret drop is unclipped and grows the table from the drop cell: {after}",
    );
}

#[test]
fn a_plain_text_drop_inserts_at_the_start_of_the_drop_cell() {
    let mut session = session_of(vec![paragraph("prose"), three_by_three()]);
    let openings = table_openings(&session, FIRST_TABLE);

    let outcome = submit_drop(
        &mut session,
        json!({ "type": "paste", "text": "x", "cellDrop": { "targetCell": openings[3] } }),
    );

    assert_transaction(&outcome);
    let after = document(&session);
    assert_eq!(
        texts(&after["content"][1])[1],
        vec!["xb0", "b1", "b2"],
        "text without tabs lands at the drop cell's first caret, not the engine selection: {after}",
    );
    assert_eq!(after["content"][0], paragraph("prose"));
}

#[test]
fn a_drop_position_that_is_not_a_cell_opening_is_refused_without_mutation() {
    let mut session = session_of(vec![three_by_three()]);
    let openings = table_openings(&session, FIRST_TABLE);
    let fragment = copied_fragment(&session, openings[0], openings[0]);
    let before = document(&session);

    for synthetic in [openings[4] + INSIDE_OPENING, openings[4] - INSIDE_OPENING] {
        let outcome = submit_drop(
            &mut session,
            move_command(&fragment, synthetic, openings[0], openings[0]),
        );
        assert_eq!(
            outcome,
            NativeBridgeOutcome::NotApplicable,
            "position {synthetic} is not a real cell and must not receive the drop",
        );
    }
    assert_eq!(drain_document_updates(&mut session), NO_DOCUMENT_UPDATES);
    assert_eq!(document(&session), before);
}

#[test]
fn a_drop_onto_a_nested_table_cell_is_refused_without_mutation() {
    let mut session = session_of(vec![table(vec![row(vec![nesting_cell(), cell("b")])])]);
    let outer = table_openings(&session, FIRST_TABLE);
    let nested = table_openings(&session, SECOND_TABLE);
    let fragment = copied_fragment(&session, outer[1], outer[1]);
    let before = document(&session);

    let outcome = submit_drop(
        &mut session,
        move_command(&fragment, nested[0], outer[1], outer[1]),
    );

    assert_eq!(outcome, NativeBridgeOutcome::NotApplicable);
    assert_eq!(document(&session), before);
}

#[test]
fn a_move_without_a_cell_matrix_is_refused_before_clearing_its_source() {
    let mut session = session_of(vec![three_by_three()]);
    let openings = table_openings(&session, FIRST_TABLE);
    let before = document(&session);

    let outcome = submit_drop(
        &mut session,
        json!({
            "type": "paste",
            "text": "no tabs here",
            "cellDrop": {
                "targetCell": openings[8],
                "movedCells": { "anchorCell": openings[0], "headCell": openings[1] },
            },
        }),
    );

    assert_eq!(outcome, NativeBridgeOutcome::NotApplicable);
    assert_eq!(drain_document_updates(&mut session), NO_DOCUMENT_UPDATES);
    assert_eq!(
        document(&session),
        before,
        "a refused move must not clear its source"
    );
}

#[test]
fn a_move_whose_source_is_not_a_cell_rectangle_is_refused() {
    let mut session = session_of(vec![paragraph("prose"), three_by_three()]);
    let openings = table_openings(&session, FIRST_TABLE);
    let fragment = copied_fragment(&session, openings[0], openings[0]);
    let before = document(&session);

    let outcome = submit_drop(
        &mut session,
        move_command(&fragment, openings[8], INSIDE_OPENING, INSIDE_OPENING),
    );

    assert_eq!(outcome, NativeBridgeOutcome::NotApplicable);
    assert_eq!(document(&session), before);
}

#[test]
fn a_move_of_already_empty_cells_writes_them_over_the_drop_cells() {
    let mut session = session_of(vec![table(vec![
        row(vec![cell(""), cell("")]),
        row(vec![cell("c0"), cell("c1")]),
    ])]);
    let openings = table_openings(&session, FIRST_TABLE);
    let fragment = copied_fragment(&session, openings[0], openings[1]);
    let before = document(&session);

    let outcome = submit_drop(
        &mut session,
        move_command(&fragment, openings[2], openings[0], openings[1]),
    );

    assert_transaction(&outcome);
    let after = document(&session);
    assert_eq!(
        texts(&after["content"][0]),
        vec![vec!["", ""], vec!["", ""]],
        "an empty source has nothing to clear but still moves onto the drop cells: {after}",
    );
    assert_single_undo_restores(&mut session, &before, &after);
}
