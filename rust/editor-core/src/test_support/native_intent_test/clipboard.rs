use super::*;
use crate::test_support::prose_corpus::{
    prose_paste_text, ProseCorpus, PROSE_TITLE_PREFIX, SMALL_PROSE,
};

#[test]
fn native_large_plain_text_paste_preserves_shared_rich_corpus_with_default_limits() {
    const OWNER: u64 = 18_101;
    const REQUEST: u64 = 18_102;
    const LARGE_PASTE_UTF16_LENGTH: usize = 65_536;
    const TARGET_BLOCK: usize = 4;
    let corpus = ProseCorpus::new(PROSE_TITLE_PREFIX, SMALL_PROSE);
    let text = prose_paste_text(LARGE_PASTE_UTF16_LENGTH);
    assert_eq!(text.encode_utf16().count(), LARGE_PASTE_UTF16_LENGTH);
    assert!(
        text.contains(crate::clipboard::LINE_BREAK),
        "the payload must span many pasted lines"
    );
    let text = text.as_str();
    let mut session = collaborative_session();
    session
        .replace_document_json(
            REQUEST - 1,
            &corpus.document_json().to_string(),
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let before = session_audit(&session);
    let target = TARGET_BLOCK;
    let position_map = session.engine.position_map().unwrap();
    let caret = position_map.effective_scalar_start(target)
        + position_map.block(target).unwrap().scalar_prefix_len;
    let planned = crate::command_planner::plan_replace_selection_text(
        session.engine.document().unwrap(),
        &session.engine.schema(),
        &crate::selection::Selection::cursor(
            position_map.scalar_to_doc(caret, session.engine.document().unwrap()),
        ),
        None,
        text,
        session.engine.resource_limits(),
        session
            .engine
            .editing_limits()
            .max_operations_per_transaction,
    )
    .unwrap()
    .unwrap();
    assert!(
        matches!(
            planned.operations.as_slice(),
            [crate::command_planner::SemanticOperation::ReplaceRange { .. }]
        ),
        "the entire native clipboard payload must lower to one structural replacement"
    );
    let request = native_intent_request(
        &mut session,
        OWNER,
        REQUEST,
        serde_json::json!({
            "type": "command", "anchor": caret, "head": caret,
            "command": {"type": "paste", "text": text},
        }),
    );
    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .expect("the complete 65,536 UTF-16 clipboard payload must fit the default transaction operation limit");
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();
    assert_eq!(outcome["documentChanged"], true, "{outcome}");
    assert_eq!(session.engine.revision(), before.document_revision + 1);

    let mut expected = before.document_json.clone();
    let destination = &before.document_json["content"][target];
    let parts: Vec<&str> = text.split(crate::clipboard::LINE_BREAK).collect();
    let mut replacement = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        let mut paragraph = destination.clone();
        if index + 1 == parts.len() {
            let original = paragraph["content"][0]["text"].as_str().unwrap();
            paragraph["content"][0]["text"] = serde_json::json!(format!("{part}{original}"));
        } else {
            paragraph["content"] = serde_json::json!([{"type":"text","text":part}]);
        }
        replacement.push(paragraph);
    }
    expected["content"]
        .as_array_mut()
        .unwrap()
        .splice(target..=target, replacement);
    assert_eq!(session.engine.document_json().unwrap(), expected,
        "paste must preserve every inserted line and all existing rich text, links, attributes, and siblings");
    let Some(ResolvedSelection::Text { anchor, head }) = session.engine.resolved_selection() else {
        panic!("plain text paste must finish with a text caret");
    };
    let expected_scalar = caret + text.chars().count() as u32;
    assert_eq!(anchor.scalar, expected_scalar);
    assert_eq!(head.scalar, expected_scalar);
    let outbox = session.collaboration_outbox().unwrap();
    assert_eq!(
        outbox.pending_document_update_count(),
        before.outbox_pending_updates + 1
    );
    assert_eq!(outbox.reserved_messages(), 0);
    assert_eq!(outbox.reserved_bytes(), 0);

    let (engine, outbox) = session.engine_and_outbox();
    assert!(engine
        .undo_with_outbox(REQUEST + 1, outbox)
        .unwrap()
        .is_some());
    assert_eq!(
        session.engine.document_json().unwrap(),
        before.document_json
    );
    assert!(
        !session.engine.can_undo(),
        "paste must be exactly one undo boundary"
    );
    let (engine, outbox) = session.engine_and_outbox();
    assert!(engine
        .redo_with_outbox(REQUEST + 2, outbox)
        .unwrap()
        .is_some());
    assert_eq!(session.engine.document_json().unwrap(), expected);
}

