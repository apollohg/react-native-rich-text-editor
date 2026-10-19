use super::*;
use crate::schema::presets::tiptap_table_schema;
use crate::test_support::mention_schema::mention_table_schema;
use serde_json::{json, Value};

const PLANNING_OWNER: u64 = 52_000;
const FIRST_PLANNING_REQUEST: u64 = 52_100;
const NOT_APPLICABLE: &str = "notApplicable";
const TRANSACTION: &str = "transaction";

fn planning_session(document: Value) -> EditorSession {
    table_session_with_schema(&document.to_string(), tiptap_table_schema())
}

fn submit(
    session: &mut EditorSession,
    request_id: u64,
    (anchor, head): (u32, u32),
    mut intent: Value,
) -> Value {
    intent["anchor"] = json!(anchor);
    intent["head"] = json!(head);
    let request = native_intent_request(session, PLANNING_OWNER, request_id, intent.clone());
    let outcome = NativeTransactionBridge::new(session)
        .submit_native_intent(&request)
        .unwrap_or_else(|error| {
            panic!(
                "{intent} at ({anchor}, {head}) on {} was refused: {} {}",
                session.engine.document_json().unwrap(),
                error.code,
                error.message
            )
        });
    serde_json::from_str(&outcome).unwrap()
}

fn paragraph(content: Value) -> Value {
    json!({ "type": "paragraph", "content": content })
}

fn empty_paragraph() -> Value {
    json!({ "type": "paragraph" })
}

fn text(value: &str) -> Value {
    json!({ "type": "text", "text": value })
}

fn marked(value: &str, marks: Value) -> Value {
    json!({ "type": "text", "text": value, "marks": marks })
}

fn doc(content: Value) -> Value {
    json!({ "type": "doc", "content": content })
}

fn mark_command(command: Value) -> Value {
    json!({ "type": "command", "command": command })
}

fn assert_no_op(session: &mut EditorSession, selection: (u32, u32), intent: Value) {
    let before = session.engine.document_json().unwrap();
    let outcome = submit(session, FIRST_PLANNING_REQUEST, selection, intent.clone());
    assert_eq!(
        outcome["type"], NOT_APPLICABLE,
        "{intent} at {selection:?} on {before} is a successful no-op: {outcome}"
    );
    assert_eq!(session.engine.document_json().unwrap(), before);
}

#[test]
fn typing_lines_over_a_paragraph_and_a_list_item_keeps_the_later_items() {
    let document = doc(json!([
        paragraph(json!([text("a")])),
        { "type": "bulletList", "content": [
            { "type": "listItem", "content": [paragraph(json!([text("b")]))] },
            { "type": "listItem", "content": [paragraph(json!([text("c")]))] }
        ] }
    ]));
    for intent in [
        json!({ "type": "replaceSelectionText", "text": "x\ny" }),
        mark_command(json!({ "type": "paste", "text": "x\ny" })),
    ] {
        let mut session = planning_session(document.clone());
        submit(&mut session, FIRST_PLANNING_REQUEST, (1, 2), intent.clone());
        assert_eq!(
            session.engine.document_json().unwrap(),
            doc(json!([
                paragraph(json!([text("ax")])),
                paragraph(json!([text("yb")])),
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [paragraph(json!([text("c")]))] }
                ] }
            ])),
            "{intent} replaces only the selected break and keeps item c"
        );
    }
}

#[test]
fn marks_over_selections_without_text_are_no_ops() {
    let mut session = planning_session(doc(json!([empty_paragraph(), empty_paragraph()])));
    for command in [
        json!({ "type": "setMark", "markType": "link", "attrs": { "href": "https://example.com" } }),
        json!({ "type": "toggleMark", "markType": "bold" }),
        json!({ "type": "unsetMark", "markType": "link" }),
    ] {
        assert_no_op(&mut session, (0, 2), mark_command(command));
    }
}

