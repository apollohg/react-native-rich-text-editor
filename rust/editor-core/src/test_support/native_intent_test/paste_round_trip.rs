use super::*;
use crate::schema::presets::tiptap_table_schema;
use crate::serialize::{from_prosemirror_json, to_html, to_prosemirror_json, UnknownTypeMode};
use crate::test_support::document_strategy::document_strategy;
use proptest::prelude::*;
use serde_json::{json, Value};

const ROUND_TRIP_OWNER: u64 = 54_000;
const FIRST_ROUND_TRIP_REQUEST: u64 = 54_100;
const ROUND_TRIP_CASES: u32 = 512;
const TWO_CELL_TABLE_HTML: &str = "<table><tr><td>c</td><td>d</td></tr></table>";

fn html_session(html: &str) -> EditorSession {
    let config = EditorSessionConfig::local_for_test();
    let mut engine = table_engine(tiptap_table_schema(), InitializationMode::LocalEmpty);
    engine
        .import_html(
            html,
            &crate::serialize::FromHtmlOptions {
                strict: false,
                allow_base64_images: false,
            },
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    EditorSession::new(
        engine,
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap()
}

fn submit_at(
    session: &mut EditorSession,
    request_id: u64,
    (anchor, head): (u32, u32),
    mut intent: Value,
) -> Result<Value, crate::session::SessionError> {
    intent["anchor"] = json!(anchor);
    intent["head"] = json!(head);
    let request = native_intent_request(session, ROUND_TRIP_OWNER, request_id, intent);
    NativeTransactionBridge::new(session)
        .submit_native_intent(&request)
        .map(|outcome| serde_json::from_str(&outcome).unwrap())
}

fn paste_copy_over_itself(
    session: &mut EditorSession,
    copied: (u32, u32),
    selection: (u32, u32),
) -> Option<String> {
    submit_at(
        session,
        FIRST_ROUND_TRIP_REQUEST,
        copied,
        json!({ "type": "setSelection" }),
    )
    .unwrap();
    let clipboard = session.engine.clipboard()?;
    let fragment = clipboard.get("fragment")?.clone();
    let before = session.engine.document_json().unwrap();
    let outcome = submit_at(
        session,
        FIRST_ROUND_TRIP_REQUEST + 1,
        selection,
        json!({ "type": "command", "command": {
            "type": "paste",
            "fragment": fragment,
            "html": clipboard["html"],
            "text": clipboard["text"],
        } }),
    );
    match outcome {
        Ok(_) => crate::transform::DocumentValidator::validate(
            session.engine.document().unwrap(),
            session.engine.schema(),
            session.engine.resource_limits(),
        )
        .err()
        .map(|error| {
            format!(
                "pasting the copy of {selection:?} on {before} left an invalid document: {error}"
            )
        }),
        Err(error) => Some(format!(
            "pasting the copy of {selection:?} back over itself on {before} failed: {} {}",
            error.code, error.message
        )),
    }
}

#[test]
fn a_copied_html_table_pasted_back_over_its_cells_changes_nothing() {
    let mut session = html_session(TWO_CELL_TABLE_HTML);
    let before = session.engine.document_json().unwrap();
    let total = session.engine.position_map().unwrap().total_scalars();
    submit_at(
        &mut session,
        FIRST_ROUND_TRIP_REQUEST,
        (0, total),
        json!({ "type": "setSelection" }),
    )
    .unwrap();
    let clipboard = session.engine.clipboard().unwrap();
    let outcome = submit_at(
        &mut session,
        FIRST_ROUND_TRIP_REQUEST + 1,
        (2, 0),
        json!({ "type": "command", "command": {
            "type": "paste",
            "fragment": clipboard["fragment"],
            "html": clipboard["html"],
            "text": clipboard["text"],
        } }),
    )
    .unwrap_or_else(|error| panic!("{} {}", error.code, error.message));
    assert_eq!(outcome["documentChanged"], false, "{outcome}");
    assert_eq!(session.engine.document_json().unwrap(), before);
}

#[test]
fn documents_holding_explicit_null_attrs_load_like_documents_without_them() {
    let schema = tiptap_table_schema();
    let table = |cell_attrs: Value| {
        json!({ "type": "doc", "content": [{ "type": "table", "content": [{ "type": "tableRow", "content": [
            { "type": "tableCell", "attrs": cell_attrs, "content": [{ "type": "paragraph" }] }
        ] }] }] })
    };
    let explicit = table(json!({ "colspan": 1, "rowspan": 1, "colwidth": null }));
    let implicit = table(json!({ "colspan": 1, "rowspan": 1 }));
    let explicit_document =
        from_prosemirror_json(&explicit, &schema, UnknownTypeMode::Error).unwrap();
    let implicit_document =
        from_prosemirror_json(&implicit, &schema, UnknownTypeMode::Error).unwrap();
    assert_eq!(explicit_document, implicit_document);
    let html_document = crate::serialize::from_html(
        &to_html(&implicit_document, &schema),
        &schema,
        &crate::serialize::FromHtmlOptions {
            strict: false,
            allow_base64_images: false,
        },
    )
    .unwrap();
    assert_eq!(
        html_document, implicit_document,
        "HTML and JSON ingestion agree on unset attrs"
    );
    assert_eq!(
        to_prosemirror_json(&explicit_document, &schema),
        to_prosemirror_json(&implicit_document, &schema),
        "the canonical JSON of stored documents is unchanged"
    );
    let session = table_session_with_schema(&explicit.to_string(), schema);
    assert_eq!(
        session.engine.document_json().unwrap(),
        to_prosemirror_json(&implicit_document, session.engine.schema())
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(ROUND_TRIP_CASES))]

    #[test]
    fn any_copied_fragment_pastes_back_over_itself(
        document in document_strategy(),
        through_html in any::<bool>(),
        paste_at_start in any::<bool>(),
        whole_document in any::<bool>(),
        anchor in any::<u32>(),
        head in any::<u32>(),
    ) {
        let mut session = if through_html {
            let schema = tiptap_table_schema();
            let parsed = from_prosemirror_json(&document, &schema, UnknownTypeMode::Error).unwrap();
            html_session(&to_html(&parsed, &schema))
        } else {
            table_session_with_schema(&document.to_string(), tiptap_table_schema())
        };
        let total = session.engine.position_map().unwrap().total_scalars();
        let copied = if whole_document {
            (0, total)
        } else {
            (anchor % (total + 1), head % (total + 1))
        };
        let selection = if paste_at_start {
            let start = copied.0.min(copied.1);
            (start, start)
        } else {
            copied
        };
        let failure = paste_copy_over_itself(&mut session, copied, selection);
        prop_assert!(failure.is_none(), "{}", failure.unwrap_or_default());
    }
}

#[test]
fn pasting_a_copied_list_back_over_its_own_text_is_a_no_op() {
    let document = json!({ "type": "doc", "content": [
        { "type": "orderedList", "content": [{ "type": "listItem", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "wxyz" }] }
        ] }] },
        { "type": "blockquote", "content": [
            { "type": "paragraph" },
            { "type": "blockquote", "content": [{ "type": "paragraph" }] }
        ] },
        { "type": "horizontalRule" }
    ] });
    let mut session = table_session_with_schema(&document.to_string(), tiptap_table_schema());
    let before = session.engine.document_json().unwrap();
    assert_eq!(paste_copy_over_itself(&mut session, (0, 7), (0, 7)), None);
    assert_eq!(session.engine.document_json().unwrap(), before);
}
