use super::large_table_fixture::{plain_table_document, session_with_document};
use crate::boundary::ResourceLimits;
use crate::schema::presets::tiptap_schema;
use crate::session::{
    CollaborationLimits, DocumentState, EditorSession, EditorSessionConfig, SessionPolicy,
};
use crate::yrs_engine::{
    Affinity, EditingLimits, EditorOffsetKind, HistoryPolicy, InitializationMode,
    ReplacementHistory, RevisionedPosition, SelectionInput, SelectionIntent, TransactionOrigin,
    TypedCommand, TypedTransaction, YrsDocumentEngine, YrsEngineConfig,
};

fn engine(mode: InitializationMode) -> YrsDocumentEngine {
    engine_with_schema(mode, tiptap_schema())
}

fn engine_with_schema(
    mode: InitializationMode,
    schema: crate::schema::Schema,
) -> YrsDocumentEngine {
    YrsDocumentEngine::new(YrsEngineConfig {
        schema,
        fragment_name: "prosemirror".into(),
        initialization_mode: mode,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: Some(crate::yrs_engine::DocumentScope {
            document_id: "position-epoch".into(),
            lineage_id: "position-epoch-lineage".into(),
        }),
    })
    .unwrap()
}

fn session_for_engine(engine: YrsDocumentEngine) -> EditorSession {
    let config = EditorSessionConfig::local_for_test();
    EditorSession::new(
        engine,
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap()
}

fn session_with_text(text: &str) -> EditorSession {
    let mut session = session_for_engine(engine(InitializationMode::LocalEmpty));
    let document = serde_json::json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{"type": "text", "text": text}],
        }],
    });
    session
        .replace_document_json(1, &document.to_string(), ReplacementHistory::ResetAndClear)
        .unwrap();
    session
}

fn point(offset: u32) -> RevisionedPosition {
    RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    }
}

fn select_at(engine: &mut YrsDocumentEngine, request_id: u64, offset: u32) {
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![],
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point(offset),
                head: point(offset),
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
}

fn insert_at(engine: &mut YrsDocumentEngine, request_id: u64, offset: u32, text: &str) {
    select_at(engine, request_id, offset);
    engine
        .apply_command(
            request_id + 1,
            TypedCommand::InsertText { text: text.into() },
        )
        .unwrap();
}

fn replica_of(session: &EditorSession) -> YrsDocumentEngine {
    let mut replica = engine_with_schema(
        InitializationMode::AwaitRemote,
        session.engine.schema().clone(),
    );
    replica
        .apply_remote_update_v1(10, &session.engine.encoded_state().unwrap())
        .unwrap();
    replica
}

fn apply_replica(session: &mut EditorSession, replica: &YrsDocumentEngine, request_id: u64) {
    session
        .engine
        .apply_remote_update_v1(request_id, &replica.encoded_state().unwrap())
        .unwrap();
}

fn selected_text(session: &EditorSession, anchor: u32, head: u32) -> String {
    let text = session.engine.document().unwrap().root().text_content();
    let start = anchor.min(head) as usize;
    let end = anchor.max(head) as usize;
    text.chars().skip(start).take(end - start).collect()
}

#[test]
fn epoch_range_resolves_after_multiple_remote_revisions() {
    let mut session = session_with_text("abcd retained compact Unicode 😀 anchors");
    let epoch = session
        .pin_position_epoch(7, session.engine.revision())
        .unwrap();
    let retained = session.latest_epoch_snapshot_for_test().unwrap();
    assert!(retained.chunks[0].stored_anchor_count() < retained.chunks[0].anchors.len());
    let mut replica = replica_of(&session);

    insert_at(&mut replica, 20, 2, "R");
    apply_replica(&mut session, &replica, 21);
    insert_at(&mut replica, 22, 0, "S");
    apply_replica(&mut session, &replica, 23);

    let resolved = session.resolve_epoch_range(7, epoch, 1, 3).unwrap();

    assert_eq!(
        selected_text(&session, resolved.anchor, resolved.head),
        "bRc"
    );
}

#[test]
fn epoch_preserves_reversed_unicode_selection() {
    let mut session = session_with_text("a😀bc");
    let epoch = session
        .pin_position_epoch(9, session.engine.revision())
        .unwrap();
    let mut replica = replica_of(&session);

    insert_at(&mut replica, 30, 0, "Ω");
    apply_replica(&mut session, &replica, 31);

    let resolved = session.resolve_epoch_range(9, epoch, 4, 1).unwrap();

    assert!(resolved.anchor > resolved.head);
    assert_eq!(
        selected_text(&session, resolved.anchor, resolved.head),
        "😀bc"
    );
}