#[test]
fn marks_across_paragraphs_apply_to_every_textblock() {
    let mut session = planning_session(doc(json!([
        paragraph(json!([text("ab")])),
        paragraph(json!([text("cd")]))
    ])));
    let link = json!([{ "type": "link", "attrs": { "href": "https://example.com" } }]);
    let steps = [
        (
            json!({ "type": "toggleMark", "markType": "bold" }),
            json!([{ "type": "bold" }]),
        ),
        (
            json!({ "type": "toggleMark", "markType": "bold" }),
            json!(null),
        ),
        (
            json!({ "type": "setMark", "markType": "link", "attrs": { "href": "https://example.com" } }),
            link.clone(),
        ),
        (
            json!({ "type": "unsetMark", "markType": "link" }),
            json!(null),
        ),
    ];
    for (index, (command, marks)) in steps.into_iter().enumerate() {
        submit(
            &mut session,
            FIRST_PLANNING_REQUEST + index as u64,
            (0, 5),
            mark_command(command.clone()),
        );
        let line = |value: &str| {
            if marks.is_null() {
                text(value)
            } else {
                marked(value, marks.clone())
            }
        };
        assert_eq!(
            session.engine.document_json().unwrap(),
            doc(json!([
                paragraph(json!([line("ab")])),
                paragraph(json!([line("cd")]))
            ])),
            "step {index}: {command}"
        );
    }
}

#[test]
fn wrapping_a_blockquote_paragraph_in_a_list_keeps_the_caret_in_that_paragraph() {
    for (content, caret, expected_document_caret) in [
        (json!([empty_paragraph()]), 0, 4),
        (json!([paragraph(json!([text("ab")]))]), 1, 5),
    ] {
        let mut session =
            planning_session(doc(json!([{ "type": "blockquote", "content": content }])));
        submit(
            &mut session,
            FIRST_PLANNING_REQUEST,
            (caret, caret),
            mark_command(
                json!({ "type": "wrapInList", "listType": "bulletList", "itemType": "listItem" }),
            ),
        );
        assert_eq!(
            session.engine.document_json().unwrap(),
            doc(json!([{ "type": "blockquote", "content": [
                { "type": "bulletList", "content": [{ "type": "listItem", "content": content }] }
            ] }]))
        );
        let Some(ResolvedSelection::Text { anchor, head }) = session.engine.resolved_selection()
        else {
            panic!("the caret stays a text selection");
        };
        assert_eq!(
            (anchor.document, head.document),
            (expected_document_caret, expected_document_caret)
        );
    }
}

#[test]
fn text_edits_at_a_caret_outside_any_textblock_are_no_ops() {
    let mut session = planning_session(doc(json!([
        { "type": "blockquote", "content": [{ "type": "horizontalRule" }] }
    ])));
    for text in [" ", "\nx", "a\nb"] {
        assert_no_op(
            &mut session,
            (0, 0),
            json!({ "type": "replaceSelectionText", "text": text }),
        );
    }
    let mut session = planning_session(doc(json!([
        empty_paragraph(),
        { "type": "image", "attrs": { "src": "split.png" } }
    ])));
    assert_no_op(&mut session, (1, 2), json!({ "type": "deleteBackward" }));
}

#[test]
fn edits_across_table_cells_change_each_cell_without_joining_them() {
    let cell = |value: Value| json!({ "type": "tableCell", "content": [paragraph(value)] });
    let mut session = planning_session(doc(json!([{ "type": "table", "content": [
        { "type": "tableRow", "content": [cell(json!([text("ab")])), cell(json!([text("cd")]))] }
    ] }])));
    submit(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (1, 4),
        json!({ "type": "replaceSelectionText", "text": "z" }),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([{ "type": "table", "content": [
            { "type": "tableRow", "content": [cell(json!([text("az")])), cell(json!([text("d")]))] }
        ] }]))
    );
}

#[test]
fn replacing_a_selected_hard_break_with_a_hard_break_changes_nothing() {
    let mut session = planning_session(doc(json!([paragraph(json!([{ "type": "hardBreak" }]))])));
    let before = session.engine.document_json().unwrap();
    let outcome = submit(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (0, 1),
        json!({ "type": "insertContentJson", "json": doc(json!([{ "type": "hardBreak" }])) }),
    );
    assert_eq!(outcome["type"], TRANSACTION, "{outcome}");
    assert_eq!(outcome["documentChanged"], false, "{outcome}");
    assert_eq!(outcome["canUndo"], false, "{outcome}");
    assert_eq!(session.engine.document_json().unwrap(), before);
}

