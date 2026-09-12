use serde_json::json;

use crate::boundary::{ResourceLimits, DEFAULT_MAX_TABLE_GRID_SLOTS, HARD_MAX_TABLE_GRID_SLOTS};
use crate::schema::presets::{prosemirror_schema, tiptap_schema};
use crate::schema::Schema;
use crate::tables::types::TableError;
use crate::tables::{TableRole, TableRoles};

const INVALID_STRUCTURE_MESSAGE: &str =
    "schema table roles are invalid: table structure is invalid";
const INVALID_ATTRIBUTES_MESSAGE: &str =
    "schema table roles are invalid: table attributes are invalid";

pub(crate) const PROSEMIRROR_TABLE_NAMES: [&str; 4] =
    ["table", "table_row", "table_cell", "table_header"];
const TIPTAP_TABLE_NAMES: [&str; 4] = ["table", "tableRow", "tableCell", "tableHeader"];

fn cell_attrs() -> serde_json::Value {
    json!({
        "colspan": { "type": "number", "default": 1, "min": 1 },
        "rowspan": { "type": "number", "default": 1, "min": 1 },
        "colwidth": { "default": null }
    })
}

fn base_nodes() -> Vec<serde_json::Value> {
    vec![
        json!({ "name": "doc", "content": "block+", "role": "doc" }),
        json!({ "name": "paragraph", "content": "inline*", "group": "block", "role": "textBlock" }),
        json!({ "name": "text", "content": "", "group": "inline", "role": "text" }),
    ]
}

fn table_nodes(names: [&str; 4], row_content: &str) -> Vec<serde_json::Value> {
    let [table, row, cell, header] = names;
    vec![
        json!({
            "name": table,
            "content": format!("{row}+"),
            "group": "block",
            "role": "block",
            "tableRole": "table"
        }),
        json!({
            "name": row,
            "content": row_content,
            "role": "block",
            "tableRole": "row"
        }),
        json!({
            "name": cell,
            "content": "block+",
            "role": "block",
            "tableRole": "cell",
            "attrs": cell_attrs()
        }),
        json!({
            "name": header,
            "content": "block+",
            "role": "block",
            "tableRole": "header_cell",
            "attrs": cell_attrs()
        }),
    ]
}

fn schema_json(nodes: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "nodes": nodes, "marks": [] })
}

fn tabled_schema_json(names: [&str; 4]) -> serde_json::Value {
    let [_, _, cell, header] = names;
    let mut nodes = base_nodes();
    nodes.extend(table_nodes(names, &format!("({cell} | {header})*")));
    schema_json(nodes)
}

pub(crate) fn tabled_schema(names: [&str; 4]) -> Schema {
    Schema::from_json(&tabled_schema_json(names)).expect("tabled schema is valid")
}

pub(crate) const SECOND_TEXT_BLOCK_NODE: &str = "heading";

pub(crate) fn tabled_schema_with_second_text_block(names: [&str; 4]) -> Schema {
    let mut json = tabled_schema_json(names);
    let nodes = json["nodes"]
        .as_array_mut()
        .expect("the tabled schema lists nodes");
    nodes.push(json!({
        "name": SECOND_TEXT_BLOCK_NODE,
        "content": "inline*",
        "group": "block",
        "role": "textBlock",
    }));
    Schema::from_json(&json).expect("the second text block schema is valid")
}

#[test]
fn schemas_without_table_metadata_resolve_to_no_roles() {
    for schema in [prosemirror_schema(), tiptap_schema()] {
        assert_eq!(TableRoles::resolve(&schema), Ok(None));
    }
}

#[test]
fn both_naming_presets_resolve_their_four_roles() {
    for names in [PROSEMIRROR_TABLE_NAMES, TIPTAP_TABLE_NAMES] {
        let schema = tabled_schema(names);
        let roles = TableRoles::resolve(&schema)
            .expect("table roles resolve")
            .expect("table roles are present");

        assert_eq!(roles.table, names[0]);
        assert_eq!(roles.row, names[1]);
        assert_eq!(roles.cell, names[2]);
        assert_eq!(roles.header_cell, names[3]);
    }
}

