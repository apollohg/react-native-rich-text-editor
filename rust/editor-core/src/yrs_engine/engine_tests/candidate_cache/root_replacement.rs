const ROOT_REPLACEMENT_PRELOAD: &str =
    r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before load"}]}]}"#;
const ROOT_REPLACEMENT_REQUEST: u64 = 70_500_000;
const ROOT_REPLACEMENT_PARAGRAPHS: usize = 6;

fn root_replacement_article() -> serde_json::Value {
    json!({
        "type": "doc",
        "content": (0..ROOT_REPLACEMENT_PARAGRAPHS).map(|index| json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": format!("Paragraph {index}: ") },
                { "type": "text", "text": "bold", "marks": [{ "type": "bold" }] },
                { "type": "text", "text": " and " },
                {
                    "type": "text",
                    "text": "link",
                    "marks": [{ "type": "link", "attrs": { "href": format!("https://example.com/{index}") } }]
                },
                { "type": "text", "text": " café 😀" }
            ]
        })).collect::<Vec<_>>()
    })
}

fn root_replacement_loaded_engine(
    history: crate::yrs_engine::ReplacementHistory,
) -> YrsDocumentEngine {
    let mut engine = transaction_engine();
    engine
        .import_json(ROOT_REPLACEMENT_PRELOAD, TransactionOrigin::DocumentImport)
        .unwrap();
    engine
        .prepare_root_replacement_json(
            ROOT_REPLACEMENT_REQUEST,
            &root_replacement_article().to_string(),
            history,
        )
        .unwrap();
    engine
}

fn with_two_inserts_after_first_scalar(text: &str) -> String {
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    format!("{first}xx{}", chars.as_str())
}

fn assert_first_local_edit_bootstraps_exact_replica(
    engine: &mut YrsDocumentEngine,
    request_id: u64,
    context: &str,
) {
    assert_eq!(
        engine.prepared_candidate_cache_store_token_for_test(),
        None,
        "{context}: no proving replica may be retained before the first local edit"
    );
    let expected_text = with_two_inserts_after_first_scalar(&root_text(engine));
    reset_prepared_candidate_cache_counts_for_test();
    engine
        .apply_typed_transaction(insert_transaction(engine, request_id))
        .unwrap();
    assert_eq!(
        take_prepared_candidate_cache_counts_for_test(),
        (0, 1),
        "{context}: the first local edit must bootstrap exactly one replica"
    );
    assert_prepared_candidate_state_vector_exact(engine);
    let bootstrapped = engine
        .prepared_candidate_cache_store_token_for_test()
        .expect("the first local edit retains its proving replica");
    assert_eq!(
        super::encode_state_bounded(
            &engine.prepared_candidate_cache.as_ref().unwrap().doc,
            &engine.resource_limits,
        )
        .unwrap(),
        engine.encoded_state().unwrap(),
        "{context}: the bootstrapped replica must encode exactly like the live store"
    );
    engine
        .apply_typed_transaction(insert_transaction(engine, request_id + 1))
        .unwrap();
    assert_eq!(
        take_prepared_candidate_cache_counts_for_test(),
        (1, 0),
        "{context}: the second local edit must reuse the retained replica"
    );
    assert_eq!(
        engine.prepared_candidate_cache_store_token_for_test(),
        Some(bootstrapped),
        "{context}: the retained replica is double-buffered across edits"
    );
    assert_prepared_candidate_state_vector_exact(engine);
    assert_eq!(root_text(engine), expected_text, "{context}");
}

#[test]
fn root_replacement_load_retains_no_proving_replica_until_the_first_local_edit() {
    for (history, context) in [
        (
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
            "undoableBoundary",
        ),
        (
            crate::yrs_engine::ReplacementHistory::ResetAndClear,
            "resetAndClear",
        ),
    ] {
        let mut engine = root_replacement_loaded_engine(history);
        assert_eq!(
            engine.document_json().unwrap(),
            root_replacement_article(),
            "{context}"
        );
        assert_first_local_edit_bootstraps_exact_replica(
            &mut engine,
            ROOT_REPLACEMENT_REQUEST + 1,
            context,
        );
    }
}

