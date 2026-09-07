//! Runtime awareness ownership: desired local state, peer projections, and
//! deterministic renewal/expiry clocks.
//!
//! The runtime owns the *lifecycle* of awareness — the desired local state
//! JSON (validated and size-bounded at set time), per-peer activity
//! deadlines, and the public peer projections — while ALL wire and clock
//! state stays sealed inside the engine-owned
//! [`crate::yrs_engine::AwarenessCodec`]. This module never constructs a
//! `yrs::sync::Awareness`, never duplicates the codec's clocks, and never
//! reads a wall clock: time enters exclusively as the `now_millis` parameter
//! of [`CollaborationRuntime::tick`], which the native host reaches through
//! Rust's returned collaboration-drive deadline.

#![allow(
    clippy::result_large_err,
    reason = "SessionError is the established unboxed session error envelope"
)]

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::ffi_v2::types::{decimal_u64, AWARENESS_CLOCK_EXHAUSTED};
use crate::session::{CollaborationLimits, ErrorDomain, SessionError, TransportState};
use crate::yrs_engine::{AwarenessLimits, YrsDocumentEngine, YrsEngineError};

use super::outbox::OutboxReservationError;
use super::protocol::{
    frame_awareness_message, TRANSPORT_REPLY_LIMIT_EXCEEDED, TRANSPORT_RESOURCE_EXHAUSTED,
};
use super::CollaborationRuntime;

/// Local desired awareness is re-published with a fresh clock every renewal
/// interval while synchronized (design owners' value).
pub const AWARENESS_RENEWAL_INTERVAL_MILLIS: u64 = 15_000;
/// Remote peers with no observed activity for this long are expired
/// (standard y-protocols removal semantics; design owners' value).
pub const AWARENESS_EXPIRY_MILLIS: u64 = 30_000;

/// Refusal code for a desired awareness state that is not valid JSON.
pub const AWARENESS_STATE_INVALID: &str = "AWARENESS_STATE_INVALID";
/// Refusal code for a tick whose deterministic time regresses.
pub const AWARENESS_TIME_REGRESSION: &str = "AWARENESS_TIME_REGRESSION";

/// Wire action reported by awareness-shaped refusals.
const AWARENESS_ACTION: &str = "awareness";

