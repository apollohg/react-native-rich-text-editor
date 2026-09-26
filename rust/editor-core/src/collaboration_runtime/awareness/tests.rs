use super::*;
use serde_json::json;

const REQUEST_ID: u64 = 88;

fn runtime() -> CollaborationRuntime {
    CollaborationRuntime::new(&CollaborationLimits::default())
}

fn engine() -> YrsDocumentEngine {
    use crate::boundary::ResourceLimits;
    use crate::yrs_engine::{EditingLimits, InitializationMode, YrsEngineConfig};
    YrsDocumentEngine::new(YrsEngineConfig {
        schema: crate::schema::presets::tiptap_schema(),
        fragment_name: "prosemirror".into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap()
}

fn context<'a>(
    engine: &'a mut YrsDocumentEngine,
    transport_state: TransportState,
    limits: &'a CollaborationLimits,
) -> AwarenessContext<'a> {
    AwarenessContext {
        engine,
        transport_state,
        limits,
    }
}

#[test]
fn awareness_limits_mirror_the_session_collaboration_limit_fields() {
    let limits = CollaborationLimits {
        max_awareness_peers: 3,
        max_awareness_peer_bytes: 64,
        max_awareness_bytes: 256,
        ..CollaborationLimits::default()
    };
    assert_eq!(
        awareness_limits(&limits),
        AwarenessLimits {
            max_awareness_peers: 3,
            max_awareness_peer_bytes: 64,
            max_awareness_bytes: 256,
        },
    );
}

#[test]
fn local_intent_peer_byte_ceiling_rejects_before_json_deserialization() {
    let mut runtime = runtime();
    let mut engine = engine();
    let limits = CollaborationLimits {
        max_awareness_peer_bytes: 64,
        ..CollaborationLimits::default()
    };
    // This is deliberately invalid JSON. If serde_json is reached, the
    // refusal changes to AWARENESS_STATE_INVALID instead of the frozen
    // maxAwarenessPeerBytes resource-limit envelope.
    let oversized_intent = "[".repeat(limits.max_awareness_peer_bytes + 1);

    let error = runtime
        .set_awareness_intent(
            REQUEST_ID,
            &oversized_intent,
            context(&mut engine, TransportState::Disconnected, &limits),
        )
        .unwrap_err();

    assert_eq!(error.domain, ErrorDomain::Boundary, "{error:?}");
    assert_eq!(error.code, "INPUT_LIMIT_EXCEEDED", "{error:?}");
    assert_eq!(error.request_id, Some(REQUEST_ID), "{error:?}");
    assert_eq!(error.limit, Some(64), "{error:?}");
    assert_eq!(error.actual, Some(65), "{error:?}");
    assert_eq!(error.message, "input exceeds limit 64: 65", "{error:?}");
    assert_eq!(
        error.details.as_ref().unwrap()["field"],
        "maxAwarenessPeerBytes",
        "{error:?}",
    );
    assert_eq!(runtime.desired_awareness(), None);
    assert_eq!(runtime.outbox().pending_protocol_reply_count(), 0);
}

#[test]
fn next_deadline_is_the_minimum_of_renewal_and_earliest_expiry() {
    let mut state = AwarenessRuntimeState::new();
    assert_eq!(
        state.next_deadline_millis(TransportState::Synchronized),
        None
    );

    state.peer_activity.insert(7, 1_000);
    state.peer_activity.insert(8, 2_000);
    assert_eq!(
        state.next_deadline_millis(TransportState::Disconnected),
        Some(1_000 + AWARENESS_EXPIRY_MILLIS),
    );

    state.desired_state = Some(json!({"n": 1}));
    state.last_local_publish_millis = Some(4_000);
    // Renewal is earlier than the earliest expiry.
    assert_eq!(
        state.next_deadline_millis(TransportState::Synchronized),
        Some(4_000 + AWARENESS_RENEWAL_INTERVAL_MILLIS),
    );
    // A disconnected transport never schedules renewal.
    assert_eq!(
        state.next_deadline_millis(TransportState::Disconnected),
        Some(1_000 + AWARENESS_EXPIRY_MILLIS),
    );
    // An unpublished desired state on a synchronized transport is due at
    // the current deterministic time.
    state.last_local_publish_millis = None;
    state.now_millis = 9_000;
    assert_eq!(
        state.next_deadline_millis(TransportState::Synchronized),
        Some(9_000),
    );
}

