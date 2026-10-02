use super::*;

#[test]
fn utf16_doc_preserves_fresh_client_ids_and_uses_utf16_offsets() {
    let first = utf16_doc();
    let second = utf16_doc();

    assert_eq!(first.offset_kind(), OffsetKind::Utf16);
    assert_eq!(second.offset_kind(), OffsetKind::Utf16);
    assert_ne!(first.client_id(), second.client_id());
}

#[test]
fn validated_import_source_reuses_one_schema_ranked_canonical_result() {
    use crate::yrs_engine::canonical::{
        reset_canonical_artifact_counts_for_test, reset_canonical_schema_context_count_for_test,
        take_canonical_artifact_counts_for_test, take_canonical_schema_context_count_for_test,
    };

    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let input = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{
                "type": "text",
                "text": "ordered",
                "marks": [{ "type": "bold" }, { "type": "italic" }]
            }]
        }]
    });
    let parsed = from_prosemirror_json(&input, &schema, UnknownTypeMode::Preserve).unwrap();
    let canonical_schema = crate::yrs_engine::canonical::CanonicalSchemaContext::new(&schema);
    let engine = transaction_engine();
    reset_canonical_artifact_counts_for_test();
    reset_canonical_schema_context_count_for_test();
    crate::yrs_engine::observability::reset_full_pass_counts_for_test();

    let input_len = serde_json::to_vec(&input).unwrap().len();
    let validated =
        ValidatedImportDocument::new(parsed, &schema, &canonical_schema, &limits, Some(input_len))
            .unwrap();
    let artifact = validated.canonical_artifact.clone();

    assert_eq!(
        validated.canonical_artifact.value(),
        &json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": "ordered",
                    "marks": [{ "type": "bold" }, { "type": "italic" }]
                }]
            }]
        })
    );
    assert_eq!(
        validated.canonical_artifact.value(),
        &crate::serialize::to_prosemirror_json(&validated.document, &schema)
    );
    let candidate = engine
        .build_candidate_from_document(validated, TransactionOrigin::DocumentImport)
        .unwrap();
    let super::EngineDocumentState::Ready {
        canonical_artifact, ..
    } = candidate.state
    else {
        panic!("validated import candidate must be ready")
    };
    assert!(artifact.ptr_eq(&canonical_artifact));
    assert_eq!(take_canonical_artifact_counts_for_test(), (1, 0));
    assert_eq!(take_canonical_schema_context_count_for_test(), 0);
    let counts = crate::yrs_engine::observability::take_full_pass_counts_for_test();
    assert_eq!(counts.canonical_mark_nodes_visited, 3);
    assert_eq!(counts.canonical_identity_predicate_nodes_visited, 0);
}

#[test]
fn canonicalized_import_projection_is_bound_to_the_final_root() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let input = json!({"type": "doc", "content": [{"type": "paragraph", "content": [{
        "type": "text", "text": "ordered", "marks": [{"type": "italic"}, {"type": "bold"}]
    }]}]});
    let parsed = from_prosemirror_json(&input, &schema, UnknownTypeMode::Preserve).unwrap();
    let original = parsed.clone();
    let canonical_schema = crate::yrs_engine::canonical::CanonicalSchemaContext::new(&schema);
    let admitted =
        ValidatedImportDocument::new(parsed, &schema, &canonical_schema, &limits, None).unwrap();
    assert_ne!(
        original, admitted.document,
        "fixture must exercise mark canonicalization"
    );
    let proof = admitted.validation.table_projection.as_ref().unwrap();
    assert!(proof
        .matching_index(
            &original,
            &crate::schema::schema_fingerprint(&schema),
            &limits
        )
        .is_none());
    assert!(proof
        .matching_index(
            &admitted.document,
            &crate::schema::schema_fingerprint(&schema),
            &limits
        )
        .is_some());
}

#[test]
fn admitted_import_runs_one_validation_certificate_and_render_path() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    let mut engine = transaction_engine();
    reset_full_pass_counts_for_test();
    crate::render::incremental::reset_cached_render_counts_for_test();

    engine
        .import_json(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]},{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();

    let passes = take_full_pass_counts_for_test();
    let render = crate::render::incremental::take_cached_render_counts_for_test();
    assert_eq!(passes.import_model_parses, 1);
    assert_eq!(passes.validated_evidence_constructions, 1);
    assert_eq!(passes.validation_certificate_constructions, 1);
    assert_eq!(passes.document_validations, 1);
    assert_eq!(passes.canonical_mark_validation_attempts, 1);
    assert_eq!(passes.canonical_mark_validation_completions, 1);
    assert_eq!(passes.canonical_projections, 1);
    assert_eq!(passes.canonical_serializations, 0);
    assert_eq!(passes.canonical_hashes, 0);
    assert_eq!(
        passes.render_limit_tree_scans, 0,
        "sealed validation evidence should replace the redundant render node/depth scan"
    );
    assert_eq!(
        render.0, 1,
        "the admitted import should build one render cache"
    );

    let artifact = &engine.derived_state.as_ref().unwrap().canonical_artifact;
    let _ = artifact.sha256();
    assert_eq!(take_full_pass_counts_for_test().canonical_hashes, 1);
    let _ = artifact.sha256();
    assert_eq!(take_full_pass_counts_for_test().canonical_hashes, 0);
}

