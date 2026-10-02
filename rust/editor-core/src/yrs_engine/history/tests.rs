use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use yrs::block::ClientID;
use yrs::types::xml::{XmlFragment, XmlOut};
use yrs::types::Text;
use yrs::updates::decoder::Decode;
use yrs::{Doc, GetString, IdSet, ReadTxn, StateVector, Transact, Update, XmlTextPrelim};

use super::{
    add_id_set_units, EditingLimits, HistoryAction, HistoryClass, HistoryMetadata,
    HistoryMetadataSlots, HistoryPolicy, HistorySnapshot, HistorySnapshotSlot, PendingReplayEvent,
    RelativeSelection, ReplayEvent, ResolvedSelection, TransactionOrigin, YrsHistory, INPUT_ORIGIN,
};

#[test]
#[cfg(feature = "table-interop")]
fn availability_audit_detects_capture_reset_without_stack_change() {
    let (_doc, mut history) = compatible_history_requiring_reservation_roll(100);
    let before = history.availability_audit().unwrap();
    let stack_lengths = (
        history.manager.undo_stack().len(),
        history.manager.redo_stack().len(),
    );
    history.manager.reset();
    assert_eq!(
        stack_lengths,
        (
            history.manager.undo_stack().len(),
            history.manager.redo_stack().len()
        )
    );
    assert_ne!(before, history.availability_audit().unwrap());
}

#[test]
#[cfg(feature = "table-interop")]
fn availability_audit_freezes_shared_metadata_and_replay_state() {
    let (_doc, mut history) = compatible_history_requiring_reservation_roll(100);
    let before = history.availability_audit().unwrap();
    assert_eq!(before, history.availability_audit().unwrap());
    let metadata = history.manager.undo_stack().last().unwrap().meta();
    let mut slots = metadata.slots();
    slots.after = Some(HistorySnapshotSlot::initialized(history_snapshot(7)));
    metadata.replace_slots(slots);
    assert_ne!(before, history.availability_audit().unwrap());
    let changed = history.availability_audit().unwrap();
    history.replay_events.push(ReplayEvent::Boundary);
    assert_ne!(changed, history.availability_audit().unwrap());
}

#[test]
#[cfg(feature = "table-interop")]
fn availability_audit_rejects_oversized_evidence() {
    let (_doc, mut history) = compatible_history_requiring_reservation_roll(100);
    history.replay_bytes = usize::MAX;
    assert!(history.availability_audit().is_none());
}

#[test]
#[cfg(feature = "table-interop")]
fn availability_audit_freezes_callback_capture_slots_before_sealing() {
    let (_doc, history) = compatible_history_requiring_reservation_roll(100);
    let metadata = HistoryMetadata::capture(history_snapshot(1));
    *history.pending_capture.lock().unwrap() = Some(metadata.clone());
    let before = history.availability_audit().unwrap();
    metadata.set_after(history_snapshot(2));
    assert_ne!(before, history.availability_audit().unwrap());
}

#[test]
#[cfg(feature = "table-interop")]
fn availability_audit_detects_equal_value_slot_replacement() {
    let (_doc, history) = compatible_history_requiring_reservation_roll(100);
    let before = history.availability_audit().unwrap();
    let metadata = history.manager.undo_stack().last().unwrap().meta();
    let mut slots = metadata.slots();
    let same_value = slots.after.as_ref().unwrap().get().unwrap().clone();
    slots.after = Some(HistorySnapshotSlot::initialized(same_value));
    metadata.replace_slots(slots);
    assert_ne!(before, history.availability_audit().unwrap());
}

#[test]
#[cfg(feature = "table-interop")]
fn availability_audit_detects_equal_value_wrapper_replacement() {
    let (_doc, mut history) = compatible_history_requiring_reservation_roll(100);
    let before = history.availability_audit().unwrap();
    let ReplayEvent::Recorded { metadata, .. } = &mut history.replay_events[0] else {
        panic!("recorded event missing");
    };
    *metadata = metadata.shared_wrapper();
    assert_ne!(before, history.availability_audit().unwrap());
}

