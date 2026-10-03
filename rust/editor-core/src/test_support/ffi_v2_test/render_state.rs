// The accessor derives, from the live v2 session alone, everything the
// (since-deleted) stateless legacy render probe provided to the staging
// adapters: full render blocks, toolbar active state, the mirrored scalar
// selection resolved to doc positions, the lenient doc<->scalar position
// mapping (including the u32::MAX extent query), and the document's scalar
// extent. deleted the legacy runtime, so the probe-parity fixture matrix went
// with it; these tests pin the accessor's own wire shape, its v2-native
// history/revision facts, and its structured errors.

fn local_json_config(document: &str) -> Value {
    json!({
        "schema": tiptap_schema_json(),
        "initialization": {
            "type": "localJson",
            "json": serde_json::from_str::<Value>(document).unwrap(),
        }
    })
}

fn exact_cell_fixture_with_policy(read_only: bool) -> (String, [u64; 4], Value) {
    let source = json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [
            { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "😀first" }] }] },
            { "type": "table_cell", "content": [{ "type": "table", "content": [{ "type": "table_row", "content": [
                { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "nested" }] }] }
            ] }] }] },
            { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "last" }] }] }
        ] }] }] });
    let mut config = json!({
        "schema": crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        "initialization": { "type": "localJson", "json": source.clone() }
    });
    if read_only {
        config["policy"] = json!({ "readOnly": true });
    }
    let id = create_handle(config);
    let records = table_records_by_position(&id);
    let inner_table = records.last().expect("nested table");
    let outer_openings = outer_cell_openings(&id);
    (
        id,
        [
            outer_openings[0],
            outer_openings[1],
            outer_openings[2],
            inner_table.cell_openings[0],
        ],
        source,
    )
}

fn exact_cell_fixture() -> (String, [u64; 4], Value) {
    exact_cell_fixture_with_policy(false)
}

fn exact_cell_request(revision: u64, anchor: Value, head: Value) -> Value {
    json!({
        "version": 1, "requestId": "1",
        "baseDocumentRevision": revision.to_string(),
        "selection": { "type": "cell", "anchorCell": anchor, "headCell": head }
    })
}

fn document_cell_point(opening: u64) -> Value {
    json!({ "kind": "document", "offset": opening })
}

fn outer_cell_openings(id: &str) -> Vec<u64> {
    table_records_by_position(id)
        .first()
        .expect("outer table")
        .cell_openings
        .clone()
}

#[test]
fn exact_document_cell_endpoint_selects_nested_only_outer_cell() {
    let (id, [first, outer, _, _], _) = exact_cell_fixture();
    let before_document = document_json_of(&id);
    let before_state = state_of(&id);
    let request = exact_cell_request(
        revision_of(&id),
        document_cell_point(first),
        document_cell_point(outer),
    );
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        request.to_string(),
    ));
    let after_render = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(after_render["selection"]["type"], "cell");
    assert_eq!(after_render["selection"]["anchorCell"], first);
    assert_eq!(after_render["selection"]["headCell"], outer);
    assert_eq!(document_json_of(&id), before_document);
    assert_eq!(
        state_of(&id)["documentRevision"],
        before_state["documentRevision"]
    );
    assert_eq!(state_of(&id)["canUndo"], before_state["canUndo"]);
    assert_eq!(state_of(&id)["canRedo"], before_state["canRedo"]);
    destroy_handle(&id);
}

#[test]
fn exact_cell_endpoints_reject_non_openings_and_cross_table_pairs_atomically() {
    let (id, [first, outer, sibling, inner], _) = exact_cell_fixture();
    let revision = revision_of(&id);
    let before = state_of(&id);
    let before_document = document_json_of(&id);
    let table_end = table_records_by_position(&id)
        .first()
        .expect("outer table")
        .source_end;
    for (label, head) in [
        ("inside first cell", first + 1),
        ("outer cell child boundary", outer + 1),
        ("before first cell", first - 1),
        ("document start", 0),
        ("table end", table_end),
        ("beyond document", u32::MAX as u64),
        ("nested table cell", inner),
    ] {
        let request = exact_cell_request(
            revision,
            document_cell_point(first),
            document_cell_point(head),
        );
        let error = err_json(&v2::editor_v2_set_selection(
            id.clone(),
            request.to_string(),
        ));
        assert_eq!(error.code, "POSITION_INVALID", "{label}: {error:?}");
        assert_eq!(state_of(&id), before, "{label} must not change state");
        assert_eq!(
            document_json_of(&id),
            before_document,
            "{label} must not change document"
        );
    }
    for (anchor, head) in [(sibling, outer), (outer, outer)] {
        let request = exact_cell_request(
            revision,
            document_cell_point(anchor),
            document_cell_point(head),
        );
        ok_json(&v2::editor_v2_set_selection(
            id.clone(),
            request.to_string(),
        ));
        let rendered = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
        assert_eq!(rendered["selection"]["anchorCell"], anchor);
        assert_eq!(rendered["selection"]["headCell"], head);
    }
    destroy_handle(&id);
}

