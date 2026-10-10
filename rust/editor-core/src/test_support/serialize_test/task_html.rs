const TIPTAP_TASK_HTML: &str = concat!(
    "<ul data-type=\"taskList\">",
    "<li data-checked=\"true\" data-type=\"taskItem\"><label><input type=\"checkbox\" checked=\"checked\"><span></span></label><div><p><strong>Done</strong></p></div></li>",
    "<li data-checked=\"false\" data-type=\"taskItem\"><label><input type=\"checkbox\"><span></span></label><div><p>Next</p></div></li>",
    "</ul>"
);

fn tiptap_task_json() -> serde_json::Value {
    serde_json::json!({ "type": "doc", "content": [{
        "type": "taskList", "content": [
            { "type": "taskItem", "attrs": { "checked": true }, "content": [{
                "type": "paragraph", "content": [{ "type": "text", "text": "Done", "marks": [{ "type": "bold" }] }]
            }] },
            { "type": "taskItem", "content": [{
                "type": "paragraph", "content": [{ "type": "text", "text": "Next" }]
            }] }
        ]
    }] })
}

#[test]
fn task_html_parses_tiptap_wrappers_without_checkbox_content() {
    let schema = schema();
    for strict in [false, true] {
        let document = from_html(
            TIPTAP_TASK_HTML,
            &schema,
            &FromHtmlOptions {
                strict,
                ..Default::default()
            },
        )
        .expect("Tiptap checkbox markup must parse in both import modes");
        assert_eq!(to_prosemirror_json(&document, &schema), tiptap_task_json());
    }
}

#[test]
fn task_html_clipboard_ignores_accessible_checkbox_labels() {
    let schema = schema();
    let html = TIPTAP_TASK_HTML.replace("<span></span>", "<span>Task checkbox</span>");
    let document = crate::serialize::html_in::from_clipboard_html_with_limits(
        &html,
        &schema,
        &default_opts(),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(to_prosemirror_json(&document, &schema), tiptap_task_json());
}

#[test]
fn task_html_clipboard_preserves_content_around_custom_div_atoms() {
    let base = schema();
    let atoms = atom_rules_schema();
    let schema = Schema::new(
        base.all_nodes()
            .chain(std::iter::once(atoms.node("counterCard").unwrap()))
            .cloned()
            .collect(),
        base.all_marks().cloned().collect(),
    );
    let source = serde_json::json!({ "type": "doc", "content": [{
        "type": "taskList", "content": [{ "type": "taskItem", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "Before" }] },
            { "type": "counterCard", "attrs": { "title": "Card", "count": 5 } },
            { "type": "paragraph", "content": [{ "type": "text", "text": "After" }] }
        ] }]
    }] });
    let document = from_prosemirror_json(&source, &schema, UnknownTypeMode::Error).unwrap();
    let html = to_html(&document, &schema);
    let pasted = crate::serialize::html_in::from_clipboard_html_with_limits(
        &html,
        &schema,
        &default_opts(),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(
        to_prosemirror_json(&pasted, &schema),
        source,
        "HTML: {html}"
    );
}

#[test]
fn task_html_serializes_exact_tiptap_markup() {
    let schema = schema();
    let document =
        from_prosemirror_json(&tiptap_task_json(), &schema, UnknownTypeMode::Error).unwrap();
    assert_eq!(to_html(&document, &schema), TIPTAP_TASK_HTML);
}

#[test]
fn task_html_checked_uses_tiptap_boolean_rules() {
    let schema = schema();
    for (attribute, checked) in [
        ("data-checked=\"true\"", true),
        ("data-checked=\"\"", true),
        ("data-checked", true),
        ("data-checked=\"false\"", false),
        ("data-checked=\"checked\"", false),
        ("data-checked=\"TRUE\"", false),
        ("data-checked=\" true \"", false),
        ("data-checked=\"1\"", false),
        ("checked=\"true\"", false),
        ("", false),
    ] {
        let html = format!("<ul data-type=\"taskList\"><li data-type=\"taskItem\" {attribute}><label>Ignored<input type=\"checkbox\" checked><span>Ignored</span></label><div><p>Task</p></div></li></ul>");
        let document = from_html(&html, &schema, &default_opts()).unwrap();
        let item = document.root().child(0).unwrap().child(0).unwrap();
        assert_eq!(item.node_type(), "taskItem", "attribute: {attribute}");
        assert_eq!(item.attrs()["checked"], checked, "attribute: {attribute}");
        assert_eq!(
            item.child_count(),
            1,
            "checkbox label must not become content"
        );
    }
}

