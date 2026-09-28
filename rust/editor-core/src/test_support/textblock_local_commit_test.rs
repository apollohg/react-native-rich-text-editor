use serde_json::{json, Value};
use yrs::GetString;

use super::large_table_fixture::{
    fixture_cell_text, keystroke_cell, multi_paragraph_cell_document, plain_table_document,
    session_with_document,
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
use crate::yrs_engine::ResolvedSelection;

const OWNER_ID: u64 = 9;
const INSERT_REQUEST_ID: u64 = 5;
const UNDO_REQUEST_ID: u64 = 6;
const REDO_REQUEST_ID: u64 = 7;
const INSERTED_TEXT: &str = "x";
const PROSE_TEXT: &str = "Prose";
const TABLE_SIZE: usize = 3;
const EMPTY_CELL: (usize, usize) = (1, 1);
const TEXT_CELL: (usize, usize) = (0, 1);
const MIDDLE_OFFSET: u32 = 3;
const LEAF_START: u32 = 0;
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
    EagerLowering,
    Generic,
    NestedTableRefusal,
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
}

#[derive(Debug, PartialEq)]
struct YrsStructure {
    fragment: String,
    sorted_clocks: Vec<u32>,
}

#[derive(Debug, PartialEq)]
struct CommitAudit {
    document_json: Value,
    encoded_structure: YrsStructure,
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
            ExpectedRoute::EagerLowering,
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

fn yrs_structure(session: &EditorSession) -> YrsStructure {
    session
        .engine
        .read_fragment_for_test(|txn, fragment| {
            let mut sorted_clocks: Vec<u32> = yrs::ReadTxn::state_vector(txn)
                .iter()
                .map(|(_, clock)| *clock)
                .collect();
            sorted_clocks.sort_unstable();
            YrsStructure {
                fragment: fragment.get_string(txn),
                sorted_clocks,
            }
        })
        .expect("the fixture has a fragment")
}

fn commit_audit(session: &EditorSession) -> CommitAudit {
    CommitAudit {
        document_json: session
            .engine
            .document_json()
            .expect("the fixture is ready"),
        encoded_structure: yrs_structure(session),
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
    let mut session = session_with_document(&case.document);
    session.attach_collaboration_runtime();
    if !localized {
        session.engine.drop_localized_text_index_for_test();
    }
    let caret = caret_scalar(&session, &case.block_path, case.offset);
    let request = insert_request(&mut session, caret);
    (session, request)
}

fn submit_insert(session: &mut EditorSession, request: &str) -> Result<Value, SessionError> {
    NativeTransactionBridge::new(session)
        .submit_native_intent(request)
        .map(|outcome| serde_json::from_str(&outcome).expect("native outcomes are JSON"))
}

fn run_case(case: &TextblockEditCase, localized: bool) -> (RunAudit, FullPassCounts) {
    let (mut session, request) = prepared_session(case, localized);
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
        ExpectedRoute::EagerLowering => {
            assert!(audit.insert.is_ok(), "{name}: {:?}", audit.insert);
            assert_eq!(
                passes.planner_simulations, 0,
                "{name}: an admitted insert skips the planner: {passes:#?}"
            );
            assert!(
                passes.document_validations > 0,
                "{name}: a block holding an inline atom lowers and validates generically: {passes:#?}"
            );
        }
        ExpectedRoute::Generic => {
            assert!(audit.insert.is_ok(), "{name}: {:?}", audit.insert);
            assert!(
                passes.planner_simulations > 0,
                "{name}: a refused insert must take the generic planner: {passes:#?}"
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
    for case in textblock_edit_cases() {
        let (localized, localized_passes) = run_case(&case, true);
        let (generic, _) = run_case(&case, false);
        eprintln!(
            "{}: route {:?}, localized passes {localized_passes:?}",
            case.name, case.route
        );
        assert_route(&case, &localized, &localized_passes);
        assert_eq!(
            localized, generic,
            "{}: the localized and generic runs diverged",
            case.name
        );
    }
}

fn table_records(cache: &CachedRenderBlocks) -> Vec<TableRenderRecord> {
    let mut records = Vec::new();
    cache.visit_table_records(&mut records);
    records.into_iter().cloned().collect()
}

#[test]
fn localized_render_transitions_equal_a_fresh_full_render() {
    for case in textblock_edit_cases() {
        let (mut session, request) = prepared_session(&case, true);
        let inserted = submit_insert(&mut session, &request);
        if case.route == ExpectedRoute::NestedTableRefusal {
            assert!(inserted.is_err(), "{}: {inserted:?}", case.name);
        } else {
            assert!(inserted.is_ok(), "{}: {inserted:?}", case.name);
        }
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
            case.name,
            fresh.materialize().len(),
            table_records(&fresh).len(),
            fresh.table_attributes.len()
        );
        assert_eq!(
            transitioned.materialize(),
            fresh.materialize(),
            "{}: render elements",
            case.name
        );
        assert_eq!(
            table_records(&transitioned),
            table_records(&fresh),
            "{}: table records",
            case.name
        );
        assert_eq!(
            transitioned.table_attributes, fresh.table_attributes,
            "{}: attribute pool",
            case.name
        );
    }
}

#[test]
fn a_native_insert_in_a_large_table_is_validated_locally() {
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
    let request = insert_request(&mut session, caret);

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
        format!("{}{INSERTED_TEXT}{}", &text[..middle], &text[middle..]),
        "the keystroke lands in the keystroke cell"
    );
    for (kind, count) in [
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
