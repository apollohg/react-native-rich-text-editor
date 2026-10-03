use std::collections::HashMap;

use proptest::prelude::*;
use serde_json::{json, Value};

use crate::boundary::DEFAULT_MAX_TABLE_GRID_SLOTS;
use crate::model::{Document, Fragment, Node};
use crate::schema::Schema;
use crate::serialize::json_in::{from_prosemirror_json, UnknownTypeMode};
use crate::tables::projection::{project_table, CellRect, ProjectedTable, TableGridBudget};
use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};
use crate::tables::types::TableError;

const TABLE_NODE: &str = "table";
const ROW_NODE: &str = "table_row";
const CELL_NODE: &str = "table_cell";
const HEADER_CELL_NODE: &str = "table_header";
const PARAGRAPH_NODE: &str = "paragraph";
const TABLE_POSITION: u32 = 0;
const FIRST_CELL_POSITION: u32 = 2;
const SINGLE_SPAN: u32 = 1;
const GENEROUS_GRID_LIMIT: usize = 4_096;

fn schema() -> Schema {
    tabled_schema(PROSEMIRROR_TABLE_NAMES)
}

fn cell(attrs: Value) -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": attrs,
        "content": [{ "type": PARAGRAPH_NODE, "content": [] }],
    })
}

fn plain_cell() -> Value {
    cell(json!({ "colspan": 1, "rowspan": 1, "colwidth": null }))
}

fn spanning_cell(colspan: u32, rowspan: u32) -> Value {
    cell(json!({ "colspan": colspan, "rowspan": rowspan, "colwidth": null }))
}

fn sized_cell(colwidth: Value) -> Value {
    cell(json!({ "colspan": 1, "rowspan": 1, "colwidth": colwidth }))
}

fn row(cells: Vec<Value>) -> Value {
    json!({ "type": ROW_NODE, "content": cells })
}

fn table(rows: Vec<Value>) -> Value {
    json!({ "type": TABLE_NODE, "content": rows })
}

fn document_with(table: Value) -> Document {
    from_prosemirror_json(
        &json!({ "type": "doc", "content": [table] }),
        &schema(),
        UnknownTypeMode::Preserve,
    )
    .expect("the table fixture parses")
}

fn project_with_limit(table: Value, limit: usize) -> Result<ProjectedTable, TableError> {
    let document = document_with(table);
    let node = document
        .root()
        .child(0)
        .expect("the fixture document holds the table")
        .clone();
    project_table(
        &node,
        TABLE_POSITION,
        &schema(),
        &mut TableGridBudget::new(limit),
    )
}

fn project(table: Value) -> ProjectedTable {
    project_with_limit(table, GENEROUS_GRID_LIMIT).expect("the fixture projects")
}

fn rect(row: u32, column: u32, rowspan: u32, colspan: u32) -> CellRect {
    CellRect {
        row,
        column,
        rowspan,
        colspan,
    }
}

fn rects(projected: &ProjectedTable) -> Vec<CellRect> {
    projected
        .cells
        .iter()
        .map(|cell| cell.rect.clone())
        .collect()
}

fn empty_table_node() -> Node {
    Node::element(TABLE_NODE.to_string(), HashMap::new(), Fragment::empty())
}

#[test]
fn reference_collision_keeps_source_but_shrinks_effective_span() {
    let projected = project(table(vec![
        row(vec![plain_cell(), spanning_cell(1, 2)]),
        row(vec![spanning_cell(2, 1)]),
    ]));
    assert_eq!(projected.cells.len(), 3);
    assert_eq!(projected.cells[2].rect, rect(1, 3, 1, 1));
    assert!(projected.irregular);
    assert_eq!(
        projected.slots,
        vec![Some(0), Some(1), None, None, None, Some(1), None, Some(2)]
    );
    assert_eq!(
        projected.synthetic.len(),
        4,
        "three generated gaps and one remaining hole"
    );
    assert_eq!(projected.compatibility_diagnostic, None);
}

