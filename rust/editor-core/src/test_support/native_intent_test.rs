use crate::boundary::ResourceLimits;
use crate::native_transaction_bridge::NativeTransactionBridge;
use crate::schema::presets::prosemirror_table_schema;
use crate::schema::presets::tiptap_schema;
use crate::session::{
    CollaborationLimits, DocumentState, EditorSession, EditorSessionConfig, SessionPolicy,
};
use crate::tables::commands::{TableCommand, CELL_INTERIOR_OFFSET};
use crate::tables::commands_tests::{
    PROSE_PREFIX_TABLE_POSITION, PROSE_PREFIX_TEXT, TABLE_POSITION,
};
use crate::yrs_engine::{
    Affinity, EditingLimits, EditorOffsetKind, InitializationMode, ReplacementHistory,
    ResolvedSelection, RevisionedPosition, SelectionInput, TransactionOrigin, TypedCommand,
    YrsDocumentEngine, YrsEngineConfig,
};

fn engine(mode: InitializationMode) -> YrsDocumentEngine {
    YrsDocumentEngine::new(YrsEngineConfig {
        schema: tiptap_schema(),
        fragment_name: "prosemirror".into(),
        initialization_mode: mode,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap()
}

fn collaborative_session_with_filter(input_filter: Option<&str>) -> EditorSession {
    let mut config = EditorSessionConfig::local_for_test();
    config.input_filter = input_filter.map(str::to_string);
    let mut session = EditorSession::new(
        engine(InitializationMode::LocalEmpty),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            1,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abcd"}]}]}"#,
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    session.attach_collaboration_runtime();
    session
}

fn collaborative_session() -> EditorSession {
    collaborative_session_with_filter(None)
}

fn scalar(offset: u32) -> RevisionedPosition {
    RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    }
}

fn native_intent_request(
    session: &mut EditorSession,
    owner_id: u64,
    request_id: u64,
    intent: serde_json::Value,
) -> String {
    let epoch = session
        .pin_position_epoch(owner_id, session.engine.revision())
        .unwrap();
    serde_json::json!({
        "version": 1,
        "requestId": request_id.to_string(),
        "ownerId": owner_id.to_string(),
        "positionEpoch": epoch.to_string(),
        "intent": intent,
    })
    .to_string()
}

#[derive(Debug, PartialEq)]
struct SessionAudit {
    document_json: serde_json::Value,
    encoded_state: Vec<u8>,
    state_vector: Vec<u8>,
    document_revision: u64,
    state_revision: u64,
    can_undo: bool,
    can_redo: bool,
    selection: Option<String>,
    stored_marks: Option<String>,
    last_committed_origin: Option<String>,
    outbox_pending_updates: usize,
    outbox_pending_bytes: usize,
    outbox_reserved_messages: usize,
    outbox_reserved_bytes: usize,
    last_reserved_upper_bound: Option<usize>,
}

fn session_audit(session: &EditorSession) -> SessionAudit {
    let outbox = session.collaboration_outbox().unwrap();
    SessionAudit {
        document_json: session.engine.document_json().unwrap(),
        encoded_state: session.engine.encoded_state().unwrap(),
        state_vector: session.engine.encode_state_vector_v1(0).unwrap(),
        document_revision: session.engine.revision(),
        state_revision: session.engine.state_revision(),
        can_undo: session.engine.can_undo(),
        can_redo: session.engine.can_redo(),
        selection: session
            .engine
            .resolved_selection()
            .map(|selection| format!("{selection:?}")),
        stored_marks: session
            .engine
            .stored_marks()
            .map(|marks| format!("{marks:?}")),
        last_committed_origin: session
            .engine
            .last_committed_origin()
            .map(|origin| origin.as_tag().to_string()),
        outbox_pending_updates: outbox.pending_document_update_count(),
        outbox_pending_bytes: outbox.pending_document_update_bytes(),
        outbox_reserved_messages: outbox.reserved_messages(),
        outbox_reserved_bytes: outbox.reserved_bytes(),
        last_reserved_upper_bound: outbox.last_reserved_upper_bound_for_test(),
    }
}

const TWO_CELL_TABLE_DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]}]}]}]}"#;
const PASTE_OWNER_ID: u64 = 81;
const PASTE_REQUEST_ID: u64 = 82;
const PASTE_UNDO_REQUEST_ID: u64 = 83;
const TAB_OWNER_ID: u64 = 71;
const TAB_REQUEST_ID: u64 = 72;
const TAB_STALE_REQUEST_ID: &str = "73";
const TAB_UNDO_REQUEST_ID: u64 = 74;
const FOREIGN_OWNER_ID: u64 = 999;

