use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::command_planner::SemanticOperation;
use crate::model::Document;
use crate::schema::Schema;
use crate::serialize::json_in::{from_prosemirror_json, UnknownTypeMode};
use crate::tables::normalize::{
    normalize_outer_table, outer_table_grid, planned_normalization_passes,
    reset_planned_normalization_passes,
};
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
const UNREACHABLE_POSITION: u32 = 10_000;
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

pub(crate) fn merged_fixture_table() -> Value {
    table(vec![
        row(vec![cell_with(3, SINGLE_SPAN, json!([100, 140, 180]), "h")]),
        row(vec![
            cell_with(SINGLE_SPAN, 2, json!([100]), "v"),
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "b"),
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([180]), "c"),
        ]),
        row(vec![
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "d"),
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([180]), "e"),
        ]),
    ])
}

pub(crate) fn limits() -> ResourceLimits {
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
fn collision_then_overlong_rowspan_matches_one_stock_reference_pass() {
    let document = document_with(vec![table(vec![
        row(vec![cell("a"), cell_with(1, 2, Value::Null, "b")]),
        row(vec![cell_with(2, 3, Value::Null, "c")]),
        row(vec![]),
    ])]);
    let gap = json!({
        "type": CELL_NODE,
        "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null },
        "content": [{ "type": PARAGRAPH_NODE }],
    });
    // Literal output of installed prosemirror-tables 1.8.5 fixTables, once.
    let expected = document_with(vec![table(vec![
        row(vec![
            cell("a"),
            cell_with(1, 2, Value::Null, "b"),
            gap.clone(),
        ]),
        row(vec![
            cell_with(2, 2, Value::Null, "c"),
            gap.clone(),
            gap.clone(),
        ]),
        row(vec![gap.clone(), gap]),
    ])]);
    assert_eq!(normalized(&document), expected);
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
fn collision_repairs_preserve_rich_sources_and_use_header_defaults_for_leading_gaps() {
    let base = schema();
    let rich = crate::schema::presets::prosemirror_schema();
    let mut nodes: Vec<_> = base.all_nodes().cloned().collect();
    nodes.push(rich.node("hard_break").unwrap().clone());
    for node in &mut nodes {
        if [TABLE_NODE, ROW_NODE, CELL_NODE, HEADER_CELL_NODE].contains(&node.name.as_str()) {
            node.attrs.insert(
                "opaque".into(),
                crate::schema::AttrSpec {
                    default: Some(Value::Null),
                    has_default: true,
                    ..Default::default()
                },
            );
        }
        if node.name == HEADER_CELL_NODE {
            node.attrs.insert(
                "background".into(),
                crate::schema::AttrSpec {
                    default: Some(json!("ivory")),
                    has_default: true,
                    ..Default::default()
                },
            );
        }
    }
    let schema = Schema::new(nodes, rich.all_marks().cloned().collect());
    let mut source = header_cell("c");
    source["attrs"]["colspan"] = json!(2);
    source["attrs"]["background"] = json!("blue");
    source["attrs"]["opaque"] = json!({"type": "table", "content": ["😀", 7]});
    source["content"] = json!([
        {"type": "paragraph", "content": [
            {"type": "text", "text": "c😀", "marks": [{"type": "bold"}]},
            {"type": "hard_break"}, {"type": "text", "text": "tail"}
        ]}, {"type": "paragraph", "content": [{"type": "text", "text": "second"}]}
    ]);
    let mut raw = table(vec![
        row(vec![cell("a"), cell_with(1, 2, Value::Null, "b")]),
        row(vec![source.clone()]),
    ]);
    raw["attrs"] = json!({"opaque": {"owner": "table"}});
    raw["content"][1]["attrs"] = json!({"opaque": {"owner": "row"}});
    let parse = |table| {
        from_prosemirror_json(
            &json!({"type":"doc", "content":[table]}),
            &schema,
            UnknownTypeMode::Preserve,
        )
        .unwrap()
    };
    let document = parse(raw.clone());
    let gap = json!({"type": CELL_NODE, "content": [{"type": PARAGRAPH_NODE}]});
    let header_gap = json!({"type": HEADER_CELL_NODE, "content": [{"type": PARAGRAPH_NODE}]});
    source["attrs"]["colspan"] = json!(1);
    raw["content"][0]["content"]
        .as_array_mut()
        .unwrap()
        .push(gap);
    raw["content"][1]["content"] = json!([header_gap.clone(), header_gap, source]);
    let operations = normalize_outer_table(&document, 0, &schema, &limits()).unwrap();
    let actual = crate::command_planner::apply_operations(&document, &schema, &operations).unwrap();
    assert_eq!(actual, parse(raw));
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
fn a_valid_merged_grid_with_agreeing_widths_plans_no_normalization() {
    let document = document_with(vec![merged_fixture_table()]);

    let projected = projection_of(&document, TABLE_POSITION);
    assert!(!projected.irregular, "the merged fixture must start valid");
    assert_eq!(
        projected.widths,
        vec![Some(100), Some(140), Some(180)],
        "the merged fixture must resolve every column width",
    );
    assert_eq!(normalize(&document), Vec::new());
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
    let containing_cell = outer
        .child(0)
        .and_then(|row| row.child(0))
        .expect("the outer cell holding the nested table exists");
    let cell_content_start = FIRST_CELL_POSITION + 1;
    let cell_content_end = cell_content_start + containing_cell.content_size();
    for position in inserted_cell_positions(&operations)
        .into_iter()
        .chain(attribute_targets(&operations))
    {
        assert!(
            position < cell_content_start || position >= cell_content_end,
            "operation at {position} falls inside the outer cell that holds the nested table",
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
use crate::selection::Selection;
use crate::tables::command_context::{
    is_action_unavailable, prepare_table_action, CellAnchorPair, PreparedTableAction, TableAction,
    TableActionCandidate, TableActionContext, TableActionOutcome,
};
use crate::tables::types::{TableActionKind, TableWorkCounters};
use crate::yrs_engine::{table_action_plan_for_test, CommandPlan, TableActionTestRequest};
use crate::yrs_engine::{EditingLimits, HistoryPolicy, OperationError, TransactionOrigin};

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
    ) -> crate::yrs_engine::OperationResult<Option<TableActionOutcome>> {
        Ok((self.plan)(candidate))
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
    selection: &'a crate::selection::Selection,
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
        selection,
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
    let selection = crate::selection::Selection::All;
    prepare_table_action(
        &context(
            document,
            &schema,
            &limits,
            &editing_limits,
            anchors,
            &selection,
        ),
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
fn preparing_an_action_invokes_the_planner_exactly_twice() {
    let document = short_row_document();
    reset_planned_normalization_passes();

    let prepared = prepare(&document, None, &typing_action()).expect("the action prepares");

    assert_eq!(
        planned_normalization_passes(),
        u64::from(prepared.counters.pre_normalization_passes)
            + u64::from(prepared.counters.post_normalization_passes),
        "the reported counters must account for every planner invocation",
    );
    assert_eq!(planned_normalization_passes(), 2);
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
    assert_eq!(prepared.plan.simulated.selection, Selection::cursor(5));
    assert_eq!(
        prepared.plan.plan.selection_after,
        Some(Selection::cursor(5))
    );
}

#[test]
fn an_unavailable_action_never_commits_its_normalization() {
    let document = short_row_document();

    let error = refused(prepare(&document, None, &unavailable_action()));

    assert_eq!(error.code, "OPERATION_INVALID");
    assert_eq!(error.request_id, REQUEST_ID);
    assert_eq!(
        error.details,
        Some(json!({ "field": "tableAction.unavailable" })),
        "a declined action reports its own field, not a normalization commit",
    );
    assert!(
        is_action_unavailable(&error),
        "a declined action is what callers may turn into a not-applicable plan",
    );
}

#[test]
fn a_step_that_cannot_apply_is_never_mistaken_for_an_unavailable_action() {
    let document = short_row_document();
    let impossible = ScriptedAction {
        kind: TableActionKind::InsertRow,
        plan: |_candidate: &TableActionCandidate<'_>| {
            Some(TableActionOutcome {
                operations: vec![SemanticOperation::DeleteRange {
                    from: UNREACHABLE_POSITION,
                    to: UNREACHABLE_POSITION + 1,
                }],
                selection_after: Selection::cursor(0),
            })
        },
    };

    let error = refused(prepare(&document, None, &impossible));

    assert_eq!(error.code, "OPERATION_INVALID", "unexpected: {error:?}");
    assert!(
        !is_action_unavailable(&error),
        "a step that cannot apply must reach the caller as an error: {error:?}",
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
    assert_eq!(
        error.details,
        Some(json!({ "field": "tableAction.grid.irregular" })),
        "an irregular result must be distinguishable from a table that vanished",
    );
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
        Some(json!({ "field": "tableAction.anchors.before" })),
        "anchors that never named real cells are distinguishable from anchors normalization moved",
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
        Some(json!({ "field": "tableAction.anchors.moved" })),
        "anchors normalization moved are distinguishable from anchors that were never real",
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
    session_table_action_plan(&session, origin)
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
use crate::yrs_engine::{
    SelectionIntent, StructuralEdit, StructuralEditBatch, TypedOperation, TypedTransaction,
};

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

pub(crate) fn seeded_session(initial_json: String) -> EditorSession {
    seeded_session_with(initial_json, EditingLimits::default())
}

fn seeded_session_with(initial_json: String, editing_limits: EditingLimits) -> EditorSession {
    DocumentApiFacade::admit(
        EditorSessionConfig {
            schema_json: None,
            fragment_name: COLLABORATION_FRAGMENT_NAME.into(),
            initialization: EditorInitialization::Local {
                initial_content: InitialContent::Json(initial_json),
            },
            resource_limits: limits(),
            editing_limits,
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

pub(crate) fn cell_identities(state: &[u8]) -> Vec<String> {
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
    editing_limits: &EditingLimits,
) -> PreparedTableAction {
    let schema = schema();
    let limits = limits();
    prepare_table_action(
        &TableActionContext {
            request_id: REQUEST_ID,
            base_document_revision: revision,
            table_pos: TABLE_POSITION,
            anchors: None,
            schema: &schema,
            resource_limits: &limits,
            editing_limits,
            document,
            selection: &crate::selection::Selection::All,
        },
        action,
    )
    .expect("the action prepares")
}

fn session_table_action_plan(
    session: &EditorSession,
    origin: TransactionOrigin,
) -> Result<CommandPlan, OperationError> {
    session_action_plan(session, origin, &typing_action(), &EditingLimits::default())
}

fn session_action_plan(
    session: &EditorSession,
    origin: TransactionOrigin,
    action: &dyn TableAction,
    editing_limits: &EditingLimits,
) -> Result<CommandPlan, OperationError> {
    let schema = schema();
    let limits = limits();
    let document = session
        .engine
        .document()
        .expect("the seeded session has a document")
        .clone();
    let revision = session.engine.revision();
    let prepared = prepared_action(&document, revision, action, editing_limits);
    table_action_plan_for_test(
        TableActionTestRequest {
            document: &document,
            schema: &schema,
            resource_limits: &limits,
            editing_limits,
            revision,
            state_revision: session.engine.state_revision(),
            yrs_state_epoch: session.engine.yrs_state_epoch(),
            origin,
        },
        prepared,
    )
}

#[test]
fn a_table_action_lowers_to_one_sealed_structural_edit_batch() {
    let session = seeded_session(identity_fixture_json());

    let plan = session_table_action_plan(&session, TransactionOrigin::LocalCommand)
        .expect("the prepared action lowers without replacing its whole table");

    let CommandPlan::Transaction(transaction) = plan else {
        panic!("a table action lowers to a document transaction");
    };
    let [TypedOperation::EditStructure(batch)] = transaction.operations.as_slice() else {
        panic!("a table action lowers to exactly one sealed structural edit batch");
    };
    assert!(
        batch.edits().len() > 1,
        "the identity fixture needs more than one disjoint edit, got {:?}",
        batch.edits(),
    );
}

#[test]
fn the_sealed_batch_keeps_every_cell_identity_the_whole_table_lowering_destroyed() {
    let mut session = seeded_session(identity_fixture_json());
    let before = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the seeded state encodes"),
    );
    assert_eq!(before.len(), IDENTITY_FIXTURE_CELLS);

    let CommandPlan::Transaction(transaction) =
        session_table_action_plan(&session, TransactionOrigin::LocalCommand)
            .expect("the prepared action lowers")
    else {
        panic!("a table action lowers to a document transaction");
    };
    session
        .engine
        .apply_typed_transaction(transaction)
        .expect("the sealed structural edit batch commits");

    let after = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the committed state encodes"),
    );
    assert_eq!(after.len(), NORMALIZED_FIXTURE_CELLS);
    for identity in &before {
        assert!(
            after.contains(identity),
            "normalization replaced the cell with identity {identity}",
        );
    }
}

#[test]
fn collision_normalization_preserves_source_identities_through_the_action_batch() {
    let mut b = cell_with(1, 2, Value::Null, "b");
    let mut c = cell_with(2, 1, Value::Null, "c");
    // Yjs history restores numeric attributes in its floating-point wire domain.
    b["attrs"]["rowspan"] = json!(2.0);
    c["attrs"]["colspan"] = json!(2.0);
    let mut session = seeded_session(
        json!({"type": "doc", "content": [table(vec![
            row(vec![cell("a"), b]),
            row(vec![c]),
        ])]})
        .to_string(),
    );
    let before = cell_identities(&session.engine.encoded_state().unwrap());
    let before_document = session.engine.document_json().unwrap();
    let prepared = prepared_action(
        session.engine.document().unwrap(),
        session.engine.revision(),
        &typing_action(),
        &EditingLimits::default(),
    );
    assert_eq!(prepared.plan.simulated.selection, Selection::cursor(9));
    assert_eq!(
        prepared.plan.plan.selection_after,
        Some(Selection::cursor(9))
    );
    assert_eq!(
        prepared
            .plan
            .simulated
            .document
            .node_at(&[0, 0, 1])
            .unwrap()
            .text_content(),
        "za"
    );
    let CommandPlan::Transaction(transaction) =
        session_table_action_plan(&session, TransactionOrigin::LocalCommand).unwrap()
    else {
        panic!("the bounded action produces a transaction");
    };
    session.engine.apply_typed_transaction(transaction).unwrap();
    let after = cell_identities(&session.engine.encoded_state().unwrap());
    assert_eq!(after.len(), 7);
    for identity in before {
        assert!(
            after.contains(&identity),
            "a reference repair replaced source {identity}"
        );
    }
    let after_document = session.engine.document_json().unwrap();
    reset_planned_normalization_passes();
    assert!(session.engine.undo(REQUEST_ID).unwrap().is_some());
    assert_eq!(session.engine.document_json().unwrap(), before_document);
    assert!(session.engine.redo(REQUEST_ID).unwrap().is_some());
    assert_eq!(session.engine.document_json().unwrap(), after_document);
    assert_eq!(planned_normalization_passes(), 0);
}

const SPANNING_COLSPAN: u32 = 2;
const WIDENED_COLSPAN: u32 = 3;
const SPANNED_FIXTURE_CELLS: usize = 5;
const SPANNED_FIXTURE_CELLS_AFTER_INSERT: usize = 7;
const INSERTED_COLUMN_INDEX: u32 = 1;
const CARET_OFFSET_IN_NEW_CELL: u32 = 2;

fn spanned_fixture_json() -> String {
    json!({
        "type": "doc",
        "content": [table(vec![
            row(vec![cell("a0"), cell("a1")]),
            row(vec![cell_with(SPANNING_COLSPAN, SINGLE_SPAN, Value::Null, "b0")]),
            row(vec![cell("c0"), cell("c1")]),
        ])],
    })
    .to_string()
}

fn empty_cell_json() -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": Value::Null },
        "content": [{ "type": PARAGRAPH_NODE, "content": [] }],
    })
}

fn empty_cell_node() -> crate::model::Node {
    document_with(vec![table(vec![row(vec![empty_cell_json()])])])
        .node_at(&[0, 0, 0])
        .expect("the fixture holds one cell")
        .clone()
}

fn child_start(document: &Document, parent_path: &[u32], child_index: u32) -> u32 {
    let mut position = 0u32;
    let mut node = document.root();
    for step in parent_path {
        let content = node.content().expect("the fixture path has content");
        for sibling in content.iter().take(*step as usize) {
            position += sibling.node_size();
        }
        position += 1;
        node = content
            .child(*step as usize)
            .expect("the fixture path exists");
    }
    let content = node.content().expect("the fixture parent has content");
    for sibling in content.iter().take(child_index as usize) {
        position += sibling.node_size();
    }
    position
}

fn widened_span_attrs() -> std::collections::HashMap<String, Value> {
    let mut attrs = std::collections::HashMap::new();
    attrs.insert("colspan".to_string(), Value::from(WIDENED_COLSPAN));
    attrs.insert("rowspan".to_string(), Value::from(SINGLE_SPAN));
    attrs.insert("colwidth".to_string(), Value::Null);
    attrs
}

fn insert_column_action(
) -> ScriptedAction<impl Fn(&TableActionCandidate<'_>) -> Option<TableActionOutcome>> {
    ScriptedAction {
        kind: TableActionKind::InsertColumn,
        plan: |candidate: &TableActionCandidate<'_>| {
            let document = candidate.document;
            let inserted = empty_cell_node();
            let inserted_size = inserted.node_size();
            let first_row = child_start(document, &[0, 0], INSERTED_COLUMN_INDEX);
            let spanning_cell = child_start(document, &[0, 1], 0);
            let last_row = child_start(document, &[0, 2], INSERTED_COLUMN_INDEX);
            Some(TableActionOutcome {
                operations: vec![
                    SemanticOperation::ReplaceRange {
                        from: first_row,
                        to: first_row,
                        content: crate::model::Fragment::from(vec![inserted.clone()]),
                    },
                    SemanticOperation::UpdateNodeAttrs {
                        pos: spanning_cell + inserted_size,
                        attrs: widened_span_attrs(),
                    },
                    SemanticOperation::ReplaceRange {
                        from: last_row + inserted_size,
                        to: last_row + inserted_size,
                        content: crate::model::Fragment::from(vec![inserted]),
                    },
                ],
                selection_after: Selection::cursor(first_row + CARET_OFFSET_IN_NEW_CELL),
            })
        },
    }
}

fn spanned_fixture_after_insert() -> Value {
    crate::serialize::json_out::node_to_json(
        document_with(vec![table(vec![
            row(vec![cell("a0"), empty_cell_json(), cell("a1")]),
            row(vec![cell_with(
                WIDENED_COLSPAN,
                SINGLE_SPAN,
                Value::Null,
                "b0",
            )]),
            row(vec![cell("c0"), empty_cell_json(), cell("c1")]),
        ])])
        .root(),
        &schema(),
    )
}

#[test]
fn inserting_a_column_across_rows_keeps_every_unchanged_cell_identity() {
    let mut session = seeded_session(spanned_fixture_json());
    let before = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the seeded state encodes"),
    );
    assert_eq!(before.len(), SPANNED_FIXTURE_CELLS);

    let plan = session_action_plan(
        &session,
        TransactionOrigin::LocalCommand,
        &insert_column_action(),
        &EditingLimits::default(),
    )
    .expect("the column insertion lowers");
    let CommandPlan::Transaction(transaction) = plan else {
        panic!("a table action lowers to a document transaction");
    };
    let [TypedOperation::EditStructure(batch)] = transaction.operations.as_slice() else {
        panic!("a column insertion lowers to exactly one sealed structural edit batch");
    };
    assert_eq!(batch.edits().len(), 3, "{:?}", batch.edits());

    session
        .engine
        .apply_typed_transaction(transaction)
        .expect("the sealed structural edit batch commits");

    assert_eq!(
        crate::serialize::json_out::node_to_json(
            session
                .engine
                .document()
                .expect("the committed session has a document")
                .root(),
            &schema(),
        ),
        spanned_fixture_after_insert(),
    );
    let after = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the committed state encodes"),
    );
    assert_eq!(after.len(), SPANNED_FIXTURE_CELLS_AFTER_INSERT);
    for identity in &before {
        assert!(
            after.contains(identity),
            "the column insertion replaced the cell with identity {identity}",
        );
    }
}

const RESTRICTED_OPERATION_BUDGET: usize = 2;
const BATCH_EDIT_COUNT: usize = 3;
const MISSING_ROW_INDEX: u32 = 9;

fn insert_column_batch(session: &EditorSession) -> StructuralEditBatch {
    let plan = session_action_plan(
        session,
        TransactionOrigin::LocalCommand,
        &insert_column_action(),
        &EditingLimits::default(),
    )
    .expect("the column insertion lowers");
    let CommandPlan::Transaction(transaction) = plan else {
        panic!("a table action lowers to a document transaction");
    };
    let [TypedOperation::EditStructure(batch)] = transaction.operations.as_slice() else {
        panic!("a column insertion lowers to exactly one sealed structural edit batch");
    };
    batch.clone()
}

fn batch_transaction(session: &EditorSession, batch: StructuralEditBatch) -> TypedTransaction {
    TypedTransaction {
        request_id: REQUEST_ID,
        base_document_revision: session.engine.revision(),
        origin: TransactionOrigin::LocalCommand,
        operations: vec![TypedOperation::EditStructure(batch)],
        selection_intent: SelectionIntent::UseOperationResult,
        history_policy: HistoryPolicy::Boundary,
    }
}

fn restricted_limits(max_operations: usize) -> EditingLimits {
    EditingLimits {
        max_operations_per_transaction: max_operations,
        ..EditingLimits::default()
    }
}

#[test]
fn a_sealed_batch_refuses_a_target_that_is_not_in_its_base_document() {
    let mut session = seeded_session(spanned_fixture_json());
    let batch = insert_column_batch(&session);
    let mut edits = batch.edits().to_vec();
    edits.push(StructuralEdit::SpliceChildren {
        parent_path: vec![0, MISSING_ROW_INDEX],
        from_child: 0,
        to_child: 0,
        content: crate::model::Fragment::from(vec![empty_cell_node()]),
    });
    let stale = batch_transaction(
        &session,
        StructuralEditBatch::new(edits, batch.selection_after().clone()),
    );

    let error = session
        .engine
        .apply_typed_transaction(stale)
        .expect_err("a target outside the base document cannot be admitted");

    assert_eq!(error.code, "OPERATION_INVALID");
    assert_eq!(error.details, Some(json!({ "field": "structure" })));
    assert_eq!(
        error.message.as_ref(),
        "structural target path is outside the document",
    );
}

#[test]
fn a_sealed_batch_refuses_an_edit_under_a_replaced_ancestor() {
    let mut session = seeded_session(spanned_fixture_json());
    let dependent = StructuralEditBatch::new(
        vec![
            StructuralEdit::SpliceChildren {
                parent_path: vec![0],
                from_child: 1,
                to_child: 2,
                content: crate::model::Fragment::empty(),
            },
            StructuralEdit::PatchAttributes {
                path: vec![0, 1, 0],
                attrs: widened_span_attrs(),
            },
        ],
        Selection::cursor(TABLE_POSITION),
    );

    let error = session
        .engine
        .apply_typed_transaction(batch_transaction(&session, dependent))
        .expect_err("a descendant of a replaced row cannot survive as a target");

    assert_eq!(error.code, "OPERATION_INVALID");
    assert_eq!(error.details, Some(json!({ "field": "structure" })));
    assert_eq!(
        error.message.as_ref(),
        "a sealed structural edit cannot survive under a replaced ancestor",
    );
}

#[test]
fn a_sealed_batch_is_charged_for_every_edit_it_carries() {
    let batch = insert_column_batch(&seeded_session(spanned_fixture_json()));
    assert_eq!(batch.edits().len(), BATCH_EDIT_COUNT);
    let mut restricted = seeded_session_with(
        spanned_fixture_json(),
        restricted_limits(RESTRICTED_OPERATION_BUDGET),
    );
    let mut generous =
        seeded_session_with(spanned_fixture_json(), restricted_limits(BATCH_EDIT_COUNT));

    let error = restricted
        .engine
        .apply_typed_transaction(batch_transaction(&restricted, batch.clone()))
        .expect_err("three batched edits cost three operations");

    assert_eq!(error.code, "OPERATION_LIMIT_EXCEEDED");
    assert_eq!(
        error.details,
        Some(json!({ "field": "maxOperationsPerTransaction" })),
    );
    assert_eq!(error.limit, Some(RESTRICTED_OPERATION_BUDGET as u64));
    assert_eq!(error.actual, Some(BATCH_EDIT_COUNT as u64));
    generous
        .engine
        .apply_typed_transaction(batch_transaction(&generous, batch))
        .expect("a budget that admits three operations admits three batched edits");
}

#[test]
fn a_refused_sealed_batch_publishes_nothing() {
    let mut session = seeded_session(spanned_fixture_json());
    let before_revision = session.engine.revision();
    let before_document = session
        .engine
        .document_json()
        .expect("the seeded document serializes");
    let before_identities = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the seeded state encodes"),
    );
    let batch = insert_column_batch(&session);
    let mut edits = batch.edits().to_vec();
    edits.push(StructuralEdit::SpliceChildren {
        parent_path: vec![0, MISSING_ROW_INDEX],
        from_child: 0,
        to_child: 0,
        content: crate::model::Fragment::from(vec![empty_cell_node()]),
    });

    session
        .engine
        .apply_typed_transaction(batch_transaction(
            &session,
            StructuralEditBatch::new(edits, batch.selection_after().clone()),
        ))
        .expect_err("one unusable edit refuses the whole sealed batch");

    assert_eq!(session.engine.revision(), before_revision);
    assert_eq!(
        session
            .engine
            .document_json()
            .expect("the refused document still serializes"),
        before_document,
    );
    assert_eq!(
        cell_identities(
            &session
                .engine
                .encoded_state()
                .expect("the refused state still encodes"),
        ),
        before_identities,
    );
}

const TWIN_FIXTURE_CELLS: usize = 2;
const TWIN_FIXTURE_CELLS_AFTER_INSERT: usize = 3;
const TWIN_CELL_TEXT: &str = "x";

fn twin_fixture_json() -> String {
    json!({
        "type": "doc",
        "content": [table(vec![row(vec![
            cell(TWIN_CELL_TEXT),
            cell(TWIN_CELL_TEXT),
        ])])],
    })
    .to_string()
}

fn insert_twin_action(
) -> ScriptedAction<impl Fn(&TableActionCandidate<'_>) -> Option<TableActionOutcome>> {
    ScriptedAction {
        kind: TableActionKind::InsertColumn,
        plan: |candidate: &TableActionCandidate<'_>| {
            let inserted = candidate
                .document
                .node_at(&[0, 0, 0])
                .expect("the twin fixture holds its first cell")
                .clone();
            let between = child_start(candidate.document, &[0, 0], INSERTED_COLUMN_INDEX);
            Some(TableActionOutcome {
                operations: vec![SemanticOperation::ReplaceRange {
                    from: between,
                    to: between,
                    content: crate::model::Fragment::from(vec![inserted]),
                }],
                selection_after: Selection::cursor(between + CARET_OFFSET_IN_NEW_CELL),
            })
        },
    }
}

#[test]
fn a_cell_inserted_between_identical_twins_keeps_both_twins_in_place() {
    let mut session = seeded_session(twin_fixture_json());
    let before = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the seeded state encodes"),
    );
    assert_eq!(before.len(), TWIN_FIXTURE_CELLS);

    let plan = session_action_plan(
        &session,
        TransactionOrigin::LocalCommand,
        &insert_twin_action(),
        &EditingLimits::default(),
    )
    .expect("the twin insertion lowers");
    let CommandPlan::Transaction(transaction) = plan else {
        panic!("a table action lowers to a document transaction");
    };
    session
        .engine
        .apply_typed_transaction(transaction)
        .expect("the sealed structural edit batch commits");

    let after = cell_identities(
        &session
            .engine
            .encoded_state()
            .expect("the committed state encodes"),
    );
    assert_eq!(after.len(), TWIN_FIXTURE_CELLS_AFTER_INSERT);
    assert_eq!(after[0], before[0], "the left twin kept its place");
    assert_eq!(after[2], before[1], "the right twin kept its place");
    assert!(
        !before.contains(&after[1]),
        "only the middle cell is newly created",
    );
}

