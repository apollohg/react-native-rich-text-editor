use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::command_planner::SemanticOperation;
use crate::model::Document;
use crate::schema::Schema;
use crate::serialize::json_in::{from_prosemirror_json, UnknownTypeMode};
use crate::tables::normalize::{normalize_outer_table, outer_table_grid};
use crate::tables::projection::{project_table, ProjectedTable, TableGridBudget};
use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};

const TABLE_NODE: &str = "table";
const ROW_NODE: &str = "table_row";
const CELL_NODE: &str = "table_cell";
const HEADER_CELL_NODE: &str = "table_header";
const PARAGRAPH_NODE: &str = "paragraph";
const TABLE_POSITION: u32 = 0;
const FIRST_CELL_POSITION: u32 = 2;
const GENEROUS_GRID_LIMIT: usize = 4_096;
const SINGLE_SPAN: u32 = 1;

pub(crate) fn schema() -> Schema {
    tabled_schema(PROSEMIRROR_TABLE_NAMES)
}

pub(crate) fn cell_with(colspan: u32, rowspan: u32, colwidth: Value, text: &str) -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": colspan, "rowspan": rowspan, "colwidth": colwidth },
        "content": [{ "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": text }] }],
    })
}

pub(crate) fn cell(text: &str) -> Value {
    cell_with(SINGLE_SPAN, SINGLE_SPAN, Value::Null, text)
}

pub(crate) fn header_cell(text: &str) -> Value {
    json!({
        "type": HEADER_CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [{ "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": text }] }],
    })
}

pub(crate) fn row(cells: Vec<Value>) -> Value {
    json!({ "type": ROW_NODE, "content": cells })
}

pub(crate) fn table(rows: Vec<Value>) -> Value {
    json!({ "type": TABLE_NODE, "content": rows })
}

pub(crate) fn document_with(content: Vec<Value>) -> Document {
    from_prosemirror_json(
        &json!({ "type": "doc", "content": content }),
        &schema(),
        UnknownTypeMode::Preserve,
    )
    .expect("the fixture document parses")
}

fn limits() -> ResourceLimits {
    ResourceLimits {
        max_table_grid_slots: GENEROUS_GRID_LIMIT,
        ..ResourceLimits::default()
    }
}

fn projection_of(document: &Document, table_pos: u32) -> ProjectedTable {
    let node = document
        .node_at(&table_path(document, table_pos))
        .expect("the fixture holds a table")
        .clone();
    project_table(
        &node,
        table_pos,
        &schema(),
        &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
    )
    .expect("the fixture projects")
}

fn table_path(document: &Document, table_pos: u32) -> Vec<u32> {
    let mut position = 0u32;
    for (index, child) in document
        .root()
        .content()
        .expect("the document has content")
        .iter()
        .enumerate()
    {
        if position == table_pos {
            return vec![u32::try_from(index).expect("fixture indices fit")];
        }
        position += child.node_size();
    }
    panic!("no top level node begins at {table_pos}");
}

fn normalize(document: &Document) -> Vec<SemanticOperation> {
    normalize_outer_table(document, TABLE_POSITION, &schema(), &limits())
        .expect("the fixture normalizes")
}

fn normalized(document: &Document) -> Document {
    let operations = normalize(document);
    crate::command_planner::apply_operations(document, &schema(), &operations)
        .expect("the normalization plan applies")
}

fn inserted_cell_positions(operations: &[SemanticOperation]) -> Vec<u32> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            SemanticOperation::ReplaceRange { from, to, .. } => {
                assert_eq!(
                    from, to,
                    "normalization must insert cells without replacing any existing child",
                );
                Some(*from)
            }
            SemanticOperation::UpdateNodeAttrs { .. } => None,
            other => panic!("normalization emitted an unexpected operation {other:?}"),
        })
        .collect()
}

fn attribute_targets(operations: &[SemanticOperation]) -> Vec<u32> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            SemanticOperation::UpdateNodeAttrs { pos, .. } => Some(*pos),
            _ => None,
        })
        .collect()
}

#[test]
fn a_valid_outer_grid_plans_no_normalization() {
    let document = document_with(vec![table(vec![
        row(vec![cell("a"), cell("b")]),
        row(vec![cell("c"), cell("d")]),
    ])]);

    assert!(!projection_of(&document, TABLE_POSITION).irregular);
    assert_eq!(normalize(&document), Vec::new());
}