#[test]
fn exact_cell_wire_is_strict_and_legacy_or_mixed_points_still_work() {
    let (id, [first, outer, sibling, _], _) = exact_cell_fixture();
    let revision = revision_of(&id);
    let base = exact_cell_request(
        revision,
        document_cell_point(first),
        document_cell_point(outer),
    );
    for (label, point) in [
        (
            "unknown key",
            json!({ "kind": "document", "offset": outer, "extra": true }),
        ),
        ("negative", json!({ "kind": "document", "offset": -1 })),
        ("fraction", json!({ "kind": "document", "offset": 1.5 })),
        (
            "overflow",
            json!({ "kind": "document", "offset": u64::MAX }),
        ),
        ("missing kind", json!({ "offset": outer })),
        ("unknown kind", json!({ "kind": "row", "offset": outer })),
        (
            "invalid affinity",
            json!({ "kind": "document", "offset": outer, "affinity": "middle" }),
        ),
    ] {
        let mut request = base.clone();
        request["selection"]["headCell"] = point;
        let error = err_json(&v2::editor_v2_set_selection(
            id.clone(),
            request.to_string(),
        ));
        assert_eq!(error.code, "CONFIG_INVALID", "{label}: {error:?}");
    }
    let mut text_request = base.clone();
    text_request["selection"] = json!({ "type": "text",
        "anchor": document_cell_point(first), "head": document_cell_point(first) });
    assert_eq!(
        err_json(&v2::editor_v2_set_selection(
            id.clone(),
            text_request.to_string()
        ))
        .code,
        "CONFIG_INVALID"
    );

    let scalar = ok_json(&v2_render::editor_v2_doc_to_scalar(
        id.clone(),
        (first + 3) as u32,
    ))["scalar"]
        .as_u64()
        .unwrap();
    let utf16 = scalar + "😀".encode_utf16().count() as u64 - "😀".chars().count() as u64;
    for kind in ["scalar", "utf16"] {
        let offset = if kind == "utf16" { utf16 } else { scalar };
        let request = exact_cell_request(
            revision,
            json!({ "kind": kind, "offset": offset }),
            document_cell_point(outer),
        );
        let outcome = v2::editor_v2_set_selection(id.clone(), request.to_string());
        assert!(
            outcome.error.is_none(),
            "{kind} offset={offset}: {:?}",
            outcome.error
        );
        ok_json(&outcome);
        let rendered = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
        assert_eq!(rendered["selection"]["anchorCell"], first, "{kind}");
        assert_eq!(rendered["selection"]["headCell"], outer, "{kind}");
    }
    let sibling_scalar = ok_json(&v2_render::editor_v2_doc_to_scalar(
        id.clone(),
        (sibling + 2) as u32,
    ))["scalar"]
        .as_u64()
        .unwrap();
    let legacy = exact_cell_request(
        revision,
        json!({ "kind": "scalar", "offset": scalar }),
        json!({ "kind": "utf16", "offset": sibling_scalar + utf16 - scalar }),
    );
    ok_json(&v2::editor_v2_set_selection(id.clone(), legacy.to_string()));
    let rendered = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(rendered["selection"]["anchorCell"], first);
    assert_eq!(rendered["selection"]["headCell"], sibling);
    destroy_handle(&id);
}

#[test]
fn exact_cell_endpoint_rejects_stale_revision_even_when_opening_is_reused() {
    let (id, [first, outer, _, _], source) = exact_cell_fixture();
    let stale_revision = revision_of(&id);
    let stale_request = exact_cell_request(
        stale_revision,
        document_cell_point(first),
        document_cell_point(outer),
    );
    let before_render = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    let replacement = source.to_string().replace("first", "other");
    ok_json(&v2::editor_v2_apply_local_api(
        id.clone(),
        replace_envelope(2, stale_revision, &replacement, "resetAndClear"),
    ));
    assert!(revision_of(&id) > stale_revision);
    let after_replace_render = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    let replacement_openings = outer_cell_openings(&id);
    assert_eq!(replacement_openings[0], first);
    assert_eq!(replacement_openings[1], outer);
    let after_replace = document_json_of(&id);
    let error = err_json(&v2::editor_v2_set_selection(
        id.clone(),
        stale_request.to_string(),
    ));
    assert_eq!(error.code, "REVISION_MISMATCH");
    assert_eq!(document_json_of(&id), after_replace);
    assert_eq!(
        ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None))["selection"],
        after_replace_render["selection"]
    );
    assert_ne!(
        before_render["documentVersion"],
        after_replace_render["documentVersion"]
    );
    destroy_handle(&id);
}