#[test]
fn reference_fallback_retains_sources_and_shared_budget_accounting() {
    let document = document_with(table(vec![
        row(vec![plain_cell(), spanning_cell(1, 2)]),
        row(vec![spanning_cell(2, 3)]),
        row(vec![]),
    ]));
    let node = document.root().child(0).unwrap();
    let before = node.clone();
    let mut budget = TableGridBudget::new(16);
    let projected = project_table(node, 0, &schema(), &mut budget).unwrap();
    assert_eq!(
        projected.compatibility_diagnostic,
        Some("overlapping-reference-cells")
    );
    assert_eq!(projected.cells.len(), 3);
    assert_eq!(node, &before);
    let regular = document_with(table(vec![row(vec![plain_cell(); 4])]));
    project_table(regular.root().child(0).unwrap(), 0, &schema(), &mut budget).unwrap();
    assert_eq!(
        project_table(&empty_table_node(), 0, &schema(), &mut budget),
        Err(TableError::GridLimit {
            limit: 16,
            actual: 17
        })
    );
}

#[test]
fn reference_virtual_growth_limit_falls_back_without_rejecting_raw_content() {
    let projected = project_with_limit(
        table(vec![
            row(vec![plain_cell(), spanning_cell(1, 2)]),
            row(vec![spanning_cell(2, 3)]),
            row(vec![]),
        ]),
        12,
    )
    .unwrap();
    assert_eq!(
        projected.compatibility_diagnostic,
        Some("virtual-grid-limit")
    );
    assert_eq!(projected.cells.len(), 3);
}

#[test]
fn reference_concurrent_merge_inserts_a_leading_gap() {
    let projected = project(table(vec![
        row(vec![spanning_cell(2, 2)]),
        row(vec![plain_cell()]),
    ]));
    assert_eq!(projected.columns, 3);
    assert_eq!(rects(&projected), vec![rect(0, 1, 2, 2), rect(1, 0, 1, 1)]);
    assert!(projected.irregular);
}

#[test]
fn reference_short_first_row_inserts_a_leading_gap() {
    let projected = project(table(vec![
        row(vec![plain_cell()]),
        row(vec![plain_cell(), plain_cell()]),
    ]));
    assert_eq!(
        rects(&projected),
        vec![rect(0, 1, 1, 1), rect(1, 0, 1, 1), rect(1, 1, 1, 1)]
    );
}

#[test]
fn reference_custom_default_ordering_requires_an_explicit_fallback() {
    for (content, heading_first, heading_name, supported) in [
        ("(heading | paragraph)+", false, "heading", false),
        ("block+", true, "heading", false),
        ("block+", false, "heading", true),
        ("paragraph+", true, "heading", true),
        ("block+", false, "1", false),
    ] {
        let base = schema();
        let mut nodes: Vec<_> = base.all_nodes().cloned().collect();
        let mut heading = base.node(PARAGRAPH_NODE).unwrap().clone();
        heading.name = heading_name.into();
        nodes.insert(if heading_first { 1 } else { 2 }, heading);
        for node in &mut nodes {
            if node.name == CELL_NODE || node.name == HEADER_CELL_NODE {
                node.content = crate::schema::content_rule::ContentRule::parse(content).unwrap();
            }
        }
        let schema = Schema::new(nodes, base.all_marks().cloned().collect());
        assert!(crate::tables::TableRoles::resolve(&schema)
            .unwrap()
            .is_some());
        let document = from_prosemirror_json(
            &json!({"type": "doc", "content": [table(vec![
                row(vec![crate::tables::normalize_tests::cell("a")]),
                row(vec![crate::tables::normalize_tests::cell("b"), crate::tables::normalize_tests::cell("c")]),
            ])]}),
            &schema,
            UnknownTypeMode::Preserve,
        )
        .unwrap();
        let before = document.root().clone();
        let projected = project_table(
            document.root().child(0).unwrap(),
            0,
            &schema,
            &mut TableGridBudget::new(256),
        )
        .unwrap();
        assert_eq!(
            projected.compatibility_diagnostic,
            (!supported).then_some("unsupported-gap-default"),
            "{content}, heading_first={heading_first}, heading_name={heading_name}"
        );
        assert_eq!(projected.cells.len(), 3);
        if supported {
            assert_eq!(
                projected.synthetic[0].node.child(0).unwrap().node_type(),
                PARAGRAPH_NODE
            );
        } else {
            assert!(projected.synthetic.is_empty());
        }
        assert_eq!(document.root(), &before);
        let operations = crate::tables::normalize::normalize_outer_table(
            &document,
            0,
            &schema,
            &crate::boundary::ResourceLimits::default(),
        );
        if supported {
            assert!(
                matches!(&operations.unwrap()[0], crate::command_planner::SemanticOperation::ReplaceRange { content, .. }
                if content.children()[0].child(0).unwrap().node_type() == PARAGRAPH_NODE)
            );
        } else {
            assert!(
                operations.is_err(),
                "unproven reference defaults cannot be persisted"
            );
        }
        let regular = from_prosemirror_json(
            &json!({"type": "doc", "content": [table(vec![row(vec![plain_cell()])])]}),
            &schema,
            UnknownTypeMode::Preserve,
        )
        .unwrap();
        assert!(
            crate::tables::normalize::normalize_outer_table(
                &regular,
                0,
                &schema,
                &crate::boundary::ResourceLimits::default(),
            )
            .unwrap()
            .is_empty(),
            "a valid table does not require gap default proof"
        );
    }
}