#[test]
fn tables_inserted_inside_a_cell_arrive_as_cell_blocks() {
    let cell = |content: Value| json!({ "type": "tableCell", "content": content });
    let mut session = planning_session(doc(json!([{ "type": "table", "content": [
        { "type": "tableRow", "content": [cell(json!([paragraph(json!([text("ab")]))]))] }
    ] }])));
    submit(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (2, 2),
        json!({ "type": "insertContentHtml", "html": "<table><tr><td>c</td><td>d</td></tr></table>" }),
    );
    let document = session.engine.document_json().unwrap();
    assert_eq!(
        document,
        doc(json!([{ "type": "table", "content": [
            { "type": "tableRow", "content": [cell(json!([
                paragraph(json!([text("ab")])),
                paragraph(json!([text("c")])),
                paragraph(json!([text("d")]))
            ]))] }
        ] }])),
        "no nested table is created"
    );
}

#[test]
fn backspace_into_a_list_item_ending_in_a_rule_joins_the_items() {
    let mut session = planning_session(doc(json!([{ "type": "bulletList", "content": [
        { "type": "listItem", "content": [paragraph(json!([text("a")])), { "type": "horizontalRule" }] },
        { "type": "listItem", "content": [paragraph(json!([text("b")]))] }
    ] }])));
    let caret = session.engine.position_map().unwrap().total_scalars() - 1;
    submit(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (caret, caret),
        json!({ "type": "deleteBackward" }),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([{ "type": "bulletList", "content": [
            { "type": "listItem", "content": [
                paragraph(json!([text("a")])),
                { "type": "horizontalRule" },
                paragraph(json!([text("b")]))
            ] }
        ] }]))
    );
}

#[test]
fn caret_link_updates_stop_at_hard_breaks_and_mentions() {
    let link = |href: &str| json!([{ "type": "link", "attrs": { "href": href } }]);
    let old = link("https://old.example");
    let mention = json!({ "type": "mention", "attrs": { "id": "user-1", "label": "Ann" } });
    let schema = mention_table_schema();
    let document = doc(json!([paragraph(json!([
        marked("ab", old.clone()),
        { "type": "hardBreak" },
        marked("cd", old.clone()),
        mention,
        marked("ef", old.clone())
    ]))]));
    let mut session = table_session_with_schema(&document.to_string(), schema);
    submit(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (1, 1),
        mark_command(
            json!({ "type": "setMark", "markType": "link", "attrs": { "href": "https://new.example" } }),
        ),
    );
    submit(
        &mut session,
        FIRST_PLANNING_REQUEST + 1,
        (4, 4),
        mark_command(json!({ "type": "unsetMark", "markType": "link" })),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([paragraph(json!([
            marked("ab", link("https://new.example")),
            { "type": "hardBreak" },
            text("cd"),
            mention,
            marked("ef", old)
        ]))]))
    );
}

fn blockquote(content: Value) -> Value {
    json!({ "type": "blockquote", "content": content })
}

fn list_item(content: Value) -> Value {
    json!({ "type": "listItem", "content": content })
}

fn bullet_list(items: Value) -> Value {
    json!({ "type": "bulletList", "content": items })
}

fn applied(session: &mut EditorSession, request_id: u64, selection: (u32, u32), intent: Value) {
    let outcome = submit(session, request_id, selection, intent.clone());
    assert_eq!(outcome["type"], TRANSACTION, "{intent} applies: {outcome}");
}

