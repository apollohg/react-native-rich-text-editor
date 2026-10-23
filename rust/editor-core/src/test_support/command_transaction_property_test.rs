use proptest::prelude::*;
use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::native_transaction_bridge::NativeTransactionBridge;
use crate::session::{
    CollaborationLimits, DocumentState, EditorSession, EditorSessionConfig, SessionError,
    SessionPolicy,
};
use crate::test_support::document_strategy::{
    document_strategy_with_inline, hard_break_json, text_strategy,
};
use crate::test_support::mention_schema::{mention_table_schema, MENTION_NODE};
use crate::transform::DocumentValidator;
use crate::yrs_engine::{
    EditingLimits, InitializationMode, ReplacementHistory, YrsDocumentCodec, YrsDocumentEngine,
    YrsEngineConfig,
};

const COMMAND_TRANSACTION_CASES: u32 = 512;
const MAX_STEPS: usize = 8;
const ENVELOPE_VERSION: u32 = 1;
const OWNER: u64 = 41_000;
const IMPORT_REQUEST: u64 = 41_001;
const FIRST_STEP_REQUEST: u64 = 41_100;
const REQUESTS_PER_STEP: u64 = 4;
const SELECTION_REQUEST_OFFSET: u64 = 1;
const COMMAND_REQUEST_OFFSET: u64 = 2;
const REFUSAL_CODES: [&str; 4] = [
    "ENGINE_INVARIANT_FAILED",
    "OPERATION_INVALID",
    "POSITION_INVALID",
    "DOCUMENT_INVALID",
];

fn mention_json() -> Value {
    json!({ "type": MENTION_NODE, "attrs": { "id": "user-1", "label": "Ann" } })
}

fn session_with_document(document: &Value) -> Option<EditorSession> {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        YrsDocumentEngine::new(YrsEngineConfig {
            schema: mention_table_schema(),
            fragment_name: "prosemirror".into(),
            initialization_mode: InitializationMode::LocalEmpty,
            resource_limits: ResourceLimits::default(),
            editing_limits: EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .unwrap(),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            IMPORT_REQUEST,
            &document.to_string(),
            ReplacementHistory::ResetAndClear,
        )
        .ok()?;
    Some(session)
}

#[derive(Debug, Clone)]
struct SelectionSeed {
    anchor: u32,
    head: u32,
    collapsed: bool,
}

impl SelectionSeed {
    fn resolve(&self, total_scalars: u32) -> (u32, u32) {
        let anchor = self.anchor % (total_scalars + 1);
        let head = if self.collapsed {
            anchor
        } else {
            self.head % (total_scalars + 1)
        };
        (anchor, head)
    }
}

#[derive(Debug, Clone)]
enum CommandStep {
    Intent(Value),
    Input(&'static str),
    Command(Value),
    CopyPaste(SelectionSeed),
    MoveSelection(u32),
    Undo,
    Redo,
}

fn typed_text_strategy() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["z", "é", "🦀", "ab", " "])
}

fn multiline_text_strategy() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "z",
        "ab",
        "a\nb",
        "\n",
        "one\n\nthree",
        "\nx",
        "x\n",
        "a\r\nb",
    ])
}

fn pasted_html_strategy() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "<p>one</p><p>two</p>",
        "<p>a<br>b</p>",
        "<br>",
        "x<br>",
        "<br>y",
        "<p></p><p>e</p>",
        "<ul><li><p>i</p></li><li><p>j</p></li></ul>",
        "<h1>head</h1><p>body</p>",
        "<blockquote><p>q</p></blockquote><p>r</p>",
        "<p>t</p><hr><p>u</p>",
        "<table><tr><td>c</td><td>d</td></tr></table>",
        "<pre>code\nline</pre>",
    ])
}

