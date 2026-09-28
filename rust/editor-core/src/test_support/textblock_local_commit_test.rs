use serde_json::{json, Value};

use super::large_table_fixture::{
    fixture_cell_text, keystroke_cell, multi_paragraph_cell_document, plain_table_document,
    session_with_document, session_with_document_and_editing_limits,
};
use crate::boundary::ResourceLimits;
use crate::model::Mark;
use crate::native_transaction_bridge::NativeTransactionBridge;
use crate::render::incremental::CachedRenderBlocks;
use crate::session::{EditorSession, SessionError};
use crate::tables::render::TableRenderRecord;
use crate::yrs_engine::observability::{
    reset_full_pass_counts_for_test, take_full_pass_counts_for_test, FullPassCounts,
};
use crate::yrs_engine::{EditingLimits, ResolvedSelection};

const OWNER_ID: u64 = 9;
const INSERT_REQUEST_ID: u64 = 5;
const UNDO_REQUEST_ID: u64 = 6;
const REDO_REQUEST_ID: u64 = 7;
const INSERTED_TEXT: &str = "x";
const INSERT_INTENTS: [&str; 2] = ["insertText", "replaceSelectionText"];
const PROSE_TEXT: &str = "Prose";
const TABLE_SIZE: usize = 3;
const EMPTY_CELL: (usize, usize) = (1, 1);
const TEXT_CELL: (usize, usize) = (0, 1);
const MIDDLE_OFFSET: u32 = 3;
const LEAF_START: u32 = 0;
const IDENTITY_PREDICATE_VISIT_CEILING: usize = 64;
const LARGE_TABLE_ROWS: usize = 1000;
const LARGE_TABLE_COLUMNS: usize = 20;
const PROSE_PATH: [u32; 1] = [0];
const EMPTY_PARAGRAPH_PATH: [u32; 1] = [1];
const PROSE_TABLE_INDEX: u32 = 2;
const CELL_PARAGRAPH_INDEX: u32 = 0;
const MULTI_PARAGRAPH_SECOND_PARAGRAPH: [u32; 4] = [0, 1, 1, 2];
const MULTI_PARAGRAPH_ATOM_PARAGRAPH: [u32; 4] = [0, 1, 1, 0];
const NESTED_TABLE_CELL_PARAGRAPH: [u32; 7] = [0, 2, 2, 0, 0, 0, 0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedRoute {
    Localized,
    Generic,
    NestedTableRefusal,
    CellBoundaryRefusal,
}

#[derive(Debug, Clone, Copy)]
enum CaretOffset {
    At(u32),
    FirstLeafEnd,
    ContentEnd,
}

struct TextblockEditCase {
    name: &'static str,
    document: Value,
    block_path: Vec<u32>,
    offset: CaretOffset,
    route: ExpectedRoute,
    intent: &'static str,
    selection_len: u32,
    replacement_text: &'static str,
    editing_limits: EditingLimits,
}

#[derive(Debug, PartialEq)]
struct CommitAudit {
    document_json: Value,
    encoded_state: Vec<u8>,
    document_revision: u64,
    state_revision: u64,
    selection: Option<ResolvedSelection>,
    stored_marks: Option<Vec<Mark>>,
    can_undo: bool,
    can_redo: bool,
    outbox_updates: usize,
    retained_history: (u64, usize),
}

#[derive(Debug, PartialEq)]
struct RunAudit {
    insert: Result<Value, SessionError>,
    after_insert: CommitAudit,
    undone: Result<bool, SessionError>,
    after_undo: CommitAudit,
    redone: Result<bool, SessionError>,
    after_redo: CommitAudit,
}

fn prose_and_table_document() -> Value {
    let mut table = plain_table_document(TABLE_SIZE, TABLE_SIZE)["content"][0].clone();
    let (row, column) = EMPTY_CELL;
    table["content"][row]["content"][column]["content"] = json!([{"type": "paragraph"}]);
    json!({
        "type": "doc",
        "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": PROSE_TEXT}]},
            {"type": "paragraph"},
            table,
        ],
    })
}

fn cell_paragraph(cell: (usize, usize)) -> Vec<u32> {
    let (row, column) = cell;
    vec![
        PROSE_TABLE_INDEX,
        u32::try_from(row).expect("fixture rows fit u32"),
        u32::try_from(column).expect("fixture columns fit u32"),
        CELL_PARAGRAPH_INDEX,
    ]
}