#[test]
fn inline_content_inserted_beside_a_block_is_wrapped_in_a_paragraph() {
    let mention = json!({ "type": "mention", "attrs": { "id": "user-1", "label": "Ann" } });
    let mut session = table_session_with_schema(
        &doc(json!([blockquote(json!([{ "type": "horizontalRule" }]))])).to_string(),
        mention_table_schema(),
    );
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (0, 0),
        json!({ "type": "insertContentJson", "json": doc(json!([mention.clone(), text(" ")])) }),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([blockquote(json!([
            paragraph(json!([mention, text(" ")])),
            { "type": "horizontalRule" }
        ]))]))
    );
}

#[test]
fn inline_content_that_a_code_block_rejects_lands_in_a_paragraph_after_it() {
    let mention = json!({ "type": "mention", "attrs": { "id": "user-1", "label": "Ann" } });
    let mut session = table_session_with_schema(
        &doc(json!([{ "type": "codeBlock", "content": [text("code")] }])).to_string(),
        mention_table_schema(),
    );
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (2, 2),
        json!({ "type": "insertContentJson", "json": doc(json!([mention.clone()])) }),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([
            { "type": "codeBlock", "content": [text("code")] },
            paragraph(json!([mention]))
        ]))
    );
}

#[test]
fn blocks_inserted_into_an_empty_first_list_paragraph_follow_it() {
    let mut session = planning_session(doc(json!([bullet_list(json!([list_item(json!([
        empty_paragraph()
    ]))]))])));
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (0, 0),
        json!({ "type": "insertContentHtml", "html": "<blockquote><p>q</p></blockquote>" }),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([bullet_list(json!([list_item(json!([
            empty_paragraph(),
            blockquote(json!([paragraph(json!([text("q")]))])),
            empty_paragraph()
        ]))]))]))
    );
}

#[test]
fn applying_the_current_list_type_unwraps_the_item() {
    let mut session = planning_session(doc(json!([bullet_list(json!([list_item(json!([
        paragraph(json!([text("a")]))
    ]))]))])));
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (0, 0),
        mark_command(json!({ "type": "applyListType", "listType": "bulletList" })),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([paragraph(json!([text("a")]))]))
    );
}

#[test]
fn wrapping_a_later_paragraph_of_a_list_item_nests_a_list_in_place() {
    let mut session = planning_session(doc(json!([bullet_list(json!([list_item(json!([
        paragraph(json!([text("a")])),
        paragraph(json!([text("b")]))
    ]))]))])));
    let total = session.engine.position_map().unwrap().total_scalars();
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (total, total),
        mark_command(
            json!({ "type": "wrapInList", "listType": "orderedList", "itemType": "listItem" }),
        ),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([bullet_list(json!([list_item(json!([
            paragraph(json!([text("a")])),
            { "type": "orderedList", "content": [
                list_item(json!([paragraph(json!([text("b")]))]))
            ] }
        ]))]))]))
    );
}

#[test]
fn code_blocks_keep_hard_breaks_as_newlines_and_refuse_mentions() {
    let mention = json!({ "type": "mention", "attrs": { "id": "user-1", "label": "Ann" } });
    let mut session = table_session_with_schema(
        &doc(json!([
            paragraph(json!([text("e"), { "type": "hardBreak" }, text("f")])),
            paragraph(json!([text("g"), mention]))
        ]))
        .to_string(),
        mention_table_schema(),
    );
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (0, 0),
        mark_command(json!({ "type": "toggleCodeBlock" })),
    );
    assert_eq!(
        session.engine.document_json().unwrap()["content"][0],
        json!({ "type": "codeBlock", "content": [text("e\nf")] })
    );
    let mention_line = "e\nf\n".chars().count() as u32;
    let mut session = table_session_with_schema(
        &session.engine.document_json().unwrap().to_string(),
        mention_table_schema(),
    );
    assert_no_op(
        &mut session,
        (mention_line, mention_line),
        mark_command(json!({ "type": "toggleCodeBlock" })),
    );
}