#[test]
fn exact_cell_selection_preserves_document_edit_history_and_redo() {
    let (id, [first, outer, _, _], _) = exact_cell_fixture();
    let original = document_json_of(&id);
    let scalar = ok_json(&v2_render::editor_v2_doc_to_scalar(
        id.clone(),
        (first + 3) as u32,
    ))["scalar"]
        .as_u64()
        .unwrap() as u32;
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        selection_envelope(1, revision_of(&id), scalar, scalar),
    ));
    ok_json(&v2::editor_v2_apply_input(
        id.clone(),
        input_envelope(2, revision_of(&id), "Z"),
    ));
    let edited = document_json_of(&id);
    assert_ne!(edited, original);
    assert_eq!(state_of(&id)["canUndo"], true);

    let edited_openings = outer_cell_openings(&id);
    let exact = exact_cell_request(
        revision_of(&id),
        document_cell_point(edited_openings[0]),
        document_cell_point(edited_openings[1]),
    );
    ok_json(&v2::editor_v2_set_selection(id.clone(), exact.to_string()));
    assert_eq!(document_json_of(&id), edited);
    ok_json(&v2::editor_v2_undo(id.clone(), history_envelope(3)));
    assert_eq!(
        document_json_of(&id),
        original,
        "one undo after exact selection must undo the user edit"
    );
    assert_eq!(state_of(&id)["canRedo"], true);

    let exact = exact_cell_request(
        revision_of(&id),
        document_cell_point(first),
        document_cell_point(outer),
    );
    ok_json(&v2::editor_v2_set_selection(id.clone(), exact.to_string()));
    assert_eq!(
        state_of(&id)["canRedo"],
        true,
        "exact selection after undo must preserve redo"
    );
    ok_json(&v2::editor_v2_redo(id.clone(), history_envelope(4)));
    assert_eq!(document_json_of(&id), edited);
    destroy_handle(&id);
}

#[test]
fn exact_cell_selection_is_admitted_under_read_only_policy() {
    let (id, [first, outer, _, _], _) = exact_cell_fixture_with_policy(true);
    let before = document_json_of(&id);
    let request = exact_cell_request(
        revision_of(&id),
        document_cell_point(first),
        document_cell_point(outer),
    );
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        request.to_string(),
    ));
    let rendered = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(rendered["selection"]["anchorCell"], first);
    assert_eq!(rendered["selection"]["headCell"], outer);
    assert_eq!(document_json_of(&id), before);
    destroy_handle(&id);
}

const FIXTURE_MULTI_BLOCK: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"ab"}]},{"type":"paragraph","content":[{"type":"text","text":"cd"}]}]}"#;
const ORDERED_LIST_START_MISSING: &str = r#"{"type":"doc","content":[{"type":"orderedList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]}]}]}"#;
const ORDERED_LIST_START_NULL: &str = r#"{"type":"doc","content":[{"type":"orderedList","attrs":{"start":null},"content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]}]}]}"#;

#[test]
fn semantic_table_render_read_is_session_pure_and_matches_viewer_flat_records() {
    let schema_json =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    let schema = crate::schema::Schema::from_json(&schema_json).unwrap();
    let source = json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [
            { "type": "table_header", "attrs": { "colspan": 2 }, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "header" }] }] },
            { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "body" }] }] }
        ] }]
    }] });
    let source_engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema,
        fragment_name: FRAGMENT_NAME.into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: Some(DocumentScope {
            document_id: DOCUMENT_ID.into(),
            lineage_id: LINEAGE_ID.into(),
        }),
    })
    .unwrap();
    let mut source_engine = source_engine;
    source_engine
        .import_json(&source.to_string(), TransactionOrigin::DocumentImport)
        .unwrap();
    let snapshot = source_engine.export_snapshot().unwrap();
    let mut config = room_config(Some(&snapshot));
    config["schema"] = schema_json.clone();
    let id = create_handle_with_state(config, Some(snapshot.encoded_state));
    let state_before = state_of(&id);
    let outbox_before =
        crate::native_bridge_test_support::outbox_pending(id.parse().unwrap()).unwrap();
    assert_eq!(outbox_before, Some((0, 0)));

    let editor = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    let repeated = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(editor, repeated);
    assert_eq!(
        state_of(&id),
        state_before,
        "render reads must not mutate document or history state"
    );
    assert_eq!(
        crate::native_bridge_test_support::outbox_pending(id.parse().unwrap()).unwrap(),
        outbox_before,
        "render reads must not enqueue outgoing document updates"
    );

    let viewer = crate::viewer::viewer_compile(crate::viewer::FfiViewerCompileRequest {
        source_kind: crate::viewer::FfiViewerSourceKind::Json,
        source: source.to_string(),
        config_json: json!({ "schema": schema_json, "initialization": { "type": "localEmpty" } })
            .to_string(),
        images_enabled: true,
        mention_prefix: None,
    })
    .value
    .unwrap();
    let mirror = native_table_mirror(&id);
    let viewer_records = viewer.table_records();
    assert_eq!(mirror.tables.len(), viewer_records.len());
    assert_eq!(mirror.attributes.len(), viewer.table_attributes().len());
    let root_id = editor["renderBlocks"][0][0]["tableId"].as_str().unwrap();
    let native = &mirror.tables[root_id];
    let (position, _) = mirror.table_start(root_id).unwrap();
    let record = &viewer_records[0];
    assert_eq!(position, record.table_pos);
    assert_eq!(position + native.doc_size, record.source_end);
    assert_eq!(native.cells.len(), record.cells.len());
    assert_eq!(
        state_of(&id),
        state_before,
        "native frame reads preserve session state"
    );
    assert_eq!(
        crate::native_bridge_test_support::outbox_pending(id.parse().unwrap()).unwrap(),
        outbox_before
    );
    destroy_handle(&id);
}