fn table_engine(mode: InitializationMode) -> YrsDocumentEngine {
    YrsDocumentEngine::new(YrsEngineConfig {
        schema: prosemirror_table_schema(),
        fragment_name: "prosemirror".into(),
        initialization_mode: mode,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap()
}

fn table_session() -> EditorSession {
    table_session_with(TWO_CELL_TABLE_DOCUMENT)
}

fn table_session_with(document: &str) -> EditorSession {
    let config = EditorSessionConfig::local_for_test();
    let mut engine = table_engine(InitializationMode::LocalEmpty);
    engine
        .import_json(document, TransactionOrigin::DocumentImport)
        .unwrap();
    let mut session = EditorSession::new(
        engine,
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session.attach_collaboration_runtime();
    session
}

fn cell_text_scalar(session: &EditorSession, cell: usize) -> u32 {
    cell_text_scalar_in(session, TABLE_POSITION, cell)
}

fn cell_text_scalar_in(session: &EditorSession, table_position: u32, cell: usize) -> u32 {
    let document = session.engine.document().unwrap();
    let index = crate::tables::admission::TableProjectionIndex::derive_or_fallback(
        document,
        &prosemirror_table_schema(),
        &ResourceLimits::default(),
    );
    let opening = index.table_at(table_position).unwrap().cells[cell].source_pos;
    session
        .engine
        .position_map()
        .unwrap()
        .doc_to_scalar(opening + CELL_INTERIOR_OFFSET, document)
}

fn table_row_texts(session: &EditorSession, row: usize) -> Vec<String> {
    session.engine.document_json().unwrap()["content"][0]["content"][row]["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            cell["content"][0]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[test]
fn native_tsv_paste_inside_a_table_applies_as_one_trusted_matrix_paste() {
    let mut session = table_session();
    let scalar = cell_text_scalar(&session, 0);
    let before = session_audit(&session);
    let intent = serde_json::json!({
        "type": "command", "anchor": scalar, "head": scalar,
        "command": {"type": "paste", "text": "x\ty\nz\tw"},
    });
    let request = native_intent_request(&mut session, PASTE_OWNER_ID, PASTE_REQUEST_ID, intent);

    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .unwrap();

    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();
    assert_eq!(outcome["type"], "transaction", "{outcome}");
    assert_eq!(outcome["documentChanged"], true);
    assert_eq!(table_row_texts(&session, 0), vec!["x", "y"]);
    assert_eq!(table_row_texts(&session, 1), vec!["z", "w"]);
    assert_eq!(
        session.engine.last_committed_origin(),
        Some(TransactionOrigin::LocalCommand)
    );
    assert_eq!(
        session
            .collaboration_outbox()
            .unwrap()
            .pending_document_update_count(),
        before.outbox_pending_updates + 1
    );

    let (engine, outbox) = session.engine_and_outbox();
    assert!(engine
        .undo_with_outbox(PASTE_UNDO_REQUEST_ID, outbox)
        .unwrap()
        .is_some());
    assert_eq!(
        session.engine.document_json().unwrap(),
        before.document_json,
        "one undo restores the table the paste replaced"
    );
    assert!(
        !session.engine.can_undo(),
        "the native paste must be a single undo step"
    );
}

#[test]
fn native_table_tab_appends_one_row_with_one_trusted_commit() {
    let mut session = table_session();
    let scalar = cell_text_scalar(&session, 1);
    let before = session_audit(&session);
    let intent = serde_json::json!({
        "type": "command", "anchor": scalar, "head": scalar,
        "command": {"type": "moveToAdjacentCell", "step": "forward", "appendRow": true},
    });
    let request = native_intent_request(&mut session, TAB_OWNER_ID, TAB_REQUEST_ID, intent);

    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();
    assert_eq!(outcome["type"], "transaction");
    assert_eq!(outcome["changed"], true);
    assert_eq!(outcome["documentChanged"], true);
    assert_eq!(session.engine.revision(), before.document_revision + 1);
    assert_eq!(
        session.engine.document_json().unwrap()["content"][0]["content"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        session
            .collaboration_outbox()
            .unwrap()
            .pending_document_update_count(),
        before.outbox_pending_updates + 1
    );
    assert_eq!(
        session.engine.last_committed_origin(),
        Some(TransactionOrigin::LocalCommand)
    );
    assert!(session.engine.can_undo());

    let after = session_audit(&session);
    let mut stale: serde_json::Value = serde_json::from_str(&request).unwrap();
    stale["requestId"] = serde_json::json!(TAB_STALE_REQUEST_ID);
    stale["ownerId"] = serde_json::json!(FOREIGN_OWNER_ID.to_string());
    assert!(NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&stale.to_string())
        .is_err());
    stale["ownerId"] = serde_json::json!(TAB_OWNER_ID.to_string());
    assert!(NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&stale.to_string())
        .is_err());
    assert_eq!(session_audit(&session), after);

    let (engine, outbox) = session.engine_and_outbox();
    assert!(engine
        .undo_with_outbox(TAB_UNDO_REQUEST_ID, outbox)
        .unwrap()
        .is_some());
    assert_eq!(
        session.engine.document_json().unwrap(),
        before.document_json
    );
}

#[test]
fn native_insert_text_applies_input_filter_per_character() {
    let mut session = collaborative_session_with_filter(Some("[0-9]"));
    let request = native_intent_request(
        &mut session,
        31,
        32,
        serde_json::json!({
            "type": "insertText",
            "anchor": 4,
            "head": 4,
            "text": "a1b2",
        }),
    );

    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();

    assert_eq!(outcome["type"], "transaction");
    assert_eq!(outcome["changed"], true);
    assert_eq!(outcome["documentChanged"], true);
    assert_eq!(
        session.engine.document().unwrap().root().text_content(),
        "abcd12"
    );
    assert_eq!(
        session
            .collaboration_outbox()
            .unwrap()
            .pending_document_update_count(),
        1,
    );
}

#[test]
fn native_replace_selection_text_applies_input_filter_per_character() {
    let mut session = collaborative_session_with_filter(Some("[0-9]"));
    let request = native_intent_request(
        &mut session,
        33,
        34,
        serde_json::json!({
            "type": "replaceSelectionText",
            "anchor": 1,
            "head": 3,
            "text": "a1b2",
        }),
    );

    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();

    assert_eq!(outcome["type"], "transaction");
    assert_eq!(outcome["changed"], true);
    assert_eq!(outcome["documentChanged"], true);
    assert_eq!(
        session.engine.document().unwrap().root().text_content(),
        "a12d"
    );
    assert_eq!(
        session
            .collaboration_outbox()
            .unwrap()
            .pending_document_update_count(),
        1,
    );
}

#[test]
fn fully_filtered_native_text_intents_are_atomic_unchanged_transactions() {
    for intent_type in ["insertText", "replaceSelectionText"] {
        let mut session = collaborative_session_with_filter(Some("[0-9]"));
        let request = native_intent_request(
            &mut session,
            35,
            36,
            serde_json::json!({
                "type": intent_type,
                "anchor": 1,
                "head": 3,
                "text": "abc",
            }),
        );
        let before = session_audit(&session);

        let outcome = NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&request)
            .unwrap();
        let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();

        assert_eq!(outcome["type"], "transaction", "{intent_type}");
        assert_eq!(outcome["changed"], false, "{intent_type}");
        assert_eq!(outcome["documentChanged"], false, "{intent_type}");
        assert_eq!(session_audit(&session), before, "{intent_type}");
    }
}

#[test]
fn same_text_native_replacement_reports_selection_change_without_document_change() {
    let mut session = collaborative_session();
    let request = native_intent_request(
        &mut session,
        41,
        42,
        serde_json::json!({
            "type": "replaceSelectionText",
            "anchor": 0,
            "head": 4,
            "text": "abcd",
        }),
    );
    let revision_before = session.engine.revision();
    let outbox_before = session
        .collaboration_outbox()
        .unwrap()
        .pending_document_update_count();

    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();

    assert_eq!(outcome["type"], "transaction");
    assert_eq!(outcome["changed"], true);
    assert_eq!(outcome["documentChanged"], false);
    assert_eq!(session.engine.revision(), revision_before);
    assert_eq!(
        session
            .collaboration_outbox()
            .unwrap()
            .pending_document_update_count(),
        outbox_before,
    );
}

#[test]
fn invalid_native_text_intent_filter_is_atomic() {
    for intent_type in ["insertText", "replaceSelectionText"] {
        let mut session = collaborative_session_with_filter(Some("[unclosed"));
        let owner_id = 37;
        let request_id = 38;
        let request = native_intent_request(
            &mut session,
            owner_id,
            request_id,
            serde_json::json!({
                "type": intent_type,
                "anchor": 1,
                "head": 3,
                "text": "a1b2",
            }),
        );
        let before = session_audit(&session);

        let error = NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&request)
            .unwrap_err();

        assert_eq!(error.code, "CONFIG_INVALID", "{intent_type}");
        assert_eq!(error.request_id, Some(request_id), "{intent_type}");
        assert_eq!(session_audit(&session), before, "{intent_type}");
        assert!(
            session
                .native_request_outcome(owner_id, request_id)
                .unwrap()
                .is_none(),
            "{intent_type}",
        );
    }
}

#[test]
fn native_input_filter_does_not_affect_non_text_intents() {
    let mut session = collaborative_session_with_filter(Some("[unclosed"));
    let request = native_intent_request(
        &mut session,
        39,
        40,
        serde_json::json!({
            "type": "deleteRange",
            "anchor": 1,
            "head": 2,
        }),
    );

    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();

    assert_eq!(outcome["type"], "transaction");
    assert_eq!(outcome["changed"], true);
    assert_eq!(
        session.engine.document().unwrap().root().text_content(),
        "acd"
    );
}

#[test]
fn command_uses_resolved_selection_without_an_intermediate_caret_commit() {
    let mut session = collaborative_session();
    let epoch = session
        .pin_position_epoch(11, session.engine.revision())
        .unwrap();
    let mut replica = engine(InitializationMode::AwaitRemote);
    replica
        .apply_remote_update_v1(2, &session.engine.encoded_state().unwrap())
        .unwrap();
    replica
        .apply_command(3, TypedCommand::InsertText { text: "R".into() })
        .unwrap();
    session
        .engine
        .apply_remote_update_v1(4, &replica.encoded_state().unwrap())
        .unwrap();
    replica
        .apply_command(5, TypedCommand::InsertText { text: "S".into() })
        .unwrap();
    session
        .engine
        .apply_remote_update_v1(6, &replica.encoded_state().unwrap())
        .unwrap();
    let resolved = session.resolve_epoch_range(11, epoch, 2, 2).unwrap();
    let revision_before = session.engine.revision();
    let state_revision_before = session.engine.state_revision();
    let outbox_before = session
        .collaboration_outbox()
        .unwrap()
        .pending_document_update_count();
    let selection = SelectionInput::Text {
        anchor: scalar(resolved.anchor),
        head: scalar(resolved.head),
    };

    let (engine, outbox) = session.engine_and_outbox();
    engine
        .apply_command_at_selection_with_outbox(
            7,
            TypedCommand::InsertText { text: "X".into() },
            selection,
            TransactionOrigin::LocalInput,
            outbox,
        )
        .unwrap();

    assert_eq!(session.engine.revision(), revision_before + 1);
    assert_eq!(session.engine.state_revision(), state_revision_before + 1);
    assert_eq!(
        session
            .collaboration_outbox()
            .unwrap()
            .pending_document_update_count(),
        outbox_before + 1,
    );
    assert_eq!(
        session.engine.document().unwrap().root().text_content(),
        "RSabXcd",
    );
}

#[test]
fn split_block_uses_the_pinned_multi_paragraph_scalar() {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        engine(InitializationMode::LocalEmpty),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            1,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]},{"type":"paragraph","content":[{"type":"text","text":"Beta"}]},{"type":"paragraph","content":[{"type":"text","text":"Gamma"}]}]}"#,
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let epoch = session
        .pin_position_epoch(19, session.engine.revision())
        .unwrap();
    let request = serde_json::json!({
        "version": 1,
        "requestId": "2",
        "ownerId": "19",
        "positionEpoch": epoch.to_string(),
        "intent": {"type": "splitBlock", "anchor": 10, "head": 10},
    });

    NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request.to_string())
        .unwrap();

    assert_eq!(
        session.engine.document_html().unwrap(),
        "<p>Alpha</p><p>Beta</p><p></p><p>Gamma</p>",
    );
}