#[test]
fn agreeing_widths_plan_no_normalization() {
    let document = document_with(vec![table(vec![
        row(vec![
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([100]), "a"),
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "b"),
        ]),
        row(vec![
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([100]), "c"),
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "d"),
        ]),
    ])]);

    assert_eq!(normalize(&document), Vec::new());
}

#[test]
fn a_short_row_gains_its_missing_cell_without_replacing_existing_cells() {
    let document = document_with(vec![table(vec![
        row(vec![cell("a"), cell("b")]),
        row(vec![cell("c")]),
    ])]);
    let before = projection_of(&document, TABLE_POSITION);
    assert!(before.irregular);
    assert_eq!(before.slots.iter().filter(|slot| slot.is_none()).count(), 1);

    let operations = normalize(&document);
    assert_eq!(operations.len(), 1);
    assert_eq!(attribute_targets(&operations), Vec::<u32>::new());
    assert_eq!(inserted_cell_positions(&operations).len(), 1);

    let after = projection_of(&normalized(&document), TABLE_POSITION);
    assert!(!after.irregular);
    assert_eq!((after.rows, after.columns), (before.rows, before.columns));
}

#[test]
fn an_overlong_rowspan_is_clamped_by_an_attribute_write_only() {
    let document = document_with(vec![table(vec![row(vec![
        cell_with(SINGLE_SPAN, 4, Value::Null, "tall"),
        cell("b"),
    ])])]);

    let operations = normalize(&document);
    assert_eq!(
        operations,
        vec![SemanticOperation::UpdateNodeAttrs {
            pos: FIRST_CELL_POSITION,
            attrs: std::collections::HashMap::from([
                ("colspan".to_string(), json!(SINGLE_SPAN)),
                ("rowspan".to_string(), json!(SINGLE_SPAN)),
                ("colwidth".to_string(), Value::Null),
            ]),
        }]
    );
    assert!(!projection_of(&normalized(&document), TABLE_POSITION).irregular);
}

#[test]
fn a_width_disagreement_rewrites_only_the_disagreeing_cell() {
    let document = document_with(vec![table(vec![
        row(vec![cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([100]), "a")]),
        row(vec![cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "b")]),
    ])]);

    let operations = normalize(&document);
    assert_eq!(attribute_targets(&operations), vec![FIRST_CELL_POSITION]);
    assert_eq!(inserted_cell_positions(&operations), Vec::<u32>::new());

    let SemanticOperation::UpdateNodeAttrs { attrs, .. } =
        operations.first().expect("one width fix")
    else {
        panic!("a width fix is an attribute write");
    };
    assert_eq!(attrs.get("colwidth"), Some(&json!([140])));
}

#[test]
fn a_row_free_table_plans_no_normalization_and_stays_irregular() {
    let document = document_with(vec![json!({ "type": TABLE_NODE, "content": [] })]);

    assert_eq!(normalize(&document), Vec::new());
    assert!(projection_of(&document, TABLE_POSITION).irregular);
}

#[test]
fn a_cell_free_table_plans_no_normalization_and_stays_irregular() {
    let document = document_with(vec![table(vec![row(Vec::new()), row(Vec::new())])]);

    assert_eq!(normalize(&document), Vec::new());
    assert!(projection_of(&document, TABLE_POSITION).irregular);
}

#[test]
fn a_missing_first_row_cell_is_inserted_at_the_reference_side() {
    let document = document_with(vec![table(vec![
        row(vec![cell("a")]),
        row(vec![cell("b"), cell("c")]),
    ])]);

    let operations = normalize(&document);
    assert_eq!(
        inserted_cell_positions(&operations),
        vec![FIRST_CELL_POSITION]
    );
    assert!(!projection_of(&normalized(&document), TABLE_POSITION).irregular);
}

#[test]
fn an_inserted_cell_copies_the_rows_own_cell_role() {
    let document = document_with(vec![table(vec![
        row(vec![header_cell("a"), header_cell("b")]),
        row(vec![header_cell("c")]),
    ])]);

    let normalized = normalized(&document);
    let table_node = normalized
        .node_at(&table_path(&normalized, TABLE_POSITION))
        .expect("the normalized table exists");
    let second_row = table_node.child(1).expect("the short row survives");
    let inserted = second_row.child(1).expect("the filler cell exists");
    assert_eq!(inserted.node_type(), HEADER_CELL_NODE);
}

