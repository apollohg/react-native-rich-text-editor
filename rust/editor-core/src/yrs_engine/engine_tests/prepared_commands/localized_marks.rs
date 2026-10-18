const LOCALIZED_MARK_REQUEST_ID: u64 = 70_032_000;
const SMALL_DOCUMENT_PARAGRAPHS: usize = 8;
const LARGE_DOCUMENT_PARAGRAPHS: usize = 512;

fn localized_mark_corpus() -> Vec<(&'static str, serde_json::Value)> {
    let text = |text: &str| json!({ "type": "text", "text": text });
    let marked = |text: &str, marks: serde_json::Value| {
        json!({ "type": "text", "text": text, "marks": marks })
    };
    let paragraph = |content: serde_json::Value| json!({ "type": "paragraph", "content": content });
    vec![
        (
            "mixedMarksAndUnicode",
            json!({ "type": "doc", "content": [
                paragraph(json!([text("lead "), marked("bold😀", json!([{ "type": "bold" }])), text(" e\u{301}丁 "), marked("ital", json!([{ "type": "italic" }]))])),
                paragraph(json!([marked("linked 🦀 text", json!([{ "type": "link", "attrs": { "href": "https://example.com/1" } }])), text(" tail")])),
                { "type": "h2", "content": [text("Heading "), marked("strong", json!([{ "type": "bold" }, { "type": "italic" }]))] },
            ]}),
        ),
        (
            "nestedContainers",
            json!({ "type": "doc", "content": [
                paragraph(json!([text("before")])),
                { "type": "blockquote", "content": [paragraph(json!([text("quoted "), marked("words", json!([{ "type": "underline" }]))]))] },
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [paragraph(json!([text("first item")]))] },
                    { "type": "listItem", "content": [
                        paragraph(json!([marked("second", json!([{ "type": "strike" }])), text(" item")])),
                        { "type": "orderedList", "attrs": { "start": 3 }, "content": [
                            { "type": "listItem", "content": [paragraph(json!([text("nested ordered")]))] },
                        ]},
                    ]},
                ]},
                { "type": "taskList", "content": [
                    { "type": "taskItem", "attrs": { "checked": true }, "content": [paragraph(json!([text("task text")]))] },
                ]},
            ]}),
        ),
        (
            "voidsAndBreaks",
            json!({ "type": "doc", "content": [
                paragraph(json!([text("line one"), { "type": "hardBreak" }, marked("line two", json!([{ "type": "code" }]))])),
                { "type": "horizontalRule" },
                paragraph(json!([text("after rule "), marked("x", json!([{ "type": "bold" }])), text("yz")])),
                { "type": "image", "attrs": { "src": "https://example.com/a.png" } },
                { "type": "codeBlock", "attrs": { "language": "rust" }, "content": [text("fn main() {}")] },
                paragraph(json!([text("tail paragraph")])),
            ]}),
        ),
    ]
}

fn localized_mark_engine(document: &serde_json::Value, anchor: u32, head: u32) -> YrsDocumentEngine {
    let mut engine = transaction_engine();
    engine
        .import_json(&document.to_string(), TransactionOrigin::DocumentImport)
        .unwrap();
    select_text(&mut engine, LOCALIZED_MARK_REQUEST_ID, anchor, head);
    engine
        .derived_state
        .as_ref()
        .unwrap()
        .canonical_artifact
        .serialized_len();
    engine
}

fn localized_mark_ranges(engine: &YrsDocumentEngine) -> Vec<(u32, u32)> {
    let map = &engine.derived_state.as_ref().unwrap().position_map;
    let mut ranges = Vec::new();
    for index in 0..map.block_count() {
        let block = map.block(index).unwrap();
        let start = block.scalar_start + block.scalar_prefix_len;
        let length = block.scalar_len;
        if block.is_void_block || length == 0 {
            continue;
        }
        ranges.push((start, start + length));
        ranges.push((start + length, start));
        if length > 2 {
            ranges.push((start + 1, start + length - 1));
            ranges.push((start + length / 3, start + 2 * length / 3 + 1));
        }
    }
    ranges
}

fn localized_mark_commands() -> Vec<TypedCommand> {
    vec![
        TypedCommand::ToggleMark {
            mark_type: "bold".into(),
        },
        TypedCommand::ToggleMark {
            mark_type: "italic".into(),
        },
        TypedCommand::ToggleMark {
            mark_type: "code".into(),
        },
        TypedCommand::SetMark {
            mark_type: "link".into(),
            attrs: [("href".to_string(), json!("https://example.com/set"))]
                .into_iter()
                .collect(),
        },
        TypedCommand::UnsetMark {
            mark_type: "link".into(),
        },
    ]
}

