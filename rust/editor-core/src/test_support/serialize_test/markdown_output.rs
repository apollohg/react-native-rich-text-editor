use crate::serialize::{to_markdown, MarkdownFlavor};

fn markdown_render(markdown: &str, flavor: MarkdownFlavor) -> String {
    let options = match flavor {
        MarkdownFlavor::Commonmark => pulldown_cmark::Options::empty(),
        MarkdownFlavor::Gfm => {
            pulldown_cmark::Options::ENABLE_TABLES
                | pulldown_cmark::Options::ENABLE_TASKLISTS
                | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
        }
    };
    let mut html = String::new();
    pulldown_cmark::html::push_html(
        &mut html,
        pulldown_cmark::Parser::new_ext(markdown, options),
    );
    html
}

fn markdown_from_json(json: serde_json::Value, schema: &Schema, flavor: MarkdownFlavor) -> String {
    let document = from_prosemirror_json(&json, schema, UnknownTypeMode::Error).unwrap();
    to_markdown(&document, schema, flavor)
}

fn markdown_roundtrip(json: serde_json::Value, schema: &Schema, flavor: MarkdownFlavor) -> String {
    let document = from_prosemirror_json(&json, schema, UnknownTypeMode::Error).unwrap();
    let markdown = to_markdown(&document, schema, flavor);
    let html = markdown_render(&markdown, flavor);
    let formatting_newlines = regex::Regex::new(r"(<!--.*?-->|<(?:/?(?:p|h[1-6]|blockquote|ol|ul|li|hr|table|thead|tbody|tr|th|td|pre)|br)[^>]*>)\n").unwrap();
    let compact = formatting_newlines.replace_all(&html, "$1");
    let parsed = from_html(
        compact.trim_end_matches('\n'),
        schema,
        &FromHtmlOptions::default(),
    )
    .unwrap();
    fn sort_marks(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(marks) = object
                    .get_mut("marks")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    marks.sort_by_key(serde_json::Value::to_string);
                }
                for child in object.values_mut() {
                    sort_marks(child);
                }
                if let Some(content) = object
                    .get_mut("content")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    let mut merged: Vec<serde_json::Value> = Vec::new();
                    for child in content.drain(..) {
                        if let Some(previous) = merged.last_mut().filter(|previous| {
                            previous["type"] == "text"
                                && child["type"] == "text"
                                && previous.get("marks") == child.get("marks")
                        }) {
                            previous["text"] = format!(
                                "{}{}",
                                previous["text"].as_str().unwrap(),
                                child["text"].as_str().unwrap()
                            )
                            .into();
                        } else {
                            merged.push(child);
                        }
                    }
                    *content = merged;
                }
            }
            serde_json::Value::Array(children) => {
                for child in children {
                    sort_marks(child);
                }
            }
            _ => {}
        }
    }
    let mut actual = to_prosemirror_json(&parsed, schema);
    let mut expected = to_prosemirror_json(&document, schema);
    sort_marks(&mut actual);
    sort_marks(&mut expected);
    assert_eq!(
        actual, expected,
        "Markdown changed document semantics:\n{markdown}\nRendered HTML:\n{html}"
    );
    markdown
}