#[test]
fn epoch_is_owner_scoped_and_release_is_terminal() {
    let mut session = session_with_text("abcd");
    let epoch = session
        .pin_position_epoch(11, session.engine.revision())
        .unwrap();

    let foreign = session.resolve_epoch_range(12, epoch, 1, 2).unwrap_err();
    assert_eq!(foreign.code, "POSITION_EPOCH_INVALID");

    session.release_position_epoch_owner(11);
    let released = session.resolve_epoch_range(11, epoch, 1, 2).unwrap_err();
    assert_eq!(released.code, "POSITION_EPOCH_INVALID");
}

#[test]
fn deleting_both_leaf_targets_resolves_through_structural_fallback() {
    let mut session = session_with_text("abcd");
    let epoch = session
        .pin_position_epoch(13, session.engine.revision())
        .unwrap();
    let mut replica = replica_of(&session);
    let replacement = serde_json::json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{"type": "text", "text": "z"}],
        }],
    });
    replica
        .prepare_root_replacement_json(
            40,
            &replacement.to_string(),
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    apply_replica(&mut session, &replica, 41);

    let resolved = session.resolve_epoch_range(13, epoch, 1, 3).unwrap();

    assert!(resolved.fallback);
    assert!(resolved.anchor <= 1);
    assert!(resolved.head <= 1);
}

#[test]
fn replacing_an_owner_pin_invalidates_only_its_previous_epoch() {
    let mut session = session_with_text("abcd");
    let first = session
        .pin_position_epoch(15, session.engine.revision())
        .unwrap();
    let other = session
        .pin_position_epoch(16, session.engine.revision())
        .unwrap();
    let replacement = session
        .pin_position_epoch(15, session.engine.revision())
        .unwrap();

    assert_ne!(first, replacement);
    assert_eq!(
        session
            .resolve_epoch_range(15, first, 0, 0)
            .unwrap_err()
            .code,
        "POSITION_EPOCH_INVALID",
    );
    session.resolve_epoch_range(15, replacement, 0, 0).unwrap();
    session.resolve_epoch_range(16, other, 0, 0).unwrap();
}

#[test]
fn pinned_owner_count_is_bounded_without_evicting_live_epochs() {
    let mut session = session_with_text("a");
    let revision = session.engine.revision();
    let first = session.pin_position_epoch(1, revision).unwrap();
    for owner_id in 2..=64 {
        session.pin_position_epoch(owner_id, revision).unwrap();
    }

    let error = session.pin_position_epoch(65, revision).unwrap_err();

    assert_eq!(error.code, "POSITION_EPOCH_LIMIT_EXCEEDED");
    session.resolve_epoch_range(1, first, 0, 1).unwrap();
}

#[test]
fn unchanged_multi_paragraph_boundary_resolves_to_the_same_scalar() {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        engine(InitializationMode::LocalEmpty),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            1,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]},{"type":"paragraph","content":[{"type":"text","text":"Beta"}]},{"type":"paragraph","content":[{"type":"text","text":"Gamma"}]}]}"#,
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let epoch = session
        .pin_position_epoch(17, session.engine.revision())
        .unwrap();

    let resolved = session.resolve_epoch_range(17, epoch, 10, 10).unwrap();

    assert_eq!(resolved.anchor, 10);
    assert_eq!(resolved.head, 10);
}

#[test]
fn unchanged_nested_list_boundaries_resolve_to_the_exact_scalar() {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        engine(InitializationMode::LocalEmpty),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            1,
            r#"{"type":"doc","content":[{"type":"blockquote","content":[{"type":"paragraph","content":[{"type":"text","text":"Quote"}]}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"Nested"}]}]}]}]}]},{"type":"paragraph"}]}"#,
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let scalar_limit = session.engine.position_map().unwrap().total_scalars();
    let epoch = session
        .pin_position_epoch(18, session.engine.revision())
        .unwrap();

    for scalar in 0..=scalar_limit {
        let resolved = session
            .resolve_epoch_range(18, epoch, scalar, scalar)
            .unwrap();
        assert_eq!(
            (resolved.anchor, resolved.head),
            (scalar, scalar),
            "unchanged scalar {scalar} drifted"
        );
        assert!(!resolved.fallback);
    }
}

