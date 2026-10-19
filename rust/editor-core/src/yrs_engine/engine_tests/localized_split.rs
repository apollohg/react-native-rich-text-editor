use super::*;
use crate::test_support::document_strategy::document_strategy;
use crate::yrs_engine::mutation::{
    force_eager_mutation_lowering_for_test, reset_split_lowering_counts_for_test,
    take_split_lowering_counts_for_test,
};
use proptest::prelude::*;
use serde_json::{json, Value};

const SPLIT_EQUIVALENCE_REQUEST: u64 = 71_400;
const SPLIT_EQUIVALENCE_CASES: u32 = 96;
const MAX_EDITS: usize = 10;
const MAX_DELETED_SCALARS: u32 = 4;
const LOCALIZED_SPLIT_COUNTS: (usize, usize, usize, usize) = (0, 0, 1, 0);
const EAGER_SPLIT_COUNTS: (usize, usize, usize, usize) = (1, 1, 0, 1);
const ENGINE_INVARIANT_CODE: &str = "ENGINE_INVARIANT_FAILED";

#[derive(Debug, Clone)]
enum StructuralEdit {
    Split { offset: u32, after: bool },
    Type { offset: u32 },
    Backspace { offset: u32 },
}

fn edit_strategy() -> impl Strategy<Value = StructuralEdit> {
    prop_oneof![
        4 => (any::<u32>(), any::<bool>())
            .prop_map(|(offset, after)| StructuralEdit::Split { offset, after }),
        1 => any::<u32>().prop_map(|offset| StructuralEdit::Type { offset }),
        1 => any::<u32>().prop_map(|offset| StructuralEdit::Backspace { offset }),
    ]
}

fn table_engine() -> YrsDocumentEngine {
    YrsDocumentEngine::new(YrsEngineConfig {
        schema: crate::schema::presets::tiptap_table_schema(),
        ..transaction_engine_config(crate::yrs_engine::EditingLimits::default())
    })
    .unwrap()
}

fn bounded_scalar_offset(engine: &YrsDocumentEngine, offset: u32) -> u32 {
    let total = engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .total_scalars();
    offset % (total + 1)
}

fn split_transaction(
    engine: &YrsDocumentEngine,
    request_id: u64,
    offset: u32,
    after: bool,
) -> TypedTransaction {
    TypedTransaction {
        request_id,
        base_document_revision: engine.revision(),
        origin: TransactionOrigin::LocalInput,
        operations: vec![TypedOperation::SplitBlock {
            at: RevisionedPosition {
                offset,
                kind: EditorOffsetKind::Scalar,
                affinity: if after {
                    Affinity::After
                } else {
                    Affinity::Before
                },
            },
            node_type: "paragraph".into(),
            attrs: HashMap::new(),
        }],
        selection_intent: SelectionIntent::UseOperationResult,
        history_policy: HistoryPolicy::Auto,
    }
}

fn compile_split_both_ways(
    engine: &YrsDocumentEngine,
    transaction: &TypedTransaction,
    context: &str,
) -> Option<CompiledTransaction> {
    reset_split_lowering_counts_for_test();
    let localized = engine.compile_typed_transaction(transaction.clone());
    let localized_counts = take_split_lowering_counts_for_test();
    let eager = {
        let _forced = force_eager_mutation_lowering_for_test();
        engine.compile_typed_transaction(transaction.clone())
    };
    let eager_counts = take_split_lowering_counts_for_test();
    match (localized, eager) {
        (Ok(localized), Ok(eager)) => {
            assert_eq!(
                localized_counts, LOCALIZED_SPLIT_COUNTS,
                "{context}: a compilable split must lower through the window without an eager scan"
            );
            assert_eq!(
                eager_counts, EAGER_SPLIT_COUNTS,
                "{context}: the forced oracle must take the eager path"
            );
            localized
                .mutation_plan
                .assert_same_lowering_for_test(&eager.mutation_plan, context);
            Some(localized)
        }
        (Err(localized), Err(eager)) => {
            assert_eq!(
                localized, eager,
                "{context}: a rejected split must fail exactly like the eager path"
            );
            assert_ne!(
                localized.code, ENGINE_INVARIANT_CODE,
                "{context}: a split must never break a lowering invariant: {localized:?}"
            );
            None
        }
        (localized, eager) => panic!(
            "{context}: localized and eager split outcomes differ: localized={:?} eager={:?}",
            localized.map(|compiled| compiled.mutation_plan.actions.len()),
            eager.map(|compiled| compiled.mutation_plan.actions.len())
        ),
    }
}

