#![allow(
    clippy::result_large_err,
    reason = "SessionError is the established unboxed six-domain boundary envelope"
)]

use std::io::{BufRead, Write};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use crate::boundary::ResourceLimits;
use crate::collaboration_runtime::awareness::awareness_limits;
use crate::collaboration_runtime::outbox::OutboundLeasePayload;
use crate::command_planner::apply_operations;
use crate::document_api::DocumentApiFacade;
use crate::native_transaction_bridge::{
    operation_error, serialize_native_outcome, NativeTransactionBridge,
    NATIVE_BRIDGE_ENVELOPE_VERSION,
};
use crate::schema::presets::{
    prosemirror_schema, prosemirror_table_schema, tiptap_schema, tiptap_table_schema,
};
use crate::schema::Schema;
use crate::serialize::json_in::{from_prosemirror_json_with_limits, UnknownTypeMode};
use crate::serialize::to_prosemirror_json;
use crate::session::{
    outbound_lease_session_error, CollaborationLimits, EditorInitialization, EditorSession,
    EditorSessionConfig, ErrorDomain, InitialContent, SessionError,
};
use crate::tables::normalize::{
    normalize_outer_table, planned_normalization_passes, reset_planned_normalization_passes,
};
use crate::tables::projection::{project_table, ProjectedTable, TableGridBudget};
use crate::yrs_engine::{DocumentScope, EditingLimits, ReplacementHistory, RootReplacementError};

const MAX_WIRE_LINE_BYTES: usize = 96 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static AVAILABILITY_WORK: std::cell::Cell<[usize; 6]> = const { std::cell::Cell::new([0; 6]) };
    static AVAILABILITY_FAILURE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn audit_work(stage: usize) {
    let mut work = AVAILABILITY_WORK.get();
    work[stage] += 1;
    AVAILABILITY_WORK.set(work);
}

fn observe_component<T>(
    enabled: bool,
    stage: usize,
    observe: impl FnOnce() -> Option<T>,
) -> Option<T> {
    if !enabled {
        return None;
    }
    #[cfg(test)]
    audit_work(stage);
    #[cfg(test)]
    if AVAILABILITY_FAILURE.get() == Some(stage) {
        return None;
    }
    let _ = stage;
    observe()
}

fn observe_pending(events: &[serde_json::Value]) -> Option<Vec<serde_json::Value>> {
    if events.len() > crate::availability_audit::MAX_ITEMS {
        return None;
    }
    let mut budget =
        crate::availability_audit::JsonBudget::new(crate::availability_audit::MAX_BYTES);
    for event in events {
        #[cfg(test)]
        audit_work(0);
        budget.observe(event, 0)?;
    }
    Some(events.to_vec())
}

#[cfg(test)]
mod availability_tests {
    use super::*;
    use crate::tables::normalize_tests::{cell, cell_with, row, seeded_session, table};
    use serde_json::{json, Value};

    #[test]
    fn availability_pending_limit_stops_before_traversal_and_other_observations() {
        let mut peer = RustPeer::new();
        peer.initialize(json!({"tables":true})).unwrap();
        peer.pending_events = vec![json!({"bytesBase64":""}); 65_537];
        AVAILABILITY_WORK.set([0; 6]);
        let reply = peer
            .command(json!({"kind":"command", "command":{"type":"deleteTable"}}))
            .unwrap();
        assert_eq!(reply["availability"]["observed"], false);
        assert_eq!(AVAILABILITY_WORK.get(), [0; 6]);
    }

    #[test]
    fn availability_failed_history_or_store_stops_later_observations() {
        for stage in [1, 2] {
            let mut peer = RustPeer::new();
            peer.initialize(json!({"tables":true})).unwrap();
            AVAILABILITY_WORK.set([0; 6]);
            AVAILABILITY_FAILURE.set(Some(stage));
            let reply = peer.command(json!({"kind":"command", "command":{"type":"deleteTable"}}));
            AVAILABILITY_FAILURE.set(None);
            let reply = reply.unwrap();
            assert_eq!(reply["availability"]["observed"], false);
            assert_eq!(reply["availability"]["contentUnchanged"], false);
            assert_eq!(reply["type"], "notApplicable");
            let mut expected = [0; 6];
            expected[1..=stage].fill(1);
            assert_eq!(AVAILABILITY_WORK.get(), expected);
        }
    }