#[cfg(feature = "table-interop")]
#[test]
fn availability_audit_retains_replaced_metadata_allocations() {
    let (_doc, history) = compatible_history_requiring_reservation_roll(100);
    let slots = HistoryMetadataSlots {
        before: Some(HistorySnapshotSlot::empty()),
        after: None,
    };
    let weak_slot = Arc::downgrade(&slots.before.as_ref().unwrap().0);
    let metadata = HistoryMetadata(Arc::new(std::sync::Mutex::new(slots)));
    let weak_wrapper = Arc::downgrade(&metadata.0);
    *history.pending_capture.lock().unwrap() = Some(metadata);
    let clock_refs = Arc::strong_count(&history.clock);
    let capture_refs = Arc::strong_count(&history.pending_capture);
    let pop_refs = Arc::strong_count(&history.pending_pop);
    let popped_refs = Arc::strong_count(&history.popped);
    let before = history.availability_audit().unwrap();
    assert_eq!(Arc::strong_count(&history.clock), clock_refs + 1);
    assert_eq!(
        Arc::strong_count(&history.pending_capture),
        capture_refs + 1
    );
    assert_eq!(Arc::strong_count(&history.pending_pop), pop_refs + 1);
    assert_eq!(Arc::strong_count(&history.popped), popped_refs + 1);
    *history.pending_capture.lock().unwrap() = None;
    assert!(weak_wrapper.upgrade().is_some());
    assert!(weak_slot.upgrade().is_some());
    drop(before);
    assert_eq!(Arc::strong_count(&history.clock), clock_refs);
    assert_eq!(Arc::strong_count(&history.pending_capture), capture_refs);
    assert_eq!(Arc::strong_count(&history.pending_pop), pop_refs);
    assert_eq!(Arc::strong_count(&history.popped), popped_refs);
    assert!(weak_wrapper.upgrade().is_none());
    assert!(weak_slot.upgrade().is_none());
}

fn history_snapshot(metadata_bytes: usize) -> HistorySnapshot {
    HistorySnapshot {
        relative_selection: RelativeSelection::All,
        resolved_selection: ResolvedSelection::All,
        stored_marks: None,
        text_length: 0,
        canonical_fingerprint: super::HistoryCanonicalIdentity::Materialized([0; 32]),
        derived_output_bytes: 0,
        metadata_bytes,
        document_snapshot: None,
    }
}

fn compatible_history_requiring_reservation_roll(metadata_limit: usize) -> (Doc, YrsHistory) {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let limits = EditingLimits {
        max_derived_output_bytes: metadata_limit,
        ..EditingLimits::default()
    };
    let mut history = YrsHistory::new(&doc, &fragment, limits, usize::MAX, Arc::new(|| 10_000));
    let origin = history
        .prepare_capture(
            50,
            TransactionOrigin::LocalInput,
            HistoryPolicy::Auto,
            HistoryClass::Insert,
            1,
            Some(history_snapshot(1)),
            1,
            &[],
            0,
        )
        .unwrap();
    {
        let mut txn = doc.transact_mut_with(origin);
        fragment.push_back(&mut txn, XmlTextPrelim::new("a"));
    }
    history.finish_capture(history_snapshot(1), Vec::new());
    assert!(history.capture_is_compatible(
        TransactionOrigin::LocalInput,
        HistoryPolicy::Auto,
        HistoryClass::Insert,
        10_000,
    ));
    history.rebase_before_next_event = true;
    (doc, history)
}

#[test]
fn excluded_event_accounts_reserved_capacity_not_only_encoded_length() {
    let mut update = Vec::with_capacity(64);
    update.extend_from_slice(&[1, 2, 3]);
    let event = ReplayEvent::Excluded {
        update,
        origin: TransactionOrigin::LocalApi,
        work_units: 3,
    };
    assert_eq!(event.encoded_bytes(), 65);
}

#[test]
fn candidate_metadata_wrapper_shares_immutable_snapshot_slots() {
    let before = HistorySnapshotSlot::empty();
    let after = HistorySnapshotSlot::empty();
    let metadata = HistoryMetadata(Arc::new(std::sync::Mutex::new(HistoryMetadataSlots {
        before: Some(before),
        after: Some(after),
    })));

    let candidate = metadata.shared_wrapper();
    assert_ne!(metadata.identity(), candidate.identity());
    let live_slots = metadata.slots();
    let candidate_slots = candidate.slots();
    assert_eq!(
        live_slots.before.unwrap().identity(),
        candidate_slots.before.unwrap().identity()
    );
    assert_eq!(
        live_slots.after.unwrap().identity(),
        candidate_slots.after.unwrap().identity()
    );
}

#[test]
fn cumulative_excluded_events_charge_reserved_payload_capacity() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );

    let mut first = history.reserve_replay_event(1, &[], 9, 1, true).unwrap();
    first.push(1);
    let event_bytes = first.capacity() + 1;
    history.max_encoded_state_bytes = event_bytes * 2;
    history.push_replay_event(ReplayEvent::Excluded {
        update: first,
        origin: TransactionOrigin::LocalApi,
        work_units: 1,
    });

    let mut second = history.reserve_replay_event(2, &[], 9, 1, true).unwrap();
    second.push(2);
    history.push_replay_event(ReplayEvent::Excluded {
        update: second,
        origin: TransactionOrigin::LocalApi,
        work_units: 1,
    });
    assert_eq!(history.replay_bytes, event_bytes * 2);
    assert_eq!(history.replay_events.len(), 2);

    let third = history.reserve_replay_event(3, &[], 9, 1, true).unwrap();
    assert!(third.capacity() < history.max_encoded_state_bytes);
    assert!(history.replay_events.is_empty());
    assert_eq!(history.replay_bytes, 0);
}

#[test]
fn id_set_accounting_counts_clock_ranges_not_clients() {
    let set = IdSet::from_iter([
        (ClientID::new(1), [2..5, 9..11]),
        (ClientID::new(2), [0..4, 4..4]),
    ]);
    assert_eq!(add_id_set_units(7, &set, 1).unwrap(), 16);
}