fn apply_edit_command(
    engine: &mut YrsDocumentEngine,
    request: u64,
    offset: u32,
    command: TypedCommand,
    context: &str,
) {
    select_text(engine, request, offset, offset);
    let before = engine.document_json().unwrap();
    if let Err(error) = engine.apply_command(request + 1, command.clone()) {
        assert_ne!(
            error.code, ENGINE_INVARIANT_CODE,
            "{context}: an edit must never break a lowering invariant: {error:?}"
        );
        assert_eq!(
            engine.document_json().unwrap(),
            before,
            "{context}: a rejected edit must leave the document unchanged"
        );
        let eager = {
            let _forced = force_eager_mutation_lowering_for_test();
            engine.apply_command(request + 1, command)
        };
        assert_eq!(
            eager.err(),
            Some(error),
            "{context}: the eager path must reject the edit identically"
        );
    }
}

fn apply_structural_edits(document: &Value, edits: &[StructuralEdit]) {
    let mut engine = table_engine();
    engine
        .import_json(&document.to_string(), TransactionOrigin::DocumentImport)
        .unwrap();
    hydrate_import_for_compile_test(&mut engine);
    let mut request = SPLIT_EQUIVALENCE_REQUEST;
    for (step, edit) in edits.iter().enumerate() {
        request += 2;
        let before = engine.document_json().unwrap();
        match edit {
            StructuralEdit::Split { offset, after } => {
                let offset = bounded_scalar_offset(&engine, *offset);
                let context =
                    format!("step {step} split at scalar {offset} (after={after}) in {before}");
                let transaction = split_transaction(&engine, request, offset, *after);
                if let Some(compiled) = compile_split_both_ways(&engine, &transaction, &context) {
                    engine.apply_compiled_transaction(compiled, true).unwrap();
                }
            }
            StructuralEdit::Type { offset } => {
                let offset = bounded_scalar_offset(&engine, *offset);
                apply_edit_command(
                    &mut engine,
                    request,
                    offset,
                    TypedCommand::InsertText { text: "k".into() },
                    &format!("step {step} type at scalar {offset} in {before}"),
                );
            }
            StructuralEdit::Backspace { offset } => {
                let offset = bounded_scalar_offset(&engine, *offset);
                apply_edit_command(
                    &mut engine,
                    request,
                    offset,
                    TypedCommand::DeleteBackward,
                    &format!("step {step} backspace at scalar {offset} in {before}"),
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(SPLIT_EQUIVALENCE_CASES))]

    #[test]
    fn localized_split_plans_equal_eager_plans_after_random_structural_edits(
        document in document_strategy(),
        edits in prop::collection::vec(edit_strategy(), 1..MAX_EDITS),
    ) {
        apply_structural_edits(&document, &edits);
    }
}

#[test]
fn localized_split_covers_block_edges_lists_and_table_cells() {
    let document = json!({
        "type": "doc",
        "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "ab" },
                { "type": "text", "text": "cd", "marks": [{ "type": "bold" }] },
                { "type": "hardBreak" },
                { "type": "text", "text": "🦀e" }
            ] },
            { "type": "paragraph" },
            { "type": "bulletList", "content": [
                { "type": "listItem", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "item" }] },
                    { "type": "paragraph", "content": [{ "type": "text", "text": "tail" }] }
                ] }
            ] },
            { "type": "table", "content": [
                { "type": "tableRow", "content": [
                    { "type": "tableCell", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "cell" }] }
                    ] },
                    { "type": "tableCell", "content": [
                        { "type": "blockquote", "content": [
                            { "type": "paragraph", "content": [{ "type": "text", "text": "quote" }] }
                        ] }
                    ] }
                ] }
            ] },
            { "type": "h1", "content": [{ "type": "text", "text": "end" }] }
        ]
    });
    let mut engine = table_engine();
    engine
        .import_json(&document.to_string(), TransactionOrigin::DocumentImport)
        .unwrap();
    hydrate_import_for_compile_test(&mut engine);
    let total = engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .total_scalars();
    let mut compiled_splits = 0usize;
    for offset in 0..=total {
        for after in [false, true] {
            let context = format!("scalar {offset} (after={after}) in {document}");
            let transaction = split_transaction(
                &engine,
                SPLIT_EQUIVALENCE_REQUEST + u64::from(offset),
                offset,
                after,
            );
            if compile_split_both_ways(&engine, &transaction, &context).is_some() {
                compiled_splits += 1;
            }
        }
    }
    assert!(
        compiled_splits > usize::try_from(total).unwrap(),
        "every textblock offset must compile a split: {compiled_splits} of {total} offsets"
    );
}