const LARGE_TABLE_OWNER: u64 = 7;

#[test]
fn twenty_thousand_slot_tables_pin_an_epoch_whose_ancestors_are_shared_per_element() {
    for (rows, columns) in [(1000, 20), (100, 200)] {
        let mut session = session_with_document(&plain_table_document(rows, columns));
        let boundaries = session.engine.build_position_epoch_snapshot().unwrap();
        let cells = rows * columns;
        let elements = 1 + rows + 2 * cells;
        eprintln!(
            "{rows}x{columns}: {} boundaries share {} ancestor anchors",
            boundaries.boundary_count(),
            boundaries.ancestor_count()
        );
        assert_eq!(
            boundaries.ancestor_count(),
            elements,
            "{rows}x{columns}: one ancestor anchor per entered element, not per boundary"
        );
        let last = boundaries.boundary_count() - 2;
        assert!(
            boundaries
                .boundary(u32::try_from(last).unwrap())
                .unwrap()
                .anchors
                .inside_table_cell(),
            "{rows}x{columns}: the last cell's text keeps its table cell ancestry"
        );

        let pinned = session.pin_position_epoch(LARGE_TABLE_OWNER, session.engine.revision());

        assert!(
            pinned.is_ok(),
            "{rows}x{columns}: the 20,000-slot fixture pins under the default budget, got {pinned:?}"
        );
    }
}

#[test]
fn shared_epoch_snapshots_are_charged_in_full_for_each_owner() {
    use crate::position_epoch::{PositionEpochLimits, PositionEpochStore};
    use std::sync::Arc;

    const FIRST_OWNER: u64 = 17;
    const SECOND_OWNER: u64 = 18;
    const OWNER_LIMIT: usize = 2;
    let session = session_with_document(&plain_table_document(2, 2));
    let snapshot = Arc::new(session.engine.build_position_epoch_snapshot().unwrap());
    let lineage = session.engine.client_id();
    let mut store = PositionEpochStore::new(PositionEpochLimits {
        max_owners: OWNER_LIMIT,
        max_boundaries: snapshot.boundary_count(),
        max_retained_bytes: snapshot.retained_bytes,
    });
    let first = store
        .install(FIRST_OWNER, lineage, snapshot.clone())
        .unwrap();
    assert!(store.boundary(FIRST_OWNER, first, lineage, 0).is_ok());
    let error = store
        .install(SECOND_OWNER, lineage, snapshot.clone())
        .unwrap_err();
    assert_eq!(error.code, "POSITION_EPOCH_LIMIT_EXCEEDED");
    assert_eq!(
        error.details,
        Some(serde_json::json!({"field": "maxPositionEpochRetainedBytes"}))
    );
    assert!(
        store.boundary(FIRST_OWNER, first, lineage, 0).is_ok(),
        "rejection preserves the existing owner"
    );
    let replacement = store
        .install(FIRST_OWNER, lineage, snapshot.clone())
        .unwrap();
    assert!(store.boundary(FIRST_OWNER, first, lineage, 0).is_err());
    let last = u32::try_from(snapshot.boundary_count() - 1).unwrap();
    assert!(store
        .boundary(FIRST_OWNER, replacement, lineage, last)
        .is_ok());
    assert_eq!(
        store
            .boundary(FIRST_OWNER, replacement, lineage, last + 1)
            .err()
            .unwrap()
            .code,
        "POSITION_INVALID"
    );
    store.release_owner(FIRST_OWNER);
    assert!(
        store.install(SECOND_OWNER, lineage, snapshot).is_ok(),
        "release returns the full charge"
    );
}

const INCREMENTAL_EPOCH_OWNER: u64 = 301;
const INCREMENTAL_EPOCH_REQUEST: u64 = 302;

fn native_epoch_edit(
    session: &mut EditorSession,
    request: u64,
    anchor: u32,
    head: u32,
    text: &str,
) -> Result<(), crate::session::SessionError> {
    let epoch = session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    submit_epoch_edit(session, epoch, request, anchor, head, text)
}

fn submit_epoch_edit(
    session: &mut EditorSession,
    epoch: u64,
    request: u64,
    anchor: u32,
    head: u32,
    text: &str,
) -> Result<(), crate::session::SessionError> {
    crate::native_transaction_bridge::NativeTransactionBridge::new(session)
        .submit_native_intent(&serde_json::json!({
            "version": 1, "ownerId": INCREMENTAL_EPOCH_OWNER.to_string(),
            "requestId": request.to_string(), "positionEpoch": epoch.to_string(),
            "intent": {"type": "replaceSelectionText", "anchor": anchor, "head": head, "text": text}
        }).to_string()).map(|_| ())
}