fn public_command_result(
    engine: &mut YrsDocumentEngine,
    request_id: u64,
    command: TypedCommand,
) -> crate::yrs_engine::OperationResult<Option<crate::yrs_engine::TypedTransactionResult>> {
    match engine.plan_command(request_id, command)? {
        CommandPlan::NotApplicable => Ok(None),
        CommandPlan::SelectionOnly(transaction) | CommandPlan::Transaction(transaction) => engine
            .apply_typed_transaction_with_result(transaction)
            .map(Some),
    }
}

fn position_map_blocks(
    map: &crate::position::PositionMap,
) -> Vec<(u32, u32, u32, u32, u32, u32, Vec<u32>, bool)> {
    (0..map.block_count())
        .map(|index| {
            let block = map.block(index).unwrap();
            (
                map.effective_doc_start(index),
                map.effective_doc_end(index),
                block.scalar_start,
                block.scalar_len,
                block.scalar_prefix_len,
                block.rendered_break_after,
                block.node_path.to_vec(),
                block.is_void_block,
            )
        })
        .collect()
}

fn assert_localized_index_presence_matches(
    left: &YrsDocumentEngine,
    right: &YrsDocumentEngine,
    case: &str,
) {
    assert_eq!(
        left.derived_state
            .as_ref()
            .unwrap()
            .localized_text_index
            .is_some(),
        right
            .derived_state
            .as_ref()
            .unwrap()
            .localized_text_index
            .is_some(),
        "{case}: localized text index presence"
    );
}

fn assert_derived_state_matches_full_rebuild(engine: &YrsDocumentEngine, case: &str) {
    let state = engine.derived_state.as_ref().unwrap();
    let txn = engine.doc.transact();
    let fragment = txn.get_xml_fragment(engine.fragment_name.as_str()).unwrap();
    let rebuilt = crate::yrs_engine::derived_state::DerivedStateCache::initialize(
        state.document.clone(),
        engine.canonical_schema.derive(&state.document).unwrap(),
        &txn,
        &fragment,
        &engine.schema,
        &engine.resource_limits,
        &engine.editing_limits,
        engine.max_length,
        &engine.schema_fingerprint,
        engine.revision,
        engine.state_revision,
        engine.yrs_state_epoch,
    )
    .unwrap();
    assert_eq!(
        state.canonical_artifact.serialized_len(),
        rebuilt.canonical_artifact.serialized_len(),
        "{case}: canonical length"
    );
    assert_eq!(
        state.canonical_artifact.text_scalar_len(),
        rebuilt.canonical_artifact.text_scalar_len(),
        "{case}: canonical text scalars"
    );
    assert_eq!(
        state.canonical_artifact.text_utf8_bytes(),
        rebuilt.canonical_artifact.text_utf8_bytes(),
        "{case}: canonical text bytes"
    );
    assert_eq!(
        state.canonical_artifact.history_snapshot_retained_charge(),
        rebuilt.canonical_artifact.history_snapshot_retained_charge(),
        "{case}: canonical retained charge"
    );
    assert_eq!(
        state.canonical_artifact.sha256(),
        rebuilt.canonical_artifact.sha256(),
        "{case}: canonical digest"
    );
    assert_eq!(
        state.materialized_identity_for_test().0,
        rebuilt.materialized_identity_for_test().0,
        "{case}: validation certificate"
    );
    if let Some(index) = state.localized_text_index.as_ref() {
        assert_eq!(
            Some(index.leaves()),
            rebuilt
                .localized_text_index
                .as_ref()
                .map(crate::yrs_engine::derived_state::LocalizedTextLeafIndex::leaves),
            "{case}: localized text leaves"
        );
    }
    assert_eq!(
        position_map_blocks(&state.position_map),
        position_map_blocks(&rebuilt.position_map),
        "{case}: position map"
    );
    assert_eq!(
        state.position_map.total_scalars(),
        rebuilt.position_map.total_scalars(),
        "{case}: position scalars"
    );
    assert_eq!(state.rendered_text, rebuilt.rendered_text, "{case}: text");
    assert_eq!(
        state.rendered_scalars, rebuilt.rendered_scalars,
        "{case}: scalars"
    );
    assert_eq!(
        state.document_text_bytes, rebuilt.document_text_bytes,
        "{case}: text bytes"
    );
    assert_eq!(
        state.document_node_count, rebuilt.document_node_count,
        "{case}: node count"
    );
    assert_eq!(
        state.render_blocks.materialize(),
        rebuilt.render_blocks.materialize(),
        "{case}: render blocks"
    );
    assert_eq!(
        state.table_projection_index, rebuilt.table_projection_index,
        "{case}: table projection"
    );
    assert_eq!(
        state.block_branch_index, rebuilt.block_branch_index,
        "{case}: block branch index"
    );
}