#[test]
fn native_multiline_paste_preserves_marks_and_structure_in_nested_and_opaque_contexts() {
    const OWNER: u64 = 18_201;
    const REQUEST: u64 = 18_202;
    const FROM: u32 = 3;
    const TO: u32 = 4;
    const LINE: &str = "X😀\n";
    const LAST_LINE: &str = "last";
    let paragraph = serde_json::json!({"type":"paragraph","content":[
        {"type":"text","text":"ab"},
        {"type":"text","text":"cd","marks":[{"type":"bold"}]},
        {"type":"text","text":"ef","marks":[{"type":"italic"}]}
    ]});
    let mut heading = paragraph.clone();
    heading["type"] = serde_json::json!("heading");
    heading["attrs"] = serde_json::json!({"level":2});
    let opaque = serde_json::json!({"type":"horizontalRule","attrs":{"future":"preserved"}});
    let cases = [
        (
            "heading",
            serde_json::json!({"type":"doc","content":[heading]}),
            "/content",
            0,
            0,
        ),
        (
            "quote",
            serde_json::json!({"type":"doc","content":[{"type":"blockquote","content":[paragraph.clone()]}]}),
            "/content/0/content",
            0,
            0,
        ),
        (
            "nested-list",
            serde_json::json!({"type":"doc","content":[{"type":"blockquote","content":[{"type":"bulletList","content":[{"type":"listItem","content":[paragraph.clone()]}]}]}]}),
            "/content/0/content/0/content/0/content",
            0,
            0,
        ),
        (
            "table-cell",
            serde_json::json!({"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[paragraph.clone()]}]}]}]}),
            "/content/0/content/0/content/0/content",
            0,
            0,
        ),
        (
            "opaque-before",
            serde_json::json!({"type":"doc","content":[opaque.clone(),paragraph.clone()]}),
            "/content",
            1,
            1,
        ),
        (
            "opaque-after",
            serde_json::json!({"type":"doc","content":[paragraph.clone(),opaque]}),
            "/content",
            0,
            0,
        ),
    ];
    let repetitions = EditingLimits::default().max_operations_per_transaction;
    let text = format!("{}{LAST_LINE}", LINE.repeat(repetitions));
    for (name, document, container_pointer, child_index, block_index) in cases {
        let mut session = match name {
            "table-cell" => {
                table_session_with_schema(&document.to_string(), prosemirror_table_schema())
            }
            "opaque-before" | "opaque-after" => {
                let mut session = default_schema_session(&document.to_string());
                session.attach_collaboration_runtime();
                session
            }
            _ => table_session_with_schema(&document.to_string(), tiptap_schema()),
        };
        let before = session_audit(&session);
        let position_map = session.engine.position_map().unwrap();
        let start = position_map.effective_scalar_start(block_index)
            + position_map.block(block_index).unwrap().scalar_prefix_len;
        let request = native_intent_request(
            &mut session,
            OWNER,
            REQUEST,
            serde_json::json!({
                "type":"command", "anchor":start + FROM, "head":start + TO,
                "command":{"type":"paste", "text":text},
            }),
        );
        let outcome = NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&request)
            .unwrap_or_else(|error| panic!("{name} multiline paste must commit once: {error:?}"));
        let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();
        assert_eq!(outcome["documentChanged"], true, "{name}: {outcome}");
        let mut expected = before.document_json.clone();
        let destination = &before.document_json.pointer(container_pointer).unwrap()[child_index];
        let mut first = destination.clone();
        first["content"] = serde_json::json!([
            {"type":"text","text":"ab"},
            {"type":"text","text":"cX😀","marks":[{"type":"bold"}]}
        ]);
        let mut inserted = vec![first];
        inserted.extend((1..repetitions).map(|_| serde_json::json!({
            "type":"paragraph","content":[{"type":"text","text":"X😀","marks":[{"type":"bold"}]}]
        })));
        inserted.push(serde_json::json!({"type":"paragraph","content":[
            {"type":"text","text":LAST_LINE,"marks":[{"type":"bold"}]},
            destination["content"][2].clone()
        ]}));
        expected
            .pointer_mut(container_pointer)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .splice(child_index..=child_index, inserted);
        assert_eq!(session.engine.document_json().unwrap(), expected,
            "{name}: only the selected marked text may change; every ancestor and opaque sibling must survive");
        let Some(ResolvedSelection::Text { anchor, head }) = session.engine.resolved_selection()
        else {
            panic!("{name} paste must finish at a text caret");
        };
        assert_eq!(
            anchor.scalar,
            start + FROM + text.chars().count() as u32,
            "{name}"
        );
        assert_eq!(head.scalar, anchor.scalar, "{name}");
        assert_eq!(
            session.engine.revision(),
            before.document_revision + 1,
            "{name}"
        );
        let (engine, outbox) = session.engine_and_outbox();
        assert!(
            engine
                .undo_with_outbox(REQUEST + 1, outbox)
                .unwrap()
                .is_some(),
            "{name}"
        );
        assert_eq!(
            session.engine.document_json().unwrap(),
            before.document_json,
            "{name}"
        );
        let (engine, outbox) = session.engine_and_outbox();
        assert!(
            engine
                .redo_with_outbox(REQUEST + 2, outbox)
                .unwrap()
                .is_some(),
            "{name}"
        );
        assert_eq!(session.engine.document_json().unwrap(), expected, "{name}");
    }
}

#[test]
fn native_coalesced_paste_retains_output_and_collaboration_admission_atomicity() {
    const OWNER: u64 = 18_301;
    const REQUEST: u64 = 18_302;
    const DERIVED_LIMIT: usize = 8 * 1024;
    const OUTBOX_LIMIT: usize = 1;
    const LINE: &str = "rich 文😀 clipboard line\n";
    const DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"keep"}]}]}"#;
    let text = LINE.repeat(EditingLimits::default().max_operations_per_transaction);
    for (field, limit, editing_limits, collaboration_limits) in [
        (
            "maxDerivedOutputBytes",
            DERIVED_LIMIT,
            EditingLimits {
                max_derived_output_bytes: DERIVED_LIMIT,
                ..EditingLimits::default()
            },
            CollaborationLimits::default(),
        ),
        (
            "maxPendingOutboxBytes",
            OUTBOX_LIMIT,
            EditingLimits::default(),
            CollaborationLimits {
                max_pending_outbox_bytes: OUTBOX_LIMIT,
                ..CollaborationLimits::default()
            },
        ),
    ] {
        let config = EditorSessionConfig::local_for_test();
        let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
            schema: tiptap_schema(),
            fragment_name: "prosemirror".into(),
            initialization_mode: InitializationMode::LocalEmpty,
            resource_limits: ResourceLimits::default(),
            editing_limits,
            max_length: None,
            scope: None,
        })
        .unwrap();
        engine
            .import_json(DOCUMENT, TransactionOrigin::DocumentImport)
            .unwrap();
        let mut session = EditorSession::new(
            engine,
            SessionPolicy::from_config(&config),
            DocumentState::LocalReady,
            collaboration_limits,
        )
        .unwrap();
        session.attach_collaboration_runtime();
        let before = session_audit(&session);
        let request = native_intent_request(
            &mut session,
            OWNER,
            REQUEST,
            serde_json::json!({
                "type":"command","anchor":1,"head":3,"command":{"type":"paste","text":text},
            }),
        );
        let error = NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&request)
            .unwrap_err();
        assert_eq!(
            error.details,
            Some(serde_json::json!({"field":field})),
            "{error:?}"
        );
        assert_eq!(error.limit, Some(limit as u64), "{error:?}");
        assert!(error.actual.unwrap() > limit as u64, "{error:?}");
        assert_eq!(session_audit(&session), before,
            "{field} rejection must retain document, Yrs state, revisions, selection, marks, history, and outbox charges");
    }
}