fn inserted_json_strategy() -> impl Strategy<Value = Value> {
    prop::sample::select(vec![
        json!({ "type": "doc", "content": [mention_json(), { "type": "text", "text": " " }] }),
        json!({ "type": "doc", "content": [hard_break_json()] }),
        json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "p" }] },
            { "type": "paragraph" }
        ] }),
        json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [hard_break_json()] }
        ] }),
        json!({ "type": "doc", "content": [{ "type": "image", "attrs": { "src": "x.png" } }] }),
        json!({ "type": "doc", "content": [{ "type": "bulletList", "content": [
            { "type": "listItem", "content": [{ "type": "paragraph", "content": [
                { "type": "text", "text": "li" }
            ] }] }
        ] }] }),
    ])
}

fn editor_command_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        prop::sample::select(vec![
            json!({ "type": "deleteBackward" }),
            json!({ "type": "splitBlock" }),
            json!({ "type": "toggleMark", "markType": "bold" }),
            json!({ "type": "toggleMark", "markType": "italic" }),
            json!({ "type": "setMark", "markType": "link", "attrs": { "href": "https://example.com" } }),
            json!({ "type": "unsetMark", "markType": "link" }),
            json!({ "type": "toggleHeading", "level": 1 }),
            json!({ "type": "toggleHeading", "level": 2 }),
            json!({ "type": "toggleBlockquote" }),
            json!({ "type": "toggleCodeBlock" }),
            json!({ "type": "applyListType", "listType": "bulletList" }),
            json!({ "type": "applyListType", "listType": "orderedList" }),
            json!({ "type": "applyListType", "listType": "taskList" }),
            json!({ "type": "wrapInList", "listType": "bulletList", "itemType": "listItem" }),
            json!({ "type": "unwrapFromList" }),
            json!({ "type": "indentListItem" }),
            json!({ "type": "outdentListItem" }),
            json!({ "type": "insertNode", "nodeType": "hardBreak" }),
            json!({ "type": "insertNode", "nodeType": "horizontalRule" }),
            json!({ "type": "toggleTaskItemChecked" }),
        ]),
        multiline_text_strategy().prop_map(|text| json!({ "type": "paste", "text": text })),
        multiline_text_strategy()
            .prop_map(|text| json!({ "type": "paste", "text": text, "plainText": true })),
        pasted_html_strategy().prop_map(|html| json!({ "type": "paste", "html": html })),
    ]
}

fn intent_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        4 => typed_text_strategy().prop_map(|text| json!({ "type": "insertText", "text": text })),
        2 => multiline_text_strategy()
            .prop_map(|text| json!({ "type": "replaceSelectionText", "text": text })),
        3 => Just(json!({ "type": "deleteBackward" })),
        2 => Just(json!({ "type": "deleteForward" })),
        1 => (0u32..3, 0u32..3).prop_map(|(before, after)| {
            json!({ "type": "deleteSurroundingText", "before": before, "after": after })
        }),
        1 => Just(json!({ "type": "deleteRange" })),
        3 => Just(json!({ "type": "splitBlock" })),
        1 => Just(json!({ "type": "deleteAndSplit" })),
        2 => pasted_html_strategy().prop_map(|html| json!({ "type": "insertContentHtml", "html": html })),
        2 => inserted_json_strategy().prop_map(|json| json!({ "type": "insertContentJson", "json": json })),
        4 => editor_command_strategy().prop_map(|command| json!({ "type": "command", "command": command })),
    ]
}

fn selection_seed_strategy() -> impl Strategy<Value = SelectionSeed> {
    (any::<u32>(), any::<u32>(), prop::bool::weighted(0.6)).prop_map(|(anchor, head, collapsed)| {
        SelectionSeed {
            anchor,
            head,
            collapsed,
        }
    })
}