#[test]
fn a_keystroke_pin_rebuilds_only_its_cell() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const ROWS: usize = 1000;
    const COLUMNS: usize = 20;
    let mut session = session_with_document(&plain_table_document(ROWS, COLUMNS));
    let index = session.engine.position_map().unwrap().block_count() / 2;
    let scalar = session
        .engine
        .position_map()
        .unwrap()
        .effective_scalar_start(index);
    native_epoch_edit(&mut session, INCREMENTAL_EPOCH_REQUEST, scalar, scalar, "x").unwrap();
    let previous = session.latest_epoch_snapshot_for_test().unwrap();
    reset_full_pass_counts_for_test();
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    assert_eq!(
        counts.epoch_block_rebuilds, 1,
        "only the edited cell's paragraph rebuilds: {counts:#?}"
    );
    assert_eq!(
        counts.yrs_tree_walks, 0,
        "indexed chunks require no root walk: {counts:#?}"
    );
    let current = session.latest_epoch_snapshot_for_test().unwrap();
    for block in 0..previous.chunks.len() {
        assert_eq!(
            std::sync::Arc::ptr_eq(&previous.chunks[block], &current.chunks[block]),
            block != index,
            "chunk {block} sharing"
        );
        assert_eq!(
            std::sync::Arc::ptr_eq(&previous.cells[block], &current.cells[block]),
            block != index,
            "cell {block} sharing"
        );
    }
}

#[test]
fn a_single_owners_keystroke_reuses_obsolete_epoch_arrays() {
    const ROWS: usize = 1000;
    const COLUMNS: usize = 20;
    let mut session = session_with_document(&plain_table_document(ROWS, COLUMNS));
    let epoch = session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let previous = session.latest_epoch_snapshot_for_test().unwrap();
    let chunks = previous.chunks.as_ptr();
    let cells = previous.cells.as_ptr();
    drop(previous);
    submit_epoch_edit(&mut session, epoch, INCREMENTAL_EPOCH_REQUEST, 0, 0, "x").unwrap();
    let replacement = session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let current = session.latest_epoch_snapshot_for_test().unwrap();
    assert_eq!(
        current.chunks.as_ptr(),
        chunks,
        "The replaced owner's chunk array must not clone every unchanged Arc"
    );
    assert_eq!(
        current.cells.as_ptr(),
        cells,
        "The replaced owner's cell array must not clone every unchanged Arc"
    );
    assert_ne!(epoch, replacement);
    assert_eq!(
        session
            .resolve_epoch_range(INCREMENTAL_EPOCH_OWNER, epoch, 0, 0)
            .unwrap_err()
            .code,
        "POSITION_EPOCH_INVALID"
    );
    drop(current);
    assert_epoch_matches_fresh(&mut session, "reused 20,000-cell epoch arrays");
}

