use super::*;
use crate::session::{
    CollaborationLimits, DocumentState, EditorSession, EditorSessionConfig, SessionPolicy,
};
use crate::yrs_engine::{InitializationMode, LoweringRefusal, ReplacementHistory};
use serde_json::Value;

const REQUEST: u64 = 73_100;
const OWNER: u64 = 73_001;
const REQUESTS_PER_CASE: u64 = 20;
const REMOTE_TEXT: &str = "remote";

struct RefusedCommand {
    label: &'static str,
    document: Value,
    setup: Vec<((u32, u32), TypedCommand)>,
    selection: (u32, u32),
    command: TypedCommand,
    refusal: LoweringRefusal,
}

fn paragraph(text: &str) -> Value {
    json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
}

fn with_tail(mut content: Vec<Value>) -> Value {
    content.push(paragraph("tail"));
    json!({ "type": "doc", "content": content })
}

fn refused_commands() -> Vec<RefusedCommand> {
    vec![
        RefusedCommand {
            label: "typing over a range that starts in a paragraph emptied by backspace",
            document: with_tail(vec![
                paragraph("xy"),
                json!({ "type": "paragraph", "content": [
                    { "type": "text", "text": "a" },
                    { "type": "hardBreak" }
                ] }),
            ]),
            setup: vec![((0, 2), TypedCommand::DeleteBackward)],
            selection: (3, 0),
            command: TypedCommand::ReplaceSelectionText { text: "z".into() },
            refusal: LoweringRefusal::UnresolvedTextTarget,
        },
        RefusedCommand {
            label: "return over a range from an empty list paragraph into its sibling",
            document: with_tail(vec![json!({ "type": "bulletList", "content": [
                { "type": "listItem", "content": [
                    { "type": "paragraph" },
                    paragraph("bcbc")
                ] }
            ] })]),
            setup: vec![],
            selection: (4, 0),
            command: TypedCommand::DeleteAndSplit,
            refusal: LoweringRefusal::CreatedTextBeforeStructuralEdit,
        },
    ]
}