#[test]
fn reference_header_gaps_use_schema_defaults_without_copying_source_payload() {
    let base = schema();
    let mut nodes: Vec<_> = base.all_nodes().cloned().collect();
    for node in &mut nodes {
        if node.name == HEADER_CELL_NODE {
            node.attrs.insert(
                "background".into(),
                crate::schema::AttrSpec {
                    default: Some(json!("ivory")),
                    has_default: true,
                    ..Default::default()
                },
            );
            node.attrs.insert(
                "opaque".into(),
                crate::schema::AttrSpec {
                    default: Some(Value::Null),
                    has_default: true,
                    ..Default::default()
                },
            );
        }
        if node.name == PARAGRAPH_NODE {
            node.attrs.insert(
                "tone".into(),
                crate::schema::AttrSpec {
                    default: Some(json!("calm")),
                    has_default: true,
                    ..Default::default()
                },
            );
        }
    }
    let schema = Schema::new(nodes, base.all_marks().cloned().collect());
    let mut header = plain_cell();
    header["type"] = json!(HEADER_CELL_NODE);
    header["attrs"]["opaque"] = json!({"type": "table_cell", "payload": ["😀", 7]});
    let document = from_prosemirror_json(
        &json!({"type": "doc", "content": [table(vec![
            row(vec![header]), row(vec![plain_cell(), plain_cell()]),
        ])]}),
        &schema,
        UnknownTypeMode::Preserve,
    )
    .unwrap();
    let before = document.root().clone();
    let projected = project_table(
        document.root().child(0).unwrap(),
        0,
        &schema,
        &mut TableGridBudget::new(64),
    )
    .unwrap();
    assert_eq!(projected.compatibility_diagnostic, None);
    let gap = &projected.synthetic[0];
    assert_eq!(gap.rect, rect(0, 0, 1, 1));
    assert_eq!(gap.node.node_type(), HEADER_CELL_NODE);
    assert_eq!(gap.node.attrs().get("background"), Some(&json!("ivory")));
    assert_eq!(gap.node.attrs().get("opaque"), Some(&Value::Null));
    assert_eq!(
        gap.node.child(0).unwrap().attrs().get("tone"),
        Some(&json!("calm"))
    );
    assert_eq!(document.root(), &before);
}

#[test]
fn a_regular_grid_projects_unchanged() {
    let projected = project(table(vec![
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
    ]));

    assert_eq!(projected.rows, 2);
    assert_eq!(projected.columns, 3);
    assert!(!projected.irregular);
    assert_eq!(
        rects(&projected),
        vec![
            rect(0, 0, SINGLE_SPAN, SINGLE_SPAN),
            rect(0, 1, SINGLE_SPAN, SINGLE_SPAN),
            rect(0, 2, SINGLE_SPAN, SINGLE_SPAN),
            rect(1, 0, SINGLE_SPAN, SINGLE_SPAN),
            rect(1, 1, SINGLE_SPAN, SINGLE_SPAN),
            rect(1, 2, SINGLE_SPAN, SINGLE_SPAN),
        ]
    );
    assert_eq!(
        projected.slots,
        (0..6).map(Some).collect::<Vec<Option<usize>>>()
    );
}