#[test]
fn epoch_array_reuse_preserves_external_references() {
    use crate::position_epoch::{EpochBlockChunk, EpochSnapshot, PinnedCellSpan};
    use std::sync::{Arc, Weak};
    const OTHER_OWNER: u64 = INCREMENTAL_EPOCH_OWNER + 1;
    enum Held {
        Snapshot(Arc<EpochSnapshot>),
        SnapshotWeak(Weak<EpochSnapshot>),
        Chunks(Arc<[Arc<EpochBlockChunk>]>),
        ChunksWeak(Weak<[Arc<EpochBlockChunk>]>),
        Cells(Arc<[Arc<PinnedCellSpan>]>),
        CellsWeak(Weak<[Arc<PinnedCellSpan>]>),
        Starts(Arc<[u32]>),
        Owner(u64),
    }
    const CASES: usize = 8;
    for case in 0..CASES {
        let mut session = session_with_document(&plain_table_document(2, 2));
        let epoch = session
            .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
            .unwrap();
        let previous = session.latest_epoch_snapshot_for_test().unwrap();
        let chunks = previous.chunks.as_ptr();
        let cells = previous.cells.as_ptr();
        let old_revision = previous.document_revision;
        let old_anchors = previous.chunks[0].anchors.to_dense();
        let old_cell = previous.cells[0].as_ref().clone();
        let old_starts = previous.scalar_starts.to_vec();
        let held = match case {
            0 => Held::Snapshot(previous.clone()),
            1 => Held::SnapshotWeak(Arc::downgrade(&previous)),
            2 => Held::Chunks(previous.chunks.clone()),
            3 => Held::ChunksWeak(Arc::downgrade(&previous.chunks)),
            4 => Held::Cells(previous.cells.clone()),
            5 => Held::CellsWeak(Arc::downgrade(&previous.cells)),
            6 => Held::Starts(previous.scalar_starts.clone()),
            _ => Held::Owner(
                session
                    .pin_position_epoch(OTHER_OWNER, session.engine.revision())
                    .unwrap(),
            ),
        };
        drop(previous);
        submit_epoch_edit(&mut session, epoch, INCREMENTAL_EPOCH_REQUEST, 0, 0, "x").unwrap();
        session
            .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
            .unwrap();
        let current = session.latest_epoch_snapshot_for_test().unwrap();
        let reusable = matches!(held, Held::Starts(_));
        assert_eq!(
            current.chunks.as_ptr() == chunks,
            reusable,
            "chunk storage, held reference case {case}"
        );
        assert_eq!(
            current.cells.as_ptr() == cells,
            reusable,
            "cell storage, held reference case {case}"
        );
        match held {
            Held::Snapshot(old) => {
                assert_eq!(old.document_revision, old_revision);
                assert_eq!(old.chunks[0].anchors.to_dense(), old_anchors);
                assert_eq!(old.cells[0].as_ref(), &old_cell);
                assert_eq!(old.scalar_starts.as_ref(), old_starts);
            }
            Held::SnapshotWeak(old) => assert!(old.upgrade().is_none()),
            Held::Chunks(old) => assert_eq!(old[0].anchors.to_dense(), old_anchors),
            Held::ChunksWeak(old) => assert!(old.upgrade().is_none()),
            Held::Cells(old) => assert_eq!(old[0].as_ref(), &old_cell),
            Held::CellsWeak(old) => assert!(old.upgrade().is_none()),
            Held::Starts(old) => assert_eq!(old.as_ref(), old_starts),
            Held::Owner(old) => {
                assert!(session.resolve_epoch_range(OTHER_OWNER, old, 0, 0).is_ok())
            }
        }
        drop(current);
        assert_epoch_matches_fresh(&mut session, &format!("held reference case {case}"));
    }
}

#[test]
fn exclusive_epoch_arrays_reuse_while_another_owner_keeps_an_older_snapshot() {
    const OLDER_OWNER: u64 = INCREMENTAL_EPOCH_OWNER + 1;
    let mut session = session_with_document(&plain_table_document(2, 2));
    let older_epoch = session
        .pin_position_epoch(OLDER_OWNER, session.engine.revision())
        .unwrap();
    let older = session.latest_epoch_snapshot_for_test().unwrap();
    let older_revision = older.document_revision;
    let older_anchors = older.chunks[0].anchors.to_dense();
    let older_cells = older.cells.to_vec();
    native_epoch_edit(&mut session, INCREMENTAL_EPOCH_REQUEST, 0, 0, "x").unwrap();
    let first_epoch = session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let first = session.latest_epoch_snapshot_for_test().unwrap();
    assert!(!std::sync::Arc::ptr_eq(&older, &first));
    let chunks = first.chunks.as_ptr();
    let cells = first.cells.as_ptr();
    drop(first);
    submit_epoch_edit(
        &mut session,
        first_epoch,
        INCREMENTAL_EPOCH_REQUEST + 1,
        0,
        0,
        "y",
    )
    .unwrap();
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let current = session.latest_epoch_snapshot_for_test().unwrap();
    assert_eq!(current.chunks.as_ptr(), chunks);
    assert_eq!(current.cells.as_ptr(), cells);
    assert_eq!(older.document_revision, older_revision);
    assert_eq!(older.chunks[0].anchors.to_dense(), older_anchors);
    assert_eq!(older.cells.as_ref(), older_cells);
    assert!(session
        .resolve_epoch_range(OLDER_OWNER, older_epoch, 0, 0)
        .is_ok());
    drop(current);
    assert_epoch_matches_fresh(
        &mut session,
        "another owner retains a different older snapshot",
    );
}

#[test]
fn restoring_a_snapshot_clears_the_latest_epoch() {
    let mut session = session_with_text("snapshot epoch");
    let saved = session.export_snapshot(INCREMENTAL_EPOCH_REQUEST).unwrap();
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    assert!(session.latest_epoch_snapshot_for_test().is_some());
    session
        .restore_snapshot(INCREMENTAL_EPOCH_REQUEST + 1, &saved)
        .unwrap();
    assert!(session.latest_epoch_snapshot_for_test().is_none());
}

