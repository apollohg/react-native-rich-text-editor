use std::collections::HashMap;

use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::model::{Document, Fragment, Mark, Node};
use crate::schema::Schema;
use crate::serialize::json_in::{from_prosemirror_json, UnknownTypeMode};
use crate::tables::admission::{validate_table_shapes, TableProjectionIndex};
use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};
use crate::tables::types::TableError;
use crate::transform::DocumentValidator;

const TABLE_NODE: &str = "table";
const ROW_NODE: &str = "table_row";
const CELL_NODE: &str = "table_cell";
const PARAGRAPH_NODE: &str = "paragraph";
const DOC_NODE: &str = "doc";
const EMPTY_TABLE_POSITION: u32 = 0;
const TABLE_AFTER_PARAGRAPH_POSITION: u32 = 4;
const FIRST_CELL_OFFSET: u32 = 2;
const NESTED_TABLE_OFFSET: u32 = 3;
const NO_ROWS: u32 = 0;
const NO_COLUMNS: u32 = 0;
const TWO_BY_TWO_SLOTS: usize = 4;
const CALLOUT_NODE: &str = "callout";
const UNKNOWN_MARK_TYPE: &str = "unknownMark";

fn schema() -> Schema {
    tabled_schema(PROSEMIRROR_TABLE_NAMES)
}

fn limits() -> ResourceLimits {
    ResourceLimits::default()
}

fn document_from(schema: &Schema, content: Vec<Value>) -> Document {
    from_prosemirror_json(
        &json!({ "type": DOC_NODE, "content": content }),
        schema,
        UnknownTypeMode::Preserve,
    )
    .expect("the admission fixture parses")
}

