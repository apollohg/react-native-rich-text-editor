use serde_json::{json, Value};

use crate::session::EditorSession;
use crate::tables::commands::TableCommand;
use crate::tables::commands_tests::{
    cell_openings, document_of, place_caret, resolved_caret, select_cells, session_select_rectangle,
};
use crate::tables::history_tests::{
    apply, awaiting_session, document_json, exchange, redo, session_from, type_into_cell, undo,
};
use crate::tables::normalize_tests::{cell, row, table};
use crate::yrs_engine::{
    Affinity, EditorOffsetKind, HistoryPolicy, RevisionedPosition, SelectionInput, SelectionIntent,
    TransactionOrigin, TypedCommand, TypedTransaction, YrsDocumentEngine,
};

const REQUEST_ID: u64 = 47;
const CELL_TEXT_OFFSET: u32 = 2;
const GRID_ROWS: [&str; 3] = ["a", "b", "c"];
const GRID_COLUMNS: u32 = 3;
const GRID_WIDTH: usize = 3;
const A0: usize = 0;
const A1: usize = 1;
const B1: usize = GRID_WIDTH + 1;
const B2: usize = GRID_WIDTH + 2;
const C0: usize = 2 * GRID_WIDTH;
const C2: usize = 2 * GRID_WIDTH + 2;
const FIRST_COLUMN: u32 = 0;
const TABLE_POSITION: u32 = 0;
const RESIZED_WIDTH: u32 = 150;
const ONE_CHARACTER: u32 = 1;
const CELL_AFTER_MERGED_A1: usize = 1;
const SURROUNDING_TEXT: &str = "prose";
const LAST_ROW: usize = 2;
const LAST_COLUMN: usize = 2;
const PROSE_PARAGRAPHS: [&str; 4] = ["abc", "defghijk", "xyz", "uvw"];
const INSIDE_FIRST_PARAGRAPH: u32 = 3;
const SECOND_PARAGRAPH_START: u32 = 6;
const INSIDE_THIRD_PARAGRAPH: u32 = 17;
const CARET_IN_SECOND_PARAGRAPH: u32 = 8;

fn grid() -> Value {
    table(
        GRID_ROWS
            .iter()
            .map(|name| {
                row((0..GRID_COLUMNS)
                    .map(|column| cell(&format!("{name}{column}")))
                    .collect())
            })
            .collect(),
    )
}

fn prose() -> Value {
    json!({ "type": "paragraph", "content": [{ "type": "text", "text": SURROUNDING_TEXT }] })
}

fn fixture(content: Vec<Value>) -> String {
    json!({ "type": "doc", "content": content }).to_string()
}

fn peers(content: &[Value]) -> (EditorSession, EditorSession) {
    let mut local = session_from(fixture(content.to_vec()));
    let mut remote = awaiting_session();
    exchange(&mut local, &mut remote);
    (local, remote)
}

fn select_cell(engine: &mut YrsDocumentEngine, index: usize) {
    select_cells(engine, index, index);
}

fn assert_remote_command_maps_selection_like_local(
    content: Vec<Value>,
    select: fn(&mut YrsDocumentEngine, usize),
    target_cell: usize,
    command: TableCommand,
    name: &str,
) {
    let (mut local, mut remote) = peers(&content);
    let mut mirror = session_from(fixture(content));
    select(&mut local.engine, target_cell);
    select(&mut mirror.engine, target_cell);
    place_caret(&mut remote.engine, target_cell);
    let before = local.engine.resolved_selection().cloned();
    apply(&mut remote, command);
    apply(&mut mirror, command);
    exchange(&mut remote, &mut local);
    assert_eq!(
        document_json(&local),
        document_json(&mirror),
        "{name}: the remote and mirrored local commands must converge on the same document",
    );
    assert_eq!(
        local.engine.resolved_selection(),
        mirror.engine.resolved_selection(),
        "{name}: a selection {before:?} must land where the same local command maps it",
    );
}

#[test]
fn a_remote_row_delete_maps_the_local_caret_like_the_same_local_delete() {
    for (content, caret_cell, name) in [
        (vec![grid()], B1, "middle row"),
        (vec![grid(), prose()], C2, "last row before prose"),
        (vec![grid()], C2, "last row of a trailing table"),
    ] {
        assert_remote_command_maps_selection_like_local(
            content,
            place_caret,
            caret_cell,
            TableCommand::DeleteTableRows,
            name,
        );
    }
}

