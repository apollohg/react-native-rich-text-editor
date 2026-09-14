use super::*;
use base64::Engine;
use yrs::updates::encoder::Encode;
use yrs::{Map, Text, XmlFragment};

fn fixture() -> [Vec<u8>; 3] {
    // Stock Yjs client42: paragraph 0..3, unrelated map 3..4, typing 4..17.
    [
        "AQMqAAcBC3Byb3NlbWlycm9yAwlwYXJhZ3JhcGgHACoABgQAKgEBYQA=",
        "AQEqAygBC2luZGVwZW5kZW50AXgBfQEA",
        "AQEqBEQqAg1jb250aW51YXRpb24tAA==",
    ]
    .map(|bytes| base64::prelude::BASE64_STANDARD.decode(bytes).unwrap())
}

fn target() -> YrsDocumentEngine {
    transaction_engine_with_resource_limits_and_mode(
        ResourceLimits::default(),
        crate::yrs_engine::InitializationMode::AwaitRemote,
    )
}

fn apply(doc: &Doc, bytes: &[u8]) {
    doc.transact_mut()
        .apply_update(Update::decode_v1(bytes).unwrap())
        .unwrap();
}

fn full(doc: &Doc) -> Vec<u8> {
    doc.transact()
        .encode_state_as_update_v1(&StateVector::default())
}

fn assert_export_matches_cache(engine: &YrsDocumentEngine) {
    let decoded = utf16_doc();
    apply(&decoded, &engine.encoded_state().unwrap());
    let txn = decoded.transact();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    let codec = YrsDocumentCodec::new(&engine.schema, &engine.resource_limits);
    assert_eq!(
        Some(codec.read_json(&fragment, &txn).unwrap()),
        engine.document_json(),
        "exported live structs must reproduce the cached projection"
    );
}

fn assert_history_rebuild(engine: &YrsDocumentEngine) {
    let replayed = engine.new_history_candidate_doc();
    engine.history.seed_candidate(910, &replayed).unwrap();
    let fragment = replayed.get_or_insert_xml_fragment("prosemirror");
    let history = engine
        .history
        .replay_into(910, &replayed, &fragment)
        .unwrap();
    assert_eq!(full(&replayed), engine.encoded_state().unwrap());
    assert_eq!(history.can_undo(), engine.can_undo());
    assert_eq!(history.can_redo(), engine.can_redo());
}

#[test]
fn fifo_and_reversed_suffix_publish_all_structs_at_each_committed_boundary() {
    let updates = fixture();
    for order in [[0, 1, 2], [0, 2, 1]] {
        for split in [false, true] {
            let mut engine = target();
            let reference = utf16_doc();
            for index in order {
                let bytes = &updates[index];
                apply(&reference, bytes);
                if split {
                    let before = atomic_audit(&engine);
                    let prepared = engine.prepare_remote_update_v1(900, bytes).unwrap();
                    assert_eq!(atomic_audit(&engine), before);
                    assert!(
                        engine
                            .commit_prepared_remote_update(prepared)
                            .unwrap()
                            .changed
                    );
                } else {
                    assert!(engine.apply_remote_update_v1(900, bytes).unwrap().changed);
                }
                assert_export_matches_cache(&engine);
                assert_eq!(engine.encoded_state().unwrap(), full(&reference));
                assert_history_rebuild(&engine);
                assert!(!engine.can_undo());
                assert!(!engine.can_redo());
                if order == [0, 2, 1] && index == 2 {
                    assert_eq!(engine.encode_state_vector_v1(900).unwrap(), vec![1, 42, 3]);
                    assert_eq!(
                        engine.document().unwrap().root().text_content(),
                        "continuation-a"
                    );
                }
                let before = atomic_audit(&engine);
                assert!(!engine.apply_remote_update_v1(901, bytes).unwrap().changed);
                assert_eq!(atomic_audit(&engine), before);
            }
            assert_eq!(engine.encode_state_vector_v1(902).unwrap(), vec![1, 42, 17]);
            assert_eq!(
                engine.document().unwrap().root().text_content(),
                "continuation-a"
            );
            assert_eq!(
                engine
                    .doc
                    .transact()
                    .get_map("independent")
                    .unwrap()
                    .get(&engine.doc.transact(), "x"),
                Some(yrs::Out::Any(yrs::Any::Number(1.0)))
            );
        }
    }
}