fn textblock_edit_cases() -> Vec<TextblockEditCase> {
    let prose = prose_and_table_document;
    let cell_text_len =
        u32::try_from(fixture_cell_text(0, 0).chars().count()).expect("fixture cell text fits u32");
    let case = |name, document, block_path: &[u32], offset, route| TextblockEditCase {
        name,
        document,
        block_path: block_path.to_vec(),
        offset,
        route,
        intent: INSERT_INTENTS[0],
        selection_len: 0,
        replacement_text: INSERTED_TEXT,
        editing_limits: EditingLimits::default(),
    };
    vec![
        case(
            "prose mid-leaf",
            prose(),
            &PROSE_PATH,
            CaretOffset::At(MIDDLE_OFFSET),
            ExpectedRoute::Localized,
        ),
        case(
            "prose leaf end",
            prose(),
            &PROSE_PATH,
            CaretOffset::ContentEnd,
            ExpectedRoute::Localized,
        ),
        case(
            "prose leaf start",
            prose(),
            &PROSE_PATH,
            CaretOffset::At(LEAF_START),
            ExpectedRoute::Localized,
        ),
        case(
            "empty paragraph",
            prose(),
            &EMPTY_PARAGRAPH_PATH,
            CaretOffset::At(LEAF_START),
            ExpectedRoute::Localized,
        ),
        case(
            "cell mid-leaf",
            prose(),
            &cell_paragraph(TEXT_CELL),
            CaretOffset::At(MIDDLE_OFFSET),
            ExpectedRoute::Localized,
        ),
        case(
            "cell leaf end",
            prose(),
            &cell_paragraph(TEXT_CELL),
            CaretOffset::At(cell_text_len),
            ExpectedRoute::Localized,
        ),
        case(
            "cell leaf start",
            prose(),
            &cell_paragraph(TEXT_CELL),
            CaretOffset::At(LEAF_START),
            ExpectedRoute::Localized,
        ),
        case(
            "first keystroke into an empty cell",
            prose(),
            &cell_paragraph(EMPTY_CELL),
            CaretOffset::At(LEAF_START),
            ExpectedRoute::Localized,
        ),
        case(
            "second paragraph of a multi-paragraph cell",
            multi_paragraph_cell_document(),
            &MULTI_PARAGRAPH_SECOND_PARAGRAPH,
            CaretOffset::At(MIDDLE_OFFSET),
            ExpectedRoute::Localized,
        ),
        case(
            "text leaf end beside the inline atom",
            multi_paragraph_cell_document(),
            &MULTI_PARAGRAPH_ATOM_PARAGRAPH,
            CaretOffset::FirstLeafEnd,
            ExpectedRoute::Localized,
        ),
        case(
            "after the inline atom",
            multi_paragraph_cell_document(),
            &MULTI_PARAGRAPH_ATOM_PARAGRAPH,
            CaretOffset::ContentEnd,
            ExpectedRoute::Generic,
        ),
        case(
            "inside the nested table",
            multi_paragraph_cell_document(),
            &NESTED_TABLE_CELL_PARAGRAPH,
            CaretOffset::At(MIDDLE_OFFSET),
            ExpectedRoute::NestedTableRefusal,
        ),
    ]
}

fn caret_scalar(session: &EditorSession, block_path: &[u32], offset: CaretOffset) -> u32 {
    let document = session.engine.document().expect("the fixture is ready");
    let mut block = document.root();
    let mut position = 0u32;
    for &index in block_path {
        let index = usize::try_from(index).expect("fixture paths fit usize");
        let content = block
            .content()
            .expect("fixture path descends through elements");
        position += content
            .iter()
            .take(index)
            .map(|sibling| sibling.node_size())
            .sum::<u32>();
        block = content.child(index).expect("fixture path names a child");
        position += 1;
    }
    let offset = match offset {
        CaretOffset::At(offset) => offset,
        CaretOffset::FirstLeafEnd => block
            .child(0)
            .expect("fixture block has a first leaf")
            .node_size(),
        CaretOffset::ContentEnd => block
            .content()
            .expect("fixture path names a textblock")
            .size(),
    };
    session
        .engine
        .position_map()
        .expect("the fixture is ready")
        .doc_to_scalar(position + offset, document)
}

fn insert_request(session: &mut EditorSession, caret: u32) -> String {
    let epoch = session
        .pin_position_epoch(OWNER_ID, session.engine.revision())
        .expect("the fixture pins a position epoch");
    json!({
        "version": 1,
        "requestId": INSERT_REQUEST_ID.to_string(),
        "ownerId": OWNER_ID.to_string(),
        "positionEpoch": epoch.to_string(),
        "intent": {"type": "insertText", "anchor": caret, "head": caret, "text": INSERTED_TEXT},
    })
    .to_string()
}