/// The only caller-controlled shape accepted by the production awareness
/// ABI. Its published value is assembled by Rust, so `cursor` can only ever
/// be the sticky engine representation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalAwarenessIntent {
    state: Value,
    focused: bool,
    #[serde(default)]
    selection: LocalAwarenessSelectionWire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum LocalAwarenessSelection {
    Text { anchor: u32, head: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AwarenessSelectionOutcome {
    pub(crate) outbound_changed: bool,
}

/// How one published intent treats the local cursor. Rust owns the sticky
/// index, so a caller that is only changing focus or application state must
/// be able to say "leave it alone" without restating a document position it
/// may no longer be able to resolve.
///
/// - key omitted -> [`Self::Retain`]: keep the sticky cursor already
///   published, whatever the document has done since;
/// - `"selection": null` -> [`Self::Clear`]: publish no cursor at all;
/// - a text selection -> [`Self::Present`]: materialize these positions
///   against the current document, refusing if they do not resolve.
#[derive(Debug, Default)]
enum LocalAwarenessSelectionWire {
    #[default]
    Retain,
    Clear,
    Present(LocalAwarenessSelection),
}

impl<'de> Deserialize<'de> for LocalAwarenessSelectionWire {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match Option::<LocalAwarenessSelection>::deserialize(deserializer)? {
            None => Ok(Self::Clear),
            Some(selection) => Ok(Self::Present(selection)),
        }
    }
}

fn awareness_state_invalid(request_id: u64, message: impl Into<String>) -> SessionError {
    let mut error = SessionError::new(ErrorDomain::Boundary, AWARENESS_STATE_INVALID, message);
    error.request_id = Some(request_id);
    error
}

fn awareness_peer_bytes_limit_error(request_id: u64, limit: usize, actual: usize) -> SessionError {
    engine_error_with_request(
        YrsEngineError::limit("INPUT_LIMIT_EXCEEDED", limit, actual)
            .with_details(serde_json::json!({ "field": "maxAwarenessPeerBytes" })),
        request_id,
    )
}

fn contains_reserved_cursor(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_reserved_cursor),
        Value::Object(entries) => entries
            .iter()
            .any(|(key, value)| key == "cursor" || contains_reserved_cursor(value)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

/// Runtime-owned awareness bookkeeping: exactly the three things the design
/// assigns to this layer — desired-state JSON, deadlines, and the inputs of
/// the public projections. Clocks, tombstones, and wire state live in the
/// engine-owned codec.
pub(crate) struct AwarenessRuntimeState {
    /// The validated desired local presence; survives disconnect, generation
    /// close, `Incompatible`, and detach/reattach by construction.
    desired_state: Option<Value>,
    /// The most recent `tick(now_millis)` observation; receives stamp peer
    /// activity with this value, so no other clock source exists.
    now_millis: u64,
    /// When the desired state was last handed to the outbox for broadcast.
    last_local_publish_millis: Option<u64>,
    /// One already-clocked local tombstone waiting for a retryable outbox
    /// reservation. Its framed bytes remain immutable across retries.
    pending_withdrawal: Option<PendingWithdrawal>,
    /// Remote client -> last observed activity (`now_millis` of the tick
    /// preceding the touching update). Sorted so expiry order is stable.
    peer_activity: BTreeMap<u64, u64>,
}

struct PendingWithdrawal {
    frame: Vec<u8>,
    retry_not_before_millis: Option<u64>,
}

impl PendingWithdrawal {
    fn new(frame: Vec<u8>, now_millis: u64) -> Self {
        Self {
            frame,
            retry_not_before_millis: now_millis.checked_add(AWARENESS_RENEWAL_INTERVAL_MILLIS),
        }
    }

    fn defer_retry(&mut self, now_millis: u64) {
        self.retry_not_before_millis = now_millis.checked_add(AWARENESS_RENEWAL_INTERVAL_MILLIS);
    }

    fn is_due(&self, now_millis: u64) -> bool {
        self.retry_not_before_millis
            .is_some_and(|deadline| now_millis >= deadline)
    }
}

impl AwarenessRuntimeState {
    pub(crate) fn new() -> Self {
        Self {
            desired_state: None,
            now_millis: 0,
            last_local_publish_millis: None,
            pending_withdrawal: None,
            peer_activity: BTreeMap::new(),
        }
    }

    pub(crate) fn reset_for_restore(&mut self) {
        self.peer_activity.clear();
        self.last_local_publish_millis = None;
    }

    fn next_deadline_millis(&self, transport_state: TransportState) -> Option<u64> {
        let local_publish =
            if transport_state == TransportState::Synchronized && self.desired_state.is_some() {
                self.last_local_publish_millis.map_or_else(
                    || Some(self.now_millis),
                    |published| published.checked_add(AWARENESS_RENEWAL_INTERVAL_MILLIS),
                )
            } else if transport_state == TransportState::Synchronized {
                self.pending_withdrawal
                    .as_ref()
                    .and_then(|pending| pending.retry_not_before_millis)
            } else {
                None
            };
        let expiry = self
            .peer_activity
            .values()
            .filter_map(|seen| seen.checked_add(AWARENESS_EXPIRY_MILLIS))
            .min();
        match (local_publish, expiry) {
            (Some(local_publish), Some(expiry)) => Some(local_publish.min(expiry)),
            (deadline @ Some(_), None) | (None, deadline @ Some(_)) => deadline,
            (None, None) => None,
        }
    }
}

pub(crate) struct AwarenessContext<'a> {
    pub(crate) engine: &'a mut YrsDocumentEngine,
    pub(crate) transport_state: TransportState,
    pub(crate) limits: &'a CollaborationLimits,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AwarenessPeerProjection {
    pub(crate) client_id: u64,
    pub(crate) clock: u32,
    pub(crate) is_local: bool,
    pub(crate) state: Value,
    pub(crate) cursor: Option<AwarenessCursorProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AwarenessCursorProjection {
    pub(crate) anchor: u32,
    pub(crate) head: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TickOutcome {
    /// The desired local state was re-published with a fresh clock.
    pub(crate) renewed_local: bool,
    /// Whether this tick queued an outbound awareness update.
    pub(crate) outbound_changed: bool,
    /// Remote clients expired by this tick, in ascending client order.
    pub(crate) expired_peers: Vec<u64>,
    /// Whether this tick changed a local or remote peer projection.
    pub(crate) peers_changed: bool,
    /// The next renewal/expiry deadline for the native host to schedule.
    pub(crate) next_deadline_millis: Option<u64>,
}

pub(crate) fn awareness_limits(limits: &CollaborationLimits) -> AwarenessLimits {
    AwarenessLimits {
        max_awareness_peers: limits.max_awareness_peers,
        max_awareness_peer_bytes: limits.max_awareness_peer_bytes,
        max_awareness_bytes: limits.max_awareness_bytes,
    }
}

/// Resolve the sticky cursor carried by a peer state (`state.cursor.anchor`
/// / `state.cursor.head`, the sticky-position wire form the legacy
/// collaboration surface established). Any missing, malformed, or
/// unresolvable point degrades the whole cursor to `None`.
fn peer_cursor_projection(
    engine: &YrsDocumentEngine,
    state: &Value,
) -> Option<AwarenessCursorProjection> {
    let cursor = state.as_object()?.get("cursor")?.as_object()?;
    let anchor = engine.resolve_awareness_sticky_doc_pos(cursor.get("anchor")?)?;
    let head = engine.resolve_awareness_sticky_doc_pos(cursor.get("head")?)?;
    Some(AwarenessCursorProjection { anchor, head })
}

fn engine_error_with_request(error: YrsEngineError, request_id: u64) -> SessionError {
    let mut error = if error.code == AWARENESS_CLOCK_EXHAUSTED {
        SessionError::transport(error)
    } else {
        SessionError::from(error)
    };
    error.request_id = Some(request_id);
    error
}

fn time_regression_error(request_id: u64, now_millis: u64, last_now_millis: u64) -> SessionError {
    let mut error = SessionError::new(
        ErrorDomain::Transport,
        AWARENESS_TIME_REGRESSION,
        "awareness tick nowMillis must not decrease",
    );
    error.request_id = Some(request_id);
    error.details = Some(serde_json::json!({
        "nowMillis": decimal_u64(now_millis),
        "lastNowMillis": decimal_u64(last_now_millis),
    }));
    error
}

/// Outbox refusals for awareness broadcasts never close a generation — the
/// desired state is already retained and the deterministic renewal clock
/// re-attempts the broadcast — so they surface as plain retryable transport
/// errors, split per the saturation ruling.
fn broadcast_reservation_error(error: OutboxReservationError, request_id: u64) -> SessionError {
    let mut session_error = match error {
        OutboxReservationError::Saturated {
            field,
            limit,
            actual,
        } => {
            let mut session_error = SessionError::new(
                ErrorDomain::Transport,
                TRANSPORT_REPLY_LIMIT_EXCEEDED,
                format!("{field} exceeded while enqueueing an awareness broadcast"),
            );
            session_error.limit = Some(limit as u64);
            session_error.actual = Some(actual as u64);
            session_error.details = Some(serde_json::json!({
                "action": AWARENESS_ACTION,
                "field": field,
                "limit": limit,
                "actual": actual,
            }));
            session_error
        }
        OutboxReservationError::Allocation => SessionError::new(
            ErrorDomain::Transport,
            TRANSPORT_RESOURCE_EXHAUSTED,
            "awareness broadcast capacity could not be reserved",
        ),
    };
    session_error.request_id = Some(request_id);
    session_error
}

impl CollaborationRuntime {
    /// Validates the monotonic clock before a stateful transport callback
    /// mutates its generation, outbox, or awareness projection. Stale
    /// generation admission runs before this check at the session boundary.
    pub(crate) fn check_now_millis(
        &self,
        request_id: u64,
        now_millis: u64,
    ) -> Result<(), SessionError> {
        if now_millis < self.awareness.now_millis {
            return Err(time_regression_error(
                request_id,
                now_millis,
                self.awareness.now_millis,
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_desired_awareness_for_test(
        &mut self,
        request_id: u64,
        state_json: &str,
        context: AwarenessContext<'_>,
    ) -> Result<(), SessionError> {
        let AwarenessContext {
            engine,
            transport_state,
            limits,
        } = context;
        let value: Value = serde_json::from_str(state_json).map_err(|error| {
            awareness_state_invalid(
                request_id,
                format!("desired awareness state is not valid JSON: {error}"),
            )
        })?;
        self.set_desired_awareness_value(request_id, value, engine, transport_state, limits)
    }

    pub(crate) fn set_awareness_intent(
        &mut self,
        request_id: u64,
        intent_json: &str,
        context: AwarenessContext<'_>,
    ) -> Result<(), SessionError> {
        let AwarenessContext {
            engine,
            transport_state,
            limits,
        } = context;
        if intent_json.len() > limits.max_awareness_peer_bytes {
            return Err(awareness_peer_bytes_limit_error(
                request_id,
                limits.max_awareness_peer_bytes,
                intent_json.len(),
            ));
        }
        let intent: LocalAwarenessIntent = serde_json::from_str(intent_json).map_err(|error| {
            awareness_state_invalid(
                request_id,
                format!("local awareness intent is invalid: {error}"),
            )
        })?;
        // Published at the awareness root, per the y-protocols convention.
        let Value::Object(mut published) = intent.state else {
            return Err(awareness_state_invalid(
                request_id,
                "local awareness intent state must be an object",
            ));
        };
        if published
            .iter()
            .any(|(key, value)| key == "cursor" || contains_reserved_cursor(value))
        {
            return Err(awareness_state_invalid(
                request_id,
                "local awareness intent must not contain the reserved cursor key",
            ));
        }
        if published.contains_key("focused") {
            return Err(awareness_state_invalid(
                request_id,
                "local awareness intent must not contain the reserved focused key",
            ));
        }
        let cursor = match intent.selection {
            LocalAwarenessSelectionWire::Present(LocalAwarenessSelection::Text {
                anchor,
                head,
            }) => engine
                .awareness_sticky_cursor(anchor, head)
                .ok_or_else(|| {
                    awareness_state_invalid(
                        request_id,
                        "local awareness selection is outside the current document",
                    )
                })?,
            LocalAwarenessSelectionWire::Clear => Value::Null,
            // Sticky indices survive every document mutation, so the cursor
            // already published stays correct without being restated.
            LocalAwarenessSelectionWire::Retain => self
                .awareness
                .desired_state
                .as_ref()
                .and_then(|state| state.get("cursor"))
                .cloned()
                .unwrap_or(Value::Null),
        };
        published.insert("focused".into(), Value::Bool(intent.focused));
        if !cursor.is_null() {
            published.insert("cursor".into(), cursor);
        }
        self.set_desired_awareness_value(
            request_id,
            Value::Object(published),
            engine,
            transport_state,
            limits,
        )
    }

    pub(crate) fn set_awareness_selection(
        &mut self,
        request_id: u64,
        selection_json: &str,
        context: AwarenessContext<'_>,
    ) -> Result<AwarenessSelectionOutcome, SessionError> {
        let selection: LocalAwarenessSelection =
            serde_json::from_str(selection_json).map_err(|error| {
                awareness_state_invalid(
                    request_id,
                    format!("local awareness selection is invalid: {error}"),
                )
            })?;
        let AwarenessContext {
            engine,
            transport_state,
            limits,
        } = context;
        let Some(Value::Object(current)) = self.awareness.desired_state.as_ref() else {
            return Ok(AwarenessSelectionOutcome {
                outbound_changed: false,
            });
        };
        let LocalAwarenessSelection::Text { anchor, head } = selection;
        let cursor = engine
            .awareness_sticky_cursor(anchor, head)
            .ok_or_else(|| {
                awareness_state_invalid(
                    request_id,
                    "local awareness selection is outside the current document",
                )
            })?;
        if current.get("cursor") == Some(&cursor) {
            return Ok(AwarenessSelectionOutcome {
                outbound_changed: false,
            });
        }
        let mut next = current.clone();
        next.insert("cursor".into(), cursor);
        self.set_desired_awareness_value(
            request_id,
            Value::Object(next),
            engine,
            transport_state,
            limits,
        )?;
        Ok(AwarenessSelectionOutcome {
            outbound_changed: transport_state == TransportState::Synchronized,
        })
    }

    fn set_desired_awareness_value(
        &mut self,
        request_id: u64,
        value: Value,
        engine: &mut YrsDocumentEngine,
        transport_state: TransportState,
        limits: &CollaborationLimits,
    ) -> Result<(), SessionError> {
        engine
            .awareness()
            .set_local_state(&value, &awareness_limits(limits))
            .map_err(|error| engine_error_with_request(error, request_id))?;
        self.awareness.desired_state = Some(value);
        self.awareness.pending_withdrawal = None;
        if transport_state == TransportState::Synchronized {
            self.broadcast_local_update(request_id, engine)?;
        }
        Ok(())
    }

    pub(crate) fn clear_desired_awareness(
        &mut self,
        request_id: u64,
        context: AwarenessContext<'_>,
    ) -> Result<(), SessionError> {
        let AwarenessContext {
            engine,
            transport_state,
            ..
        } = context;
        if self.awareness.desired_state.is_none() {
            return Ok(());
        }
        engine
            .awareness()
            .clear_local_state()
            .map_err(|error| engine_error_with_request(error, request_id))?;
        self.awareness.desired_state = None;
        self.awareness.last_local_publish_millis = None;
        let message = frame_awareness_message(
            &engine
                .awareness()
                .encode_local_update_v1()
                .map_err(|error| engine_error_with_request(error, request_id))?,
        );
        if transport_state == TransportState::Synchronized {
            if let Err(error) = self.enqueue_awareness_broadcast(request_id, message.clone()) {
                self.awareness.pending_withdrawal =
                    Some(PendingWithdrawal::new(message, self.awareness.now_millis));
                return Err(error);
            }
        } else {
            self.awareness.pending_withdrawal =
                Some(PendingWithdrawal::new(message, self.awareness.now_millis));
        }
        Ok(())
    }

    /// The retained desired local awareness state, if any.
    pub(crate) fn desired_awareness(&self) -> Option<&Value> {
        self.awareness.desired_state.as_ref()
    }

    pub(crate) fn peers(&self, engine: &mut YrsDocumentEngine) -> Vec<AwarenessPeerProjection> {
        let snapshot = engine.awareness().peer_snapshot();
        snapshot
            .into_iter()
            .map(|peer| {
                let cursor = peer_cursor_projection(engine, &peer.state);
                AwarenessPeerProjection {
                    client_id: peer.client_id,
                    clock: peer.clock,
                    is_local: peer.is_local,
                    state: peer.state,
                    cursor,
                }
            })
            .collect()
    }

    pub(crate) fn clear_transport_peers(&mut self, engine: &mut YrsDocumentEngine) -> bool {
        let peers_changed = !engine.awareness().peer_snapshot().is_empty();
        if engine.awareness().clear_transport_states().is_ok() {
            self.awareness.peer_activity.clear();
        }
        peers_changed
    }

    pub(crate) fn apply_awareness_frame(
        &mut self,
        engine: &mut YrsDocumentEngine,
        limits: &CollaborationLimits,
        payload: &[u8],
    ) -> Result<(), YrsEngineError> {
        let applied = engine
            .awareness()
            .apply_remote_update_v1(payload, &awareness_limits(limits))?;
        let local_client = engine.awareness().client_id();
        let now_millis = self.awareness.now_millis;
        for client in applied.touched_clients {
            if client != local_client {
                self.awareness.peer_activity.insert(client, now_millis);
            }
        }
        for client in applied.removed_clients {
            self.awareness.peer_activity.remove(&client);
        }
        Ok(())
    }

    pub(crate) fn prepare_handshake_republish(
        &mut self,
        engine: &mut YrsDocumentEngine,
        limits: &CollaborationLimits,
    ) -> Result<Option<Vec<u8>>, YrsEngineError> {
        let Some(desired) = self.awareness.desired_state.clone() else {
            return Ok(None);
        };
        engine
            .awareness()
            .set_local_state(&desired, &awareness_limits(limits))?;
        Ok(Some(frame_awareness_message(
            &engine.awareness().encode_local_update_v1()?,
        )))
    }

    pub(crate) fn mark_local_awareness_published(&mut self) {
        self.awareness.last_local_publish_millis = Some(self.awareness.now_millis);
    }

    pub(crate) fn tick(
        &mut self,
        request_id: u64,
        now_millis: u64,
        context: AwarenessContext<'_>,
    ) -> Result<TickOutcome, SessionError> {
        let AwarenessContext {
            engine,
            transport_state,
            limits,
        } = context;
        self.check_now_millis(request_id, now_millis)?;
        self.awareness.now_millis = now_millis;

        let expired_peers: Vec<u64> = self
            .awareness
            .peer_activity
            .iter()
            .filter(|(_, seen)| now_millis.saturating_sub(**seen) >= AWARENESS_EXPIRY_MILLIS)
            .map(|(client, _)| *client)
            .collect();
        for client in &expired_peers {
            engine.awareness().remove_remote_state(*client);
            self.awareness.peer_activity.remove(client);
        }

        let mut renewed_local = false;
        let mut withdrawal_enqueued = false;
        if transport_state == TransportState::Synchronized {
            let withdrawal_due = self
                .awareness
                .pending_withdrawal
                .as_ref()
                .is_some_and(|pending| pending.is_due(now_millis));
            if withdrawal_due {
                let frame = self
                    .awareness
                    .pending_withdrawal
                    .as_ref()
                    .expect("a due withdrawal remains pending")
                    .frame
                    .clone();
                if let Err(error) = self.enqueue_awareness_broadcast(request_id, frame) {
                    self.awareness
                        .pending_withdrawal
                        .as_mut()
                        .expect("a refused withdrawal remains pending")
                        .defer_retry(now_millis);
                    return Err(error);
                }
                self.awareness.pending_withdrawal = None;
                withdrawal_enqueued = true;
            } else if let Some(desired) = self.awareness.desired_state.clone() {
                let renewal_due =
                    self.awareness
                        .last_local_publish_millis
                        .is_none_or(|published| {
                            now_millis.saturating_sub(published)
                                >= AWARENESS_RENEWAL_INTERVAL_MILLIS
                        });
                if renewal_due {
                    engine
                        .awareness()
                        .set_local_state(&desired, &awareness_limits(limits))
                        .map_err(|error| engine_error_with_request(error, request_id))?;
                    self.broadcast_local_update(request_id, engine)?;
                    renewed_local = true;
                }
            }
        }

        Ok(TickOutcome {
            renewed_local,
            outbound_changed: renewed_local || withdrawal_enqueued,
            peers_changed: renewed_local || !expired_peers.is_empty(),
            expired_peers,
            next_deadline_millis: self.awareness.next_deadline_millis(transport_state),
        })
    }

    /// Frames and enqueues the codec's current local update, then restarts
    /// the renewal clock.
    fn broadcast_local_update(
        &mut self,
        request_id: u64,
        engine: &mut YrsDocumentEngine,
    ) -> Result<(), SessionError> {
        let message = frame_awareness_message(
            &engine
                .awareness()
                .encode_local_update_v1()
                .map_err(|error| engine_error_with_request(error, request_id))?,
        );
        self.enqueue_awareness_broadcast(request_id, message)?;
        self.mark_local_awareness_published();
        Ok(())
    }

    /// Reserve-then-install one framed awareness broadcast after any document
    /// update it may reference.
    fn enqueue_awareness_broadcast(
        &mut self,
        request_id: u64,
        message: Vec<u8>,
    ) -> Result<(), SessionError> {
        let reservation = self
            .outbox
            .reserve_awareness_broadcast(message.len())
            .map_err(|error| broadcast_reservation_error(error, request_id))?;
        self.outbox
            .install_awareness_broadcast(reservation, request_id, message);
        Ok(())
    }
}

#[cfg(test)]
#[path = "awareness/tests.rs"]
mod tests;
