use super::native_frame::build_native_frame;
use super::types::FfiTableFrame;
use crate::native_transaction_bridge::{NativeTransactionBridge, NATIVE_BRIDGE_ENVELOPE_VERSION};
use crate::session::EditorSession;
use crate::test_support::large_table_fixture::{
    session_with_document, session_with_schema_and_document, two_table_document,
};
use serde_json::{json, Value};

const OWNER: u64 = 16_001;
const EDITOR: &str = "16000";
const FIRST_REQUEST: u64 = 16_002;
const SELECTION_STATE_KEY: &str = "selectionState";
const FRAME_STATE_KEYS: [&str; 7] = [
    "activeState",
    "documentIsEmpty",
    "documentVersion",
    "historyState",
    "scalarLength",
    "selection",
    "stateRevision",
];
const RANGE_WIDTHS: [u32; 3] = [1, 4, 9];
const INSERTED_TEXT: &str = "typed🦀";

struct NativeView {
    session: EditorSession,
    epoch: String,
    next_request: u64,
}

impl NativeView {
    fn published(document: &Value) -> Self {
        Self::published_session(session_with_document(document))
    }

    fn published_session(session: EditorSession) -> Self {
        let mut view = Self {
            session,
            epoch: String::new(),
            next_request: FIRST_REQUEST,
        };
        view.publish();
        view
    }

    fn unpublished(document: &Value) -> Self {
        let mut session = session_with_document(document);
        let epoch = session
            .pin_position_epoch(OWNER, session.engine.revision())
            .unwrap()
            .to_string();
        Self {
            session,
            epoch,
            next_request: FIRST_REQUEST,
        }
    }

    fn publish(&mut self) -> (Value, FfiTableFrame) {
        let frame = build_native_frame(&mut self.session, EDITOR, Some(OWNER), None).unwrap();
        self.epoch = frame.position_epoch.expect("an owned frame pins its epoch");
        (
            serde_json::from_str(&frame.snapshot_json).unwrap(),
            frame.tables,
        )
    }

    fn submit_as(&mut self, request: u64, intent: Value) -> Value {
        let envelope = json!({
            "version": NATIVE_BRIDGE_ENVELOPE_VERSION,
            "requestId": request.to_string(),
            "ownerId": OWNER.to_string(),
            "positionEpoch": self.epoch,
            "intent": intent,
        })
        .to_string();
        let outcome = NativeTransactionBridge::new(&mut self.session)
            .submit_native_intent(&envelope)
            .unwrap_or_else(|error| panic!("intent {intent} failed: {error:?}"));
        serde_json::from_str(&outcome).unwrap()
    }

    fn submit(&mut self, intent: Value) -> Value {
        let request = self.next_request;
        self.next_request += 1;
        self.submit_as(request, intent)
    }

    fn select(&mut self, anchor: u32, head: u32) -> Value {
        self.submit(json!({ "type": "setSelection", "anchor": anchor, "head": head }))
    }

    fn scalar_length(&self) -> u32 {
        self.session.engine.position_map().unwrap().total_scalars()
    }
}

fn frame_state((snapshot, tables): &(Value, FfiTableFrame)) -> Value {
    assert!(
        tables.attributes.is_empty()
            && tables.removed_attribute_keys.is_empty()
            && tables.tables.is_empty()
            && tables.removed_table_keys.is_empty()
            && tables.cell_updates.is_empty(),
        "a selection-only frame must not change tables: {tables:?}"
    );
    assert_eq!(
        snapshot["renderPatch"]["deleteCount"], 0,
        "a selection-only frame must not patch the render: {snapshot}"
    );
    assert_eq!(
        snapshot["renderPatch"]["renderBlocks"],
        json!([]),
        "a selection-only frame must not publish render blocks: {snapshot}"
    );
    Value::Object(
        FRAME_STATE_KEYS
            .iter()
            .map(|key| ((*key).to_owned(), snapshot[*key].clone()))
            .collect(),
    )
}

fn without_selection_state(outcome: &Value) -> Value {
    let mut outcome = outcome.clone();
    outcome.as_object_mut().unwrap().remove(SELECTION_STATE_KEY);
    outcome
}