const PARAGRAPH_PATH: [u32; 4] = [0, 0, 0, 0];
const PARAGRAPH_CONTENT_START: u32 = 0;
const SPLICED_TEXT: &str = "r";
const RETEXT_TEXT: &str = "q";

fn retext_edit() -> StructuralEdit {
    StructuralEdit::InsertContentText {
        parent_path: PARAGRAPH_PATH.to_vec(),
        parent_offset: PARAGRAPH_CONTENT_START,
        text: RETEXT_TEXT.to_string(),
        marks: Vec::new(),
    }
}

fn paragraph_splice_edit() -> StructuralEdit {
    StructuralEdit::SpliceChildren {
        parent_path: PARAGRAPH_PATH.to_vec(),
        from_child: 0,
        to_child: 0,
        content: crate::model::Fragment::from(vec![crate::model::Node::text(
            SPLICED_TEXT.to_string(),
            Vec::new(),
        )]),
    }
}

#[test]
fn a_sealed_batch_refuses_splicing_and_retexting_one_parent_in_either_order() {
    for (case, edits) in [
        ("splice first", vec![paragraph_splice_edit(), retext_edit()]),
        ("retext first", vec![retext_edit(), paragraph_splice_edit()]),
    ] {
        let mut session = seeded_session(twin_fixture_json());
        let batch = StructuralEditBatch::new(edits, Selection::cursor(TABLE_POSITION));

        let error = match session
            .engine
            .apply_typed_transaction(batch_transaction(&session, batch))
        {
            Ok(commit) => panic!("{case} must be refused, but it committed {commit:?}"),
            Err(error) => error,
        };

        assert_eq!(error.code, "OPERATION_INVALID", "{case}");
        assert_eq!(
            error.message.as_ref(),
            "a sealed structural edit batch cannot splice and retext one parent",
            "{case}",
        );
    }
}