#[test]
fn prepared_textblock_marks_match_public_path_and_full_rebuild() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    let mut localized_commits = 0usize;
    for (name, document) in localized_mark_corpus() {
        let ranges = localized_mark_ranges(&localized_mark_engine(&document, 0, 0));
        for (anchor, head) in ranges {
            for command in localized_mark_commands() {
                let case = format!("{name} {anchor}..{head} {command:?}");
                let mut prepared = localized_mark_engine(&document, anchor, head);
                let mut public = localized_mark_engine(&document, anchor, head);
                let before_document = prepared.document_json().unwrap();
                reset_full_pass_counts_for_test();
                let prepared_result =
                    prepared.apply_command(LOCALIZED_MARK_REQUEST_ID + 1, command.clone());
                let counts = take_full_pass_counts_for_test();
                let public_result =
                    public_command_result(&mut public, LOCALIZED_MARK_REQUEST_ID + 1, command.clone());
                assert_eq!(prepared_result, public_result, "{case}: result");
                assert_eq!(prepared.document_json(), public.document_json(), "{case}: json");
                assert_eq!(prepared.document_html(), public.document_html(), "{case}: html");
                assert_eq!(
                    prepared.resolved_selection(),
                    public.resolved_selection(),
                    "{case}: selection"
                );
                assert_eq!(prepared.stored_marks(), public.stored_marks(), "{case}: stored");
                assert_eq!(prepared.can_undo(), public.can_undo(), "{case}: can undo");
                assert_eq!(prepared.can_redo(), public.can_redo(), "{case}: can redo");
                assert_derived_state_matches_full_rebuild(&prepared, &case);
                assert_localized_index_presence_matches(&prepared, &public, &case);
                let changed = prepared.document_json().unwrap() != before_document;
                if changed && matches!(command, TypedCommand::ToggleMark { .. }) {
                    localized_commits += 1;
                    assert_eq!(
                        (
                            counts.canonical_mark_tree_scans,
                            counts.canonical_serializations,
                            counts.canonical_hashes,
                            counts.rendered_text_derivations,
                            counts.yrs_tree_walks,
                        ),
                        (0, 0, 0, 0, 0),
                        "{case}: a textblock mark change must not repeat document-wide derivations: {counts:?}"
                    );
                }
                if changed {
                    assert_eq!(
                        prepared.undo(LOCALIZED_MARK_REQUEST_ID + 2).unwrap(),
                        public.undo(LOCALIZED_MARK_REQUEST_ID + 2).unwrap(),
                        "{case}: undo"
                    );
                    assert_eq!(prepared.document_json(), public.document_json(), "{case}: undo");
                    assert_derived_state_matches_full_rebuild(&prepared, &format!("{case} undo"));
                    assert_localized_index_presence_matches(&prepared, &public, &format!("{case} undo"));
                    assert_eq!(
                        prepared.redo(LOCALIZED_MARK_REQUEST_ID + 3).unwrap(),
                        public.redo(LOCALIZED_MARK_REQUEST_ID + 3).unwrap(),
                        "{case}: redo"
                    );
                    assert_eq!(prepared.document_json(), public.document_json(), "{case}: redo");
                    assert_derived_state_matches_full_rebuild(&prepared, &format!("{case} redo"));
                    assert_localized_index_presence_matches(&prepared, &public, &format!("{case} redo"));
                }
            }
        }
    }
    assert!(
        localized_commits > 0,
        "the corpus must exercise localized textblock mark commits"
    );
}