fn insert_peer_text(doc: &Doc, text: &str) {
    insert_text_from_peer(doc, &Doc::new(), text);
}

fn insert_text_from_peer(doc: &Doc, peer: &Doc, text: &str) {
    let peer_fragment = peer.get_or_insert_xml_fragment("history-test");
    let shared = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    peer.transact_mut()
        .apply_update(Update::decode_v1(&shared).expect("peer decodes the shared state"))
        .expect("peer applies the shared state");
    let target = match peer_fragment.get(&peer.transact(), 0) {
        Some(XmlOut::Text(target)) => target,
        _ => panic!("peer sees the shared text container"),
    };
    target.insert(&mut peer.transact_mut(), 0, text);
    let peer_update = peer
        .transact()
        .encode_state_as_update_v1(&doc.transact().state_vector());
    doc.transact_mut_with(TransactionOrigin::RemoteSync.as_yrs_origin())
        .apply_update(Update::decode_v1(&peer_update).expect("local decodes the peer update"))
        .expect("local applies the peer update");
}

#[test]
fn repeated_history_preserves_foreign_text_written_before_container_removal() {
    const EXCLUDED_ORIGIN: &str = "excluded-history-fixture";
    const FIRST_CLIENT: u64 = 1;
    const SECOND_CLIENT: u64 = 2;
    for (peer, local_client, peer_client) in [
        (false, FIRST_CLIENT, SECOND_CLIENT),
        (true, FIRST_CLIENT, SECOND_CLIENT),
        (true, SECOND_CLIENT, FIRST_CLIENT),
    ] {
        for local in ["local", "lo🦀cal"] {
            let doc = Doc::with_client_id(local_client);
            let expected = format!("foreign{local}");
            let fragment = doc.get_or_insert_xml_fragment("history-test");
            let mut history = YrsHistory::new(
                &doc,
                &fragment,
                EditingLimits::default(),
                usize::MAX,
                Arc::new(|| 10_000),
            );
            let text = fragment.push_back(
                &mut doc.transact_mut_with(INPUT_ORIGIN),
                XmlTextPrelim::new(local),
            );
            if peer {
                insert_text_from_peer(&doc, &Doc::with_client_id(peer_client), "foreign");
            } else {
                text.insert(&mut doc.transact_mut_with(EXCLUDED_ORIGIN), 0, "foreign");
            }
            history.manager.reset();
            fragment.remove(&mut doc.transact_mut_with(INPUT_ORIGIN), 0);
            assert_eq!(fragment.get_string(&doc.transact()), "");
            for cycle in 0..3 {
                let request = cycle * 4 + 1;
                assert!(history.undo(request, &doc, &fragment).unwrap().changed);
                assert_eq!(
                    fragment.get_string(&doc.transact()),
                    expected,
                    "restore removal, peer={peer}, cycle={cycle}"
                );
                assert!(history.undo(request + 1, &doc, &fragment).unwrap().changed);
                assert_eq!(fragment.get_string(&doc.transact()), "foreign", "undo creation must preserve earlier foreign content, peer={peer}, cycle={cycle}");
                assert!(history.redo(request + 2, &doc, &fragment).unwrap().changed);
                assert_eq!(fragment.get_string(&doc.transact()), expected);
                assert!(history.redo(request + 3, &doc, &fragment).unwrap().changed);
                assert_eq!(
                    fragment.get_string(&doc.transact()),
                    "",
                    "redo explicit removal must not leave an empty container"
                );
                assert_eq!(fragment.len(&doc.transact()), 0);
            }
        }
    }
}

#[test]
fn undo_creation_preserves_an_independently_inserted_empty_child_after_recreation() {
    const EXCLUDED_ORIGIN: &str = "excluded-empty-child-fixture";
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    let parent = {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        let parent = fragment.push_back(&mut txn, yrs::XmlElementPrelim::empty("paragraph"));
        parent.push_back(&mut txn, XmlTextPrelim::new("local"));
        parent
    };
    parent.push_back(
        &mut doc.transact_mut_with(EXCLUDED_ORIGIN),
        XmlTextPrelim::new(""),
    );
    history.manager.reset();
    fragment.remove(&mut doc.transact_mut_with(INPUT_ORIGIN), 0);
    assert!(history.undo(1, &doc, &fragment).unwrap().changed);
    assert!(history.undo(2, &doc, &fragment).unwrap().changed);
    let txn = doc.transact();
    assert_eq!(
        fragment.len(&txn),
        1,
        "the independently authored empty child keeps its parent"
    );
    let XmlOut::Element(parent) = fragment.get(&txn, 0).unwrap() else {
        panic!("preserved parent")
    };
    assert_eq!(
        parent.len(&txn),
        1,
        "only the independently inserted child survives"
    );
    let XmlOut::Text(child) = parent.get(&txn, 0).unwrap() else {
        panic!("preserved empty child")
    };
    assert_eq!(child.len(&txn), 0);
}