#[test]
fn markdown_standard_blocks_roundtrip_in_both_naming_conventions() {
    for schema in [crate::tiptap_schema(), crate::prosemirror_schema()] {
        let ordered = schema
            .all_nodes()
            .find(|spec| matches!(spec.role, NodeRole::List { ordered: true }))
            .unwrap()
            .name
            .clone();
        let bullet = schema
            .all_nodes()
            .find(|spec| {
                matches!(spec.role, NodeRole::List { ordered: false }) && spec.html_rules.is_none()
            })
            .unwrap()
            .name
            .clone();
        let item = schema
            .all_nodes()
            .find(|spec| matches!(spec.role, NodeRole::ListItem) && spec.html_rules.is_none())
            .unwrap()
            .name
            .clone();
        let hard_break = schema
            .all_nodes()
            .find(|spec| matches!(spec.role, NodeRole::HardBreak))
            .unwrap()
            .name
            .clone();
        let rule = schema
            .all_nodes()
            .find(|spec| spec.html_tag.as_deref() == Some("hr"))
            .unwrap()
            .name
            .clone();
        let json = serde_json::json!({"type":"doc", "content":[
            {"type":"heading", "attrs":{"level":2}, "content":[{"type":"text", "text":"Title"}]},
            {"type":"paragraph", "content":[{"type":"text", "text":"before"}, {"type":hard_break}, {"type":"text", "text":"after"}]},
            {"type":"blockquote", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"Quote"}]}]},
            {"type":ordered, "attrs":{"start":9}, "content":[{"type":item, "content":[
                {"type":"paragraph", "content":[{"type":"text", "text":"first"}]},
                {"type":bullet, "content":[{"type":item, "content":[{"type":"paragraph", "content":[{"type":"text", "text":"nested"}]}]}]},
                {"type":"paragraph", "content":[{"type":"text", "text":"continuation"}]}
            ]}, {"type":item, "content":[{"type":"paragraph", "content":[{"type":"text", "text":"second"}]}]}]},
            {"type":rule}
        ]});
        let markdown = markdown_roundtrip(json, &schema, MarkdownFlavor::Commonmark);
        assert!(markdown.starts_with("## Title\n\n"), "{markdown}");
        assert!(markdown.contains("9. first"), "{markdown}");
        assert!(markdown.contains("10. second"), "{markdown}");
    }
}

#[test]
fn markdown_literal_syntax_unicode_and_indentation_remain_text() {
    for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
        for text in [
            "# heading",
            "1. list",
            "- bullet",
            "[x] task",
            "***",
            "[link](url)",
            "<b>html</b> &amp;",
            r"a_b *c* ~d~ `e` | f \",
            "    indented",
            "a\n\nb",
            "https://example.org user@example.org www.example.org",
            "中文 café 🦀",
        ] {
            markdown_roundtrip(
                serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":text}]}]}),
                &schema(),
                flavor,
            );
        }
    }
}

#[test]
fn markdown_marks_crossing_ranges_whitespace_and_punctuation_roundtrip() {
    for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
        let markdown = markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
                {"type":"text", "text":"bold ", "marks":[{"type":"bold"}]},
                {"type":"text", "text":"both", "marks":[{"type":"bold"},{"type":"italic"}]},
                {"type":"text", "text":" italic", "marks":[{"type":"italic"}]},
                {"type":"text", "text":" under ", "marks":[{"type":"underline"}]},
                {"type":"text", "text":"strike", "marks":[{"type":"strike"}]},
                {"type":"text", "text":"a"}, {"type":"text", "text":"!", "marks":[{"type":"bold"}]}, {"type":"text", "text":"b"}
            ]}]}),
            &schema(),
            flavor,
        );
        assert!(markdown.contains("<u>"), "{markdown}");
        if flavor == MarkdownFlavor::Gfm {
            let strike = markdown_from_json(
                serde_json::json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"strike","marks":[{"type":"strike"}]}]}]}),
                &schema(),
                flavor,
            );
            assert_eq!(strike, "~~strike~~");
        }
    }
    let markdown = markdown_from_json(
        serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
            {"type":"text", "text":"one ", "marks":[{"type":"bold"}]},
            {"type":"text", "text":"two", "marks":[{"type":"bold"}]}
        ]}]}),
        &schema(),
        MarkdownFlavor::Commonmark,
    );
    assert_eq!(markdown, "**one two**");
    assert_eq!(
        markdown_render(&markdown, MarkdownFlavor::Commonmark),
        "<p><strong>one two</strong></p>\n"
    );
}

#[test]
fn markdown_code_delimiters_and_languages_do_not_change_code() {
    for text in [
        "a`b",
        "`edge`",
        "  ",
        " a ",
        "first\nsecond",
        "*** \\ <tag>",
    ] {
        markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":text, "marks":[{"type":"code"}]}]}]}),
            &schema(),
            MarkdownFlavor::Commonmark,
        );
    }
    let markdown = markdown_from_json(
        serde_json::json!({"type":"doc", "content":[{"type":"codeBlock", "attrs":{"language":"rust"}, "content":[{"type":"text", "text":"```\nlet x = `value`;\n"}]}]}),
        &schema(),
        MarkdownFlavor::Commonmark,
    );
    assert!(markdown.starts_with("````rust\n"), "{markdown}");
    let html = markdown_render(&markdown, MarkdownFlavor::Commonmark);
    let parsed = scraper::Html::parse_fragment(&html);
    let code = parsed
        .select(&scraper::Selector::parse("pre > code").unwrap())
        .next()
        .unwrap();
    assert_eq!(code.text().collect::<String>(), "```\nlet x = `value`;\n");
    assert_eq!(code.value().attr("class"), Some("language-rust"));
}