fn marked_prose_document() -> Value {
    json!({
        "type": "doc",
        "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "plain " },
                { "type": "text", "text": "bold", "marks": [{ "type": "bold" }] },
                { "type": "text", "text": " link", "marks": [
                    { "type": "italic" },
                    { "type": "link", "attrs": { "href": "https://example.com" } }
                ] },
                { "type": "hard_break" },
                { "type": "text", "text": "after 🦀 break" }
            ] },
            { "type": "heading", "attrs": { "level": 2 }, "content": [
                { "type": "text", "text": "Heading" }
            ] },
            { "type": "bullet_list", "content": [
                { "type": "list_item", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "first item" }] }
                ] },
                { "type": "list_item", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "second" }] }
                ] }
            ] },
            { "type": "blockquote", "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "quoted" }] }
            ] },
            { "type": "paragraph" }
        ]
    })
}

fn selections(scalar_length: u32) -> Vec<(u32, u32)> {
    let mut selections: Vec<(u32, u32)> = (0..=scalar_length).map(|at| (at, at)).collect();
    for width in RANGE_WIDTHS {
        for start in 0..=scalar_length.saturating_sub(width) {
            selections.push((start, start + width));
            selections.push((start + width, start));
        }
    }
    selections.push((0, scalar_length));
    selections
}

#[test]
fn selection_only_intents_return_exactly_the_state_their_full_frame_publishes() {
    for (name, document) in [
        ("marked prose", marked_prose_document()),
        ("two tables", two_table_document()),
    ] {
        let mut view = NativeView::published(&document);
        let scalar_length = view.scalar_length();
        let mut compared = 0;
        for (anchor, head) in selections(scalar_length) {
            let outcome = view.select(anchor, head);
            let selection_state = outcome
                .get(SELECTION_STATE_KEY)
                .unwrap_or_else(|| {
                    panic!("{name} {anchor}..{head}: no selection state in {outcome}")
                })
                .clone();
            let full_frame = view.publish();
            assert_eq!(
                selection_state,
                frame_state(&full_frame),
                "{name} {anchor}..{head}: intent state differs from the full frame"
            );
            compared += 1;
        }
        assert!(
            compared > scalar_length as usize,
            "{name}: compared {compared} selections"
        );
    }
}

#[test]
fn a_view_that_skips_selection_frames_stays_equivalent_to_one_that_publishes_them() {
    let document = marked_prose_document();
    let mut skipping = NativeView::published(&document);
    let mut publishing = NativeView::published(&document);
    let scalar_length = skipping.scalar_length();
    let script = [
        (0, 0),
        (8, 8),
        (7, 15),
        (scalar_length, 3),
        (scalar_length / 2, scalar_length / 2),
    ];
    for (step, (anchor, head)) in script.into_iter().enumerate() {
        let skipped = skipping.select(anchor, head);
        let published = publishing.select(anchor, head);
        let published_frame = publishing.publish();
        assert_eq!(skipped, published, "step {step}: intent outcomes differ");
        assert_eq!(
            skipped[SELECTION_STATE_KEY],
            frame_state(&published_frame),
            "step {step}: skipped state differs from the published frame"
        );
    }
    let caret = scalar_length / 2;
    let insert =
        json!({ "type": "insertText", "anchor": caret, "head": caret, "text": INSERTED_TEXT });
    let skipped_insert = skipping.submit(insert.clone());
    let published_insert = publishing.submit(insert);
    assert_eq!(
        skipped_insert, published_insert,
        "an edit through the epoch kept across skipped frames must match the republished epoch"
    );
    assert_eq!(skipped_insert["positionFallback"], false);
    assert!(skipped_insert.get(SELECTION_STATE_KEY).is_none());
    assert_eq!(
        skipping.publish(),
        publishing.publish(),
        "the post-edit frames must be identical"
    );
    let after_edit = caret + INSERTED_TEXT.chars().count() as u32;
    let skipped = skipping.select(after_edit, 0);
    let published = publishing.select(after_edit, 0);
    assert_eq!(skipped, published);
    assert_eq!(
        skipped[SELECTION_STATE_KEY]["historyState"]["canUndo"],
        true
    );
    assert_eq!(
        skipped[SELECTION_STATE_KEY],
        frame_state(&publishing.publish()),
        "the post-edit selection state must equal the full frame"
    );
    assert_eq!(
        skipping.session.engine.document_json(),
        publishing.session.engine.document_json()
    );
}