#[test]
fn a_remote_column_delete_maps_the_local_caret_like_the_same_local_delete() {
    for (caret_cell, name) in [
        (B1, "middle column"),
        (B2, "last column"),
        (C2, "last cell"),
    ] {
        assert_remote_command_maps_selection_like_local(
            vec![grid(), prose()],
            place_caret,
            caret_cell,
            TableCommand::DeleteTableColumns,
            name,
        );
    }
}

#[test]
fn a_remote_table_delete_maps_the_local_caret_like_the_same_local_delete() {
    for (content, name) in [
        (vec![grid(), prose()], "table before prose"),
        (vec![prose(), grid()], "table after prose"),
    ] {
        assert_remote_command_maps_selection_like_local(
            content,
            place_caret,
            B1,
            TableCommand::DeleteTable { table_pos: None },
            name,
        );
    }
}

#[test]
fn a_remote_structural_delete_maps_a_local_cell_selection_like_the_same_local_delete() {
    for (command, name) in [
        (TableCommand::DeleteTableRows, "row delete"),
        (TableCommand::DeleteTableColumns, "column delete"),
    ] {
        assert_remote_command_maps_selection_like_local(
            vec![grid(), prose()],
            select_cell,
            B1,
            command,
            name,
        );
    }
}

#[test]
fn a_remote_merge_over_the_local_caret_moves_it_forward_like_a_prosemirror_mapping() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    place_caret(&mut local.engine, A1);
    session_select_rectangle(&mut remote, A0, A1);
    apply(&mut remote, TableCommand::MergeTableCells);
    exchange(&mut remote, &mut local);
    let next_cell_text = cell_openings(&local.engine)[CELL_AFTER_MERGED_A1] + CELL_TEXT_OFFSET;
    assert_eq!(
        resolved_caret(&local.engine),
        Some((next_cell_text, next_cell_text)),
        "a caret in the merged-away a1 must search forward from its removed slot to the start of a2: {}",
        document_json(&local),
    );
}

#[test]
fn a_remote_delete_of_everything_a_local_undo_item_wrote_drops_that_item() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    type_into_cell(&mut local, C2);
    assert!(local.engine.can_undo(), "the local typing is undoable");
    exchange(&mut local, &mut remote);
    place_caret(&mut remote.engine, C2);
    apply(&mut remote, TableCommand::DeleteTableRows);
    exchange(&mut remote, &mut local);
    assert!(
        !local.engine.can_undo(),
        "the only undo item wrote text the remote peer deleted, so nothing is left to undo",
    );
    let settled = document_json(&local);
    let (engine, outbox) = local.engine_and_outbox();
    assert_eq!(
        engine
            .undo_with_outbox(REQUEST_ID, outbox)
            .expect("an unavailable undo is not an error"),
        None,
        "undo must be refused when no undo item remains",
    );
    assert_eq!(document_json(&local), settled);
}

#[test]
fn a_remote_delete_of_everything_a_local_redo_item_restores_drops_that_item() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    type_into_cell(&mut local, C2);
    undo(&mut local);
    assert!(local.engine.can_redo(), "the undone typing is redoable");
    exchange(&mut local, &mut remote);
    place_caret(&mut remote.engine, C2);
    apply(&mut remote, TableCommand::DeleteTableRows);
    exchange(&mut remote, &mut local);
    assert!(
        !local.engine.can_redo(),
        "the only redo item restores text into a row the remote peer deleted",
    );
}

#[test]
fn a_remote_delete_elsewhere_keeps_the_local_undo_item() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    type_into_cell(&mut local, A0);
    exchange(&mut local, &mut remote);
    place_caret(&mut remote.engine, C2);
    apply(&mut remote, TableCommand::DeleteTableRows);
    exchange(&mut remote, &mut local);
    assert!(
        local.engine.can_undo(),
        "typing in a surviving row must stay undoable"
    );
    let before_undo = document_json(&local);
    undo(&mut local);
    assert_ne!(
        document_json(&local),
        before_undo,
        "the surviving undo item must still revert the typing",
    );
}

fn delete_backward_in(session: &mut EditorSession, index: usize) {
    place_caret(&mut session.engine, index);
    let (engine, outbox) = session.engine_and_outbox();
    engine
        .apply_command_with_outbox(REQUEST_ID, TypedCommand::DeleteBackward, outbox)
        .expect("the backspace applies")
        .expect("the backspace produced a transaction");
}