#[test]
fn undo_keeps_a_container_an_earlier_undo_recreated() {
    let now = Arc::new(AtomicU64::new(10_000));
    let source = Arc::clone(&now);
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(move || source.load(Ordering::SeqCst)),
    );
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.push_back(&mut txn, XmlTextPrelim::new("local"));
    }
    now.fetch_add(5_000, Ordering::SeqCst);
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.remove(&mut txn, 0);
    }
    assert_eq!(history.manager.undo_stack().len(), 2);
    assert_eq!(fragment.get_string(&doc.transact()), "");

    assert!(history.undo(1, &doc, &fragment).unwrap().changed);
    assert_eq!(fragment.get_string(&doc.transact()), "local");

    insert_peer_text(&doc, "peer");
    assert_eq!(fragment.get_string(&doc.transact()), "peerlocal");

    assert!(history.undo(1, &doc, &fragment).unwrap().changed);
    assert_eq!(
        fragment.get_string(&doc.transact()),
        "peer",
        "undo must not delete a container an earlier undo recreated while a peer wrote into it",
    );
}

#[test]
fn undo_keeps_a_text_container_holding_peer_content() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.push_back(&mut txn, XmlTextPrelim::new("local"));
    }
    insert_peer_text(&doc, "peer");
    assert_eq!(fragment.get_string(&doc.transact()), "peerlocal");

    let pop = history.undo(1, &doc, &fragment).unwrap();
    assert!(pop.changed);
    assert_eq!(fragment.get_string(&doc.transact()), "peer");
}

#[test]
fn redo_keeps_a_text_container_a_prior_undo_recreated() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.push_back(&mut txn, XmlTextPrelim::new("local"));
    }
    history.manager.reset();
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.remove(&mut txn, 0);
    }
    assert_eq!(fragment.get_string(&doc.transact()), "");

    assert!(history.undo(1, &doc, &fragment).unwrap().changed);
    assert_eq!(fragment.get_string(&doc.transact()), "local");

    insert_peer_text(&doc, "peer");
    assert_eq!(fragment.get_string(&doc.transact()), "peerlocal");

    assert!(history.redo(1, &doc, &fragment).unwrap().changed);
    assert_eq!(
        fragment.get_string(&doc.transact()),
        "peer",
        "redo must not delete the container a peer wrote into",
    );
}

#[test]
fn a_stack_item_that_reverts_only_protected_containers_is_drained() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.push_back(&mut txn, XmlTextPrelim::new(""));
    }
    insert_peer_text(&doc, "peer");
    assert_eq!(fragment.get_string(&doc.transact()), "peer");
    assert!(history.can_undo());

    let pop = history.undo(1, &doc, &fragment).unwrap();
    assert!(!pop.changed);
    assert!(pop.pruned > 0);
    assert_eq!(fragment.get_string(&doc.transact()), "peer");
    assert!(
        !history.can_undo(),
        "a stack drained of unrevertible items must not report an available undo",
    );
    assert!(!history.can_redo());
}

#[test]
fn a_redo_item_that_reverts_only_protected_containers_is_drained() {
    let now = Arc::new(AtomicU64::new(10_000));
    let source = Arc::clone(&now);
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(move || source.load(Ordering::SeqCst)),
    );
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.push_back(&mut txn, XmlTextPrelim::new(""));
    }
    now.fetch_add(5_000, Ordering::SeqCst);
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.remove(&mut txn, 0);
    }
    assert!(history.undo(1, &doc, &fragment).unwrap().changed);
    insert_peer_text(&doc, "peer");
    assert_eq!(fragment.get_string(&doc.transact()), "peer");
    assert!(history.can_redo());

    let pop = history.redo(2, &doc, &fragment).unwrap();
    assert!(!pop.changed);
    assert!(pop.pruned > 0);
    assert_eq!(fragment.get_string(&doc.transact()), "peer");
    assert!(
        !history.can_redo(),
        "a redo stack drained of unrevertible items must not report an available redo",
    );
}

#[test]
fn acting_stack_match_requires_the_same_items() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let history = {
        let history = YrsHistory::new(
            &doc,
            &fragment,
            EditingLimits::default(),
            usize::MAX,
            Arc::new(|| 10_000),
        );
        {
            let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
            fragment.push_back(&mut txn, XmlTextPrelim::new("local"));
        }
        history
    };
    let unrelated = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );

    assert!(history.acting_stack_matches(&history, HistoryAction::Undo));
    assert!(
        !history.acting_stack_matches(&unrelated, HistoryAction::Undo),
        "a replayed stack that holds different items must not authorise dropping live history",
    );
}