fn assert_epoch_matches_fresh(session: &mut EditorSession, label: &str) {
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let cached = session.latest_epoch_snapshot_for_test().unwrap();
    let fresh = session.engine.build_position_epoch_snapshot().unwrap();
    assert_eq!(
        cached.scalar_starts, fresh.scalar_starts,
        "{label}: scalar starts"
    );
    assert_eq!(
        cached.retained_bytes, fresh.retained_bytes,
        "{label}: retained charge"
    );
    assert_eq!(
        cached.cells, fresh.cells,
        "{label}: pinned cells and text points"
    );
    assert_eq!(
        cached.boundary_count(),
        fresh.boundary_count(),
        "{label}: boundary count"
    );
    for (index, (actual, expected)) in cached
        .chunks
        .iter()
        .flat_map(|chunk| chunk.anchors.iter())
        .zip(fresh.chunks.iter().flat_map(|chunk| chunk.anchors.iter()))
        .enumerate()
    {
        assert_eq!(
            actual, expected,
            "{label}: boundary {index}, including its ancestor chain"
        );
    }
}

#[test]
fn incremental_epochs_equal_full_rebuilds_structurally() {
    use crate::tables::commands::{TableCommand, TableEdge};
    const EPOCH_SEEDED_STEPS: usize = 500;
    const OPERATION_CYCLE: usize = 8;
    const EPOCH_RANDOM_SEED: u32 = 0x3f24a7c1;
    const RANDOM_MULTIPLIER: u32 = 1_664_525;
    const RANDOM_INCREMENT: u32 = 1_013_904_223;
    const REMOTE_REQUEST_OFFSET: u64 = 10_000;
    let fixtures = [
        ("3x3", plain_table_document(3, 3)),
        (
            "multi-paragraph/nested",
            super::large_table_fixture::multi_paragraph_cell_document(),
        ),
        (
            "prose",
            serde_json::json!({"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"alpha🦀 retained compact Unicode anchors"}]},
                {"type":"paragraph","content":[{"type":"text","text":"beta"}]},
                {"type":"paragraph"}
            ]}),
        ),
    ];
    for (label, source) in fixtures {
        let _clients = super::deterministic_clients::DeterministicClients::new();
        let mut session = session_with_document(&source);
        assert_epoch_matches_fresh(&mut session, label);
        if label == "prose" {
            let initial = session.latest_epoch_snapshot_for_test().unwrap();
            assert!(initial.chunks[0].stored_anchor_count() < initial.chunks[0].anchors.len());
        }
        let mut random = EPOCH_RANDOM_SEED;
        for step in 0..EPOCH_SEEDED_STEPS {
            random = random
                .wrapping_mul(RANDOM_MULTIPLIER)
                .wrapping_add(RANDOM_INCREMENT);
            let request = INCREMENTAL_EPOCH_REQUEST + u64::try_from(step).unwrap();
            let map = session.engine.position_map().unwrap();
            let block_index = step % map.block_count();
            let block = map.block(block_index).unwrap();
            let start = map.effective_scalar_start(block_index) + block.scalar_prefix_len;
            let length = block.scalar_len - block.scalar_prefix_len;
            let nested = (1..block.node_path.len())
                .filter(|depth| {
                    let node = session
                        .engine
                        .document()
                        .unwrap()
                        .node_at(&block.node_path[..*depth])
                        .unwrap();
                    session
                        .engine
                        .schema()
                        .node(node.node_type())
                        .is_some_and(|spec| {
                            spec.table_role == Some(crate::tables::TableRole::Table)
                        })
                })
                .count()
                > 1;
            if nested && matches!(step % OPERATION_CYCLE, 0 | 1 | 2 | 7) {
                let revision = session.engine.revision();
                let error =
                    native_epoch_edit(&mut session, request, start, start, "x").unwrap_err();
                assert_eq!(error.code, "OPERATION_INVALID");
                assert_eq!(
                    error.details,
                    Some(serde_json::json!({"field":"nestedTable"}))
                );
                assert_eq!(session.engine.revision(), revision);
                assert_epoch_matches_fresh(
                    &mut session,
                    &format!("{label} rejected nested edit {step}"),
                );
                continue;
            }
            let insert = start + random % (length + 1);
            let replace = start + random % length.max(1);
            match step % OPERATION_CYCLE {
                0 | 7 => native_epoch_edit(&mut session, request, insert, insert, "x").unwrap(),
                1 => native_epoch_edit(
                    &mut session,
                    request,
                    replace,
                    replace + u32::from(length > 0),
                    "🦀",
                )
                .unwrap(),
                2 => native_epoch_edit(
                    &mut session,
                    request,
                    replace,
                    replace + u32::from(length > 0),
                    "",
                )
                .unwrap(),
                3 => {
                    let command = if label == "prose" {
                        TypedCommand::SplitBlock
                    } else {
                        TypedCommand::Table(TableCommand::AddTableRow {
                            side: TableEdge::After,
                        })
                    };
                    session.engine.apply_command(request, command).unwrap();
                }
                4 => {
                    session
                        .engine
                        .undo(request)
                        .unwrap_or_else(|error| panic!("{label} step {step} undo: {error:?}"));
                }
                5 => {
                    session
                        .engine
                        .redo(request)
                        .unwrap_or_else(|error| panic!("{label} step {step} redo: {error:?}"));
                }
                6 => {
                    let mut peer = replica_of(&session);
                    insert_at(&mut peer, request + REMOTE_REQUEST_OFFSET, 0, "r");
                    session
                        .engine
                        .apply_remote_update_v1(request, &peer.encoded_state().unwrap())
                        .unwrap();
                }
                _ => unreachable!(),
            }
            session
                .engine
                .validate_history_replay_for_test(request)
                .unwrap_or_else(|error| panic!("{label} step {step} replay: {error:?}"));
            assert_epoch_matches_fresh(&mut session, &format!("{label} step {step}"));
        }
    }
}