#[test]
fn native_multiline_paste_keeps_custom_schema_split_choices_when_one_type_cannot_fit() {
    const OWNER: u64 = 18_401;
    const REQUEST: u64 = 18_402;
    const CARET: u32 = 5;
    const TEXT: &str = "one\ntwo\nthree";
    let schema = crate::schema::Schema::from_json(&serde_json::json!({
        "nodes":[
            {"name":"doc","role":"doc","content":"paragraph firstContinuation? secondContinuation?"},
            {"name":"paragraph","role":"textBlock","content":"text*","htmlTag":"p"},
            {"name":"firstContinuation","role":"textBlock","content":"text*","attrs":{"tone":{"default":"first"}}},
            {"name":"secondContinuation","role":"textBlock","content":"text*","attrs":{"tone":{"default":"second"}}},
            {"name":"text","role":"text"}
        ], "marks":[]
    })).unwrap();
    let document = serde_json::json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"left right"}]}
    ]});
    let mut session = table_session_with_schema(&document.to_string(), schema);
    let before = session_audit(&session);
    let plan = crate::command_planner::plan_replace_selection_text(
        session.engine.document().unwrap(),
        &session.engine.schema(),
        &crate::selection::Selection::cursor(CARET + 1),
        None,
        TEXT,
        session.engine.resource_limits(),
        session
            .engine
            .editing_limits()
            .max_operations_per_transaction,
    )
    .unwrap()
    .unwrap();
    let split_types: Vec<&str> = plan
        .operations
        .iter()
        .filter_map(|operation| match operation {
            crate::command_planner::SemanticOperation::SplitBlock { node_type, .. } => {
                Some(node_type.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(split_types, ["firstContinuation", "secondContinuation"],
        "a custom schema must retain its per-boundary choices when uniform clipboard blocks do not fit");
    let request = native_intent_request(
        &mut session,
        OWNER,
        REQUEST,
        serde_json::json!({
            "type":"command","anchor":CARET,"head":CARET,
            "command":{"type":"paste","text":TEXT},
        }),
    );
    let outcome = NativeTransactionBridge::new(&mut session)
        .submit_native_intent(&request)
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();
    assert_eq!(outcome["documentChanged"], true);
    let expected = serde_json::json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"left one"}]},
        {"type":"firstContinuation","content":[{"type":"text","text":"two"}]},
        {"type":"secondContinuation","content":[{"type":"text","text":"threeright"}]}
    ]});
    assert_eq!(session.engine.document_json().unwrap(), expected);
    for (index, tone) in [(1, "first"), (2, "second")] {
        assert_eq!(
            session
                .engine
                .document()
                .unwrap()
                .root()
                .child(index)
                .unwrap()
                .attrs()
                .get("tone"),
            Some(&serde_json::json!(tone)),
            "split blocks must retain their declared style defaults"
        );
    }
    let (engine, outbox) = session.engine_and_outbox();
    assert!(engine
        .undo_with_outbox(REQUEST + 1, outbox)
        .unwrap()
        .is_some());
    assert_eq!(
        session.engine.document_json().unwrap(),
        before.document_json
    );
    let (engine, outbox) = session.engine_and_outbox();
    assert!(engine
        .redo_with_outbox(REQUEST + 2, outbox)
        .unwrap()
        .is_some());
    assert_eq!(session.engine.document_json().unwrap(), expected);
}