#[test]
fn markdown_links_and_images_escape_destinations_and_metadata() {
    let json = serde_json::json!({"type":"doc", "content":[
        {"type":"paragraph", "content":[{"type":"text", "text":"click here", "marks":[{"type":"link", "attrs":{"href":"https://example.org/a(b)?q=x&n=1"}},{"type":"bold"}]}]},
        {"type":"image", "attrs":{"src":"https://example.org/a(b).png", "alt":"a [picture]", "title":"quoted \"title\""}},
        {"type":"image", "attrs":{"src":"https://example.org/sized.png", "width":200, "height":100}}
    ]});
    let markdown = markdown_from_json(json, &schema(), MarkdownFlavor::Commonmark);
    assert!(markdown.contains("[**click"), "{markdown}");
    assert!(markdown.contains("!["), "{markdown}");
    assert!(markdown.contains("<img"), "{markdown}");
    let html = markdown_render(&markdown, MarkdownFlavor::Commonmark);
    let parsed = scraper::Html::parse_fragment(&html);
    let link = parsed
        .select(&scraper::Selector::parse("a").unwrap())
        .next()
        .unwrap();
    assert_eq!(
        link.value().attr("href"),
        Some("https://example.org/a(b)?q=x&n=1")
    );
    assert_eq!(link.text().collect::<String>(), "click here");
    let images = parsed
        .select(&scraper::Selector::parse("img").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].value().attr("alt"), Some("a [picture]"));
    assert_eq!(images[0].value().attr("title"), Some("quoted \"title\""));
    assert_eq!(images[1].value().attr("width"), Some("200"));
    assert_eq!(images[1].value().attr("height"), Some("100"));
}

#[test]
fn markdown_gfm_tasks_and_commonmark_html_fallback_preserve_checked_state() {
    let json = tiptap_task_json();
    let gfm = markdown_from_json(json.clone(), &schema(), MarkdownFlavor::Gfm);
    assert!(gfm.starts_with("- [x] **Done**\n"), "{gfm}");
    assert!(gfm.contains("- [ ] Next"), "{gfm}");
    let html = markdown_render(&gfm, MarkdownFlavor::Gfm);
    assert!(html.contains("type=\"checkbox\" checked=\"\""), "{html}");
    assert_eq!(
        markdown_from_json(json, &schema(), MarkdownFlavor::Commonmark),
        TIPTAP_TASK_HTML
    );
}

#[test]
fn markdown_gfm_tables_require_simple_cells_and_a_real_header() {
    let schema = crate::schema::presets::tiptap_table_schema();
    let cell = |kind: &str, text: &str| serde_json::json!({"type":kind, "content":[{"type":"paragraph", "content":[{"type":"text", "text":text}]}]});
    let json = serde_json::json!({"type":"doc", "content":[{"type":"table", "content":[
        {"type":"tableRow", "content":[cell("tableHeader", "Name"), cell("tableHeader", "Value")]},
        {"type":"tableRow", "content":[cell("tableCell", "a|b"), cell("tableCell", "two")]}
    ]}]});
    let gfm = markdown_roundtrip(json.clone(), &schema, MarkdownFlavor::Gfm);
    assert!(gfm.contains("| Name | Value |\n| --- | --- |"), "{gfm}");
    assert!(gfm.contains("a\\|b"), "{gfm}");
    for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
        for variant in ["merged", "noHeader", "blocks"] {
            let mut complex = json.clone();
            match variant {
                "merged" => {
                    complex["content"][0]["content"][0]["content"][0]["attrs"] =
                        serde_json::json!({"colspan":2})
                }
                "noHeader" => {
                    for cell in complex["content"][0]["content"][0]["content"]
                        .as_array_mut()
                        .unwrap()
                    {
                        cell["type"] = "tableCell".into();
                    }
                }
                "blocks" => complex["content"][0]["content"][1]["content"][0]["content"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({"type":"paragraph"})),
                _ => unreachable!(),
            }
            let document =
                from_prosemirror_json(&complex, &schema, UnknownTypeMode::Error).unwrap();
            assert_eq!(
                to_markdown(&document, &schema, flavor),
                to_html(&document, &schema),
                "{variant}"
            );
        }
    }
}