#[test]
fn custom_role_names_resolve_when_unique() {
    let names = ["grid", "gridRow", "gridCell", "gridHeader"];
    let roles = TableRoles::resolve(&tabled_schema(names))
        .expect("table roles resolve")
        .expect("table roles are present");

    assert_eq!(roles.row, "gridRow");
    assert_eq!(roles.header_cell, "gridHeader");
}

#[test]
fn a_partial_set_of_table_roles_is_rejected() {
    let mut nodes = base_nodes();
    nodes.push(json!({
        "name": "table",
        "content": "block+",
        "group": "block",
        "role": "block",
        "tableRole": "table"
    }));

    let error = Schema::from_json(&schema_json(nodes)).expect_err("incomplete roles are invalid");
    assert_eq!(error, INVALID_STRUCTURE_MESSAGE);
}

#[test]
fn a_duplicated_table_role_is_rejected() {
    let mut nodes = base_nodes();
    nodes.extend(table_nodes(
        PROSEMIRROR_TABLE_NAMES,
        "(table_cell | table_header)*",
    ));
    nodes.push(json!({
        "name": "second_cell",
        "content": "block+",
        "role": "block",
        "tableRole": "cell"
    }));

    let error = Schema::from_json(&schema_json(nodes)).expect_err("duplicate roles are invalid");
    assert_eq!(error, INVALID_STRUCTURE_MESSAGE);
}

#[test]
fn an_unrecognized_table_role_is_dropped() {
    let mut nodes = base_nodes();
    nodes.push(json!({
        "name": "oddity",
        "content": "block+",
        "group": "block",
        "role": "block",
        "tableRole": "footer"
    }));

    let schema = Schema::from_json(&schema_json(nodes)).expect("unknown roles are ignored");
    assert_eq!(TableRoles::resolve(&schema), Ok(None));
    assert_eq!(schema.node("oddity").unwrap().table_role, None);
}

#[test]
fn a_row_content_rule_that_requires_a_cell_is_rejected() {
    let mut nodes = base_nodes();
    nodes.extend(table_nodes(
        PROSEMIRROR_TABLE_NAMES,
        "(table_cell | table_header)+",
    ));

    let error = Schema::from_json(&schema_json(nodes))
        .expect_err("a row must admit being fully covered by spans");
    assert_eq!(error, INVALID_STRUCTURE_MESSAGE);
}

#[test]
fn a_row_accepts_zero_cells_and_both_cell_kinds() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let row = schema.node("table_row").expect("row node");
    let empty: [&str; 0] = [];

    assert!(row
        .content
        .matches(&empty, |child, symbol| *child == symbol));
    assert!(row
        .content
        .matches(&["table_cell", "table_header"], |child, symbol| *child
            == symbol));
}

#[test]
fn a_table_requires_at_least_one_row() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let table = schema.node("table").expect("table node");
    let empty: [&str; 0] = [];

    assert!(!table
        .content
        .matches(&empty, |child, symbol| *child == symbol));
    assert!(table
        .content
        .matches(&["table_row"], |child, symbol| *child == symbol));
}

#[test]
fn cells_without_span_attributes_are_rejected() {
    let mut nodes = base_nodes();
    let mut table = table_nodes(PROSEMIRROR_TABLE_NAMES, "(table_cell | table_header)*");
    table[2]["attrs"] = json!({});
    nodes.extend(table);

    let error = Schema::from_json(&schema_json(nodes)).expect_err("cells declare span attributes");
    assert_eq!(error, INVALID_ATTRIBUTES_MESSAGE);
}

