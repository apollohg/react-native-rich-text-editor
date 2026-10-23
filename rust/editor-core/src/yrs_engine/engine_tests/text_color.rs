use super::*;

const TEXT_STYLE: &str = crate::text_color::TEXT_STYLE_MARK;
const COLOR: &str = crate::text_color::COLOR_ATTR;

fn set_color(value: serde_json::Value) -> TypedCommand {
    TypedCommand::SetMark {
        mark_type: TEXT_STYLE.into(),
        attrs: HashMap::from([(COLOR.into(), value)]),
    }
}

fn colored_document(color: serde_json::Value) -> serde_json::Value {
    json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abcd","marks":[{"type":"bold"},{"type":TEXT_STYLE,"attrs":{COLOR:color}}]}]}]})
}

#[test]
fn text_color_range_replacement_history_and_invalid_requests_are_atomic() {
    let mut engine = transaction_engine();
    engine
        .import_json(
            &colored_document(json!("red")).to_string(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    select_text(&mut engine, 1, 1, 3);
    let initial = engine.document_json().unwrap();
    engine
        .apply_command(2, set_color(json!("rgba(0, 0, 255, 0.5)")))
        .unwrap();
    let blue = engine.document_json().unwrap();
    assert_eq!(
        blue["content"][0]["content"][1]["marks"],
        json!([{"type":"bold"},{"type":TEXT_STYLE,"attrs":{COLOR:"#0000ff80"}}])
    );
    assert_eq!(
        engine.active_state().unwrap().mark_attrs[TEXT_STYLE][COLOR],
        "#0000ff80"
    );
    engine.undo(3).unwrap().unwrap();
    assert_eq!(engine.document_json().unwrap(), initial);
    engine.redo(4).unwrap().unwrap();
    assert_eq!(engine.document_json().unwrap(), blue);
    for value in [
        json!("inherit"),
        json!("url(x)"),
        json!(false),
        json!("rgba(1,2,3,no)"),
    ] {
        let before = atomic_audit(&engine);
        assert!(
            engine.apply_command(5, set_color(value.clone())).is_err(),
            "{value}"
        );
        assert_eq!(atomic_audit(&engine), before, "invalid command {value}");
        assert!(
            engine
                .import_json(
                    &colored_document(value.clone()).to_string(),
                    TransactionOrigin::DocumentImport
                )
                .is_err(),
            "{value}"
        );
        assert_eq!(atomic_audit(&engine), before, "invalid document {value}");
    }
    select_text(&mut engine, 6, 0, 4);
    let mixed = engine.active_state().unwrap();
    assert!(!mixed.marks[TEXT_STYLE]);
    assert!(!mixed.mark_attrs.contains_key(TEXT_STYLE));
    select_text(&mut engine, 7, 1, 3);
    engine
        .apply_command(
            8,
            TypedCommand::UnsetMark {
                mark_type: TEXT_STYLE.into(),
            },
        )
        .unwrap();
    assert_eq!(
        engine.document_json().unwrap()["content"][0]["content"][1]["marks"],
        json!([{"type":"bold"}])
    );
}

#[test]
fn text_color_caret_changes_only_subsequent_typing_and_preserves_other_marks() {
    let mut engine = transaction_engine();
    engine
        .import_json(
            &colored_document(json!("red")).to_string(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    select_text(&mut engine, 10, 2, 2);
    let before = engine.document_json().unwrap();
    engine.apply_command(11, set_color(json!("blue"))).unwrap();
    assert_eq!(engine.document_json().unwrap(), before);
    assert_eq!(
        engine.active_state().unwrap().mark_attrs[TEXT_STYLE][COLOR],
        "#0000ffff"
    );
    engine
        .apply_command(12, TypedCommand::InsertText { text: "X".into() })
        .unwrap();
    let inserted = engine.document_json().unwrap();
    assert_eq!(inserted["content"][0]["content"][1]["text"], "X");
    assert_eq!(
        inserted["content"][0]["content"][1]["marks"],
        json!([{"type":"bold"},{"type":TEXT_STYLE,"attrs":{COLOR:"#0000ffff"}}])
    );
    engine
        .apply_command(
            13,
            TypedCommand::UnsetMark {
                mark_type: TEXT_STYLE.into(),
            },
        )
        .unwrap();
    assert_eq!(engine.document_json().unwrap(), inserted);
    engine
        .apply_command(14, TypedCommand::InsertText { text: "Y".into() })
        .unwrap();
    let cleared = engine.document_json().unwrap();
    assert_eq!(cleared["content"][0]["content"][2]["text"], "Y");
    assert_eq!(
        cleared["content"][0]["content"][2]["marks"],
        json!([{"type":"bold"}])
    );
}

#[test]
fn text_color_remote_named_color_projects_canonically_without_repair_writes() {
    use yrs::{GetString, Text};
    let mut source = transaction_engine();
    source
        .import_json(
            &colored_document(json!("red")).to_string(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let remote = Doc::with_options(Options {
        client_id: ClientID::new(777),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    remote
        .transact_mut()
        .apply_update(Update::decode_v1(&source.encoded_state().unwrap()).unwrap())
        .unwrap();
    let root = remote.get_or_insert_xml_fragment("prosemirror");
    let mut txn = remote.transact_mut();
    let XmlOut::Element(paragraph) = root.get(&txn, 0).unwrap() else {
        panic!("paragraph missing")
    };
    let XmlOut::Text(text) = paragraph.get(&txn, 0).unwrap() else {
        panic!("text missing")
    };
    text.format(
        &mut txn,
        0,
        4,
        yrs::types::Attrs::from([(
            TEXT_STYLE.into(),
            yrs::Any::from(HashMap::from([(
                COLOR.to_string(),
                yrs::Any::from("rebeccapurple"),
            )])),
        )]),
    );
    drop(txn);
    let bytes = remote
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    let state_vector = remote.transact().state_vector();
    source.apply_remote_update_v1(20, &bytes).unwrap();
    assert_eq!(
        source.doc.transact().state_vector(),
        state_vector,
        "colour projection must not create local CRDT operations"
    );
    assert_eq!(
        source.document_json().unwrap()["content"][0]["content"][0]["marks"][1]["attrs"][COLOR],
        "#663399ff"
    );
    assert!(source.document_html().unwrap().contains("color:#663399ff"));
    assert!(
        text.get_string(&remote.transact())
            .contains("color=\"rebeccapurple\""),
        "remote Yjs must retain its original CSS value"
    );
}

#[test]
fn text_color_empty_command_attrs_use_schema_default_for_stored_typing() {
    for default in [serde_json::Value::Null, json!("red")] {
        let mut config = transaction_engine_config(crate::yrs_engine::EditingLimits::default());
        let mut marks = config.schema.all_marks().cloned().collect::<Vec<_>>();
        marks
            .iter_mut()
            .find(|mark| mark.name == TEXT_STYLE)
            .unwrap()
            .attrs
            .get_mut(COLOR)
            .unwrap()
            .default = Some(default.clone());
        config.schema =
            crate::schema::Schema::new(config.schema.all_nodes().cloned().collect(), marks);
        let mut engine = YrsDocumentEngine::new(config).unwrap();
        engine
            .apply_command(
                30,
                TypedCommand::SetMark {
                    mark_type: TEXT_STYLE.into(),
                    attrs: HashMap::new(),
                },
            )
            .unwrap();
        let expected = if default.is_null() {
            serde_json::Value::Null
        } else {
            json!("#ff0000ff")
        };
        assert_eq!(
            engine.active_state().unwrap().mark_attrs[TEXT_STYLE][COLOR],
            expected
        );
        engine
            .apply_command(
                31,
                TypedCommand::InsertText {
                    text: "typed".into(),
                },
            )
            .unwrap();
        assert_eq!(
            engine.document_json().unwrap()["content"][0]["content"][0]["marks"][0]["attrs"][COLOR],
            expected
        );
    }
}

#[test]
fn text_color_html_keeps_opaque_metadata_on_import_and_saved_document_admission() {
    let mut engine = transaction_engine();
    let html = r#"<p><span data-future="widget" style="color:red">opaque</span></p>"#;
    engine
        .import_html(
            html,
            &FromHtmlOptions::default(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let imported = engine.document_json().unwrap();
    assert_eq!(imported["content"][0]["content"][0]["type"], "__opaque");
    assert_eq!(
        imported["content"][0]["content"][0]["attrs"]["html_attrs"]["data-future"],
        "widget"
    );
    let saved = engine.encoded_state().unwrap();
    let mut restored = transaction_engine_with_resource_limits_and_mode(
        ResourceLimits::default(),
        crate::yrs_engine::InitializationMode::AwaitRemote,
    );
    restored.apply_remote_update_v1(40, &saved).unwrap();
    assert_eq!(restored.document_json().unwrap(), imported);
    assert!(restored
        .document_html()
        .unwrap()
        .contains("data-future=\"widget\""));
    assert!(restored
        .document_html()
        .unwrap()
        .contains("style=\"color:red\""));
}

#[test]
fn text_color_custom_text_style_preserves_declared_attrs_and_color_opt_in() {
    for (supports_color, allow_undeclared_attrs) in [(false, false), (false, true), (true, false)] {
        let mut config = transaction_engine_config(crate::yrs_engine::EditingLimits::default());
        let mut marks = config.schema.all_marks().cloned().collect::<Vec<_>>();
        let spec = marks
            .iter_mut()
            .find(|mark| mark.name == TEXT_STYLE)
            .unwrap();
        spec.allow_undeclared_attrs = allow_undeclared_attrs;
        if !supports_color {
            spec.attrs.remove(COLOR);
        }
        spec.attrs.insert(
            "fontfamily".into(),
            crate::schema::AttrSpec {
                default: Some(serde_json::Value::Null),
                has_default: true,
                ..Default::default()
            },
        );
        config.schema =
            crate::schema::Schema::new(config.schema.all_nodes().cloned().collect(), marks);
        let mut engine = YrsDocumentEngine::new(config).unwrap();
        for style in ["", " style=\"color:red\""] {
            let html = format!("<p><span fontfamily=\"serif\"{style}>text</span></p>");
            engine
                .import_html(
                    &html,
                    &FromHtmlOptions::default(),
                    TransactionOrigin::DocumentImport,
                )
                .unwrap();
            let document = engine.document_json().unwrap();
            let attrs = &document["content"][0]["content"][0]["marks"][0]["attrs"];
            assert_eq!(attrs["fontfamily"], "serif", "{html}");
            if supports_color && !style.is_empty() {
                assert_eq!(attrs[COLOR], "#ff0000ff");
            }
            if !supports_color {
                assert!(attrs.get(COLOR).is_none());
            }
            let exported = engine.document_html().unwrap();
            engine
                .import_html(
                    &exported,
                    &FromHtmlOptions::default(),
                    TransactionOrigin::DocumentImport,
                )
                .unwrap();
            assert_eq!(engine.document_json().unwrap(), document, "{exported}");
        }
    }
}