#[test]
fn dropping_acting_stack_items_only_touches_the_requested_direction() {
    let now = Arc::new(AtomicU64::new(10_000));
    let source = Arc::clone(&now);
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(move || source.load(Ordering::SeqCst)),
    );
    for text in ["a", "b", "c"] {
        {
            let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
            fragment.push_back(&mut txn, XmlTextPrelim::new(text));
        }
        now.fetch_add(5_000, Ordering::SeqCst);
    }
    assert!(history.undo(1, &doc, &fragment).unwrap().changed);
    assert_eq!(history.manager.undo_stack().len(), 2);
    assert_eq!(history.manager.redo_stack().len(), 1);

    history.drop_acting_stack_items(&doc, &fragment, HistoryAction::Undo, 1);
    assert_eq!(history.manager.undo_stack().len(), 1);
    assert_eq!(history.manager.redo_stack().len(), 1);

    history.drop_acting_stack_items(&doc, &fragment, HistoryAction::Redo, 5);
    assert_eq!(history.manager.undo_stack().len(), 1);
    assert!(history.manager.redo_stack().is_empty());
    assert_eq!(fragment.get_string(&doc.transact()), "ab");
}

#[test]
fn private_local_origin_is_captured_but_remote_origin_is_preserved_by_undo() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );

    {
        let mut txn = doc.transact_mut_with(TransactionOrigin::RemoteSync.as_yrs_origin());
        fragment.push_back(&mut txn, XmlTextPrelim::new("remote"));
    }
    assert_eq!(history.manager.undo_stack().len(), 0);

    history.manager.reset();
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.push_back(&mut txn, XmlTextPrelim::new("local"));
    }
    assert_eq!(history.manager.undo_stack().len(), 1);
    assert!(history.manager.undo_blocking());
    assert_eq!(fragment.get_string(&doc.transact()), "remote");
}

#[test]
fn replay_reservation_is_fallible_and_does_not_clear_existing_history() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    {
        let mut txn = doc.transact_mut_with(INPUT_ORIGIN);
        fragment.push_back(&mut txn, XmlTextPrelim::new("local"));
    }
    let undo_groups = history.manager.undo_stack().len();

    let error = history
        .reserve_replay_event(41, &[], usize::MAX, 1, false)
        .unwrap_err();
    assert_eq!(error.code, "OPERATION_RESOURCE_EXHAUSTED");
    assert_eq!(history.manager.undo_stack().len(), undo_groups);
    assert!(history.manager.can_undo());
}

#[test]
fn roll_baseline_reservation_failure_keeps_resource_exhausted() {
    let (_doc, mut history) = compatible_history_requiring_reservation_roll(100);
    super::set_roll_baseline_reservation_failure_for_test(true);
    let error = history
        .prepare_capture(
            52,
            TransactionOrigin::LocalInput,
            HistoryPolicy::Auto,
            HistoryClass::Insert,
            1,
            Some(history_snapshot(59)),
            41,
            &[],
            0,
        )
        .unwrap_err();
    super::set_roll_baseline_reservation_failure_for_test(false);
    assert_eq!(error.code, "OPERATION_RESOURCE_EXHAUSTED");
    assert_eq!(
        error.details,
        Some(serde_json::json!({ "field": "historyReplay" }))
    );
    // Recovery: the identical capture succeeds once allocation recovers.
    history
        .prepare_capture(
            52,
            TransactionOrigin::LocalInput,
            HistoryPolicy::Auto,
            HistoryClass::Insert,
            1,
            Some(history_snapshot(59)),
            41,
            &[],
            0,
        )
        .unwrap();
}

#[test]
fn accepted_action_reservation_failure_keeps_resource_exhausted() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    super::set_accepted_action_reservation_failure_for_test(true);
    let error = history
        .accept_action(41, super::HistoryAction::Undo, Vec::new())
        .unwrap_err();
    super::set_accepted_action_reservation_failure_for_test(false);
    assert_eq!(error.code, "OPERATION_RESOURCE_EXHAUSTED");
    assert_eq!(
        error.details,
        Some(serde_json::json!({ "field": "historyReplay" }))
    );
    history
        .accept_action(41, super::HistoryAction::Undo, Vec::new())
        .unwrap();
}

#[test]
fn candidate_events_reservation_failure_keeps_resource_exhausted() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    super::set_candidate_events_reservation_failure_for_test(true);
    let Err(error) = history.replay_into(41, &doc, &fragment) else {
        panic!("injected candidate events failure must reject")
    };
    super::set_candidate_events_reservation_failure_for_test(false);
    assert_eq!(error.code, "OPERATION_RESOURCE_EXHAUSTED");
    assert_eq!(
        error.details,
        Some(serde_json::json!({ "field": "historyReplay" }))
    );
    history.replay_into(41, &doc, &fragment).unwrap();
}

#[test]
fn event_replacement_reservation_failure_keeps_resource_exhausted() {
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| 10_000),
    );
    super::set_event_replacement_reservation_failure_for_test(true);
    let Err(error) = history.prepare_replay_event_slot(41, false) else {
        panic!("injected replacement failure must reject")
    };
    super::set_event_replacement_reservation_failure_for_test(false);
    assert_eq!(error.code, "OPERATION_RESOURCE_EXHAUSTED");
    assert_eq!(
        error.details,
        Some(serde_json::json!({ "field": "historyReplay" }))
    );
    history.prepare_replay_event_slot(41, false).unwrap();
}