fn commit_audit(session: &EditorSession) -> CommitAudit {
    CommitAudit {
        document_json: session
            .engine
            .document_json()
            .expect("the fixture is ready"),
        encoded_state: session.engine.encoded_state().expect("the fixture encodes"),
        document_revision: session.engine.revision(),
        state_revision: session.engine.state_revision(),
        selection: session.engine.resolved_selection().cloned(),
        stored_marks: session.engine.stored_marks().map(<[Mark]>::to_vec),
        can_undo: session.engine.can_undo(),
        can_redo: session.engine.can_redo(),
        outbox_updates: session
            .collaboration_outbox()
            .expect("the fixture attaches a collaboration runtime")
            .pending_document_update_count(),
        retained_history: session.engine.retained_history_for_test(),
    }
}

fn prepared_session(case: &TextblockEditCase, localized: bool) -> (EditorSession, String) {
    let mut session =
        session_with_document_and_editing_limits(&case.document, case.editing_limits.clone());
    session.attach_collaboration_runtime();
    if !localized {
        session.engine.drop_localized_text_index_for_test();
    }
    let caret = caret_scalar(&session, &case.block_path, case.offset);
    let request = insert_request(&mut session, caret);
    let mut request: Value = serde_json::from_str(&request).expect("request is JSON");
    request["intent"]["type"] = json!(case.intent);
    request["intent"]["head"] = json!(caret + case.selection_len);
    if INSERT_INTENTS.contains(&case.intent) {
        request["intent"]["text"] = json!(case.replacement_text);
    } else {
        request["intent"]
            .as_object_mut()
            .expect("intent is an object")
            .remove("text");
    }
    (session, request.to_string())
}

fn submit_insert(session: &mut EditorSession, request: &str) -> Result<Value, SessionError> {
    NativeTransactionBridge::new(session)
        .submit_native_intent(request)
        .map(|outcome| serde_json::from_str(&outcome).expect("native outcomes are JSON"))
}

fn run_case(case: &TextblockEditCase, localized: bool, intent: &str) -> (RunAudit, FullPassCounts) {
    let _clients = super::deterministic_clients::DeterministicClients::new();
    let (mut session, request) = prepared_session(case, localized);
    let mut request: Value = serde_json::from_str(&request).expect("request is JSON");
    request["intent"]["type"] = json!(intent);
    let request = request.to_string();
    reset_full_pass_counts_for_test();
    let insert = submit_insert(&mut session, &request);
    let passes = take_full_pass_counts_for_test();
    let after_insert = commit_audit(&session);
    let undone = NativeTransactionBridge::new(&mut session).undo(UNDO_REQUEST_ID);
    let after_undo = commit_audit(&session);
    let redone = NativeTransactionBridge::new(&mut session).redo(REDO_REQUEST_ID);
    let after_redo = commit_audit(&session);
    (
        RunAudit {
            insert,
            after_insert,
            undone,
            after_undo,
            redone,
            after_redo,
        },
        passes,
    )
}

fn assert_route(case: &TextblockEditCase, audit: &RunAudit, passes: &FullPassCounts) {
    let name = case.name;
    let document_wide = [
        ("canonical_projections", passes.canonical_projections),
        ("canonical_serializations", passes.canonical_serializations),
        ("canonical_hashes", passes.canonical_hashes),
        ("yrs_tree_walks", passes.yrs_tree_walks),
        ("document_validations", passes.document_validations),
        ("planner_simulations", passes.planner_simulations),
        (
            "ordinary_step_applications",
            passes.ordinary_step_applications,
        ),
    ];
    match case.route {
        ExpectedRoute::Localized => {
            assert!(audit.insert.is_ok(), "{name}: {:?}", audit.insert);
            for (kind, count) in document_wide {
                assert_eq!(
                    count, 0,
                    "{name}: the localized insert ran {kind}: {passes:#?}"
                );
            }
        }
        ExpectedRoute::Generic => {
            assert!(audit.insert.is_ok(), "{name}: {:?}", audit.insert);
            assert!(
                passes.planner_simulations > 0,
                "{name}: a refused insert must take the generic planner: {passes:#?}"
            );
        }
        ExpectedRoute::CellBoundaryRefusal => {
            let error = audit
                .insert
                .as_ref()
                .expect_err("cell boundary joins remain refused");
            assert_eq!(error.code, "OPERATION_INVALID", "{name}: {error:?}");
            assert_eq!(
                error
                    .details
                    .as_ref()
                    .and_then(|details| details.get("field")),
                Some(&json!("tableCellBoundary")),
                "{name}: {error:?}"
            );
            assert!(
                !audit.after_insert.can_undo,
                "{name}: refusal must not create history"
            );
        }
        ExpectedRoute::NestedTableRefusal => {
            let error = audit
                .insert
                .as_ref()
                .expect_err("an insert into a nested table is refused");
            assert_eq!(error.code, "OPERATION_INVALID", "{name}: {error:?}");
        }
    }
}