fn ordered_list_document_with_start(start: Value, labels: &[&str]) -> String {
    let items = labels
        .iter()
        .map(|label| {
            json!({
                "type": "listItem",
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": label }],
                }],
            })
        })
        .collect::<Vec<_>>();
    json!({
        "type": "doc",
        "content": [{
            "type": "orderedList",
            "attrs": { "start": start },
            "content": items,
        }],
    })
    .to_string()
}

#[test]
fn render_update_ordered_list_start_bound_is_exact_or_rejected_at_validation() {
    let id = create_handle(local_json_config(ORDERED_LIST_START_MISSING));
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(
        update["renderBlocks"][0][0]["listContext"]["index"],
        json!(1),
        "an absent ordered-list start must default to one"
    );
    destroy_handle(&id);

    let id = create_handle(local_json_config(ORDERED_LIST_START_NULL));
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(
        update["renderBlocks"][0][0]["listContext"]["index"],
        json!(1),
        "a null ordered-list start means absent and must default to one"
    );
    destroy_handle(&id);

    let id = create_handle(local_json_config(&ordered_list_document_with_start(
        json!(MAX_ORDERED_LIST_START),
        &["last"],
    )));
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(
        update["renderBlocks"][0][0]["listContext"]["index"],
        json!(MAX_ORDERED_LIST_START),
        "the v2 render accessor must preserve the largest admitted start exactly"
    );
    destroy_handle(&id);

    let malformed_starts = [
        json!(-1),
        json!(1.5),
        json!("1"),
        json!(u64::from(MAX_ORDERED_LIST_START) + 1),
        json!(u64::from(u32::MAX) + 1),
        json!(1e30),
    ];
    let malformed_documents = malformed_starts
        .into_iter()
        .map(|start| ordered_list_document_with_start(start, &["bad"]));

    let id = create_handle(local_json_config(&ordered_list_document_with_start(
        json!(MAX_ORDERED_LIST_START),
        &["last", "headroom"],
    )));
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    let indexes = update["renderBlocks"]
        .as_array()
        .expect("render blocks is an array")
        .iter()
        .flat_map(|block| block.as_array().expect("each render block is an array"))
        .filter_map(|element| element["listContext"]["index"].as_u64())
        .collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![
            u64::from(MAX_ORDERED_LIST_START),
            u64::from(MAX_ORDERED_LIST_START) + 1
        ],
        "the admitted start bound must leave headroom for every item index a document can hold"
    );
    destroy_handle(&id);

    for document in malformed_documents {
        let error = err_json(&v2::editor_v2_create(
            local_json_config(&document).to_string(),
            None,
        ));
        assert_error(&error, "document", "DOCUMENT_INVALID", None);
    }
}

#[test]
fn render_update_is_one_complete_atomic_snapshot() {
    let id = create_handle(local_json_config(FIXTURE_MULTI_BLOCK));
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    let keys: std::collections::BTreeSet<&str> = update
        .as_object()
        .expect("render update is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "renderBlocks",
            "renderPatch",
            "selection",
            "activeState",
            "historyState",
            "documentVersion",
            "stateRevision",
            "scalarLength",
            "documentIsEmpty",
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<&str>>(),
        "the no-mirror update carries exactly the frozen accessor keys: {keys:?}",
    );

    // History and version are the v2 engine's own facts, consistent with
    // getState at every revision.
    let assert_history_matches_state = |id: &str| {
        let state = state_of(id);
        let update = ok_json(&v2_render::editor_v2_render_update(
            id.to_string(),
            None,
            None,
        ));
        assert_eq!(update["documentVersion"], state["documentRevision"]);
        assert_eq!(update["stateRevision"], state["stateRevision"]);
        assert_eq!(
            update["historyState"],
            json!({
                "canUndo": state["canUndo"].as_bool().unwrap(),
                "canRedo": state["canRedo"].as_bool().unwrap(),
            })
        );
    };
    assert_history_matches_state(&id);
    ok_json(&v2::editor_v2_apply_input(
        id.clone(),
        input_envelope(41, revision_of(&id), "Z"),
    ));
    assert_history_matches_state(&id);
    ok_json(&v2::editor_v2_undo(id.clone(), history_envelope(42)));
    assert_history_matches_state(&id);
    destroy_handle(&id);
}