#[test]
fn repeated_split_after_deleting_a_lists_trailing_paragraph_creates_blank_lines() {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        engine(InitializationMode::LocalEmpty),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            1,
            r#"{"type":"doc","content":[{"type":"blockquote","content":[{"type":"paragraph","content":[{"type":"text","text":"quote"}]}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"nested"}]}]}]}]}]},{"type":"paragraph"}]}"#,
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let trailing_paragraph = session.engine.position_map().unwrap().total_scalars();
    let delete = native_intent_request(
        &mut session,
        20,
        2,
        serde_json::json!({
            "type": "deleteBackward",
            "anchor": trailing_paragraph,
            "head": trailing_paragraph,
        }),
    );
    NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&delete)
        .unwrap();

    for request_id in 3..=6 {
        let ResolvedSelection::Text { anchor, head } = session.engine.resolved_selection().unwrap()
        else {
            panic!("the caret must remain a text selection");
        };
        assert_eq!(anchor.scalar, head.scalar);
        let scalar = head.scalar;
        let split = native_intent_request(
            &mut session,
            20,
            request_id,
            serde_json::json!({
                "type": "splitBlock",
                "anchor": scalar,
                "head": scalar,
            }),
        );
        NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&split)
            .unwrap();
    }

    assert_eq!(
        session.engine.document_html().unwrap(),
        "<blockquote><p>quote</p></blockquote><ul><li><p>one</p></li><li><p>two</p><ul><li><p>nested</p></li></ul></li></ul><p></p><p></p>",
    );
}