#[test]
fn textblock_local_inserts_commit_exactly_like_the_generic_path() {
    for intent in INSERT_INTENTS {
        for case in textblock_edit_cases() {
            let (localized, localized_passes) = run_case(&case, true, intent);
            let (generic, _) = run_case(&case, false, intent);
            eprintln!(
                "{intent} {}: route {:?}, localized passes {localized_passes:?}",
                case.name, case.route
            );
            assert_route(&case, &localized, &localized_passes);
            assert_eq!(
                localized, generic,
                "{intent} {}: the localized and generic runs diverged",
                case.name
            );
        }
    }
}

fn textblock_range_cases() -> Vec<TextblockEditCase> {
    let case = |name, document, block_path: Vec<u32>, offset, selection_len, intent, route| {
        TextblockEditCase {
            name,
            document,
            block_path,
            offset,
            selection_len,
            intent,
            route,
            replacement_text: INSERTED_TEXT,
            editing_limits: EditingLimits::default(),
        }
    };
    let mut two_leaves = prose_and_table_document();
    two_leaves["content"][0]["content"] = json!([
        {"type": "text", "text": "ab", "marks": [{"type": "bold"}]},
        {"type": "text", "text": "cd"}
    ]);
    let unicode = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": "a😀", "marks": [{"type": "bold"}]},
        {"type": "text", "text": "λcd"}
    ]}]});
    let mut expanding = case(
        "growing non-BMP replacement",
        prose_and_table_document(),
        cell_paragraph(TEXT_CELL),
        CaretOffset::At(MIDDLE_OFFSET),
        1,
        "replaceSelectionText",
        ExpectedRoute::Localized,
    );
    expanding.replacement_text = "😀xy";
    vec![
        expanding,
        case(
            "delete non-BMP text across marked leaves",
            unicode.clone(),
            PROSE_PATH.to_vec(),
            CaretOffset::At(1),
            2,
            "deleteRange",
            ExpectedRoute::Localized,
        ),
        case(
            "replace non-BMP text across marked leaves",
            unicode,
            PROSE_PATH.to_vec(),
            CaretOffset::At(1),
            2,
            "replaceSelectionText",
            ExpectedRoute::Localized,
        ),
        case(
            "IME replaces two characters",
            prose_and_table_document(),
            cell_paragraph(TEXT_CELL),
            CaretOffset::At(MIDDLE_OFFSET),
            2,
            "replaceSelectionText",
            ExpectedRoute::Localized,
        ),
        case(
            "backspace within a leaf",
            prose_and_table_document(),
            cell_paragraph(TEXT_CELL),
            CaretOffset::At(MIDDLE_OFFSET),
            0,
            "deleteBackward",
            ExpectedRoute::Localized,
        ),
        case(
            "forward delete within a leaf",
            prose_and_table_document(),
            cell_paragraph(TEXT_CELL),
            CaretOffset::At(MIDDLE_OFFSET),
            0,
            "deleteForward",
            ExpectedRoute::Localized,
        ),
        case(
            "delete across adjacent differently marked leaves",
            two_leaves,
            PROSE_PATH.to_vec(),
            CaretOffset::At(1),
            2,
            "deleteRange",
            ExpectedRoute::Localized,
        ),
        case(
            "delete a cell range",
            prose_and_table_document(),
            cell_paragraph(TEXT_CELL),
            CaretOffset::At(MIDDLE_OFFSET),
            2,
            "deleteRange",
            ExpectedRoute::Localized,
        ),
        case(
            "backspace at a cell start",
            prose_and_table_document(),
            cell_paragraph(TEXT_CELL),
            CaretOffset::At(LEAF_START),
            0,
            "deleteBackward",
            ExpectedRoute::CellBoundaryRefusal,
        ),
        case(
            "delete text before an inline atom",
            multi_paragraph_cell_document(),
            MULTI_PARAGRAPH_ATOM_PARAGRAPH.to_vec(),
            CaretOffset::At(MIDDLE_OFFSET),
            2,
            "deleteRange",
            ExpectedRoute::Localized,
        ),
        case(
            "delete covering an inline atom",
            multi_paragraph_cell_document(),
            MULTI_PARAGRAPH_ATOM_PARAGRAPH.to_vec(),
            CaretOffset::FirstLeafEnd,
            1,
            "deleteRange",
            ExpectedRoute::Generic,
        ),
        case(
            "delete inside a nested table",
            multi_paragraph_cell_document(),
            NESTED_TABLE_CELL_PARAGRAPH.to_vec(),
            CaretOffset::At(MIDDLE_OFFSET),
            2,
            "deleteRange",
            ExpectedRoute::NestedTableRefusal,
        ),
        case(
            "same-length text replacement",
            prose_and_table_document(),
            cell_paragraph(TEXT_CELL),
            CaretOffset::At(MIDDLE_OFFSET),
            1,
            "replaceSelectionText",
            ExpectedRoute::Localized,
        ),
        case(
            "delete deepest text from the only paragraph",
            json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "a"}]}]}),
            PROSE_PATH.to_vec(),
            CaretOffset::At(LEAF_START),
            1,
            "deleteRange",
            ExpectedRoute::Localized,
        ),
        case(
            "delete all text leaves an empty cell",
            prose_and_table_document(),
            cell_paragraph(TEXT_CELL),
            CaretOffset::At(LEAF_START),
            u32::try_from(fixture_cell_text(0, 0).len()).unwrap(),
            "deleteRange",
            ExpectedRoute::Localized,
        ),
    ]
}

