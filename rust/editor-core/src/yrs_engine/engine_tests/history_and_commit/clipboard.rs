fn clipboard_paste(
    fragment: Option<String>,
    html: Option<String>,
    text: Option<String>,
    plain_text: bool,
) -> TypedCommand {
    TypedCommand::Paste {
        fragment,
        html,
        text,
        plain_text,
        allow_base64_images: false,
        input_filter: None,
    }
}

#[test]
fn clipboard_rich_replacement_is_atomic_and_undoable() {
    let mut source = transaction_engine();
    source.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a😀b","marks":[{"type":"bold"}]}]},{"type":"paragraph","content":[{"type":"text","text":"tail"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut source, 1, 1, 5);
    let copied = source.clipboard().unwrap();
    assert_eq!(copied["text"], "😀b\nt");
    let mut target = transaction_engine();
    target.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut target, 2, 2, 9);
    let before = target.document_json();
    let result = target
        .apply_command(
            3,
            clipboard_paste(
                copied["fragment"].as_str().map(str::to_owned),
                None,
                None,
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert!(result.changed);
    assert_eq!(
        replacement_blocks(&target),
        vec![
            ("paragraph".into(), "be😀b".into()),
            ("paragraph".into(), "tter".into())
        ]
    );
    let after = target.document_json();
    target.undo_with_result(4).unwrap().unwrap();
    assert_eq!(target.document_json(), before);
    target.redo_with_result(5).unwrap().unwrap();
    assert_eq!(target.document_json(), after);
}

#[test]
fn clipboard_malformed_falls_back_without_partial_mutation() {
    let mut engine = transaction_engine();
    engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abcdef"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut engine, 1, 1, 5);
    engine
        .apply_command(
            2,
            clipboard_paste(
                Some("{bad".into()),
                Some("<p><strong>rich</strong></p>".into()),
                Some("plain".into()),
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(engine.document().unwrap().root().text_content(), "arichf");
    assert_eq!(
        engine
            .document()
            .unwrap()
            .root()
            .child(0)
            .unwrap()
            .child(1)
            .unwrap()
            .marks()[0]
            .mark_type(),
        "bold"
    );
}

#[test]
fn clipboard_plain_from_html_inherits_destination_marks() {
    let mut engine = transaction_engine();
    engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"ab","marks":[{"type":"italic"}]}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut engine, 1, 1, 1);
    engine
        .apply_command(
            2,
            clipboard_paste(
                None,
                Some("<p><strong>plain</strong></p>".into()),
                None,
                true,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(engine.document().unwrap().root().text_content(), "aplainb");
    assert!(engine
        .document()
        .unwrap()
        .root()
        .child(0)
        .unwrap()
        .content()
        .unwrap()
        .iter()
        .all(|node| node.marks().iter().all(|mark| mark.mark_type() == "italic")));
}

#[test]
fn clipboard_explicit_empty_plain_cuts_selected_content_once() {
    let mut engine = transaction_engine();
    engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abcdef"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut engine, 1, 1, 5);
    engine
        .apply_command(2, clipboard_paste(None, None, Some("".into()), true))
        .unwrap()
        .unwrap();
    assert_eq!(engine.document().unwrap().root().text_content(), "af");
    engine.undo_with_result(3).unwrap().unwrap();
    assert_eq!(engine.document().unwrap().root().text_content(), "abcdef");
}

#[test]
fn clipboard_html_multiple_paragraphs_join_destination_edges() {
    let mut engine = transaction_engine();
    engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abcdef"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut engine, 1, 2, 4);
    engine
        .apply_command(
            2,
            clipboard_paste(None, Some("<p>one</p><p>two</p>".into()), None, false),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        replacement_blocks(&engine),
        vec![
            ("paragraph".into(), "abone".into()),
            ("paragraph".into(), "twoef".into())
        ]
    );
}

#[test]
fn clipboard_atom_copy_paste_and_cut_keep_attrs_and_undo() {
    let mut engine = transaction_engine();
    engine.import_json(r#"{"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.com/image.png","alt":"diagram","width":123,"height":45}}]}"#, TransactionOrigin::DocumentImport).unwrap();
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: 1,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![],
            selection_intent: SelectionIntent::Set(SelectionInput::Node {
                at: RevisionedPosition {
                    offset: 0,
                    kind: EditorOffsetKind::Scalar,
                    affinity: Affinity::After,
                },
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    let before = engine.document_json();
    let copied = engine.clipboard().unwrap();
    assert_eq!(copied["text"], "diagram");
    engine
        .apply_command(2, clipboard_paste(None, None, Some("".into()), true))
        .unwrap()
        .unwrap();
    assert_eq!(
        engine
            .document()
            .unwrap()
            .root()
            .child(0)
            .unwrap()
            .node_type(),
        "paragraph"
    );
    engine.undo_with_result(3).unwrap().unwrap();
    assert_eq!(engine.document_json(), before);
    engine
        .apply_command(
            4,
            clipboard_paste(
                copied["fragment"].as_str().map(str::to_owned),
                None,
                None,
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(engine.document_json(), before);
}

#[test]
fn clipboard_plain_node_selection_replaces_atom() {
    let mut engine = transaction_engine();
    engine
        .import_json(
            r#"{"type":"doc","content":[{"type":"horizontalRule"}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: 1,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![],
            selection_intent: SelectionIntent::Set(SelectionInput::Node {
                at: RevisionedPosition {
                    offset: 0,
                    kind: EditorOffsetKind::Scalar,
                    affinity: Affinity::After,
                },
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    engine
        .apply_command(
            2,
            clipboard_paste(None, None, Some("replacement".into()), true),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        replacement_blocks(&engine),
        vec![("paragraph".into(), "replacement".into())]
    );
}

#[test]
fn clipboard_empty_rich_falls_back_and_filter_preserves_source_marks() {
    let mut engine = transaction_engine();
    engine
        .apply_command(
            1,
            clipboard_paste(
                None,
                Some("<script>evil</script>".into()),
                Some("fallback".into()),
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(engine.document().unwrap().root().text_content(), "fallback");
    select_text(&mut engine, 2, 0, 8);
    let mut command = clipboard_paste(None, Some("<p><b>a1b2</b></p>".into()), None, false);
    if let TypedCommand::Paste { input_filter, .. } = &mut command {
        *input_filter = Some("[a-z]".into());
    }
    engine.apply_command(3, command).unwrap().unwrap();
    assert_eq!(engine.document().unwrap().root().text_content(), "ab");
    assert_eq!(
        engine
            .document()
            .unwrap()
            .root()
            .child(0)
            .unwrap()
            .child(0)
            .unwrap()
            .marks()[0]
            .mark_type(),
        "bold"
    );
}

#[test]
fn clipboard_closed_list_replaces_empty_destination_block() {
    let mut source = transaction_engine();
    source.import_json(r#"{"type":"doc","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"outer"}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"inner"}]}]}]}]}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    let data =
        crate::clipboard::export(source.document().unwrap(), &Selection::All, &source.schema)
            .unwrap();
    let mut target = transaction_engine();
    target
        .apply_command(
            1,
            clipboard_paste(
                data["fragment"].as_str().map(str::to_owned),
                None,
                None,
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(target.document_json(), source.document_json());
}

#[test]
fn clipboard_incompatible_fragment_supplies_plain_text_without_source_schema() {
    let mut engine = transaction_engine();
    let fragment = json!({"version":1,"openStart":1,"openEnd":1,"text":"Sam","document":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"foreignMention","attrs":{"label":"Sam"}}]}]}}).to_string();
    engine
        .apply_command(1, clipboard_paste(Some(fragment), None, None, true))
        .unwrap()
        .unwrap();
    assert_eq!(engine.document().unwrap().root().text_content(), "Sam");
}

#[test]
fn clipboard_partial_nested_list_retains_open_boundaries() {
    let mut source = transaction_engine();
    source.import_json(r#"{"type":"doc","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"outer"}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"inner"}]}]}]}]}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    let from = source
        .position_map()
        .unwrap()
        .doc_to_scalar(13, source.document().unwrap());
    let to = source
        .position_map()
        .unwrap()
        .doc_to_scalar(15, source.document().unwrap());
    select_text(&mut source, 1, from, to);
    let copied = source.clipboard().unwrap();
    assert_eq!(copied["text"], "nn");
    source
        .apply_command(
            2,
            clipboard_paste(
                copied["fragment"].as_str().map(str::to_owned),
                None,
                None,
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        source.document().unwrap().root().text_content(),
        "outerinner"
    );
    assert_eq!(
        source
            .document()
            .unwrap()
            .root()
            .child(0)
            .unwrap()
            .node_type(),
        "bulletList"
    );
}

#[test]
fn clipboard_forbidden_image_falls_back_without_deleting_selection() {
    let mut source = transaction_engine();
    source.import_json(r#"{"type":"doc","content":[{"type":"image","attrs":{"src":"data:image/png;base64,AAAA","alt":"diagram"}}]}"#, TransactionOrigin::DocumentImport).unwrap();
    let copied =
        crate::clipboard::export(source.document().unwrap(), &Selection::All, &source.schema)
            .unwrap();
    let mut target = transaction_engine();
    target
        .apply_command(
            1,
            clipboard_paste(
                copied["fragment"].as_str().map(str::to_owned),
                None,
                Some("fallback".into()),
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(target.document().unwrap().root().text_content(), "fallback");
}

#[test]
fn clipboard_block_atom_pastes_at_empty_and_text_destinations() {
    let mut source = transaction_engine();
    source.import_json(r#"{"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.com/a.png","alt":"A"}}]}"#, TransactionOrigin::DocumentImport).unwrap();
    let copied =
        crate::clipboard::export(source.document().unwrap(), &Selection::All, &source.schema)
            .unwrap();
    for initial in [
        r#"{"type":"doc","content":[{"type":"paragraph"}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"prefix"}]}]}"#,
    ] {
        let mut target = transaction_engine();
        target
            .import_json(initial, TransactionOrigin::DocumentImport)
            .unwrap();
        let end = target
            .document()
            .unwrap()
            .root()
            .text_content()
            .chars()
            .count() as u32;
        select_text(&mut target, 1, end, end);
        target
            .apply_command(
                2,
                clipboard_paste(
                    copied["fragment"].as_str().map(str::to_owned),
                    None,
                    None,
                    false,
                ),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            target
                .document()
                .unwrap()
                .root()
                .content()
                .unwrap()
                .iter()
                .last()
                .unwrap()
                .node_type(),
            "image"
        );
        assert!(matches!(
            target.resolved_selection(),
            Some(ResolvedSelection::Node { .. })
        ));
    }
}

#[test]
fn clipboard_partial_list_pastes_marks_into_paragraph() {
    let mut source = transaction_engine();
    source.import_json(r#"{"type":"doc","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"item","marks":[{"type":"bold"}]}]}]}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    let copied = crate::clipboard::export(
        source.document().unwrap(),
        &Selection::text(4, 6),
        &source.schema,
    )
    .unwrap();
    let mut target = transaction_engine();
    target.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"ab"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut target, 1, 1, 1);
    target
        .apply_command(
            2,
            clipboard_paste(
                copied["fragment"].as_str().map(str::to_owned),
                None,
                None,
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(target.document().unwrap().root().text_content(), "ateb");
    assert_eq!(target.document().unwrap().root().child_count(), 1);
    assert_eq!(
        target
            .document()
            .unwrap()
            .root()
            .child(0)
            .unwrap()
            .child(1)
            .unwrap()
            .marks()[0]
            .mark_type(),
        "bold"
    );
}

#[test]
fn clipboard_external_headings_and_code_blocks_keep_structure() {
    for (html, expected_type) in [("<h1>Heading</h1>", "h1"), ("<pre>code</pre>", "codeBlock")] {
        let mut engine = transaction_engine();
        select_text(&mut engine, 1, 0, 0);
        engine
            .apply_command(2, clipboard_paste(None, Some(html.into()), None, false))
            .unwrap()
            .unwrap();
        assert_eq!(
            engine
                .document()
                .unwrap()
                .root()
                .child(0)
                .unwrap()
                .node_type(),
            expected_type
        );
    }
}

#[test]
fn clipboard_filtered_inline_atom_uses_fallback_without_deleting_selection() {
    let schema = crate::schema::Schema::from_json(&json!({"nodes":[
        {"name":"doc","role":"doc","content":"block+"},
        {"name":"paragraph","role":"textBlock","group":"block","content":"inline*","htmlTag":"p"},
        {"name":"text","role":"text","group":"inline"},
        {"name":"inlineImage","role":"inline","group":"inline","isVoid":true,"attrs":{"src":{},"alt":{"default":""}}}
    ],"marks":[]})).unwrap();
    let config = YrsEngineConfig {
        schema,
        fragment_name: "prosemirror".into(),
        initialization_mode: crate::yrs_engine::InitializationMode::LocalEmpty,
        resource_limits: ResourceLimits::default(),
        editing_limits: Default::default(),
        max_length: None,
        scope: None,
    };
    let mut source = YrsDocumentEngine::new(config.clone()).unwrap();
    source.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"inlineImage","attrs":{"src":"data:image/png;base64,AAAA","alt":"diagram"}}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    let copied =
        crate::clipboard::export(source.document().unwrap(), &Selection::All, &source.schema)
            .unwrap();
    let mut target = YrsDocumentEngine::new(config).unwrap();
    target.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"selected"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut target, 1, 0, 8);
    target
        .apply_command(
            2,
            clipboard_paste(
                copied["fragment"].as_str().map(str::to_owned),
                None,
                Some("fallback".into()),
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(target.document().unwrap().root().text_content(), "fallback");
}

#[test]
fn clipboard_input_filter_preserves_unlabelled_atoms() {
    let mut source = transaction_engine();
    source
        .import_json(
            r#"{"type":"doc","content":[{"type":"horizontalRule"}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let copied =
        crate::clipboard::export(source.document().unwrap(), &Selection::All, &source.schema)
            .unwrap();
    let mut target = transaction_engine();
    let mut command = clipboard_paste(
        copied["fragment"].as_str().map(str::to_owned),
        None,
        None,
        false,
    );
    if let TypedCommand::Paste { input_filter, .. } = &mut command {
        *input_filter = Some("[a-z]".into());
    }
    target.apply_command(1, command).unwrap().unwrap();
    assert_eq!(
        target
            .document()
            .unwrap()
            .root()
            .child(0)
            .unwrap()
            .node_type(),
        "horizontalRule"
    );
}

#[test]
fn clipboard_intentionally_empty_block_still_replaces_selection() {
    let source = transaction_engine();
    let copied =
        crate::clipboard::export(source.document().unwrap(), &Selection::All, &source.schema)
            .unwrap();
    let mut target = transaction_engine();
    target.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"selected"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
    select_text(&mut target, 1, 0, 8);
    target
        .apply_command(
            2,
            clipboard_paste(
                copied["fragment"].as_str().map(str::to_owned),
                None,
                None,
                false,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        replacement_blocks(&target),
        vec![("paragraph".into(), "".into())]
    );
}

#[test]
fn clipboard_copied_heading_keeps_type_in_empty_paragraph() {
    for (initial, from, to, expected) in [
        (
            r#"{"type":"doc","content":[{"type":"paragraph"}]}"#,
            0,
            0,
            vec![("h1".into(), "Heading".into())],
        ),
        (
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"replace"}]}]}"#,
            0,
            7,
            vec![("h1".into(), "Heading".into())],
        ),
        (
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"paragraph"},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#,
            7,
            7,
            vec![
                ("paragraph".into(), "before".into()),
                ("h1".into(), "Heading".into()),
                ("paragraph".into(), "after".into()),
            ],
        ),
    ] {
        let mut source = transaction_engine();
        source.import_json(r#"{"type":"doc","content":[{"type":"h1","content":[{"type":"text","text":"Heading"}]}]}"#, TransactionOrigin::DocumentImport).unwrap();
        select_text(&mut source, 1, 0, 7);
        let copied = source.clipboard().unwrap();
        let mut target = transaction_engine();
        target
            .import_json(initial, TransactionOrigin::DocumentImport)
            .unwrap();
        select_text(&mut target, 1, from, to);
        let before = target.document_json();
        target
            .apply_command(
                2,
                clipboard_paste(
                    copied["fragment"].as_str().map(str::to_owned),
                    None,
                    None,
                    false,
                ),
            )
            .unwrap()
            .unwrap();
        assert_eq!(replacement_blocks(&target), expected);
        let after = target.document_json();
        target.undo_with_result(3).unwrap().unwrap();
        assert_eq!(target.document_json(), before);
        target.redo_with_result(4).unwrap().unwrap();
        assert_eq!(target.document_json(), after);
    }
}