#[test]
fn reserve_induced_compatible_roll_rejects_one_over_standalone_metadata_atomically() {
    let (_doc, mut history) = compatible_history_requiring_reservation_roll(100);
    // This is the state produced when a prior excluded event requires the
    // next recorded event to start a fresh replay epoch. The capture still
    // starts compatible, so only reservation discovers the rollover.
    let undo_groups = history.manager.undo_stack().len();
    let replay_events = history.replay_events.len();
    let replay_bytes = history.replay_bytes;
    let replay_work_units = history.replay_work_units;
    let replay_metadata_bytes = history.replay_metadata_bytes;
    let epoch_baseline = history.epoch_baseline.clone();

    let error = history
        .prepare_capture(
            51,
            TransactionOrigin::LocalInput,
            HistoryPolicy::Auto,
            HistoryClass::Insert,
            1,
            Some(history_snapshot(60)),
            41,
            &[],
            0,
        )
        .unwrap_err();
    assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
    assert_eq!(error.limit, Some(100));
    assert_eq!(error.actual, Some(101));
    assert_eq!(history.manager.undo_stack().len(), undo_groups);
    assert!(history.manager.can_undo());
    assert_eq!(history.replay_events.len(), replay_events);
    assert_eq!(history.replay_bytes, replay_bytes);
    assert_eq!(history.replay_work_units, replay_work_units);
    assert_eq!(history.replay_metadata_bytes, replay_metadata_bytes);
    assert_eq!(history.epoch_baseline, epoch_baseline);
    assert!(history.rebase_before_next_event);
    assert!(history.pending_replay_event.is_none());
    assert!(history
        .pending_capture
        .lock()
        .expect("pending capture lock")
        .is_none());
}

#[test]
fn reserve_induced_compatible_roll_accepts_exact_standalone_metadata_boundary() {
    let (_doc, mut history) = compatible_history_requiring_reservation_roll(100);

    history
        .prepare_capture(
            52,
            TransactionOrigin::LocalInput,
            HistoryPolicy::Auto,
            HistoryClass::Insert,
            1,
            Some(history_snapshot(59)),
            41,
            &[],
            0,
        )
        .unwrap();

    assert!(!history.rebase_before_next_event);
    assert!(history.manager.undo_stack().is_empty());
    assert!(history.replay_events.is_empty());
    assert!(matches!(
        history.pending_replay_event,
        Some(PendingReplayEvent::Recorded {
            metadata_increment: 100,
            ..
        })
    ));
}

fn recorded_text_history(edits: usize, policy: HistoryPolicy) -> (Doc, YrsHistory) {
    const CLOCK_MILLIS: u64 = 10_000;
    const UPDATE_RESERVATION_BYTES: usize = 1024;
    let doc = Doc::new();
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let text = fragment.push_back(&mut doc.transact_mut(), XmlTextPrelim::new("seed"));
    let mut history = YrsHistory::new(
        &doc,
        &fragment,
        EditingLimits::default(),
        usize::MAX,
        Arc::new(|| CLOCK_MILLIS),
    );
    for index in 0..edits {
        let baseline = super::encode_full_state(&doc);
        let origin = history
            .prepare_capture(
                index as u64,
                TransactionOrigin::LocalInput,
                policy,
                HistoryClass::Insert,
                1,
                Some(history_snapshot(index + 1)),
                index + 2,
                &baseline,
                UPDATE_RESERVATION_BYTES,
            )
            .unwrap();
        let update = {
            let mut txn = doc.transact_mut_with(origin);
            let end = text.len(&txn);
            text.insert(&mut txn, end, "x");
            txn.encode_update_v1()
        };
        history.finish_capture(history_snapshot(index + 2), update);
    }
    assert_eq!(
        text.get_string(&doc.transact()),
        format!("seed{}", "x".repeat(edits))
    );
    (doc, history)
}

#[test]
fn coalesced_history_accounting_stops_when_the_newest_record_covers_the_stack() {
    use crate::yrs_engine::observability::HISTORY_REPLAY_METADATA_VISITS;
    const EDITS: usize = 256;
    let (doc, history) = recorded_text_history(EDITS, HistoryPolicy::Auto);
    assert_eq!(history.manager.undo_stack().len(), 1);
    assert_eq!(history.replay_events.len(), EDITS);
    let retained = history.replay_metadata_bytes;
    HISTORY_REPLAY_METADATA_VISITS.set(0);
    assert_eq!(
        history.retained_metadata_bytes(EDITS as u64).unwrap(),
        retained
    );
    assert_eq!(
        HISTORY_REPLAY_METADATA_VISITS.get(),
        1,
        "one coalesced undo item shares both immutable snapshot slots with the latest actual edit"
    );
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    let candidate_doc = Doc::new();
    let candidate_fragment = candidate_doc.get_or_insert_xml_fragment("history-test");
    candidate_doc
        .transact_mut()
        .apply_update(Update::decode_v1(&history.epoch_baseline).unwrap())
        .unwrap();
    let candidate = history
        .replay_into(EDITS as u64, &candidate_doc, &candidate_fragment)
        .unwrap();
    assert_eq!(
        candidate_fragment.get_string(&candidate_doc.transact()),
        fragment.get_string(&doc.transact())
    );
    assert_eq!(
        candidate.retained_metadata_bytes(EDITS as u64).unwrap(),
        retained
    );
}