#[test]
fn native_collapsed_multiline_paste_and_next_character_respect_explicit_stored_marks() {
    const OWNER: u64 = 18_501;
    const REQUEST: u64 = 18_502;
    const CARET: u32 = 2;
    const LINE: &str = "X😀\n";
    const LAST_LINE: &str = "last";
    const TYPED_TEXT: &str = "!";
    const DOCUMENT: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abcd","marks":[{"type":"italic"}]}]}]}"#;
    let repetitions = EditingLimits::default().max_operations_per_transaction;
    let text = format!("{}{LAST_LINE}", LINE.repeat(repetitions));
    for enable_bold in [true, false] {
        let mut session = table_session_with_schema(DOCUMENT, tiptap_schema());
        let selection = native_intent_request(
            &mut session,
            OWNER,
            REQUEST - 1,
            serde_json::json!({
                "type":"setSelection", "anchor":CARET, "head":CARET,
            }),
        );
        NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&selection)
            .unwrap();
        let command = if enable_bold {
            serde_json::json!({"type":"setMark","markType":"bold"})
        } else {
            serde_json::json!({"type":"toggleMark","markType":"italic"})
        };
        let request = native_intent_request(
            &mut session,
            OWNER,
            REQUEST,
            serde_json::json!({
                "type":"command","anchor":CARET,"head":CARET,"command":command,
            }),
        );
        let mark_outcome = NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&request)
            .unwrap();
        let stored_marks = session
            .engine
            .stored_marks()
            .unwrap_or_else(|| panic!("collapsed mark command must store its explicit mark set: enable_bold={enable_bold}, outcome={mark_outcome}, selection={:?}, document={:?}", session.engine.resolved_selection(), session.engine.document_json()));
        assert_eq!(stored_marks.is_empty(), !enable_bold);
        let before = session_audit(&session);
        let request = native_intent_request(
            &mut session,
            OWNER,
            REQUEST + 1,
            serde_json::json!({
                "type":"command","anchor":CARET,"head":CARET,
                "command":{"type":"paste","text":text},
            }),
        );
        NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&request)
            .unwrap();
        let inserted_text = |text: &str| {
            let mut node = serde_json::json!({"type":"text","text":text});
            if enable_bold {
                node["marks"] = serde_json::json!([{"type":"bold"},{"type":"italic"}]);
            }
            node
        };
        let mut content = vec![serde_json::json!({"type":"paragraph","content":[
            {"type":"text","text":"ab","marks":[{"type":"italic"}]},
            inserted_text("X😀")
        ]})];
        content.extend((1..repetitions).map(|_| {
            serde_json::json!({
                "type":"paragraph","content":[inserted_text("X😀")]
            })
        }));
        content.push(serde_json::json!({"type":"paragraph","content":[
            inserted_text(LAST_LINE),
            {"type":"text","text":"cd","marks":[{"type":"italic"}]}
        ]}));
        let expected_paste = serde_json::json!({"type":"doc","content":content});
        assert_eq!(
            session.engine.document_json().unwrap(),
            expected_paste,
            "enable_bold={enable_bold}"
        );
        let caret = CARET + text.chars().count() as u32;
        let request = native_intent_request(
            &mut session,
            OWNER,
            REQUEST + 2,
            serde_json::json!({
                "type":"insertText","anchor":caret,"head":caret,"text":TYPED_TEXT,
            }),
        );
        NativeTransactionBridge::new(&mut session)
            .submit_native_intent(&request)
            .unwrap();
        let mut expected_typed = expected_paste.clone();
        expected_typed["content"][repetitions]["content"][0]["text"] =
            serde_json::json!(format!("{LAST_LINE}{TYPED_TEXT}"));
        assert_eq!(session.engine.document_json().unwrap(), expected_typed,
            "native typing after paste must inherit the user's explicit style (enable_bold={enable_bold})");
        let (engine, outbox) = session.engine_and_outbox();
        assert!(engine
            .undo_with_outbox(REQUEST + 3, outbox)
            .unwrap()
            .is_some());
        assert_eq!(session.engine.document_json().unwrap(), expected_paste);
        let (engine, outbox) = session.engine_and_outbox();
        assert!(engine
            .undo_with_outbox(REQUEST + 4, outbox)
            .unwrap()
            .is_some());
        assert_eq!(
            session.engine.document_json().unwrap(),
            before.document_json
        );
    }
}