fn step_strategy() -> impl Strategy<Value = (SelectionSeed, CommandStep)> {
    (
        selection_seed_strategy(),
        prop_oneof![
            12 => intent_strategy().prop_map(CommandStep::Intent),
            2 => multiline_text_strategy().prop_map(CommandStep::Input),
            2 => editor_command_strategy().prop_map(CommandStep::Command),
            2 => selection_seed_strategy().prop_map(CommandStep::CopyPaste),
            1 => any::<u32>().prop_map(CommandStep::MoveSelection),
            1 => Just(CommandStep::Undo),
            1 => Just(CommandStep::Redo),
        ],
    )
}

fn inline_strategy() -> BoxedStrategy<Value> {
    prop_oneof![
        3 => text_strategy(),
        2 => Just(hard_break_json()),
        1 => Just(mention_json()),
    ]
    .boxed()
}

fn total_scalars(session: &EditorSession) -> u32 {
    session.engine.position_map().unwrap().total_scalars()
}

fn native_intent(
    session: &mut EditorSession,
    request_id: u64,
    (anchor, head): (u32, u32),
    intent: &Value,
) -> Result<String, SessionError> {
    let epoch = session
        .pin_position_epoch(OWNER, session.engine.revision())
        .unwrap();
    let mut intent = intent.clone();
    intent["anchor"] = json!(anchor);
    intent["head"] = json!(head);
    let request = json!({
        "version": ENVELOPE_VERSION,
        "requestId": request_id.to_string(),
        "ownerId": OWNER.to_string(),
        "positionEpoch": epoch.to_string(),
        "intent": intent,
    })
    .to_string();
    NativeTransactionBridge::new(session).submit_native_intent(&request)
}

fn select(session: &mut EditorSession, request_id: u64, selection: (u32, u32)) {
    native_intent(
        session,
        request_id,
        selection,
        &json!({ "type": "setSelection" }),
    )
    .expect("selecting a resolved scalar range succeeds");
}

fn api_request(session: &EditorSession, request_id: u64, payload: (&str, Value)) -> String {
    let (field, value) = payload;
    let mut request = json!({
        "version": ENVELOPE_VERSION,
        "requestId": request_id.to_string(),
        "baseDocumentRevision": session.engine.revision().to_string(),
    });
    request[field] = value;
    request.to_string()
}

fn scalar_position(offset: u32) -> Value {
    json!({ "offset": offset, "kind": "scalar" })
}

fn copy_paste_intent(
    session: &mut EditorSession,
    request_id: u64,
    source: &SelectionSeed,
) -> Option<Value> {
    let copied = source.resolve(total_scalars(session));
    select(session, request_id, copied);
    let clipboard = session.engine.clipboard().unwrap();
    clipboard.get("fragment").map(|fragment| {
        json!({ "type": "command", "command": {
            "type": "paste",
            "fragment": fragment,
            "html": clipboard["html"],
            "text": clipboard["text"],
        } })
    })
}

fn move_selection_intent(selection: (u32, u32), at: u32) -> Value {
    let (anchor, head) = selection;
    json!({ "type": "command", "command": {
        "type": "moveSelection",
        "range": {
            "from": scalar_position(anchor.min(head)),
            "to": scalar_position(anchor.max(head)),
        },
        "at": scalar_position(at),
    } })
}

