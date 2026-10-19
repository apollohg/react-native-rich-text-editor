const PASTE_ROOT_WINDOW_REQUEST_ID: u64 = 70_034_000;
const PASTE_SCALING_SMALL_PARAGRAPHS: usize = 8;
const PASTE_SCALING_LARGE_PARAGRAPHS: usize = 512;

fn plain_text_paste(text: &str) -> TypedCommand {
    TypedCommand::Paste {
        fragment: None,
        html: None,
        text: Some(text.into()),
        plain_text: false,
        allow_base64_images: false,
        input_filter: None,
        cell_drop: None,
    }
}

fn paste_root_window_corpus() -> Vec<(&'static str, serde_json::Value)> {
    let text = |text: &str| json!({ "type": "text", "text": text });
    let marked = |text: &str, marks: serde_json::Value| {
        json!({ "type": "text", "text": text, "marks": marks })
    };
    let paragraph = |content: serde_json::Value| json!({ "type": "paragraph", "content": content });
    vec![
        (
            "markedParagraphs",
            json!({ "type": "doc", "content": [
                paragraph(json!([text("lead "), marked("bold", json!([{ "type": "bold" }])), text(" tail")])),
                paragraph(json!([marked("linked", json!([{ "type": "link", "attrs": { "href": "https://example.com/1" } }])), text(" plain 😀")])),
                { "type": "h2", "content": [text("Heading")] },
                paragraph(json!([text("last")])),
            ]}),
        ),
        (
            "nestedAndVoids",
            json!({ "type": "doc", "content": [
                paragraph(json!([text("before")])),
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [paragraph(json!([text("item")]))] },
                ]},
                paragraph(json!([text("line"), { "type": "hardBreak" }, marked("code", json!([{ "type": "code" }]))])),
                { "type": "horizontalRule" },
                paragraph(json!([text("after")])),
            ]}),
        ),
    ]
}

fn paste_root_window_texts() -> [&'static str; 4] {
    [
        "single line",
        "first\nsecond",
        "a\n\nb\nc",
        "e\u{301} 東京\n😀 line\n",
    ]
}

fn paste_selections(engine: &YrsDocumentEngine) -> Vec<(u32, u32)> {
    let map = &engine.derived_state.as_ref().unwrap().position_map;
    let mut selections = Vec::new();
    for index in 0..map.block_count() {
        let block = map.block(index).unwrap();
        if block.is_void_block {
            continue;
        }
        let start = block.scalar_start + block.scalar_prefix_len;
        let end = start + block.scalar_len;
        selections.push((start, start));
        selections.push((end, end));
        if block.scalar_len > 2 {
            selections.push((start + 1, start + 1));
            selections.push((start + 1, end - 1));
        }
        if index + 1 < map.block_count() {
            let next = map.block(index + 1).unwrap();
            selections.push((start + 1, next.scalar_start + next.scalar_prefix_len + 1));
        }
    }
    selections
}

fn is_root_window_paste(engine: &mut YrsDocumentEngine, request_id: u64, text: &str) -> bool {
    matches!(
        engine.plan_command(request_id, plain_text_paste(text)),
        Ok(CommandPlan::Transaction(transaction))
            if matches!(
                transaction.operations.as_slice(),
                [TypedOperation::ReplaceStructure(replacement)] if replacement.parent_path().is_empty()
            )
    )
}