#[test]
fn task8_fourth_remediation_local_max_timestamp_has_no_deadline() {
    let mut state = AwarenessRuntimeState::new();
    state.now_millis = u64::MAX;
    state.desired_state = Some(json!({"name": "local"}));
    state.last_local_publish_millis = Some(u64::MAX);

    assert_eq!(
        state.next_deadline_millis(TransportState::Synchronized),
        None,
    );
}

#[test]
fn task8_fourth_remediation_remote_max_timestamp_has_no_deadline() {
    let mut state = AwarenessRuntimeState::new();
    state.now_millis = u64::MAX;
    state.peer_activity.insert(7, u64::MAX);

    assert_eq!(
        state.next_deadline_millis(TransportState::Disconnected),
        None,
    );
}

#[test]
fn task8_fourth_remediation_mixed_deadlines_keep_the_representable_candidate() {
    let mut state = AwarenessRuntimeState::new();
    state.desired_state = Some(json!({"name": "local"}));
    state.last_local_publish_millis = Some(u64::MAX);
    state.peer_activity.insert(7, 1_000);
    assert_eq!(
        state.next_deadline_millis(TransportState::Synchronized),
        Some(1_000 + AWARENESS_EXPIRY_MILLIS),
    );

    state.last_local_publish_millis = Some(2_000);
    state.peer_activity.clear();
    state.peer_activity.insert(7, u64::MAX);
    assert_eq!(
        state.next_deadline_millis(TransportState::Synchronized),
        Some(2_000 + AWARENESS_RENEWAL_INTERVAL_MILLIS),
    );
}

#[test]
fn task8_fourth_remediation_tick_at_equal_max_does_no_false_clock_work() {
    let mut runtime = runtime();
    let mut engine = engine();
    let limits = CollaborationLimits::default();
    runtime.awareness.now_millis = u64::MAX;
    runtime.awareness.desired_state = Some(json!({"name": "local"}));
    runtime.awareness.last_local_publish_millis = Some(u64::MAX);
    runtime.awareness.peer_activity.insert(7, u64::MAX);

    let outcome = runtime
        .tick(
            REQUEST_ID,
            u64::MAX,
            context(&mut engine, TransportState::Synchronized, &limits),
        )
        .unwrap();

    assert!(!outcome.renewed_local, "{outcome:?}");
    assert!(!outcome.outbound_changed, "{outcome:?}");
    assert!(outcome.expired_peers.is_empty(), "{outcome:?}");
    assert!(!outcome.peers_changed, "{outcome:?}");
    assert_eq!(outcome.next_deadline_millis, None, "{outcome:?}");
    assert_eq!(runtime.awareness.last_local_publish_millis, Some(u64::MAX));
    assert_eq!(runtime.awareness.peer_activity.get(&7), Some(&u64::MAX));
    assert_eq!(runtime.outbox().pending_protocol_reply_count(), 0);
}

#[test]
fn reset_for_restore_clears_peer_bookkeeping_and_restarts_the_renewal_clock() {
    let mut state = AwarenessRuntimeState::new();
    state.now_millis = 9_000;
    state.desired_state = Some(json!({"n": 1}));
    state.last_local_publish_millis = Some(4_000);
    state.peer_activity.insert(7, 1_000);
    state.peer_activity.insert(8, 2_000);

    state.reset_for_restore();

    // The desired state survives; prior-store deadlines are gone.
    assert_eq!(state.desired_state, Some(json!({"n": 1})));
    assert!(state.peer_activity.is_empty());
    assert_eq!(state.last_local_publish_millis, None);
    // With no broadcast for the new store, a synchronized renewal is due
    // at the current deterministic time, never on the prior store's
    // schedule.
    assert_eq!(
        state.next_deadline_millis(TransportState::Synchronized),
        Some(9_000),
    );
    assert_eq!(
        state.next_deadline_millis(TransportState::Disconnected),
        None
    );
}

