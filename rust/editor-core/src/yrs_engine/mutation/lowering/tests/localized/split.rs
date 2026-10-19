#[test]
fn localized_split_charges_and_limit_errors_equal_eager_at_every_textblock_position() {
    const REQUEST: u64 = 721;
    const BASE_SCAN_WORK: usize = 23;
    const UNBOUNDED_WORK: usize = 1_000_000;
    const LIMIT_SAMPLES: usize = 48;
    let schema = crate::schema::presets::tiptap_table_schema();
    let limits = ResourceLimits::default();
    let editing_limits = EditingLimits::default();
    let source = json!({
        "type": "doc",
        "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "ab" },
                { "type": "text", "text": "c🦀", "marks": [{ "type": "bold" }] },
                { "type": "hardBreak" },
                { "type": "text", "text": "d" }
            ] },
            { "type": "horizontalRule" },
            { "type": "paragraph" },
            { "type": "orderedList", "attrs": { "start": 3 }, "content": [
                { "type": "listItem", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "one" }] },
                    { "type": "paragraph", "content": [{ "type": "text", "text": "tail" }] },
                    { "type": "bulletList", "content": [
                        { "type": "listItem", "content": [
                            { "type": "paragraph", "content": [{ "type": "text", "text": "deep" }] }
                        ] }
                    ] }
                ] },
                { "type": "listItem", "content": [{ "type": "paragraph" }] }
            ] },
            { "type": "table", "content": [
                { "type": "tableRow", "content": [
                    { "type": "tableCell", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "cell" }] },
                        { "type": "image", "attrs": { "src": "cell.png" } }
                    ] },
                    { "type": "tableCell", "content": [
                        { "type": "blockquote", "content": [
                            { "type": "h2", "content": [{ "type": "text", "text": "quoted" }] }
                        ] }
                    ] }
                ] }
            ] },
            { "type": "h1", "content": [{ "type": "text", "text": "end" }] }
        ]
    });
    let document = from_prosemirror_json(&source, &schema, UnknownTypeMode::Preserve).unwrap();
    let doc = seeded_document(&source, &schema, &limits);
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    let seed = MutationLookupSeed::build(
        REQUEST,
        &txn,
        &fragment,
        &schema,
        &document,
        &limits,
        &editing_limits,
        None,
        "schema-a",
        3,
        7,
    )
    .unwrap();
    let compile_eager = |position: u32, after: &Document, action_limit, scan_limit| {
        let mut compiler = MutationCompiler::new(
            REQUEST,
            &txn,
            &fragment,
            &schema,
            action_limit,
            scan_limit,
            BASE_SCAN_WORK,
        )?;
        compiler.split_block(0, &document, after, position, &schema, &limits)?;
        compiler.finish(Some(0))
    };
    let compile_localized = |position: u32, after: &Document, action_limit, scan_limit| {
        LocalizedSplitCompiler::try_new(
            REQUEST,
            &txn,
            &fragment,
            &schema,
            action_limit,
            scan_limit,
            BASE_SCAN_WORK,
            LocalizedSplitLocator {
                document: &document,
                position,
            },
            &seed,
            &limits,
            &editing_limits,
            None,
            "schema-a",
            3,
            7,
        )?
        .unwrap_or_else(|| panic!("split at {position} must localize"))
        .split_block(0, &document, after, position, &schema, &limits)
    };
    let mut checked_positions = Vec::new();
    let mut rejected_positions = Vec::new();
    for position in 0..=document.root().content_size() {
        let Ok(resolved) = document.resolve(position) else {
            continue;
        };
        if !schema.is_text_block(resolved.parent(&document).node_type()) {
            continue;
        }
        let Ok((after, _)) = apply_step_canonical_marks(
            &document,
            &Step::SplitBlock {
                pos: position,
                node_type: "paragraph".into(),
                attrs: HashMap::new(),
            },
            &schema,
        ) else {
            continue;
        };
        let (baseline, localized) = match (
            compile_eager(position, &after, UNBOUNDED_WORK, UNBOUNDED_WORK),
            compile_localized(position, &after, UNBOUNDED_WORK, UNBOUNDED_WORK),
        ) {
            (Ok(baseline), Ok(localized)) => (baseline, localized),
            (Err(eager), Err(localized)) => {
                assert_eq!(
                    localized, eager,
                    "split at {position}: rejections must match"
                );
                checked_positions.push(position);
                rejected_positions.push((position, eager.message));
                continue;
            }
            (eager, localized) => panic!(
                "split at {position}: outcomes differ: eager {:?} localized {:?}",
                eager.err(),
                localized.err()
            ),
        };
        localized.assert_same_lowering_for_test(&baseline, &format!("split at {position}"));
        preflight_mutation_plan(REQUEST, &localized, &txn).unwrap();
        let exact_actions = baseline.compilation_work_for_test();
        let stride = (exact_actions / LIMIT_SAMPLES).max(1);
        for action_limit in (0..=exact_actions)
            .step_by(stride)
            .chain([exact_actions - 1, exact_actions])
        {
            let context = format!("split at {position} with action limit {action_limit}");
            match (
                compile_localized(position, &after, action_limit, UNBOUNDED_WORK),
                compile_eager(position, &after, action_limit, UNBOUNDED_WORK),
            ) {
                (Ok(localized), Ok(eager)) => {
                    localized.assert_same_lowering_for_test(&eager, &context)
                }
                (Err(localized), Err(eager)) => assert_eq!(localized, eager, "{context}"),
                (localized, eager) => panic!(
                    "{context}: outcomes differ: localized {:?} eager {:?}",
                    localized.err(),
                    eager.err()
                ),
            }
        }
        let mut exhausted_preflight = localized.clone();
        exhausted_preflight.set_work_limit_for_test(
            baseline.compilation_work_for_test() + baseline.expected_preflight_work_for_test() - 1,
        );
        let mut eager_exhausted_preflight = baseline.clone();
        eager_exhausted_preflight.set_work_limit_for_test(
            baseline.compilation_work_for_test() + baseline.expected_preflight_work_for_test() - 1,
        );
        assert_eq!(
            preflight_mutation_plan(REQUEST, &exhausted_preflight, &txn).unwrap_err(),
            preflight_mutation_plan(REQUEST, &eager_exhausted_preflight, &txn).unwrap_err(),
            "split at {position}: preflight exhaustion must match"
        );
        checked_positions.push(position);
    }
    let mut textblock_positions = 0usize;
    let mut pending = vec![document.root()];
    while let Some(node) = pending.pop() {
        if schema.is_text_block(node.node_type()) {
            textblock_positions += usize::try_from(node.content_size()).unwrap() + 1;
        } else if let Some(content) = node.content() {
            pending.extend(content.iter());
        }
    }
    eprintln!("checked split positions: {checked_positions:?}");
    eprintln!("rejected by both lowerings: {rejected_positions:?}");
    assert_eq!(
        checked_positions.len(),
        textblock_positions,
        "every textblock position, including empty blocks, list items and table cells, must be checked"
    );
}