#[test]
fn a_local_deletion_stays_undoable_only_while_its_container_survives_a_remote_delete() {
    for (deleted_in, undoable, name) in [
        (C2, false, "a backspace inside the removed row"),
        (A0, true, "a backspace inside a surviving row"),
    ] {
        let (mut local, mut remote) = peers(&[grid(), prose()]);
        delete_backward_in(&mut local, deleted_in);
        exchange(&mut local, &mut remote);
        place_caret(&mut remote.engine, C2);
        apply(&mut remote, TableCommand::DeleteTableRows);
        exchange(&mut remote, &mut local);
        assert_eq!(
            local.engine.can_undo(),
            undoable,
            "{name}: undo can restore the deleted character only into a live container",
        );
    }
}

#[test]
fn undo_restoring_a_selection_the_remote_peer_removed_maps_it_to_a_surviving_anchor() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    let unresized = document_json(&local);
    select_cells(&mut local.engine, C0, C0);
    apply(
        &mut local,
        TableCommand::SetTableColumnWidth {
            width: RESIZED_WIDTH,
            column: Some(FIRST_COLUMN),
            table_pos: Some(TABLE_POSITION),
        },
    );
    type_into_cell(&mut local, C2);
    exchange(&mut local, &mut remote);
    place_caret(&mut remote.engine, C2);
    apply(&mut remote, TableCommand::DeleteTableRows);
    exchange(&mut remote, &mut local);
    assert!(
        local.engine.can_undo(),
        "the resize outlives the removed row, so it stays undoable"
    );

    undo(&mut local);
    let mut expected = unresized;
    expected["content"][0]["content"]
        .as_array_mut()
        .expect("the table holds rows")
        .truncate(GRID_ROWS.len() - 1);
    assert_eq!(
        document_json(&local),
        expected,
        "undo reverts the resize and keeps the remote row delete",
    );
    let following_prose = document_of(&local.engine)
        .node_at(&[0])
        .expect("the table survives")
        .node_size()
        + ONE_CHARACTER;
    assert_eq!(
        resolved_caret(&local.engine),
        Some((following_prose, following_prose)),
        "the restored cell selection on the removed c0 maps forward into the prose after the table",
    );
}

fn cell_text_at(session: &EditorSession, row: usize, column: usize) -> String {
    document_json(session)["content"][0]["content"][row]["content"][column]["content"][0]["content"]
        [0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_local_deletion_whose_row_was_restored_by_undo_stays_undoable_after_a_remote_edit() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    delete_backward_in(&mut local, C2);
    assert_eq!(cell_text_at(&local, LAST_ROW, LAST_COLUMN), "2");
    apply(&mut local, TableCommand::DeleteTableRows);
    undo(&mut local);
    assert_eq!(
        cell_text_at(&local, LAST_ROW, LAST_COLUMN),
        "2",
        "undo restores the row as redone copies"
    );
    exchange(&mut local, &mut remote);
    type_into_cell(&mut remote, A0);
    exchange(&mut remote, &mut local);
    assert!(
        local.engine.can_undo(),
        "the backspace's text parent was redone, so undo can still restore its character",
    );
    undo(&mut local);
    assert_eq!(
        cell_text_at(&local, LAST_ROW, LAST_COLUMN),
        "c2",
        "undo restores the deleted character into the redone cell",
    );
}

#[test]
fn a_dropped_redo_item_stays_dropped_after_later_undo_and_redo() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    select_cells(&mut local.engine, C0, C0);
    apply(
        &mut local,
        TableCommand::SetTableColumnWidth {
            width: RESIZED_WIDTH,
            column: Some(FIRST_COLUMN),
            table_pos: Some(TABLE_POSITION),
        },
    );
    type_into_cell(&mut local, C2);
    undo(&mut local);
    exchange(&mut local, &mut remote);
    place_caret(&mut remote.engine, C2);
    apply(&mut remote, TableCommand::DeleteTableRows);
    exchange(&mut remote, &mut local);
    assert!(
        !local.engine.can_redo(),
        "the undone typing restores into the removed row, so it is dropped",
    );

    undo(&mut local);
    assert!(
        local.engine.can_redo(),
        "undoing the resize makes it redoable"
    );
    redo(&mut local);
    assert!(
        !local.engine.can_redo(),
        "replaying history must not resurrect the dropped typing on the redo stack",
    );
    let (engine, outbox) = local.engine_and_outbox();
    assert_eq!(
        engine
            .redo_with_outbox(REQUEST_ID, outbox)
            .expect("an unavailable redo is not an error"),
        None,
    );
}