#[test]
fn normalization_never_targets_a_nested_descendant() {
    let nested = table(vec![
        row(vec![cell("n1"), cell("n2")]),
        row(vec![cell("n3")]),
    ]);
    let outer_cell = json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [nested],
    });
    let document = document_with(vec![table(vec![
        row(vec![outer_cell, cell("b")]),
        row(vec![cell("c")]),
    ])]);

    let nested_table_pos = FIRST_CELL_POSITION + 1;
    let nested_grid = outer_table_grid(&document, nested_table_pos, &schema(), &limits());
    assert!(
        nested_grid.is_err(),
        "a nested table is never an outer normalization target",
    );

    let operations = normalize(&document);
    let outer = document
        .node_at(&table_path(&document, TABLE_POSITION))
        .expect("the outer table exists");
    let nested_end = nested_table_pos
        + outer
            .child(0)
            .and_then(|row| row.child(0))
            .and_then(|cell| cell.child(0))
            .expect("the nested table exists")
            .node_size();
    for position in inserted_cell_positions(&operations)
        .into_iter()
        .chain(attribute_targets(&operations))
    {
        assert!(
            position <= nested_table_pos || position >= nested_end,
            "operation at {position} falls inside the nested table",
        );
    }
    assert!(!operations.is_empty());
}

#[test]
fn a_table_position_that_holds_no_table_is_refused() {
    let document = document_with(vec![json!({
        "type": PARAGRAPH_NODE,
        "content": [{ "type": "text", "text": "plain" }]
    })]);

    let error = normalize_outer_table(&document, TABLE_POSITION, &schema(), &limits())
        .expect_err("a paragraph is not a normalization target");
    assert_eq!(error.code, "DOCUMENT_INVALID");
    assert_eq!(
        outer_table_grid(&document, TABLE_POSITION, &schema(), &limits()),
        Ok(None)
    );
}

use crate::command_planner::SemanticCommandHistory;
use crate::position::PositionMap;
use crate::selection::Selection;
use crate::tables::command_context::{
    prepare_table_action, CellAnchorPair, PreparedTableAction, TableAction, TableActionCandidate,
    TableActionContext, TableActionOutcome,
};
use crate::tables::types::{TableActionKind, TableWorkCounters};
use crate::yrs_engine::{table_action_plan_for_test, CommandPlan, TableActionTestRequest};
use crate::yrs_engine::{
    EditingLimits, HistoryPolicy, OperationError, ResolvedPoint, ResolvedSelection,
    TransactionOrigin,
};

const REQUEST_ID: u64 = 41;
const BASE_REVISION: u64 = 7;
const FIRST_CELL_TEXT_POSITION: u32 = 4;

struct ScriptedAction<F> {
    kind: TableActionKind,
    plan: F,
}

impl<F> TableAction for ScriptedAction<F>
where
    F: Fn(&TableActionCandidate<'_>) -> Option<TableActionOutcome>,
{
    fn kind(&self) -> TableActionKind {
        self.kind
    }

    fn plan(
        &self,
        candidate: &TableActionCandidate<'_>,
        _schema: &Schema,
        _limits: &ResourceLimits,
    ) -> Option<TableActionOutcome> {
        (self.plan)(candidate)
    }
}

fn typing_action(
) -> ScriptedAction<impl Fn(&TableActionCandidate<'_>) -> Option<TableActionOutcome>> {
    ScriptedAction {
        kind: TableActionKind::Header,
        plan: |_candidate: &TableActionCandidate<'_>| {
            Some(TableActionOutcome {
                operations: vec![SemanticOperation::InsertText {
                    pos: FIRST_CELL_TEXT_POSITION,
                    text: "z".to_string(),
                    marks: Vec::new(),
                }],
                selection_after: Selection::cursor(FIRST_CELL_TEXT_POSITION + 1),
            })
        },
    }
}

fn unavailable_action(
) -> ScriptedAction<impl Fn(&TableActionCandidate<'_>) -> Option<TableActionOutcome>> {
    ScriptedAction {
        kind: TableActionKind::Merge,
        plan: |_candidate: &TableActionCandidate<'_>| None,
    }
}

