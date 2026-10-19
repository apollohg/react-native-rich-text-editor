const LARGE_LOADED_PARAGRAPHS: usize = 385;
const VERY_LARGE_LOADED_PARAGRAPHS: usize = 1537;
const SHARED_SNAPSHOT_PARAGRAPHS: usize = 24;
const LOADED_BODY: &str = "Native editing preserves each word, its formatting, and the caret while the document grows. ";

fn loaded_article(paragraphs: usize) -> serde_json::Value {
    json!({
        "type": "doc",
        "content": (0..paragraphs).map(|index| json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": format!("Paragraph {index:04}: {LOADED_BODY}") },
                { "type": "text", "text": "bold emphasis", "marks": [{ "type": "bold" }] },
                { "type": "text", "text": ", " },
                { "type": "text", "text": "italic detail", "marks": [{ "type": "italic" }] },
                { "type": "text", "text": ", " },
                {
                    "type": "text",
                    "text": "linked reference",
                    "marks": [{ "type": "link", "attrs": { "href": format!("https://example.com/{index}") } }]
                },
                { "type": "text", "text": ". Unicode: café, e\u{301}, 東京, 😀." }
            ]
        })).collect::<Vec<_>>()
    })
}

#[test]
fn a_format_after_an_undoable_load_keeps_the_load_undoable() {
    const REQUEST_ID: u64 = 70_040_000;
    let paragraphs = LARGE_LOADED_PARAGRAPHS;
    let mut engine = transaction_engine();
    engine
        .import_json(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before load"}]}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let before_load = engine.document_json().unwrap();
    engine
        .prepare_root_replacement_json(
            REQUEST_ID,
            &loaded_article(paragraphs).to_string(),
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    let loaded = engine.document_json().unwrap();
    let block = engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .block(paragraphs / 2)
        .unwrap()
        .into_owned();
    let start = block.scalar_start + block.scalar_prefix_len;
    select_text(&mut engine, REQUEST_ID + 1, start, start + 9);
    engine
        .apply_command(
            REQUEST_ID + 2,
            TypedCommand::ToggleMark {
                mark_type: "bold".into(),
            },
        )
        .unwrap()
        .unwrap();

    engine.undo(REQUEST_ID + 3).unwrap().unwrap();
    assert_eq!(
        engine.document_json().unwrap(),
        loaded,
        "{paragraphs} paragraphs: undo the format"
    );
    assert!(
        engine.can_undo(),
        "{paragraphs} paragraphs: the load must stay undoable: its retained snapshots fit the history budget"
    );
    engine.undo(REQUEST_ID + 4).unwrap().unwrap();
    assert_eq!(
        engine.document_json().unwrap(),
        before_load,
        "{paragraphs} paragraphs: undo the load"
    );
}

#[test]
fn a_format_after_reloading_a_very_large_document_keeps_the_reload_undoable() {
    const REQUEST_ID: u64 = 70_042_000;
    let article = loaded_article(VERY_LARGE_LOADED_PARAGRAPHS).to_string();
    let mut engine = transaction_engine();
    engine
        .prepare_root_replacement_json(
            REQUEST_ID,
            &article,
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    let block = engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .block(VERY_LARGE_LOADED_PARAGRAPHS / 2)
        .unwrap()
        .into_owned();
    let start = block.scalar_start + block.scalar_prefix_len;
    let toggle = TypedCommand::ToggleMark {
        mark_type: "bold".into(),
    };
    select_text(&mut engine, REQUEST_ID + 1, start, start + 9);
    engine
        .apply_command(REQUEST_ID + 2, toggle.clone())
        .unwrap()
        .unwrap();
    let before_reload = engine.document_json().unwrap();
    engine
        .prepare_root_replacement_json(
            REQUEST_ID + 3,
            &article,
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    let reloaded = engine.document_json().unwrap();
    select_text(&mut engine, REQUEST_ID + 4, start, start + 9);
    engine
        .apply_command(REQUEST_ID + 5, toggle)
        .unwrap()
        .unwrap();

    engine.undo(REQUEST_ID + 6).unwrap().unwrap();
    assert_eq!(engine.document_json().unwrap(), reloaded, "undo the format");
    assert!(
        engine.can_undo(),
        "the reload must stay undoable: the reload's after state and the format's before state \
         are one retained snapshot"
    );
    engine.undo(REQUEST_ID + 7).unwrap().unwrap();
    assert_eq!(engine.document_json().unwrap(), before_reload, "undo the reload");
}

#[test]
fn history_snapshots_are_shared_only_between_identical_document_states() {
    const REQUEST_ID: u64 = 70_041_000;
    let mut engine = transaction_engine();
    engine
        .import_json(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before load"}]}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    engine
        .prepare_root_replacement_json(
            REQUEST_ID,
            &loaded_article(SHARED_SNAPSHOT_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    let loaded = engine.document_json().unwrap();
    select_text(&mut engine, REQUEST_ID + 1, 0, 9);
    let toggle = TypedCommand::ToggleMark {
        mark_type: "italic".into(),
    };
    engine
        .apply_command(REQUEST_ID + 2, toggle.clone())
        .unwrap()
        .unwrap();
    let formatted = engine.document_json().unwrap();
    let point = RevisionedPosition {
        offset: 20,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    };
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID + 3,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![TypedOperation::InsertText {
                at: point,
                text: "unrecorded".into(),
                marks: Vec::new(),
            }],
            selection_intent: SelectionIntent::Preserve,
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    let unrecorded = engine.document_json().unwrap();
    select_text(&mut engine, REQUEST_ID + 4, 0, 9);
    engine.apply_command(REQUEST_ID + 5, toggle).unwrap().unwrap();

    let [load, first_format, second_format] =
        <[[Option<usize>; 2]; 3]>::try_from(engine.history.document_snapshot_identities_for_test())
            .expect("load and two formats are separate undo groups");
    assert!(
        load.iter().chain(&first_format).chain(&second_format).all(Option::is_some),
        "every snapshot of this small document is retained: {load:?} {first_format:?} {second_format:?}"
    );
    assert_eq!(
        load[1], first_format[0],
        "the load's after state is exactly the first format's before state"
    );
    assert_ne!(
        first_format[1], second_format[0],
        "an unrecorded edit changed the document between the two formats"
    );
    assert_eq!(
        engine.retained_history_for_test().1,
        engine.history.undo_stack_metadata_bytes_for_test(),
        "the replay ledger charges the shared snapshot once, exactly like the undo stack"
    );

    engine.undo(REQUEST_ID + 6).unwrap().unwrap();
    assert_eq!(engine.document_json().unwrap(), unrecorded);
    engine.undo(REQUEST_ID + 7).unwrap().unwrap();
    let first_run = engine.document_json().unwrap()["content"][0]["content"][0].clone();
    assert!(
        first_run["text"]
            .as_str()
            .is_some_and(|text| text.contains("unrecorded"))
            && first_run.get("marks").is_none(),
        "undoing the first format unmarks its run and keeps the unrecorded edit: {first_run}"
    );
    assert_ne!(engine.document_json().unwrap(), formatted);
    assert_ne!(engine.document_json().unwrap(), loaded);
    assert!(engine.can_undo(), "the load stays undoable");
}

#[test]
fn a_format_that_rolls_history_charges_its_shared_before_snapshot_in_full() {
    const REQUEST_ID: u64 = 70_043_000;
    let mut engine = transaction_engine_with_editing_limits(crate::yrs_engine::EditingLimits {
        max_undo_groups: 1,
        ..crate::yrs_engine::EditingLimits::default()
    });
    engine
        .prepare_root_replacement_json(
            REQUEST_ID,
            &loaded_article(SHARED_SNAPSHOT_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    let load_after = engine.history.document_snapshot_identities_for_test()[0][1];
    select_text(&mut engine, REQUEST_ID + 1, 0, 9);
    engine
        .apply_command(
            REQUEST_ID + 2,
            TypedCommand::ToggleMark {
                mark_type: "italic".into(),
            },
        )
        .unwrap()
        .unwrap();

    let [format] =
        <[[Option<usize>; 2]; 1]>::try_from(engine.history.document_snapshot_identities_for_test())
            .expect("a second undo group rolls the load out of history");
    assert!(
        load_after.is_some() && format[0] == load_after,
        "the format's before state reuses the load's after snapshot: {load_after:?} {format:?}"
    );
    assert_eq!(
        engine.retained_history_for_test().1,
        engine.history.undo_stack_metadata_bytes_for_test(),
        "once the load is rolled away the format's before snapshot is its only owner"
    );
}

fn single_paragraph_snapshot_charge(text: &str) -> usize {
    let mut engine = transaction_engine();
    engine
        .import_json(
            &json!({
                "type": "doc",
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": text }]
                }]
            })
            .to_string(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let state = engine.derived_state.as_ref().unwrap();
    crate::yrs_engine::derived_state::history_document_snapshot_retained_bytes(
        crate::yrs_engine::derived_state::HistoryDocumentSnapshotRetainedInput {
            document: &state.document,
            canonical_artifact: &state.canonical_artifact,
            position_map: &state.position_map,
            rendered_text: &state.rendered_text,
            render_blocks: &state.render_blocks,
            schema_fingerprint: &state.schema_fingerprint,
            fragment_name: &engine.fragment_name,
            scope: engine.scope.as_ref(),
        },
    )
    .unwrap()
    .get()
}

#[test]
fn a_history_snapshot_charges_each_retained_copy_of_its_text_once() {
    const GROWTH: usize = 4096;
    const RETAINED_TEXT_COPIES: usize = 3;
    let short = "a".repeat(GROWTH);
    let long = "a".repeat(2 * GROWTH);
    let growth =
        single_paragraph_snapshot_charge(&long) - single_paragraph_snapshot_charge(&short);
    assert_eq!(
        growth,
        RETAINED_TEXT_COPIES * GROWTH,
        "the document text node, the render text run and the rendered text are the only \
         retained copies of the paragraph text"
    );
}

#[test]
fn an_undoable_load_retains_its_encoded_update_not_the_admission_bound() {
    const REQUEST_ID: u64 = 70_044_000;
    const UPDATE_BUFFER_GROWTH_FACTOR: usize = 2;
    let mut engine = transaction_engine();
    let (events_before, bytes_before, _) = engine.history.replay_audit_for_test();
    engine
        .prepare_root_replacement_json(
            REQUEST_ID,
            &loaded_article(LARGE_LOADED_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    let (events_after, bytes_after, _) = engine.history.replay_audit_for_test();
    let encoded_len = engine.encoded_state().unwrap().len();
    assert_eq!(events_after, events_before + 1, "one recorded load event");
    let retained = bytes_after - bytes_before;
    assert!(
        retained > 0 && retained <= UPDATE_BUFFER_GROWTH_FACTOR * encoded_len + 1,
        "the load event retains {retained} bytes for a {encoded_len}-byte document state"
    );
}
