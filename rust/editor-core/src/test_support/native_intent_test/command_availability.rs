use super::*;
use crate::test_support::mention_schema::mention_table_schema;
use serde_json::{json, Value};

const AVAILABILITY_OWNER: u64 = 53_000;
const FIRST_AVAILABILITY_REQUEST: u64 = 53_100;

fn toolbar_commands() -> Vec<(&'static str, Value)> {
    vec![
        ("toggleBlockquote", json!({ "type": "toggleBlockquote" })),
        ("toggleCodeBlock", json!({ "type": "toggleCodeBlock" })),
        (
            "toggleHeading1",
            json!({ "type": "toggleHeading", "level": 1 }),
        ),
        (
            "wrapBulletList",
            json!({ "type": "applyListType", "listType": "bulletList" }),
        ),
        (
            "wrapOrderedList",
            json!({ "type": "applyListType", "listType": "orderedList" }),
        ),
        (
            "wrapTaskList",
            json!({ "type": "applyListType", "listType": "taskList" }),
        ),
    ]
}

fn availability_documents() -> Vec<Value> {
    let paragraph = |content: Value| json!({ "type": "paragraph", "content": content });
    let text = |value: &str| json!({ "type": "text", "text": value });
    let item = |content: Value| json!({ "type": "listItem", "content": content });
    let mention = json!({ "type": "mention", "attrs": { "id": "user-1", "label": "Ann" } });
    vec![
        json!({ "type": "doc", "content": [
            { "type": "bulletList", "content": [
                item(json!([paragraph(json!([text("a")])), { "type": "blockquote", "content": [paragraph(json!([text("b")]))] }])),
                item(json!([paragraph(json!([text("c")])), { "type": "heading", "attrs": { "level": 2 }, "content": [text("d")] }]))
            ] }
        ] }),
        json!({ "type": "doc", "content": [
            paragraph(json!([text("e"), { "type": "hardBreak" }, text("f")])),
            paragraph(json!([text("g"), mention])),
            { "type": "blockquote", "content": [paragraph(json!([text("h")]))] },
            { "type": "heading", "attrs": { "level": 1 }, "content": [text("i")] }
        ] }),
        json!({ "type": "doc", "content": [
            { "type": "orderedList", "content": [
                item(json!([paragraph(json!([text("j")])), { "type": "bulletList", "content": [item(json!([paragraph(json!([text("k")]))]))] }]))
            ] },
            { "type": "codeBlock", "content": [text("l")] }
        ] }),
        json!({ "type": "doc", "content": [
            { "type": "table", "content": [{ "type": "tableRow", "content": [
                { "type": "tableCell", "content": [paragraph(json!([text("m")])), { "type": "blockquote", "content": [paragraph(json!([text("n")]))] }] },
                { "type": "tableCell", "content": [{ "type": "bulletList", "content": [item(json!([paragraph(json!([text("o")]))]))] }] }
            ] }] }
        ] }),
    ]
}

fn submit_at(session: &mut EditorSession, request_id: u64, caret: u32, mut intent: Value) -> Value {
    intent["anchor"] = json!(caret);
    intent["head"] = json!(caret);
    let request = native_intent_request(session, AVAILABILITY_OWNER, request_id, intent.clone());
    let outcome = NativeTransactionBridge::new(session)
        .submit_native_intent(&request)
        .unwrap_or_else(|error| {
            panic!(
                "{intent} at {caret} on {} was refused: {} {}",
                session.engine.document_json().unwrap(),
                error.code,
                error.message
            )
        });
    serde_json::from_str(&outcome).unwrap()
}

#[test]
fn structural_commands_apply_exactly_when_the_toolbar_offers_them() {
    let mut request_id = FIRST_AVAILABILITY_REQUEST;
    for document in availability_documents() {
        let total = table_session_with_schema(&document.to_string(), mention_table_schema())
            .engine
            .position_map()
            .unwrap()
            .total_scalars();
        for caret in 0..=total {
            for (toolbar_name, command) in toolbar_commands() {
                let mut session =
                    table_session_with_schema(&document.to_string(), mention_table_schema());
                request_id += 2;
                submit_at(
                    &mut session,
                    request_id,
                    caret,
                    json!({ "type": "setSelection" }),
                );
                let offered = session.engine.active_state().unwrap().commands[toolbar_name];
                let before = session.engine.document_json().unwrap();
                let outcome = submit_at(
                    &mut session,
                    request_id + 1,
                    caret,
                    json!({ "type": "command", "command": command.clone() }),
                );
                let changed = session.engine.document_json().unwrap() != before;
                assert_eq!(
                    changed, offered,
                    "{toolbar_name} offered={offered} at caret {caret} on {before}: {outcome}"
                );
                if !offered {
                    assert_eq!(
                        outcome["type"], "notApplicable",
                        "{toolbar_name} at {caret}: {outcome}"
                    );
                }
            }
        }
    }
}

#[test]
fn node_insertion_applies_exactly_when_the_toolbar_lists_the_node_as_insertable() {
    let mut request_id = FIRST_AVAILABILITY_REQUEST;
    for document in availability_documents() {
        let total = table_session_with_schema(&document.to_string(), mention_table_schema())
            .engine
            .position_map()
            .unwrap()
            .total_scalars();
        for caret in 0..=total {
            for node_type in ["hardBreak", "horizontalRule"] {
                let mut session =
                    table_session_with_schema(&document.to_string(), mention_table_schema());
                request_id += 2;
                submit_at(
                    &mut session,
                    request_id,
                    caret,
                    json!({ "type": "setSelection" }),
                );
                let offered = session
                    .engine
                    .active_state()
                    .unwrap()
                    .insertable_nodes
                    .iter()
                    .any(|name| name == node_type);
                let before = session.engine.document_json().unwrap();
                let outcome = submit_at(
                    &mut session,
                    request_id + 1,
                    caret,
                    json!({ "type": "command", "command": { "type": "insertNode", "nodeType": node_type } }),
                );
                let changed = session.engine.document_json().unwrap() != before;
                assert_eq!(
                    changed, offered,
                    "{node_type} insertable={offered} at caret {caret} on {before}: {outcome}"
                );
            }
        }
    }
}