fn context<'a>(
    document: &'a Document,
    schema: &'a Schema,
    limits: &'a ResourceLimits,
    editing_limits: &'a EditingLimits,
    anchors: Option<CellAnchorPair>,
) -> TableActionContext<'a> {
    TableActionContext {
        request_id: REQUEST_ID,
        base_document_revision: BASE_REVISION,
        table_pos: TABLE_POSITION,
        anchors,
        schema,
        resource_limits: limits,
        editing_limits,
        document,
    }
}

fn prepare(
    document: &Document,
    anchors: Option<CellAnchorPair>,
    action: &dyn TableAction,
) -> Result<PreparedTableAction, OperationError> {
    let schema = schema();
    let limits = limits();
    let editing_limits = EditingLimits::default();
    prepare_table_action(
        &context(document, &schema, &limits, &editing_limits, anchors),
        action,
    )
}

fn refused(prepared: Result<PreparedTableAction, OperationError>) -> OperationError {
    match prepared {
        Ok(prepared) => panic!(
            "preparation must fail, but it planned {} operations",
            prepared.plan.plan.operations.len()
        ),
        Err(error) => error,
    }
}

fn short_row_document() -> Document {
    document_with(vec![table(vec![
        row(vec![cell("a"), cell("b")]),
        row(vec![cell("c")]),
    ])])
}

#[test]
fn an_explicit_action_runs_exactly_one_pre_pass_and_one_post_pass() {
    let document = short_row_document();

    let prepared = prepare(&document, None, &typing_action()).expect("the action prepares");

    assert_eq!(
        prepared.counters,
        TableWorkCounters {
            pre_normalization_passes: 1,
            post_normalization_passes: 1,
            normalization_operations: 1,
        }
    );
    assert_eq!(prepared.origin, TransactionOrigin::LocalCommand);
    assert_eq!(prepared.kind, TableActionKind::Header);
    assert_eq!(
        prepared.plan.plan.history,
        SemanticCommandHistory::InputBoundary
    );
    assert!(!projection_of(&prepared.plan.simulated.document, TABLE_POSITION).irregular);
}

#[test]
fn a_valid_grid_action_runs_two_passes_that_plan_nothing() {
    let document = document_with(vec![table(vec![
        row(vec![cell("a"), cell("b")]),
        row(vec![cell("c"), cell("d")]),
    ])]);

    let prepared = prepare(&document, None, &typing_action()).expect("the action prepares");

    assert_eq!(
        prepared.counters,
        TableWorkCounters {
            pre_normalization_passes: 1,
            post_normalization_passes: 1,
            normalization_operations: 0,
        }
    );
    assert_eq!(prepared.plan.plan.operations.len(), 1);
}

#[test]
fn an_unavailable_action_never_commits_its_normalization() {
    let document = short_row_document();

    let error = refused(prepare(&document, None, &unavailable_action()));

    assert_eq!(error.code, "OPERATION_INVALID");
    assert_eq!(error.request_id, REQUEST_ID);
    assert_eq!(
        error.details,
        Some(json!({ "field": "tableAction" })),
        "a failed action reports the action field, not a normalization commit",
    );
}

#[test]
fn an_action_that_leaves_an_invalid_grid_fails_preparation() {
    let document = document_with(vec![table(vec![row(vec![cell("a")])])]);
    let emptying_action = ScriptedAction {
        kind: TableActionKind::DeleteRow,
        plan: |_candidate: &TableActionCandidate<'_>| {
            Some(TableActionOutcome {
                operations: vec![SemanticOperation::DeleteRange { from: 2, to: 7 }],
                selection_after: Selection::cursor(2),
            })
        },
    };

    let error = refused(prepare(&document, None, &emptying_action));

    assert_eq!(error.code, "OPERATION_INVALID");
    assert_eq!(error.details, Some(json!({ "field": "tableAction.grid" })));
}

#[test]
fn a_gap_is_never_a_usable_action_anchor() {
    let document = short_row_document();
    let gap_anchor = CellAnchorPair {
        anchor: FIRST_CELL_POSITION,
        head: FIRST_CELL_POSITION + 100,
    };

    let error = refused(prepare(&document, Some(gap_anchor), &typing_action()));

    assert_eq!(
        error.details,
        Some(json!({ "field": "tableAction.anchors" }))
    );
}

