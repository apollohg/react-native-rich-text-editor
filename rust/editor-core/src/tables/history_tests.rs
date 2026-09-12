use serde_json::json;

use crate::document_api::DocumentApiFacade;
use crate::session::{
    CollaborationLimits, EditorInitialization, EditorSession, EditorSessionConfig,
};
use crate::tables::commands::TableCommand;
use crate::tables::commands_tests::{
    drain_document_updates, session_cell_openings, session_select_rectangle,
};
use crate::tables::normalize::{planned_normalization_passes, reset_planned_normalization_passes};
use crate::tables::normalize_tests::{cell, cell_with, limits, row, schema, seeded_session, table};
use crate::yrs_engine::{
    Affinity, DocumentScope, EditingLimits, EditorOffsetKind, HistoryPolicy, RevisionedPosition,
    SelectionInput, SelectionIntent, TransactionOrigin, TypedCommand, TypedTransaction,
};

const REQUEST_ID: u64 = 31;
const COLLABORATION_FRAGMENT_NAME: &str = "prosemirror";
const HISTORY_DOCUMENT_ID: &str = "tables-history-document";
const HISTORY_LINEAGE_ID: &str = "tables-history-lineage";
const CELL_TEXT_OFFSET: u32 = 2;
const NO_NORMALIZATION_PASSES: u64 = 0;
const NO_DOCUMENT_UPDATES: usize = 0;
const ONE_DOCUMENT_UPDATE: usize = 1;
const HISTORY_ROUND_TRIPS: usize = 3;
const REMOTE_TEXT: &str = "remote";
const MERGE_ANCHOR_CELL: usize = 0;
const MERGE_HEAD_CELL: usize = 1;
const UNRELATED_CELL_AFTER_MERGE: usize = 4;
const SINGLE_SPAN: u32 = 1;
const SPLIT_CELL: usize = 0;
const MINTED_CELL_AFTER_SPLIT: usize = 1;
const MERGED_TOP_ROW_CELLS: usize = 2;
const UNMERGED_TOP_ROW_CELLS: usize = 3;

fn merge_fixture() -> String {
    json!({
        "type": "doc",
        "content": [table(vec![
            row(vec![cell("a0"), cell("a1"), cell("a2")]),
            row(vec![cell("b0"), cell("b1"), cell("b2")]),
        ])],
    })
    .to_string()
}

fn split_fixture() -> String {
    json!({
        "type": "doc",
        "content": [table(vec![
            row(vec![cell_with(2, SINGLE_SPAN, serde_json::Value::Null, "wide")]),
            row(vec![cell("b0"), cell("b1")]),
        ])],
    })
    .to_string()
}

fn session_from(fixture: String) -> EditorSession {
    let mut session = seeded_session(fixture);
    session.attach_collaboration_runtime();
    session
}

fn local_session() -> EditorSession {
    let mut session = seeded_session(merge_fixture());
    session.attach_collaboration_runtime();
    session
}

fn awaiting_session() -> EditorSession {
    let mut session = DocumentApiFacade::admit(
        EditorSessionConfig {
            schema_json: None,
            fragment_name: COLLABORATION_FRAGMENT_NAME.into(),
            initialization: EditorInitialization::Room {
                scope: DocumentScope {
                    document_id: HISTORY_DOCUMENT_ID.into(),
                    lineage_id: HISTORY_LINEAGE_ID.into(),
                },
                snapshot: None,
            },
            resource_limits: limits(),
            editing_limits: EditingLimits::default(),
            collaboration_limits: CollaborationLimits::default(),
            max_length: None,
            read_only: false,
            input_filter: None,
            allow_base64_images: false,
        },
        schema(),
    )
    .expect("the awaiting replica is admitted");
    session.attach_collaboration_runtime();
    session
}

fn document_json(session: &EditorSession) -> serde_json::Value {
    session.engine.document_json().expect("the engine is ready")
}

fn top_row_cells(session: &EditorSession) -> usize {
    document_json(session)["content"][0]["content"][0]["content"]
        .as_array()
        .expect("the top row holds cells")
        .len()
}