#[test]
fn plain_text_paste_root_windows_match_eager_lowering_and_full_rebuild() {
    use crate::yrs_engine::mutation::{
        force_eager_mutation_lowering_for_test, reset_root_window_lowering_counts_for_test,
        take_root_window_lowering_counts_for_test,
    };

    let mut root_window_pastes = 0usize;
    for (name, document) in paste_root_window_corpus() {
        let selections = paste_selections(&localized_mark_engine(&document, 0, 0));
        for (anchor, head) in selections {
            for text in paste_root_window_texts() {
                let case = format!("{name} {anchor}..{head} {text:?}");
                let mut localized = localized_mark_engine(&document, anchor, head);
                let mut eager = localized_mark_engine(&document, anchor, head);
                let root_window = is_root_window_paste(
                    &mut localized_mark_engine(&document, anchor, head),
                    PASTE_ROOT_WINDOW_REQUEST_ID,
                    text,
                );

                reset_root_window_lowering_counts_for_test();
                let eager_result = {
                    let _eager_lowering = force_eager_mutation_lowering_for_test();
                    eager.apply_command(PASTE_ROOT_WINDOW_REQUEST_ID + 1, plain_text_paste(text))
                };
                let (_, _, eager_hits, ..) = take_root_window_lowering_counts_for_test();
                let localized_result =
                    localized.apply_command(PASTE_ROOT_WINDOW_REQUEST_ID + 1, plain_text_paste(text));
                let (eager_text_scans, eager_parent_scans, localized_hits, ..) =
                    take_root_window_lowering_counts_for_test();

                assert_eq!(eager_hits, 0, "{case}: the reference must lower eagerly");
                if root_window {
                    root_window_pastes += 1;
                    assert_eq!(
                        (localized_hits, eager_text_scans, eager_parent_scans),
                        (1, 0, 0),
                        "{case}: a root-window paste must lower without a whole-document Yrs scan"
                    );
                }
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
                assert_derived_state_matches_full_rebuild(&localized, &case);
                assert_localized_index_presence_matches(&localized, &eager, &case);
                if !matches!(&localized_result, Ok(Some(result)) if result.changed) {
                    continue;
                }
                assert_eq!(
                    localized.undo(PASTE_ROOT_WINDOW_REQUEST_ID + 2).unwrap(),
                    eager.undo(PASTE_ROOT_WINDOW_REQUEST_ID + 2).unwrap(),
                    "{case}: undo"
                );
                assert_eq!(localized.document_json(), eager.document_json(), "{case}: undo");
                assert_derived_state_matches_full_rebuild(&localized, &format!("{case} undo"));
                assert_eq!(
                    localized.redo(PASTE_ROOT_WINDOW_REQUEST_ID + 3).unwrap(),
                    eager.redo(PASTE_ROOT_WINDOW_REQUEST_ID + 3).unwrap(),
                    "{case}: redo"
                );
                assert_eq!(localized.document_json(), eager.document_json(), "{case}: redo");
                assert_derived_state_matches_full_rebuild(&localized, &format!("{case} redo"));
            }
        }
    }
    assert!(
        root_window_pastes > 0,
        "the corpus must exercise root-window paste lowering"
    );
}

fn middle_paragraph_paste_text_materializations(paragraphs: usize, forced_eager: bool) -> usize {
    use crate::yrs_engine::mutation::{
        force_eager_mutation_lowering_for_test, reset_materialized_yrs_text_count_for_test,
        reset_root_window_lowering_counts_for_test, take_materialized_yrs_text_count_for_test,
        take_root_window_lowering_counts_for_test,
    };

    const BODY: &str = "Native editing preserves each word while the document grows. ";
    let document = json!({
        "type": "doc",
        "content": (0..paragraphs).map(|index| json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": format!("Paragraph {index:04}: {BODY}") },
                { "type": "text", "text": "bold emphasis", "marks": [{ "type": "bold" }] }
            ]
        })).collect::<Vec<_>>()
    });
    let probe = localized_mark_engine(&document, 0, 0);
    let block = probe
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .block(paragraphs / 2)
        .unwrap()
        .into_owned();
    let caret = block.scalar_start + block.scalar_prefix_len + 10;
    let mut engine = localized_mark_engine(&document, caret, caret);
    hydrate_import_for_compile_test(&mut engine);
    let CommandPlan::Transaction(transaction) = engine
        .plan_command(
            PASTE_ROOT_WINDOW_REQUEST_ID + 10,
            plain_text_paste("pasted first\npasted second\npasted third"),
        )
        .unwrap()
    else {
        panic!("{paragraphs} paragraphs: a multi-line paste plans a transaction");
    };
    reset_root_window_lowering_counts_for_test();
    reset_materialized_yrs_text_count_for_test();
    let compiled = if forced_eager {
        let _eager_lowering = force_eager_mutation_lowering_for_test();
        engine.compile_typed_transaction(transaction)
    } else {
        engine.compile_typed_transaction(transaction)
    };
    compiled.unwrap();
    let materialized = take_materialized_yrs_text_count_for_test();
    let (_, _, localized_hits, ..) = take_root_window_lowering_counts_for_test();
    assert_eq!(
        localized_hits,
        usize::from(!forced_eager),
        "{paragraphs} paragraphs, forced eager {forced_eager}: root-window lowering"
    );
    materialized
}

#[test]
fn root_window_paste_lowering_reads_no_yrs_text_at_any_document_length() {
    let eager_small = middle_paragraph_paste_text_materializations(PASTE_SCALING_SMALL_PARAGRAPHS, true);
    let eager_large = middle_paragraph_paste_text_materializations(PASTE_SCALING_LARGE_PARAGRAPHS, true);
    let localized_small =
        middle_paragraph_paste_text_materializations(PASTE_SCALING_SMALL_PARAGRAPHS, false);
    let localized_large =
        middle_paragraph_paste_text_materializations(PASTE_SCALING_LARGE_PARAGRAPHS, false);
    assert!(
        eager_small < eager_large,
        "eager lowering materializes every Yrs text: {eager_small} vs {eager_large}"
    );
    assert_eq!(
        (localized_small, localized_large),
        (0, 0),
        "root-window paste lowering must not materialize Yrs text"
    );
}