#[test]
fn textblock_local_ranges_commit_exactly_like_the_generic_path() {
    for case in textblock_range_cases() {
        let (localized, passes) = run_case(&case, true, case.intent);
        let (generic, _) = run_case(&case, false, case.intent);
        assert_eq!(
            localized, generic,
            "{}: local and generic range edits diverged",
            case.name
        );
        assert_route(&case, &localized, &passes);
    }
}

#[test]
fn textblock_local_replacement_preserves_aggregate_output_budget_rejections() {
    const OUTPUT_BUDGETS: [usize; 3] = [1024, 2048, 4096];
    const PROSE_SCALARS: usize = 512;
    let mut rejected = 0;
    let mut accepted = 0;
    for budget in OUTPUT_BUDGETS {
        let mut case = TextblockEditCase {
            name: "replacement at output budget",
            document: json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "a".repeat(PROSE_SCALARS)}]}]}),
            block_path: PROSE_PATH.to_vec(),
            offset: CaretOffset::At(MIDDLE_OFFSET),
            selection_len: 1,
            intent: "replaceSelectionText",
            replacement_text: "😀xy",
            editing_limits: EditingLimits::default(),
            route: ExpectedRoute::Localized,
        };
        case.editing_limits.max_derived_output_bytes = budget;
        let (local, _) = run_case(&case, true, case.intent);
        let (generic, _) = run_case(&case, false, case.intent);
        assert_eq!(
            local, generic,
            "replacement diverged at output budget {budget}"
        );
        if local.insert.is_ok() {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    assert!(
        rejected > 0 && accepted > 0,
        "budgets must exercise rejection and acceptance: rejected {rejected}, accepted {accepted}"
    );
}

fn table_records(cache: &CachedRenderBlocks) -> Vec<TableRenderRecord> {
    let mut records = Vec::new();
    cache.visit_table_records(&mut records);
    records
        .into_iter()
        .map(|(_, table)| table.clone())
        .collect()
}