#[test]
fn peer_cursor_projection_requires_two_resolvable_sticky_points() {
    let mut engine = engine();
    engine
        .import_json(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"unit seed"}]}]}"#,
            crate::yrs_engine::TransactionOrigin::DocumentImport,
        )
        .unwrap();

    // Non-object states, missing cursors, and malformed points degrade.
    assert_eq!(peer_cursor_projection(&engine, &json!("plain")), None);
    assert_eq!(peer_cursor_projection(&engine, &json!({"name": "x"})), None);
    assert_eq!(
        peer_cursor_projection(
            &engine,
            &json!({"cursor": {"anchor": {"bogus": true}, "head": {"bogus": true}}}),
        ),
        None,
    );
    assert_eq!(
        peer_cursor_projection(&engine, &json!({"cursor": {"anchor": 1}})),
        None,
        "a cursor missing its head never resolves half a projection",
    );
}

#[test]
fn broadcast_reservation_errors_split_per_the_saturation_ruling() {
    let saturated = broadcast_reservation_error(
        OutboxReservationError::Saturated {
            field: super::super::outbox::OUTBOX_MESSAGES_FIELD,
            limit: 2,
            actual: 3,
        },
        REQUEST_ID,
    );
    assert_eq!(saturated.code, TRANSPORT_REPLY_LIMIT_EXCEEDED);
    assert_eq!(saturated.domain, ErrorDomain::Transport);
    assert_eq!(saturated.request_id, Some(REQUEST_ID));
    assert_eq!(saturated.limit, Some(2));
    assert_eq!(saturated.actual, Some(3));

    let allocation = broadcast_reservation_error(OutboxReservationError::Allocation, REQUEST_ID);
    assert_eq!(allocation.code, TRANSPORT_RESOURCE_EXHAUSTED);
    assert_eq!(allocation.request_id, Some(REQUEST_ID));
}

#[test]
fn set_desired_awareness_rejects_invalid_json_atomically() {
    let mut runtime = runtime();
    let mut engine = engine();
    let limits = CollaborationLimits::default();

    let error = runtime
        .set_desired_awareness_for_test(
            REQUEST_ID,
            "{broken",
            context(&mut engine, TransportState::Disconnected, &limits),
        )
        .unwrap_err();
    assert_eq!(error.code, AWARENESS_STATE_INVALID);
    assert_eq!(error.request_id, Some(REQUEST_ID));
    assert_eq!(runtime.desired_awareness(), None);

    runtime
        .set_desired_awareness_for_test(
            REQUEST_ID,
            r#"{"name":"kept"}"#,
            context(&mut engine, TransportState::Disconnected, &limits),
        )
        .unwrap();
    assert_eq!(runtime.desired_awareness(), Some(&json!({"name": "kept"})));
    // Disconnected transports retain without broadcasting.
    assert_eq!(runtime.outbox().pending_protocol_reply_count(), 0);
}

#[test]
fn tick_expires_tracked_peers_and_reports_the_next_deadline() {
    let mut runtime = runtime();
    let mut engine = engine();
    let limits = CollaborationLimits::default();

    // Install two peers through the runtime path so activity is tracked.
    runtime
        .tick(
            REQUEST_ID,
            0,
            context(&mut engine, TransportState::Disconnected, &limits),
        )
        .unwrap();
    let update = |client: u64, clock: u32| {
        use yrs::updates::encoder::Encode as _;
        let mut clients = std::collections::HashMap::new();
        clients.insert(
            yrs::ClientID::new(client),
            yrs::sync::awareness::AwarenessUpdateEntry {
                clock,
                json: r#"{"u":1}"#.into(),
            },
        );
        yrs::sync::awareness::AwarenessUpdate { clients }.encode_v1()
    };
    runtime
        .apply_awareness_frame(&mut engine, &limits, &update(21, 1))
        .unwrap();
    let outcome = runtime
        .tick(
            REQUEST_ID,
            10_000,
            context(&mut engine, TransportState::Disconnected, &limits),
        )
        .unwrap();
    assert_eq!(outcome.expired_peers, Vec::<u64>::new());
    runtime
        .apply_awareness_frame(&mut engine, &limits, &update(22, 1))
        .unwrap();

    // Exactly at the boundary peer 21 expires; peer 22 (seen at 10s)
    // stays and owns the next deadline.
    let outcome = runtime
        .tick(
            REQUEST_ID,
            AWARENESS_EXPIRY_MILLIS,
            context(&mut engine, TransportState::Disconnected, &limits),
        )
        .unwrap();
    assert_eq!(outcome.expired_peers, vec![21]);
    assert!(!outcome.renewed_local);
    assert_eq!(
        outcome.next_deadline_millis,
        Some(10_000 + AWARENESS_EXPIRY_MILLIS),
    );
    assert_eq!(runtime.peers(&mut engine).len(), 1);
}

