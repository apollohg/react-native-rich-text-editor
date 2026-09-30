const CANONICAL_CACHE_TEST_LIMIT: usize = 16 * 1024;
const CANONICAL_CACHE_TEST_TEXT_BYTES: usize = 4 * 1024;
const CANONICAL_CACHE_TEST_CLOCK_MILLIS: u64 = 10_000;

fn canonical_cache_engine() -> YrsDocumentEngine {
    let mut engine = YrsDocumentEngine::new_with_history_clock(
        YrsEngineConfig {
            schema: tiptap_schema(),
            fragment_name: "prosemirror".into(),
            initialization_mode: crate::yrs_engine::InitializationMode::LocalEmpty,
            resource_limits: ResourceLimits::default(),
            editing_limits: crate::yrs_engine::EditingLimits {
                max_derived_output_bytes: CANONICAL_CACHE_TEST_LIMIT,
                ..crate::yrs_engine::EditingLimits::default()
            },
            max_length: None,
            scope: Some(crate::yrs_engine::DocumentScope {
                document_id: "doc".into(),
                lineage_id: "lineage".into(),
            }),
        },
        Arc::new(|| CANONICAL_CACHE_TEST_CLOCK_MILLIS),
    )
    .unwrap();
    engine
        .import_json(
            &json!({"type":"doc","content":[{"type":"paragraph","content":[
        {"type":"text","text":"a".repeat(CANONICAL_CACHE_TEST_TEXT_BYTES)}]}]})
            .to_string(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    engine
}

fn canonical_cache_insert(
    engine: &YrsDocumentEngine,
    request_id: u64,
    policy: HistoryPolicy,
) -> TypedTransaction {
    TypedTransaction {
        request_id,
        base_document_revision: engine.revision(),
        origin: TransactionOrigin::LocalInput,
        operations: vec![TypedOperation::InsertText {
            at: RevisionedPosition {
                offset: 1,
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::After,
            },
            text: "🙂".into(),
            marks: vec![],
        }],
        selection_intent: SelectionIntent::UseOperationResult,
        history_policy: policy,
    }
}

#[test]
fn canonical_cache_preserves_history_admission_grouping_rollover_and_undo() {
    use crate::yrs_engine::canonical::CanonicalSpliceCache;
    let mut cached = canonical_cache_engine();
    let mut uncached = canonical_cache_engine();
    let mut saw_cache = false;
    let mut saw_eviction = false;
    let mut saw_reseed = false;
    for index in 0..48 {
        let policy = if index % 3 == 0 {
            HistoryPolicy::Boundary
        } else {
            HistoryPolicy::Auto
        };
        let request_id = 108_300 + index;
        cached
            .apply_typed_transaction(canonical_cache_insert(&cached, request_id, policy))
            .unwrap();
        CanonicalSpliceCache::without_for_test(|| {
            uncached.apply_typed_transaction(canonical_cache_insert(&uncached, request_id, policy))
        })
        .unwrap();
        assert_eq!(
            cached.document_json(),
            uncached.document_json(),
            "edit={index}"
        );
        assert_eq!(
            cached.history.replay_metadata_bytes_for_test(),
            uncached.history.replay_metadata_bytes_for_test(),
            "edit={index}: cache cannot change history metadata"
        );
        assert_eq!(cached.can_undo(), uncached.can_undo());
        assert_eq!(
            cached.history.retained_units(request_id).unwrap(),
            uncached.history.retained_units(request_id).unwrap()
        );
        if let Some(cache) = &cached.canonical_splice_cache {
            saw_reseed |= saw_eviction;
            saw_cache = true;
            assert!(
                cache.retained_bytes().unwrap()
                    <= cached
                        .history
                        .cache_metadata_headroom(request_id, 0)
                        .unwrap()
            );
        } else {
            saw_eviction |= saw_cache;
        }
        if index == 0 {
            assert!(
                saw_cache,
                "The fixture must exercise the actual over-budget snapshot cache path"
            );
        }
    }
    assert!(
        saw_eviction && saw_reseed,
        "Ordinary history growth must evict the cache; a later rollover allows seeding again"
    );
    let mut request_id = 108_400;
    while cached.can_undo() || uncached.can_undo() {
        assert_eq!(
            cached.undo(request_id).unwrap().is_some(),
            uncached.undo(request_id).unwrap().is_some()
        );
        assert_eq!(cached.document_json(), uncached.document_json());
        assert!(cached.canonical_splice_cache.is_none());
        request_id += 1;
    }
    while cached.can_redo() || uncached.can_redo() {
        assert_eq!(
            cached.redo(request_id).unwrap().is_some(),
            uncached.redo(request_id).unwrap().is_some()
        );
        assert_eq!(cached.document_json(), uncached.document_json());
        assert!(cached.canonical_splice_cache.is_none());
        request_id += 1;
    }
}

#[test]
fn canonical_cache_drops_failed_staging_and_invalidates_external_transitions() {
    let mut engine = canonical_cache_engine();
    engine
        .apply_typed_transaction(canonical_cache_insert(
            &engine,
            108_500,
            HistoryPolicy::Boundary,
        ))
        .unwrap();
    assert!(engine.canonical_splice_cache.is_some());
    let stale = engine
        .compile_typed_transaction(canonical_cache_insert(
            &engine,
            108_510,
            HistoryPolicy::Boundary,
        ))
        .unwrap();
    engine
        .apply_typed_transaction(canonical_cache_insert(
            &engine,
            108_511,
            HistoryPolicy::Boundary,
        ))
        .unwrap();
    let current = atomic_audit(&engine);
    assert!(engine.apply_compiled_transaction(stale, false).is_err());
    assert_eq!(atomic_audit(&engine), current);
    assert!(
        engine.canonical_splice_cache.is_some(),
        "Rejecting a stale prepared command preserves current cache authority"
    );
    let before = atomic_audit(&engine);
    set_compiled_commit_stage_failpoint_for_test(Some(
        CompiledCommitPreparationStage::HistoryReservation,
    ));
    let failed = engine.apply_typed_transaction(canonical_cache_insert(
        &engine,
        108_501,
        HistoryPolicy::Boundary,
    ));
    set_compiled_commit_stage_failpoint_for_test(None);
    assert!(failed.is_err());
    assert_eq!(
        atomic_audit(&engine),
        before,
        "Failed staging must not publish history, document or revision changes"
    );
    assert!(engine.canonical_splice_cache.is_none());
    engine
        .apply_typed_transaction(canonical_cache_insert(
            &engine,
            108_502,
            HistoryPolicy::Boundary,
        ))
        .unwrap();
    assert!(engine.canonical_splice_cache.is_some());
    let snapshot = engine.export_snapshot().unwrap();
    engine.restore_snapshot(&snapshot).unwrap();
    assert!(
        engine.canonical_splice_cache.is_none(),
        "Even an unchanged snapshot reset invalidates the cache"
    );
    engine
        .apply_typed_transaction(canonical_cache_insert(
            &engine,
            108_503,
            HistoryPolicy::Boundary,
        ))
        .unwrap();
    assert!(engine.canonical_splice_cache.is_some());
    let json = engine.document_json_string().unwrap();
    engine
        .import_json(&json, TransactionOrigin::DocumentImport)
        .unwrap();
    assert!(
        engine.canonical_splice_cache.is_none(),
        "Unchanged import resets history authority"
    );
    engine
        .apply_typed_transaction(canonical_cache_insert(
            &engine,
            108_504,
            HistoryPolicy::Boundary,
        ))
        .unwrap();
    assert!(engine.canonical_splice_cache.is_some());
    let mut peer = transaction_engine_with_resource_limits_and_mode(
        ResourceLimits::default(),
        crate::yrs_engine::InitializationMode::AwaitRemote,
    );
    peer.apply_remote_update_v1(108_505, &engine.encoded_state().unwrap())
        .unwrap();
    peer.apply_typed_transaction(canonical_cache_insert(
        &peer,
        108_506,
        HistoryPolicy::Boundary,
    ))
    .unwrap();
    engine
        .apply_remote_update_v1(108_507, &peer.encoded_state().unwrap())
        .unwrap();
    assert!(engine.canonical_splice_cache.is_none());
    assert_eq!(engine.document_json(), peer.document_json());
}

#[test]
fn optional_canonical_cache_allocation_failure_preserves_edit_and_history() {
    use crate::yrs_engine::canonical::{CacheAllocation, CanonicalSpliceCache};
    for allocation in [CacheAllocation::Buffer, CacheAllocation::Path] {
        let failed_request = match allocation {
            CacheAllocation::Buffer => 108_601,
            CacheAllocation::Path => 108_600,
        };
        let mut cached = canonical_cache_engine();
        let mut uncached = canonical_cache_engine();
        for request_id in 108_600..108_603 {
            let apply = || {
                cached.apply_typed_transaction(canonical_cache_insert(
                    &cached,
                    request_id,
                    HistoryPolicy::Boundary,
                ))
            };
            let actual = if request_id == failed_request {
                CanonicalSpliceCache::failing_allocation_for_test(allocation, apply)
            } else {
                let mut apply = apply;
                apply()
            }
            .unwrap();
            let expected = CanonicalSpliceCache::without_for_test(|| {
                uncached.apply_typed_transaction(canonical_cache_insert(
                    &uncached,
                    request_id,
                    HistoryPolicy::Boundary,
                ))
            })
            .unwrap();
            assert_eq!(
                actual, expected,
                "allocation={allocation:?}, request={request_id}"
            );
            assert_eq!(cached.document_json(), uncached.document_json());
            assert_eq!(
                cached.history.stack_depths_for_test(),
                uncached.history.stack_depths_for_test()
            );
            assert_eq!(
                cached.history.replay_metadata_bytes_for_test(),
                uncached.history.replay_metadata_bytes_for_test()
            );
            assert_eq!(
                cached.canonical_splice_cache.is_some(),
                request_id != failed_request
            );
        }
        for request_id in 108_610..108_613 {
            assert_eq!(
                cached.undo(request_id).unwrap(),
                uncached.undo(request_id).unwrap()
            );
            assert_eq!(cached.document_json(), uncached.document_json());
        }
    }
}

#[test]
fn canonical_cache_edit_after_undo_preserves_redo_metadata_admission() {
    use crate::yrs_engine::canonical::CanonicalSpliceCache;
    let mut cached = canonical_cache_engine();
    let mut uncached = canonical_cache_engine();
    for request_id in 108_700..108_710 {
        cached
            .apply_typed_transaction(canonical_cache_insert(
                &cached,
                request_id,
                HistoryPolicy::Boundary,
            ))
            .unwrap();
        CanonicalSpliceCache::without_for_test(|| {
            uncached.apply_typed_transaction(canonical_cache_insert(
                &uncached,
                request_id,
                HistoryPolicy::Boundary,
            ))
        })
        .unwrap();
    }
    for request_id in 108_710..108_713 {
        assert_eq!(
            cached.undo(request_id).unwrap(),
            uncached.undo(request_id).unwrap()
        );
    }
    assert!(cached.can_redo());
    assert!(cached.canonical_splice_cache.is_none());
    let pending = crate::yrs_engine::engine::history_state::history_metadata_bytes(
        None,
        &cached.fragment_name,
    ) * 2;
    let headroom = cached
        .history
        .cache_metadata_headroom(108_720, pending)
        .unwrap();
    assert!(
        headroom < CANONICAL_CACHE_TEST_LIMIT,
        "Undo and redo metadata must remain charged before the new edit"
    );
    let actual = cached
        .apply_typed_transaction(canonical_cache_insert(
            &cached,
            108_720,
            HistoryPolicy::Boundary,
        ))
        .unwrap();
    let expected = CanonicalSpliceCache::without_for_test(|| {
        uncached.apply_typed_transaction(canonical_cache_insert(
            &uncached,
            108_720,
            HistoryPolicy::Boundary,
        ))
    })
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(cached.document_json(), uncached.document_json());
    assert_eq!(
        cached.history.replay_metadata_bytes_for_test(),
        uncached.history.replay_metadata_bytes_for_test()
    );
    assert!(!cached.can_redo());
    assert_eq!(cached.can_redo(), uncached.can_redo());
    let cache = cached.canonical_splice_cache.as_ref().expect(
        "The post-undo edit must seed a cache within headroom including live redo and both pending snapshots",
    );
    assert!(cache.retained_bytes().unwrap() <= headroom);
    let mut request_id = 108_730;
    while cached.can_undo() || uncached.can_undo() {
        assert_eq!(
            cached.undo(request_id).unwrap(),
            uncached.undo(request_id).unwrap()
        );
        assert_eq!(cached.document_json(), uncached.document_json());
        request_id += 1;
    }
}