#[test]
fn the_first_cell_anchors_at_its_own_opening_position() {
    let projected = project(table(vec![row(vec![plain_cell(), plain_cell()])]));
    let first = projected.cells.first().expect("the row placed a cell");
    let second = projected.cells.get(1).expect("the row placed two cells");

    assert_eq!(first.source_pos, FIRST_CELL_POSITION);
    assert_eq!(second.source_pos, FIRST_CELL_POSITION + 4);
}

#[test]
fn every_real_cell_keeps_exactly_one_source_anchor() {
    let projected = project(table(vec![
        row(vec![plain_cell(), spanning_cell(2, 2)]),
        row(vec![plain_cell()]),
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
    ]));

    let mut anchors: Vec<u32> = projected.cells.iter().map(|cell| cell.source_pos).collect();
    let placed = anchors.len();
    anchors.sort_unstable();
    anchors.dedup();

    assert_eq!(placed, 6, "every cell node is projected exactly once");
    assert_eq!(anchors.len(), placed, "no two cells share a source anchor");
}

#[test]
fn projection_never_mutates_the_source_document() {
    let fixture = table(vec![
        row(vec![spanning_cell(2, 9), plain_cell()]),
        row(vec![plain_cell()]),
    ]);
    let document = document_with(fixture);
    let before = document.root().clone();
    let node = document.root().child(0).expect("the table is present");

    let projected = project_table(
        node,
        TABLE_POSITION,
        &schema(),
        &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
    )
    .expect("the irregular fixture still projects");

    assert!(projected.irregular);
    assert_eq!(
        document.root(),
        &before,
        "projection must not rewrite a single node"
    );
}

#[test]
fn a_cell_advances_past_the_column_a_rowspan_already_owns() {
    let projected = project(table(vec![
        row(vec![spanning_cell(1, 2), plain_cell()]),
        row(vec![plain_cell()]),
    ]));

    assert!(
        !projected.irregular,
        "advancing the cursor over an owned column is ordinary placement, not damage"
    );
    assert_eq!(projected.columns, 2);
    assert_eq!(
        rects(&projected),
        vec![
            rect(0, 0, 2, SINGLE_SPAN),
            rect(0, 1, SINGLE_SPAN, SINGLE_SPAN),
            rect(1, 1, SINGLE_SPAN, SINGLE_SPAN),
        ]
    );
}

#[test]
fn a_wide_cell_anchors_at_the_first_column_its_whole_rectangle_fits() {
    let projected = project(table(vec![
        row(vec![spanning_cell(1, 2), plain_cell(), plain_cell()]),
        row(vec![spanning_cell(2, 1)]),
    ]));

    assert_eq!(
        rects(&projected),
        vec![
            rect(0, 0, 2, SINGLE_SPAN),
            rect(0, 1, SINGLE_SPAN, SINGLE_SPAN),
            rect(0, 2, SINGLE_SPAN, SINGLE_SPAN),
            rect(1, 1, SINGLE_SPAN, 2),
        ],
        "a two-wide cell may not straddle the occupied first column"
    );
    assert!(!projected.irregular);
}

#[test]
fn a_collision_uses_reference_span_replacement_and_flags_the_raw_grid() {
    let projected = project(table(vec![
        row(vec![plain_cell(), spanning_cell(1, 2)]),
        row(vec![spanning_cell(2, 1)]),
    ]));

    assert!(projected.irregular);
    assert_eq!(
        rects(&projected),
        vec![
            rect(0, 0, SINGLE_SPAN, SINGLE_SPAN),
            rect(0, 1, 2, SINGLE_SPAN),
            rect(1, 3, SINGLE_SPAN, SINGLE_SPAN),
        ],
        "reference additions precede the span-adjusted source cell"
    );
    assert_eq!(
        projected.columns, 4,
        "reference additions grow the virtual grid past the raw extent"
    );
    assert_eq!(
        projected.slots,
        vec![Some(0), Some(1), None, None, None, Some(1), None, Some(2),]
    );
}