#[test]
fn task8_third_remediation_runtime_renewal_marks_local_peer_changed() {
    let mut runtime = runtime();
    let mut engine = engine();
    let limits = CollaborationLimits::default();

    runtime
        .set_desired_awareness_for_test(
            REQUEST_ID,
            r#"{"name":"renewed"}"#,
            context(&mut engine, TransportState::Synchronized, &limits),
        )
        .unwrap();
    let before = runtime.peers(&mut engine);

    let outcome = runtime
        .tick(
            REQUEST_ID,
            AWARENESS_RENEWAL_INTERVAL_MILLIS,
            context(&mut engine, TransportState::Synchronized, &limits),
        )
        .unwrap();
    let after = runtime.peers(&mut engine);

    assert!(outcome.renewed_local, "{outcome:?}");
    assert!(outcome.outbound_changed, "{outcome:?}");
    assert!(outcome.expired_peers.is_empty(), "{outcome:?}");
    assert!(outcome.peers_changed, "{outcome:?}");
    assert_eq!(before.len(), 1);
    assert_eq!(after.len(), 1);
    assert!(after[0].clock > before[0].clock, "{before:?} -> {after:?}");
}

fn assert_clock_exhausted(error: &SessionError) {
    assert_eq!(error.domain, ErrorDomain::Transport, "{error:?}");
    assert_eq!(error.code, "AWARENESS_CLOCK_EXHAUSTED", "{error:?}");
    assert_eq!(error.request_id, Some(REQUEST_ID), "{error:?}");
    assert!(
        error.message.contains("fresh editor identity is required"),
        "{error:?}",
    );
    assert_eq!(
        error.details.as_ref().unwrap()["requiresFreshEditorIdentity"],
        true,
        "{error:?}",
    );
    assert_eq!(
        error.details.as_ref().unwrap()["retryable"],
        false,
        "{error:?}",
    );
}

fn runtime_with_clock(clock: u32) -> (CollaborationRuntime, YrsDocumentEngine) {
    let mut runtime = runtime();
    let mut engine = engine();
    let limits = CollaborationLimits::default();
    runtime
        .set_desired_awareness_for_test(
            REQUEST_ID,
            r#"{"name":"before"}"#,
            context(&mut engine, TransportState::Disconnected, &limits),
        )
        .unwrap();
    engine.awareness().set_live_local_clock_for_test(clock);
    (runtime, engine)
}

#[test]
fn set_and_renew_report_clock_exhaustion_without_mutating_state_or_outbox() {
    let limits = CollaborationLimits::default();
    for clock in [u32::MAX - 1, u32::MAX] {
        let (mut runtime, mut engine) = runtime_with_clock(clock);
        let before_peers = runtime.peers(&mut engine);
        let before_replies = runtime.outbox().pending_protocol_reply_count();

        let error = runtime
            .set_desired_awareness_for_test(
                REQUEST_ID,
                r#"{"name":"after"}"#,
                context(&mut engine, TransportState::Synchronized, &limits),
            )
            .unwrap_err();
        assert_clock_exhausted(&error);
        assert_eq!(runtime.peers(&mut engine), before_peers);
        assert_eq!(
            runtime.outbox().pending_protocol_reply_count(),
            before_replies,
        );

        runtime.awareness.last_local_publish_millis = Some(0);
        let error = runtime
            .tick(
                REQUEST_ID,
                AWARENESS_RENEWAL_INTERVAL_MILLIS,
                context(&mut engine, TransportState::Synchronized, &limits),
            )
            .unwrap_err();
        assert_clock_exhausted(&error);
        assert_eq!(runtime.peers(&mut engine), before_peers);
        assert_eq!(
            runtime.outbox().pending_protocol_reply_count(),
            before_replies,
        );
    }
}