fn session_with_document(document: &Value) -> EditorSession {
    let config = EditorSessionConfig::local_for_test();
    let mut session = EditorSession::new(
        YrsDocumentEngine::new(YrsEngineConfig {
            schema: crate::schema::presets::tiptap_table_schema(),
            ..transaction_engine_config(crate::yrs_engine::EditingLimits::default())
        })
        .unwrap(),
        SessionPolicy::from_config(&config),
        DocumentState::LocalReady,
        CollaborationLimits::default(),
    )
    .unwrap();
    session
        .replace_document_json(
            REQUEST,
            &document.to_string(),
            ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    session
}

fn prepared_session(case: &RefusedCommand, request: u64) -> EditorSession {
    let mut session = session_with_document(&case.document);
    for (index, ((anchor, head), command)) in case.setup.iter().enumerate() {
        let step_request = request + 2 * index as u64;
        select_text(&mut session.engine, step_request, *anchor, *head);
        session
            .engine
            .apply_command(step_request + 1, command.clone())
            .unwrap_or_else(|error| panic!("{}: setup step {index}: {error:?}", case.label))
            .unwrap();
    }
    let (anchor, head) = case.selection;
    select_text(
        &mut session.engine,
        request + REQUESTS_PER_CASE / 2,
        anchor,
        head,
    );
    session
}

#[derive(Debug, PartialEq)]
struct RefusalAudit {
    engine: AtomicAudit,
    prepared_candidate_state_vector: Option<StateVector>,
    lookup_matches_fresh: bool,
    canonical_sha256: [u8; 32],
    position_map_scalars: u32,
    pinned_epoch_range: Result<crate::position_epoch::ResolvedEpochRange, String>,
    request_outcome: Result<Option<String>, String>,
}

fn refusal_audit(session: &EditorSession, epoch: u64, request: u64) -> RefusalAudit {
    let state = session.engine.derived_state.as_ref().unwrap();
    let total = state.position_map.total_scalars();
    RefusalAudit {
        engine: atomic_audit(&session.engine),
        prepared_candidate_state_vector: session
            .engine
            .prepared_candidate_cache
            .as_ref()
            .map(|cache| cache.state_vector.clone()),
        lookup_matches_fresh: session.engine.mutation_lookup_matches_fresh_for_test(),
        canonical_sha256: state.canonical_artifact.sha256(),
        position_map_scalars: total,
        pinned_epoch_range: session
            .resolve_epoch_range(OWNER, epoch, 0, total)
            .map_err(|error| error.code),
        request_outcome: session
            .native_request_outcome(OWNER, request)
            .map(|outcome| outcome.map(str::to_owned))
            .map_err(|error| error.code),
    }
}

fn direct_transaction(
    session: &EditorSession,
    case: &RefusedCommand,
    request: u64,
) -> TypedTransaction {
    match session
        .engine
        .plan_command(request, case.command.clone())
        .unwrap_or_else(|error| panic!("{}: planning failed: {error:?}", case.label))
    {
        CommandPlan::Transaction(transaction) => transaction,
        plan => panic!(
            "{}: expected a direct transaction, planned {plan:?}",
            case.label
        ),
    }
}

fn semantic_preview(
    engine: &YrsDocumentEngine,
    transaction: &TypedTransaction,
) -> crate::yrs_engine::OperationResult<crate::model::Document> {
    let state = engine.derived_state.as_ref().unwrap();
    let selection = state.legacy_selection();
    crate::yrs_engine::compiler::compile_transaction_with_view(
        crate::yrs_engine::compiler::CompilationContext {
            document: &state.document,
            selection: Some(&selection),
            schema: &engine.schema,
            resource_limits: &engine.resource_limits,
            editing_limits: &engine.editing_limits,
            document_revision: engine.revision,
            max_length: engine.max_length,
            validation_certificate: None,
        },
        transaction.clone(),
        None,
    )
    .map(|compiled| compiled.preview)
}

fn collect_text_outside(
    node: &crate::model::Node,
    content_start: u32,
    (from, to): (u32, u32),
    outside: &mut (String, String),
) {
    let mut position = content_start;
    for child in node
        .content()
        .into_iter()
        .flat_map(|content| content.iter())
    {
        if let Some(text) = child.text_str() {
            for (offset, character) in (position..).zip(text.chars()) {
                if offset < from {
                    outside.0.push(character);
                } else if offset >= to {
                    outside.1.push(character);
                }
            }
        } else if child.is_element() {
            collect_text_outside(child, position + 1, (from, to), outside);
        }
        position += child.node_size();
    }
}

fn text_outside_selection(engine: &YrsDocumentEngine) -> (String, String) {
    let state = engine.derived_state.as_ref().unwrap();
    let Selection::Text { anchor, head } = state.legacy_selection() else {
        panic!("the refused commands run at a text selection")
    };
    let mut outside = (String::new(), String::new());
    collect_text_outside(
        state.document.root(),
        0,
        (anchor.min(head), anchor.max(head)),
        &mut outside,
    );
    outside
}

fn assert_text_outside_selection_survives(
    engine: &YrsDocumentEngine,
    (before, after): &(String, String),
    label: &str,
) {
    let text = engine.document().unwrap().root().text_content();
    assert!(
        text.starts_with(before.as_str()) && text.ends_with(after.as_str()),
        "{label}: text outside the edited range must survive: {before:?} .. {after:?} in {text:?}"
    );
}

#[test]
fn a_lowering_refusal_leaves_no_trace_and_the_structural_retry_commits_the_simulated_document() {
    for (index, case) in refused_commands().into_iter().enumerate() {
        let request = REQUEST + REQUESTS_PER_CASE * (index as u64 + 1);
        let mut session = prepared_session(&case, request);
        let direct = direct_transaction(&session, &case, request + 11);
        let simulated = semantic_preview(&session.engine, &direct)
            .unwrap_or_else(|error| panic!("{}: semantic compile failed: {error:?}", case.label));
        let epoch = session
            .pin_position_epoch(OWNER, session.engine.revision())
            .unwrap();
        let before = refusal_audit(&session, epoch, request + 12);
        let before_json = session.engine.document_json().unwrap();
        let outside = text_outside_selection(&session.engine);

        let refused = session
            .engine
            .apply_typed_transaction(direct)
            .expect_err(case.label);
        assert_eq!(
            refused.lowering_refusal,
            Some(case.refusal),
            "{}: the direct transaction must be refused by the lowering: {refused:?}",
            case.label
        );
        assert_eq!(
            refusal_audit(&session, epoch, request + 12),
            before,
            "{}: a refused lowering attempt must leave no trace",
            case.label
        );

        session
            .engine
            .apply_command(request + 12, case.command.clone())
            .unwrap_or_else(|error| panic!("{}: structural retry failed: {error:?}", case.label))
            .unwrap();
        assert_eq!(
            session.engine.document(),
            Some(&simulated),
            "{}: the structural retry must commit the planner's simulated document",
            case.label
        );
        assert_text_outside_selection_survives(&session.engine, &outside, case.label);
        let after_json = session.engine.document_json().unwrap();
        assert!(
            session.engine.undo(request + 13).unwrap().is_some(),
            "{}",
            case.label
        );
        assert_eq!(
            session.engine.document_json().unwrap(),
            before_json,
            "{}: undo restores the document",
            case.label
        );
        assert!(
            session.engine.redo(request + 14).unwrap().is_some(),
            "{}",
            case.label
        );
        assert_eq!(
            session.engine.document_json().unwrap(),
            after_json,
            "{}: redo reapplies the structural retry",
            case.label
        );
    }
}

#[test]
fn a_structural_retry_converges_with_a_concurrent_remote_edit_elsewhere() {
    for (index, case) in refused_commands().into_iter().enumerate() {
        let request = REQUEST + REQUESTS_PER_CASE * (index as u64 + 10);
        let mut session = prepared_session(&case, request);
        let mut peer = YrsDocumentEngine::new(YrsEngineConfig {
            schema: session.engine.schema().clone(),
            initialization_mode: InitializationMode::AwaitRemote,
            ..transaction_engine_config(crate::yrs_engine::EditingLimits::default())
        })
        .unwrap();
        peer.apply_remote_update_v1(request + 11, &session.engine.encoded_state().unwrap())
            .unwrap();
        let end = peer
            .derived_state
            .as_ref()
            .unwrap()
            .position_map
            .total_scalars();
        select_text(&mut peer, request + 12, end, end);
        peer.apply_command(
            request + 13,
            TypedCommand::InsertText {
                text: REMOTE_TEXT.into(),
            },
        )
        .unwrap()
        .unwrap();

        let outside = text_outside_selection(&session.engine);
        session
            .engine
            .apply_command(request + 14, case.command.clone())
            .unwrap_or_else(|error| panic!("{}: structural retry failed: {error:?}", case.label))
            .unwrap();
        assert_text_outside_selection_survives(&session.engine, &outside, case.label);
        let local_only = session.engine.document_json().unwrap();
        let local_update = session.engine.encoded_state().unwrap();
        session
            .engine
            .apply_remote_update_v1(request + 15, &peer.encoded_state().unwrap())
            .unwrap();
        peer.apply_remote_update_v1(request + 16, &local_update)
            .unwrap();

        let merged = session.engine.document_json().unwrap();
        assert_eq!(
            merged,
            peer.document_json().unwrap(),
            "{}: both peers converge",
            case.label
        );
        let mut expected = local_only.clone();
        let content = expected["content"].as_array_mut().unwrap();
        let tail = content.last_mut().unwrap();
        tail["content"][0]["text"] = json!(format!("tail{REMOTE_TEXT}"));
        assert_eq!(
            merged, expected,
            "{}: the remote edit survives next to the structural retry {local_only}",
            case.label
        );
    }
}