#[test]
fn root_replacement_load_undo_and_redo_precede_an_exact_first_local_edit() {
    let mut engine = transaction_engine();
    engine
        .import_json(ROOT_REPLACEMENT_PRELOAD, TransactionOrigin::DocumentImport)
        .unwrap();
    let before_load = engine.document_json().unwrap();
    engine
        .prepare_root_replacement_json(
            ROOT_REPLACEMENT_REQUEST,
            &root_replacement_article().to_string(),
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();

    assert!(engine.undo(ROOT_REPLACEMENT_REQUEST + 1).unwrap().is_some());
    assert_eq!(engine.document_json().unwrap(), before_load);
    assert!(engine.redo(ROOT_REPLACEMENT_REQUEST + 2).unwrap().is_some());
    assert_eq!(engine.document_json().unwrap(), root_replacement_article());

    assert_first_local_edit_bootstraps_exact_replica(
        &mut engine,
        ROOT_REPLACEMENT_REQUEST + 3,
        "after undo and redo of the load",
    );
}

#[test]
fn root_replacement_load_then_remote_update_precedes_an_exact_first_local_edit() {
    use yrs::{diff_updates_v1, encode_state_vector_from_update_v1};

    let mut engine =
        root_replacement_loaded_engine(crate::yrs_engine::ReplacementHistory::UndoableBoundary);
    let loaded_state = engine.encoded_state().unwrap();
    let mut peer = YrsDocumentEngine::new(YrsEngineConfig {
        initialization_mode: crate::yrs_engine::InitializationMode::AwaitRemote,
        ..transaction_engine_config(crate::yrs_engine::EditingLimits::default())
    })
    .unwrap();
    assert!(
        peer.apply_remote_update_v1(ROOT_REPLACEMENT_REQUEST + 1, &loaded_state)
            .unwrap()
            .changed
    );
    peer.apply_typed_transaction(insert_transaction(&peer, ROOT_REPLACEMENT_REQUEST + 2))
        .unwrap();
    let remote_delta = diff_updates_v1(
        &peer.encoded_state().unwrap(),
        &encode_state_vector_from_update_v1(&loaded_state).unwrap(),
    )
    .unwrap();

    assert!(
        engine
            .apply_remote_update_v1(ROOT_REPLACEMENT_REQUEST + 3, &remote_delta)
            .unwrap()
            .changed
    );
    assert_eq!(root_text(&engine), root_text(&peer));

    assert_first_local_edit_bootstraps_exact_replica(
        &mut engine,
        ROOT_REPLACEMENT_REQUEST + 4,
        "after a remote update",
    );
    assert_eq!(
        root_text(&engine),
        with_two_inserts_after_first_scalar(&root_text(&peer))
    );
}

#[test]
fn root_replacement_load_snapshot_restores_into_an_exact_first_local_edit() {
    let mut engine =
        root_replacement_loaded_engine(crate::yrs_engine::ReplacementHistory::UndoableBoundary);
    let snapshot = engine.export_snapshot().unwrap();

    assert_first_local_edit_bootstraps_exact_replica(
        &mut engine,
        ROOT_REPLACEMENT_REQUEST + 1,
        "before the snapshot is restored",
    );
    assert!(engine.restore_snapshot(&snapshot).unwrap().changed);
    assert_eq!(engine.document_json().unwrap(), root_replacement_article());
    assert_first_local_edit_bootstraps_exact_replica(
        &mut engine,
        ROOT_REPLACEMENT_REQUEST + 3,
        "after restoring the load snapshot in place",
    );

    let mut restored = YrsDocumentEngine::new_with_snapshot(
        YrsEngineConfig {
            initialization_mode: crate::yrs_engine::InitializationMode::AwaitRemote,
            ..transaction_engine_config(crate::yrs_engine::EditingLimits::default())
        },
        &snapshot,
    )
    .unwrap();
    assert_eq!(restored.document_json().unwrap(), root_replacement_article());
    assert_first_local_edit_bootstraps_exact_replica(
        &mut restored,
        ROOT_REPLACEMENT_REQUEST + 5,
        "after restoring the load snapshot into a new engine",
    );
}