#[test]
fn render_update_cannot_mix_fields_with_a_concurrent_mutation() {
    use std::sync::mpsc::{sync_channel, RecvTimeoutError};
    use std::time::Duration;

    let id = create_handle(local_json_config(FIXTURE_MULTI_BLOCK));
    let base_revision = revision_of(&id);
    let state_before = state_of(&id);
    let (entered_tx, entered_rx) = sync_channel(0);
    let (resume_tx, resume_rx) = sync_channel(0);
    let _hook = v2_render::install_render_snapshot_test_hook(
        id.parse().expect("editor handle is a canonical u64"),
        entered_tx,
        resume_rx,
    );

    let render_id = id.clone();
    let render_thread =
        std::thread::spawn(move || v2_render::editor_v2_render_update(render_id, None, None));
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("render snapshot reached the forced pause");

    let mutation_id = id.clone();
    let (mutation_tx, mutation_rx) = sync_channel(1);
    let mutation_thread = std::thread::spawn(move || {
        let result = v2::editor_v2_apply_input(mutation_id, input_envelope(71, base_revision, "Z"));
        mutation_tx.send(result).unwrap();
    });
    assert!(matches!(
        mutation_rx.recv_timeout(Duration::from_millis(50)),
        Err(RecvTimeoutError::Timeout)
    ));

    resume_tx.send(()).unwrap();
    let snapshot = ok_json(&render_thread.join().expect("render thread succeeds"));
    let mutation = mutation_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("mutation completes after the snapshot releases the editor");
    ok_json(&mutation);
    mutation_thread.join().expect("mutation thread succeeds");

    assert_eq!(
        snapshot["documentVersion"],
        state_before["documentRevision"]
    );
    assert_eq!(snapshot["stateRevision"], state_before["stateRevision"]);
    assert_eq!(
        snapshot["historyState"],
        json!({ "canUndo": false, "canRedo": false })
    );
    assert_eq!(snapshot["selection"]["type"], json!("text"));
    assert_eq!(snapshot["selection"]["anchor"], json!(1));
    assert_eq!(snapshot["selection"]["head"], json!(1));
    assert!(snapshot["scalarLength"].as_u64().is_some());
    assert!(
        !snapshot["renderBlocks"].to_string().contains('Z'),
        "render content must come from the same pre-mutation state"
    );
    assert_eq!(revision_of(&id), base_revision + 1);
    destroy_handle(&id);
}

const ACTIVE_STATE_DOC: &str = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"plain "},{"type":"text","text":"bold","marks":[{"type":"bold"}]}]}]}"#;

#[test]
fn identical_selection_mirror_reuses_table_active_state_without_mutating_the_engine() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    let (id, [first, _, _, _], _) = exact_cell_fixture();
    let scalar = ok_json(&v2_render::editor_v2_doc_to_scalar(
        id.clone(),
        (first + 3) as u32,
    ))["scalar"]
        .as_u64()
        .unwrap() as u32;
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        selection_envelope(64, revision_of(&id), scalar, scalar),
    ));
    let expected = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    let state = state_of(&id);
    let document = document_json_of(&id);
    reset_full_pass_counts_for_test();
    let mirrored = ok_json(&v2_render::editor_v2_render_update(
        id.clone(),
        Some(scalar),
        Some(scalar),
    ));
    let passes = take_full_pass_counts_for_test();
    assert_eq!(
        mirrored, expected,
        "the identical mirror must preserve every snapshot field"
    );
    assert_eq!(
        passes.table_command_availability_plans, 0,
        "an identical mirror must reuse admitted active state: {passes:#?}"
    );
    assert_eq!(state_of(&id), state);
    assert_eq!(document_json_of(&id), document);
    destroy_handle(&id);
}