#[test]
fn task_html_nested_and_ordinary_lists_round_trip_without_type_confusion() {
    let schema = schema();
    let source = serde_json::json!({ "type": "doc", "content": [{
        "type": "taskList", "content": [{ "type": "taskItem", "content": [
            { "type": "paragraph" },
            tiptap_task_json()["content"][0],
            { "type": "bulletList", "content": [{ "type": "listItem", "content": [{ "type": "paragraph" }] }] },
            { "type": "orderedList", "content": [{ "type": "listItem", "content": [{ "type": "paragraph" }] }] }
        ] }]
    }] });
    let document = from_prosemirror_json(&source, &schema, UnknownTypeMode::Error).unwrap();
    let exported = to_html(&document, &schema);
    let imported = from_html(
        &exported,
        &schema,
        &FromHtmlOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        to_prosemirror_json(&imported, &schema),
        source,
        "HTML: {exported}"
    );
    let ordinary = from_html("<ul><li><p>Bullet</p></li></ul>", &schema, &default_opts()).unwrap();
    assert_eq!(ordinary.root().child(0).unwrap().node_type(), "bulletList");
    assert_eq!(
        ordinary
            .root()
            .child(0)
            .unwrap()
            .child(0)
            .unwrap()
            .node_type(),
        "listItem"
    );
}

fn html_rules_container_schema() -> Schema {
    Schema::from_json(&serde_json::json!({ "nodes": [
        { "name": "doc", "content": "block+", "role": "doc" },
        { "name": "paragraph", "content": "inline*", "group": "block", "role": "textBlock", "htmlTag": "p" },
        { "name": "callout", "content": "block+", "group": "block", "role": "block",
          "attrs": { "count": { "default": 0 } },
          "html": { "tag": "aside", "staticAttrs": { "data-type": "callout" }, "attrMap": { "count": "data-count" } } },
        { "name": "badge", "content": "text*", "group": "inline", "role": "inline",
          "attrs": { "count": { "default": 0 } },
          "html": { "tag": "span", "staticAttrs": { "data-type": "badge" }, "attrMap": { "count": "data-count" } } },
        { "name": "text", "group": "inline", "role": "text" }
    ], "marks": [] })).expect("HTML rules must admit content-bearing nodes")
}

#[test]
fn task_html_rules_support_inline_containers() {
    let schema = html_rules_container_schema();
    for html in [
        "<p>Before <span data-type=\"badge\" data-count=\"3\">inside</span> after</p>",
        "<span data-type=\"badge\" data-count=\"3\">inside</span>",
    ] {
        let document = from_html(
            html,
            &schema,
            &FromHtmlOptions {
                strict: true,
                ..Default::default()
            },
        )
        .unwrap();
        let paragraph = document.root().child(0).unwrap();
        let badge = paragraph
            .content()
            .unwrap()
            .iter()
            .find(|node| node.node_type() == "badge")
            .unwrap();
        assert_eq!(badge.attrs()["count"], 3);
        assert_eq!(badge.child(0).unwrap().text_str(), Some("inside"));
        let expected = if html.starts_with("<p>") {
            html.to_string()
        } else {
            format!("<p>{html}</p>")
        };
        assert_eq!(to_html(&document, &schema), expected);
    }
}

#[test]
fn task_html_rules_support_container_nodes_with_mapped_attributes() {
    let schema = html_rules_container_schema();
    let html = "<aside data-type=\"callout\" data-count=\"3\"><p>Inside</p></aside>";
    let document = from_html(
        html,
        &schema,
        &FromHtmlOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    let callout = document.root().child(0).unwrap();
    assert!(!callout.is_void());
    assert_eq!(callout.attrs()["count"], 3);
    assert_eq!(
        callout.child(0).unwrap().child(0).unwrap().text_str(),
        Some("Inside")
    );
    assert_eq!(to_html(&document, &schema), html);
}