#[test]
fn selection_state_is_withheld_whenever_the_owner_needs_a_full_frame() {
    let document = marked_prose_document();

    let mut never_published = NativeView::unpublished(&document);
    let outcome = never_published.select(1, 1);
    assert!(
        outcome.get(SELECTION_STATE_KEY).is_none(),
        "an owner without a published frame has no render to reuse: {outcome}"
    );

    let mut view = NativeView::published(&document);
    let edit =
        view.submit(json!({ "type": "insertText", "anchor": 2, "head": 2, "text": INSERTED_TEXT }));
    assert_eq!(edit["documentChanged"], true);
    assert!(
        edit.get(SELECTION_STATE_KEY).is_none(),
        "document edits always need a frame: {edit}"
    );

    let unpublished_edit = view.select(1, 1);
    assert!(
        unpublished_edit.get(SELECTION_STATE_KEY).is_none(),
        "a selection after an unpublished edit must not skip the render: {unpublished_edit}"
    );

    view.publish();
    let request = view.next_request;
    let current = view.select(3, 5);
    assert!(
        current.get(SELECTION_STATE_KEY).is_some(),
        "a selection on the published revision carries its state: {current}"
    );
    let replayed = view.submit_as(
        request,
        json!({ "type": "setSelection", "anchor": 3, "head": 5 }),
    );
    assert_eq!(
        replayed,
        without_selection_state(&current),
        "an idempotent replay returns the retained outcome without stale selection state"
    );
}

const TABLE_CELL_NODE: &str = "table_cell";

fn caret_in_table_cell(view: &NativeView) -> u32 {
    let document = view.session.engine.document().unwrap();
    let map = view.session.engine.position_map().unwrap();
    (0..=view.scalar_length())
        .find(|scalar| {
            let resolved = document
                .resolve(map.scalar_to_doc(*scalar, document))
                .unwrap();
            let mut node = document.root();
            resolved.node_path.iter().any(|index| {
                node = node.child(*index as usize).unwrap();
                node.node_type() == TABLE_CELL_NODE
            })
        })
        .expect("the two-table fixture has a caret inside a table cell")
}

fn assert_keystroke_accepts_the_kept_epoch(view: &mut NativeView, caret: u32, context: &str) {
    let kept_epoch = view.epoch.clone();
    let outcome = view.select(caret, caret);
    assert!(
        outcome.get(SELECTION_STATE_KEY).is_some(),
        "{context}: the selection must take the fast path: {outcome}"
    );
    assert_eq!(
        view.epoch, kept_epoch,
        "{context}: no frame re-pinned the epoch"
    );
    let document_before = view.session.engine.document_json();
    let keystroke = view.submit(json!({
        "type": "insertText", "anchor": caret, "head": caret, "text": INSERTED_TEXT
    }));
    assert_eq!(
        keystroke["positionFallback"], false,
        "{context}: the kept epoch must resolve without remapping: {keystroke}"
    );
    assert_eq!(keystroke["documentChanged"], true, "{context}: {keystroke}");
    assert_ne!(
        view.session.engine.document_json(),
        document_before,
        "{context}"
    );
    let (frame, _) = view.publish();
    let inserted_head = caret + INSERTED_TEXT.chars().count() as u32;
    assert_eq!(
        frame["selection"]["headScalar"], inserted_head,
        "{context}: the keystroke landed at the selected caret"
    );
}

#[test]
fn the_pinned_epoch_stays_valid_for_the_next_keystroke_after_a_selection_only_intent() {
    let mut text_view = NativeView::published(&marked_prose_document());
    let text_caret = text_view.scalar_length() / 2;
    assert_keystroke_accepts_the_kept_epoch(&mut text_view, text_caret, "text selection");

    let mut cell_view = NativeView::published(&two_table_document());
    let cell_caret = caret_in_table_cell(&cell_view);
    assert_keystroke_accepts_the_kept_epoch(&mut cell_view, cell_caret, "table cell selection");
}

const DIVIDER_NODE: &str = "divider";
const DIVIDED_ROOT_CONTENT: &str = "paragraph+ (divider paragraph+)*";

