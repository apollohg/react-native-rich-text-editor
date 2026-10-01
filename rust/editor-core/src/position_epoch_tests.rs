use super::*;
use crate::test_support::large_table_fixture::{plain_table_document, session_with_document};

#[test]
fn rejected_epoch_updates_preserve_installed_state_and_error_order() {
    const OWNER: u64 = 41;
    const OTHER_OWNER: u64 = 42;
    enum Rejection {
        Boundaries,
        Owners,
        RetainedBytes,
        Overflow,
        Exhaustion,
    }
    for rejection in [
        Rejection::Boundaries,
        Rejection::Owners,
        Rejection::RetainedBytes,
        Rejection::Overflow,
        Rejection::Exhaustion,
    ] {
        let session = session_with_document(&plain_table_document(2, 2));
        let mut latest = Arc::new(session.engine.build_position_epoch_snapshot().unwrap());
        let mut store = PositionEpochStore::new(PositionEpochLimits::default());
        let lineage = session.engine.client_id();
        let epoch = store.install(OWNER, lineage, latest.clone()).unwrap();
        let mut anchors = latest.chunks[0].anchors.clone();
        anchors.push(anchors.last().unwrap().clone());
        let update = EpochSnapshotUpdate::new(
            &latest,
            latest.yrs_state_epoch + 1,
            latest.document_revision + 1,
            vec![Arc::new(EpochBlockChunk::new(anchors).unwrap())],
            vec![],
            vec![0],
            vec![],
        )
        .unwrap();
        assert!(update.retained_bytes > latest.retained_bytes);
        assert!(update.boundary_count > latest.boundary_count());
        let (target, expected_code, expected_field) = match rejection {
            Rejection::Boundaries => {
                store.limits.max_boundaries = latest.boundary_count();
                store.limits.max_retained_bytes = latest.retained_bytes;
                (
                    OWNER,
                    "POSITION_EPOCH_LIMIT_EXCEEDED",
                    Some("maxPositionEpochBoundaries"),
                )
            }
            Rejection::Owners => {
                store.limits.max_owners = 1;
                store.limits.max_retained_bytes = latest.retained_bytes;
                (
                    OTHER_OWNER,
                    "POSITION_EPOCH_LIMIT_EXCEEDED",
                    Some("maxPositionEpochOwners"),
                )
            }
            Rejection::RetainedBytes => {
                store.limits.max_retained_bytes = latest.retained_bytes;
                (
                    OWNER,
                    "POSITION_EPOCH_LIMIT_EXCEEDED",
                    Some("maxPositionEpochRetainedBytes"),
                )
            }
            Rejection::Overflow => {
                store.retained_bytes = usize::MAX;
                (
                    OWNER,
                    "POSITION_EPOCH_LIMIT_EXCEEDED",
                    Some("maxPositionEpochRetainedBytes"),
                )
            }
            Rejection::Exhaustion => (OWNER, "POSITION_EPOCH_EXHAUSTED", None),
        };
        store.next_epoch_id = u64::MAX;
        let before_next = store.next_epoch_id;
        let before_charge = store.retained_bytes;
        let before_snapshot = Arc::as_ptr(&latest);
        let before_chunks = latest.chunks.as_ptr();
        let before_cells = latest.cells.as_ptr();
        let before_starts = latest.scalar_starts.to_vec();
        let before_revision = latest.document_revision;
        let before_chunk_length = latest.chunks[0].anchors.len();
        let error = store
            .install_update(target, lineage, &mut latest, update)
            .unwrap_err();
        assert_eq!(error.code, expected_code);
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|value| value.get("field"))
                .and_then(|value| value.as_str()),
            expected_field
        );
        assert_eq!(
            store.next_epoch_id, before_next,
            "rejection must not consume an epoch identifier"
        );
        assert_eq!(
            store.retained_bytes, before_charge,
            "rejection must not alter accounting"
        );
        assert_eq!(Arc::as_ptr(&latest), before_snapshot);
        assert_eq!(latest.chunks.as_ptr(), before_chunks);
        assert_eq!(latest.cells.as_ptr(), before_cells);
        assert_eq!(latest.scalar_starts.as_ref(), before_starts);
        assert_eq!(latest.document_revision, before_revision);
        assert_eq!(latest.chunks[0].anchors.len(), before_chunk_length);
        assert_eq!(store.owner_pins.get(&OWNER), Some(&epoch));
        assert_eq!(
            store
                .boundary(OWNER, epoch, lineage, 0)
                .unwrap()
                .document_revision,
            before_revision
        );
    }
}

#[test]
fn staged_cell_points_only_attach_to_rebuilt_exclusive_chunks() {
    let session = session_with_document(&plain_table_document(2, 2));
    let previous = session.engine.build_position_epoch_snapshot().unwrap();
    let cell = previous.cells[0].clone();
    assert!(!cell.points.is_empty());
    let block = cell.block_range.start;
    let other = block + 1;
    let other_chunk =
        Arc::new(EpochBlockChunk::new(previous.chunks[other].anchors.clone()).unwrap());
    assert!(
        EpochSnapshotUpdate::new(
            &previous,
            previous.yrs_state_epoch + 1,
            previous.document_revision + 1,
            vec![other_chunk],
            vec![cell.clone()],
            vec![other],
            vec![0]
        )
        .is_none(),
        "A changed cell must not mutate an unchanged chunk"
    );
    let rebuilt = Arc::new(EpochBlockChunk::new(previous.chunks[block].anchors.clone()).unwrap());
    let retained = rebuilt.clone();
    assert!(
        EpochSnapshotUpdate::new(
            &previous,
            previous.yrs_state_epoch + 1,
            previous.document_revision + 1,
            vec![rebuilt],
            vec![cell.clone()],
            vec![block],
            vec![0]
        )
        .is_none(),
        "Shared rebuilt chunks cannot be modified"
    );
    assert_eq!(retained.anchors, previous.chunks[block].anchors);
    let rebuilt = Arc::new(EpochBlockChunk::new(previous.chunks[block].anchors.clone()).unwrap());
    assert!(
        EpochSnapshotUpdate::new(
            &previous,
            previous.yrs_state_epoch + 1,
            previous.document_revision + 1,
            vec![rebuilt],
            vec![cell],
            vec![block],
            vec![0]
        )
        .is_some(),
        "An exclusive rebuilt chunk accepts its cell points"
    );
}