fn paragraph(text: &str) -> Value {
    json!({
        "type": PARAGRAPH_NODE,
        "content": [{ "type": "text", "text": text }],
    })
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

fn row(cells: Vec<Value>) -> Value {
    json!({ "type": ROW_NODE, "content": cells })
}

fn table(rows: Vec<Value>) -> Value {
    json!({ "type": TABLE_NODE, "content": rows })
}

fn regular_two_by_two() -> Value {
    table(vec![
        row(vec![plain_cell(), plain_cell()]),
        row(vec![plain_cell(), plain_cell()]),
    ])
}

fn index_for(schema: &Schema, document: &Document) -> TableProjectionIndex {
    validate_table_shapes(document, schema, &limits()).expect("the fixture projects")
}

#[test]
fn a_table_with_no_rows_is_admitted_and_projects_as_empty_geometry() {
    let schema = schema();
    let document = document_from(&schema, vec![table(vec![])]);

    DocumentValidator::validate(&document, &schema, &limits())
        .expect("the transient empty table is admitted");

    let index = index_for(&schema, &document);
    let projected = index
        .table_at(EMPTY_TABLE_POSITION)
        .expect("the empty table is indexed at its document position");

    assert_eq!(index.len(), 1);
    assert_eq!(projected.rows, NO_ROWS);
    assert_eq!(projected.columns, NO_COLUMNS);
    assert!(projected.slots.is_empty());
    assert!(projected.irregular);
    assert_eq!(
        index.irregular_positions().collect::<Vec<_>>(),
        vec![EMPTY_TABLE_POSITION]
    );
}

#[test]
fn the_cardinality_exception_does_not_reach_non_table_nodes() {
    let schema = schema();
    let empty_document = document_from(&schema, vec![]);

    let error = DocumentValidator::validate(&empty_document, &schema, &limits())
        .expect_err("a doc with no blocks still fails its content expression");

    assert_eq!(error.code, "DOCUMENT_INVALID");
    assert!(
        error
            .message
            .contains("does not match its content expression"),
        "{error:?}"
    );
}

#[test]
fn the_cardinality_exception_does_not_reach_other_roleless_containers() {
    let schema = tabled_schema_with_callout();
    let document = document_from(
        &schema,
        vec![json!({ "type": CALLOUT_NODE, "content": [] })],
    );

    let error = DocumentValidator::validate(&document, &schema, &limits())
        .expect_err("an empty non-table container is not the approved cardinality exception");

    assert_eq!(error.code, "DOCUMENT_INVALID");
    assert!(error.message.contains(CALLOUT_NODE), "{error:?}");
}

#[test]
fn an_empty_table_still_runs_attribute_checks() {
    let schema = tabled_schema_with_required_table_attribute();
    let missing = document_from(&schema, vec![table(vec![])]);

    let error = DocumentValidator::validate(&missing, &schema, &limits())
        .expect_err("the empty table still requires its declared attributes");
    assert_eq!(error.code, "REQUIRED_ATTRIBUTE_MISSING");

    let wrong_type = document_from(
        &schema,
        vec![json!({
            "type": TABLE_NODE,
            "attrs": { "gridId": 5 },
            "content": [],
        })],
    );
    let error = DocumentValidator::validate(&wrong_type, &schema, &limits())
        .expect_err("the empty table still type-checks its declared attributes");
    assert_eq!(error.code, "DOCUMENT_INVALID");
    assert!(error.message.contains("gridId"), "{error:?}");
}

#[test]
fn an_empty_table_still_runs_mark_checks_on_its_siblings() {
    let schema = schema();
    let marked_text = Node::text(
        "x".to_string(),
        vec![Mark::new(UNKNOWN_MARK_TYPE.to_string(), HashMap::new())],
    );
    let paragraph = Node::element(
        PARAGRAPH_NODE.to_string(),
        HashMap::new(),
        Fragment::from(vec![marked_text]),
    );
    let empty_table = Node::element(
        TABLE_NODE.to_string(),
        HashMap::new(),
        Fragment::from(Vec::new()),
    );
    let document = Document::new(Node::element(
        DOC_NODE.to_string(),
        HashMap::new(),
        Fragment::from(vec![empty_table, paragraph]),
    ));

    let error = DocumentValidator::validate(&document, &schema, &limits())
        .expect_err("unknown marks stay fatal beside an empty table");

    assert_eq!(error.code, "UNKNOWN_MARK");
}

#[test]
fn an_empty_table_still_runs_depth_checks() {
    let schema = schema();
    let document = document_from(&schema, vec![table(vec![])]);
    let shallow = ResourceLimits {
        max_document_depth: 1,
        ..limits()
    };

    let error = DocumentValidator::validate(&document, &schema, &shallow)
        .expect_err("the empty table is still charged against document depth");

    assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
    assert_eq!(error.limit, Some(1));
}

#[test]
fn irregular_geometry_is_admitted_and_left_untouched() {
    let schema = schema();
    let irregular = table(vec![
        row(vec![spanning_cell(2, 1)]),
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
    ]);
    let document = document_from(&schema, vec![irregular.clone()]);
    let before = document.clone();

    DocumentValidator::validate(&document, &schema, &limits())
        .expect("irregular geometry is admissible content");
    let index = index_for(&schema, &document);
    let projected = index
        .table_at(EMPTY_TABLE_POSITION)
        .expect("the irregular table is indexed");

    assert!(projected.irregular);
    assert_eq!(projected.rows, 2);
    assert_eq!(projected.columns, 3);
    assert_eq!(before.root(), document.root());
}

#[test]
fn overlong_rowspans_and_width_disagreements_stay_admissible() {
    let schema = schema();
    let overlong = table(vec![row(vec![spanning_cell(1, 9)])]);
    let disagreeing_widths = table(vec![
        row(vec![cell(
            json!({ "colspan": 1, "rowspan": 1, "colwidth": [100] }),
        )]),
        row(vec![cell(
            json!({ "colspan": 1, "rowspan": 1, "colwidth": [220] }),
        )]),
    ]);
    let document = document_from(&schema, vec![overlong, disagreeing_widths]);

    DocumentValidator::validate(&document, &schema, &limits())
        .expect("overlong spans and width disagreement are admissible");
    let index = index_for(&schema, &document);

    assert_eq!(index.len(), 2);
    let first = index
        .table_at(EMPTY_TABLE_POSITION)
        .expect("the overlong table is indexed");
    assert!(first.irregular);
    assert_eq!(first.rows, 1);
}

#[test]
fn missing_slots_are_reported_without_being_filled() {
    let schema = schema();
    let document = document_from(
        &schema,
        vec![table(vec![
            row(vec![plain_cell(), plain_cell()]),
            row(vec![plain_cell()]),
        ])],
    );

    let index = index_for(&schema, &document);
    let projected = index
        .table_at(EMPTY_TABLE_POSITION)
        .expect("the ragged table is indexed");

    assert!(projected.irregular);
    assert_eq!(projected.slots.len(), TWO_BY_TWO_SLOTS);
    assert_eq!(
        projected.slots.iter().filter(|slot| slot.is_none()).count(),
        1
    );
    assert_eq!(projected.cells.len(), 3);
}

#[test]
fn document_positions_address_every_table_and_its_cells() {
    let schema = schema();
    let document = document_from(&schema, vec![paragraph("hi"), regular_two_by_two()]);

    let index = index_for(&schema, &document);
    let projected = index
        .table_at(TABLE_AFTER_PARAGRAPH_POSITION)
        .expect("the trailing table is indexed at its document position");

    assert_eq!(
        index.positions().collect::<Vec<_>>(),
        vec![TABLE_AFTER_PARAGRAPH_POSITION]
    );
    assert!(!projected.irregular);
    assert_eq!(
        projected
            .cells
            .first()
            .expect("the table has cells")
            .source_pos,
        TABLE_AFTER_PARAGRAPH_POSITION + FIRST_CELL_OFFSET
    );
}

#[test]
fn nested_tables_are_projected_independently() {
    let schema = schema();
    let inner = table(vec![row(vec![plain_cell()])]);
    let outer = table(vec![row(vec![json!({
        "type": CELL_NODE,
        "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null },
        "content": [inner],
    })])]);
    let document = document_from(&schema, vec![outer]);

    let index = index_for(&schema, &document);

    assert_eq!(
        index.positions().collect::<Vec<_>>(),
        vec![
            EMPTY_TABLE_POSITION,
            EMPTY_TABLE_POSITION + NESTED_TABLE_OFFSET
        ]
    );
    assert!(index.irregular_positions().next().is_none());
}

#[test]
fn nested_irregular_tables_are_reported_at_their_own_positions() {
    let schema = schema();
    let inner = table(vec![
        row(vec![spanning_cell(2, 1)]),
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
    ]);
    let outer = table(vec![row(vec![json!({
        "type": CELL_NODE,
        "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null },
        "content": [inner],
    })])]);
    let document = document_from(&schema, vec![outer]);

    let index = index_for(&schema, &document);

    assert_eq!(
        index.irregular_positions().collect::<Vec<_>>(),
        vec![EMPTY_TABLE_POSITION + NESTED_TABLE_OFFSET]
    );
}

#[test]
fn invalid_span_domains_are_rejected_by_projection() {
    let schema = schema();
    for span in [json!(0), json!(-1), json!(1.5), json!("2")] {
        let document = document_from(
            &schema,
            vec![table(vec![row(vec![cell(
                json!({ "colspan": span, "rowspan": 1, "colwidth": null }),
            )])])],
        );

        assert_eq!(
            validate_table_shapes(&document, &schema, &limits()),
            Err(TableError::InvalidAttributes),
            "colspan {span} must not project"
        );
    }
}

#[test]
fn invalid_width_types_are_rejected_by_projection() {
    let schema = schema();
    for width in [json!("120"), json!(-4), json!({ "0": 120 })] {
        let document = document_from(
            &schema,
            vec![table(vec![row(vec![cell(
                json!({ "colspan": 1, "rowspan": 1, "colwidth": [width] }),
            )])])],
        );

        assert_eq!(
            validate_table_shapes(&document, &schema, &limits()),
            Err(TableError::InvalidAttributes),
        );
    }
}

#[test]
fn the_grid_budget_is_charged_across_every_table_in_the_document() {
    let schema = schema();
    let document = document_from(&schema, vec![regular_two_by_two(), regular_two_by_two()]);
    let exact = ResourceLimits {
        max_table_grid_slots: TWO_BY_TWO_SLOTS * 2,
        ..limits()
    };
    let one_short = ResourceLimits {
        max_table_grid_slots: TWO_BY_TWO_SLOTS * 2 - 1,
        ..limits()
    };

    assert!(validate_table_shapes(&document, &schema, &exact).is_ok());
    assert_eq!(
        validate_table_shapes(&document, &schema, &one_short),
        Err(TableError::GridLimit {
            limit: TWO_BY_TWO_SLOTS * 2 - 1,
            actual: TWO_BY_TWO_SLOTS * 2,
        })
    );
}

#[test]
fn schemas_without_table_roles_index_nothing() {
    let schema = crate::schema::presets::prosemirror_schema();
    let document = from_prosemirror_json(
        &json!({ "type": DOC_NODE, "content": [paragraph("plain")] }),
        &schema,
        UnknownTypeMode::Preserve,
    )
    .expect("the plain fixture parses");

    let index = validate_table_shapes(&document, &schema, &limits())
        .expect("a schema without table roles projects nothing");

    assert_eq!(index.len(), 0);
    assert!(!index.projection_failed());
}

#[test]
fn a_failed_projection_falls_back_to_absent_geometry() {
    let schema = schema();
    let document = document_from(&schema, vec![regular_two_by_two()]);
    let starved = ResourceLimits {
        max_table_grid_slots: 1,
        ..limits()
    };

    let index = TableProjectionIndex::derive_or_fallback(&document, &schema, &starved);

    assert!(index.projection_failed());
    assert_eq!(index.len(), 0);
    assert!(index.table_at(EMPTY_TABLE_POSITION).is_none());
    assert!(index.irregular_positions().next().is_none());
}

fn tabled_schema_with_callout() -> Schema {
    Schema::from_json(&json!({
        "nodes": [
            { "name": DOC_NODE, "content": "block+", "role": "doc" },
            { "name": PARAGRAPH_NODE, "content": "inline*", "group": "block", "role": "textBlock" },
            { "name": "text", "content": "", "group": "inline", "role": "text" },
            { "name": CALLOUT_NODE, "content": "block+", "group": "block", "role": "block" },
            {
                "name": TABLE_NODE,
                "content": format!("{ROW_NODE}+"),
                "group": "block",
                "role": "block",
                "tableRole": "table"
            },
            {
                "name": ROW_NODE,
                "content": format!("({CELL_NODE} | table_header)*"),
                "role": "block",
                "tableRole": "row"
            },
            {
                "name": CELL_NODE,
                "content": "block+",
                "role": "block",
                "tableRole": "cell",
                "attrs": cell_attrs()
            },
            {
                "name": "table_header",
                "content": "block+",
                "role": "block",
                "tableRole": "header_cell",
                "attrs": cell_attrs()
            }
        ],
        "marks": []
    }))
    .expect("the callout tabled schema is valid")
}

fn tabled_schema_with_required_table_attribute() -> Schema {
    Schema::from_json(&json!({
        "nodes": [
            { "name": DOC_NODE, "content": "block+", "role": "doc" },
            { "name": PARAGRAPH_NODE, "content": "inline*", "group": "block", "role": "textBlock" },
            { "name": "text", "content": "", "group": "inline", "role": "text" },
            {
                "name": TABLE_NODE,
                "content": format!("{ROW_NODE}+"),
                "group": "block",
                "role": "block",
                "tableRole": "table",
                "attrs": { "gridId": { "type": "string" } }
            },
            {
                "name": ROW_NODE,
                "content": format!("({CELL_NODE} | table_header)*"),
                "role": "block",
                "tableRole": "row"
            },
            {
                "name": CELL_NODE,
                "content": "block+",
                "role": "block",
                "tableRole": "cell",
                "attrs": cell_attrs()
            },
            {
                "name": "table_header",
                "content": "block+",
                "role": "block",
                "tableRole": "header_cell",
                "attrs": cell_attrs()
            }
        ],
        "marks": []
    }))
    .expect("the required-attribute tabled schema is valid")
}

fn cell_attrs() -> Value {
    json!({
        "colspan": { "type": "number", "default": 1, "min": 1 },
        "rowspan": { "type": "number", "default": 1, "min": 1 },
        "colwidth": { "default": null }
    })
}

#[test]
fn the_tabled_presets_resolve_their_roles() {
    for schema in [
        crate::schema::presets::prosemirror_table_schema(),
        crate::schema::presets::tiptap_table_schema(),
    ] {
        crate::tables::TableRoles::resolve(&schema)
            .expect("the tabled preset declares valid roles");
    }
}

#[test]
fn integral_float_spans_and_widths_from_yjs_project_like_integers() {
    let schema = crate::schema::presets::prosemirror_table_schema();
    let document = from_prosemirror_json(
        &json!({"type":"doc","content":[{"type":"table","content":[
            {"type":"table_row","content":[
                {"type":"table_cell","attrs":{"colspan":1.0,"rowspan":1.0,"colwidth":[120.0]},
                 "content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]}]}]}]}),
        &schema,
        UnknownTypeMode::Preserve,
    )
    .expect("the web payload parses");

    DocumentValidator::validate(&document, &schema, &limits()).expect("document validates");
    let index =
        validate_table_shapes(&document, &schema, &limits()).expect("the web payload projects");
    let projected = index
        .table_at(EMPTY_TABLE_POSITION)
        .expect("the table is indexed");
    assert_eq!(projected.columns, 1);
    assert_eq!(projected.widths, vec![Some(120)]);

    let fractional = from_prosemirror_json(
        &json!({"type":"doc","content":[{"type":"table","content":[
            {"type":"table_row","content":[
                {"type":"table_cell","attrs":{"colspan":1.5,"rowspan":1.0},
                 "content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]}]}]}]}),
        &schema,
        UnknownTypeMode::Preserve,
    )
    .expect("the fractional payload parses");
    assert_eq!(
        validate_table_shapes(&fractional, &schema, &limits()),
        Err(TableError::InvalidAttributes),
    );
}
