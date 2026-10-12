#[test]
fn markdown_export_defaults_to_commonmark_and_is_a_read_only_operation() {
    let editor_id = create_editor(json!({"initialization":{"type":"localJson", "json":{
        "type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"gone", "marks":[{"type":"strike"}]}]}]
    }}}));
    let before = super::editor::editor_v2_get_state(editor_id.clone()).value;
    for (flavor, expected) in [
        (None, "<s>gone</s>"),
        (Some("commonmark"), "<s>gone</s>"),
        (Some("gfm"), "~~gone~~"),
    ] {
        let result = super::editor::editor_v2_get_document_markdown(
            editor_id.clone(),
            flavor.map(str::to_owned),
        );
        assert!(result.error.is_none(), "{result:?}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.value.unwrap()).unwrap(),
            json!({"markdown":expected})
        );
    }
    assert_eq!(
        super::editor::editor_v2_get_state(editor_id.clone()).value,
        before,
        "export must leave document/history/collaboration revisions unchanged"
    );
    super::editor::editor_v2_destroy(editor_id);
}

#[test]
fn markdown_export_rejects_unknown_flavor_and_preserves_lifecycle_errors() {
    let local = create_editor(json!({"initialization":{"type":"localEmpty"}}));
    let invalid =
        super::editor::editor_v2_get_document_markdown(local.clone(), Some("markdown".into()))
            .error
            .unwrap();
    assert_eq!(invalid.code, "CONFIG_INVALID");
    assert_eq!(invalid.domain, "boundary");
    super::editor::editor_v2_destroy(local.clone());
    assert!(super::editor::editor_v2_get_document_markdown(local, None)
        .error
        .is_some());
    let room = create_editor(
        json!({"initialization":{"type":"room", "documentId":"markdown-doc", "lineageId":"markdown-lineage"}}),
    );
    let not_ready = super::editor::editor_v2_get_document_markdown(room.clone(), None)
        .error
        .unwrap();
    assert_eq!(not_ready.code, "ENGINE_NOT_READY");
    super::editor::editor_v2_destroy(room);
}

#[test]
fn markdown_export_enforces_output_limits_without_changing_state() {
    const OUTPUT_LIMIT: usize = 64 * 1024;
    let editor_id = create_editor(json!({
        "limits":{"editing":{"maxDerivedOutputBytes":OUTPUT_LIMIT}},
        "initialization":{"type":"localJson", "json":{"type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":".".repeat(16 * 1024)}]}]}}
    }));
    let before = super::editor::editor_v2_get_state(editor_id.clone()).value;
    let error = super::editor::editor_v2_get_document_markdown(editor_id.clone(), None)
        .error
        .expect("Markdown expansion must obey the configured output budget");
    assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
    assert_eq!(error.limit, Some(OUTPUT_LIMIT.to_string()));
    assert!(error.actual.unwrap().parse::<usize>().unwrap() > OUTPUT_LIMIT);
    assert_eq!(
        super::editor::editor_v2_get_state(editor_id.clone()).value,
        before
    );
    super::editor::editor_v2_destroy(editor_id);
}