#[test]
fn localized_render_transitions_equal_a_fresh_full_render() {
    for case in textblock_edit_cases()
        .into_iter()
        .chain(textblock_range_cases())
    {
        let (mut session, request) = prepared_session(&case, true);
        let inserted = submit_insert(&mut session, &request);
        if matches!(
            case.route,
            ExpectedRoute::NestedTableRefusal | ExpectedRoute::CellBoundaryRefusal
        ) {
            assert!(inserted.is_err(), "{}: {inserted:?}", case.name);
        } else {
            assert!(inserted.is_ok(), "{}: {inserted:?}", case.name);
        }
        assert_render_matches_fresh(&session, case.name);
        if inserted.is_ok() && session.engine.can_undo() {
            NativeTransactionBridge::new(&mut session)
                .undo(UNDO_REQUEST_ID)
                .unwrap();
            assert_render_matches_fresh(&session, &format!("{} undo", case.name));
            NativeTransactionBridge::new(&mut session)
                .redo(REDO_REQUEST_ID)
                .unwrap();
            assert_render_matches_fresh(&session, &format!("{} redo", case.name));
        }
    }
    use crate::tables::commands::{TableCommand, TableEdge};
    use crate::yrs_engine::{InitializationMode, TypedCommand, YrsDocumentEngine, YrsEngineConfig};
    const FIRST_REQUEST: u64 = 20;
    const REQUESTS_PER_COMMAND: u64 = 4;
    let mut session = session_with_document(&plain_table_document(TABLE_SIZE, TABLE_SIZE));
    let mut replica = YrsDocumentEngine::new(YrsEngineConfig {
        schema: session.engine.schema().clone(),
        fragment_name: "prosemirror".into(),
        initialization_mode: InitializationMode::AwaitRemote,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap();
    replica
        .apply_remote_update_v1(FIRST_REQUEST, &session.engine.encoded_state().unwrap())
        .unwrap();
    for (index, command) in [
        TableCommand::AddTableRow {
            side: TableEdge::Before,
        },
        TableCommand::AddTableColumn {
            side: TableEdge::Before,
        },
        TableCommand::DeleteTableRows,
        TableCommand::DeleteTableColumns,
    ]
    .into_iter()
    .enumerate()
    {
        let request = FIRST_REQUEST + REQUESTS_PER_COMMAND * (index as u64 + 1);
        let name = format!("structural {command:?}");
        assert!(
            session
                .engine
                .apply_command(request, TypedCommand::Table(command))
                .unwrap()
                .is_some(),
            "{name} applies"
        );
        assert_render_matches_fresh(&session, &name);
        session.engine.undo(request + 1).unwrap();
        assert_render_matches_fresh(&session, &format!("{name} undo"));
        session.engine.redo(request + 2).unwrap();
        assert_render_matches_fresh(&session, &format!("{name} redo"));
        replica
            .apply_remote_update_v1(request, &session.engine.encoded_state().unwrap())
            .unwrap();
        let cache = replica.cached_render_blocks().unwrap();
        let fresh = CachedRenderBlocks::build(
            replica.document().unwrap(),
            replica.schema(),
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(
            cache.materialize(),
            fresh.materialize(),
            "{name} remote render"
        );
        assert_eq!(
            cache.table_projection_index, fresh.table_projection_index,
            "{name} remote projection"
        );
        assert_eq!(
            cache.table_attributes, fresh.table_attributes,
            "{name} remote attributes"
        );
        assert_eq!(
            session.engine.document_json(),
            replica.document_json(),
            "{name} remote document converges"
        );
    }
}

fn assert_render_matches_fresh(session: &EditorSession, name: &str) {
    let transitioned = session
        .engine
        .cached_render_blocks()
        .expect("the fixture is ready");
    let fresh = CachedRenderBlocks::build(
        session.engine.document().expect("the fixture is ready"),
        session.engine.schema(),
        &ResourceLimits::default(),
    )
    .expect("the committed document renders");
    eprintln!(
        "{}: {} blocks, {} table records, {} pooled attributes",
        name,
        fresh.materialize().len(),
        table_records(&fresh).len(),
        fresh.table_attributes.len()
    );
    assert_eq!(
        transitioned.materialize(),
        fresh.materialize(),
        "{}: render elements",
        name
    );
    assert_eq!(
        table_records(&transitioned),
        table_records(&fresh),
        "{}: table records",
        name
    );
    assert_eq!(
        transitioned.table_attributes, fresh.table_attributes,
        "{}: attribute pool",
        name
    );
    assert_eq!(
        transitioned.table_projection_index, fresh.table_projection_index,
        "{name}: table projection"
    );
}

#[test]
fn a_native_insert_in_a_large_table_is_validated_locally() {
    assert_large_table_edit_is_validated_locally(INSERT_INTENTS[0]);
}

#[test]
fn a_native_delete_in_a_large_table_is_validated_locally() {
    assert_large_table_edit_is_validated_locally("deleteBackward");
}

fn assert_large_table_edit_is_validated_locally(intent: &str) {
    let mut session =
        session_with_document(&plain_table_document(LARGE_TABLE_ROWS, LARGE_TABLE_COLUMNS));
    let cell = keystroke_cell(LARGE_TABLE_ROWS, LARGE_TABLE_COLUMNS);
    let block_path = [
        0,
        u32::try_from(cell / LARGE_TABLE_COLUMNS).expect("rows fit u32"),
        u32::try_from(cell % LARGE_TABLE_COLUMNS).expect("columns fit u32"),
        CELL_PARAGRAPH_INDEX,
    ];
    let caret = caret_scalar(&session, &block_path, CaretOffset::At(MIDDLE_OFFSET));
    let point = crate::yrs_engine::RevisionedPosition {
        offset: caret,
        kind: crate::yrs_engine::EditorOffsetKind::Scalar,
        affinity: crate::yrs_engine::Affinity::After,
    };
    session
        .engine
        .apply_typed_transaction_with_result(crate::yrs_engine::TypedTransaction {
            request_id: INSERT_REQUEST_ID - 1,
            base_document_revision: session.engine.revision(),
            origin: crate::yrs_engine::TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: crate::yrs_engine::SelectionIntent::Set(
                crate::yrs_engine::SelectionInput::Text {
                    anchor: point,
                    head: point,
                },
            ),
            history_policy: crate::yrs_engine::HistoryPolicy::Skip,
        })
        .unwrap();
    session
        .engine
        .active_state()
        .expect("the displayed caret has current command availability");
    let request = insert_request(&mut session, caret);
    let mut request: Value = serde_json::from_str(&request).expect("request is JSON");
    request["intent"]["type"] = json!(intent);
    if intent != INSERT_INTENTS[0] {
        request["intent"].as_object_mut().unwrap().remove("text");
    }
    let request = request.to_string();

    reset_full_pass_counts_for_test();
    submit_insert(&mut session, &request).expect("the keystroke applies");
    let passes = take_full_pass_counts_for_test();
    eprintln!("1000x20 native keystroke: {passes:#?}");

    let (row, column) = (cell / LARGE_TABLE_COLUMNS, cell % LARGE_TABLE_COLUMNS);
    let text = fixture_cell_text(row, column);
    let middle = usize::try_from(MIDDLE_OFFSET).expect("offsets fit usize");
    assert_eq!(
        session
            .engine
            .document_json()
            .expect("the fixture is ready")["content"][0]["content"][row]["content"][column]
            ["content"][0]["content"][0]["text"],
        if intent == INSERT_INTENTS[0] {
            format!("{}{INSERTED_TEXT}{}", &text[..middle], &text[middle..])
        } else {
            format!("{}{}", &text[..middle - 1], &text[middle..])
        },
        "the keystroke lands in the keystroke cell"
    );
    assert!(
        passes.canonical_identity_predicate_nodes_visited <= IDENTITY_PREDICATE_VISIT_CEILING,
        "canonical identity walked {} nodes",
        passes.canonical_identity_predicate_nodes_visited,
    );
    assert_eq!(
        (
            passes.canonical_projections,
            passes.canonical_serializations,
            passes.canonical_hashes
        ),
        (1, 1, 1),
        "an over-budget history snapshot retains one eager digest",
    );
    for (kind, count) in [
        ("yrs_tree_walks", passes.yrs_tree_walks),
        (
            "table_projection_derivations",
            passes.table_projection_derivations,
        ),
        (
            "table_command_availability_plans",
            passes.table_command_availability_plans,
        ),
        (
            "active_applicability_passes",
            passes.active_applicability_passes,
        ),
        ("document_validations", passes.document_validations),
        ("planner_simulations", passes.planner_simulations),
        (
            "rendered_text_derivations",
            passes.rendered_text_derivations,
        ),
        ("raw_document_text_scans", passes.raw_document_text_scans),
        (
            "validation_certificate_constructions",
            passes.validation_certificate_constructions,
        ),
        (
            "validated_evidence_constructions",
            passes.validated_evidence_constructions,
        ),
    ] {
        assert_eq!(count, 0, "a single-textblock keystroke ran {kind}");
    }
}

fn storage_reuse_audit() -> (
    FullPassCounts,
    Vec<TableRenderRecord>,
    Vec<TableRenderRecord>,
) {
    const ROWS: usize = 50;
    const COLUMNS: usize = 4;
    let mut session = session_with_document(&plain_table_document(ROWS, COLUMNS));
    let before = table_records(&session.engine.cached_render_blocks().unwrap());
    let target = keystroke_cell(ROWS, COLUMNS);
    let path = [0, (target / COLUMNS) as u32, (target % COLUMNS) as u32, 0];
    let caret = caret_scalar(&session, &path, CaretOffset::At(MIDDLE_OFFSET));
    let request = insert_request(&mut session, caret);
    reset_full_pass_counts_for_test();
    submit_insert(&mut session, &request).expect("the keystroke applies");
    let counts = take_full_pass_counts_for_test();
    let after = table_records(&session.engine.cached_render_blocks().unwrap());
    for (index, (old, new)) in before[0].cells.iter().zip(&after[0].cells).enumerate() {
        if index == target {
            assert_ne!(
                old.content_key, new.content_key,
                "edited cell gets new content"
            );
        } else {
            assert_eq!(
                old.content_key, new.content_key,
                "cell {index} retains content"
            );
        }
    }
    (counts, before, after)
}

#[test]
fn storage_identical_cells_reuse_their_render_entry_without_hashing() {
    let (counts, before, after) = storage_reuse_audit();
    assert_eq!(
        counts.cell_content_keys, 1,
        "one key for the edited cell: {counts:#?}"
    );
    assert_eq!(
        counts.attribute_serializations, 0,
        "no attributes changed: {counts:#?}"
    );
    for (index, (old, new)) in before[0].cells.iter().zip(&after[0].cells).enumerate() {
        if old.content_key == new.content_key {
            assert!(
                std::sync::Arc::ptr_eq(old, new),
                "cell {index} reuses its whole entry"
            );
        }
    }
}

#[test]
fn a_textblock_edit_carries_the_table_projection() {
    let case = textblock_edit_cases()
        .into_iter()
        .find(|case| case.name == "cell mid-leaf")
        .unwrap();
    let (mut session, request) = prepared_session(&case, true);
    let old = session.engine.document().unwrap().clone();
    let cache = session.engine.cached_render_blocks().unwrap();
    submit_insert(&mut session, &request).unwrap();
    let new = session.engine.document().unwrap();
    reset_full_pass_counts_for_test();
    let carried = cache
        .transition_localized_textblock(
            &old,
            new,
            session.engine.schema(),
            PROSE_TABLE_INDEX as usize,
            INSERTED_TEXT.chars().count() as i32,
            &ResourceLimits::default(),
        )
        .unwrap();
    let counts = take_full_pass_counts_for_test();
    assert_eq!(
        counts.table_projection_derivations, 0,
        "text does not change table geometry: {counts:#?}"
    );
    let fresh = CachedRenderBlocks::build(new, session.engine.schema(), &ResourceLimits::default())
        .unwrap();
    assert_eq!(
        carried.cache.table_projection_index,
        fresh.table_projection_index
    );
    assert_eq!(carried.cache.materialize(), fresh.materialize());
}

#[test]
fn the_first_keystroke_into_an_empty_cell_replaces_its_branch_entry() {
    let case = textblock_edit_cases()
        .into_iter()
        .find(|case| case.name == "first keystroke into an empty cell")
        .expect("empty cell case");
    assert_next_keystroke_stays_indexed(&case, true);
}

#[test]
fn range_edits_keep_the_next_keystroke_indexed() {
    for case in textblock_range_cases()
        .into_iter()
        .filter(|case| case.route == ExpectedRoute::Localized && case.block_path.len() > 1)
    {
        assert_next_keystroke_stays_indexed(&case, false);
    }
}

fn assert_next_keystroke_stays_indexed(case: &TextblockEditCase, creates_branch: bool) {
    let (mut session, request) = prepared_session(case, true);
    let block_index = (0..session.engine.position_map().unwrap().block_count())
        .find(|index| {
            session
                .engine
                .position_map()
                .unwrap()
                .block(*index)
                .unwrap()
                .node_path
                .as_slice()
                == case.block_path
        })
        .unwrap();
    let before = session
        .engine
        .block_branch_index_for_test()
        .unwrap()
        .block_branches(block_index)
        .unwrap()
        .texts
        .clone();
    for keystroke in 0..2 {
        let mut request: Value = serde_json::from_str(&request).unwrap();
        request["requestId"] = json!((INSERT_REQUEST_ID + keystroke).to_string());
        if keystroke != 0 {
            let ResolvedSelection::Text { head, .. } = session.engine.resolved_selection().unwrap()
            else {
                panic!("{} leaves a text caret", case.name);
            };
            let caret = head.scalar;
            request = serde_json::from_str(&insert_request(&mut session, caret)).unwrap();
            request["requestId"] = json!((INSERT_REQUEST_ID + keystroke).to_string());
        }
        crate::yrs_engine::reset_import_lookup_event_count_for_test();
        reset_full_pass_counts_for_test();
        submit_insert(&mut session, &request.to_string()).unwrap();
        let counts = take_full_pass_counts_for_test();
        let lookup_events = crate::yrs_engine::take_import_lookup_event_count_for_test();
        assert!(
            session.engine.mutation_lookup_matches_fresh_for_test(),
            "{} keystroke {keystroke}: retained lookup payload matches a fresh full build",
            case.name
        );
        const LOCAL_LOOKUP_EVENT_CEILING: usize = 8;
        assert!(
            lookup_events <= LOCAL_LOOKUP_EVENT_CEILING,
            "keystroke {keystroke}: full lookup hydration visited {lookup_events} nodes"
        );
        assert_eq!(
            counts.yrs_tree_walks, 0,
            "keystroke {keystroke}: {counts:#?}"
        );
        assert_eq!(
            counts.document_validations, 0,
            "keystroke {keystroke} remains local"
        );
        assert_eq!(
            counts.cell_content_keys, 1,
            "keystroke {keystroke} reuses other cells"
        );
        let after = session.engine.block_branch_index_for_test().unwrap();
        if keystroke != 0 || creates_branch {
            assert!(!after.block_branches(block_index).unwrap().texts.is_empty());
        }
        if keystroke == 0 && creates_branch {
            assert_ne!(
                before,
                after.block_branches(block_index).unwrap().texts,
                "new XmlText branch is indexed"
            );
        }
        assert_render_matches_fresh(&session, &format!("{} keystroke {keystroke}", case.name));
    }
}