fn run_step(
    session: &mut EditorSession,
    request_id: u64,
    selection: (u32, u32),
    step: &CommandStep,
) -> Option<(Value, Result<(), SessionError>)> {
    let command_request = request_id + COMMAND_REQUEST_OFFSET;
    let intent = |session: &mut EditorSession, intent: Value| {
        let outcome = native_intent(session, command_request, selection, &intent).map(drop);
        (intent, outcome)
    };
    Some(match step {
        CommandStep::Intent(value) => intent(session, value.clone()),
        CommandStep::CopyPaste(source) => {
            let paste = copy_paste_intent(session, request_id + SELECTION_REQUEST_OFFSET, source)?;
            intent(session, paste)
        }
        CommandStep::MoveSelection(target) => {
            let at = target % (total_scalars(session) + 1);
            intent(session, move_selection_intent(selection, at))
        }
        CommandStep::Input(text) => {
            select(session, request_id + SELECTION_REQUEST_OFFSET, selection);
            let request = api_request(session, command_request, ("text", json!(text)));
            let outcome = NativeTransactionBridge::new(session)
                .submit_input(&request)
                .map(drop);
            (json!({ "input": text }), outcome)
        }
        CommandStep::Command(command) => {
            select(session, request_id + SELECTION_REQUEST_OFFSET, selection);
            let request = api_request(session, command_request, ("command", command.clone()));
            let outcome = NativeTransactionBridge::new(session)
                .submit_command(&request)
                .map(drop);
            (json!({ "apiCommand": command }), outcome)
        }
        CommandStep::Undo => {
            let outcome = NativeTransactionBridge::new(session)
                .undo(command_request)
                .map(drop);
            (json!({ "history": "undo" }), outcome)
        }
        CommandStep::Redo => {
            let outcome = NativeTransactionBridge::new(session)
                .redo(command_request)
                .map(drop);
            (json!({ "history": "redo" }), outcome)
        }
    })
}

#[derive(Debug, PartialEq)]
struct CommittedState {
    document: Value,
    revision: u64,
}

fn committed_state(session: &EditorSession) -> CommittedState {
    CommittedState {
        document: session.engine.document_json().unwrap(),
        revision: session.engine.revision(),
    }
}

fn assert_document_consistent(session: &EditorSession, context: &str) {
    let engine = &session.engine;
    DocumentValidator::validate(
        engine.document().unwrap(),
        engine.schema(),
        engine.resource_limits(),
    )
    .unwrap_or_else(|error| panic!("{context}: committed document is invalid: {error}"));
    let authority = engine
        .read_fragment_for_test(|txn, fragment| {
            YrsDocumentCodec::new(engine.schema(), engine.resource_limits())
                .read_json(fragment, txn)
                .unwrap()
        })
        .unwrap();
    assert_eq!(
        authority,
        engine.document_json().unwrap(),
        "{context}: the Yrs authority diverged from the committed document"
    );
}

fn run_command_sequence(document: &Value, steps: &[(SelectionSeed, CommandStep)]) {
    let Some(mut session) = session_with_document(document) else {
        return;
    };
    assert_document_consistent(&session, &format!("import of {document}"));
    let mut replay = Vec::with_capacity(steps.len());
    for (index, (selection_seed, step)) in steps.iter().enumerate() {
        let request_id = FIRST_STEP_REQUEST + REQUESTS_PER_STEP * index as u64;
        let before = committed_state(&session);
        let selection = selection_seed.resolve(total_scalars(&session));
        let Some((sent, outcome)) = run_step(&mut session, request_id, selection, step) else {
            continue;
        };
        replay.push(json!({ "selection": [selection.0, selection.1], "sent": sent }));
        let context = format!(
            "step {index} sent {sent} at {selection:?} on {}; replay from {document}: {}",
            before.document,
            Value::Array(replay.clone())
        );
        if let Err(error) = outcome {
            assert!(
                !REFUSAL_CODES.contains(&error.code.as_str()),
                "{context}: a real editor command was refused with {}: {}",
                error.code,
                error.message
            );
            assert_eq!(
                committed_state(&session),
                before,
                "{context}: failed command {} changed the document",
                error.code
            );
        }
        assert_document_consistent(&session, &context);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(COMMAND_TRANSACTION_CASES))]

    #[test]
    fn real_editor_commands_never_hit_transaction_refusals(
        document in document_strategy_with_inline(inline_strategy()),
        steps in prop::collection::vec(step_strategy(), 1..MAX_STEPS),
    ) {
        run_command_sequence(&document, &steps);
    }
}

