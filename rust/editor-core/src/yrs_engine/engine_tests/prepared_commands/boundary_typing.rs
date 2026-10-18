const BOUNDARY_TYPING_REQUEST_ID: u64 = 70_033_000;
const BOUNDARY_TYPED_TEXT: &str = "x";

#[derive(Debug, Clone)]
enum BoundaryTyping {
    InsertCommand,
    StoredMarkToggle(&'static str),
    ExplicitMarks(Vec<Mark>),
}

fn boundary_typing_corpus() -> Vec<(&'static str, serde_json::Value)> {
    let text = |text: &str| json!({ "type": "text", "text": text });
    let marked = |text: &str, marks: serde_json::Value| {
        json!({ "type": "text", "text": text, "marks": marks })
    };
    let link = |href: &str| json!([{ "type": "link", "attrs": { "href": href } }]);
    let paragraph = |content: serde_json::Value| json!({ "type": "paragraph", "content": content });
    vec![
        (
            "boldPlainItalic",
            json!({ "type": "doc", "content": [
                paragraph(json!([marked("bold", json!([{ "type": "bold" }])), text(" plain "), marked("ital", json!([{ "type": "italic" }]))])),
            ]}),
        ),
        (
            "adjacentLinksWithDifferentAttrs",
            json!({ "type": "doc", "content": [
                paragraph(json!([marked("first", link("https://example.com/a")), marked("second", link("https://example.com/b")), text(" tail")])),
            ]}),
        ),
        (
            "codeAndStackedMarks",
            json!({ "type": "doc", "content": [
                paragraph(json!([marked("code", json!([{ "type": "code" }])), text(" mid "), marked("both", json!([{ "type": "bold" }, { "type": "italic" }])), marked("bold", json!([{ "type": "bold" }]))])),
            ]}),
        ),
        (
            "unicodeAroundStrike",
            json!({ "type": "doc", "content": [
                paragraph(json!([text("e\u{301}😀 "), marked("gone", json!([{ "type": "strike" }])), text("!")])),
                paragraph(json!([marked("underlined", json!([{ "type": "underline" }])), text("東京")])),
            ]}),
        ),
    ]
}

fn boundary_typing_modes() -> Vec<BoundaryTyping> {
    vec![
        BoundaryTyping::InsertCommand,
        BoundaryTyping::StoredMarkToggle("bold"),
        BoundaryTyping::StoredMarkToggle("italic"),
        BoundaryTyping::StoredMarkToggle("code"),
        BoundaryTyping::ExplicitMarks(Vec::new()),
        BoundaryTyping::ExplicitMarks(vec![Mark::new("bold".into(), HashMap::new())]),
    ]
}

fn run_edges(document: &serde_json::Value, engine: &YrsDocumentEngine) -> Vec<u32> {
    let map = &engine.derived_state.as_ref().unwrap().position_map;
    let mut edges = Vec::new();
    for (index, paragraph) in document["content"].as_array().unwrap().iter().enumerate() {
        let block = map.block(index).unwrap();
        let mut edge = block.scalar_start + block.scalar_prefix_len;
        edges.push(edge);
        for run in paragraph["content"].as_array().unwrap() {
            edge += u32::try_from(run["text"].as_str().unwrap().chars().count()).unwrap();
            edges.push(edge);
        }
    }
    edges
}

fn type_at_caret(
    engine: &mut YrsDocumentEngine,
    caret: u32,
    typing: &BoundaryTyping,
) -> crate::yrs_engine::OperationResult<Option<crate::yrs_engine::TypedTransactionResult>> {
    match typing {
        BoundaryTyping::InsertCommand => engine.apply_command(
            BOUNDARY_TYPING_REQUEST_ID + 2,
            TypedCommand::InsertText {
                text: BOUNDARY_TYPED_TEXT.into(),
            },
        ),
        BoundaryTyping::StoredMarkToggle(mark_type) => {
            engine.apply_command(
                BOUNDARY_TYPING_REQUEST_ID + 1,
                TypedCommand::ToggleMark {
                    mark_type: (*mark_type).into(),
                },
            )?;
            engine.apply_command(
                BOUNDARY_TYPING_REQUEST_ID + 2,
                TypedCommand::InsertText {
                    text: BOUNDARY_TYPED_TEXT.into(),
                },
            )
        }
        BoundaryTyping::ExplicitMarks(marks) => engine
            .apply_typed_transaction_with_result(TypedTransaction {
                request_id: BOUNDARY_TYPING_REQUEST_ID + 2,
                base_document_revision: engine.revision(),
                origin: TransactionOrigin::LocalInput,
                operations: vec![TypedOperation::InsertText {
                    at: RevisionedPosition {
                        offset: caret,
                        kind: EditorOffsetKind::Scalar,
                        affinity: Affinity::After,
                    },
                    text: BOUNDARY_TYPED_TEXT.into(),
                    marks: marks.clone(),
                }],
                selection_intent: SelectionIntent::UseOperationResult,
                history_policy: HistoryPolicy::Auto,
            })
            .map(Some),
    }
}

fn eager_typing_at_caret(
    engine: &mut YrsDocumentEngine,
    caret: u32,
    typing: &BoundaryTyping,
) -> crate::yrs_engine::OperationResult<Option<crate::yrs_engine::TypedTransactionResult>> {
    use crate::yrs_engine::mutation::force_eager_mutation_lowering_for_test;
    let _eager_lowering = force_eager_mutation_lowering_for_test();
    type_at_caret(engine, caret, typing)
}

#[test]
fn typing_at_run_edges_matches_eager_lowering_and_full_rebuild() {
    use crate::yrs_engine::mutation::{
        reset_localized_lookup_counts_for_test, take_localized_lookup_counts_for_test,
    };

    for (name, document) in boundary_typing_corpus() {
        let edges = run_edges(&document, &localized_mark_engine(&document, 0, 0));
        for caret in edges {
            for typing in boundary_typing_modes() {
                let case = format!("{name} caret {caret} {typing:?}");
                let mut localized = localized_mark_engine(&document, caret, caret);
                let mut eager = localized_mark_engine(&document, caret, caret);

                reset_localized_lookup_counts_for_test();
                let eager_result = eager_typing_at_caret(&mut eager, caret, &typing);
                let (_, eager_hits, _) = take_localized_lookup_counts_for_test();
                let localized_result = type_at_caret(&mut localized, caret, &typing);
                let (_, localized_hits, _) = take_localized_lookup_counts_for_test();

                assert_eq!(eager_hits, 0, "{case}: the reference must lower eagerly");
                assert_eq!(
                    localized_hits, 1,
                    "{case}: typing at a run edge must lower through the localized textblock"
                );
                assert_eq!(localized_result, eager_result, "{case}: result");
                assert_eq!(localized.document_json(), eager.document_json(), "{case}: json");
                assert_eq!(localized.document_html(), eager.document_html(), "{case}: html");
                assert_eq!(
                    localized.resolved_selection(),
                    eager.resolved_selection(),
                    "{case}: selection"
                );
                assert_eq!(localized.stored_marks(), eager.stored_marks(), "{case}: stored");
                assert_eq!(localized.can_undo(), eager.can_undo(), "{case}: can undo");
                assert_eq!(localized.can_redo(), eager.can_redo(), "{case}: can redo");
                assert_derived_state_matches_full_rebuild(&localized, &case);
                assert_localized_index_presence_matches(&localized, &eager, &case);

                assert_eq!(
                    localized.undo(BOUNDARY_TYPING_REQUEST_ID + 3).unwrap(),
                    eager.undo(BOUNDARY_TYPING_REQUEST_ID + 3).unwrap(),
                    "{case}: undo"
                );
                assert_eq!(localized.document_json(), eager.document_json(), "{case}: undo");
                assert_derived_state_matches_full_rebuild(&localized, &format!("{case} undo"));
                assert_eq!(
                    localized.redo(BOUNDARY_TYPING_REQUEST_ID + 4).unwrap(),
                    eager.redo(BOUNDARY_TYPING_REQUEST_ID + 4).unwrap(),
                    "{case}: redo"
                );
                assert_eq!(localized.document_json(), eager.document_json(), "{case}: redo");
                assert_derived_state_matches_full_rebuild(&localized, &format!("{case} redo"));
            }
        }
    }
}

#[test]
fn a_typing_streak_after_a_marked_run_stays_localized() {
    use crate::yrs_engine::mutation::{
        reset_localized_lookup_counts_for_test, take_localized_lookup_counts_for_test,
    };
    const STREAK: u32 = 4;

    let (_, document) = boundary_typing_corpus().remove(0);
    let run_end = run_edges(&document, &localized_mark_engine(&document, 0, 0))[1];
    let mut localized = localized_mark_engine(&document, run_end, run_end);
    let mut eager = localized_mark_engine(&document, run_end, run_end);
    for step in 0..STREAK {
        let case = format!("keystroke {step} after the bold run");
        reset_localized_lookup_counts_for_test();
        let localized_result =
            type_at_caret(&mut localized, run_end + step, &BoundaryTyping::InsertCommand);
        let (lookup_builds, localized_hits, _) = take_localized_lookup_counts_for_test();
        let eager_result =
            eager_typing_at_caret(&mut eager, run_end + step, &BoundaryTyping::InsertCommand);
        assert_eq!(
            (lookup_builds, localized_hits),
            (0, 1),
            "{case}: no lookup rebuild and one localized lowering"
        );
        assert_eq!(localized_result, eager_result, "{case}: result");
        assert_eq!(localized.document_json(), eager.document_json(), "{case}: json");
        assert_derived_state_matches_full_rebuild(&localized, &case);
    }
}