#[test]
fn render_update_active_state_uses_authoritative_or_explicit_mirror_selection() {
    let id = create_handle(local_json_config(ACTIVE_STATE_DOC));

    // The authoritative initial cursor is at the document start.
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(update["activeState"]["marks"]["bold"], json!(false));

    // A scalar mirror inside the bold word (scalars 7..=10) activates it.
    let update = ok_json(&v2_render::editor_v2_render_update(
        id.clone(),
        Some(8),
        Some(8),
    ));
    assert_eq!(update["activeState"]["marks"]["bold"], json!(true));

    // The engine now tracks a selection inside the bold word. Without a
    // mirror, both selection and active state must use that exact state.
    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        selection_envelope(61, revision, 8, 8),
    ));
    let expected_selection = ok_json(&v2_render::editor_v2_resolve_scalar_selection(
        id.clone(),
        8,
        8,
    ));
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(update["selection"], expected_selection);
    assert_eq!(update["activeState"]["marks"]["bold"], json!(true));

    destroy_handle(&id);
}

#[test]
fn render_update_active_state_no_mirror_uses_engine_stored_marks() {
    let id = create_handle(local_json_config(ACTIVE_STATE_DOC));

    // Collapse the engine selection into the plain region and toggle bold:
    // the engine records a stored mark for the next typed character.
    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        selection_envelope(62, revision, 3, 3),
    ));
    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(
            63,
            revision,
            json!({ "type": "toggleMark", "markType": "bold" }),
        ),
    ));

    // The atomic snapshot evaluates the authoritative selection and its
    // stored marks together.
    let update = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(update["activeState"]["marks"]["bold"], json!(true));

    let mirrored = ok_json(&v2_render::editor_v2_render_update(
        id.clone(),
        Some(3),
        Some(3),
    ));
    assert_eq!(
        mirrored["activeState"]["marks"]["bold"],
        json!(false),
        "an explicit mirror ignores stored marks even at the installed selection"
    );
    assert_eq!(
        ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None)),
        update,
        "reading a mirror must preserve the engine's stored marks"
    );

    destroy_handle(&id);
}

#[test]
fn staging_render_accessor_errors_are_structured() {
    // Unknown session: lifecycle/ENGINE_DESTROYED on every accessor.
    let unknown = "424242".to_string();
    for result in [
        v2_render::editor_v2_render_update(unknown.clone(), None, None),
        v2_render::editor_v2_resolve_scalar_selection(unknown.clone(), 0, 0),
        v2_render::editor_v2_doc_to_scalar(unknown.clone(), 0),
        v2_render::editor_v2_scalar_to_doc(unknown.clone(), 0),
    ] {
        let error = err_json(&result);
        assert_error(&error, "lifecycle", "ENGINE_DESTROYED", None);
    }

    // Malformed handle: boundary/CONFIG_INVALID, no request id.
    let error = err_json(&v2_render::editor_v2_render_update(
        "not-a-handle".into(),
        None,
        None,
    ));
    assert_error(&error, "boundary", "CONFIG_INVALID", None);

    let id = create_handle(json!({ "initialization": { "type": "localEmpty" } }));

    // A one-sided mirror is a boundary misuse, never a guessed selection.
    for (anchor, head) in [(Some(1u32), None), (None, Some(1u32))] {
        let error = err_json(&v2_render::editor_v2_render_update(
            id.clone(),
            anchor,
            head,
        ));
        assert_error(&error, "boundary", "CONFIG_INVALID", None);
    }

    // An AwaitRemote room owns no document yet: operation/ENGINE_NOT_READY.
    let room = create_handle(room_config(None));
    for result in [
        v2_render::editor_v2_render_update(room.clone(), None, None),
        v2_render::editor_v2_resolve_scalar_selection(room.clone(), 0, 0),
        v2_render::editor_v2_doc_to_scalar(room.clone(), 0),
        v2_render::editor_v2_scalar_to_doc(room.clone(), 0),
    ] {
        let error = err_json(&result);
        assert_error(&error, "operation", "ENGINE_NOT_READY", None);
    }
    destroy_handle(&room);

    // Destroyed session: lifecycle/ENGINE_DESTROYED.
    destroy_handle(&id);
    let error = err_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_error(&error, "lifecycle", "ENGINE_DESTROYED", None);
}

/// Reported active mark state, as the toolbar reads it.
///
/// `NativeToolbarState` on iOS is built from the render update's `activeState`
/// (see `activeState["marks"]` in `NativeEditorExpoView.swift`), so that is the
/// surface a toolbar button's lit/unlit state actually comes from.
fn active_mark(id: &str, mark_type: &str) -> Value {
    let update = ok_json(&v2_render::editor_v2_render_update(
        id.to_string(),
        None,
        None,
    ));
    update["activeState"]["marks"][mark_type].clone()
}