#[test]
fn set_selection_uses_the_pinned_multi_paragraph_scalar() {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        engine(InitializationMode::LocalEmpty),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            1,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]},{"type":"paragraph","content":[{"type":"text","text":"Beta"}]},{"type":"paragraph","content":[{"type":"text","text":"Gamma"}]}]}"#,
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let epoch = session
        .pin_position_epoch(23, session.engine.revision())
        .unwrap();
    let request = serde_json::json!({
        "version": 1,
        "requestId": "2",
        "ownerId": "23",
        "positionEpoch": epoch.to_string(),
        "intent": {"type": "setSelection", "anchor": 10, "head": 10},
    });

    NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request.to_string())
        .unwrap();

    assert!(matches!(
        session.engine.resolved_selection(),
        Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head })
            if anchor.scalar == 10 && head.scalar == 10
    ));
}

const PROSE_AND_GRID_DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Third"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Fourth"}]}]}]}]}]}"#;
const CELL_OWNER_ID: u64 = 91;
const CELL_COMMIT_REQUEST_ID: u64 = 92;
const REMOTE_REQUEST_ID: u64 = 93;
const REMOTE_DELIVERY_REQUEST_ID: u64 = 94;
const FIRST_GRID_CELL: usize = 0;
const COMPOSED_TEXT: &str = "Q";
const REMOTE_PROSE_TEXT: &str = "R";
const CELL_REMOVED_CODE: &str = "POSITION_EPOCH_CELL_REMOVED";
const TWO_PARAGRAPH_CELL_DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]},{"type":"paragraph","content":[{"type":"text","text":"Extra"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]}]}]}]}"#;
const THREE_PARAGRAPH_CELL_DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]},{"type":"paragraph","content":[{"type":"text","text":"Extra"}]},{"type":"paragraph","content":[{"type":"text","text":"Third"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]}]}]}]}"#;
const EMPTY_MIDDLE_PARAGRAPH_CELL_DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]},{"type":"paragraph"},{"type":"paragraph","content":[{"type":"text","text":"Third"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]}]}]}]}"#;
const FIRST_PARAGRAPH_TEXT: &str = "First";
const SECOND_PARAGRAPH_TEXT: &str = "Extra";
const THIRD_PARAGRAPH_TEXT: &str = "Third";
const SPLIT_HEAD_TEXT: &str = "Fir";
const SPLIT_TAIL_TEXT: &str = "st";
const HEADER_CELL_NODE: &str = "table_header";
const PARAGRAPH_NODE: &str = "paragraph";
const HEADING_NODE: &str = "heading";
const HEADING_LEVEL: u8 = 1;
const EMPTY_CELLS_DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph"}]},{"type":"table_cell","content":[{"type":"paragraph"}]}]}]}]}"#;
const PARAGRAPH_BREAK_SCALARS: u32 = 1;