#[test]
fn store_delta_negative_control_omits_an_integrated_suffix_above_a_hole() {
    let [baseline, _, typing] = fixture();
    let live = utf16_doc();
    apply(&live, &baseline);
    let candidate = utf16_doc();
    apply(&candidate, &baseline);
    apply(&candidate, &typing);
    let live_sv = live.transact().state_vector();
    assert_eq!(candidate.transact().state_vector(), live_sv);
    assert!(!candidate.transact().has_missing_updates());
    let store_delta = candidate.transact().encode_state_as_update_v1(&live_sv);
    assert_eq!(store_delta, vec![0, 0]);
    apply(&live, &store_delta);
    assert_ne!(full(&live), full(&candidate));
}

#[test]
fn suffix_and_delete_effects_survive_replay_and_local_undo_redo() {
    let [baseline, prerequisite, typing] = fixture();
    let mut engine = target();
    engine.apply_remote_update_v1(920, &baseline).unwrap();
    engine.apply_remote_update_v1(921, &typing).unwrap();
    assert_history_rebuild(&engine);
    let source = utf16_doc();
    apply(&source, &engine.encoded_state().unwrap());
    let source_sv = source.transact().state_vector();
    {
        let mut txn = source.transact_mut();
        let fragment = txn.get_xml_fragment("prosemirror").unwrap();
        let paragraph = fragment.get(&txn, 0).unwrap().into_xml_element().unwrap();
        let text = paragraph.get(&txn, 0).unwrap().into_xml_text().unwrap();
        text.remove_range(&mut txn, 0, 1);
    }
    let deletion = source.transact().encode_state_as_update_v1(&source_sv);
    assert_eq!(
        Update::decode_v1(&deletion).unwrap().state_vector(),
        StateVector::default()
    );
    engine.apply_remote_update_v1(923, &deletion).unwrap();
    assert_eq!(
        engine.document().unwrap().root().text_content(),
        "ontinuation-a"
    );
    assert!(!engine.can_undo());
    assert_history_rebuild(&engine);
    assert_export_matches_cache(&engine);
    engine.apply_remote_update_v1(922, &prerequisite).unwrap();
    assert_eq!(
        engine.document().unwrap().root().text_content(),
        "ontinuation-a"
    );
    assert_history_rebuild(&engine);
    engine
        .apply_command(
            924,
            TypedCommand::InsertText {
                text: "local".into(),
            },
        )
        .unwrap();
    let with_local = engine.document_json();
    engine.undo(925).unwrap().expect("local edit is undoable");
    assert_eq!(
        engine.document().unwrap().root().text_content(),
        "ontinuation-a"
    );
    assert_export_matches_cache(&engine);
    assert_history_rebuild(&engine);
    engine.redo(926).unwrap().expect("local edit is redoable");
    assert_eq!(engine.document_json(), with_local);
    assert_export_matches_cache(&engine);
    assert_history_rebuild(&engine);
}

#[test]
fn publication_charges_trimmed_payload_and_safe_hole_redundancy() {
    let [baseline, prerequisite, typing] = fixture();
    let mut engine = target();
    engine.apply_remote_update_v1(930, &baseline).unwrap();
    let candidate = utf16_doc();
    apply(&candidate, &baseline);
    apply(&candidate, &typing);
    let redundant_input = full(&candidate);
    assert_eq!(typing.len(), 22);
    assert_eq!(redundant_input.len(), 60);
    let before = engine.history.replay_audit_for_test();
    engine
        .apply_remote_update_v1(931, &redundant_input)
        .unwrap();
    let after = engine.history.replay_audit_for_test();
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.1 - before.1, typing.len() + 1);
    assert_history_rebuild(&engine);
    apply(&candidate, &prerequisite);
    let accepted = yrs::diff_updates_v1(&full(&candidate), &vec![1, 42, 3]).unwrap();
    assert_eq!(accepted.len(), 41);
    assert!(accepted.len() > prerequisite.len());
    assert!(accepted.len() < full(&candidate).len());
    engine.apply_remote_update_v1(932, &prerequisite).unwrap();
    let completed = engine.history.replay_audit_for_test();
    assert_eq!(completed.1 - after.1, accepted.len() + 1);
    assert_history_rebuild(&engine);
}