#[test]
fn ranged_deletion_across_a_nested_table_keeps_selection_representable() {
    let document = json!({ "type": "doc", "content": [
        { "type": "blockquote", "content": [
            { "type": "blockquote", "content": [{ "type": "horizontalRule" }] }
        ] }
    ] });
    let table = json!({ "type": "table", "content": [
        { "type": "tableRow", "content": [
            { "type": "tableCell", "content": [paragraph(json!([plain("c")]))] },
            { "type": "tableCell", "content": [paragraph(json!([plain("d")]))] }
        ] }
    ] });
    let mut expected = document.clone();
    expected["content"][0]["content"][0]["content"] = json!([table, { "type": "horizontalRule" }]);
    let cases = [
        ((8, 3), "deleteBackward"),
        ((3, 8), "deleteBackward"),
        ((8, 3), "deleteForward"),
        ((3, 8), "deleteForward"),
    ];
    for (selection, deletion) in cases {
        let mut session = session_with_document(&document).unwrap();
        let steps = [
            (
                (0, 0),
                json!({ "type": "insertContentHtml", "html":
                "<table><tr><td>c</td><td>d</td></tr></table>" }),
            ),
            (
                (4, 4),
                json!({ "type": "insertContentJson", "json": {
                "type": "doc", "content": [{ "type": "image", "attrs": { "src": "x.png" } }]
            } }),
            ),
            (selection, json!({ "type": deletion })),
        ];
        for (index, (selection, intent)) in steps.iter().enumerate() {
            let request_id = FIRST_STEP_REQUEST + REQUESTS_PER_STEP * index as u64;
            native_intent(&mut session, request_id, *selection, intent).unwrap_or_else(|error| {
                panic!(
                    "step {index}: {intent} at {selection:?}: {}: {}",
                    error.code, error.message
                )
            });
            assert_document_consistent(&session, &format!("step {index}: {intent}"));
        }
        assert_eq!(session.engine.document_json().unwrap(), expected,
            "{deletion} at {selection:?} must remove the selected image and empty paragraph while retaining the table");
        let Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head }) =
            session.engine.resolved_selection()
        else {
            panic!("{deletion} at {selection:?} must leave a text caret");
        };
        let expected_caret = selection.0.min(selection.1);
        assert_eq!(
            (anchor.scalar, head.scalar),
            (expected_caret, expected_caret),
            "{deletion} at {selection:?} must collapse at the start of the deleted range"
        );
    }
}

#[test]
fn inline_content_pasted_inside_a_marked_run_in_one_yrs_text_splits_that_text() {
    let mut session = session_with_document(&json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [
            { "type": "text", "text": "bc" },
            { "type": "text", "text": "wxyz", "marks": [{ "type": "italic" }] }
        ] }
    ] }))
    .unwrap();
    let steps = [
        (
            (2, 3),
            json!({ "type": "replaceSelectionText", "text": "z" }),
        ),
        (
            (5, 5),
            json!({ "type": "command", "command": { "type": "paste", "html": "<p>c</p>" } }),
        ),
        (
            (3, 3),
            json!({ "type": "insertContentJson", "json": {
                "type": "doc",
                "content": [mention_json(), { "type": "text", "text": " " }]
            } }),
        ),
    ];
    for (index, (selection, intent)) in steps.iter().enumerate() {
        let request_id = FIRST_STEP_REQUEST + REQUESTS_PER_STEP * index as u64;
        let before = session.engine.document_json().unwrap();
        native_intent(&mut session, request_id, *selection, intent).unwrap_or_else(|error| {
            panic!(
                "step {index} {intent} at {selection:?} on {before}: {}: {}",
                error.code, error.message
            )
        });
        assert_document_consistent(&session, &format!("step {index} {intent}"));
    }
    let italic = || json!([{ "type": "italic" }]);
    assert_eq!(
        session.engine.document_json().unwrap()["content"],
        json!([{ "type": "paragraph", "content": [
            { "type": "text", "text": "bc" },
            { "type": "text", "text": "z", "marks": italic() },
            mention_json(),
            { "type": "text", "text": " " },
            { "type": "text", "text": "xy", "marks": italic() },
            { "type": "text", "text": "c" },
            { "type": "text", "text": "z", "marks": italic() }
        ] }])
    );
}