struct PinnedCellCommit {
    epoch: u64,
    scalar: u32,
}

fn pin_cell_commit(session: &mut EditorSession) -> PinnedCellCommit {
    let scalar = cell_text_scalar_in(session, PROSE_PREFIX_TABLE_POSITION, FIRST_GRID_CELL);
    let epoch = session
        .pin_position_epoch(CELL_OWNER_ID, session.engine.revision())
        .unwrap();
    PinnedCellCommit { epoch, scalar }
}

fn submit_cell_commit(
    session: &mut EditorSession,
    pinned: &PinnedCellCommit,
) -> Result<serde_json::Value, crate::session::SessionError> {
    let request = serde_json::json!({
        "version": 1,
        "requestId": CELL_COMMIT_REQUEST_ID.to_string(),
        "ownerId": CELL_OWNER_ID.to_string(),
        "positionEpoch": pinned.epoch.to_string(),
        "intent": {
            "type": "insertText",
            "anchor": pinned.scalar,
            "head": pinned.scalar,
            "text": COMPOSED_TEXT,
        },
    });
    NativeTransactionBridge::new(session)
        .submit_native_intent(&request.to_string())
        .map(|outcome| serde_json::from_str(&outcome).unwrap())
}

fn apply_remote_peer_edit(session: &mut EditorSession, edit: impl FnOnce(&mut YrsDocumentEngine)) {
    let mut replica = table_engine(InitializationMode::AwaitRemote);
    replica
        .apply_remote_update_v1(REMOTE_REQUEST_ID, &session.engine.encoded_state().unwrap())
        .unwrap();
    edit(&mut replica);
    session
        .engine
        .apply_remote_update_v1(
            REMOTE_DELIVERY_REQUEST_ID,
            &replica.encoded_state().unwrap(),
        )
        .unwrap();
}

fn remote_command_at(
    replica: &mut YrsDocumentEngine,
    anchor: u32,
    head: u32,
    command: TypedCommand,
) {
    replica
        .apply_command_at_selection_with_outbox(
            REMOTE_REQUEST_ID,
            command,
            SelectionInput::Text {
                anchor: scalar(anchor),
                head: scalar(head),
            },
            TransactionOrigin::LocalCommand,
            None,
        )
        .unwrap()
        .expect("the remote peer's edit applies");
}

fn first_grid_cell_text(session: &EditorSession) -> serde_json::Value {
    session.engine.document_json().unwrap()["content"][1]["content"][0]["content"][0]["content"][0]
        ["content"][0]["text"]
        .clone()
}