#[test]
fn growth_from_a_collision_is_checked_against_the_budget() {
    let fixture = || {
        table(vec![
            row(vec![plain_cell(), spanning_cell(1, 2)]),
            row(vec![spanning_cell(2, 1)]),
        ])
    };

    assert_eq!(
        project_with_limit(fixture(), 7),
        Err(TableError::GridLimit {
            limit: 7,
            actual: 8,
        }),
        "the grown extent, not the raw extent, is what the budget must admit"
    );
    assert_eq!(
        project_with_limit(fixture(), 8)
            .expect("the grown extent fits an eight slot budget")
            .columns,
        4
    );
}

#[test]
fn an_overlong_rowspan_is_clamped_only_in_the_projection() {
    let fixture = table(vec![
        row(vec![spanning_cell(1, 7)]),
        row(vec![plain_cell()]),
    ]);
    let document = document_with(fixture);
    let node = document.root().child(0).expect("the table is present");

    let projected = project_table(
        node,
        TABLE_POSITION,
        &schema(),
        &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
    )
    .expect("an overlong rowspan projects");

    assert_eq!(
        rects(&projected),
        vec![
            rect(0, 1, 2, SINGLE_SPAN),
            rect(1, 0, SINGLE_SPAN, SINGLE_SPAN),
        ]
    );
    assert!(projected.irregular);

    let raw_rowspan = node
        .child(0)
        .and_then(|row| row.child(0))
        .and_then(|cell| cell.attrs().get("rowspan").cloned())
        .expect("the raw cell keeps its attributes");
    assert_eq!(
        raw_rowspan,
        json!(7),
        "raw storage retains the original span"
    );
}

#[test]
fn holes_become_synthetic_slots_after_every_real_cell_is_placed() {
    let projected = project(table(vec![
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
        row(vec![plain_cell()]),
    ]));

    assert!(projected.irregular);
    assert_eq!(
        projected.slots,
        vec![Some(0), Some(1), Some(2), Some(3), None, None]
    );
    assert_eq!(
        projected.cells.len(),
        4,
        "synthetic slots carry no source cell"
    );
}

#[test]
fn an_empty_table_is_a_finite_empty_frame() {
    let projected = project_table(
        &empty_table_node(),
        TABLE_POSITION,
        &schema(),
        &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
    )
    .expect("an empty table is not an error");

    assert_eq!(projected.rows, 0);
    assert_eq!(projected.columns, 0);
    assert!(projected.cells.is_empty());
    assert!(projected.slots.is_empty());
    assert!(projected.widths.is_empty());
    assert!(projected.irregular);
}

#[test]
fn an_empty_frame_still_costs_one_grid_slot() {
    let schema = schema();
    let mut budget = TableGridBudget::new(1);

    project_table(&empty_table_node(), TABLE_POSITION, &schema, &mut budget)
        .expect("the first empty frame fits a single slot budget");
    let second = project_table(&empty_table_node(), TABLE_POSITION, &schema, &mut budget);

    assert_eq!(
        second,
        Err(TableError::GridLimit {
            limit: 1,
            actual: 2,
        }),
        "a caller iterating empty frames must stay bounded in the number of frames"
    );
}

#[test]
fn effective_rectangles_never_overlap() {
    for fixture in [
        table(vec![
            row(vec![spanning_cell(2, 2), plain_cell()]),
            row(vec![plain_cell(), plain_cell()]),
            row(vec![plain_cell(), plain_cell(), plain_cell()]),
        ]),
        table(vec![
            row(vec![spanning_cell(1, 3), spanning_cell(3, 1)]),
            row(vec![spanning_cell(2, 2)]),
            row(vec![plain_cell()]),
        ]),
    ] {
        let projected = project(fixture);
        let covered: usize = projected
            .cells
            .iter()
            .map(|cell| (cell.rect.rowspan as usize) * (cell.rect.colspan as usize))
            .sum();
        let filled = projected.slots.iter().filter(|slot| slot.is_some()).count();

        assert_eq!(
            covered, filled,
            "every covered slot belongs to exactly one cell"
        );
    }
}