/// Toolbar button state must update the moment the button is pressed.
///
/// Pressing bold with a collapsed caret is a state-only transaction — it stores
/// the mark without touching the document — so if the reported active state
/// ignores stored marks the button stays unlit until the user types a character
/// and the document finally carries the mark. That is the "bold doesn't light
/// up until I type" behaviour.
#[test]
fn collapsed_mark_toggle_updates_reported_toolbar_state_before_the_next_character() {
    let id = create_handle(json!({ "initialization": { "type": "localEmpty" } }));

    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(1, 0, json!({ "type": "insertText", "text": "word" })),
    ));
    assert_eq!(
        active_mark(&id, "bold"),
        json!(false),
        "precondition: bold is off while typing plain text"
    );

    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(
            2,
            revision,
            json!({ "type": "toggleMark", "markType": "bold" }),
        ),
    ));

    assert_eq!(
        active_mark(&id, "bold"),
        json!(true),
        "the bold button must read as active immediately after it is pressed, \
         before any character is typed"
    );

    // And it must stay active once the next character actually arrives.
    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(3, revision, json!({ "type": "insertText", "text": "X" })),
    ));
    assert_eq!(
        active_mark(&id, "bold"),
        json!(true),
        "bold must remain active while typing inside the bold run"
    );

    destroy_handle(&id);
}

/// The mirror: switching a mark off with a collapsed caret must clear the
/// button immediately too, rather than waiting for the next keystroke.
#[test]
fn collapsed_mark_untoggle_clears_reported_toolbar_state_immediately() {
    let id = create_handle(json!({ "initialization": { "type": "localEmpty" } }));

    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(1, 0, json!({ "type": "toggleMark", "markType": "bold" })),
    ));
    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(2, revision, json!({ "type": "insertText", "text": "bold" })),
    ));
    assert_eq!(active_mark(&id, "bold"), json!(true));

    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(
            3,
            revision,
            json!({ "type": "toggleMark", "markType": "bold" }),
        ),
    ));
    assert_eq!(
        active_mark(&id, "bold"),
        json!(false),
        "switching bold off must unlight the button before the next character"
    );

    destroy_handle(&id);
}

/// The caret the host renders, as scalar offsets.
///
/// A collapsed caret serializes as a text selection whose anchor and head
/// coincide; the scalar pair is what the native view maps onto its own text
/// storage, so it is the offset a user sees the caret drawn at.
fn caret_scalar(id: &str) -> u64 {
    let update = ok_json(&v2_render::editor_v2_render_update(
        id.to_string(),
        None,
        None,
    ));
    let selection = &update["selection"];
    assert_eq!(selection["type"], json!("text"), "{selection:?}");
    let anchor = selection["anchorScalar"]
        .as_u64()
        .unwrap_or_else(|| panic!("selection carries a scalar anchor: {selection:?}"));
    let head = selection["headScalar"]
        .as_u64()
        .unwrap_or_else(|| panic!("selection carries a scalar head: {selection:?}"));
    assert_eq!(anchor, head, "the caret must stay collapsed: {selection:?}");
    anchor
}

/// Converting a line into a list item must leave the caret on the same
/// character it was on before.
///
/// Wrapping shifts every scalar offset in the line: the bullet list, list item,
/// and paragraph opening tokens sit in front of the text, so the same character
/// reports a higher offset afterwards. If the caret is carried over as a raw
/// number rather than re-resolved through the new structure, it lands short of
/// where the user left it — visibly jumping backwards into the text.
#[test]
fn converting_a_line_into_a_list_item_keeps_the_caret_on_the_same_character() {
    let id = create_handle(json!({ "initialization": { "type": "localEmpty" } }));

    ok_json(&v2::editor_v2_apply_input(
        id.clone(),
        input_envelope(1, 0, "one"),
    ));
    assert_eq!(
        caret_scalar(&id),
        3,
        "precondition: the caret sits after the third character of a bare line"
    );

    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(
            2,
            revision,
            json!({ "type": "applyListType", "listType": "bulletList" }),
        ),
    ));

    // "one" now begins two scalars in, behind the list and item openings, so the
    // end of the same text is offset 5 rather than 3.
    assert_eq!(
        caret_scalar(&id),
        5,
        "the caret must still sit at the end of the converted line, not at the \
         offset it held before the wrap"
    );

    destroy_handle(&id);
}

#[test]
fn default_schema_list_wrap_keeps_the_caret_on_the_same_character() {
    let created = ok_json(&v2::editor_v2_create(
        json!({ "initialization": { "type": "localEmpty" } }).to_string(),
        None,
    ));
    let id = created["editorId"].as_str().unwrap().to_string();

    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(1, 0, json!({ "type": "insertText", "text": "one" })),
    ));
    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(
            2,
            revision,
            json!({
                "type": "wrapInList",
                "listType": "bullet_list",
                "itemType": "list_item"
            }),
        ),
    ));

    assert_eq!(caret_scalar(&id), 5);
    destroy_handle(&id);
}