#[test]
fn table_roles_serialize_in_snake_case() {
    for (role, expected) in [
        (TableRole::Table, "table"),
        (TableRole::Row, "row"),
        (TableRole::Cell, "cell"),
        (TableRole::HeaderCell, "header_cell"),
    ] {
        assert_eq!(serde_json::to_value(role).unwrap(), json!(expected));
        assert_eq!(role.as_str(), expected);
        assert_eq!(TableRole::from_schema_name(expected), Some(role));
    }

    assert_eq!(TableRole::from_schema_name("headerCell"), None);
}

#[test]
fn table_errors_describe_their_cause() {
    let messages = [
        TableError::InvalidStructure.to_string(),
        TableError::InvalidAttributes.to_string(),
        TableError::GridLimit {
            limit: DEFAULT_MAX_TABLE_GRID_SLOTS,
            actual: DEFAULT_MAX_TABLE_GRID_SLOTS + 1,
        }
        .to_string(),
        TableError::WorkLimit.to_string(),
        TableError::Allocation.to_string(),
    ];

    assert!(messages[2].contains("25000"), "unexpected: {}", messages[2]);
    assert!(messages[2].contains("25001"), "unexpected: {}", messages[2]);

    let mut unique = messages.to_vec();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), messages.len());
}

#[test]
fn grid_slot_limits_accept_the_default_and_the_hard_ceiling() {
    for value in [1, DEFAULT_MAX_TABLE_GRID_SLOTS, HARD_MAX_TABLE_GRID_SLOTS] {
        let limits =
            ResourceLimits::try_from_config(Some(&json!({ "maxTableGridSlots": value }))).unwrap();
        assert_eq!(limits.max_table_grid_slots, value);
    }

    assert_eq!(
        ResourceLimits::default().max_table_grid_slots,
        DEFAULT_MAX_TABLE_GRID_SLOTS
    );
}

#[test]
fn grid_slot_limits_reject_zero_and_one_past_the_ceiling() {
    for value in [0, HARD_MAX_TABLE_GRID_SLOTS + 1] {
        let error = ResourceLimits::try_from_config(Some(&json!({ "maxTableGridSlots": value })))
            .expect_err("grid slot limit is out of range");

        assert_eq!(error.code(), "INVALID_RESOURCE_LIMIT");
        assert_eq!(error.limit, Some(HARD_MAX_TABLE_GRID_SLOTS));
        assert_eq!(error.actual, Some(value));
        assert_eq!(
            error.details,
            Some(json!({ "field": "maxTableGridSlots" })),
            "unexpected details for {value}"
        );
    }
}

#[test]
fn grid_slot_limits_stay_independent_of_node_byte_and_depth_limits() {
    let defaults = ResourceLimits::default();
    let limits = ResourceLimits::try_from_config(Some(
        &json!({ "maxTableGridSlots": HARD_MAX_TABLE_GRID_SLOTS }),
    ))
    .unwrap();

    assert_eq!(limits.max_document_nodes, defaults.max_document_nodes);
    assert_eq!(limits.max_document_depth, defaults.max_document_depth);
    assert_eq!(limits.max_schema_nodes, defaults.max_schema_nodes);
    assert_eq!(limits.max_input_bytes, defaults.max_input_bytes);
    assert_eq!(
        limits.max_schema_expression_bytes,
        defaults.max_schema_expression_bytes
    );
}

const TABLES_FREE_BASE_FINGERPRINT: &str =
    "65d5fcdf50178691d184b3f80befe7272a6144d4c4505b77fa309a50736431e3";

#[test]
fn table_roles_are_absent_from_tables_free_fingerprints() {
    let tables_free = Schema::from_json(&schema_json(base_nodes())).unwrap();
    let tabled = tabled_schema(PROSEMIRROR_TABLE_NAMES);

    assert_eq!(
        crate::schema::schema_fingerprint(&tables_free),
        TABLES_FREE_BASE_FINGERPRINT
    );
    assert_ne!(
        crate::schema::schema_fingerprint(&tabled),
        TABLES_FREE_BASE_FINGERPRINT
    );
}