#[test]
fn markdown_unknown_atoms_and_empty_blocks_use_existing_html_fallback() {
    let schema = atom_rules_schema();
    let json = serde_json::json!({"type":"doc", "content":[{"type":"counterCard", "attrs":{"title":"<Card>", "count":5}}, {"type":"paragraph"}]});
    let document = from_prosemirror_json(&json, &schema, UnknownTypeMode::Error).unwrap();
    let markdown = to_markdown(&document, &schema, MarkdownFlavor::Commonmark);
    assert_eq!(
        markdown,
        format!(
            "{}\n\n<p></p>",
            to_html(
                &Document::new(Node::element(
                    "doc".into(),
                    HashMap::new(),
                    Fragment::from(vec![document.root().child(0).unwrap().clone()])
                )),
                &schema
            )
        )
    );
}

#[test]
fn markdown_keeps_adjacent_lists_and_heading_hard_breaks_separate() {
    let list = |text: &str| serde_json::json!({"type":"bulletList", "content":[{"type":"listItem", "content":[{"type":"paragraph", "content":[{"type":"text", "text":text}]}]}]});
    markdown_roundtrip(
        serde_json::json!({"type":"doc", "content":[list("first"), list("second")]}),
        &schema(),
        MarkdownFlavor::Commonmark,
    );
    markdown_roundtrip(
        serde_json::json!({"type":"doc", "content":[{"type":"heading", "attrs":{"level":3}, "content":[{"type":"text", "text":"before"},{"type":"hardBreak"},{"type":"text", "text":"after"}]}]}),
        &schema(),
        MarkdownFlavor::Commonmark,
    );
}

#[test]
fn markdown_preserves_urls_that_a_markdown_renderer_would_percent_encode() {
    for href in [
        "https://example.org/a (b)?q=\"x\"&n=1",
        "a<b>",
        "line\nbreak",
    ] {
        markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"link", "marks":[{"type":"link", "attrs":{"href":href}}]}]}]}),
            &schema(),
            MarkdownFlavor::Commonmark,
        );
    }
}

#[test]
fn markdown_gfm_table_code_pipes_and_hard_breaks_remain_in_their_cells() {
    let schema = crate::schema::presets::tiptap_table_schema();
    let json = serde_json::json!({"type":"doc", "content":[{"type":"table", "content":[
        {"type":"tableRow", "content":[{"type":"tableHeader", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"Header"}]}]}]},
        {"type":"tableRow", "content":[{"type":"tableCell", "content":[{"type":"paragraph", "content":[
            {"type":"text", "text":"a\\|b|c", "marks":[{"type":"code"}]}, {"type":"hardBreak"}, {"type":"text", "text":"next"}
        ]}]}]}
    ]}]});
    markdown_roundtrip(json, &schema, MarkdownFlavor::Gfm);
}

#[test]
fn markdown_emphasis_transitions_do_not_extend_or_drop_formatting() {
    for left in [vec![], vec!["bold"], vec!["italic"], vec!["bold", "italic"]] {
        for right in [vec![], vec!["bold"], vec!["italic"], vec!["bold", "italic"]] {
            for (a, b) in [
                ("a", "b"),
                ("a*", "b"),
                ("中文", "字"),
                ("a ", " b"),
                ("!", "?"),
            ] {
                let marks = |names: &[&str]| {
                    names
                        .iter()
                        .map(|name| serde_json::json!({"type":name}))
                        .collect::<Vec<_>>()
                };
                let json = serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
                    {"type":"text", "text":a, "marks":marks(&left)}, {"type":"text", "text":b, "marks":marks(&right)}
                ]}]});
                let document =
                    from_prosemirror_json(&json, &schema(), UnknownTypeMode::Error).unwrap();
                let expected = from_html(
                    &to_html(&document, &schema()),
                    &schema(),
                    &FromHtmlOptions::default(),
                )
                .unwrap();
                markdown_roundtrip(
                    to_prosemirror_json(&expected, &schema()),
                    &schema(),
                    MarkdownFlavor::Commonmark,
                );
            }
        }
    }
}