#[test]
fn moving_text_onto_a_block_boundary_wraps_it_in_a_paragraph() {
    let mut session = planning_session(doc(json!([
        blockquote(json!([{ "type": "horizontalRule" }])),
        paragraph(json!([text("abc")]))
    ])));
    let total = session.engine.position_map().unwrap().total_scalars();
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (total - 2, total),
        mark_command(json!({ "type": "moveSelection",
            "range": { "from": { "offset": total - 2, "kind": "scalar" }, "to": { "offset": total, "kind": "scalar" } },
            "at": { "offset": 0, "kind": "scalar" } })),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([
            blockquote(json!([paragraph(json!([text("bc")])), { "type": "horizontalRule" }])),
            paragraph(json!([text("a")]))
        ]))
    );
}

#[test]
fn a_hard_break_typed_over_a_selection_reaching_a_quoted_image_keeps_the_image() {
    let mut session = planning_session(doc(json!([
        { "type": "heading", "attrs": { "level": 2 }, "content": [text("ab")] },
        blockquote(json!([{ "type": "image", "attrs": { "src": "split.png" } }]))
    ])));
    let total = session.engine.position_map().unwrap().total_scalars();
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (1, total),
        mark_command(json!({ "type": "insertNode", "nodeType": "hardBreak" })),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([
            { "type": "heading", "attrs": { "level": 2 }, "content": [text("a"), { "type": "hardBreak" }] },
            blockquote(json!([{ "type": "image", "attrs": { "src": "split.png" } }]))
        ]))
    );
}

#[test]
fn range_marks_over_hard_breaks_and_mentions_format_every_text_run() {
    let mention = json!({ "type": "mention", "attrs": { "id": "user-1", "label": "Ann" } });
    let schema = mention_table_schema();
    let line = |marks: Value| {
        let run = |value: &str| {
            if marks.is_null() {
                text(value)
            } else {
                marked(value, marks.clone())
            }
        };
        doc(json!([paragraph(json!([
            run("ab"),
            { "type": "hardBreak" },
            run("cd"),
            mention,
            run("ef")
        ]))]))
    };
    let mut session = table_session_with_schema(&line(json!(null)).to_string(), schema);
    let total = session.engine.position_map().unwrap().total_scalars();
    let link = json!([{ "type": "link", "attrs": { "href": "https://example.com" } }]);
    let steps = [
        (
            json!({ "type": "toggleMark", "markType": "bold" }),
            json!([{ "type": "bold" }]),
        ),
        (
            json!({ "type": "toggleMark", "markType": "bold" }),
            json!(null),
        ),
        (
            json!({ "type": "setMark", "markType": "link", "attrs": { "href": "https://example.com" } }),
            link,
        ),
        (
            json!({ "type": "unsetMark", "markType": "link" }),
            json!(null),
        ),
    ];
    for (index, (command, marks)) in steps.into_iter().enumerate() {
        submit(
            &mut session,
            FIRST_PLANNING_REQUEST + index as u64,
            (0, total),
            mark_command(command.clone()),
        );
        assert_eq!(
            session.engine.document_json().unwrap(),
            line(marks),
            "step {index}: {command}"
        );
    }
}