    #[test]
    fn availability_refusal_reports_actual_preparation_and_history_audit() {
        let mut peer = RustPeer::new();
        peer.session = Some(seeded_session(
            json!({"type":"doc", "content":[table(vec![
                row(vec![cell("a"), cell_with(1, 2, Value::Null, "b")]),
                row(vec![cell_with(2, 3, Value::Null, "c")]), row(vec![]),
            ])]})
            .to_string(),
        ));
        peer.session
            .as_mut()
            .unwrap()
            .attach_collaboration_runtime();
        let reply = peer
            .command(json!({"kind":"command", "at":3,
            "command":{"type":"addTableRow", "side":"after"}}))
            .unwrap();
        assert_eq!(reply["availability"]["reason"], "irregular-prepared-grid");
        assert_eq!(reply["availability"]["historyUnchanged"], true);
        assert_eq!(reply["availability"]["historyMetadataUnchanged"], true);
        assert_eq!(reply["availability"]["outboxUnchanged"], true);
        assert_eq!(reply["availability"]["encodedStateUnchanged"], true);
        assert_eq!(reply["availability"]["revisionUnchanged"], true);
        let typing = peer.command(json!({"kind":"input", "text":"z"})).unwrap();
        assert_eq!(typing["availability"]["reason"], Value::Null);
        assert_eq!(typing["availability"]["encodedStateUnchanged"], false);
        assert_eq!(typing["availability"]["historyUnchanged"], false);
        assert_ne!(
            reply["availability"]["requestId"],
            typing["availability"]["requestId"]
        );
    }

    #[test]
    fn availability_valid_row_insertion_remains_an_edit() {
        let mut peer = RustPeer::new();
        peer.session = Some(seeded_session(
            json!({"type":"doc", "content":[table(vec![
                row(vec![cell("a"), cell("b")]), row(vec![cell("c"), cell("d")]),
            ])]})
            .to_string(),
        ));
        peer.session
            .as_mut()
            .unwrap()
            .attach_collaboration_runtime();
        let reply = peer
            .command(json!({"kind":"command", "at":3,
            "command":{"type":"addTableRow", "side":"after"}}))
            .unwrap();
        assert_eq!(reply["availability"]["reason"], Value::Null);
        assert_eq!(reply["availability"]["observed"], true);
        assert_eq!(reply["availability"]["encodedStateUnchanged"], false);
        assert_eq!(reply["availability"]["historyUnchanged"], false);
        assert_eq!(reply["availability"]["outboxUnchanged"], false);
    }
}
const MAX_REQUEST_ID_BYTES: usize = 128;
const COLLABORATION_FRAGMENT_NAME: &str = "prosemirror";
const AWAIT_SEED_DOCUMENT_ID: &str = "table-interop-document";
const AWAIT_SEED_LINEAGE_ID: &str = "table-interop-lineage";
const LEASE_ACTION: &str = "leaseOutbound";
const PROJECTED_TABLE_POSITION: u32 = 0;
const PROJECTED_TABLE_DOCUMENT_ROOT: &str = "doc";
const ACK_ACTION: &str = "ackOutbound";
const AWARENESS_EVENT_KIND: &str = "awareness";
const AWARENESS_ACTION: &str = "awareness";
const TABLE_NORMALIZATION_FAILED: &str = "TABLE_NORMALIZATION_FAILED";
const AUTONOMOUS_REPAIR_CANARY_FAILED: &str = "AUTONOMOUS_REPAIR_CANARY_FAILED";
const TABLE_INTEROP_OWNER_ID: u64 = 1;
const SET_SELECTION_INTENT: &str = "setSelection";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventOrigin {
    Local,
    History,
    Remote,
}