#[derive(Debug, Clone)]
enum RootWindowEdit {
    Split { offset: u32, after: bool },
    Delete { offset: u32, len: u32 },
}

fn root_window_edit_strategy() -> impl Strategy<Value = RootWindowEdit> {
    prop_oneof![
        3 => (any::<u32>(), any::<bool>())
            .prop_map(|(offset, after)| RootWindowEdit::Split { offset, after }),
        2 => (any::<u32>(), 1u32..MAX_DELETED_SCALARS)
            .prop_map(|(offset, len)| RootWindowEdit::Delete { offset, len }),
    ]
}

fn delete_transaction(
    engine: &YrsDocumentEngine,
    request_id: u64,
    from: u32,
    to: u32,
) -> TypedTransaction {
    let point = |offset| RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    };
    TypedTransaction {
        request_id,
        base_document_revision: engine.revision(),
        origin: TransactionOrigin::LocalInput,
        operations: vec![TypedOperation::DeleteRange {
            range: RevisionedRange {
                from: point(from),
                to: point(to),
            },
        }],
        selection_intent: SelectionIntent::UseOperationResult,
        history_policy: HistoryPolicy::Auto,
    }
}

fn retained_source_bytes(artifact: &crate::yrs_engine::canonical::CanonicalArtifact) -> i128 {
    i128::try_from(
        artifact
            .derived_metrics_for_test()
            .4
            .unwrap()
            .source_document_retained_bytes,
    )
    .unwrap()
}