#[test]
fn history_accounting_avoids_a_second_stack_pass_without_a_replay_ledger() {
    use crate::yrs_engine::observability::HISTORY_STACK_METADATA_VISITS;
    const EDITS: usize = 256;
    for policy in [HistoryPolicy::Auto, HistoryPolicy::Boundary] {
        let (_doc, mut history) = recorded_text_history(EDITS, policy);
        history.replay_events.clear();
        history.replay_metadata_bytes = 0;
        let expected = original_unmirrored_metadata_bytes(&history, 0).unwrap();
        assert!(
            expected > 0,
            "{policy:?}: the live stack still owns snapshots"
        );
        HISTORY_STACK_METADATA_VISITS.set(0);
        assert_eq!(history.retained_metadata_bytes(0).unwrap(), expected);
        assert_eq!(
            HISTORY_STACK_METADATA_VISITS.get(),
            history.manager.undo_stack().len(),
            "{policy:?}: empty ledgers must retain the original single stack traversal"
        );
    }
}

fn original_unmirrored_metadata_bytes(
    history: &YrsHistory,
    request_id: u64,
) -> super::OperationResult<usize> {
    let mut mirrored = std::collections::HashSet::new();
    for event in &history.replay_events {
        if let ReplayEvent::Recorded { metadata, .. } = event {
            let slots = metadata.slots();
            for slot in [slots.before, slots.after].into_iter().flatten() {
                mirrored.insert(slot.identity());
            }
        }
    }
    let mut total = 0usize;
    for item in history
        .manager
        .undo_stack()
        .iter()
        .chain(history.manager.redo_stack())
    {
        let slots = item.meta().slots();
        for slot in [slots.before, slots.after].into_iter().flatten() {
            if mirrored.insert(slot.identity()) {
                let snapshot = slot.get().ok_or_else(|| {
                    super::OperationError::engine_invariant_failed(
                        request_id,
                        None,
                        "retained history contains an unsealed snapshot slot",
                    )
                })?;
                total = total.checked_add(snapshot.metadata_bytes).ok_or_else(|| {
                    super::metadata_limit_error(request_id, &history.limits, usize::MAX)
                })?;
            }
        }
    }
    Ok(total)
}

fn assert_history_membership_matches_original(history: &mut YrsHistory, label: &str) {
    const BOUNDARIES_PER_STACK_ITEM: usize = 4;
    let original_events = history.replay_events.len();
    assert_history_membership_lookup_matches_original(history, label);
    let stack_items = history.manager.undo_stack().len() + history.manager.redo_stack().len();
    history.replay_events.extend(
        std::iter::repeat_with(|| ReplayEvent::Boundary)
            .take((stack_items + 1) * BOUNDARIES_PER_STACK_ITEM),
    );
    assert_history_membership_lookup_matches_original(
        history,
        &format!("{label}: boundary-heavy ledger"),
    );
    history.replay_events.truncate(original_events);
}

fn assert_history_membership_lookup_matches_original(history: &YrsHistory, label: &str) {
    const REQUEST: u64 = 80_000;
    let expected = original_unmirrored_metadata_bytes(history, REQUEST);
    assert_eq!(
        history.unmirrored_stack_metadata_bytes(REQUEST),
        expected,
        "{label}"
    );
    if let Ok(bytes) = expected {
        let retained = history.replay_metadata_bytes.saturating_add(bytes);
        assert_eq!(
            history.retained_metadata_bytes(REQUEST).unwrap(),
            retained,
            "{label}: total fee"
        );
        let available = history
            .limits
            .max_derived_output_bytes
            .saturating_sub(retained);
        for pending in [0, available, available.saturating_add(1)] {
            assert_eq!(
                history.cache_metadata_headroom(REQUEST, pending),
                retained
                    .checked_add(pending)
                    .and_then(|used| history.limits.max_derived_output_bytes.checked_sub(used)),
                "{label}: pending={pending}"
            );
        }
    }
}

