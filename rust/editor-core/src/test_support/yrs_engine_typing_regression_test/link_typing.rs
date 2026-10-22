#[test]
fn typing_after_a_loaded_html_link_does_not_extend_the_link() {
    let mut engine = engine();
    engine
        .import_html(
            r#"<p>New to LinkedIn? <a href="https://example.com/join">Join now</a></p>"#,
            &Default::default(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    place_caret(
        &mut engine,
        1,
        "New to LinkedIn? Join now".chars().count() as u32,
        Affinity::Before,
    );

    type_text(&mut engine, 2, " more");

    let content = document(&engine);
    let runs = content["content"][0]["content"].as_array().unwrap();
    assert_eq!(
        runs.len(),
        3,
        "typing after a loaded link must create a plain run: {content}"
    );
    assert_eq!(runs[1]["text"], "Join now");
    assert_eq!(
        runs[1]["marks"][0]["attrs"]["href"],
        "https://example.com/join"
    );
    assert_eq!(runs[2], plain(" more"));
}

#[test]
fn typing_after_a_rich_paste_does_not_extend_the_link() {
    let mut engine = engine();
    engine
        .apply_command(
            1,
            TypedCommand::Paste {
                fragment: None,
                html: Some(
                    r#"<p>New to LinkedIn? <a href="https://example.com/join">Join now</a></p>"#
                        .into(),
                ),
                text: Some("New to LinkedIn? Join now".into()),
                plain_text: false,
                allow_base64_images: false,
                input_filter: None,
                cell_drop: None,
            },
        )
        .unwrap()
        .expect("rich paste must apply");

    engine
        .apply_command(
            2,
            TypedCommand::ReplaceSelectionText {
                text: " more".into(),
            },
        )
        .unwrap()
        .expect("native text replacement after paste must apply");

    let content = document(&engine);
    let runs = content["content"][0]["content"].as_array().unwrap();
    assert_eq!(
        runs.len(),
        3,
        "typing after a rich paste must create a plain run: {content}"
    );
    assert_eq!(runs[1]["text"], "Join now");
    assert_eq!(runs[2], plain(" more"));
}

#[test]
fn typing_respects_link_boundaries_without_dropping_other_marks() {
    for schema in [crate::prosemirror_schema(), tiptap_schema()] {
        for (html, offset, expected_html) in [
            (
                r#"<p><a href="https://example.com">ab</a></p>"#,
                0,
                r#"<p>X<a href="https://example.com">ab</a></p>"#,
            ),
            (
                r#"<p><a href="https://example.com">ab</a></p>"#,
                1,
                r#"<p><a href="https://example.com">aXb</a></p>"#,
            ),
            (
                r#"<p><a href="https://example.com">ab</a></p>"#,
                2,
                r#"<p><a href="https://example.com">ab</a>X</p>"#,
            ),
            (
                r#"<p><strong><a href="https://example.com">ab</a></strong></p>"#,
                2,
                r#"<p><strong><a href="https://example.com">ab</a>X</strong></p>"#,
            ),
            (
                r#"<p><a href="https://example.com">ab<strong>cd</strong></a></p>"#,
                2,
                r#"<p><a href="https://example.com">abX<strong>cd</strong></a></p>"#,
            ),
            (
                r#"<p><a href="https://example.com/one">ab</a><a href="https://example.com/two">cd</a></p>"#,
                2,
                r#"<p><a href="https://example.com/one">ab</a>X<a href="https://example.com/two">cd</a></p>"#,
            ),
            (
                r#"<p><a href="https://example.com">ab</a><br></p>"#,
                3,
                r#"<p><a href="https://example.com">ab</a><br>X</p>"#,
            ),
        ] {
            let mut engine = engine_with_schema(schema.clone());
            engine
                .import_html(html, &Default::default(), TransactionOrigin::DocumentImport)
                .unwrap();
            let mut expected = engine_with_schema(schema.clone());
            expected
                .import_html(
                    expected_html,
                    &Default::default(),
                    TransactionOrigin::DocumentImport,
                )
                .unwrap();
            place_caret(&mut engine, 1, offset, Affinity::Before);

            type_text(&mut engine, 2, "X");

            assert_eq!(
                document(&engine),
                document(&expected),
                "typing at offset {offset} in {html}"
            );
        }
    }
}

#[test]
fn toggling_bold_after_a_link_does_not_restore_the_link() {
    let mut engine = engine();
    engine
        .import_html(
            r#"<p><a href="https://example.com">ab</a></p>"#,
            &Default::default(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    place_caret(&mut engine, 1, 2, Affinity::Before);
    toggle_mark(&mut engine, 2, "bold");
    type_text(&mut engine, 3, "X");
    let content = document(&engine);
    assert_eq!(content["content"][0]["content"][1], marked("X", &["bold"]));
}

#[test]
fn return_after_a_link_does_not_carry_the_link_to_the_next_line() {
    let mut engine = engine();
    engine
        .import_html(
            r#"<p><a href="https://example.com">ab</a></p>"#,
            &Default::default(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    place_caret(&mut engine, 1, 2, Affinity::Before);
    press_return(&mut engine, 2);
    type_text(&mut engine, 3, "next");
    let content = document(&engine);
    assert_eq!(
        content["content"][1],
        paragraph(serde_json::json!([plain("next")]))
    );
}

#[test]
fn explicitly_set_non_inclusive_marks_can_be_typed_and_cleared() {
    let schema = crate::schema::Schema::from_json(&serde_json::json!({
        "nodes": [
            {"name":"doc", "content":"paragraph+", "role":"doc"},
            {"name":"paragraph", "content":"text*", "role":"textBlock"},
            {"name":"text", "role":"text"}
        ],
        "marks": [{"name":"custom", "inclusive":false}]
    }))
    .unwrap();
    let mut engine = engine_with_schema(schema);
    engine
        .apply_command(
            1,
            TypedCommand::SetMark {
                mark_type: "custom".into(),
                attrs: Default::default(),
            },
        )
        .unwrap()
        .expect("setting a stored mark must apply");
    type_text(&mut engine, 2, "a");
    type_text(&mut engine, 3, "b");
    toggle_mark(&mut engine, 4, "custom");
    type_text(&mut engine, 5, "c");
    assert_eq!(
        document(&engine),
        doc(vec![paragraph(serde_json::json!([
            marked("ab", &["custom"]),
            plain("c"),
        ]))])
    );
}

#[test]
fn custom_schema_inclusivity_controls_typing_at_a_mark_end() {
    for inclusive in [None, Some(true), Some(false)] {
        let mut mark = serde_json::json!({"name":"custom"});
        if let Some(inclusive) = inclusive {
            mark["inclusive"] = inclusive.into();
        }
        let schema = crate::schema::Schema::from_json(&serde_json::json!({
            "nodes": [
                {"name":"doc", "content":"paragraph+", "role":"doc"},
                {"name":"paragraph", "content":"text*", "role":"textBlock"},
                {"name":"text", "role":"text"}
            ],
            "marks": [mark]
        }))
        .unwrap();
        let mut engine = engine_with_schema(schema);
        import_json(
            &mut engine,
            doc(vec![paragraph(serde_json::json!([marked(
                "a",
                &["custom"]
            )]))]),
        );
        place_caret(&mut engine, 1, 1, Affinity::Before);
        type_text(&mut engine, 2, "b");
        let expected = if inclusive == Some(false) {
            serde_json::json!([marked("a", &["custom"]), plain("b")])
        } else {
            serde_json::json!([marked("ab", &["custom"])])
        };
        assert_eq!(
            document(&engine),
            doc(vec![paragraph(expected)]),
            "inclusive: {inclusive:?}"
        );
    }
}

#[test]
fn replacing_link_text_preserves_the_link_only_when_it_continues_after_the_selection() {
    for (from, to, expected_html) in [
        (0, 1, r#"<p><a href="https://example.com">Xb</a></p>"#),
        (1, 2, r#"<p><a href="https://example.com">a</a>X</p>"#),
        (0, 2, "<p>X</p>"),
    ] {
        let mut engine = engine();
        engine
            .import_html(
                r#"<p><a href="https://example.com">ab</a></p>"#,
                &Default::default(),
                TransactionOrigin::DocumentImport,
            )
            .unwrap();
        let mut expected = engine_with_schema(tiptap_schema());
        expected
            .import_html(
                expected_html,
                &Default::default(),
                TransactionOrigin::DocumentImport,
            )
            .unwrap();
        let point = |offset| RevisionedPosition {
            offset,
            kind: EditorOffsetKind::Scalar,
            affinity: Affinity::Before,
        };
        engine
            .apply_typed_transaction(TypedTransaction {
                request_id: 1,
                base_document_revision: engine.revision(),
                origin: TransactionOrigin::LocalInput,
                operations: vec![],
                selection_intent: SelectionIntent::Set(SelectionInput::Text {
                    anchor: point(from),
                    head: point(to),
                }),
                history_policy: HistoryPolicy::Skip,
            })
            .unwrap();
        engine
            .apply_command(2, TypedCommand::ReplaceSelectionText { text: "X".into() })
            .unwrap()
            .expect("replacing selected link text must apply");
        assert_eq!(
            document(&engine),
            document(&expected),
            "replacing link range {from}..{to}"
        );
    }
}