fn assert_root_window_derivations_match_full_rebuild(
    engine: &YrsDocumentEngine,
    previous: &crate::yrs_engine::canonical::CanonicalArtifact,
    compiled: &CompiledTransaction,
    counts: crate::yrs_engine::observability::FullPassCounts,
    context: &str,
) {
    let Some(derivations) = compiled.preview_derivations.as_ref() else {
        return;
    };
    let rendered = crate::render::rendered_text(&compiled.preview, &engine.schema);
    assert_eq!(
        &*derivations.rendered_text, rendered,
        "{context}: spliced rendered text must equal a full render"
    );
    assert_eq!(
        derivations.rendered_scalars,
        u32::try_from(rendered.chars().count()).unwrap(),
        "{context}: spliced rendered scalar count"
    );
    assert_eq!(
        derivations.document_text_bytes,
        crate::yrs_engine::compiler::document_text_bytes(&compiled.preview).unwrap(),
        "{context}: spliced document text bytes"
    );
    assert_eq!(
        derivations.document_node_count,
        crate::editor_state::document_node_count(compiled.preview.root()),
        "{context}: spliced document node count"
    );
    if !compiled.localized_semantic_used {
        let canonical = crate::yrs_engine::canonical::CanonicalSchemaContext::new(&engine.schema);
        let artifact = compiled.canonical_artifact.as_ref().unwrap();
        let full = canonical.derive(artifact.source_document()).unwrap();
        let full_previous = canonical.derive(previous.source_document()).unwrap();
        let (length, admission, scalars, bytes, _) = artifact.derived_metrics_for_test();
        let (full_length, full_admission, full_scalars, full_bytes, _) =
            full.derived_metrics_for_test();
        assert_eq!(
            (length, admission, scalars, bytes),
            (full_length, full_admission, full_scalars, full_bytes),
            "{context}: spliced canonical metrics must equal a full derivation"
        );
        assert_eq!(
            retained_source_bytes(artifact) - retained_source_bytes(previous),
            retained_source_bytes(&full) - retained_source_bytes(&full_previous),
            "{context}: the spliced retained-byte change must equal the full derivation change"
        );
    }
    assert_eq!(
        (
            counts.rendered_text_derivations,
            counts.raw_document_text_scans,
            counts.document_node_count_scans,
            counts.canonical_serializations
        ),
        (0, 0, 0, 0),
        "{context}: a root-window edit must not re-derive document-wide text or canonical output: {counts:?}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(SPLIT_EQUIVALENCE_CASES))]

    #[test]
    fn root_window_preview_derivations_equal_full_rebuilds_after_random_structural_edits(
        document in document_strategy(),
        edits in prop::collection::vec(root_window_edit_strategy(), 1..MAX_EDITS),
    ) {
        let mut engine = table_engine();
        engine
            .import_json(&document.to_string(), TransactionOrigin::DocumentImport)
            .unwrap();
        hydrate_import_for_compile_test(&mut engine);
        let mut request = SPLIT_EQUIVALENCE_REQUEST;
        for (step, edit) in edits.into_iter().enumerate() {
            request += 1;
            let before = engine.document_json().unwrap();
            let (transaction, context) = match edit {
                RootWindowEdit::Split { offset, after } => {
                    let offset = bounded_scalar_offset(&engine, offset);
                    (
                        split_transaction(&engine, request, offset, after),
                        format!("step {step} split at scalar {offset} in {before}"),
                    )
                }
                RootWindowEdit::Delete { offset, len } => {
                    let from = bounded_scalar_offset(&engine, offset);
                    let to = bounded_scalar_offset(&engine, from.saturating_add(len)).max(from);
                    (
                        delete_transaction(&engine, request, from, to),
                        format!("step {step} delete scalars {from}..{to} in {before}"),
                    )
                }
            };
            let previous = engine.derived_state.as_ref().unwrap().canonical_artifact.clone();
            crate::yrs_engine::observability::reset_full_pass_counts_for_test();
            let compiled = engine.compile_typed_transaction(transaction);
            let counts = crate::yrs_engine::observability::take_full_pass_counts_for_test();
            if let Ok(compiled) = compiled {
                assert_root_window_derivations_match_full_rebuild(
                    &engine, &previous, &compiled, counts, &context,
                );
                crate::yrs_engine::derived_state::reset_localized_index_lifecycle_counts_for_test();
                engine.apply_compiled_transaction(compiled, true).unwrap();
                let (index_builds, ..) =
                    crate::yrs_engine::derived_state::take_localized_index_lifecycle_counts_for_test();
                let state = engine.derived_state.as_ref().unwrap();
                let installed = state.localized_text_index.as_ref().map(|index| index.leaves());
                let rebuilt = state
                    .rebuilt_localized_text_index_for_test(&engine.resource_limits, &engine.schema)
                    .map(|index| index.leaves());
                assert_eq!(
                    installed, rebuilt,
                    "{context}: the committed text leaf index must equal a full rebuild"
                );
                assert_eq!(
                    index_builds, 0,
                    "{context}: a root-window commit must splice the text leaf index"
                );
            }
        }
    }
}
