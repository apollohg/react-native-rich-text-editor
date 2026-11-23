#[test]
fn text_output_separates_every_text_block_after_the_first_at_any_depth() {
    let schema = crate::schema::presets::tiptap_table_schema();
    let json = serde_json::json!({"type":"doc", "content":[
        {"type":"heading", "attrs":{"level":2}, "content":[{"type":"text", "text":"Title"}]},
        {"type":"paragraph", "content":[
            {"type":"text", "text":"line"}, {"type":"hardBreak"}, {"type":"text", "text":"wrap"}
        ]},
        {"type":"paragraph"},
        {"type":"horizontalRule"},
        {"type":"bulletList", "content":[
            {"type":"listItem", "content":[
                {"type":"paragraph", "content":[{"type":"text", "text":"outer"}]},
                {"type":"bulletList", "content":[
                    {"type":"listItem", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"inner"}]}]}
                ]}
            ]}
        ]},
        {"type":"blockquote", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"quoted"}]}]},
        {"type":"codeBlock", "content":[{"type":"text", "text":"let a;\nlet b;"}]},
        {"type":"table", "content":[
            {"type":"tableRow", "content":[
                {"type":"tableCell", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"A1"}]}]},
                {"type":"tableCell", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"B1"}]}]}
            ]}
        ]}
    ]});
    let document = from_prosemirror_json(&json, &schema, UnknownTypeMode::Error).unwrap();
    for (block_separator, expected) in [
        ("", "Titlelinewrapouterinnerquotedlet a;\nlet b;A1B1"),
        ("\n", "Title\nlinewrap\n\nouter\ninner\nquoted\nlet a;\nlet b;\nA1\nB1"),
        (" | ", "Title | linewrap |  | outer | inner | quoted | let a;\nlet b; | A1 | B1"),
    ] {
        let text = crate::serialize::to_text(&document, &schema, block_separator);
        assert_eq!(text, expected, "text for separator {block_separator:?}");
        assert_eq!(
            crate::serialize::text_byte_len(&document, &schema, block_separator),
            text.len(),
            "projected length for separator {block_separator:?}"
        );
    }
    assert_eq!(
        crate::serialize::to_text(&document, &schema, ""),
        document.root().text_content(),
        "an empty separator matches plain concatenation"
    );
}
