const ROOT_WINDOW_LOOKUP_REQUEST_ID: u64 = 70_036_000;

fn assert_lookup_matches_fresh_build(engine: &YrsDocumentEngine, case: &str) {
    let txn = engine.doc.transact();
    let fragment = txn.get_xml_fragment(engine.fragment_name.as_str()).unwrap();
    engine
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .assert_payload_matches_fresh_build_for_test(&txn, &fragment, &engine.schema, case);
}

#[test]
fn root_window_commits_splice_the_lookup_seed_and_branch_index() {
    use crate::yrs_engine::mutation::{
        reset_localized_lookup_counts_for_test, take_localized_lookup_counts_for_test,
    };

    let text = |text: &str| json!({ "type": "text", "text": text });
    let paragraph = |content: &str| json!({ "type": "paragraph", "content": [text(content)] });
    let document = json!({ "type": "doc", "content": [
        paragraph("opening words"),
        { "type": "bulletList", "content": [
            { "type": "listItem", "content": [paragraph("first item")] },
            { "type": "listItem", "content": [paragraph("second item")] }
        ]},
        { "type": "horizontalRule" },
        { "type": "paragraph", "content": [text("marked "), { "type": "text", "text": "bold", "marks": [{ "type": "bold" }] }] },
        { "type": "blockquote", "content": [paragraph("quoted")] },
        paragraph("closing words"),
    ]});
    let probe = localized_mark_engine(&document, 0, 0);
    let map = &probe.derived_state.as_ref().unwrap().position_map;
    let start_of = |index: usize| {
        let block = map.block(index).unwrap();
        block.scalar_start + block.scalar_prefix_len
    };
    let last = map.block_count() - 1;
    let cases = [
        ("middleOfFirstParagraph", start_of(0) + 4, start_of(0) + 4, "a\nb\nc"),
        ("startOfDocument", start_of(0), start_of(0), "lead\n"),
        ("insideMarkedParagraph", start_of(4) + 3, start_of(4) + 3, "x\ny"),
        ("acrossListAndRule", start_of(0) + 2, start_of(4) + 2, "joined\nlines"),
        ("endOfDocument", start_of(last) + 13, start_of(last) + 13, "\ntrail\nmore"),
    ];
    for (case, anchor, head, pasted) in cases {
        let mut engine = localized_mark_engine(&document, anchor, head);
        hydrate_import_for_compile_test(&mut engine);
        reset_localized_lookup_counts_for_test();
        engine
            .apply_command(ROOT_WINDOW_LOOKUP_REQUEST_ID, plain_text_paste(pasted))
            .unwrap()
            .unwrap();
        let (full_lookup_builds, ..) = take_localized_lookup_counts_for_test();
        assert_eq!(full_lookup_builds, 0, "{case}: the commit splices instead of rebuilding");
        assert_lookup_matches_fresh_build(&engine, case);
        assert_derived_state_matches_full_rebuild(&engine, case);
        assert!(
            engine
                .derived_state
                .as_ref()
                .unwrap()
                .block_branch_index
                .is_some(),
            "{case}: the spliced branch index is available"
        );

        reset_localized_lookup_counts_for_test();
        engine
            .apply_command(
                ROOT_WINDOW_LOOKUP_REQUEST_ID + 1,
                TypedCommand::InsertText { text: "z".into() },
            )
            .unwrap()
            .unwrap();
        let (builds_after, localized_hits, _) = take_localized_lookup_counts_for_test();
        assert_eq!(
            (builds_after, localized_hits),
            (0, 1),
            "{case}: the next keystroke lowers locally on the spliced seed"
        );
        assert_lookup_matches_fresh_build(&engine, &format!("{case} keystroke"));
        assert_derived_state_matches_full_rebuild(&engine, &format!("{case} keystroke"));
    }
}
