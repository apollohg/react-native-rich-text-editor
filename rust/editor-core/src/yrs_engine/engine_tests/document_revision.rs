use super::*;
use crate::native_transaction_bridge::NativeTransactionBridge;
use crate::tables::commands::{TableCommand, TableEdge};
use crate::test_support::large_table_fixture::{plain_table_document, session_with_document};
use crate::yrs_engine::engine::DocumentChangeScope;
use crate::yrs_engine::{EditingLimits, InitializationMode};

#[test]
fn every_document_revision_records_exactly_one_scope() {
    const SIDE: usize = 3;
    const OWNER: u64 = 91;
    const REQUEST: u64 = 92;
    let mut session = session_with_document(&plain_table_document(SIDE, SIDE));
    session.engine.scope = Some(crate::yrs_engine::DocumentScope {
        document_id: "scope-test".into(),
        lineage_id: "scope-lineage".into(),
    });
    let saved = session.engine.export_snapshot().unwrap();
    let mut previous = session.engine.revision();
    let mut document_scope = session.engine.document_scope_revision();
    let check = |engine: &YrsDocumentEngine,
                 previous: &mut u64,
                 document_scope: &mut u64,
                 local: bool,
                 label: &str| {
        assert_eq!(
            engine.revision(),
            *previous + 1,
            "{label}: one revision advances"
        );
        assert_eq!(
            engine.last_recorded_revision,
            engine.revision(),
            "{label}: scope is recorded"
        );
        assert_eq!(
            engine.recorded_change_count,
            engine.revision(),
            "{label}: exactly one recording per advance"
        );
        if local {
            assert_eq!(
                engine.document_scope_revision(),
                *document_scope,
                "{label}: local edit preserves structure revision"
            );
            assert!(matches!(
                engine.last_change_scope,
                DocumentChangeScope::Textblock { .. }
            ));
        } else {
            assert_eq!(
                engine.document_scope_revision(),
                engine.revision(),
                "{label}: document scope invalidated"
            );
            assert_eq!(engine.last_change_scope, DocumentChangeScope::Document);
        }
        *previous = engine.revision();
        *document_scope = engine.document_scope_revision();
    };
    let epoch = session.pin_position_epoch(OWNER, previous).unwrap();
    NativeTransactionBridge::new(&mut session).submit_native_intent(&json!({
        "version": 1, "requestId": REQUEST.to_string(), "ownerId": OWNER.to_string(),
        "positionEpoch": epoch.to_string(), "intent": {"type":"insertText","anchor":0,"head":0,"text":"x"}
    }).to_string()).unwrap();
    check(
        &session.engine,
        &mut previous,
        &mut document_scope,
        true,
        "keystroke",
    );
    session
        .engine
        .apply_command(
            REQUEST + 1,
            TypedCommand::Table(TableCommand::AddTableRow {
                side: TableEdge::After,
            }),
        )
        .unwrap()
        .unwrap();
    check(
        &session.engine,
        &mut previous,
        &mut document_scope,
        false,
        "row insertion",
    );
    session.engine.undo(REQUEST + 2).unwrap();
    check(
        &session.engine,
        &mut previous,
        &mut document_scope,
        false,
        "undo",
    );
    session.engine.redo(REQUEST + 3).unwrap();
    check(
        &session.engine,
        &mut previous,
        &mut document_scope,
        false,
        "redo",
    );
    let mut peer = YrsDocumentEngine::new(YrsEngineConfig {
        schema: session.engine.schema().clone(),
        fragment_name: session.engine.fragment_name.clone(),
        initialization_mode: InitializationMode::AwaitRemote,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap();
    peer.apply_remote_update_v1(REQUEST + 4, &session.engine.encoded_state().unwrap())
        .unwrap();
    peer.apply_command(
        REQUEST + 5,
        TypedCommand::InsertText {
            text: "remote".into(),
        },
    )
    .unwrap()
    .unwrap();
    session
        .engine
        .apply_remote_update_v1(REQUEST + 6, &peer.encoded_state().unwrap())
        .unwrap();
    check(
        &session.engine,
        &mut previous,
        &mut document_scope,
        false,
        "remote",
    );
    session
        .replace_document_json(
            REQUEST + 7,
            &plain_table_document(SIDE + 1, SIDE).to_string(),
            crate::yrs_engine::ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    check(
        &session.engine,
        &mut previous,
        &mut document_scope,
        false,
        "import",
    );
    session.engine.restore_snapshot(&saved).unwrap();
    check(
        &session.engine,
        &mut previous,
        &mut document_scope,
        false,
        "snapshot",
    );
}

#[test]
fn cached_table_availability_equals_fresh_evaluation() {
    use crate::test_support::large_table_fixture::{
        grid_boundary_document, two_table_document, GRID_BOUNDARY_FIXTURE_SLOTS,
    };
    const AVAILABILITY_SEEDED_STEPS: usize = 12;
    const OWNER: u64 = 131;
    const REQUEST_BASE: u64 = 132;
    const REQUESTS_PER_STEP: u64 = 5;
    const SELECTION_STRIDE: usize = 7919;
    const STRUCTURAL_STEP_PERIOD: usize = 3;
    let merged =
        json!({"type":"doc", "content":[crate::tables::normalize_tests::merged_fixture_table()]});
    let mut empty = plain_table_document(2, 2);
    empty["content"][0]["content"][0]["content"][0]["content"][0] = json!({"type":"paragraph"});
    for (name, fixture) in [
        ("empty", empty),
        ("two tables", two_table_document()),
        ("merged", merged),
        (
            "grid boundary",
            grid_boundary_document(GRID_BOUNDARY_FIXTURE_SLOTS),
        ),
    ] {
        let mut session = session_with_document(&fixture);
        let compare = |engine: &YrsDocumentEngine, step, phase| {
            let state = engine.derived_state.as_ref().unwrap();
            let fresh = crate::editor_state::active_state_for_debug_invariant(
                &state.document,
                engine.schema(),
                &state.legacy_selection(),
                engine.stored_marks(),
                engine.resource_limits(),
                state.document_node_count,
            );
            assert_eq!(
                engine.active_state().unwrap(),
                fresh,
                "{name}, step {step}, {phase}: cached vs fresh"
            );
        };
        for step in 0..AVAILABILITY_SEEDED_STEPS {
            let request = REQUEST_BASE + step as u64 * REQUESTS_PER_STEP;
            let map = session.engine.position_map().unwrap();
            let block = if step == 0 {
                0
            } else {
                (step * SELECTION_STRIDE) % map.block_count()
            };
            let doc_pos = map.effective_doc_start(block);
            let caret = map.doc_to_scalar(doc_pos, session.engine.document().unwrap());
            select_text(&mut session.engine, request, caret, caret);
            compare(&session.engine, step, "selection");
            if step == 0 {
                let map = session.engine.position_map().unwrap();
                let last = map.effective_scalar_start(map.block_count() - 1);
                select_text(&mut session.engine, request + 2, caret, last);
                compare(&session.engine, step, "forward range across cells");
                select_text(&mut session.engine, request + 3, last, caret);
                compare(&session.engine, step, "backward range across cells");
                select_text(&mut session.engine, request + 4, caret, caret);
            }
            if step % STRUCTURAL_STEP_PERIOD == 1
                && session
                    .engine
                    .position_map()
                    .unwrap()
                    .block(block)
                    .unwrap()
                    .doc_end
                    > session
                        .engine
                        .position_map()
                        .unwrap()
                        .block(block)
                        .unwrap()
                        .doc_start
            {
                select_text(&mut session.engine, request + 2, caret, caret + 1);
                compare(&session.engine, step, "range in the same cell");
                select_text(&mut session.engine, request + 3, caret, caret);
                compare(&session.engine, step, "caret restored");
            }
            if step % STRUCTURAL_STEP_PERIOD == STRUCTURAL_STEP_PERIOD - 1 {
                let command = if step % 2 == 0 {
                    TableCommand::AddTableColumn {
                        side: TableEdge::After,
                    }
                } else {
                    TableCommand::AddTableRow {
                        side: TableEdge::After,
                    }
                };
                let result = session
                    .engine
                    .apply_command(request + 1, TypedCommand::Table(command));
                if let Err(error) = result {
                    assert_eq!(
                        name, "grid boundary",
                        "{name} structural command: {error:?}"
                    );
                    assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
                }
            } else {
                let scalar = session
                    .engine
                    .position_map()
                    .unwrap()
                    .doc_to_scalar(doc_pos, session.engine.document().unwrap());
                let epoch = session
                    .pin_position_epoch(OWNER, session.engine.revision())
                    .unwrap();
                let result = NativeTransactionBridge::new(&mut session).submit_native_intent(&json!({
                    "version":1, "requestId":(request + 1).to_string(), "ownerId":OWNER.to_string(), "positionEpoch":epoch.to_string(),
                    "intent":{"type":"insertText", "anchor":scalar, "head":scalar, "text":"x"}
                }).to_string());
                assert!(result.is_ok(), "{name} step {step}: {result:?}");
            }
            compare(&session.engine, step, "edit");
        }
    }
}

#[test]
fn table_availability_cache_respects_limit_changes_and_optional_retention() {
    const SIDE: usize = 3;
    const OWNER: u64 = 171;
    const REQUEST: u64 = 172;
    const NO_RETAINED_BYTES: usize = 0;
    let mut session = session_with_document(&plain_table_document(SIDE, SIDE));
    let _ = session.engine.active_state().unwrap();
    let original_limits = session.engine.resource_limits.clone();
    session.engine.resource_limits.max_table_grid_slots = SIDE;
    let state = session.engine.derived_state.as_ref().unwrap();
    assert_eq!(
        session.engine.active_state().unwrap(),
        crate::editor_state::active_state_for_debug_invariant(
            &state.document,
            session.engine.schema(),
            &state.legacy_selection(),
            session.engine.stored_marks(),
            session.engine.resource_limits(),
            state.document_node_count
        )
    );
    session.engine.resource_limits = original_limits;
    let state = session.engine.derived_state.as_ref().unwrap();
    let commands = state
        .table_command_availability(
            &state.document,
            session.engine.schema(),
            &state.legacy_selection(),
            session.engine.resource_limits(),
            NO_RETAINED_BYTES,
            &state.render_blocks,
            session.engine.document_scope_revision(),
        )
        .unwrap();
    let fresh = crate::editor_state::command_applicability(
        &state.document,
        session.engine.schema(),
        &state.legacy_selection(),
        session.engine.resource_limits(),
    );
    for (name, value) in commands {
        assert_eq!(
            Some(&value),
            fresh.get(&name),
            "optional cache refusal: {name}"
        );
    }
    let epoch = session
        .pin_position_epoch(OWNER, session.engine.revision())
        .unwrap();
    NativeTransactionBridge::new(&mut session).submit_native_intent(&json!({
        "version":1,"requestId":REQUEST.to_string(),"ownerId":OWNER.to_string(),"positionEpoch":epoch.to_string(),
        "intent":{"type":"insertText","anchor":0,"head":0,"text":"x"}
    }).to_string()).unwrap();
    let state = session.engine.derived_state.as_ref().unwrap();
    assert_eq!(
        session.engine.active_state().unwrap(),
        crate::editor_state::active_state_for_debug_invariant(
            &state.document,
            session.engine.schema(),
            &state.legacy_selection(),
            session.engine.stored_marks(),
            session.engine.resource_limits(),
            state.document_node_count
        )
    );
}

#[test]
fn table_availability_invalidates_after_a_structural_edit_in_another_table() {
    use crate::test_support::large_table_fixture::two_table_document;
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const REQUEST: u64 = 181;
    let mut session = session_with_document(&two_table_document());
    let positions: Vec<_> = session
        .engine
        .table_projection_index()
        .unwrap()
        .positions()
        .collect();
    let cell_caret = |engine: &YrsDocumentEngine, table| {
        let cell = &engine
            .table_projection_index()
            .unwrap()
            .table_at(table)
            .unwrap()
            .cells[0];
        engine.position_map().unwrap().doc_to_scalar(
            cell.source_pos + crate::tables::commands::NODE_OPENING_TOKENS,
            engine.document().unwrap(),
        )
    };
    let first = cell_caret(&session.engine, positions[0]);
    select_text(&mut session.engine, REQUEST, first, first);
    session.engine.active_state().unwrap();
    let second = cell_caret(&session.engine, positions[1]);
    select_text(&mut session.engine, REQUEST + 1, second, second);
    session
        .engine
        .apply_command(
            REQUEST + 2,
            TypedCommand::Table(TableCommand::AddTableColumn {
                side: TableEdge::After,
            }),
        )
        .unwrap()
        .unwrap();
    let first = cell_caret(&session.engine, positions[0]);
    select_text(&mut session.engine, REQUEST + 3, first, first);
    reset_full_pass_counts_for_test();
    let actual = session.engine.active_state().unwrap();
    let passes = take_full_pass_counts_for_test();
    assert!(
        passes.table_command_availability_plans > 0,
        "the other table's edit invalidates the prior entry: {passes:?}"
    );
    let state = session.engine.derived_state.as_ref().unwrap();
    let expected = crate::editor_state::active_state_for_debug_invariant(
        &state.document,
        session.engine.schema(),
        &state.legacy_selection(),
        session.engine.stored_marks(),
        session.engine.resource_limits(),
        state.document_node_count,
    );
    assert_eq!(actual, expected);
}

#[test]
fn authoritative_render_reuses_selection_and_history_results() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const SIDE: usize = 2;
    const REQUEST: u64 = 191;
    const CARET: u32 = 2;
    const OWNER: u64 = 192;
    let mut session = session_with_document(&plain_table_document(SIDE, SIDE));
    session
        .engine
        .apply_command(REQUEST, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .unwrap();
    let point = RevisionedPosition {
        offset: CARET,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    };
    let selection = session
        .engine
        .apply_typed_transaction_with_result(TypedTransaction {
            request_id: REQUEST + 1,
            base_document_revision: session.engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point,
                head: point,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    let check = |engine: &YrsDocumentEngine, expected, label| {
        reset_full_pass_counts_for_test();
        assert_eq!(
            engine.active_state().unwrap(),
            expected,
            "{label}: authoritative result"
        );
        let counts = take_full_pass_counts_for_test();
        assert_eq!(counts.active_applicability_passes, 0, "{label}: {counts:?}");
        assert_eq!(
            counts.table_command_availability_plans, 0,
            "{label}: {counts:?}"
        );
    };
    check(&session.engine, selection.active_state, "selection");
    let epoch = session
        .pin_position_epoch(OWNER, session.engine.revision())
        .unwrap();
    reset_full_pass_counts_for_test();
    NativeTransactionBridge::new(&mut session).submit_native_intent(&json!({
        "version":1,"requestId":(REQUEST + 2).to_string(),"ownerId":OWNER.to_string(),"positionEpoch":epoch.to_string(),
        "intent":{"type":"insertText","anchor":CARET,"head":CARET,"text":"x"}
    }).to_string()).unwrap();
    let typing = take_full_pass_counts_for_test();
    assert_eq!(
        typing.table_command_availability_plans, 0,
        "typing after displayed selection: {typing:?}"
    );
    assert_eq!(
        typing.active_applicability_passes, 0,
        "typing after displayed selection: {typing:?}"
    );
    assert_eq!(
        typing.table_projection_derivations, 0,
        "typing after displayed selection: {typing:?}"
    );

    let undo = session
        .engine
        .undo_with_result(REQUEST + 3)
        .unwrap()
        .unwrap();
    check(&session.engine, undo.active_state, "undo");
    let redo = session
        .engine
        .redo_with_result(REQUEST + 4)
        .unwrap()
        .unwrap();
    check(&session.engine, redo.active_state, "redo");
}