fn encoded_state(session: &EditorSession) -> Vec<u8> {
    session.engine.encoded_state().expect("the state encodes")
}

fn apply(session: &mut EditorSession, command: TableCommand) {
    reset_planned_normalization_passes();
    let (engine, outbox) = session.engine_and_outbox();
    engine
        .apply_command_with_outbox(REQUEST_ID, TypedCommand::Table(command), outbox)
        .unwrap_or_else(|error| panic!("{command:?} applies: {error:?}"))
        .unwrap_or_else(|| panic!("{command:?} produced a transaction"));
}

fn type_into_cell(session: &mut EditorSession, index: usize) {
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
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point,
                head: point,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .expect("the caret lands inside the cell");
    reset_planned_normalization_passes();
    let (engine, outbox) = session.engine_and_outbox();
    engine
        .apply_command_with_outbox(
            REQUEST_ID,
            TypedCommand::InsertText {
                text: REMOTE_TEXT.into(),
            },
            outbox,
        )
        .expect("the remote peer types")
        .expect("typing produced a transaction");
}

fn undo(session: &mut EditorSession) {
    reset_planned_normalization_passes();
    let (engine, outbox) = session.engine_and_outbox();
    engine
        .undo_with_outbox(REQUEST_ID, outbox)
        .expect("the undo applies")
        .expect("the undo produced a commit");
}

fn redo(session: &mut EditorSession) {
    reset_planned_normalization_passes();
    let (engine, outbox) = session.engine_and_outbox();
    engine
        .redo_with_outbox(REQUEST_ID, outbox)
        .expect("the redo applies")
        .expect("the redo produced a commit");
}

fn exchange(from: &mut EditorSession, to: &mut EditorSession) -> usize {
    drain_document_updates(to);
    let state_vector = to
        .engine
        .encode_state_vector_v1(REQUEST_ID)
        .expect("the recipient encodes its state vector");
    let diff = from
        .engine
        .encode_diff_v1(REQUEST_ID, &state_vector)
        .expect("the sender encodes its diff");
    let prepared = to
        .engine
        .prepare_remote_update_v1(REQUEST_ID, &diff)
        .expect("the diff prepares");
    to.engine
        .commit_prepared_remote_update(prepared)
        .expect("the diff commits");
    drain_document_updates(to)
}

#[test]
fn repeated_undo_and_redo_restores_merged_content_without_reusing_the_same_bytes() {
    let mut session = local_session();
    let unmerged = document_json(&session);
    let unmerged_state = encoded_state(&session);
    session_select_rectangle(&mut session, MERGE_ANCHOR_CELL, MERGE_HEAD_CELL);

    apply(&mut session, TableCommand::MergeTableCells);

    let merged = document_json(&session);
    assert_ne!(merged, unmerged, "the merge must actually change the table");
    assert_eq!(top_row_cells(&session), MERGED_TOP_ROW_CELLS);

    for round in 0..HISTORY_ROUND_TRIPS {
        undo(&mut session);
        assert_eq!(
            planned_normalization_passes(),
            NO_NORMALIZATION_PASSES,
            "undo {round} planned a normalization pass",
        );
        assert_eq!(
            document_json(&session),
            unmerged,
            "undo {round} must restore the unmerged content and attributes",
        );

        redo(&mut session);
        assert_eq!(
            planned_normalization_passes(),
            NO_NORMALIZATION_PASSES,
            "redo {round} planned a normalization pass",
        );
        assert_eq!(
            document_json(&session),
            merged,
            "redo {round} must restore the merged content and attributes",
        );
    }

    undo(&mut session);
    assert_eq!(document_json(&session), unmerged);
    assert_ne!(
        encoded_state(&session),
        unmerged_state,
        "restoring content must not be asserted as identical CRDT bytes",
    );
}