#[test]
fn admitted_import_hydrates_before_seed_consumers_but_not_selection_only_state() {
    let mut typed_input = import_document_with_unavailable_lookup_seed();
    typed_input
        .apply_typed_transaction(insert_transaction(&typed_input, 65_100))
        .unwrap();
    assert!(typed_input
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_ready_for_test());

    let mut command = import_document_with_unavailable_lookup_seed();
    command
        .apply_command(65_101, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .expect("default-selection command should apply without preparatory selection");
    assert!(command
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_ready_for_test());

    let mut selection = import_document_with_unavailable_lookup_seed();
    selection
        .apply_typed_transaction(TypedTransaction {
            request_id: 65_102,
            base_document_revision: selection.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![],
            selection_intent: SelectionIntent::Set(SelectionInput::All),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    assert!(selection
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_unavailable_for_test());

    let mut rich_local_api = import_document_with_unavailable_lookup_seed();
    rich_local_api
        .apply_typed_transaction_with_result(TypedTransaction {
            request_id: 65_103,
            base_document_revision: rich_local_api.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![],
            selection_intent: SelectionIntent::Preserve,
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    assert!(rich_local_api
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_unavailable_for_test());

    let mut history = import_document_with_unavailable_lookup_seed();
    assert!(history.undo(65_104).unwrap().is_none());
    assert!(history
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_unavailable_for_test());
    history
        .apply_command(65_105, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .unwrap();
    force_lookup_seed_unavailable(&mut history);
    let unavailable_before_undo =
        Arc::clone(&history.derived_state.as_ref().unwrap().mutation_lookup_seed);
    assert!(history.undo(65_106).unwrap().is_some());
    assert!(!Arc::ptr_eq(
        &unavailable_before_undo,
        &history.derived_state.as_ref().unwrap().mutation_lookup_seed
    ));
    let unavailable_before_redo =
        Arc::clone(&history.derived_state.as_ref().unwrap().mutation_lookup_seed);
    assert!(history.redo(65_107).unwrap().is_some());
    assert!(!Arc::ptr_eq(
        &unavailable_before_redo,
        &history.derived_state.as_ref().unwrap().mutation_lookup_seed
    ));
}

#[test]
fn deferred_insert_shape_and_output_bound_eligibility_is_exact() {
    let exact = deferred_insert_fixture(DeferredInsertCase::StrictInteriorEqualMarks);
    assert_eq!(
        exact.execution_admission_kind(),
        ExecutionAdmissionKind::Deferred
    );

    for case in [
        DeferredInsertCase::Empty,
        DeferredInsertCase::LeafBoundary,
        DeferredInsertCase::MarkMismatch,
        DeferredInsertCase::StructuralGrowth,
        DeferredInsertCase::UnavailableUpperBound,
        DeferredInsertCase::OverflowingUpperBound,
        DeferredInsertCase::OneOverOutputLimit,
    ] {
        assert_eq!(
            deferred_insert_fixture(case).execution_admission_kind(),
            ExecutionAdmissionKind::Eager,
            "{case:?}",
        );
    }
}

#[test]
fn eager_semantic_errors_precede_staged_hydration_failure() {
    use crate::yrs_engine::mutation::{
        set_lookup_seed_hydration_failpoint_for_test, LookupSeedHydrationFailpoint,
    };

    for case in eager_pre_admission_error_cases() {
        let mut engine = case.engine;
        let before = atomic_audit(&engine);
        set_lookup_seed_hydration_failpoint_for_test(Some(
            LookupSeedHydrationFailpoint::InitialReservation,
        ));
        let error = engine
            .apply_command(case.request_id, case.command)
            .unwrap_err();
        set_lookup_seed_hydration_failpoint_for_test(None);
        assert_eq!(error, case.expected_error, "{}", case.name);
        assert_eq!(atomic_audit(&engine), before, "{}", case.name);
    }
}

#[test]
fn first_imported_deferred_insert_uses_two_serializations_two_hashes_once() {
    use crate::yrs_engine::mutation::{
        reset_localized_lookup_counts_for_test, take_localized_lookup_counts_for_test,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, reset_prepared_admission_counts_for_test,
        take_full_pass_counts_for_test, take_prepared_admission_counts_for_test,
    };

    let mut engine = import_document_with_unavailable_lookup_seed();
    select_text(&mut engine, 65_199, 2, 2);
    assert!(engine
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_unavailable_for_test());
    reset_full_pass_counts_for_test();
    reset_prepared_admission_counts_for_test();
    reset_localized_lookup_counts_for_test();

    engine
        .apply_command(65_200, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .expect("strict-interior imported insert should apply");

    let passes = take_full_pass_counts_for_test();
    assert_eq!(passes.planner_simulations, 1);
    assert_eq!(passes.document_validations, 1);
    assert_eq!(passes.canonical_serializations, 2);
    assert_eq!(passes.canonical_hashes, 2);
    assert_eq!(take_localized_lookup_counts_for_test(), (0, 1, 1));
    let admission = take_prepared_admission_counts_for_test();
    assert_eq!(admission.staged_seed_preparations, 1);
    assert_eq!(admission.installed_base_seed_publications, 0);
}

#[test]
fn public_insert_uses_eager_admission_after_admissible_resource_limit_change() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, reset_prepared_admission_counts_for_test,
        take_full_pass_counts_for_test, take_prepared_admission_counts_for_test,
    };

    let mut engine = import_document_with_unavailable_lookup_seed();
    select_text(&mut engine, 65_201, 2, 2);
    engine.resource_limits.max_input_bytes -= 1;
    let changed_limits = engine.resource_limits.clone();
    let mut preconfigured = transaction_engine();
    preconfigured.resource_limits = changed_limits;
    preconfigured
        .import_json(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abc"}]}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    select_text(&mut preconfigured, 65_201, 2, 2);
    let command = TypedCommand::InsertText { text: "x".into() };
    let preparation = std::cell::RefCell::new(None);
    assert!(matches!(
        engine
            .plan_command_internal(65_202, command.clone(), Some(&preparation))
            .unwrap(),
        CommandPlan::Transaction(_)
    ));
    assert!(matches!(
        preparation.into_inner().unwrap().execution_admission,
        crate::yrs_engine::prepared_admission::ExecutionSemanticAdmission::Eager(_)
    ));
    let installed = Arc::clone(&engine.derived_state.as_ref().unwrap().mutation_lookup_seed);
    reset_full_pass_counts_for_test();
    reset_prepared_admission_counts_for_test();

    let result = engine.apply_command(65_202, command).unwrap().unwrap();
    let passes = take_full_pass_counts_for_test();
    let counts = take_prepared_admission_counts_for_test();
    let preconfigured_result = preconfigured
        .apply_command(65_202, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .unwrap();

    assert!(result.changed);
    assert_eq!(passes.planner_simulations, 1);
    assert_eq!(passes.document_validations, 4);
    assert_eq!(result, preconfigured_result);
    assert_eq!(engine.document_json(), preconfigured.document_json());
    assert_eq!(engine.document_html(), preconfigured.document_html());
    assert_eq!(
        engine.resolved_selection(),
        preconfigured.resolved_selection()
    );
    assert!(!Arc::ptr_eq(
        &installed,
        &engine.derived_state.as_ref().unwrap().mutation_lookup_seed,
    ));
    assert!(engine
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_ready_for_test());
    assert_eq!(counts.staged_seed_preparations, 1);
    assert_eq!(counts.installed_base_seed_publications, 0);
}

#[test]
fn private_prepared_command_orchestrator_finalizes_deferred_admission_once() {
    use crate::yrs_engine::mutation::{
        reset_localized_lookup_counts_for_test, take_localized_lookup_counts_for_test,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, reset_prepared_admission_counts_for_test,
        take_full_pass_counts_for_test, take_prepared_admission_counts_for_test,
    };
    use crate::yrs_engine::TransactionCommit;

    let mut engine = import_document_with_unavailable_lookup_seed();
    let mut public = import_document_with_unavailable_lookup_seed();
    select_text(&mut engine, 65_260, 2, 2);
    select_text(&mut public, 65_260, 2, 2);
    let installed = Arc::clone(&engine.derived_state.as_ref().unwrap().mutation_lookup_seed);
    let preparation = std::cell::RefCell::new(None);
    reset_full_pass_counts_for_test();
    reset_prepared_admission_counts_for_test();
    reset_localized_lookup_counts_for_test();

    let CommandPlan::Transaction(transaction) = engine
        .plan_command_internal(
            65_261,
            TypedCommand::InsertText { text: "x".into() },
            Some(&preparation),
        )
        .unwrap()
    else {
        panic!("strict-interior imported insert must produce a transaction")
    };
    let proof = preparation
        .into_inner()
        .expect("strict-interior imported insert must retain its exact proof");
    assert!(matches!(
        &proof.execution_admission,
        crate::yrs_engine::prepared_admission::ExecutionSemanticAdmission::Deferred(_)
    ));
    let (commit, result) = engine
        .apply_prepared_command_transaction(
            transaction,
            proof,
            true,
            &mut OutboundUpdateSink::detached(),
        )
        .unwrap();
    let result = result.expect("changed command must return a result");
    let authority_counts = take_compiled_commit_authority_counts_for_test();
    let passes = take_full_pass_counts_for_test();
    let admission = take_prepared_admission_counts_for_test();
    assert_eq!(passes.planner_simulations, 1);
    assert_eq!(passes.document_validations, 1);
    assert_eq!(passes.canonical_serializations, 2);
    assert_eq!(passes.canonical_hashes, 2);
    assert_eq!(take_localized_lookup_counts_for_test(), (0, 1, 1));
    assert_eq!(admission.staged_seed_preparations, 1);
    assert_eq!(admission.staged_identity_materializations, 1);
    assert_eq!(admission.installed_base_seed_publications, 0);
    assert_eq!(admission.deferred_capsules_created, 1);
    assert_eq!(admission.deferred_capsules_finalized, 1);
    assert_eq!(authority_counts, (1, 1));
    assert!(!Arc::ptr_eq(
        &installed,
        &engine.derived_state.as_ref().unwrap().mutation_lookup_seed
    ));
    assert!(engine
        .derived_state
        .as_ref()
        .unwrap()
        .mutation_lookup_seed
        .is_ready_for_test());
    assert_eq!(
        commit,
        TransactionCommit {
            request_id: result.request_id,
            changed: result.changed,
            document_revision: result.document_revision,
            state_revision: result.state_revision,
            origin: result.origin,
        }
    );

    let public_result = public
        .apply_command(65_261, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .unwrap();
    assert_eq!(result, public_result);
    assert_eq!(engine.document_json(), public.document_json());
    assert_eq!(engine.resolved_selection(), public.resolved_selection());
    assert_eq!(engine.stored_marks(), public.stored_marks());
    assert_eq!(engine.can_undo(), public.can_undo());
    assert_eq!(engine.can_redo(), public.can_redo());
    let private_undo = engine.undo(65_262).unwrap().unwrap();
    let public_undo = public.undo(65_262).unwrap().unwrap();
    assert_eq!(private_undo, public_undo);
    assert_eq!(engine.document_json(), public.document_json());
    assert_eq!(engine.resolved_selection(), public.resolved_selection());
    assert_eq!(engine.stored_marks(), public.stored_marks());
    assert_eq!(engine.can_undo(), public.can_undo());
    assert_eq!(engine.can_redo(), public.can_redo());
}

#[test]
fn first_imported_prepared_insert_reuses_the_before_history_charge() {
    use crate::model::{
        reset_history_snapshot_retained_bytes_traversals_for_test,
        take_history_snapshot_retained_bytes_traversals_for_test,
    };

    let mut engine = import_document_with_unavailable_lookup_seed();
    select_text(&mut engine, 65_263, 2, 2);
    reset_history_snapshot_retained_bytes_traversals_for_test();

    engine
        .apply_command(65_264, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .expect("strict-interior imported insert must apply");

    assert_eq!(
        take_history_snapshot_retained_bytes_traversals_for_test(),
        1,
        "history admission reuses the before charge and traverses only the prepared after document"
    );
}

#[test]
fn first_imported_prepared_insert_uses_localized_history_render_evidence() {
    use crate::render::incremental::{
        reset_cached_render_counts_for_test, reset_localized_render_transition_counts_for_test,
        take_cached_render_counts_for_test, take_localized_render_transition_counts_for_test,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    let mut engine = import_document_with_unavailable_lookup_seed();
    select_text(&mut engine, 65_265, 2, 2);
    reset_full_pass_counts_for_test();
    reset_cached_render_counts_for_test();
    reset_localized_render_transition_counts_for_test();

    engine
        .apply_command(65_266, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .expect("strict-interior imported insert must apply");

    let passes = take_full_pass_counts_for_test();
    let localized = take_localized_render_transition_counts_for_test();
    assert_eq!((passes.render_limit_tree_scans, localized), (0, (1, 1, 0)));
    assert_eq!(
        (
            passes.position_map_clones,
            passes.position_map_compactions,
            passes.rendered_text_derivations,
        ),
        (1, 1, 0),
        "sealed strict-interior evidence must incrementally derive the candidate map and text",
    );
    assert_eq!(take_cached_render_counts_for_test(), (0, 1, 1, 0, 0));
}

#[test]
fn tampered_localized_history_render_evidence_falls_back_with_exact_results() {
    use crate::render::incremental::{
        reset_cached_render_counts_for_test, reset_localized_render_transition_counts_for_test,
        take_cached_render_counts_for_test, take_localized_render_transition_counts_for_test,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    use crate::yrs_engine::prepared_admission::{
        DeferredCommandAdmission, ExecutionSemanticAdmission,
    };

    for case in DeferredCommandAdmission::history_render_tamper_cases_for_test() {
        let mut actual = import_document_with_unavailable_lookup_seed();
        let mut expected = import_document_with_unavailable_lookup_seed();
        select_text(&mut actual, 65_267, 2, 2);
        select_text(&mut expected, 65_267, 2, 2);
        let command = TypedCommand::InsertText { text: "x".into() };
        let preparation = std::cell::RefCell::new(None);
        let CommandPlan::Transaction(transaction) = actual
            .plan_command_internal(65_268, command.clone(), Some(&preparation))
            .unwrap()
        else {
            panic!("strict-interior imported insert must produce a transaction")
        };
        let mut proof = preparation.into_inner().unwrap();
        let ExecutionSemanticAdmission::Deferred(deferred) = &mut proof.execution_admission else {
            panic!("strict-interior imported insert must retain deferred evidence")
        };
        deferred.tamper_history_render_for_test(case);
        reset_full_pass_counts_for_test();
        reset_cached_render_counts_for_test();
        reset_localized_render_transition_counts_for_test();

        let actual_result = actual
            .apply_prepared_command_transaction(
                transaction,
                proof,
                true,
                &mut OutboundUpdateSink::detached(),
            )
            .unwrap()
            .1
            .unwrap();
        let passes = take_full_pass_counts_for_test();
        let cached = take_cached_render_counts_for_test();
        let localized = take_localized_render_transition_counts_for_test();
        let expected_result = expected.apply_command(65_268, command).unwrap().unwrap();

        assert_eq!(actual_result, expected_result, "{case}");
        assert_eq!(actual.document_json(), expected.document_json(), "{case}");
        assert_eq!(
            actual.resolved_selection(),
            expected.resolved_selection(),
            "{case}"
        );
        assert_eq!(actual.can_undo(), expected.can_undo(), "{case}");
        assert_eq!(passes.render_limit_tree_scans, 1, "{case}");
        assert_eq!(cached, (0, 1, 1, 0, 0), "{case}");
        assert_eq!(localized, (1, 0, 1), "{case}");
    }
}

#[test]
fn localized_history_render_errors_fall_back_with_exact_results() {
    use crate::render::incremental::{
        reset_cached_render_counts_for_test, reset_localized_render_transition_counts_for_test,
        set_localized_render_failure_stage_for_test, take_cached_render_counts_for_test,
        take_localized_render_transition_counts_for_test, LocalizedRenderFailureStage,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    for stage in [
        LocalizedRenderFailureStage::Allocation,
        LocalizedRenderFailureStage::Resource,
        LocalizedRenderFailureStage::Position,
        LocalizedRenderFailureStage::Invariant,
    ] {
        let mut actual = import_document_with_unavailable_lookup_seed();
        let mut expected = import_document_with_unavailable_lookup_seed();
        let two_blocks = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abc"}]},{"type":"paragraph","content":[{"type":"text","text":"suffix"}]}]}"#;
        actual
            .import_json(two_blocks, TransactionOrigin::DocumentImport)
            .unwrap();
        expected
            .import_json(two_blocks, TransactionOrigin::DocumentImport)
            .unwrap();
        select_text(&mut actual, 65_269, 2, 2);
        select_text(&mut expected, 65_269, 2, 2);
        reset_full_pass_counts_for_test();
        reset_cached_render_counts_for_test();
        reset_localized_render_transition_counts_for_test();
        set_localized_render_failure_stage_for_test(Some(stage));

        let actual_result = actual
            .apply_command(65_270, TypedCommand::InsertText { text: "x".into() })
            .unwrap()
            .unwrap();
        set_localized_render_failure_stage_for_test(None);
        let passes = take_full_pass_counts_for_test();
        let cached = take_cached_render_counts_for_test();
        let localized = take_localized_render_transition_counts_for_test();
        let expected_result = expected
            .apply_command(65_270, TypedCommand::InsertText { text: "x".into() })
            .unwrap()
            .unwrap();

        assert_eq!(actual_result, expected_result, "{stage:?}");
        assert_eq!(
            actual.document_json(),
            expected.document_json(),
            "{stage:?}"
        );
        assert_eq!(
            actual.resolved_selection(),
            expected.resolved_selection(),
            "{stage:?}"
        );
        assert_eq!(actual.can_undo(), expected.can_undo(), "{stage:?}");
        assert_eq!(passes.render_limit_tree_scans, 1, "{stage:?}");
        assert_eq!(cached, (0, 1, 1, 0, 0), "{stage:?}");
        assert_eq!(localized, (1, 0, 1), "{stage:?}");
    }
}

#[test]
fn private_prepared_eager_noninsert_uses_staged_context_without_identity() {
    use crate::yrs_engine::observability::{
        reset_prepared_admission_counts_for_test, take_prepared_admission_counts_for_test,
    };
    use crate::yrs_engine::TransactionCommit;

    let mut engine = import_document_with_unavailable_lookup_seed();
    let mut public = import_document_with_unavailable_lookup_seed();
    select_text(&mut engine, 65_263, 0, 2);
    select_text(&mut public, 65_263, 0, 2);
    let preparation = std::cell::RefCell::new(None);
    reset_prepared_admission_counts_for_test();
    let command = TypedCommand::ToggleMark {
        mark_type: "bold".into(),
    };
    let CommandPlan::Transaction(transaction) = engine
        .plan_command_internal(65_264, command.clone(), Some(&preparation))
        .unwrap()
    else {
        panic!("range mark command must produce a transaction")
    };
    let proof = preparation.into_inner().unwrap();
    assert!(matches!(
        &proof.execution_admission,
        crate::yrs_engine::prepared_admission::ExecutionSemanticAdmission::Eager(_)
    ));

    let (commit, result) = engine
        .apply_prepared_command_transaction(
            transaction,
            proof,
            true,
            &mut OutboundUpdateSink::detached(),
        )
        .unwrap();
    let result = result.unwrap();
    let admission = take_prepared_admission_counts_for_test();
    assert_eq!(admission.staged_seed_preparations, 1);
    assert_eq!(admission.staged_identity_materializations, 0);
    assert_eq!(admission.installed_base_seed_publications, 0);
    assert_eq!(
        commit,
        TransactionCommit {
            request_id: result.request_id,
            changed: result.changed,
            document_revision: result.document_revision,
            state_revision: result.state_revision,
            origin: result.origin,
        }
    );

    let public_result = public.apply_command(65_264, command).unwrap().unwrap();
    assert_eq!(result, public_result);
    assert_eq!(engine.document_json(), public.document_json());
    assert_eq!(engine.resolved_selection(), public.resolved_selection());
    assert_eq!(engine.stored_marks(), public.stored_marks());
    assert_eq!(engine.can_undo(), public.can_undo());
    assert_eq!(engine.can_redo(), public.can_redo());
}

#[test]
fn private_prepared_history_error_precedes_staged_hydration_failure() {
    use crate::yrs_engine::mutation::{
        set_lookup_seed_hydration_failpoint_for_test, LookupSeedHydrationFailpoint,
    };
    use crate::yrs_engine::observability::{
        reset_prepared_admission_counts_for_test, take_prepared_admission_counts_for_test,
    };

    let limits = crate::yrs_engine::EditingLimits {
        max_derived_output_bytes: 100,
        ..crate::yrs_engine::EditingLimits::default()
    };
    let mut engine = transaction_engine_with_editing_limits(limits);
    engine
        .import_json(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abc"}]}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    select_text(&mut engine, 65_265, 2, 2);
    engine.derived_state.as_mut().unwrap().canonical_artifact = engine
        .derived_state
        .as_ref()
        .unwrap()
        .canonical_artifact
        .with_admission_upper_bound_for_test(usize::MAX);
    let expected_actual = super::history_metadata_bytes(engine.stored_marks(), "prosemirror") * 2;
    let preparation = std::cell::RefCell::new(None);
    let CommandPlan::Transaction(transaction) = engine
        .plan_command_internal(
            65_266,
            TypedCommand::InsertText { text: "x".into() },
            Some(&preparation),
        )
        .unwrap()
    else {
        panic!("insert command must produce a transaction")
    };
    let proof = preparation.into_inner().unwrap();
    let installed = Arc::clone(&engine.derived_state.as_ref().unwrap().mutation_lookup_seed);
    let before = atomic_audit(&engine);
    reset_prepared_admission_counts_for_test();
    set_lookup_seed_hydration_failpoint_for_test(Some(
        LookupSeedHydrationFailpoint::InitialReservation,
    ));
    let error = engine
        .apply_prepared_command_transaction(
            transaction,
            proof,
            true,
            &mut OutboundUpdateSink::detached(),
        )
        .unwrap_err();
    set_lookup_seed_hydration_failpoint_for_test(None);

    assert_eq!(
        error,
        crate::yrs_engine::OperationError::document_limit_exceeded(
            65_266,
            None,
            "maxDerivedOutputBytes",
            100,
            expected_actual as u64,
        )
    );
    assert_eq!(atomic_audit(&engine), before);
    assert!(Arc::ptr_eq(
        &installed,
        &engine.derived_state.as_ref().unwrap().mutation_lookup_seed
    ));
    let admission = take_prepared_admission_counts_for_test();
    assert_eq!(admission.staged_seed_preparations, 0);
    assert_eq!(admission.installed_base_seed_publications, 0);
}

#[test]
fn private_prepared_deferred_compiler_failure_is_prewrite_and_atomic() {
    use crate::yrs_engine::compiler::{set_atomic_failpoint_for_test, AtomicFailpoint};
    use crate::yrs_engine::observability::{
        reset_prepared_admission_counts_for_test, take_prepared_admission_counts_for_test,
    };

    let mut engine = import_document_with_unavailable_lookup_seed();
    select_text(&mut engine, 65_267, 2, 2);
    let preparation = std::cell::RefCell::new(None);
    let CommandPlan::Transaction(transaction) = engine
        .plan_command_internal(
            65_268,
            TypedCommand::InsertText { text: "x".into() },
            Some(&preparation),
        )
        .unwrap()
    else {
        panic!("strict-interior imported insert must produce a transaction")
    };
    let proof = preparation.into_inner().unwrap();
    assert!(matches!(
        &proof.execution_admission,
        crate::yrs_engine::prepared_admission::ExecutionSemanticAdmission::Deferred(_)
    ));
    let installed = Arc::clone(&engine.derived_state.as_ref().unwrap().mutation_lookup_seed);
    let before = atomic_audit(&engine);
    reset_prepared_admission_counts_for_test();
    set_atomic_failpoint_for_test(Some(AtomicFailpoint::SemanticCompilation));
    let error = engine
        .apply_prepared_command_transaction(
            transaction,
            proof,
            true,
            &mut OutboundUpdateSink::detached(),
        )
        .unwrap_err();
    set_atomic_failpoint_for_test(None);

    assert_eq!(error.code, "ENGINE_INVARIANT_FAILED");
    assert_eq!(atomic_audit(&engine), before);
    assert!(Arc::ptr_eq(
        &installed,
        &engine.derived_state.as_ref().unwrap().mutation_lookup_seed
    ));
    let admission = take_prepared_admission_counts_for_test();
    assert_eq!(admission.staged_seed_preparations, 1);
    assert_eq!(admission.staged_identity_materializations, 1);
    assert_eq!(admission.installed_base_seed_publications, 0);
    assert_eq!(admission.deferred_capsules_finalized, 1);
}

#[test]
fn eager_non_insert_first_mutations_do_not_materialize_base_identity() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, reset_prepared_admission_counts_for_test,
        take_full_pass_counts_for_test, take_prepared_admission_counts_for_test,
    };

    let mut toggle = import_document_with_unavailable_lookup_seed();
    select_text(&mut toggle, 65_201, 0, 2);
    reset_full_pass_counts_for_test();
    reset_prepared_admission_counts_for_test();
    toggle
        .apply_command(
            65_202,
            TypedCommand::ToggleMark {
                mark_type: "bold".into(),
            },
        )
        .unwrap()
        .unwrap();
    let toggle_passes = take_full_pass_counts_for_test();
    let toggle_admission = take_prepared_admission_counts_for_test();
    assert_eq!(toggle_passes.canonical_serializations, 3);
    assert_eq!(toggle_passes.canonical_hashes, 2);
    assert_eq!(toggle_admission.staged_identity_materializations, 0);

    let mut wrap = import_document_with_unavailable_lookup_seed();
    reset_full_pass_counts_for_test();
    reset_prepared_admission_counts_for_test();
    wrap.apply_command(
        65_203,
        TypedCommand::WrapInList {
            list_type: "bulletList".into(),
            item_type: "listItem".into(),
        },
    )
    .unwrap()
    .unwrap();
    let wrap_passes = take_full_pass_counts_for_test();
    let wrap_admission = take_prepared_admission_counts_for_test();
    assert_eq!(wrap_passes.canonical_serializations, 3);
    assert_eq!(wrap_passes.canonical_hashes, 2);
    assert_eq!(wrap_admission.staged_identity_materializations, 0);

    let mut undo = import_document_with_unavailable_lookup_seed();
    undo.apply_command(65_204, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .unwrap();
    force_lookup_seed_unavailable(&mut undo);
    reset_full_pass_counts_for_test();
    reset_prepared_admission_counts_for_test();
    undo.undo(65_205).unwrap().unwrap();
    let undo_passes = take_full_pass_counts_for_test();
    let undo_admission = take_prepared_admission_counts_for_test();
    assert_eq!(undo_passes.canonical_serializations, 0);
    assert_eq!(undo_passes.canonical_hashes, 0);
    assert_eq!(undo_admission.staged_identity_materializations, 0);

    let mut redo = import_document_with_unavailable_lookup_seed();
    redo.apply_command(65_206, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .unwrap();
    redo.undo(65_207).unwrap().unwrap();
    force_lookup_seed_unavailable(&mut redo);
    reset_full_pass_counts_for_test();
    reset_prepared_admission_counts_for_test();
    redo.redo(65_208).unwrap().unwrap();
    let redo_passes = take_full_pass_counts_for_test();
    let redo_admission = take_prepared_admission_counts_for_test();
    assert_eq!(redo_passes.canonical_serializations, 0);
    assert_eq!(redo_passes.canonical_hashes, 0);
    assert_eq!(redo_admission.staged_identity_materializations, 0);
}

include!("import_admission/staged_authority.rs");

#[test]
fn a_table_import_performs_each_document_wide_pass_once() {
    use crate::render::incremental::{
        reset_cached_render_counts_for_test, take_cached_render_counts_for_test,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const ROWS: usize = 1000;
    const COLUMNS: usize = 20;
    const REQUEST: u64 = 120;
    let source =
        crate::test_support::large_table_fixture::plain_table_document(ROWS, COLUMNS).to_string();
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema: crate::schema::presets::prosemirror_table_schema(),
        fragment_name: "prosemirror".into(),
        initialization_mode: crate::yrs_engine::InitializationMode::LocalEmpty,
        resource_limits: ResourceLimits::default(),
        editing_limits: crate::yrs_engine::EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap();
    reset_full_pass_counts_for_test();
    reset_cached_render_counts_for_test();
    crate::yrs_engine::observability::take_node_json_projections_for_test();
    crate::yrs_engine::position::reset_relative_position_traversal_counts_for_test();
    engine
        .prepare_root_replacement_json(
            REQUEST,
            &source,
            crate::yrs_engine::ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    let renders = take_cached_render_counts_for_test();
    let relative_walks =
        crate::yrs_engine::position::take_relative_position_traversal_counts_for_test();
    eprintln!("table import: {counts:#?}; cached renders: {renders:?}");
    assert_eq!(
        counts.json_value_deserializations, 0,
        "plain table replacement must not allocate an intermediate JSON value tree"
    );
    assert_eq!(
        relative_walks,
        (0, 0, 0),
        "the imported table's initial cursor must use the candidate branch index"
    );
    let state = engine.derived_state.as_ref().unwrap();
    let txn = engine.doc.transact();
    let fragment = txn.get_xml_fragment(engine.fragment_name.as_str()).unwrap();
    assert_eq!(
        state.relative_selection,
        crate::yrs_engine::derived_state::operation_result_to_relative(
            &txn,
            &fragment,
            &state.legacy_selection,
            &engine.schema,
            None,
        ),
        "the committed cursor must preserve the exact root-walk anchor and association"
    );
    assert_eq!(
        crate::yrs_engine::observability::take_node_json_projections_for_test(),
        0,
        "root import must lower admitted nodes without projecting JSON"
    );
    assert_eq!(
        counts.canonical_projections, 0,
        "root replacement must not build an intermediate canonical JSON tree"
    );
    assert_eq!(counts.document_validations, 1);
    assert_eq!(counts.rendered_text_derivations, 0);
    assert_eq!(renders.0, 1);
    assert!(counts.canonical_serializations <= 1, "{counts:#?}");
    assert_eq!(counts.table_projection_derivations, 0);
    assert_eq!(
        engine.document_json().unwrap(),
        serde_json::from_str::<serde_json::Value>(&source).unwrap()
    );
}

#[test]
fn canonical_child_reuse_preserves_reset_and_undoable_root_replacements() {
    use crate::yrs_engine::observability::take_node_json_projections_for_test;
    use crate::yrs_engine::ReplacementHistory;
    const REQUEST: u64 = 65_300;
    let source = serde_json::json!({"type":"doc","content":[
        {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"hé🙂","marks":[{"type":"bold"}]}]},
        {"type":"blockquote","content":[{"type":"paragraph","content":[{"type":"text","text":"nested"}]}]},
        {"type":"unknown","attrs":{"nested":[true,null]}}
    ]});
    let schema = tiptap_schema();
    let parsed = crate::serialize::from_prosemirror_json(
        &source,
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    let expected = crate::serialize::to_prosemirror_json(&parsed, &schema);
    for history in [
        ReplacementHistory::ResetAndClear,
        ReplacementHistory::UndoableBoundary,
    ] {
        let mut engine = transaction_engine();
        let before = engine.document_json().unwrap();
        take_node_json_projections_for_test();
        engine
            .prepare_root_replacement_json(REQUEST, &source.to_string(), history)
            .unwrap();
        assert_eq!(
            take_node_json_projections_for_test(),
            0,
            "an exact admitted replacement must lower nodes without projecting JSON"
        );
        assert_eq!(engine.document_json().unwrap(), expected);
        let undone = engine.undo(REQUEST + 1).unwrap();
        if history == ReplacementHistory::UndoableBoundary {
            assert!(undone.is_some());
            assert_eq!(engine.document_json().unwrap(), before);
            assert!(engine.redo(REQUEST + 2).unwrap().is_some());
            assert_eq!(engine.document_json().unwrap(), expected);
        } else {
            assert!(undone.is_none(), "reset must retain no undo entry");
        }
    }
}

#[test]
fn compact_root_json_matches_legacy_import_documents_fees_and_errors() {
    use crate::yrs_engine::{ReplacementHistory, RootReplacementError};
    let sources = [
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a🦀é"}]}]}"#,
        r#"{"ty\u0070e":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a\n\uD83E\uDD80"}]}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph"},{"type":"paragraph","content":[]}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"ignored","content":[{"type":"paragraph"}]}]}]}"#,
        r#"{"type":"doc","content":[{"type":"horizontalRule","content":[{"type":"paragraph"}]}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph","attrs":{"textAlign":"right"},"content":[{"type":"text","text":"rich","marks":[{"type":"bold"}]}]}]}"#,
        r#"{"type":"doc","content":[{"type":"future","extra":null,"content":[{"type":"paragraph"}]}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":null}]}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph","content":null}]}"#,
        r#"{"type":"doc","type":"paragraph","content":[{"type":"text","text":"duplicate"}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":null,"text":"last wins"}]}]}"#,
        r#"{"type":"doc","content":[{"type":"paragraph"}],"extra":1e400}"#,
        r#"{"type":"doc","content":[{"type":"paragraph"}]} trailing"#,
        r#"{"type":"doc","content":[{"type":"paragraph"}]"#,
        r#"{"type":"doc","content":[null]}"#,
        r#"{"type":"doc","content":[{"type":"h1","content":[{"type":"text","text":"legacy"}]}]}"#,
    ];
    for input in sources {
        for limits in [
            ResourceLimits::default(),
            ResourceLimits {
                max_document_nodes: 2,
                ..ResourceLimits::default()
            },
            ResourceLimits {
                max_document_depth: 2,
                ..ResourceLimits::default()
            },
            ResourceLimits {
                max_input_bytes: input.len() - 1,
                ..ResourceLimits::default()
            },
        ] {
            let input_limit = limits.max_input_bytes;
            let configuration_limits = ResourceLimits {
                max_input_bytes: ResourceLimits::default().max_input_bytes,
                ..limits
            };
            let mut actual = transaction_engine_with_resource_limits_and_mode(
                configuration_limits.clone(),
                crate::yrs_engine::InitializationMode::LocalEmpty,
            );
            let mut oracle = transaction_engine_with_resource_limits_and_mode(
                configuration_limits,
                crate::yrs_engine::InitializationMode::LocalEmpty,
            );
            actual.resource_limits.max_input_bytes = input_limit;
            oracle.resource_limits.max_input_bytes = input_limit;
            let before = actual.document_json();
            let revision = actual.revision();
            let expected = crate::serialize::json_in::with_legacy_json_for_test(|| {
                oracle.prepare_root_replacement_json(
                    65_400,
                    input,
                    ReplacementHistory::ResetAndClear,
                )
            });
            let result = actual.prepare_root_replacement_json(
                65_400,
                input,
                ReplacementHistory::ResetAndClear,
            );
            match (result, expected) {
                (Ok(_), Ok(_)) => {
                    assert_eq!(actual.document(), oracle.document(), "document: {input}");
                    let current = actual.derived_state.as_ref().unwrap();
                    let expected = oracle.derived_state.as_ref().unwrap();
                    assert_eq!(
                        current.document.history_snapshot_retained_bytes(),
                        expected.document.history_snapshot_retained_bytes(),
                        "model fee: {input}"
                    );
                    assert_eq!(
                        current
                            .canonical_artifact
                            .history_snapshot_retained_charge(),
                        expected
                            .canonical_artifact
                            .history_snapshot_retained_charge(),
                        "artifact fee: {input}"
                    );
                }
                (Err(actual_error), Err(expected_error)) => {
                    match (actual_error, expected_error) {
                        (
                            RootReplacementError::Admission(actual),
                            RootReplacementError::Admission(expected),
                        ) => assert_eq!(actual, expected, "admission error: {input}"),
                        (
                            RootReplacementError::Transaction(actual),
                            RootReplacementError::Transaction(expected),
                        ) => assert_eq!(actual, expected, "transaction error: {input}"),
                        (actual, expected) => {
                            panic!("error phase for {input}: {actual:?}; {expected:?}")
                        }
                    }
                    assert_eq!(actual.document_json(), before, "rejected state: {input}");
                    assert_eq!(actual.revision(), revision, "rejected revision: {input}");
                    assert!(!actual.can_undo(), "rejected history: {input}");
                }
                (actual, expected) => {
                    panic!("import parity for {input}: actual={actual:?}; expected={expected:?}")
                }
            }
        }
    }
}

#[test]
fn compact_root_json_preserves_near_ceiling_fallback_on_a_small_stack() {
    const SMALL_STACK_BYTES: usize = 128 * 1024;
    const ADMITTED_DEPTH: usize = 256;
    for nesting in [7, 8, 60, 64, 70] {
        std::thread::Builder::new()
            .stack_size(SMALL_STACK_BYTES)
            .spawn(move || {
                let mut input = String::from(r#"{"type":"doc","content":["#);
                for _ in 0..nesting {
                    input.push_str(r#"{"type":"blockquote","content":["#);
                }
                input.push_str(r#"{"type":"paragraph","content":[{"type":"text","text":"deep"}]}"#);
                for _ in 0..=nesting {
                    input.push_str("]}");
                }
                let limits = ResourceLimits {
                    max_document_depth: ADMITTED_DEPTH,
                    ..ResourceLimits::default()
                };
                let mut actual = transaction_engine_with_resource_limits_and_mode(
                    limits.clone(),
                    crate::yrs_engine::InitializationMode::LocalEmpty,
                );
                let mut oracle = transaction_engine_with_resource_limits_and_mode(
                    limits,
                    crate::yrs_engine::InitializationMode::LocalEmpty,
                );
                actual
                    .prepare_root_replacement_json(
                        65_401,
                        &input,
                        crate::yrs_engine::ReplacementHistory::ResetAndClear,
                    )
                    .unwrap();
                crate::serialize::json_in::with_legacy_json_for_test(|| {
                    oracle.prepare_root_replacement_json(
                        65_401,
                        &input,
                        crate::yrs_engine::ReplacementHistory::ResetAndClear,
                    )
                })
                .unwrap();
                assert_eq!(actual.document(), oracle.document(), "nesting {nesting}");
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

#[test]
fn compact_root_json_preserves_projected_schema_fallback() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    let schema = crate::schema::Schema::from_json(&json!({
        "nodes": [
            {"name":"doc","content":"block+","role":"doc"},
            {"name":"body","content":"inline*","group":"block","role":"textBlock",
             "json":{"type":"para","attrs":{}}},
            {"name":"text","group":"inline","role":"text"}
        ], "marks": []
    }))
    .unwrap();
    for node_type in ["body", "para"] {
        let input = json!({"type":"doc","content":[{"type":node_type,
            "content":[{"type":"text","text":"projected"}]}]})
        .to_string();
        let make_engine = || {
            YrsDocumentEngine::new(YrsEngineConfig {
                schema: schema.clone(),
                fragment_name: "prosemirror".into(),
                initialization_mode: crate::yrs_engine::InitializationMode::LocalEmpty,
                resource_limits: ResourceLimits::default(),
                editing_limits: crate::yrs_engine::EditingLimits::default(),
                max_length: None,
                scope: None,
            })
            .unwrap()
        };
        let mut actual = make_engine();
        let mut oracle = make_engine();
        reset_full_pass_counts_for_test();
        actual
            .prepare_root_replacement_json(
                65_402,
                &input,
                crate::yrs_engine::ReplacementHistory::ResetAndClear,
            )
            .unwrap();
        assert_eq!(
            take_full_pass_counts_for_test().json_value_deserializations,
            1,
            "projected native type and wire alias must use the full parser: {node_type}"
        );
        crate::serialize::json_in::with_legacy_json_for_test(|| {
            oracle.prepare_root_replacement_json(
                65_402,
                &input,
                crate::yrs_engine::ReplacementHistory::ResetAndClear,
            )
        })
        .unwrap();
        assert_eq!(actual.document(), oracle.document(), "{node_type}");
        assert_eq!(
            actual.document_json(),
            oracle.document_json(),
            "{node_type}"
        );
    }
}

#[test]
fn compact_root_json_preserves_history_boundaries_and_replay() {
    use crate::yrs_engine::{EditingLimits, ReplacementHistory};
    let snapshot_metadata = super::history_metadata_bytes(None, "prosemirror");
    let standalone = snapshot_metadata * 2;
    let retained_pair = standalone * 2;
    for limit in [
        standalone - 1,
        standalone,
        retained_pair - 1,
        retained_pair,
        EditingLimits::default().max_derived_output_bytes,
    ] {
        let limits = EditingLimits {
            max_derived_output_bytes: limit,
            ..EditingLimits::default()
        };
        let mut actual = transaction_engine_with_editing_limits(limits.clone());
        let mut oracle = transaction_engine_with_editing_limits(limits);
        for (index, text) in ["first", "second"].into_iter().enumerate() {
            let request = 65_410 + index as u64;
            let input = json!({"type":"doc","content":[{"type":"paragraph",
                "content":[{"type":"text","text":text}]}]})
            .to_string();
            let before = atomic_audit(&actual);
            let result = actual.prepare_root_replacement_json(
                request,
                &input,
                ReplacementHistory::UndoableBoundary,
            );
            let expected = crate::serialize::json_in::with_legacy_json_for_test(|| {
                oracle.prepare_root_replacement_json(
                    request,
                    &input,
                    ReplacementHistory::UndoableBoundary,
                )
            });
            match (result, expected) {
                (Ok(_), Ok(_)) => assert!(limit >= standalone),
                (
                    Err(crate::yrs_engine::RootReplacementError::Transaction(actual_error)),
                    Err(crate::yrs_engine::RootReplacementError::Transaction(expected_error)),
                ) => {
                    assert_eq!(actual_error, expected_error, "limit {limit}");
                    assert_eq!(limit, standalone - 1);
                    assert_eq!(atomic_audit(&actual), before, "atomic rejection at {limit}");
                }
                (result, expected) => panic!("history parity at {limit}: {result:?}; {expected:?}"),
            }
            assert_eq!(actual.document(), oracle.document(), "limit {limit}");
            assert_eq!(actual.can_undo(), oracle.can_undo(), "limit {limit}");
        }
        for request in 65_420..65_422 {
            assert_eq!(
                actual.undo(request).unwrap(),
                oracle.undo(request).unwrap(),
                "limit {limit}"
            );
            assert_eq!(actual.document(), oracle.document(), "undo at {limit}");
        }
        for request in 65_422..65_424 {
            assert_eq!(
                actual.redo(request).unwrap(),
                oracle.redo(request).unwrap(),
                "limit {limit}"
            );
            assert_eq!(actual.document(), oracle.document(), "redo at {limit}");
        }
    }
}