#[test]
fn widths_follow_the_pinned_scan_and_count_rule() {
    let two_rows = project(table(vec![
        row(vec![sized_cell(json!([100]))]),
        row(vec![sized_cell(json!([140]))]),
    ]));
    let three_rows = project(table(vec![
        row(vec![sized_cell(json!([100]))]),
        row(vec![sized_cell(json!([100]))]),
        row(vec![sized_cell(json!([140]))]),
    ]));

    assert_eq!(two_rows.widths, vec![Some(140)]);
    assert_eq!(three_rows.widths, vec![Some(100)]);
}

#[test]
fn a_rowspan_contributes_its_width_once_per_covered_row() {
    let projected = project(table(vec![
        row(vec![cell(
            json!({ "colspan": 1, "rowspan": 2, "colwidth": [100] }),
        )]),
        row(vec![]),
        row(vec![sized_cell(json!([140]))]),
    ]));

    assert_eq!(
        projected.widths,
        vec![Some(100)],
        "two covered rows confirm the candidate before the third row disagrees"
    );
}

#[test]
fn an_unset_width_leaves_the_column_unresolved() {
    let projected = project(table(vec![row(vec![plain_cell(), sized_cell(json!([0]))])]));

    assert_eq!(projected.widths, vec![None, None]);
}

#[test]
fn a_zero_span_is_a_typed_attribute_failure() {
    let failure = project_with_limit(
        table(vec![row(vec![cell(
            json!({ "colspan": 0, "rowspan": 1, "colwidth": null }),
        )])]),
        GENEROUS_GRID_LIMIT,
    );

    assert_eq!(failure, Err(TableError::InvalidAttributes));
}

#[test]
fn a_non_numeric_span_is_a_typed_attribute_failure() {
    let failure = project_with_limit(
        table(vec![row(vec![cell(
            json!({ "colspan": "two", "rowspan": 1, "colwidth": null }),
        )])]),
        GENEROUS_GRID_LIMIT,
    );

    assert_eq!(failure, Err(TableError::InvalidAttributes));
}

#[test]
fn a_negative_column_width_is_a_typed_attribute_failure() {
    let failure = project_with_limit(
        table(vec![row(vec![sized_cell(json!([-4]))])]),
        GENEROUS_GRID_LIMIT,
    );

    assert_eq!(failure, Err(TableError::InvalidAttributes));
}

#[test]
fn an_overflowing_span_is_rejected_without_proportional_allocation() {
    let failure = project_with_limit(
        table(vec![row(vec![spanning_cell(u32::MAX, u32::MAX)])]),
        DEFAULT_MAX_TABLE_GRID_SLOTS,
    );

    assert_eq!(
        failure,
        Err(TableError::GridLimit {
            limit: DEFAULT_MAX_TABLE_GRID_SLOTS,
            actual: u32::MAX as usize,
        })
    );
}

#[test]
fn the_default_grid_limit_admits_its_own_boundary_and_rejects_one_slot_more() {
    let admitted = project_with_limit(
        table(vec![row(vec![spanning_cell(
            DEFAULT_MAX_TABLE_GRID_SLOTS as u32,
            SINGLE_SPAN,
        )])]),
        DEFAULT_MAX_TABLE_GRID_SLOTS,
    )
    .expect("the boundary grid projects");
    assert_eq!(admitted.columns, DEFAULT_MAX_TABLE_GRID_SLOTS as u32);

    let rejected = project_with_limit(
        table(vec![row(vec![spanning_cell(
            DEFAULT_MAX_TABLE_GRID_SLOTS as u32 + 1,
            SINGLE_SPAN,
        )])]),
        DEFAULT_MAX_TABLE_GRID_SLOTS,
    );
    assert_eq!(
        rejected,
        Err(TableError::GridLimit {
            limit: DEFAULT_MAX_TABLE_GRID_SLOTS,
            actual: DEFAULT_MAX_TABLE_GRID_SLOTS + 1,
        })
    );
}