#[test]
fn markdown_crossing_mark_boundaries_unicode_punctuation_and_terminal_breaks_roundtrip() {
    markdown_roundtrip(
        serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
            {"type":"text", "text":"a", "marks":[{"type":"bold"}]},
            {"type":"text", "text":"b", "marks":[{"type":"bold"},{"type":"italic"}]},
            {"type":"text", "text":"c", "marks":[{"type":"italic"}]}
        ]}]}),
        &schema(),
        MarkdownFlavor::Commonmark,
    );
    markdown_roundtrip(
        serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
            {"type":"text", "text":"a"}, {"type":"text", "text":"。", "marks":[{"type":"bold"}]}, {"type":"text", "text":"b"}, {"type":"hardBreak"}
        ]}]}),
        &schema(),
        MarkdownFlavor::Commonmark,
    );
}

#[test]
fn markdown_custom_schema_attributes_are_preserved_by_html_fallback() {
    let mut nodes = schema().all_nodes().cloned().collect::<Vec<_>>();
    let mut marks = schema().all_marks().cloned().collect::<Vec<_>>();
    for name in ["h2", "codeBlock", "bulletList", "listItem"] {
        nodes
            .iter_mut()
            .find(|spec| spec.name == name)
            .unwrap()
            .attrs
            .insert(
                "id".into(),
                AttrSpec {
                    default: Some(serde_json::Value::Null),
                    has_default: true,
                    ..Default::default()
                },
            );
    }
    marks
        .iter_mut()
        .find(|spec| spec.name == "code")
        .unwrap()
        .attrs
        .insert(
            "kind".into(),
            AttrSpec {
                default: Some(serde_json::Value::Null),
                has_default: true,
                ..Default::default()
            },
        );
    let custom = Schema::new(nodes, marks);
    for json in [
        serde_json::json!({"type":"doc", "content":[{"type":"h2", "attrs":{"id":"anchor"}, "content":[{"type":"text", "text":"Title"}]}]}),
        serde_json::json!({"type":"doc", "content":[{"type":"codeBlock", "attrs":{"id":"sample"}, "content":[{"type":"text", "text":"code\r\n"}]}]}),
        serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"sample", "marks":[{"type":"code", "attrs":{"kind":"literal"}}]}]}]}),
        serde_json::json!({"type":"doc", "content":[{"type":"bulletList", "content":[{"type":"listItem", "attrs":{"id":"item"}, "content":[{"type":"paragraph", "content":[{"type":"text", "text":"item"}]}]}]}]}),
    ] {
        let document = from_prosemirror_json(&json, &custom, UnknownTypeMode::Error).unwrap();
        let markdown = to_markdown(&document, &custom, MarkdownFlavor::Gfm);
        assert!(
            markdown.contains("id=\"") || markdown.contains("kind=\""),
            "custom attributes were dropped: {markdown}"
        );
    }
}

#[test]
fn markdown_code_line_endings_that_fences_normalise_use_html() {
    for text in ["a\r\nb\r", "no final newline"] {
        let document = from_prosemirror_json(&serde_json::json!({"type":"doc", "content":[{"type":"codeBlock", "content":[{"type":"text", "text":text}]}]}), &schema(), UnknownTypeMode::Error).unwrap();
        assert_eq!(
            to_markdown(&document, &schema(), MarkdownFlavor::Commonmark),
            to_html(&document, &schema())
        );
    }
}

#[test]
fn markdown_inline_html_fallback_keeps_literal_markdown_characters() {
    for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
        markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
                {"type":"text", "text":"a*X*", "marks":[{"type":"bold"}]},
                {"type":"text", "text":"b", "marks":[{"type":"bold"},{"type":"italic"}]},
                {"type":"text", "text":"c", "marks":[{"type":"italic"}]},
                {"type":"text", "text":" *literal* [link](url)\n# title"}
            ]}]}),
            &schema(),
            flavor,
        );
        markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
                {"type":"text", "text":"*x*\ny", "marks":[{"type":"code"}]}
            ]}]}),
            &schema(),
            flavor,
        );
    }
}