#[test]
fn normalization_that_changes_a_merge_rectangle_refuses_the_action() {
    let document = short_row_document();
    let first_row_cell_size = document
        .node_at(&table_path(&document, TABLE_POSITION))
        .and_then(|table| table.child(0))
        .and_then(|row| row.child(0))
        .expect("the first cell exists")
        .node_size();
    let second_column_cell = FIRST_CELL_POSITION + first_row_cell_size;
    let second_row_cell = FIRST_CELL_POSITION + 2 * first_row_cell_size + 2;
    let anchors = CellAnchorPair {
        anchor: second_column_cell,
        head: second_row_cell,
    };

    let error = refused(prepare(&document, Some(anchors), &typing_action()));

    assert_eq!(
        error.details,
        Some(json!({ "field": "tableAction.anchors" }))
    );
}

#[test]
fn an_unchanged_merge_rectangle_keeps_the_action_available() {
    let document = short_row_document();
    let first_row_cell_size = document
        .node_at(&table_path(&document, TABLE_POSITION))
        .and_then(|table| table.child(0))
        .and_then(|row| row.child(0))
        .expect("the first cell exists")
        .node_size();
    let anchors = CellAnchorPair {
        anchor: FIRST_CELL_POSITION,
        head: FIRST_CELL_POSITION + first_row_cell_size,
    };

    let prepared = prepare(&document, Some(anchors), &typing_action())
        .expect("an unchanged rectangle still prepares");

    assert_eq!(prepared.counters.pre_normalization_passes, 1);
}

fn regular_fixture_json() -> String {
    json!({
        "type": "doc",
        "content": [table(vec![
            row(vec![cell("a"), cell("b")]),
            row(vec![cell("c"), cell("d")]),
        ])],
    })
    .to_string()
}

fn table_action_command_plan(origin: TransactionOrigin) -> Result<CommandPlan, OperationError> {
    let session = seeded_session(regular_fixture_json());
    session_table_action_plan(&session, origin, true)
}

#[test]
fn a_table_action_commits_one_history_boundary_under_the_local_command_origin() {
    let plan = table_action_command_plan(TransactionOrigin::LocalCommand)
        .expect("a local command carries the table action");

    let CommandPlan::Transaction(transaction) = plan else {
        panic!("a table action produces a document transaction");
    };
    assert_eq!(transaction.history_policy, HistoryPolicy::Boundary);
    assert_eq!(transaction.origin, TransactionOrigin::LocalCommand);
}

#[test]
fn a_remote_or_history_origin_can_never_carry_a_table_action() {
    for origin in [
        TransactionOrigin::RemoteSync,
        TransactionOrigin::UndoRedo,
        TransactionOrigin::LocalInput,
        TransactionOrigin::LocalApi,
        TransactionOrigin::SnapshotRestore,
        TransactionOrigin::DocumentImport,
    ] {
        let error = match table_action_command_plan(origin) {
            Ok(_) => panic!("{origin:?} must never carry a table action"),
            Err(error) => error,
        };
        assert_eq!(error.code, "TRANSACTION_INVALID");
        assert_eq!(error.details, Some(json!({ "field": "origin" })));
    }
}

use yrs::branch::Branch;
use yrs::types::xml::{XmlElementRef, XmlFragment, XmlOut};
use yrs::updates::decoder::Decode;
use yrs::{Doc, ReadTxn, Transact, Update};

use crate::document_api::DocumentApiFacade;
use crate::session::{
    CollaborationLimits, EditorInitialization, EditorSession, EditorSessionConfig, InitialContent,
};
use crate::yrs_engine::TypedTransaction;

const COLLABORATION_FRAGMENT_NAME: &str = "prosemirror";
const IDENTITY_FIXTURE_CELLS: usize = 3;
const NORMALIZED_FIXTURE_CELLS: usize = 4;

fn identity_fixture_json() -> String {
    json!({
        "type": "doc",
        "content": [table(vec![
            row(vec![cell_with(SINGLE_SPAN, 4, Value::Null, "tall"), cell("b")]),
            row(vec![cell("c")]),
            row(Vec::new()),
        ])],
    })
    .to_string()
}

fn seeded_session(initial_json: String) -> EditorSession {
    DocumentApiFacade::admit(
        EditorSessionConfig {
            schema_json: None,
            fragment_name: COLLABORATION_FRAGMENT_NAME.into(),
            initialization: EditorInitialization::Local {
                initial_content: InitialContent::Json(initial_json),
            },
            resource_limits: limits(),
            editing_limits: EditingLimits::default(),
            collaboration_limits: CollaborationLimits::default(),
            max_length: None,
            read_only: false,
            input_filter: None,
            allow_base64_images: false,
        },
        schema(),
    )
    .expect("the table fixture is admitted")
}