#[test]
fn a_gap_in_the_change_scope_log_forces_a_full_rebuild() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const LOG_GAP_EDITS: u64 = 65;
    let mut session = session_with_text("gap");
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let revision = session.engine.revision();
    for edit in 0..LOG_GAP_EDITS {
        insert_at(
            &mut session.engine,
            INCREMENTAL_EPOCH_REQUEST + edit * 2,
            0,
            "x",
        );
    }
    assert!(session.engine.change_scopes_since(revision).is_none());
    reset_full_pass_counts_for_test();
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    assert_eq!(
        counts.yrs_tree_walks, 1,
        "a missing revision must use a full build: {counts:#?}"
    );
    assert_epoch_matches_fresh(&mut session, "change-log gap");
}

#[test]
fn incremental_and_full_epochs_resolve_identically_after_remote_cell_changes() {
    use crate::tables::commands::{TableCommand, TableHeaderTarget};
    const REMOTE_CHANGE_REQUEST: u64 = 9001;
    for command in [
        TableCommand::DeleteTableRows,
        TableCommand::ToggleTableHeader {
            target: TableHeaderTarget::Cell,
        },
    ] {
        let deletes_cell = matches!(command, TableCommand::DeleteTableRows);
        let mut document = plain_table_document(3, 3);
        document["content"][0]["content"][0]["content"][1]["content"][0]["content"][0]["text"] =
            serde_json::json!("retained compact cell café 😀 anchors");
        let mut incremental = session_with_document(&document);
        native_epoch_edit(&mut incremental, INCREMENTAL_EPOCH_REQUEST, 0, 0, "x").unwrap();
        let incremental_epoch = incremental
            .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, incremental.engine.revision())
            .unwrap();
        let cached = incremental.latest_epoch_snapshot_for_test().unwrap();
        assert!(cached.chunks[1].stored_anchor_count() < cached.chunks[1].anchors.len());
        let mut full = session_for_engine(replica_of(&incremental));
        let full_epoch = full
            .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, full.engine.revision())
            .unwrap();
        let rebuilt = full.latest_epoch_snapshot_for_test().unwrap();
        let mut peer = replica_of(&incremental);
        select_at(&mut peer, REMOTE_CHANGE_REQUEST, 0);
        peer.apply_command(REMOTE_CHANGE_REQUEST + 1, TypedCommand::Table(command))
            .unwrap()
            .expect("remote cell change applies");
        let update = peer.encoded_state().unwrap();
        incremental
            .engine
            .apply_remote_update_v1(REMOTE_CHANGE_REQUEST, &update)
            .unwrap();
        full.engine
            .apply_remote_update_v1(REMOTE_CHANGE_REQUEST, &update)
            .unwrap();
        assert_eq!(cached.boundary_count(), rebuilt.boundary_count());
        for scalar in 0..u32::try_from(cached.boundary_count()).unwrap() {
            for affinity in [Affinity::Before, Affinity::After] {
                let actual = incremental.engine.resolve_position_epoch_boundary(
                    &cached.boundary(scalar).unwrap(),
                    affinity,
                    scalar,
                );
                let expected = full.engine.resolve_position_epoch_boundary(
                    &rebuilt.boundary(scalar).unwrap(),
                    affinity,
                    scalar,
                );
                assert_eq!(
                    actual, expected,
                    "remote deletion={deletes_cell}, scalar {scalar}, {affinity:?}"
                );
            }
        }
        let actual = submit_epoch_edit(
            &mut incremental,
            incremental_epoch,
            REMOTE_CHANGE_REQUEST + 2,
            0,
            0,
            "q",
        );
        let expected =
            submit_epoch_edit(&mut full, full_epoch, REMOTE_CHANGE_REQUEST + 2, 0, 0, "q");
        if deletes_cell {
            assert_eq!(actual.unwrap_err().code, "POSITION_EPOCH_CELL_REMOVED");
            assert_eq!(expected.unwrap_err().code, "POSITION_EPOCH_CELL_REMOVED");
        } else {
            actual.unwrap();
            expected.unwrap();
        }
        assert_eq!(
            incremental.engine.document_json(),
            full.engine.document_json()
        );
    }
}