#[test]
fn markdown_inline_atom_labels_are_literal_and_opaque_html_stays_in_a_block() {
    let schema = mention_schema();
    let document = from_prosemirror_json(
        &serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[
            {"type":"mention", "attrs":{"id":"person", "label":"*name* [x](url)"}}
        ]}]}),
        &schema,
        UnknownTypeMode::Error,
    )
    .unwrap();
    for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
        let markdown = to_markdown(&document, &schema, flavor);
        let html = markdown_render(&markdown, flavor);
        let fragment = scraper::Html::parse_fragment(&html);
        let span = fragment
            .select(&scraper::Selector::parse("span").unwrap())
            .next()
            .unwrap();
        assert_eq!(
            span.text().collect::<String>(),
            "*name* [x](url)",
            "{markdown}\n{html}"
        );
        assert_eq!(
            span.select(&scraper::Selector::parse("em, a").unwrap())
                .count(),
            0
        );
    }
    let document = from_html(
        "<p>before <widget>*literal*</widget> after</p>",
        &schema,
        &FromHtmlOptions::default(),
    )
    .unwrap();
    let markdown = to_markdown(&document, &schema, MarkdownFlavor::Gfm);
    assert!(markdown.starts_with("<p>"), "{markdown}");
    assert!(
        !markdown_render(&markdown, MarkdownFlavor::Gfm).contains("<em>"),
        "{markdown}"
    );
}

#[test]
fn markdown_html_table_fallback_preserves_blank_lines_and_literal_syntax() {
    let schema = crate::schema::presets::tiptap_table_schema();
    for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
        markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"table", "content":[
                {"type":"tableRow", "content":[{"type":"tableCell", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"before\n\n*literal*\n# title"}]}]}]}
            ]}]}),
            &schema,
            flavor,
        );
    }
}

#[test]
fn markdown_html_fallback_handles_blank_lines_in_attributes_and_opaque_fragments() {
    let schema = mention_schema();
    for html in [
        "<table><tbody><tr><td>cell\n\n*literal*</td></tr></tbody></table>",
        "<p>before <select><option>one\n\n*literal*</option></select> after</p>",
        "<p>before <svg><text>one\n\n*literal*</text></svg> after</p>",
        "<p>before <widget>one\n\n*literal*</widget> after</p>",
        "<p>before <widget><span\n data-name=\"one\n\ntwo\">*literal*\n\nnext</span></widget> after</p>",
    ] {
        let document = from_html(html, &schema, &FromHtmlOptions::default()).unwrap();
        for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
            let markdown = to_markdown(&document, &schema, flavor);
            let rendered = markdown_render(&markdown, flavor);
            let parsed = from_html(rendered.trim_end_matches('\n'), &schema, &FromHtmlOptions::default()).unwrap();
            assert_eq!(to_prosemirror_json(&parsed, &schema), to_prosemirror_json(&document, &schema), "{markdown}\n{rendered}");
        }
    }
    let mut nodes = schema.all_nodes().cloned().collect::<Vec<_>>();
    nodes
        .iter_mut()
        .find(|spec| spec.name == "h2")
        .unwrap()
        .attrs
        .insert(
            "id".into(),
            AttrSpec {
                default: Some(serde_json::Value::Null),
                has_default: true,
                ..Default::default()
            },
        );
    let schema = Schema::new(nodes, schema.all_marks().cloned().collect());
    for flavor in [MarkdownFlavor::Commonmark, MarkdownFlavor::Gfm] {
        markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"h2", "attrs":{"id":"one\n\ntwo"}, "content":[{"type":"text", "text":"*literal*"}]}]}),
            &schema,
            flavor,
        );
        markdown_roundtrip(
            serde_json::json!({"type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"link", "marks":[{"type":"link", "attrs":{"href":"one\n\ntwo"}}]}]}]}),
            &schema,
            flavor,
        );
        let source = "one\n\ntwo";
        let markdown = markdown_from_json(
            serde_json::json!({"type":"doc", "content":[{"type":"image", "attrs":{"src":source}}]}),
            &schema,
            flavor,
        );
        let rendered = markdown_render(&markdown, flavor);
        let html = scraper::Html::parse_fragment(&rendered);
        let image = html
            .select(&scraper::Selector::parse("img").unwrap())
            .next()
            .unwrap();
        assert_eq!(
            image.value().attr("src"),
            Some(source),
            "{markdown}\n{rendered}"
        );
    }
}