fn cell_identities(state: &[u8]) -> Vec<String> {
    let replica = Doc::new();
    {
        let mut txn = replica.transact_mut();
        txn.apply_update(Update::decode_v1(state).expect("the encoded state decodes"))
            .expect("the encoded state applies");
    }
    let txn = replica.transact();
    let fragment = txn
        .get_xml_fragment(COLLABORATION_FRAGMENT_NAME)
        .expect("the collaboration fragment exists");
    let mut identities = Vec::new();
    for table_index in 0..fragment.len(&txn) {
        let Some(XmlOut::Element(table)) = fragment.get(&txn, table_index) else {
            continue;
        };
        for row_index in 0..table.len(&txn) {
            let Some(XmlOut::Element(row)) = table.get(&txn, row_index) else {
                continue;
            };
            for cell_index in 0..row.len(&txn) {
                let Some(XmlOut::Element(cell)) = row.get(&txn, cell_index) else {
                    continue;
                };
                identities.push(format!(
                    "{:?}",
                    <XmlElementRef as AsRef<Branch>>::as_ref(&cell).id()
                ));
            }
        }
    }
    identities
}

fn prepared_action(
    document: &Document,
    revision: u64,
    action: &dyn TableAction,
) -> PreparedTableAction {
    let schema = schema();
    let limits = limits();
    let editing_limits = EditingLimits::default();
    prepare_table_action(
        &TableActionContext {
            request_id: REQUEST_ID,
            base_document_revision: revision,
            table_pos: TABLE_POSITION,
            anchors: None,
            schema: &schema,
            resource_limits: &limits,
            editing_limits: &editing_limits,
            document,
        },
        action,
    )
    .expect("the action prepares")
}

fn session_table_action_plan(
    session: &EditorSession,
    origin: TransactionOrigin,
    guard_whole_table_lowering: bool,
) -> Result<CommandPlan, OperationError> {
    let schema = schema();
    let limits = limits();
    let editing_limits = EditingLimits::default();
    let document = session
        .engine
        .document()
        .expect("the seeded session has a document")
        .clone();
    let revision = session.engine.revision();
    let prepared = prepared_action(&document, revision, &typing_action());
    table_action_plan_for_test(
        TableActionTestRequest {
            document: &document,
            schema: &schema,
            resource_limits: &limits,
            editing_limits: &editing_limits,
            revision,
            state_revision: session.engine.state_revision(),
            yrs_state_epoch: session.engine.yrs_state_epoch(),
            origin,
            guard_whole_table_lowering,
        },
        prepared,
    )
}

#[test]
fn a_table_action_is_refused_rather_than_replacing_its_whole_table() {
    let session = seeded_session(identity_fixture_json());

    let error = match session_table_action_plan(&session, TransactionOrigin::LocalCommand, true) {
        Ok(_) => panic!("a whole table replacement must never reach the engine"),
        Err(error) => error,
    };

    assert_eq!(error.code, "ENGINE_INVARIANT_FAILED");
}

#[test]
fn the_whole_table_lowering_this_release_refuses_would_destroy_cell_identities() {
    let mut session = seeded_session(identity_fixture_json());
    let before = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the seeded state encodes"),
    );
    assert_eq!(before.len(), IDENTITY_FIXTURE_CELLS);

    let CommandPlan::Transaction(transaction) =
        session_table_action_plan(&session, TransactionOrigin::LocalCommand, false)
            .expect("the prepared action lowers")
    else {
        panic!("a table action lowers to a document transaction");
    };
    assert!(
        matches!(
            transaction.operations.as_slice(),
            [crate::yrs_engine::TypedOperation::ReplaceStructure(_)]
        ),
        "the engine can only express this action as one structural replacement",
    );
    session
        .engine
        .apply_typed_transaction(transaction)
        .expect("the whole table replacement commits");

    let after = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the replaced state encodes"),
    );
    assert_eq!(after.len(), NORMALIZED_FIXTURE_CELLS);
    assert!(
        before.iter().any(|identity| !after.contains(identity)),
        "the refused lowering would have preserved every cell identity",
    );
}
