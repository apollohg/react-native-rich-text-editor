use serde_json::json;

use crate::boundary::{BoundaryError, ResourceLimits};
use crate::session::{
    CollaborationLimits, DocumentState, ErrorDomain, OperationFailureClass, SessionError,
    TransportState,
};
use crate::yrs_engine::{EditingLimits, OperationError, YrsEngineError};

use super::types::{FfiError, FfiJsonResult, FfiUnitResult, ERROR_DOMAINS, OPERATION_ERROR_CODES};

fn shared_contract() -> serde_json::Value {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../scripts/tests/security-contract-fixtures.json"
    ))
    .unwrap();
    fixtures["ffiV2ErrorContract"].clone()
}

#[test]
fn render_update_emits_table_references_without_pools() {
    let editor_id = create_editor(json!({
        "schema": crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        "initialization": { "type": "localJson", "json": {
            "type": "doc", "content": [{ "type": "table", "content": [{
                "type": "table_row", "content": [{ "type": "table_cell", "content": [{
                    "type": "paragraph", "content": [{ "type": "text", "text": "private-cell-content" }]
                }] }]
            }] }]
        } }
    }));
    let result = super::render::editor_v2_render_update(editor_id.clone(), None, None);
    assert_eq!(
        super::editor::editor_v2_destroy(editor_id).value,
        Some(true)
    );
    let raw = result.value.expect("table snapshot");
    let snapshot: serde_json::Value = serde_json::from_str(&raw).expect("snapshot JSON");
    for key in ["tableAttributes", "tableRecords", "tableInputMappings"] {
        assert!(
            snapshot.get(key).is_none(),
            "JavaScript snapshot contains {key}"
        );
    }
    let reference = snapshot["renderBlocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|block| block.as_array().unwrap())
        .find(|element| element["type"] == "table")
        .expect("root table reference");
    assert_eq!(reference.as_object().unwrap().len(), 2);
    assert!(!reference["tableId"]
        .as_str()
        .expect("stable table identifier")
        .is_empty());
    assert!(
        !raw.contains("private-cell-content"),
        "cell content stays native"
    );
}

#[test]
fn decimal_u64_serializer_keeps_the_full_u64_domain_as_canonical_strings() {
    for value in [0, (1_u64 << 53) + 1, u64::MAX] {
        assert_eq!(
            super::types::decimal_u64(value),
            json!(value.to_string()),
            "u64 {value} must cross the v2 wire as decimal text"
        );
    }
}

#[test]
fn request_envelope_errors_preserve_admitted_zero_but_omit_unadmitted_ids() {
    let editor_id = create_editor(json!({
        "initialization": { "type": "localEmpty" },
    }));

    let admitted = super::editor::editor_v2_replace_document(
        editor_id.clone(),
        json!({
            "version": 1,
            "requestId": "0",
            "history": "resetAndClear",
            "unexpected": true,
        })
        .to_string(),
    )
    .error
    .expect("invalid request must carry an error");
    assert_eq!(admitted.code, "CONFIG_INVALID");
    assert_eq!(admitted.request_id.as_deref(), Some("0"));

    let unadmitted = super::editor::editor_v2_replace_document(
        editor_id.clone(),
        json!({
            "version": 1,
            "requestId": "00",
            "history": "resetAndClear",
        })
        .to_string(),
    )
    .error
    .expect("invalid request id must carry an error");
    assert_eq!(unadmitted.code, "CONFIG_INVALID");
    assert_eq!(unadmitted.request_id, None);

    assert_eq!(
        super::editor::editor_v2_destroy(editor_id).value,
        Some(true)
    );
}

type BridgeMutationEntry = fn(String, String) -> FfiJsonResult;

fn bridge_mutation_entries() -> [(&'static str, BridgeMutationEntry, serde_json::Value); 4] {
    [
        (
            "applyInput",
            super::editor::editor_v2_apply_input,
            json!({ "text": "x" }),
        ),
        (
            "applyCommand",
            super::editor::editor_v2_apply_command,
            json!({ "command": { "type": "toggleBlockquote" } }),
        ),
        (
            "setSelection",
            super::editor::editor_v2_set_selection,
            json!({ "selection": { "type": "all" } }),
        ),
        (
            "applyLocalApi",
            super::editor::editor_v2_apply_local_api,
            json!({
                "setJson": { "type": "doc" },
                "history": "resetAndClear",
            }),
        ),
    ]
}

#[test]
fn bridge_mutation_parse_errors_recover_an_admitted_zero_request_id() {
    let editor_id = create_editor(json!({
        "initialization": { "type": "localEmpty" },
    }));

    for (label, apply, payload) in bridge_mutation_entries() {
        let mut request = json!({
            "version": 1,
            "requestId": "0",
            "baseDocumentRevision": "0",
            "unexpected": true,
        });
        for (key, value) in payload.as_object().expect("payload object") {
            request[key] = value.clone();
        }

        let error = apply(editor_id.clone(), request.to_string())
            .error
            .expect("malformed request must carry an error");
        assert_eq!(error.code, "CONFIG_INVALID", "{label}: {error:?}");
        assert_eq!(
            error.request_id.as_deref(),
            Some("0"),
            "{label}: canonical request id must survive parse failure"
        );
    }

    assert_eq!(
        super::editor::editor_v2_destroy(editor_id).value,
        Some(true)
    );
}

#[test]
fn bridge_mutation_parse_errors_omit_an_unadmitted_request_id() {
    let editor_id = create_editor(json!({
        "initialization": { "type": "localEmpty" },
    }));

    for (label, apply, payload) in bridge_mutation_entries() {
        let mut request = json!({
            "version": 1,
            "requestId": "00",
            "baseDocumentRevision": "0",
        });
        for (key, value) in payload.as_object().expect("payload object") {
            request[key] = value.clone();
        }

        let error = apply(editor_id.clone(), request.to_string())
            .error
            .expect("malformed request must carry an error");
        assert_eq!(error.code, "CONFIG_INVALID", "{label}: {error:?}");
        assert_eq!(
            error.request_id, None,
            "{label}: noncanonical request id must not be admitted"
        );
    }

    assert_eq!(
        super::editor::editor_v2_destroy(editor_id).value,
        Some(true)
    );
}

const CREATE_LIMIT_CASES: [(&str, &str, u64); 21] = [
    ("resource", "maxInputBytes", 64 * 1024 * 1024),
    ("resource", "maxDocumentNodes", 1_000_000),
    ("resource", "maxDocumentDepth", 1_024),
    ("resource", "maxSchemaNodes", 10_000),
    ("resource", "maxSchemaExpressionBytes", 1024 * 1024),
    ("resource", "maxCollaborationMessageBytes", 64 * 1024 * 1024),
    ("resource", "maxEncodedStateBytes", 256 * 1024 * 1024),
    ("editing", "maxOperationsPerTransaction", 4_096),
    ("editing", "maxUndoGroups", 2_000),
    ("editing", "maxUndoRetainedUnits", 8_000_000),
    ("editing", "maxDerivedOutputBytes", 128 * 1024 * 1024),
    ("collaboration", "maxFramesPerMessage", 1_024),
    ("collaboration", "maxFrameBytes", 64 * 1024 * 1024),
    (
        "collaboration",
        "maxAggregateResponseBytes",
        64 * 1024 * 1024,
    ),
    ("collaboration", "maxAwarenessPeers", 10_000),
    ("collaboration", "maxAwarenessPeerBytes", 1024 * 1024),
    ("collaboration", "maxAwarenessBytes", 64 * 1024 * 1024),
    ("collaboration", "maxPendingOutboxMessages", 4_096),
    ("collaboration", "maxPendingOutboxBytes", 64 * 1024 * 1024),
    (
        "collaboration",
        "maxPendingDependencyUpdateBytes",
        64 * 1024 * 1024,
    ),
    ("collaboration", "maxPendingDependencyUpdateWork", 8_000_000),
];

include!("tests/create_admission.rs");

include!("tests/wire_contract.rs");