#[test]
fn unchanged_epoch_reuses_its_snapshot_until_the_last_owner_releases() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const OTHER_OWNER: u64 = INCREMENTAL_EPOCH_OWNER + 1;
    let mut session = session_with_text("shared");
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let first = session.latest_epoch_snapshot_for_test().unwrap();
    reset_full_pass_counts_for_test();
    session
        .pin_position_epoch(OTHER_OWNER, session.engine.revision())
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    assert_eq!(counts.epoch_block_rebuilds, 0);
    assert_eq!(counts.yrs_tree_walks, 0);
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &session.latest_epoch_snapshot_for_test().unwrap()
    ));
    session.release_position_epoch_owner(INCREMENTAL_EPOCH_OWNER);
    assert!(session.latest_epoch_snapshot_for_test().is_some());
    session.release_native_binding(OTHER_OWNER);
    assert!(session.latest_epoch_snapshot_for_test().is_none());
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    session.teardown();
    assert!(session.latest_epoch_snapshot_for_test().is_none());
}

#[test]
fn releasing_the_latest_epochs_owner_drops_its_unowned_cache() {
    const OLDER_OWNER: u64 = INCREMENTAL_EPOCH_OWNER + 1;
    let mut session = session_with_text("older owner");
    let older = session
        .pin_position_epoch(OLDER_OWNER, session.engine.revision())
        .unwrap();
    native_epoch_edit(&mut session, INCREMENTAL_EPOCH_REQUEST, 0, 0, "x").unwrap();
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    session.release_position_epoch_owner(INCREMENTAL_EPOCH_OWNER);
    assert!(
        session.latest_epoch_snapshot_for_test().is_none(),
        "a released epoch cannot remain as an uncharged cache"
    );
    assert!(
        session
            .resolve_epoch_range(OLDER_OWNER, older, 0, 0)
            .is_ok(),
        "the older owner's pin remains valid"
    );
}

#[test]
fn a_cell_with_a_block_atom_rebuilds_without_losing_its_ancestor_chain() {
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const CELL_BLOCKS: usize = 3;
    let mut source = plain_table_document(1, 2);
    source["content"][0]["content"][0]["content"][0]["content"] = serde_json::json!([
        {"type":"paragraph","content":[{"type":"text","text":"before"}]},
        {"type":"horizontal_rule"},
        {"type":"paragraph","content":[{"type":"text","text":"after"}]}
    ]);
    let mut session = session_with_document(&source);
    native_epoch_edit(&mut session, INCREMENTAL_EPOCH_REQUEST, 0, 0, "x").unwrap();
    reset_full_pass_counts_for_test();
    session
        .pin_position_epoch(INCREMENTAL_EPOCH_OWNER, session.engine.revision())
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    assert_eq!(counts.epoch_block_rebuilds, CELL_BLOCKS, "{counts:#?}");
    assert_eq!(counts.yrs_tree_walks, 0, "{counts:#?}");
    assert_epoch_matches_fresh(&mut session, "cell with block atom");
}
