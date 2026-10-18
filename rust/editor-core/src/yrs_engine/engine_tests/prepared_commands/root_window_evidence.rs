const ROOT_WINDOW_EVIDENCE_REQUEST_ID: u64 = 70_035_000;
const ROOT_WINDOW_EVIDENCE_SMALL_PARAGRAPHS: usize = 8;
const ROOT_WINDOW_EVIDENCE_LARGE_PARAGRAPHS: usize = 512;

fn root_window_evidence_document(paragraphs: usize) -> serde_json::Value {
    json!({
        "type": "doc",
        "content": (0..paragraphs).map(|index| json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": format!("Paragraph {index:04} body ") },
                { "type": "text", "text": "marked", "marks": [{ "type": "italic" }] }
            ]
        })).collect::<Vec<_>>()
    })
}

fn middle_paragraph_caret(engine: &YrsDocumentEngine, paragraphs: usize) -> u32 {
    let block = engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .block(paragraphs / 2)
        .unwrap()
        .into_owned();
    block.scalar_start + block.scalar_prefix_len + 4
}

fn middle_paste_commit_passes(paragraphs: usize) -> (usize, usize, bool) {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    let document = root_window_evidence_document(paragraphs);
    let caret = middle_paragraph_caret(&localized_mark_engine(&document, 0, 0), paragraphs);
    let mut engine = localized_mark_engine(&document, caret, caret);
    reset_full_pass_counts_for_test();
    engine
        .apply_command(
            ROOT_WINDOW_EVIDENCE_REQUEST_ID,
            plain_text_paste("first pasted\nsecond pasted\nthird pasted"),
        )
        .unwrap()
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    let materialized = engine
        .derived_state
        .as_ref()
        .unwrap()
        .validation_certificate
        .canonical_fingerprint_materialized_for_test();
    assert_derived_state_matches_full_rebuild(&engine, &format!("{paragraphs} paragraphs"));
    engine
        .apply_command(
            ROOT_WINDOW_EVIDENCE_REQUEST_ID + 1,
            TypedCommand::InsertText {
                text: "identity".into(),
            },
        )
        .unwrap()
        .unwrap();
    assert_derived_state_matches_full_rebuild(
        &engine,
        &format!("{paragraphs} paragraphs after an identity-dependent insert"),
    );
    (
        counts.validation_certificate_constructions,
        counts.canonical_hashes,
        materialized,
    )
}

#[test]
fn a_root_window_commit_derives_its_certificate_from_the_changed_window() {
    let small = middle_paste_commit_passes(ROOT_WINDOW_EVIDENCE_SMALL_PARAGRAPHS);
    let large = middle_paste_commit_passes(ROOT_WINDOW_EVIDENCE_LARGE_PARAGRAPHS);
    assert_eq!(
        (small, large),
        ((0, 0, false), (0, 0, false)),
        "(whole-document certificate constructions, canonical hashes, fingerprint materialized at commit) for a middle paste"
    );
}

#[test]
fn root_window_certificates_track_depth_and_metrics_like_a_full_rebuild() {
    let text = |text: &str| json!({ "type": "text", "text": text });
    let paragraph = |content: &str| json!({ "type": "paragraph", "content": [text(content)] });
    let document = json!({ "type": "doc", "content": [
        paragraph("opening"),
        { "type": "blockquote", "content": [
            { "type": "blockquote", "content": [
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [paragraph("deepest")] }
                ]}
            ]}
        ]},
        { "type": "orderedList", "attrs": { "start": 4 }, "content": [
            { "type": "listItem", "content": [paragraph("ordered")] }
        ]},
        paragraph("closing"),
    ]});
    let probe = localized_mark_engine(&document, 0, 0);
    let map = &probe.derived_state.as_ref().unwrap().position_map;
    let start_of = |index: usize| {
        let block = map.block(index).unwrap();
        block.scalar_start + block.scalar_prefix_len
    };
    let deepest = start_of(1);
    let closing = start_of(map.block_count() - 1);
    let cases = [
        ("pasteIntoDeepestParagraph", deepest + 2, deepest + 2, Some("x\ny"), false),
        ("replaceAcrossDeepSubtree", start_of(0) + 2, closing + 2, Some("flat"), true),
        ("deleteAcrossDeepSubtree", start_of(0) + 2, closing + 2, None, true),
        ("pasteBeforeOpening", start_of(0), start_of(0), Some("lead\n"), false),
        ("pasteAtClosingEnd", closing + 7, closing + 7, Some("\ntrail\nmore"), false),
    ];
    for (case, anchor, head, pasted, removes_deepest) in cases {
        let mut engine = localized_mark_engine(&document, anchor, head);
        let depth_before = engine
            .derived_state
            .as_ref()
            .unwrap()
            .validation_certificate
            .depth_slots();
        let command = match pasted {
            Some(text) => plain_text_paste(text),
            None => TypedCommand::DeleteBackward,
        };
        engine
            .apply_command(ROOT_WINDOW_EVIDENCE_REQUEST_ID + 1, command)
            .unwrap()
            .unwrap();
        let depth_after = engine
            .derived_state
            .as_ref()
            .unwrap()
            .validation_certificate
            .depth_slots();
        assert_eq!(
            depth_after < depth_before,
            removes_deepest,
            "{case}: depth slots {depth_before} -> {depth_after}"
        );
        assert_derived_state_matches_full_rebuild(&engine, case);
        engine.undo(ROOT_WINDOW_EVIDENCE_REQUEST_ID + 2).unwrap().unwrap();
        assert_derived_state_matches_full_rebuild(&engine, &format!("{case} undo"));
        engine.redo(ROOT_WINDOW_EVIDENCE_REQUEST_ID + 3).unwrap().unwrap();
        assert_derived_state_matches_full_rebuild(&engine, &format!("{case} redo"));
    }
}