fn assert_cell_commit_refused_without_mutation(
    session: &mut EditorSession,
    pinned: &PinnedCellCommit,
) {
    let remote = session_audit(session);
    let error = submit_cell_commit(session, pinned)
        .expect_err("a commit anchored in a removed cell must be refused");
    assert_eq!(error.code, CELL_REMOVED_CODE, "{error:?}");
    assert_eq!(error.request_id, Some(CELL_COMMIT_REQUEST_ID));
    assert_eq!(
        session_audit(session),
        remote,
        "the refused commit must leave the remote result untouched"
    );
    assert!(
        !session
            .engine
            .document_json()
            .unwrap()
            .to_string()
            .contains(COMPOSED_TEXT),
        "the composed text must not land anywhere: {}",
        session.engine.document_json().unwrap()
    );
}

#[test]
fn cell_commit_after_a_remote_table_deletion_is_refused_instead_of_landing_in_prose() {
    let mut session = table_session_with(PROSE_AND_GRID_DOCUMENT);
    let pinned = pin_cell_commit(&mut session);

    apply_remote_peer_edit(&mut session, |replica| {
        replica
            .apply_command(
                REMOTE_REQUEST_ID,
                TypedCommand::Table(TableCommand::DeleteTable {
                    table_pos: Some(PROSE_PREFIX_TABLE_POSITION),
                }),
            )
            .unwrap()
            .expect("the remote peer deletes the table");
    });
    assert_eq!(
        session.engine.document_json().unwrap()["content"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "only the prose paragraph survives the remote deletion"
    );

    assert_cell_commit_refused_without_mutation(&mut session, &pinned);
}

#[test]
fn cell_commit_after_a_remote_deletion_of_its_row_is_refused() {
    let mut session = table_session_with(PROSE_AND_GRID_DOCUMENT);
    let pinned = pin_cell_commit(&mut session);

    apply_remote_peer_edit(&mut session, |replica| {
        remote_command_at(
            replica,
            pinned.scalar,
            pinned.scalar,
            TypedCommand::Table(TableCommand::DeleteTableRows),
        );
    });
    assert_eq!(
        first_grid_cell_text(&session),
        THIRD_PARAGRAPH_TEXT,
        "the remote peer removed the composing cell's row"
    );

    assert_cell_commit_refused_without_mutation(&mut session, &pinned);
}

#[test]
fn cell_commit_after_a_remote_edit_elsewhere_lands_in_its_moved_cell() {
    let mut session = table_session_with(PROSE_AND_GRID_DOCUMENT);
    let pinned = pin_cell_commit(&mut session);
    apply_remote_peer_edit(&mut session, |replica| {
        remote_command_at(
            replica,
            0,
            0,
            TypedCommand::InsertText {
                text: REMOTE_PROSE_TEXT.into(),
            },
        );
    });
    let remote_revision = session.engine.revision();

    let outcome = submit_cell_commit(&mut session, &pinned).unwrap();

    assert_eq!(outcome["type"], "transaction", "{outcome}");
    assert_eq!(outcome["positionFallback"], false, "{outcome}");
    assert_eq!(session.engine.revision(), remote_revision + 1);
    assert_eq!(
        session.engine.document_json().unwrap()["content"][0]["content"][0]["text"],
        format!("{REMOTE_PROSE_TEXT}{PROSE_PREFIX_TEXT}")
    );
    assert_eq!(
        first_grid_cell_text(&session),
        format!("{COMPOSED_TEXT}{FIRST_PARAGRAPH_TEXT}")
    );
}

#[test]
fn cell_commit_after_a_remote_header_toggle_of_its_cell_stays_in_that_cell() {
    let mut session = table_session_with(PROSE_AND_GRID_DOCUMENT);
    let pinned = pin_cell_commit(&mut session);
    apply_remote_peer_edit(&mut session, |replica| {
        remote_command_at(
            replica,
            pinned.scalar,
            pinned.scalar,
            TypedCommand::Table(TableCommand::ToggleTableHeader {
                target: crate::tables::commands::TableHeaderTarget::Cell,
            }),
        );
    });
    let remote_revision = session.engine.revision();
    let remote_cell =
        session.engine.document_json().unwrap()["content"][1]["content"][0]["content"][0].clone();
    assert_eq!(remote_cell["type"], HEADER_CELL_NODE, "{remote_cell}");

    let outcome = submit_cell_commit(&mut session, &pinned);

    let outcome = outcome.expect("the retyped cell is the same logical cell");
    assert_eq!(outcome["type"], "transaction", "{outcome}");
    assert_eq!(session.engine.revision(), remote_revision + 1);
    let cell =
        session.engine.document_json().unwrap()["content"][1]["content"][0]["content"][0].clone();
    assert_eq!(cell["type"], HEADER_CELL_NODE, "{cell}");
    assert_eq!(
        first_grid_cell_text(&session),
        format!("{COMPOSED_TEXT}{FIRST_PARAGRAPH_TEXT}"),
        "{outcome}"
    );
}

#[test]
fn cell_commit_after_a_remote_deletion_of_its_empty_column_is_not_retargeted_to_the_identical_neighbor(
) {
    let mut session = table_session_with(EMPTY_CELLS_DOCUMENT);
    let pinned = pin_cell_commit(&mut session);

    apply_remote_peer_edit(&mut session, |replica| {
        remote_command_at(
            replica,
            pinned.scalar,
            pinned.scalar,
            TypedCommand::Table(TableCommand::DeleteTableColumns),
        );
    });
    assert_eq!(
        session.engine.document_json().unwrap()["content"][1]["content"][0]["content"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "the empty neighbor now occupies the composing cell's grid slot"
    );

    assert_cell_commit_refused_without_mutation(&mut session, &pinned);
}

#[test]
fn cell_commit_after_a_remote_peer_splits_its_paragraph_at_the_composition_keeps_its_live_text_leaf(
) {
    let mut session = table_session_with(PROSE_AND_GRID_DOCUMENT);
    let cell_start = cell_text_scalar_in(&session, PROSE_PREFIX_TABLE_POSITION, FIRST_GRID_CELL);
    let epoch = session
        .pin_position_epoch(CELL_OWNER_ID, session.engine.revision())
        .unwrap();
    let pinned = PinnedCellCommit {
        epoch,
        scalar: cell_start + SPLIT_HEAD_TEXT.chars().count() as u32,
    };
    apply_remote_peer_edit(&mut session, |replica| {
        remote_command_at(
            replica,
            pinned.scalar,
            pinned.scalar,
            TypedCommand::SplitBlock,
        );
    });
    let remote_revision = session.engine.revision();

    let outcome = submit_cell_commit(&mut session, &pinned).unwrap();

    assert_eq!(outcome["type"], "transaction", "{outcome}");
    assert_eq!(
        outcome["positionFallback"], false,
        "the split keeps the head text leaf, so no fallback is involved: {outcome}"
    );
    assert_eq!(session.engine.revision(), remote_revision + 1);
    let document = session.engine.document_json().unwrap();
    assert_eq!(
        document["content"][1]["content"][0]["content"][0]["content"],
        serde_json::json!([
            paragraph(&format!("{SPLIT_HEAD_TEXT}{COMPOSED_TEXT}")),
            paragraph(SPLIT_TAIL_TEXT),
        ]),
        "{outcome} {document}"
    );
}

#[derive(Clone, Copy)]
struct CellCaret {
    cell_start: u32,
    scalar: u32,
}

fn end_of_first_paragraph() -> u32 {
    FIRST_PARAGRAPH_TEXT.chars().count() as u32
}

fn start_of_second_paragraph() -> u32 {
    end_of_first_paragraph() + PARAGRAPH_BREAK_SCALARS
}

fn end_of_second_paragraph() -> u32 {
    start_of_second_paragraph() + SECOND_PARAGRAPH_TEXT.chars().count() as u32
}

fn cell_after_remote_edit(
    document: &str,
    caret_in_cell: u32,
    edit: impl FnOnce(&mut YrsDocumentEngine, CellCaret),
) -> serde_json::Value {
    let mut session = table_session_with(document);
    let cell_start = cell_text_scalar_in(&session, PROSE_PREFIX_TABLE_POSITION, FIRST_GRID_CELL);
    let caret = CellCaret {
        cell_start,
        scalar: cell_start + caret_in_cell,
    };
    let pinned = PinnedCellCommit {
        epoch: session
            .pin_position_epoch(CELL_OWNER_ID, session.engine.revision())
            .unwrap(),
        scalar: caret.scalar,
    };
    apply_remote_peer_edit(&mut session, |replica| edit(replica, caret));
    let remote_revision = session.engine.revision();
    let remote_cell =
        session.engine.document_json().unwrap()["content"][1]["content"][0]["content"][0].clone();

    let outcome = submit_cell_commit(&mut session, &pinned).unwrap();

    assert_eq!(outcome["type"], "transaction", "{outcome}");
    assert_eq!(
        outcome["positionFallback"], true,
        "the remote edit recreated the composing text leaf: {outcome}; remote cell {remote_cell}"
    );
    assert_eq!(session.engine.revision(), remote_revision + 1);
    let document = session.engine.document_json().unwrap();
    assert_eq!(
        document["content"][0]["content"][0]["text"],
        PROSE_PREFIX_TEXT
    );
    document["content"][1]["content"][0]["content"][0].clone()
}

fn toggle_header_of_the_composing_cell(replica: &mut YrsDocumentEngine, caret: CellCaret) {
    remote_command_at(
        replica,
        caret.scalar,
        caret.scalar,
        TypedCommand::Table(TableCommand::ToggleTableHeader {
            target: crate::tables::commands::TableHeaderTarget::Cell,
        }),
    );
}

fn join_paragraph_into_previous(
    paragraph_start_in_cell: u32,
) -> impl FnOnce(&mut YrsDocumentEngine, CellCaret) {
    move |replica, caret| {
        let paragraph_start = caret.cell_start + paragraph_start_in_cell;
        remote_command_at(
            replica,
            paragraph_start,
            paragraph_start,
            TypedCommand::DeleteBackward,
        );
    }
}

fn text_block(node_type: &str, text: &str) -> serde_json::Value {
    serde_json::json!({"type": node_type, "content": [{"type": "text", "text": text}]})
}

fn paragraph(text: &str) -> serde_json::Value {
    text_block(PARAGRAPH_NODE, text)
}

#[test]
fn cell_commit_after_a_remote_peer_joins_its_paragraph_stays_in_that_cell() {
    let cell = cell_after_remote_edit(
        TWO_PARAGRAPH_CELL_DOCUMENT,
        start_of_second_paragraph(),
        join_paragraph_into_previous(start_of_second_paragraph()),
    );

    assert_eq!(
        cell["content"],
        serde_json::json!([paragraph(&format!(
            "{FIRST_PARAGRAPH_TEXT}{COMPOSED_TEXT}{SECOND_PARAGRAPH_TEXT}"
        ))]),
        "the composed text stays at the joined seam: {cell}"
    );
}

#[test]
fn cell_commit_at_the_end_of_a_joined_paragraph_stays_before_the_next_paragraph() {
    let cell = cell_after_remote_edit(
        THREE_PARAGRAPH_CELL_DOCUMENT,
        end_of_second_paragraph(),
        join_paragraph_into_previous(start_of_second_paragraph()),
    );

    assert_eq!(
        cell["content"],
        serde_json::json!([
            paragraph(&format!(
                "{FIRST_PARAGRAPH_TEXT}{SECOND_PARAGRAPH_TEXT}{COMPOSED_TEXT}"
            )),
            paragraph(THIRD_PARAGRAPH_TEXT),
        ]),
        "{cell}"
    );
}

#[test]
fn cell_commit_in_an_empty_paragraph_joined_backward_lands_at_the_end_of_the_previous_paragraph() {
    let cell = cell_after_remote_edit(
        EMPTY_MIDDLE_PARAGRAPH_CELL_DOCUMENT,
        start_of_second_paragraph(),
        join_paragraph_into_previous(start_of_second_paragraph()),
    );

    assert_eq!(
        cell["content"],
        serde_json::json!([
            paragraph(&format!("{FIRST_PARAGRAPH_TEXT}{COMPOSED_TEXT}")),
            paragraph(THIRD_PARAGRAPH_TEXT),
        ]),
        "{cell}"
    );
}

#[test]
fn cell_commit_at_the_end_of_a_paragraph_stays_there_after_a_remote_header_toggle() {
    let cell = cell_after_remote_edit(
        TWO_PARAGRAPH_CELL_DOCUMENT,
        end_of_first_paragraph(),
        toggle_header_of_the_composing_cell,
    );

    assert_eq!(cell["type"], HEADER_CELL_NODE, "{cell}");
    assert_eq!(
        cell["content"],
        serde_json::json!([
            paragraph(&format!("{FIRST_PARAGRAPH_TEXT}{COMPOSED_TEXT}")),
            paragraph(SECOND_PARAGRAPH_TEXT),
        ])
    );
}

#[test]
fn cell_commit_at_the_start_of_a_paragraph_stays_there_after_a_remote_header_toggle() {
    let cell = cell_after_remote_edit(
        TWO_PARAGRAPH_CELL_DOCUMENT,
        start_of_second_paragraph(),
        toggle_header_of_the_composing_cell,
    );

    assert_eq!(cell["type"], HEADER_CELL_NODE, "{cell}");
    assert_eq!(
        cell["content"],
        serde_json::json!([
            paragraph(FIRST_PARAGRAPH_TEXT),
            paragraph(&format!("{COMPOSED_TEXT}{SECOND_PARAGRAPH_TEXT}")),
        ])
    );
}

#[test]
fn cell_commit_at_the_end_of_a_paragraph_stays_there_after_a_remote_block_type_change() {
    let cell = cell_after_remote_edit(
        TWO_PARAGRAPH_CELL_DOCUMENT,
        end_of_first_paragraph(),
        |replica, caret| {
            remote_command_at(
                replica,
                caret.scalar,
                caret.scalar,
                TypedCommand::ToggleHeading {
                    level: HEADING_LEVEL,
                },
            );
        },
    );
    let mut heading = text_block(
        HEADING_NODE,
        &format!("{FIRST_PARAGRAPH_TEXT}{COMPOSED_TEXT}"),
    );
    heading["attrs"] = serde_json::json!({"level": HEADING_LEVEL});

    assert_eq!(
        cell["content"],
        serde_json::json!([heading, paragraph(SECOND_PARAGRAPH_TEXT)])
    );
}
