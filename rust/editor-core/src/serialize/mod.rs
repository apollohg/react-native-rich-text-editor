pub mod html_in;
pub mod html_out;
pub mod json_in;
pub mod json_out;
pub mod markdown_out;
pub use markdown_out::{to_markdown, MarkdownFlavor};

#[allow(unused_imports)]
pub use html_in::{from_html, from_html_with_limits, FromHtmlOptions, ParseError};
pub use html_out::to_html;
#[allow(unused_imports)]
pub use json_in::{
    from_prosemirror_json, from_prosemirror_json_with_limits, JsonParseError, UnknownTypeMode,
};
pub(crate) use json_in::{
    normalized_wire_json_node_type, parse_wire_heading_level_str, rehydrate_reserved_html_opaque,
};
pub(crate) use json_out::node_to_json as node_to_prosemirror_json;
pub use json_out::to_prosemirror_json;

const TASK_CHECKED_ATTR: &str = "checked";
const TASK_CONTENT_HTML_TAG: &str = "div";

fn is_tiptap_task_item(spec: &crate::schema::NodeSpec) -> bool {
    matches!(spec.role, crate::schema::NodeRole::ListItem)
        && spec.html_rules.as_ref().is_some_and(|rules| {
            rules.static_attrs.iter().any(|(name, value)| {
                name == "data-type" && value == crate::schema::presets::TASK_ITEM_NODE_NAME
            })
        })
}

fn reset_unset_attr(
    attrs: &mut std::collections::HashMap<String, serde_json::Value>,
    spec: &crate::schema::NodeSpec,
    name: &str,
) {
    match spec.attrs.get(name).and_then(|attr| attr.default.as_ref()) {
        Some(default) => {
            attrs.insert(
                name.to_string(),
                crate::boundary::clone_json_value_stack_safe(default),
            );
        }
        None => {
            attrs.remove(name);
        }
    }
}

fn default_node_attrs(
    spec: &crate::schema::NodeSpec,
) -> std::collections::HashMap<String, serde_json::Value> {
    spec.attrs
        .iter()
        .filter_map(|(name, attr)| {
            attr.default.as_ref().map(|value| {
                (
                    name.clone(),
                    crate::boundary::clone_json_value_stack_safe(value),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod text_color_tests {
    use super::*;
    use crate::schema::presets::tiptap_schema;

    #[test]
    fn text_color_html_nested_overrides_and_uncolored_spans() {
        let schema = tiptap_schema();
        let doc = from_html(r#"<p><strong><span style="color: red">a<span>b</span><span style="color: #0f08">c</span><span style="color: var(--x)">d</span></span></strong></p>"#, &schema, &FromHtmlOptions::default()).unwrap();
        let json = to_prosemirror_json(&doc, &schema);
        let text = json["content"][0]["content"].as_array().unwrap();
        let runs: Vec<_> = text
            .iter()
            .map(|run| {
                (
                    run["text"].as_str().unwrap(),
                    run["marks"][1]["attrs"]["color"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            runs,
            vec![
                ("a", "#ff0000ff"),
                ("b", "#ff0000ff"),
                ("c", "#00ff0088"),
                ("d", "#ff0000ff")
            ]
        );
        let html = to_html(&doc, &schema);
        assert!(html.contains("style=\"color:#00ff0088\""), "{html}");
        assert_eq!(
            to_prosemirror_json(
                &from_html(&html, &schema, &FromHtmlOptions::default()).unwrap(),
                &schema
            ),
            json
        );
    }

    #[test]
    fn text_color_html_preserves_custom_span_marks_and_opaque_spans() {
        let base = tiptap_schema();
        let mut marks = base.all_marks().cloned().collect::<Vec<_>>();
        marks.push(crate::schema::MarkSpec {
            name: "highlight".into(),
            html_tag: Some("span".into()),
            attrs: Default::default(),
            excludes: None,
            inclusive: true,
            allow_undeclared_attrs: false,
        });
        let schema = crate::schema::Schema::new(base.all_nodes().cloned().collect(), marks);
        let doc = from_html(
            r#"<p><span style="color:blue">custom</span></p>"#,
            &schema,
            &FromHtmlOptions::default(),
        )
        .unwrap();
        let json = to_prosemirror_json(&doc, &schema);
        assert_eq!(
            json["content"][0]["content"][0]["marks"],
            serde_json::json!([{"type":"highlight"},{"type":"textStyle","attrs":{"color":"#0000ffff"}}])
        );
        let doc = from_html(
            r#"<p><span data-future="widget">opaque</span></p>"#,
            &base,
            &FromHtmlOptions::default(),
        )
        .unwrap();
        assert_eq!(
            doc.root().child(0).unwrap().child(0).unwrap().node_type(),
            "__opaque"
        );
    }

    #[test]
    fn text_color_clipboard_preserves_nested_colors_and_discards_unsupported_css() {
        let schema = tiptap_schema();
        let doc = html_in::from_clipboard_html_with_limits(
            r#"<p><span style="color:red;background-image:url(evil)">outer<span style="color:#0f08;font-weight:bold">inner</span><span style="color:var(--unknown)">inherits</span></span></p>"#,
            &schema, &FromHtmlOptions::default(), &crate::boundary::ResourceLimits::default(),
        ).unwrap();
        let paragraph = doc.root().child(0).unwrap();
        let runs = paragraph
            .content()
            .unwrap()
            .iter()
            .map(|node| {
                (
                    node.text_str().unwrap(),
                    node.marks()
                        .iter()
                        .find(|mark| mark.mark_type() == crate::text_color::TEXT_STYLE_MARK)
                        .unwrap()
                        .attrs()[crate::text_color::COLOR_ATTR]
                        .as_str()
                        .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            runs,
            vec![
                ("outer", "#ff0000ff"),
                ("inner", "#00ff0088"),
                ("inherits", "#ff0000ff")
            ]
        );
        assert!(paragraph
            .child(1)
            .unwrap()
            .marks()
            .iter()
            .any(|mark| mark.mark_type() == "bold"));
        let html = to_html(&doc, &schema);
        assert!(!html.contains("background"));
        assert!(!html.contains("var("));
    }

    #[test]
    fn text_color_json_canonicalizes_and_rejects_invalid_values() {
        let schema = tiptap_schema();
        for (input, expected) in [
            ("red", "#ff0000ff"),
            ("rebeccapurple", "#663399ff"),
            ("#AbC", "#aabbccff"),
            ("rgba(12, 34, 56, 0.5)", "#0c223880"),
            ("transparent", "#00000000"),
        ] {
            let json = serde_json::json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hello","marks":[{"type":"textStyle","attrs":{"color":input}}]}]}]});
            let doc = from_prosemirror_json(&json, &schema, UnknownTypeMode::Error).unwrap();
            assert_eq!(
                to_prosemirror_json(&doc, &schema)["content"][0]["content"][0]["marks"][0]["attrs"]
                    ["color"],
                expected,
                "{input}"
            );
        }
        for input in [
            serde_json::json!("currentColor"),
            serde_json::json!("red; background:blue"),
            serde_json::json!(42),
        ] {
            let json = serde_json::json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hello","marks":[{"type":"textStyle","attrs":{"color":input}}]}]}]});
            assert!(
                from_prosemirror_json(&json, &schema, UnknownTypeMode::Error).is_err(),
                "{input}"
            );
        }
    }
}