#[test]
fn sequential_textblock_marks_keep_carried_state_equal_to_full_rebuild() {
    let document = localized_mark_corpus()
        .into_iter()
        .find(|(name, _)| *name == "nestedContainers")
        .unwrap()
        .1;
    let ranges = localized_mark_ranges(&localized_mark_engine(&document, 0, 0));
    let mut prepared = localized_mark_engine(&document, 0, 0);
    let mut public = localized_mark_engine(&document, 0, 0);
    for (index, (anchor, head)) in ranges.into_iter().enumerate() {
        let request_id = LOCALIZED_MARK_REQUEST_ID + 100 + 4 * index as u64;
        select_text(&mut prepared, request_id, anchor, head);
        select_text(&mut public, request_id, anchor, head);
        let command = TypedCommand::ToggleMark {
            mark_type: if index % 2 == 0 { "bold" } else { "italic" }.into(),
        };
        let case = format!("sequential {index} {anchor}..{head} {command:?}");
        assert_eq!(
            prepared.apply_command(request_id + 1, command.clone()),
            public_command_result(&mut public, request_id + 1, command),
            "{case}"
        );
        assert_eq!(prepared.document_json(), public.document_json(), "{case}");
        assert_derived_state_matches_full_rebuild(&prepared, &case);
        assert_localized_index_presence_matches(&prepared, &public, &case);
    }
}

fn middle_paragraph_toggle_derivation_work(paragraphs: usize) -> [usize; 11] {
    use crate::yrs_engine::derived_state::{
        reset_localized_index_lifecycle_counts_for_test, reset_localized_index_metrics_for_test,
        take_localized_index_lifecycle_counts_for_test, take_localized_index_metrics_for_test,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    const BODY: &str = "Native editing preserves each word while the document grows. ";
    let document = json!({
        "type": "doc",
        "content": (0..paragraphs).map(|index| json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": format!("Paragraph {index:04}: {BODY}") },
                { "type": "text", "text": "bold emphasis", "marks": [{ "type": "bold" }] },
                { "type": "text", "text": ", italic detail.", "marks": [{ "type": "italic" }] }
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
    let start = block.scalar_start + block.scalar_prefix_len + BODY.len() as u32;
    let mut engine = localized_mark_engine(&document, start, start + 20);
    reset_full_pass_counts_for_test();
    reset_localized_index_lifecycle_counts_for_test();
    reset_localized_index_metrics_for_test();
    engine
        .apply_command(
            LOCALIZED_MARK_REQUEST_ID + 500,
            TypedCommand::ToggleMark {
                mark_type: "bold".into(),
            },
        )
        .unwrap()
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    let (index_builds, ..) = take_localized_index_lifecycle_counts_for_test();
    let (index_path_hops, index_visits, ..) = take_localized_index_metrics_for_test();
    assert_derived_state_matches_full_rebuild(&engine, &format!("{paragraphs} paragraphs"));
    [
        counts.document_validations,
        counts.canonical_mark_tree_scans,
        counts.canonical_mark_nodes_visited,
        counts.canonical_projections,
        counts.canonical_serializations,
        counts.canonical_hashes,
        counts.rendered_text_derivations,
        counts.yrs_tree_walks,
        index_builds,
        index_path_hops,
        index_visits,
    ]
}

#[test]
fn textblock_mark_toggle_derivations_do_not_grow_with_document_length() {
    const MARK_VALIDATION_NODES: usize = 4;
    let small = middle_paragraph_toggle_derivation_work(SMALL_DOCUMENT_PARAGRAPHS);
    let large = middle_paragraph_toggle_derivation_work(LARGE_DOCUMENT_PARAGRAPHS);
    assert_eq!(
        small, large,
        "[validations, mark scans, mark nodes, projections, serializations, hashes, rendered texts, Yrs walks, index builds, index hops, index visits]"
    );
    assert_eq!(large, [0, 0, MARK_VALIDATION_NODES, 0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn unset_mark_without_the_mark_in_range_is_an_unchanged_command() {
    const ANCHOR: u32 = 1;
    const HEAD: u32 = 9;
    let document = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [
                { "type": "text", "text": "plain " },
                { "type": "text", "text": "bold", "marks": [{ "type": "bold" }] },
                { "type": "text", "text": " tail" }
            ]
        }]
    });
    let mut engine = localized_mark_engine(&document, ANCHOR, HEAD);
    let before_document = engine.document_json().unwrap();
    let before_revision = engine.revision();
    let result = engine
        .apply_command(
            LOCALIZED_MARK_REQUEST_ID + 600,
            TypedCommand::UnsetMark {
                mark_type: "link".into(),
            },
        )
        .expect("unsetting an absent mark is not an engine invariant failure")
        .expect("unsetting an absent mark over a text range is applicable");
    assert!(!result.changed, "{result:?}");
    assert_eq!(result.document_revision, before_revision);
    assert_eq!(engine.revision(), before_revision);
    assert_eq!(engine.document_json().unwrap(), before_document);
    assert!(!engine.can_undo());
}