#[test]
fn dragging_text_after_itself_into_a_quote_of_blocks_wraps_it_in_a_paragraph() {
    let document: Value = serde_json::from_str(r#"{"content": [{"content": [{"content": [{"content": [{"attrs": {"id": "user-1", "label": "Ann"}, "type": "mention"}, {"type": "hardBreak"}], "type": "paragraph"}, {"attrs": {"src": "split.png"}, "type": "image"}], "type": "blockquote"}], "type": "blockquote"}, {"content": [{"content": [{"content": [{"content": [{"content": [{"marks": [{"type": "bold"}], "text": "d\u00e9", "type": "text"}, {"marks": [{"type": "bold"}], "text": "\ud83e\udd80x", "type": "text"}], "type": "h1"}, {"attrs": {"src": "split.png"}, "type": "image"}], "type": "blockquote"}], "type": "tableCell"}, {"content": [{"content": [{"content": [{"content": [{"type": "hardBreak"}], "type": "paragraph"}, {"content": [{"type": "hardBreak"}], "type": "h1"}], "type": "listItem"}, {"content": [{"content": [], "type": "paragraph"}], "type": "listItem"}], "type": "bulletList"}, {"content": [{"content": [{"marks": [{"type": "bold"}], "text": "a", "type": "text"}], "type": "h2"}], "type": "blockquote"}], "type": "tableCell"}], "type": "tableRow"}], "type": "table"}, {"content": [{"type": "horizontalRule"}, {"content": [], "type": "h1"}], "type": "blockquote"}], "type": "doc"}"#).unwrap();
    let mut session = table_session_with_schema(&document.to_string(), mention_table_schema());
    let steps = [
        ((0, 11), serde_json::from_str::<Value>(r#"{"html": "x<br>", "type": "insertContentHtml"}"#).unwrap()),
        ((6, 6), serde_json::from_str::<Value>(r#"{"command": {"html": "x<br>", "type": "paste"}, "type": "command"}"#).unwrap()),
        ((20, 21), serde_json::from_str::<Value>(r#"{"command": {"at": {"kind": "scalar", "offset": 25}, "range": {"from": {"kind": "scalar", "offset": 20}, "to": {"kind": "scalar", "offset": 21}}, "type": "moveSelection"}, "type": "command"}"#).unwrap()),
    ];
    for (index, (selection, intent)) in steps.into_iter().enumerate() {
        applied(
            &mut session,
            FIRST_PLANNING_REQUEST + index as u64,
            selection,
            intent,
        );
    }
}

#[test]
fn a_split_over_list_items_keeps_the_first_items_leading_paragraph() {
    let document: Value = serde_json::from_str(r#"{"content": [{"content": [{"content": [{"content": [{"content": [], "type": "paragraph"}], "type": "listItem"}, {"content": [{"content": [], "type": "paragraph"}], "type": "listItem"}], "type": "orderedList"}], "type": "blockquote"}, {"content": [{"content": [{"content": [{"text": "bc", "type": "text"}, {"attrs": {"id": "user-1", "label": "Ann"}, "type": "mention"}, {"text": "bc", "type": "text"}], "type": "paragraph"}, {"content": [{"type": "hardBreak"}, {"type": "hardBreak"}], "type": "paragraph"}], "type": "listItem"}, {"content": [{"content": [{"marks": [{"type": "bold"}], "text": "\ud83e\udd80x", "type": "text"}], "type": "paragraph"}], "type": "listItem"}], "type": "orderedList"}], "type": "doc"}"#).unwrap();
    let mut session = table_session_with_schema(&document.to_string(), mention_table_schema());
    let steps = [
        ((14, 21), serde_json::from_str::<Value>(r#"{"json": {"content": [{"attrs": {"src": "x.png"}, "type": "image"}], "type": "doc"}, "type": "insertContentJson"}"#).unwrap()),
        ((11, 11), serde_json::from_str::<Value>(r#"{"command": {"plainText": true, "text": "\nx", "type": "paste"}, "type": "command"}"#).unwrap()),
        ((8, 21), serde_json::from_str::<Value>(r#"{"type": "deleteAndSplit"}"#).unwrap()),
    ];
    for (index, (selection, intent)) in steps.into_iter().enumerate() {
        applied(
            &mut session,
            FIRST_PLANNING_REQUEST + index as u64,
            selection,
            intent,
        );
    }
}

#[test]
fn backspace_over_a_range_into_a_code_block_deletes_the_range_without_joining_a_mention_into_code()
{
    let mention = json!({ "type": "mention", "attrs": { "id": "user-1", "label": "Ann" } });
    let mut session = table_session_with_schema(
        &doc(json!([
            empty_paragraph(),
            { "type": "codeBlock", "content": [text("code\nline")] },
            paragraph(json!([mention.clone()]))
        ]))
        .to_string(),
        mention_table_schema(),
    );
    applied(
        &mut session,
        FIRST_PLANNING_REQUEST,
        (12, 4),
        json!({ "type": "deleteBackward" }),
    );
    assert_eq!(
        session.engine.document_json().unwrap(),
        doc(json!([
            empty_paragraph(),
            { "type": "codeBlock", "content": [text("co")] },
            paragraph(json!([mention]))
        ]))
    );
}