#[test]
fn undoing_a_merge_keeps_a_concurrent_remote_edit_and_writes_no_repairs() {
    let mut local = local_session();
    let mut remote = awaiting_session();
    assert_eq!(
        exchange(&mut local, &mut remote),
        NO_DOCUMENT_UPDATES,
        "seeding the awaiting replica must not write a repair",
    );

    session_select_rectangle(&mut local, MERGE_ANCHOR_CELL, MERGE_HEAD_CELL);
    drain_document_updates(&mut local);
    apply(&mut local, TableCommand::MergeTableCells);
    assert_eq!(
        drain_document_updates(&mut local),
        ONE_DOCUMENT_UPDATE,
        "the merge must write one update, or the repair zeroes below prove nothing",
    );
    assert_eq!(
        exchange(&mut local, &mut remote),
        NO_DOCUMENT_UPDATES,
        "receiving the merge must not write a repair",
    );
    assert_eq!(top_row_cells(&remote), MERGED_TOP_ROW_CELLS);

    type_into_cell(&mut remote, UNRELATED_CELL_AFTER_MERGE);
    assert_eq!(
        exchange(&mut remote, &mut local),
        NO_DOCUMENT_UPDATES,
        "receiving the remote text must not write a repair",
    );

    undo(&mut local);
    assert_eq!(planned_normalization_passes(), NO_NORMALIZATION_PASSES);
    assert_eq!(
        top_row_cells(&local),
        UNMERGED_TOP_ROW_CELLS,
        "undo must reverse the local merge",
    );
    assert!(
        document_json(&local).to_string().contains(REMOTE_TEXT),
        "undo must leave the concurrent remote edit alone",
    );

    assert_eq!(exchange(&mut local, &mut remote), NO_DOCUMENT_UPDATES);
    assert_eq!(exchange(&mut remote, &mut local), NO_DOCUMENT_UPDATES);
    assert_eq!(
        document_json(&local),
        document_json(&remote),
        "the replicas must converge after the selective undo",
    );

    redo(&mut local);
    assert_eq!(
        top_row_cells(&local),
        MERGED_TOP_ROW_CELLS,
        "redo must reapply only the local merge",
    );
    assert!(
        document_json(&local).to_string().contains(REMOTE_TEXT),
        "redo must leave the concurrent remote edit alone",
    );
    assert_eq!(exchange(&mut local, &mut remote), NO_DOCUMENT_UPDATES);
    assert_eq!(
        document_json(&local),
        document_json(&remote),
        "the replicas must converge after the selective redo",
    );
}

#[test]
fn undoing_a_split_never_destroys_remote_content_written_into_the_cell_it_minted() {
    let mut local = session_from(split_fixture());
    let mut remote = awaiting_session();
    assert_eq!(exchange(&mut local, &mut remote), NO_DOCUMENT_UPDATES);

    session_select_rectangle(&mut local, SPLIT_CELL, SPLIT_CELL);
    apply(&mut local, TableCommand::SplitTableCell);
    assert_eq!(
        exchange(&mut local, &mut remote),
        NO_DOCUMENT_UPDATES,
        "receiving the split must not write a repair",
    );
    assert_eq!(top_row_cells(&remote), MERGED_TOP_ROW_CELLS);

    type_into_cell(&mut remote, MINTED_CELL_AFTER_SPLIT);
    assert_eq!(
        exchange(&mut remote, &mut local),
        NO_DOCUMENT_UPDATES,
        "receiving the remote text must not write a repair",
    );

    undo(&mut local);
    assert_eq!(planned_normalization_passes(), NO_NORMALIZATION_PASSES);
    assert!(
        document_json(&local).to_string().contains(REMOTE_TEXT),
        "undoing the split must not destroy the peer content inside the cell it minted",
    );

    assert_eq!(exchange(&mut local, &mut remote), NO_DOCUMENT_UPDATES);
    assert_eq!(exchange(&mut remote, &mut local), NO_DOCUMENT_UPDATES);
    assert_eq!(
        document_json(&local),
        document_json(&remote),
        "the replicas must converge after the selective undo",
    );
}