#[test]
fn reconnect_publication_reports_clock_exhaustion_without_enqueuing() {
    let limits = CollaborationLimits::default();
    for clock in [u32::MAX - 1, u32::MAX] {
        let (mut runtime, mut engine) = runtime_with_clock(clock);
        let before_peers = runtime.peers(&mut engine);

        let error = runtime
            .prepare_handshake_republish(&mut engine, &limits)
            .unwrap_err();

        assert_eq!(error.code, "AWARENESS_CLOCK_EXHAUSTED", "{error:?}");
        assert_eq!(runtime.peers(&mut engine), before_peers);
        assert_eq!(runtime.outbox().pending_protocol_reply_count(), 0);
    }
}

mod table_cell_presence {
    use super::*;
    use crate::boundary::ResourceLimits;
    use crate::tables::commands::TableCommand;
    use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};
    use crate::yrs_engine::{
        CellSelectionPoint, EditingLimits, HistoryPolicy, InitializationMode, SelectionInput,
        SelectionIntent, TransactionOrigin, TypedCommand, TypedTransaction, YrsEngineConfig,
        DEFAULT_POSITION_AFFINITY,
    };

    const PUBLISHER_REQUEST_ID: u64 = 91;
    const FIRST_TABLE: usize = 0;
    const SECOND_TABLE: usize = 1;
    const TOP_LEFT: usize = 0;
    const TOP_RIGHT: usize = 1;
    const BOTTOM_RIGHT: usize = 3;

    fn tabled_engine(initialization_mode: InitializationMode) -> YrsDocumentEngine {
        YrsDocumentEngine::new(YrsEngineConfig {
            schema: tabled_schema(PROSEMIRROR_TABLE_NAMES),
            fragment_name: "prosemirror".into(),
            initialization_mode,
            resource_limits: ResourceLimits::default(),
            editing_limits: EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .expect("the tabled engine initializes")
    }

    fn cell(text: &str) -> Value {
        json!({
            "type": "table_cell",
            "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null },
            "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }],
        })
    }

    fn two_by_two(prefix: &str) -> Value {
        json!({
            "type": "table",
            "content": [
                { "type": "table_row", "content": [cell(&format!("{prefix}0")), cell(&format!("{prefix}1"))] },
                { "type": "table_row", "content": [cell(&format!("{prefix}2")), cell(&format!("{prefix}3"))] },
            ],
        })
    }

    fn table_openings(engine: &YrsDocumentEngine) -> Vec<Vec<u32>> {
        let index = engine
            .table_projection_index()
            .expect("a ready engine carries a table projection");
        index
            .positions()
            .map(|position| {
                index
                    .table_at(position)
                    .expect("every listed position projects a table")
                    .cells
                    .iter()
                    .map(|cell| cell.source_pos)
                    .collect()
            })
            .collect()
    }

    struct Peers {
        publisher: YrsDocumentEngine,
        publisher_runtime: CollaborationRuntime,
        observer: YrsDocumentEngine,
        observer_runtime: CollaborationRuntime,
        limits: CollaborationLimits,
    }

    fn two_table_peers() -> Peers {
        let mut publisher = tabled_engine(InitializationMode::LocalEmpty);
        publisher
            .import_json(
                &json!({ "type": "doc", "content": [two_by_two("a"), two_by_two("b")] })
                    .to_string(),
                TransactionOrigin::DocumentImport,
            )
            .expect("the two-table fixture imports");
        let mut observer = tabled_engine(InitializationMode::AwaitRemote);
        observer
            .apply_remote_update_v1(REQUEST_ID, &publisher.encoded_state().unwrap())
            .expect("the observer admits the publisher's document");
        assert_eq!(observer.document_json(), publisher.document_json());
        Peers {
            publisher,
            publisher_runtime: runtime(),
            observer,
            observer_runtime: runtime(),
            limits: CollaborationLimits::default(),
        }
    }

    fn intent(selection: Value) -> String {
        json!({ "state": { "user": "publisher" }, "focused": true, "selection": selection })
            .to_string()
    }

    fn cell_selection(anchor_cell: u32, head_cell: u32) -> Value {
        json!({ "type": "cell", "anchorCell": anchor_cell, "headCell": head_cell })
    }

    impl Peers {
        fn try_publish(&mut self, selection: Value) -> Result<Value, SessionError> {
            self.publisher_runtime.set_awareness_intent(
                PUBLISHER_REQUEST_ID,
                &intent(selection),
                context(
                    &mut self.publisher,
                    TransportState::Disconnected,
                    &self.limits,
                ),
            )?;
            Ok(self
                .publisher_runtime
                .desired_awareness()
                .cloned()
                .expect("the published state is retained"))
        }

        fn publish_cells(&mut self, anchor_cell: u32, head_cell: u32) -> Value {
            self.try_publish(cell_selection(anchor_cell, head_cell))
                .expect("the publisher admits its own real-cell rectangle")
        }

        fn deliver_presence(&mut self) {
            let update = self
                .publisher
                .awareness()
                .encode_local_update_v1()
                .expect("the publisher encodes its presence");
            self.observer_runtime
                .apply_awareness_frame(&mut self.observer, &self.limits, &update)
                .expect("the observer admits the publisher's presence");
        }

        fn observed_publisher(&mut self) -> AwarenessPeerProjection {
            let peers = self.observer_runtime.peers(&mut self.observer);
            peers
                .iter()
                .find(|peer| !peer.is_local)
                .cloned()
                .unwrap_or_else(|| panic!("no remote peer among {peers:?}"))
        }
    }

    #[test]
    fn a_remote_rectangle_resolves_to_the_same_real_cells() {
        let mut peers = two_table_peers();
        let cells = table_openings(&peers.publisher);
        let (anchor, head) = (
            cells[FIRST_TABLE][TOP_LEFT],
            cells[FIRST_TABLE][BOTTOM_RIGHT],
        );

        peers.publish_cells(anchor, head);
        peers.deliver_presence();

        let observed = peers.observed_publisher();
        assert_eq!(
            observed.cell_rectangle,
            Some(AwarenessCellRectangleProjection {
                anchor_cell: anchor,
                head_cell: head,
            }),
            "observed peer: {observed:?}",
        );
        assert_eq!(
            observed.cursor,
            Some(AwarenessCursorProjection { anchor, head }),
            "the ordinary cursor fallback addresses the same cell openings",
        );
    }

    #[test]
    fn a_remote_rectangle_whose_anchor_cell_was_deleted_keeps_only_the_cursor() {
        let mut peers = two_table_peers();
        let cells = table_openings(&peers.publisher);
        let (anchor, head) = (
            cells[FIRST_TABLE][TOP_LEFT],
            cells[FIRST_TABLE][BOTTOM_RIGHT],
        );
        peers.publish_cells(anchor, head);
        peers.deliver_presence();

        peers
            .observer
            .apply_typed_transaction(TypedTransaction {
                request_id: REQUEST_ID,
                base_document_revision: peers.observer.revision(),
                origin: TransactionOrigin::LocalApi,
                operations: Vec::new(),
                selection_intent: SelectionIntent::Set(SelectionInput::Cell {
                    anchor: CellSelectionPoint::Document {
                        opening: anchor,
                        affinity: DEFAULT_POSITION_AFFINITY,
                    },
                    head: CellSelectionPoint::Document {
                        opening: anchor,
                        affinity: DEFAULT_POSITION_AFFINITY,
                    },
                }),
                history_policy: HistoryPolicy::Skip,
            })
            .expect("the observer selects the anchor column");
        peers
            .observer
            .apply_command(
                REQUEST_ID,
                TypedCommand::Table(TableCommand::DeleteTableColumns),
            )
            .expect("the column deletion plans")
            .expect("the column deletion applies");

        let remaining = table_openings(&peers.observer);
        assert_eq!(
            remaining[FIRST_TABLE].len(),
            2,
            "the first table keeps one column: {remaining:?}",
        );
        let observed = peers.observed_publisher();
        assert_eq!(
            observed.cell_rectangle, None,
            "a deleted anchor cell must not slide onto its surviving neighbour {:?}",
            remaining[FIRST_TABLE][TOP_LEFT],
        );
        assert!(
            observed.cursor.is_some(),
            "the valid cursor fallback survives: {observed:?}",
        );
    }

    #[test]
    fn a_remote_rectangle_spanning_two_tables_keeps_only_the_cursor() {
        let mut peers = two_table_peers();
        let cells = table_openings(&peers.publisher);
        let first =
            peers.publish_cells(cells[FIRST_TABLE][TOP_RIGHT], cells[FIRST_TABLE][TOP_RIGHT]);
        let second =
            peers.publish_cells(cells[SECOND_TABLE][TOP_LEFT], cells[SECOND_TABLE][TOP_LEFT]);
        let mut spanning = first.clone();
        spanning[AWARENESS_CELL_RECTANGLE_KEY]["head"] =
            second[AWARENESS_CELL_RECTANGLE_KEY]["head"].clone();

        assert_eq!(
            peers.observer.resolve_awareness_cell_rectangle(&first),
            Some((cells[FIRST_TABLE][TOP_RIGHT], cells[FIRST_TABLE][TOP_RIGHT])),
        );
        assert_eq!(
            peers.observer.resolve_awareness_cell_rectangle(&second),
            Some((cells[SECOND_TABLE][TOP_LEFT], cells[SECOND_TABLE][TOP_LEFT])),
        );
        assert_eq!(
            peers.observer.resolve_awareness_cell_rectangle(&spanning),
            None,
            "real cells in different tables never form a rectangle: {spanning}",
        );
        assert!(peer_cursor_projection(&peers.observer, &spanning).is_some());
    }

    #[test]
    fn the_rectangle_extension_counts_toward_the_peer_byte_ceiling() {
        let mut measured = two_table_peers();
        let cells = table_openings(&measured.publisher);
        let (anchor, head) = (
            cells[FIRST_TABLE][TOP_LEFT],
            cells[FIRST_TABLE][BOTTOM_RIGHT],
        );
        let text_state = measured
            .try_publish(json!({ "type": "text", "anchor": anchor, "head": head }))
            .expect("the text cursor publishes under default limits");
        let cell_state = measured.publish_cells(anchor, head);
        let text_bytes = text_state.to_string().len();
        let cell_bytes = cell_state.to_string().len();
        assert!(
            cell_bytes > text_bytes && cell_bytes - 1 >= intent(cell_selection(anchor, head)).len(),
            "the extension must be what crosses the ceiling: text {text_bytes}, cell {cell_bytes}",
        );

        let mut bounded = two_table_peers();
        bounded.limits.max_awareness_peer_bytes = cell_bytes - 1;
        let accepted = bounded
            .try_publish(json!({ "type": "text", "anchor": anchor, "head": head }))
            .expect("the text cursor fits under the ceiling");
        let error = bounded
            .try_publish(cell_selection(anchor, head))
            .expect_err("the rectangle extension pushes the state over the ceiling");

        assert_eq!(error.code, "INPUT_LIMIT_EXCEEDED", "{error:?}");
        assert_eq!(
            error.details,
            Some(json!({ "field": "maxAwarenessPeerBytes" })),
            "{error:?}",
        );
        assert_eq!(
            bounded.publisher_runtime.desired_awareness(),
            Some(&accepted),
            "a refused rectangle leaves the previous presence in place",
        );
    }
}