#[test]
fn lowering_refusals_retry_structurally_through_every_command_entry_point() {
    let refused_after_backspace = || {
        json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "xy" }] },
            { "type": "paragraph", "content": [{ "type": "text", "text": "a" }, hard_break_json()] }
        ] })
    };
    let typed_over_emptied_paragraph = json!([{ "type": "paragraph", "content": [
        { "type": "text", "text": "z" },
        hard_break_json()
    ] }]);
    let cases = [
        (
            "native return from an empty list paragraph into its sibling",
            json!({ "type": "doc", "content": [{ "type": "bulletList", "content": [
                { "type": "listItem", "content": [
                    { "type": "paragraph" },
                    { "type": "paragraph", "content": [{ "type": "text", "text": "bcbc" }] }
                ] }
            ] }] }),
            (4, 0),
            CommandStep::Intent(json!({ "type": "deleteAndSplit" })),
            json!([{ "type": "bulletList", "content": [
                { "type": "listItem", "content": [{ "type": "paragraph" }] },
                { "type": "listItem", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "bcbc" }] }
                ] }
            ] }]),
        ),
        (
            "native return over the boundary after an empty paragraph",
            json!({ "type": "doc", "content": [
                { "type": "paragraph" },
                { "type": "paragraph", "content": [{ "type": "text", "text": "b" }] }
            ] }),
            (1, 2),
            CommandStep::Intent(json!({ "type": "deleteAndSplit" })),
            json!([
                { "type": "paragraph" },
                { "type": "paragraph", "content": [{ "type": "text", "text": "b" }] }
            ]),
        ),
        (
            "native typing over an emptied paragraph",
            refused_after_backspace(),
            (3, 0),
            CommandStep::Intent(json!({ "type": "replaceSelectionText", "text": "z" })),
            typed_over_emptied_paragraph.clone(),
        ),
        (
            "api command typing over an emptied paragraph",
            refused_after_backspace(),
            (3, 0),
            CommandStep::Command(json!({ "type": "replaceSelectionText", "text": "z" })),
            typed_over_emptied_paragraph,
        ),
    ];
    for (index, (label, document, selection, step, expected)) in cases.into_iter().enumerate() {
        let mut session = session_with_document(&document).unwrap();
        let request_id = FIRST_STEP_REQUEST + REQUESTS_PER_STEP * 2 * index as u64;
        if label.ends_with("emptied paragraph") {
            native_intent(
                &mut session,
                request_id,
                (0, 2),
                &json!({ "type": "deleteBackward" }),
            )
            .unwrap();
        }
        let (sent, outcome) = run_step(
            &mut session,
            request_id + REQUESTS_PER_STEP,
            selection,
            &step,
        )
        .unwrap();
        outcome.unwrap_or_else(|error| {
            panic!(
                "{label}: {sent} was refused with {}: {}",
                error.code, error.message
            )
        });
        assert_document_consistent(&session, label);
        assert_eq!(
            session.engine.document_json().unwrap()["content"],
            expected,
            "{label}"
        );
    }
}