fn divided_schema() -> crate::schema::Schema {
    crate::schema::Schema::from_json(&json!({
        "nodes": [
            { "name": "doc", "content": DIVIDED_ROOT_CONTENT, "role": "doc" },
            { "name": "paragraph", "content": "text*", "group": "block", "role": "textBlock", "htmlTag": "p" },
            { "name": DIVIDER_NODE, "content": "", "group": "block", "role": "block", "isVoid": true, "htmlTag": "hr" },
            { "name": "text", "content": "", "role": "text" }
        ],
        "marks": []
    }))
    .unwrap()
}

fn divided_document() -> Value {
    json!({
        "type": "doc",
        "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "first" }] },
            { "type": DIVIDER_NODE },
            { "type": "paragraph", "content": [{ "type": "text", "text": "second" }] }
        ]
    })
}

fn root_block_count(view: &NativeView) -> usize {
    view.session.engine.document_json().unwrap()["content"]
        .as_array()
        .unwrap()
        .len()
}

fn divider_insertable(state: &Value) -> bool {
    state["activeState"]["insertableNodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node == DIVIDER_NODE)
}

fn assert_move_matches_a_fresh_session(
    view: &mut NativeView,
    caret: u32,
    divider_expected: bool,
    context: &str,
) {
    let moved = view.select(caret, caret)[SELECTION_STATE_KEY].clone();
    let document = view.session.engine.document_json().unwrap();
    let mut fresh = NativeView::published_session(session_with_schema_and_document(
        divided_schema(),
        &document,
    ));
    let reference = fresh.select(caret, caret)[SELECTION_STATE_KEY].clone();
    assert_eq!(
        moved["activeState"], reference["activeState"],
        "{context}: the edited session must agree with a fresh load of {document}"
    );
    assert_eq!(moved["selection"], reference["selection"], "{context}");
    assert_eq!(
        divider_insertable(&moved),
        divider_expected,
        "{context}: divider insertability in {document}"
    );
}

#[test]
fn insertable_nodes_follow_sibling_structure_edits_between_selection_moves() {
    let mut view = NativeView::published_session(session_with_schema_and_document(
        divided_schema(),
        &divided_document(),
    ));
    let first = 2;
    let second_middle = "first".len() as u32 + 4;
    assert_move_matches_a_fresh_session(&mut view, first, false, "before the divider");
    assert_move_matches_a_fresh_session(&mut view, second_middle, false, "in the last block");

    let split = view
        .submit(json!({ "type": "splitBlock", "anchor": second_middle, "head": second_middle }));
    assert_eq!(split["documentChanged"], true, "{split}");
    assert_eq!(
        root_block_count(&view),
        4,
        "the split adds a sibling paragraph"
    );
    view.publish();
    assert_move_matches_a_fresh_session(
        &mut view,
        second_middle - 1,
        true,
        "before a split sibling",
    );
    assert_move_matches_a_fresh_session(
        &mut view,
        second_middle + 2,
        false,
        "in the new last block",
    );
    assert_move_matches_a_fresh_session(&mut view, first, false, "still before the divider");

    let merge = view.submit(
        json!({ "type": "deleteBackward", "anchor": second_middle + 1, "head": second_middle + 1 }),
    );
    assert_eq!(merge["documentChanged"], true, "{merge}");
    assert_eq!(
        root_block_count(&view),
        3,
        "the merge removes the sibling paragraph"
    );
    view.publish();
    assert_move_matches_a_fresh_session(
        &mut view,
        second_middle - 1,
        false,
        "after merging the split back",
    );
}

const MIXED_WIDTH_PARAGRAPH: &str = "東京 😀 café e\u{301} 👩\u{200d}💻 mixed width text";
const MIXED_WIDTH_PARAGRAPHS: usize = 12;
const MIXED_WIDTH_INSERT: &str = "字😀";

fn mixed_width_document() -> Value {
    json!({
        "type": "doc",
        "content": (0..MIXED_WIDTH_PARAGRAPHS)
            .map(|index| json!({ "type": "paragraph", "content": [
                { "type": "text", "text": format!("{index}: {MIXED_WIDTH_PARAGRAPH}") }
            ] }))
            .collect::<Vec<_>>()
    })
}

fn character_walk_utf16(text: &str, scalar: u32) -> Option<u32> {
    let mut characters = text.chars();
    let mut utf16 = 0u32;
    for _ in 0..scalar {
        utf16 += characters.next()?.len_utf16() as u32;
    }
    Some(utf16)
}

fn assert_resolved_utf16_matches_a_character_walk(view: &NativeView, context: &str) {
    let engine = &view.session.engine;
    let rendered = crate::render::rendered_text(engine.document().unwrap(), engine.schema());
    let points = match engine.resolved_selection().unwrap() {
        crate::yrs_engine::ResolvedSelection::Text { anchor, head } => {
            vec![anchor.clone(), head.clone()]
        }
        other => panic!("{context}: expected a text selection, got {other:?}"),
    };
    for point in points {
        assert_eq!(
            Some(point.utf16),
            character_walk_utf16(&rendered, point.scalar),
            "{context}: UTF-16 of scalar {} after CJK and astral text",
            point.scalar
        );
    }
}

#[test]
fn edits_after_cjk_and_astral_text_resolve_utf16_like_a_character_walk() {
    let mut view = NativeView::published(&mixed_width_document());
    let paragraph_scalars = MIXED_WIDTH_PARAGRAPH.chars().count() as u32;
    for caret in [
        paragraph_scalars,
        view.scalar_length() / 2,
        view.scalar_length() - 3,
    ] {
        view.select(caret, caret);
        assert_resolved_utf16_matches_a_character_walk(&view, &format!("select {caret}"));
        view.submit(json!({ "type": "insertText", "anchor": caret, "head": caret, "text": MIXED_WIDTH_INSERT }));
        view.publish();
        assert_resolved_utf16_matches_a_character_walk(&view, &format!("insert at {caret}"));
        let after = caret + MIXED_WIDTH_INSERT.chars().count() as u32;
        view.submit(json!({ "type": "deleteBackward", "anchor": after, "head": after }));
        view.publish();
        assert_resolved_utf16_matches_a_character_walk(&view, &format!("delete at {after}"));
        view.submit(
            json!({ "type": "command", "anchor": caret - 4, "head": caret,
            "command": { "type": "toggleMark", "markType": "bold" } }),
        );
        view.publish();
        assert_resolved_utf16_matches_a_character_walk(&view, &format!("bold before {caret}"));
    }
}

#[test]
fn selection_points_planned_through_the_block_index_match_the_fragment_walk() {
    use crate::yrs_engine::revisioned_position_to_relative_point;
    use crate::yrs_engine::{Affinity, EditorOffsetKind, RevisionedPosition};

    for (name, document) in [
        ("marked prose", marked_prose_document()),
        ("mixed width", mixed_width_document()),
        ("two tables", two_table_document()),
        (
            "multi-paragraph cell",
            crate::test_support::large_table_fixture::multi_paragraph_cell_document(),
        ),
    ] {
        let session = session_with_document(&document);
        let engine = &session.engine;
        let model = engine.document().unwrap();
        let position_map = engine.position_map().unwrap();
        let block_branch_index = engine.block_branch_index().unwrap();
        let rendered = crate::render::rendered_text(model, engine.schema());
        let utf16_length = rendered.encode_utf16().count() as u32;
        let mut indexed_positions = 0;
        engine
            .read_fragment_for_test(|txn, fragment| {
                for (kind, length) in [
                    (EditorOffsetKind::Scalar, position_map.total_scalars()),
                    (EditorOffsetKind::Utf16, utf16_length),
                ] {
                    for offset in 0..=length {
                        for affinity in [Affinity::Before, Affinity::After] {
                            let position = RevisionedPosition {
                                offset,
                                kind,
                                affinity,
                            };
                            let resolve = |index| {
                                revisioned_position_to_relative_point(
                                    txn,
                                    fragment,
                                    position,
                                    &rendered,
                                    position_map,
                                    model,
                                    engine.schema(),
                                    index,
                                )
                            };
                            let walked = resolve(None);
                            assert_eq!(
                                resolve(Some(block_branch_index)),
                                walked,
                                "{name}: {kind:?} offset {offset} with {affinity:?}"
                            );
                            indexed_positions += usize::from(walked.is_some());
                        }
                    }
                }
            })
            .unwrap();
        assert!(indexed_positions > 0, "{name}: no position resolved");
    }
}
