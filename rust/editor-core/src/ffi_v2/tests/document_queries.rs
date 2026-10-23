#[test]
fn document_queries_read_text_without_html_escaping_or_implicit_separators() {
    let editor_id = create_editor(json!({"initialization":{"type":"localJson", "json":{
        "type":"doc", "content":[
            {"type":"paragraph", "content":[
                {"type":"text", "text":"<&> café 👩🏽‍💻", "marks":[{"type":"bold"}]},
                {"type":"hardBreak"}, {"type":"text", "text":"tail"}
            ]},
            {"type":"blockquote", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"nested"}]}]},
            {"type":"horizontalRule"}
        ]
    }}}));
    let before = super::editor::editor_v2_get_state(editor_id.clone()).value;
    let text = super::editor::editor_v2_get_document_text(editor_id.clone());
    assert!(text.error.is_none(), "text query failed: {text:?}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text.value.unwrap()).unwrap(),
        json!({"text":"<&> café 👩🏽‍💻tailnested"})
    );
    let empty = super::editor::editor_v2_get_document_is_empty(editor_id.clone());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&empty.value.unwrap()).unwrap(),
        json!({"isEmpty":false})
    );
    assert_eq!(
        super::editor::editor_v2_get_state(editor_id.clone()).value,
        before,
        "queries must not change revisions, history, or collaboration state"
    );
    super::editor::editor_v2_destroy(editor_id);
}

#[test]
fn document_queries_preserve_structural_emptiness() {
    for (content, expected) in [
        (json!([{"type":"paragraph"}]), true),
        (json!([{"type":"paragraph"}, {"type":"paragraph"}]), false),
        (json!([{"type":"horizontalRule"}]), false),
        (
            json!([{"type":"paragraph", "content":[{"type":"text", "text":"\u{200b}"}]}]),
            false,
        ),
        (
            json!([{"type":"blockquote", "content":[{"type":"paragraph"}]}]),
            false,
        ),
    ] {
        let editor_id = create_editor(
            json!({"initialization":{"type":"localJson", "json":{"type":"doc", "content":content}}}),
        );
        let result = super::editor::editor_v2_get_document_is_empty(editor_id.clone());
        assert!(
            result.error.is_none(),
            "emptiness query failed for {content}: {result:?}"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.value.unwrap()).unwrap(),
            json!({"isEmpty":expected}),
            "structural emptiness for {content}"
        );
        super::editor::editor_v2_destroy(editor_id);
    }
}

#[test]
fn document_queries_preserve_lifecycle_errors() {
    let room = create_editor(
        json!({"initialization":{"type":"room", "documentId":"query-doc", "lineageId":"query-lineage"}}),
    );
    for result in [
        super::editor::editor_v2_get_document_text(room.clone()),
        super::editor::editor_v2_get_document_is_empty(room.clone()),
    ] {
        assert_eq!(
            result.error.expect("query before readiness must fail").code,
            "ENGINE_NOT_READY"
        );
    }
    super::editor::editor_v2_destroy(room.clone());
    for result in [
        super::editor::editor_v2_get_document_text(room.clone()),
        super::editor::editor_v2_get_document_is_empty(room.clone()),
    ] {
        assert!(result.error.is_some(), "query after destroy must fail");
    }
}

#[test]
fn document_queries_return_escaped_text_within_the_admitted_output_budget() {
    const OUTPUT_LIMIT: usize = 64 * 1024;
    const TEXT_LENGTH: usize = OUTPUT_LIMIT / 4;
    let text = "\t".repeat(TEXT_LENGTH);
    let editor_id = create_editor(json!({
        "limits":{"editing":{"maxDerivedOutputBytes":OUTPUT_LIMIT}},
        "initialization":{"type":"localJson", "json":{"type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":text}]}]}}
    }));
    let before = super::editor::editor_v2_get_state(editor_id.clone()).value;
    let result = super::editor::editor_v2_get_document_text(editor_id.clone());
    assert!(
        result.error.is_none(),
        "admitted text query failed: {result:?}"
    );
    let serialized = result.value.unwrap();
    assert!(serialized.len() <= OUTPUT_LIMIT);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&serialized).unwrap(),
        json!({"text":text})
    );
    assert_eq!(
        super::editor::editor_v2_get_state(editor_id.clone()).value,
        before
    );
    super::editor::editor_v2_destroy(editor_id);
}