#[test]
fn concurrent_local_typing_into_a_remotely_removed_row_is_dropped_from_history() {
    let (mut local, mut remote) = peers(&[grid(), prose()]);
    place_caret(&mut remote.engine, C2);
    apply(&mut remote, TableCommand::DeleteTableRows);
    type_into_cell(&mut local, C2);
    assert!(local.engine.can_undo());
    exchange(&mut remote, &mut local);
    exchange(&mut local, &mut remote);
    assert_eq!(
        document_json(&local),
        document_json(&remote),
        "both peers converge"
    );
    assert!(
        !local.engine.can_undo(),
        "the concurrent typing went down with its row, so nothing is left to undo",
    );
    let following_prose = document_of(&local.engine)
        .node_at(&[0])
        .expect("the table survives")
        .node_size()
        + ONE_CHARACTER;
    assert_eq!(
        resolved_caret(&local.engine),
        Some((following_prose, following_prose)),
        "the caret after the typing in the removed last row moves into the prose after the table",
    );
}

fn prose_paragraphs() -> Vec<Value> {
    PROSE_PARAGRAPHS
        .iter()
        .map(|text| json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] }))
        .collect()
}

fn select_text(engine: &mut YrsDocumentEngine, anchor: u32, head: u32) {
    let document = document_of(engine).clone();
    let map = engine.position_map().expect("the engine is ready");
    let point = |position| RevisionedPosition {
        offset: map.doc_to_scalar(position, &document),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    let (anchor, head) = (point(anchor), point(head));
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Text { anchor, head }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the text selection applies");
}

fn remote_backspace(remote: &mut EditorSession, anchor: u32, head: u32) -> Value {
    select_text(&mut remote.engine, anchor, head);
    let (engine, outbox) = remote.engine_and_outbox();
    engine
        .apply_command_with_outbox(REQUEST_ID, TypedCommand::DeleteBackward, outbox)
        .expect("the remote backspace applies")
        .expect("the remote backspace produced a transaction");
    document_json(remote)
}

#[test]
fn a_remote_paragraph_join_keeps_the_local_caret_in_the_joined_paragraph() {
    let (mut local, mut remote) = peers(&prose_paragraphs());
    select_text(
        &mut local.engine,
        CARET_IN_SECOND_PARAGRAPH,
        CARET_IN_SECOND_PARAGRAPH,
    );
    let joined = remote_backspace(&mut remote, SECOND_PARAGRAPH_START, SECOND_PARAGRAPH_START);
    assert_eq!(joined["content"][0]["content"][0]["text"], "abcdefghijk");
    exchange(&mut remote, &mut local);
    let joined_paragraph_end = document_of(&local.engine)
        .node_at(&[0])
        .expect("the joined paragraph survives")
        .node_size()
        - ONE_CHARACTER;
    let (anchor, head) = resolved_caret(&local.engine).expect("a text caret");
    assert_eq!(anchor, head);
    assert!(
        anchor <= joined_paragraph_end,
        "a prose join must not push the caret into the next paragraph: {anchor} > {joined_paragraph_end}",
    );
}

#[test]
fn a_remote_prose_range_delete_keeps_the_pre_table_fallback() {
    let (mut local, mut remote) = peers(&prose_paragraphs());
    select_text(
        &mut local.engine,
        CARET_IN_SECOND_PARAGRAPH,
        CARET_IN_SECOND_PARAGRAPH,
    );
    let remaining = remote_backspace(&mut remote, INSIDE_FIRST_PARAGRAPH, INSIDE_THIRD_PARAGRAPH);
    assert_eq!(
        remaining["content"][0]["content"][0]["text"], "abyz",
        "the remote range delete joins the first and third paragraphs"
    );
    exchange(&mut remote, &mut local);
    assert_eq!(
        resolved_caret(&local.engine),
        Some((CARET_IN_SECOND_PARAGRAPH, CARET_IN_SECOND_PARAGRAPH)),
        "removed prose without table structure keeps the base fallback instead of the table slot climb",
    );
}