#[test]
fn failed_suffix_history_reservation_and_stale_commit_leave_everything_intact() {
    let [baseline, prerequisite, typing] = fixture();
    let mut engine = target();
    engine.apply_remote_update_v1(940, &baseline).unwrap();
    let before = atomic_audit(&engine);
    let history_before = engine.history.replay_ledger_allocation_audit_for_test();
    let pending_before = engine.quarantined_remote_update.clone();
    crate::yrs_engine::history::set_replay_update_allocation_failure_for_test(true);
    let failed = engine.prepare_remote_update_v1(941, &typing);
    crate::yrs_engine::history::set_replay_update_allocation_failure_for_test(false);
    assert_eq!(failed.err().unwrap().code, "OPERATION_RESOURCE_EXHAUSTED");
    assert_eq!(atomic_audit(&engine), before);
    assert_eq!(
        engine.history.replay_ledger_allocation_audit_for_test(),
        history_before
    );
    assert_eq!(engine.quarantined_remote_update, pending_before);
    let prepared = engine.prepare_remote_update_v1(942, &typing).unwrap();
    engine.apply_remote_update_v1(943, &prerequisite).unwrap();
    let before = atomic_audit(&engine);
    let history_before = engine.history.replay_ledger_allocation_audit_for_test();
    let error = engine.commit_prepared_remote_update(prepared).unwrap_err();
    assert_eq!(error.code, "ENGINE_INVARIANT_FAILED");
    assert_eq!(atomic_audit(&engine), before);
    assert_eq!(
        engine.history.replay_ledger_allocation_audit_for_test(),
        history_before
    );
    engine.apply_remote_update_v1(944, &typing).unwrap();
    assert_export_matches_cache(&engine);
    assert_history_rebuild(&engine);
}

#[test]
fn suffix_replay_work_rolls_at_the_exact_budget_and_preserves_gapped_checkpoints() {
    let [baseline, prerequisite, typing] = fixture();
    let exact = baseline.len() + typing.len();
    for (budget, suffix_events) in [(exact - 1, 1), (exact, 2), (exact + prerequisite.len(), 2)] {
        let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
            schema: tiptap_schema(),
            fragment_name: "prosemirror".into(),
            initialization_mode: crate::yrs_engine::InitializationMode::AwaitRemote,
            resource_limits: ResourceLimits::default(),
            editing_limits: crate::yrs_engine::EditingLimits {
                max_undo_retained_units: budget as u64,
                ..crate::yrs_engine::EditingLimits::default()
            },
            max_length: None,
            scope: None,
        })
        .unwrap();
        engine.apply_remote_update_v1(950, &baseline).unwrap();
        engine.apply_remote_update_v1(951, &typing).unwrap();
        assert_eq!(engine.history.replay_audit_for_test().0, suffix_events);
        assert_export_matches_cache(&engine);
        assert_history_rebuild(&engine);
        engine.apply_remote_update_v1(952, &prerequisite).unwrap();
        assert_eq!(engine.history.replay_audit_for_test().0, 1);
        let checkpoint = utf16_doc();
        engine.history.seed_candidate(953, &checkpoint).unwrap();
        assert_eq!(
            checkpoint.transact().state_vector().encode_v1(),
            vec![1, 42, 3]
        );
        assert!(full(&checkpoint).len() > baseline.len());
        assert_history_rebuild(&engine);
        assert_eq!(
            engine.document().unwrap().root().text_content(),
            "continuation-a"
        );
    }
}

#[test]
fn over_limit_suffix_rejects_before_live_history_or_quarantine_changes() {
    let [baseline, _, typing] = fixture();
    let candidate = utf16_doc();
    apply(&candidate, &baseline);
    apply(&candidate, &typing);
    let mut engine = transaction_engine_with_resource_limits_and_mode(
        ResourceLimits {
            max_encoded_state_bytes: full(&candidate).len() - 1,
            ..ResourceLimits::default()
        },
        crate::yrs_engine::InitializationMode::AwaitRemote,
    );
    engine.apply_remote_update_v1(960, &baseline).unwrap();
    let before = atomic_audit(&engine);
    let history_before = engine.history.replay_ledger_allocation_audit_for_test();
    let pending_before = engine.quarantined_remote_update.clone();
    let error = engine.prepare_remote_update_v1(961, &typing).err().unwrap();
    assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
    assert_eq!(atomic_audit(&engine), before);
    assert_eq!(
        engine.history.replay_ledger_allocation_audit_for_test(),
        history_before
    );
    assert_eq!(engine.quarantined_remote_update, pending_before);
}