#[test]
fn a_hard_break_inserted_inside_text_keeps_the_trailing_atom_gap_out_of_the_layout() {
    let bold = || json!([{ "type": "bold" }]);
    let italic = || json!([{ "type": "italic" }]);
    let cell = |content: Value| {
        json!({ "type": "tableRow", "content": [
        { "type": "tableCell", "content": [content] }
    ] })
    };
    let list_paragraph = |inline: Value| {
        json!({ "type": "bulletList", "content": [
            { "type": "listItem", "content": [{ "type": "paragraph", "content": inline }] }
        ] })
    };
    let document = |inline: Value| {
        json!({ "type": "doc", "content": [{ "type": "table", "content": [
            cell(json!({ "type": "paragraph", "content": [
                { "type": "text", "text": "q", "marks": bold() }
            ] })),
            cell(list_paragraph(inline))
        ] }] })
    };
    let mut session = session_with_document(&document(json!([
        { "type": "text", "text": "wxyz", "marks": bold() },
        { "type": "text", "text": "dé", "marks": italic() },
        mention_json()
    ])))
    .unwrap();
    let (sent, outcome) = run_step(
        &mut session,
        FIRST_STEP_REQUEST,
        (6, 6),
        &CommandStep::Command(json!({ "type": "insertNode", "nodeType": "hardBreak" })),
    )
    .unwrap();
    outcome.unwrap_or_else(|error| {
        panic!("{sent} was refused with {}: {}", error.code, error.message)
    });
    assert_document_consistent(&session, "hard break inside bold text before an atom");
    assert_eq!(
        session.engine.document_json().unwrap(),
        document(json!([
            { "type": "text", "text": "wx", "marks": bold() },
            hard_break_json(),
            { "type": "text", "text": "yz", "marks": bold() },
            { "type": "text", "text": "dé", "marks": italic() },
            mention_json()
        ]))
    );
}

fn paragraph(inline: Value) -> Value {
    json!({ "type": "paragraph", "content": inline })
}

fn marked(text: &str, marks: Value) -> Value {
    json!({ "type": "text", "text": text, "marks": marks })
}

fn plain(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

fn apply_mark_command(document: Value, selection: Option<(u32, u32)>, command: Value) -> Value {
    let mut session =
        session_with_document(&json!({ "type": "doc", "content": document })).unwrap();
    let selection = selection.unwrap_or((0, total_scalars(&session)));
    let label = format!("{command} at {selection:?} on {document}");
    let (_, outcome) = run_step(
        &mut session,
        FIRST_STEP_REQUEST,
        selection,
        &CommandStep::Intent(json!({ "type": "command", "command": command })),
    )
    .unwrap();
    outcome.unwrap_or_else(|error| panic!("{label}: {}: {}", error.code, error.message));
    assert_document_consistent(&session, &label);
    session.engine.document_json().unwrap()["content"].clone()
}

#[test]
fn marks_format_the_text_around_hard_breaks_in_one_operation_per_textblock() {
    let bold = || json!([{ "type": "bold" }]);
    let toggle_bold = || json!({ "type": "toggleMark", "markType": "bold" });
    let break_line =
        |run: &dyn Fn(&str) -> Value| paragraph(json!([run("cd"), hard_break_json(), run("ef")]));
    let mut session =
        session_with_document(&json!({ "type": "doc", "content": [break_line(&plain)] })).unwrap();
    let whole = (0, total_scalars(&session));
    select(&mut session, FIRST_STEP_REQUEST, whole);
    let plan = session
        .engine
        .plan_command(
            FIRST_STEP_REQUEST + 1,
            crate::yrs_engine::TypedCommand::ToggleMark {
                mark_type: "bold".into(),
            },
        )
        .unwrap();
    let crate::yrs_engine::CommandPlan::Transaction(transaction) = plan else {
        panic!("bold over cd<br>ef must plan a transaction, planned {plan:?}")
    };
    assert!(
        matches!(
            transaction.operations.as_slice(),
            [crate::yrs_engine::TypedOperation::AddMark { .. }]
        ),
        "bold over one textblock is one AddMark over its whole text extent: {:?}",
        transaction.operations
    );
    assert_eq!(
        apply_mark_command(json!([break_line(&plain)]), None, toggle_bold()),
        json!([break_line(&|text| marked(text, bold()))]),
        "bold over cd<br>ef"
    );
    assert_eq!(
        apply_mark_command(
            json!([paragraph(json!([plain("ab")])), break_line(&plain)]),
            None,
            toggle_bold(),
        ),
        json!([
            paragraph(json!([marked("ab", bold())])),
            break_line(&|text| marked(text, bold()))
        ]),
        "bold over two blocks where one contains a hard break"
    );
}