impl EventOrigin {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::History => "history",
            Self::Remote => "remote",
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    id: String,
    operation: String,
    payload: serde_json::Value,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitializePayload {
    #[serde(default)]
    schema: Option<SchemaPreset>,
    #[serde(default)]
    tables: bool,
    #[serde(default)]
    await_seed: bool,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum SchemaPreset {
    Tiptap,
    Prosemirror,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum LocalMutation {
    Input {
        text: String,
    },
    Command {
        command: serde_json::Value,
        #[serde(default)]
        at: Option<u32>,
        #[serde(default)]
        head: Option<u32>,
    },
    Selection {
        selection: serde_json::Value,
    },
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AwarenessIntentPayload {
    intent: serde_json::Value,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdatePayload {
    update_base64: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateVectorPayload {
    state_vector_base64: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectTablePayload {
    schema: serde_json::Value,
    table: serde_json::Value,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyPayload {}

struct RustPeer {
    session: Option<EditorSession>,
    pending_events: Vec<serde_json::Value>,
    emitted_events: Vec<serde_json::Value>,
    next_request_id: u64,
    autonomous_repair_writes: u64,
}

pub fn serve<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut peer = RustPeer::new();
    loop {
        let Some(line) = read_bounded_line(&mut reader)? else {
            return Ok(());
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (reply, stop) = peer.handle(line);
        writeln!(writer, "{reply}")?;
        writer.flush()?;
        if stop {
            return Ok(());
        }
    }
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let (consumed, terminated) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                break;
            }
            match memchr::memchr(b'\n', available) {
                Some(index) => {
                    admit_line_length(bytes.len() + index)?;
                    bytes.extend_from_slice(&available[..index]);
                    (index + 1, true)
                }
                None => {
                    admit_line_length(bytes.len() + available.len())?;
                    bytes.extend_from_slice(available);
                    (available.len(), false)
                }
            }
        };
        reader.consume(consumed);
        if terminated {
            return Ok(Some(String::from_utf8(bytes)?));
        }
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(bytes)?))
}

fn admit_line_length(length: usize) -> Result<(), Box<dyn std::error::Error>> {
    if length > MAX_WIRE_LINE_BYTES {
        return Err(format!(
            "wire line exceeds the {MAX_WIRE_LINE_BYTES} byte ceiling at {length} bytes"
        )
        .into());
    }
    Ok(())
}

fn peer_error(code: &str, message: impl Into<String>) -> SessionError {
    SessionError::new(ErrorDomain::Boundary, code, message)
}

fn config_invalid(message: impl Into<String>) -> SessionError {
    peer_error("CONFIG_INVALID", message)
}

fn reply_line(
    id: &str,
    outcome: Result<serde_json::Value, SessionError>,
    events: Vec<serde_json::Value>,
) -> String {
    let (value, error) = match outcome {
        Ok(value) => (value, serde_json::Value::Null),
        Err(error) => (
            serde_json::Value::Null,
            serde_json::json!({ "code": error.code, "message": error.message }),
        ),
    };
    serde_json::json!({
        "id": id,
        "value": value,
        "error": error,
        "events": events,
    })
    .to_string()
}

impl RustPeer {
    fn new() -> Self {
        Self {
            session: None,
            pending_events: Vec::new(),
            emitted_events: Vec::new(),
            next_request_id: 1,
            autonomous_repair_writes: 0,
        }
    }

    fn handle(&mut self, line: &str) -> (String, bool) {
        let request: WireRequest = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("table-interop: unparseable request line: {error}");
                let id = recover_request_identifier(line);
                return (
                    reply_line(
                        &id,
                        Err(config_invalid(format!("unparseable request: {error}"))),
                        Vec::new(),
                    ),
                    false,
                );
            }
        };
        if request.id.is_empty() || request.id.len() > MAX_REQUEST_ID_BYTES {
            return (
                reply_line(
                    "",
                    Err(config_invalid(format!(
                        "request identifiers are 1..={MAX_REQUEST_ID_BYTES} bytes"
                    ))),
                    Vec::new(),
                ),
                false,
            );
        }
        let stop = request.operation == "shutdown";
        let outcome = self.dispatch(&request.operation, request.payload);
        let events = std::mem::take(&mut self.emitted_events);
        (reply_line(&request.id, outcome, events), stop)
    }

    fn dispatch(
        &mut self,
        operation: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        match operation {
            "initialize" => self.initialize(payload),
            "command" => self.command(payload),
            "undo" => self.history(payload, true),
            "redo" => self.history(payload, false),
            "applyUpdate" => self.apply_update(payload),
            "drain" => self.drain(payload),
            "snapshot" => self.snapshot(payload),
            "stateVector" => self.state_vector(payload),
            "stateDiff" => self.state_diff(payload),
            "projectTable" => project_table_payload(payload),
            "repairTableDuringRemoteWindow" => self.repair_table_during_remote_window(payload),
            "normalizeTable" => normalize_table_payload(payload),
            "setAwareness" => self.set_awareness(payload),
            "applyAwareness" => self.apply_awareness(payload),
            "shutdown" => self.shutdown(payload),
            operation => Err(peer_error(
                "UNSUPPORTED_OPERATION",
                format!("operation {operation} is not served by the Rust peer"),
            )),
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        request_id
    }

    fn session_mut(&mut self) -> Result<&mut EditorSession, SessionError> {
        self.session.as_mut().ok_or_else(|| {
            peer_error(
                "PEER_NOT_INITIALIZED",
                "the peer has no session; send initialize first",
            )
        })
    }

    fn initialize(
        &mut self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        let payload: InitializePayload = parse_payload(payload)?;
        if self.session.is_some() {
            return Err(config_invalid("the peer session is already initialized"));
        }
        let schema = match (
            payload.schema.unwrap_or(SchemaPreset::Tiptap),
            payload.tables,
        ) {
            (SchemaPreset::Tiptap, false) => tiptap_schema(),
            (SchemaPreset::Tiptap, true) => tiptap_table_schema(),
            (SchemaPreset::Prosemirror, false) => prosemirror_schema(),
            (SchemaPreset::Prosemirror, true) => prosemirror_table_schema(),
        };
        let initialization = if payload.await_seed {
            EditorInitialization::Room {
                scope: DocumentScope {
                    document_id: AWAIT_SEED_DOCUMENT_ID.into(),
                    lineage_id: AWAIT_SEED_LINEAGE_ID.into(),
                },
                snapshot: None,
            }
        } else {
            EditorInitialization::Local {
                initial_content: InitialContent::Empty,
            }
        };
        let config = EditorSessionConfig {
            schema_json: None,
            fragment_name: COLLABORATION_FRAGMENT_NAME.into(),
            initialization,
            resource_limits: ResourceLimits::default(),
            editing_limits: EditingLimits::default(),
            collaboration_limits: CollaborationLimits::default(),
            max_length: None,
            read_only: false,
            input_filter: None,
            allow_base64_images: false,
        };
        let mut session = DocumentApiFacade::admit(config, schema)?;
        session.attach_collaboration_runtime();
        let revision = session.engine.revision();
        self.session = Some(session);
        Ok(serde_json::json!({ "documentRevision": revision.to_string() }))
    }

    fn command(&mut self, payload: serde_json::Value) -> Result<serde_json::Value, SessionError> {
        let mutation: LocalMutation = parse_payload(payload)?;
        reset_planned_normalization_passes();
        crate::tables::command_context::reset_preparation_refusal();
        let setup_revision = self.session_mut()?.engine.state_revision();
        let setup_document_revision = self.session_mut()?.engine.revision();
        let setup_events = self.pending_events.len();
        if let LocalMutation::Command {
            at: Some(at), head, ..
        } = &mutation
        {
            self.anchor_selection(*at, *head)?;
        }
        let request_id = self.next_request_id();
        let pending_count_before = self.pending_events.len();
        let pending_before = observe_pending(&self.pending_events);
        let session = self.session_mut()?;
        let base_document_revision = session.engine.revision();
        let state_revision_before = session.engine.state_revision();
        let epoch_before = session.engine.yrs_state_epoch();
        let history_before = observe_component(pending_before.is_some(), 1, || {
            session.engine.availability_history_audit()
        });
        let history_metadata_before = observe_component(history_before.is_some(), 2, || {
            session.engine.availability_history_metadata_audit()
        });
        let outbox_before = observe_component(history_metadata_before.is_some(), 3, || {
            session
                .collaboration_outbox()
                .and_then(|outbox| outbox.availability_audit())
        });
        let outbox_count_before = session.collaboration_outbox().map(|outbox| {
            [
                outbox.pending_document_update_count(),
                outbox.pending_document_update_bytes(),
            ]
        });
        let encoded_before = observe_component(outbox_before.is_some(), 4, || {
            session.engine.availability_encoded_audit()
        });
        let raw_before = observe_component(encoded_before.is_some(), 5, || {
            session.engine.availability_content_audit()
        });
        let envelope = local_mutation_envelope(&mutation, request_id, base_document_revision);
        let mut bridge = NativeTransactionBridge::new(session);
        let outcome = match mutation {
            LocalMutation::Input { .. } => bridge.submit_input(&envelope),
            LocalMutation::Command { .. } => bridge.submit_command(&envelope),
            LocalMutation::Selection { .. } => bridge.submit_selection(&envelope),
        }?;
        let session = self.session_mut()?;
        let document_changed = session.engine.revision() != base_document_revision;
        let unavailable = matches!(
            outcome,
            crate::native_transaction_bridge::NativeBridgeOutcome::NotApplicable
        );
        let reason = crate::tables::command_context::take_preparation_refusal(request_id)
            .filter(|_| unavailable);
        let mut value = parse_outcome(serialize_native_outcome(outcome, false, document_changed));
        self.capture_outbound(EventOrigin::Local, request_id)?;
        let pending_unchanged = pending_before
            .as_ref()
            .is_some_and(|before| &self.pending_events == before);
        let setup_emitted = pending_count_before.saturating_sub(setup_events);
        let session = self.session_mut()?;
        let history_after = observe_component(raw_before.is_some(), 1, || {
            session.engine.availability_history_audit()
        });
        let history_metadata_after = observe_component(history_after.is_some(), 2, || {
            session.engine.availability_history_metadata_audit()
        });
        let outbox_after = observe_component(history_metadata_after.is_some(), 3, || {
            session
                .collaboration_outbox()
                .and_then(|outbox| outbox.availability_audit())
        });
        let encoded_after = observe_component(outbox_after.is_some(), 4, || {
            session.engine.availability_encoded_audit()
        });
        let raw_after = observe_component(encoded_after.is_some(), 5, || {
            session.engine.availability_content_audit()
        });
        let observed = pending_before.is_some()
            && history_before.is_some()
            && history_after.is_some()
            && history_metadata_before.is_some()
            && history_metadata_after.is_some()
            && outbox_before.is_some()
            && outbox_after.is_some()
            && encoded_before.is_some()
            && encoded_after.is_some()
            && raw_before.is_some()
            && raw_after.is_some();
        value["availability"] = serde_json::json!({
            "requestId": request_id.to_string(),
            "command": envelope,
            "observed": observed,
            "auditStatus": if observed { "complete" } else { "unavailable-or-over-budget" },
            "auditLimits": { "maxComponentBytes": 16 * 1024 * 1024, "maxItems": 65_536 },
            "reason": reason,
            "historyUnchanged": observed && history_before == history_after
                && history_metadata_before == history_metadata_after,
            "historyMetadataUnchanged": observed && history_metadata_before == history_metadata_after,
            "outboxUnchanged": observed && outbox_before == outbox_after && pending_unchanged,
            "encodedStateUnchanged": observed && encoded_before == encoded_after,
            "contentUnchanged": observed && raw_before == raw_after,
            "revisionUnchanged": base_document_revision == session.engine.revision()
                && state_revision_before == session.engine.state_revision()
                && epoch_before == session.engine.yrs_state_epoch(),
            "before": {
                "historyCounts": history_before.as_ref().map(|audit| audit.counts()),
                "historyMetadataItems": history_metadata_before.as_ref().map(Vec::len),
                "outboxCountAndBytes": outbox_count_before,
                "encodedState": encoded_before.map(|bytes| BASE64.encode(bytes)),
                "documentRevision": base_document_revision.to_string(),
                "stateRevision": state_revision_before.to_string(),
                "yrsStateEpoch": epoch_before.to_string(),
            },
            "after": {
                "historyCounts": history_after.as_ref().map(|audit| audit.counts()),
                "historyMetadataItems": history_metadata_after.as_ref().map(Vec::len),
                "outboxCountAndBytes": session.collaboration_outbox().map(|outbox|
                    [outbox.pending_document_update_count(), outbox.pending_document_update_bytes()]),
                "encodedState": encoded_after.map(|bytes| BASE64.encode(bytes)),
                "documentRevision": session.engine.revision().to_string(),
                "stateRevision": session.engine.state_revision().to_string(),
                "yrsStateEpoch": session.engine.yrs_state_epoch().to_string(),
            },
            "selectionSetup": {
                "documentRevisionBefore": setup_document_revision.to_string(),
                "documentRevisionAfter": base_document_revision.to_string(),
                "stateRevisionBefore": setup_revision.to_string(),
                "stateRevisionAfter": state_revision_before.to_string(),
                "emittedEvents": setup_emitted,
                "events": pending_before.as_ref().map(|events| &events[setup_events..]),
            },
        });
        Ok(value)
    }

    fn anchor_selection(&mut self, at: u32, head: Option<u32>) -> Result<(), SessionError> {
        let request_id = self.next_request_id();
        let session = self.session_mut()?;
        let base_document_revision = session.engine.revision();
        let document = session
            .engine
            .document()
            .ok_or_else(|| config_invalid("the peer has no document to anchor a command in"))?
            .clone();
        let scalar = session
            .engine
            .position_map()
            .ok_or_else(|| config_invalid("the peer has no position map to anchor a command in"))?
            .doc_to_scalar(at, &document);
        let Some(head) = head else {
            let epoch =
                session.pin_position_epoch(TABLE_INTEROP_OWNER_ID, base_document_revision)?;
            let intent = serde_json::json!({
                "version": NATIVE_BRIDGE_ENVELOPE_VERSION,
                "requestId": request_id.to_string(),
                "ownerId": TABLE_INTEROP_OWNER_ID.to_string(),
                "positionEpoch": epoch.to_string(),
                "intent": {
                    "type": SET_SELECTION_INTENT,
                    "anchor": scalar,
                    "head": scalar,
                },
            })
            .to_string();
            NativeTransactionBridge::new(session).submit_native_intent(&intent)?;
            self.capture_outbound(EventOrigin::Local, request_id)?;
            return Ok(());
        };
        let point = serde_json::json!({ "offset": scalar, "kind": "scalar" });
        let head_scalar = session
            .engine
            .position_map()
            .ok_or_else(|| config_invalid("the peer has no position map to anchor a command in"))?
            .doc_to_scalar(head, &document);
        let selection = serde_json::json!({
            "type": "cell",
            "anchorCell": point,
            "headCell": { "offset": head_scalar, "kind": "scalar" },
        });
        let envelope = serde_json::json!({
            "version": NATIVE_BRIDGE_ENVELOPE_VERSION,
            "requestId": request_id.to_string(),
            "baseDocumentRevision": base_document_revision.to_string(),
            "selection": selection,
        })
        .to_string();
        NativeTransactionBridge::new(session).submit_selection(&envelope)?;
        self.capture_outbound(EventOrigin::Local, request_id)?;
        Ok(())
    }

    fn history(
        &mut self,
        payload: serde_json::Value,
        undo: bool,
    ) -> Result<serde_json::Value, SessionError> {
        let _: EmptyPayload = parse_payload(payload)?;
        reset_planned_normalization_passes();
        let request_id = self.next_request_id();
        let session = self.session_mut()?;
        let mut bridge = NativeTransactionBridge::new(session);
        let applied = if undo {
            bridge.undo(request_id)?
        } else {
            bridge.redo(request_id)?
        };
        let session = self.session_mut()?;
        let revision = session.engine.revision();
        self.capture_outbound(EventOrigin::History, request_id)?;
        Ok(serde_json::json!({
            "applied": applied,
            "documentRevision": revision.to_string(),
        }))
    }

    fn apply_update(
        &mut self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        let payload: UpdatePayload = parse_payload(payload)?;
        reset_planned_normalization_passes();
        let update = decode_base64(&payload.update_base64, "updateBase64")?;
        let request_id = self.next_request_id();
        let session = self.session_mut()?;
        let prepared = session
            .engine
            .prepare_remote_update_v1(request_id, &update)
            .map_err(operation_error)?;
        let commit = session
            .engine
            .commit_prepared_remote_update(prepared)
            .map_err(operation_error)?;
        self.capture_outbound(EventOrigin::Remote, request_id)?;
        Ok(serde_json::json!({
            "changed": commit.changed,
            "documentRevision": commit.revision.to_string(),
        }))
    }

    fn repair_table_during_remote_window(
        &mut self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        let _: EmptyPayload = parse_payload(payload)?;
        reset_planned_normalization_passes();
        let request_id = self.next_request_id();
        let session = self.session_mut()?;
        let authored = session.engine.document_json_string().ok_or_else(|| {
            peer_error(
                AUTONOMOUS_REPAIR_CANARY_FAILED,
                "the peer has no document to repair",
            )
        })?;
        let (engine, outbox) = session.engine_and_outbox();
        let commit = engine
            .prepare_root_replacement_json_with_outbox(
                request_id,
                &authored,
                ReplacementHistory::UndoableBoundary,
                outbox,
            )
            .map_err(|error| match error {
                RootReplacementError::Admission(admission) => SessionError::from(admission),
                RootReplacementError::Transaction(transaction) => operation_error(transaction),
            })?;
        self.capture_outbound(EventOrigin::Remote, request_id)?;
        Ok(serde_json::json!({
            "changed": commit.changed,
            "documentRevision": commit.document_revision.to_string(),
            "normalizationPasses": planned_normalization_passes(),
        }))
    }

    fn set_awareness(
        &mut self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        let payload: AwarenessIntentPayload = parse_payload(payload)?;
        let request_id = self.next_request_id();
        let session = self.session_mut()?;
        let document_revision_before = session.engine.revision();
        if payload.intent.is_null() {
            session.clear_desired_awareness(request_id)?;
        } else {
            session.set_awareness_intent(request_id, &payload.intent.to_string())?;
        }
        let update = session
            .engine
            .awareness()
            .encode_local_update_v1()
            .map_err(SessionError::from)?;
        let document_revision = session.engine.revision();
        if document_revision != document_revision_before {
            return Err(peer_error(
                "AWARENESS_MUTATED_DOCUMENT",
                "publishing awareness must never change the document",
            ));
        }
        self.pending_events.push(serde_json::json!({
            "kind": AWARENESS_EVENT_KIND,
            "origin": EventOrigin::Local.as_str(),
            "bytesBase64": BASE64.encode(update),
        }));
        Ok(serde_json::json!({ "documentRevision": document_revision.to_string() }))
    }

    fn apply_awareness(
        &mut self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        let payload: UpdatePayload = parse_payload(payload)?;
        let update = decode_base64(&payload.update_base64, "updateBase64")?;
        let session = self.session_mut()?;
        let document_revision_before = session.engine.revision();
        let limits = awareness_limits(&CollaborationLimits::default());
        let applied = session
            .engine
            .awareness()
            .apply_remote_update_v1(&update, &limits)
            .map_err(SessionError::from)?;
        let document_revision = session.engine.revision();
        if document_revision != document_revision_before {
            return Err(peer_error(
                "AWARENESS_MUTATED_DOCUMENT",
                "applying awareness must never change the document",
            ));
        }
        Ok(serde_json::json!({
            "touchedClients": applied
                .touched_clients
                .iter()
                .map(|client| client.to_string())
                .collect::<Vec<String>>(),
            "removedClients": applied
                .removed_clients
                .iter()
                .map(|client| client.to_string())
                .collect::<Vec<String>>(),
            "documentRevision": document_revision.to_string(),
        }))
    }

    fn awareness_peers(&mut self) -> Result<serde_json::Value, SessionError> {
        let session = self.session_mut()?;
        let peers = session
            .awareness_peers()
            .map_err(|mut error| {
                error.details = Some(serde_json::json!({ "action": AWARENESS_ACTION }));
                error
            })?
            .into_iter()
            .map(|peer| {
                serde_json::json!({
                    "clientId": peer.client_id.to_string(),
                    "isLocal": peer.is_local,
                    "state": peer.state,
                    "cursor": peer.cursor.map(|cursor| serde_json::json!({
                        "anchor": cursor.anchor,
                        "head": cursor.head,
                    })),
                    "cellRectangle": peer.cell_rectangle.map(|rectangle| serde_json::json!({
                        "anchorCell": rectangle.anchor_cell,
                        "headCell": rectangle.head_cell,
                    })),
                })
            })
            .collect::<Vec<serde_json::Value>>();
        Ok(serde_json::Value::Array(peers))
    }

    fn drain(&mut self, payload: serde_json::Value) -> Result<serde_json::Value, SessionError> {
        let _: EmptyPayload = parse_payload(payload)?;
        let count = self.pending_events.len();
        self.emitted_events = std::mem::take(&mut self.pending_events);
        Ok(serde_json::json!({
            "count": count,
            "pendingDependencies": self.has_pending_dependencies(),
        }))
    }

    fn snapshot(&mut self, payload: serde_json::Value) -> Result<serde_json::Value, SessionError> {
        let _: EmptyPayload = parse_payload(payload)?;
        let autonomous_repair_writes = self.autonomous_repair_writes;
        let pending_dependencies = self.has_pending_dependencies();
        let awareness_peers = self.awareness_peers()?;
        let session = self.session_mut()?;
        Ok(serde_json::json!({
            "awarenessPeers": awareness_peers,
            "mounted": session.engine.is_ready(),
            "pendingDependencies": pending_dependencies,
            "json": session.engine.document_json(),
            "html": session.engine.document_html(),
            "documentRevision": session.engine.revision().to_string(),
            "stateRevision": session.engine.state_revision().to_string(),
            "canUndo": session.engine.can_undo(),
            "canRedo": session.engine.can_redo(),
            "autonomousRepairWrites": autonomous_repair_writes,
            "normalizationPassesAfterLastAction": planned_normalization_passes(),
        }))
    }

    fn state_vector(
        &mut self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        let _: EmptyPayload = parse_payload(payload)?;
        let request_id = self.next_request_id();
        let session = self.session_mut()?;
        let encoded = session
            .engine
            .encode_state_vector_v1(request_id)
            .map_err(operation_error)?;
        Ok(serde_json::json!({ "stateVectorBase64": BASE64.encode(encoded) }))
    }

    fn state_diff(
        &mut self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        let payload: StateVectorPayload = parse_payload(payload)?;
        let state_vector = decode_base64(&payload.state_vector_base64, "stateVectorBase64")?;
        let request_id = self.next_request_id();
        let session = self.session_mut()?;
        let diff = session
            .engine
            .encode_diff_v1(request_id, &state_vector)
            .map_err(operation_error)?;
        Ok(serde_json::json!({ "updateBase64": BASE64.encode(diff) }))
    }

    fn shutdown(&mut self, payload: serde_json::Value) -> Result<serde_json::Value, SessionError> {
        let _: EmptyPayload = parse_payload(payload)?;
        if let Some(session) = self.session.as_mut() {
            session.teardown();
        }
        Ok(serde_json::json!({}))
    }

    fn has_pending_dependencies(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.engine.pending_remote_dependency_bytes() > 0)
    }

    fn capture_outbound(
        &mut self,
        origin: EventOrigin,
        request_id: u64,
    ) -> Result<(), SessionError> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let (_, outbox) = session.engine_and_outbox();
        let Some(outbox) = outbox else {
            return Ok(());
        };
        loop {
            let lease = outbox.lease_next().map_err(|error| {
                outbound_lease_session_error(error, request_id, LEASE_ACTION, None)
            })?;
            let Some(lease) = lease else {
                return Ok(());
            };
            let (kind, bytes) = match lease.payload {
                OutboundLeasePayload::DocumentUpdate(update) => ("document", update),
                OutboundLeasePayload::ProtocolReply(frame) => ("protocol", frame),
            };
            outbox.ack_lease(lease.lease_id).map_err(|error| {
                outbound_lease_session_error(
                    error,
                    request_id,
                    ACK_ACTION,
                    Some(lease.lease_id.value()),
                )
            })?;
            if kind == "document" && origin == EventOrigin::Remote {
                self.autonomous_repair_writes = self.autonomous_repair_writes.saturating_add(1);
            }
            self.pending_events.push(serde_json::json!({
                "kind": kind,
                "origin": origin.as_str(),
                "bytesBase64": BASE64.encode(bytes),
            }));
        }
    }
}

fn local_mutation_envelope(
    mutation: &LocalMutation,
    request_id: u64,
    base_document_revision: u64,
) -> String {
    let mut envelope = serde_json::json!({
        "version": NATIVE_BRIDGE_ENVELOPE_VERSION,
        "requestId": request_id.to_string(),
        "baseDocumentRevision": base_document_revision.to_string(),
    });
    match mutation {
        LocalMutation::Input { text } => {
            envelope["text"] = serde_json::Value::String(text.clone());
        }
        LocalMutation::Command { command, .. } => {
            envelope["command"] = command.clone();
        }
        LocalMutation::Selection { selection } => {
            envelope["selection"] = selection.clone();
        }
    }
    envelope.to_string()
}

fn parse_outcome(serialized: String) -> serde_json::Value {
    serde_json::from_str(&serialized).expect("native outcomes serialize as JSON objects")
}

fn normalize_table_payload(payload: serde_json::Value) -> Result<serde_json::Value, SessionError> {
    let payload: ProjectTablePayload = parse_payload(payload)?;
    let limits = ResourceLimits::default();
    let schema = Schema::from_json(&payload.schema)
        .map_err(|error| config_invalid(format!("the normalized schema is invalid: {error}")))?;
    let document = from_prosemirror_json_with_limits(
        &serde_json::json!({
            "type": PROJECTED_TABLE_DOCUMENT_ROOT,
            "content": [payload.table],
        }),
        &schema,
        UnknownTypeMode::Preserve,
        &limits,
    )
    .map_err(|error| config_invalid(format!("the normalized table is invalid: {error}")))?;
    let operations =
        normalize_outer_table(&document, PROJECTED_TABLE_POSITION, &schema, &limits)
            .map_err(|error| peer_error(TABLE_NORMALIZATION_FAILED, error.message.to_string()))?;
    let normalized = apply_operations(&document, &schema, &operations).map_err(|()| {
        peer_error(
            TABLE_NORMALIZATION_FAILED,
            "the normalization plan does not apply to its own table",
        )
    })?;
    let json = to_prosemirror_json(&normalized, &schema);
    let table = json
        .get("content")
        .and_then(|content| content.get(0))
        .cloned()
        .ok_or_else(|| peer_error(TABLE_NORMALIZATION_FAILED, "the table did not survive"))?;
    Ok(serde_json::json!({
        "table": table,
        "operations": operations.len(),
    }))
}

fn project_table_payload(payload: serde_json::Value) -> Result<serde_json::Value, SessionError> {
    let payload: ProjectTablePayload = parse_payload(payload)?;
    let limits = ResourceLimits::default();
    let schema = Schema::from_json(&payload.schema)
        .map_err(|error| config_invalid(format!("the projected schema is invalid: {error}")))?;
    let document = from_prosemirror_json_with_limits(
        &serde_json::json!({
            "type": PROJECTED_TABLE_DOCUMENT_ROOT,
            "content": [payload.table],
        }),
        &schema,
        UnknownTypeMode::Preserve,
        &limits,
    )
    .map_err(|error| config_invalid(format!("the projected table is invalid: {error}")))?;
    let table = document
        .root()
        .child(0)
        .ok_or_else(|| config_invalid("the projected payload carried no table node"))?;
    let projected = project_table(
        table,
        PROJECTED_TABLE_POSITION,
        &schema,
        &mut TableGridBudget::new(limits.max_table_grid_slots),
    )
    .map_err(|error| peer_error("TABLE_PROJECTION_FAILED", error.to_string()))?;
    Ok(projected_table_json(&projected, &schema))
}

fn projected_table_json(projected: &ProjectedTable, schema: &Schema) -> serde_json::Value {
    let anchors: Vec<Option<u32>> = projected
        .slots
        .iter()
        .map(|slot| {
            slot.and_then(|index| projected.cells.get(index))
                .map(|cell| cell.source_pos)
        })
        .collect();
    serde_json::json!({
        "rows": projected.rows,
        "columns": projected.columns,
        "widths": projected.widths,
        "irregular": projected.irregular,
        "slots": anchors,
        "compatibilityDiagnostic": projected.compatibility_diagnostic,
        "synthetic": projected.synthetic.iter().map(|region| serde_json::json!({
            "row": region.rect.row,
            "column": region.rect.column,
            "rowspan": region.rect.rowspan,
            "colspan": region.rect.colspan,
            "node": synthetic_node_json(&region.effective_node(), schema),
        })).collect::<Vec<_>>(),
    })
}

fn synthetic_node_json(node: &crate::model::Node, schema: &Schema) -> serde_json::Value {
    let mut value = crate::serialize::json_out::node_to_json(node, schema);
    let mut pending = vec![(node, &mut value)];
    while let Some((node, json)) = pending.pop() {
        json["attrs"] = serde_json::json!(node.attrs());
        if let (Some(content), Some(children)) = (
            node.content(),
            json.get_mut("content")
                .and_then(serde_json::Value::as_array_mut),
        ) {
            pending.extend(content.iter().zip(children.iter_mut()));
        }
    }
    value
}

fn parse_payload<T: serde::de::DeserializeOwned>(
    payload: serde_json::Value,
) -> Result<T, SessionError> {
    serde_json::from_value(payload)
        .map_err(|error| config_invalid(format!("invalid request payload: {error}")))
}

fn decode_base64(encoded: &str, field: &str) -> Result<Vec<u8>, SessionError> {
    BASE64
        .decode(encoded)
        .map_err(|error| config_invalid(format!("{field} is not valid base64: {error}")))
}

fn recover_request_identifier(line: &str) -> String {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| {
            value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|id| !id.is_empty() && id.len() <= MAX_REQUEST_ID_BYTES)
        .unwrap_or_default()
}