#[test]
fn the_budget_charges_each_candidate_table_once() {
    let document = document_with(table(vec![
        row(vec![plain_cell(), plain_cell()]),
        row(vec![plain_cell(), plain_cell()]),
    ]));
    let node = document.root().child(0).expect("the table is present");
    let schema = schema();
    let mut budget = TableGridBudget::new(4);

    project_table(node, TABLE_POSITION, &schema, &mut budget).expect("the first table fits");
    let second = project_table(node, TABLE_POSITION, &schema, &mut budget);

    assert_eq!(
        second,
        Err(TableError::GridLimit {
            limit: 4,
            actual: 8,
        }),
        "a four slot budget pays for one four slot table, not for repeated passes over it"
    );
}

#[test]
fn a_table_of_empty_rows_exhausts_the_work_budget_before_the_grid_budget() {
    let rows = vec![row(vec![]); 200];
    let failure = project_with_limit(table(rows), 8);

    assert_eq!(
        failure,
        Err(TableError::WorkLimit),
        "rows without cells cost no grid slots, so only the work budget bounds them"
    );
}

#[test]
fn a_schema_without_table_roles_is_a_typed_structure_failure() {
    let failure = project_table(
        &empty_table_node(),
        TABLE_POSITION,
        &crate::prosemirror_schema(),
        &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
    );

    assert_eq!(failure, Err(TableError::InvalidStructure));
}

#[test]
fn a_node_that_is_not_the_table_role_is_a_typed_structure_failure() {
    let failure = project_table(
        &Node::element(ROW_NODE.to_string(), HashMap::new(), Fragment::empty()),
        TABLE_POSITION,
        &schema(),
        &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
    );

    assert_eq!(failure, Err(TableError::InvalidStructure));
}

#[test]
fn header_cells_project_beside_ordinary_cells() {
    let projected = project(table(vec![
        row(vec![json!({
            "type": HEADER_CELL_NODE,
            "attrs": { "colspan": 2, "rowspan": 1, "colwidth": [120, 120] },
            "content": [{ "type": PARAGRAPH_NODE, "content": [] }],
        })]),
        row(vec![plain_cell(), plain_cell()]),
    ]));

    assert_eq!(
        rects(&projected),
        vec![
            rect(0, 0, SINGLE_SPAN, 2),
            rect(1, 0, SINGLE_SPAN, SINGLE_SPAN),
            rect(1, 1, SINGLE_SPAN, SINGLE_SPAN),
        ]
    );
    assert_eq!(projected.widths, vec![Some(120), Some(120)]);
}

fn arbitrary_table() -> impl Strategy<Value = Value> {
    prop::collection::vec(
        prop::collection::vec((1u32..4, 1u32..4, prop::option::of(40u32..200)), 0..4),
        0..5,
    )
    .prop_map(|rows| {
        table(
            rows.into_iter()
                .map(|cells| {
                    row(cells
                        .into_iter()
                        .map(|(colspan, rowspan, width)| {
                            let colwidth = match width {
                                None => Value::Null,
                                Some(width) => Value::Array(vec![json!(width); colspan as usize]),
                            };
                            cell(json!({
                                "colspan": colspan,
                                "rowspan": rowspan,
                                "colwidth": colwidth,
                            }))
                        })
                        .collect())
                })
                .collect(),
        )
    })
}

proptest! {
    #[test]
    fn projection_is_deterministic_for_identical_raw_input(fixture in arbitrary_table()) {
        let document = document_with(fixture);
        let node = document.root().child(0).expect("the table is present");
        let schema = schema();

        let first = project_table(
            node,
            TABLE_POSITION,
            &schema,
            &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
        );
        let second = project_table(
            node,
            TABLE_POSITION,
            &schema,
            &mut TableGridBudget::new(GENEROUS_GRID_LIMIT),
        );

        prop_assert_eq!(first, second);
    }

    #[test]
    fn every_projected_slot_is_covered_by_at_most_one_cell(fixture in arbitrary_table()) {
        let projected = project(fixture);
        let covered: usize = projected
            .cells
            .iter()
            .map(|cell| (cell.rect.rowspan as usize) * (cell.rect.colspan as usize))
            .sum();
        let filled = projected.slots.iter().filter(|slot| slot.is_some()).count();

        prop_assert_eq!(covered, filled);
        prop_assert_eq!(
            projected.slots.len(),
            (projected.rows as usize) * (projected.columns as usize)
        );
        prop_assert_eq!(projected.widths.len(), projected.columns as usize);
    }
}
