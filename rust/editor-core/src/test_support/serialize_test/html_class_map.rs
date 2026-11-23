fn class_mapped_tiptap_schema_json() -> serde_json::Value {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/schema-fingerprints.json"
    )))
    .unwrap();
    let mut schema = fixtures["fingerprints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fixture| fixture["name"] == "Tiptap-compatible camelCase schema")
        .unwrap()["schema"]
        .clone();
    let alignment_classes = serde_json::json!({
        "textAlign": {
            "center": "ql-align-center",
            "right": "ql-align-right"
        }
    });
    for node in schema["nodes"].as_array_mut().unwrap() {
        match node["name"].as_str().unwrap() {
            "paragraph" | "h2" => node["htmlClassMap"] = alignment_classes.clone(),
            "codeBlock" => {
                node["htmlClassMap"] = serde_json::json!({
                    "language": { "rust": "language-rust" }
                })
            }
            _ => {}
        }
    }
    schema
}

fn class_mapped_tiptap_schema() -> Schema {
    Schema::from_json(&class_mapped_tiptap_schema_json()).unwrap()
}

#[test]
fn html_class_map_imports_mapped_classes_and_lets_inline_alignment_win() {
    let schema = class_mapped_tiptap_schema();
    let html = concat!(
        "<p class=\"ql-align-center\">centered</p>",
        "<h2 class=\"title ql-align-right\">heading</h2>",
        "<p class=\"ql-align-justify\">unsupported</p>",
        "<p class=\"ql-align-center\" style=\"text-align:right\">style wins</p>",
        "<p style=\"text-align:center\">style only</p>",
        "<h3 class=\"ql-align-center\">unmapped node</h3>",
        "<pre class=\"language-rust\">fn main() {}</pre>"
    );
    let document = from_html(html, &schema, &FromHtmlOptions::default()).unwrap();
    let json = to_prosemirror_json(&document, &schema);
    let imported = json["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["type"].as_str().unwrap().to_string(),
                node["attrs"]["textAlign"]
                    .as_str()
                    .or_else(|| node["attrs"]["language"].as_str())
                    .map(str::to_string),
            )
        })
        .collect::<Vec<_>>();
    let expected = [
        ("paragraph", Some("center")),
        ("heading", Some("right")),
        ("paragraph", None),
        ("paragraph", Some("right")),
        ("paragraph", Some("center")),
        ("heading", None),
        ("codeBlock", Some("rust")),
    ]
    .map(|(node_type, value)| (node_type.to_string(), value.map(str::to_string)));
    assert_eq!(imported, expected, "imported attrs for {json}");
}

#[test]
fn html_class_map_exports_mapped_values_as_classes_and_others_unchanged() {
    let schema = class_mapped_tiptap_schema();
    let html = concat!(
        "<p class=\"ql-align-center\">centered</p>",
        "<h2 style=\"text-align:right\">heading</h2>",
        "<p style=\"text-align:left\">unmapped value</p>",
        "<h3 style=\"text-align:center\">unmapped node</h3>",
        "<p>plain</p>",
        "<pre class=\"language-rust\">fn main() {}</pre>"
    );
    let document = from_html(html, &schema, &FromHtmlOptions::default()).unwrap();
    let exported = to_html(&document, &schema);
    assert_eq!(
        exported,
        concat!(
            "<p class=\"ql-align-center\">centered</p>",
            "<h2 class=\"ql-align-right\">heading</h2>",
            "<p style=\"text-align:left\">unmapped value</p>",
            "<h3 style=\"text-align:center\">unmapped node</h3>",
            "<p>plain</p>",
            "<pre class=\"language-rust\">fn main() {}</pre>"
        )
    );
    let reimported = from_html(&exported, &schema, &FromHtmlOptions::default()).unwrap();
    assert_eq!(
        to_prosemirror_json(&reimported, &schema),
        to_prosemirror_json(&document, &schema),
        "class-mapped HTML round-trips"
    );
    let markdown = crate::serialize::to_markdown(
        &document,
        &schema,
        crate::serialize::MarkdownFlavor::Commonmark,
    );
    assert!(
        markdown.starts_with("<p class=\"ql-align-center\">centered</p>"),
        "Markdown HTML fallback uses the mapped class: {markdown:?}"
    );
}

#[test]
fn html_class_map_rejects_mappings_the_schema_cannot_honour() {
    for (node_name, class_map, expected_message) in [
        (
            "paragraph",
            serde_json::json!({ "missing": { "x": "x-class" } }),
            "htmlClassMap names undeclared attribute 'missing'",
        ),
        (
            "paragraph",
            serde_json::json!({ "textAlign": { "justify": "ql-align-justify" } }),
            "htmlClassMap maps unsupported value \"justify\" of attribute 'textAlign'",
        ),
        (
            "paragraph",
            serde_json::json!({ "textAlign": { "center": "two classes" } }),
            "htmlClassMap has invalid class name 'two classes'",
        ),
        (
            "paragraph",
            serde_json::json!({ "textAlign": { "center": "" } }),
            "htmlClassMap has invalid class name ''",
        ),
        (
            "paragraph",
            serde_json::json!({ "textAlign": { "center": "same", "right": "same" } }),
            "htmlClassMap maps class 'same' more than once",
        ),
        (
            "paragraph",
            serde_json::json!({ "textAlign": "ql-align-center" }),
            "node htmlClassMap must map attributes to class names",
        ),
    ] {
        let mut schema = class_mapped_tiptap_schema_json();
        let node = schema["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["name"] == node_name)
            .unwrap();
        node["htmlClassMap"] = class_map.clone();
        let error = Schema::from_json(&schema).expect_err(&format!("{class_map} must be rejected"));
        assert!(
            error.contains(expected_message),
            "{class_map} rejected with {error:?}, expected {expected_message:?}"
        );
    }
}

#[test]
fn html_class_map_rejects_a_node_that_already_uses_the_class_attribute() {
    let mut schema = class_mapped_tiptap_schema_json();
    let paragraph = schema["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|node| node["name"] == "paragraph")
        .unwrap();
    paragraph["attrs"]["class"] = serde_json::json!({ "default": null });
    let error = Schema::from_json(&schema).expect_err("class attr collides with htmlClassMap");
    assert!(
        error.contains("htmlClassMap conflicts with another use of the class attribute"),
        "unexpected error {error:?}"
    );
}