/// The same check with the caret parked mid-word rather than at the end, so a
/// fix that merely pins the caret to the end of the line cannot pass.
#[test]
fn converting_a_line_into_a_list_item_keeps_a_mid_word_caret_in_place() {
    let id = create_handle(json!({ "initialization": { "type": "localEmpty" } }));

    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(1, 0, json!({ "type": "insertText", "text": "one" })),
    ));
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        selection_envelope(2, revision_of(&id), 1, 1),
    ));
    assert_eq!(caret_scalar(&id), 1, "precondition: caret between o and n");

    let revision = revision_of(&id);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(
            3,
            revision,
            json!({ "type": "applyListType", "listType": "bulletList" }),
        ),
    ));

    assert_eq!(
        caret_scalar(&id),
        3,
        "a caret one character into the line must still be one character in \
         after the wrap"
    );

    destroy_handle(&id);
}

/// Emptiness must be answerable from the core, not re-derived by the host.
///
/// The iOS placeholder is currently driven by scanning the rendered characters
/// in the text view's own storage (`RichTextEditorView.isRenderedContentEmpty`).
/// That scan structurally cannot see an empty list item: the bullet marker is
/// drawn from block structure rather than stored as text, so a document holding
/// one empty bullet looks character-for-character identical to an empty
/// document and the placeholder stays up over a visible bullet.
///
/// The render update is the payload the host already consumes, so it has to
/// carry a signal that separates the two.
#[test]
fn the_render_update_distinguishes_an_empty_document_from_an_empty_list_item() {
    let empty = create_handle(json!({ "initialization": { "type": "localEmpty" } }));
    let empty_update = ok_json(&v2_render::editor_v2_render_update(
        empty.clone(),
        None,
        None,
    ));

    let listed = create_handle(json!({ "initialization": { "type": "localEmpty" } }));
    ok_json(&v2::editor_v2_apply_command(
        listed.clone(),
        command_envelope(
            1,
            0,
            json!({ "type": "applyListType", "listType": "bulletList" }),
        ),
    ));
    let listed_update = ok_json(&v2_render::editor_v2_render_update(
        listed.clone(),
        None,
        None,
    ));

    assert_eq!(
        empty_update["documentIsEmpty"],
        json!(true),
        "a fresh editor holds nothing the user authored"
    );
    assert_eq!(
        listed_update["documentIsEmpty"],
        json!(false),
        "one empty bullet is content: it renders no characters, so only the \
         core can tell the host this editor is no longer empty"
    );

    destroy_handle(&empty);
    destroy_handle(&listed);
}

/// A blank second line is content too.
///
/// Pressing Return in an empty editor leaves two blank lines. Not one character
/// exists in the document, so nothing downstream of the rendered text can tell
/// this apart from an untouched editor — only the core knows the user added a
/// line, and the placeholder has to get out of the way of it.
#[test]
fn a_blank_line_added_with_return_stops_the_document_being_empty() {
    let id = create_handle(json!({ "initialization": { "type": "localEmpty" } }));

    let before = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(
        before["documentIsEmpty"],
        json!(true),
        "precondition: a fresh editor is empty"
    );

    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(1, 0, json!({ "type": "splitBlock" })),
    ));

    let after = ok_json(&v2_render::editor_v2_render_update(id.clone(), None, None));
    assert_eq!(
        after["documentIsEmpty"],
        json!(false),
        "two blank lines are content, even though neither holds a character"
    );

    // The caret belongs on the new second line, not left behind on the first.
    // Both lines are blank, so the second line is the end of the document.
    assert_eq!(
        json!(caret_scalar(&id)),
        after["scalarLength"],
        "Return must leave the caret on the blank line it just created, which \
         with both lines blank is the end of the document"
    );

    destroy_handle(&id);
}

#[test]
fn code_block_return_adds_line_then_exits_on_extra_return() {
    let id = create_handle(local_json_config(
        r#"{"type":"doc","content":[{"type":"codeBlock","attrs":{"language":"rust"},"content":[{"type":"text","text":"code"}]}]}"#,
    ));
    ok_json(&v2::editor_v2_set_selection(
        id.clone(),
        selection_envelope_with_affinity(1, revision_of(&id), 4, 4, "before"),
    ));
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(2, revision_of(&id), json!({"type":"splitBlock"})),
    ));
    assert_eq!(
        document_json_of(&id)["content"][0]["content"][0]["text"],
        "code\n"
    );
    assert_eq!(caret_scalar(&id), 5);
    ok_json(&v2::editor_v2_apply_command(
        id.clone(),
        command_envelope(3, revision_of(&id), json!({"type":"splitBlock"})),
    ));
    let document = document_json_of(&id);
    assert_eq!(document["content"].as_array().unwrap().len(), 2);
    assert_eq!(document["content"][0]["content"][0]["text"], "code");
    assert_eq!(document["content"][0]["attrs"]["language"], "rust");
    assert_eq!(document["content"][1]["type"], "paragraph");
    destroy_handle(&id);
}