#[test]
fn history_membership_preserves_charges_through_undo_redo_and_rebase() {
    const EDITS: usize = 12;
    let (doc, mut history) = recorded_text_history(EDITS, HistoryPolicy::Boundary);
    let fragment = doc.get_or_insert_xml_fragment("history-test");
    assert_eq!(history.manager.undo_stack().len(), EDITS);
    assert_history_membership_matches_original(&mut history, "separate undo groups");
    for index in 0..EDITS {
        assert!(history.undo(index as u64, &doc, &fragment).unwrap().changed);
        assert_history_membership_matches_original(&mut history, &format!("undo {index}"));
    }
    assert_eq!(fragment.get_string(&doc.transact()), "seed");
    for index in 0..EDITS {
        assert!(history.redo(index as u64, &doc, &fragment).unwrap().changed);
        assert_history_membership_matches_original(&mut history, &format!("redo {index}"));
    }
    assert_eq!(
        fragment.get_string(&doc.transact()),
        format!("seed{}", "x".repeat(EDITS))
    );
    history.replay_events.push(ReplayEvent::Boundary);
    history
        .replay_events
        .push(ReplayEvent::Action(HistoryAction::Undo));
    history.replay_events.push(ReplayEvent::Excluded {
        update: Vec::new(),
        origin: TransactionOrigin::RemoteSync,
        work_units: 0,
    });
    assert_history_membership_matches_original(&mut history, "intervening non-metadata events");
    history.replay_events.clear();
    history.replay_metadata_bytes = 0;
    assert_history_membership_matches_original(&mut history, "empty ledger with retained stacks");
    assert!(history.unmirrored_stack_metadata_bytes(0).unwrap() > 0);
    history.roll_epoch(super::encode_full_state(&doc));
    assert_history_membership_matches_original(&mut history, "rebased empty history");
}

#[test]
fn history_membership_preserves_slot_identity_and_original_error_order() {
    const SNAPSHOT_BYTES: usize = 9;
    let (_doc, mut history) = recorded_text_history(2, HistoryPolicy::Boundary);
    for event in &mut history.replay_events {
        if let ReplayEvent::Recorded { metadata, .. } = event {
            *metadata = metadata.shared_wrapper();
        }
    }
    let stack: Vec<_> = history
        .manager
        .undo_stack()
        .iter()
        .map(|item| item.meta().clone())
        .collect();
    let unsealed = HistorySnapshotSlot::empty();
    let shared = HistorySnapshotSlot::initialized(history_snapshot(SNAPSHOT_BYTES));
    stack[0].replace_slots(HistoryMetadataSlots {
        before: Some(unsealed.clone()),
        after: Some(shared.clone()),
    });
    stack[1].replace_slots(HistoryMetadataSlots {
        before: Some(shared),
        after: Some(HistorySnapshotSlot::initialized(history_snapshot(
            SNAPSHOT_BYTES,
        ))),
    });
    let ReplayEvent::Recorded { metadata, .. } = history.replay_events.last().unwrap() else {
        panic!("latest real edit must carry history metadata");
    };
    metadata.replace_slots(HistoryMetadataSlots {
        before: Some(unsealed),
        after: None,
    });
    assert_eq!(history.unmirrored_stack_metadata_bytes(0).unwrap(), SNAPSHOT_BYTES * 2,
        "a mirrored unsealed slot is ignored; aliases count once and distinct equal values count separately");
    assert_history_membership_matches_original(&mut history, "partial overlap and aliases");
    history.replay_events.clear();
    assert_history_membership_matches_original(&mut history, "unmirrored unsealed slot");
    assert!(history.unmirrored_stack_metadata_bytes(0).is_err());
    stack[0].replace_slots(HistoryMetadataSlots {
        before: Some(HistorySnapshotSlot::initialized(history_snapshot(
            usize::MAX,
        ))),
        after: Some(HistorySnapshotSlot::initialized(history_snapshot(1))),
    });
    stack[1].replace_slots(HistoryMetadataSlots {
        before: Some(HistorySnapshotSlot::empty()),
        after: None,
    });
    assert_history_membership_matches_original(
        &mut history,
        "overflow before a later unsealed slot",
    );
    assert_eq!(
        history.unmirrored_stack_metadata_bytes(0).unwrap_err().code,
        "DOCUMENT_LIMIT_EXCEEDED"
    );
    stack[0].replace_slots(HistoryMetadataSlots {
        before: Some(HistorySnapshotSlot::empty()),
        after: None,
    });
    assert_history_membership_matches_original(&mut history, "unsealed before later values");
    assert_eq!(
        history.unmirrored_stack_metadata_bytes(0).unwrap_err().code,
        "ENGINE_INVARIANT_FAILED"
    );
}

#[test]
fn history_membership_only_clones_slots_for_the_fee_pass() {
    use crate::yrs_engine::observability::HISTORY_OWNED_SLOT_READS;
    const EDITS: usize = 256;
    for policy in [HistoryPolicy::Auto, HistoryPolicy::Boundary] {
        let (_doc, history) = recorded_text_history(EDITS, policy);
        let expected_bytes = original_unmirrored_metadata_bytes(&history, 0).unwrap();
        let expected_reads = if policy == HistoryPolicy::Auto {
            0
        } else {
            EDITS
        };
        HISTORY_OWNED_SLOT_READS.set(0);
        assert_eq!(
            history.unmirrored_stack_metadata_bytes(0).unwrap(),
            expected_bytes
        );
        assert_eq!(
            HISTORY_OWNED_SLOT_READS.get(),
            expected_reads,
            "{policy:?}: identity-only membership scans must not clone snapshot ownership"
        );
    }
}
