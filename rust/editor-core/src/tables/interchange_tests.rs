use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::model::Document;
use crate::schema::presets::prosemirror_table_schema;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::serialize::html_in::{from_html_with_limits, FromHtmlOptions};
use crate::serialize::html_out::TABLE_COLWIDTH_SEPARATOR;
use crate::serialize::{to_html, to_prosemirror_json};
use crate::tables::admission::TableProjectionIndex;
use crate::tables::interchange::{table_clipboard_fragment, InterchangeFailure};
use crate::tables::normalize_tests::{cell, cell_with, header_cell, limits, row, table};
use crate::yrs_engine::{
    EditingLimits, InitializationMode, ReplacementHistory, TransactionOrigin, YrsDocumentEngine,
    YrsEngineConfig,
};

const TABLE_NODE: &str = "table";
const HEADER_CELL_NODE: &str = "table_header";
const PARAGRAPH_NODE: &str = "paragraph";
const SINGLE_SPAN: u32 = 1;
const MALFORMED_COLUMN_WIDTHS: &str = "100,abc";
const UNSET_COLUMN_WIDTHS: &str = "0,";
const DECLARED_COLUMN_WIDTH: u32 = 100;
const DOUBLE_SPAN: u32 = 2;
const CELL_INTERIOR: u32 = 2;
const ROW_NODE: &str = "table_row";
const CELL_NODE: &str = "table_cell";
const OVERSIZED_ROWSPAN: u32 = 3;

const REPLACEMENT_REQUEST_ID: u64 = 23;
const FRAGMENT_NAME: &str = "prosemirror";

fn nesting_cell() -> serde_json::Value {
    json!({
        "type": "table_cell",
        "attrs": { "colspan": SINGLE_SPAN, "rowspan": SINGLE_SPAN, "colwidth": serde_json::Value::Null },
        "content": [
            { "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "outer" }] },
            table(vec![row(vec![cell("inner")])]),
        ],
    })
}

fn replacement_engine() -> YrsDocumentEngine {
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema: schema(),
        fragment_name: FRAGMENT_NAME.into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: limits(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .expect("the tabled engine initializes");
    engine
        .import_json(
            &json!({ "type": "doc", "content": [{ "type": PARAGRAPH_NODE }] }).to_string(),
            TransactionOrigin::DocumentImport,
        )
        .expect("the empty fixture imports");
    engine
}

fn schema() -> Schema {
    prosemirror_table_schema()
}

fn import(html: &str) -> Document {
    from_html_with_limits(
        html,
        &schema(),
        &FromHtmlOptions::default(),
        &ResourceLimits::default(),
    )
    .expect("the interchange fixture imports")
}

fn document_with(content: Vec<serde_json::Value>) -> Document {
    crate::serialize::json_in::from_prosemirror_json(
        &json!({ "type": "doc", "content": content }),
        &schema(),
        crate::serialize::UnknownTypeMode::Error,
    )
    .expect("the interchange fixture parses")
}

#[test]
fn a_browser_table_round_trips_through_sections_spans_and_column_widths() {
    let source = concat!(
        "<table>",
        "<colgroup><col style=\"width: 100px\"><col></colgroup>",
        "<thead><tr><th colspan=\"2\" data-colwidth=\"100,140\"><p>head</p></th></tr></thead>",
        "<tbody><tr><td rowspan=\"2\" data-colwidth=\"100\"><p>tall</p></td>",
        "<td data-colwidth=\"140\"><p>a</p></td></tr>",
        "<tr><td data-colwidth=\"140\"><p>b</p></td></tr></tbody>",
        "<tfoot><tr><td data-colwidth=\"100\"><p>f1</p></td>",
        "<td data-colwidth=\"140\"><p>f2</p></td></tr></tfoot>",
        "</table>"
    );
    let imported = import(source);
    let expected = document_with(vec![table(vec![
        row(vec![json!({
            "type": HEADER_CELL_NODE,
            "attrs": { "colspan": 2, "rowspan": SINGLE_SPAN, "colwidth": [100, 140] },
            "content": [{ "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "head" }] }],
        })]),
        row(vec![
            cell_with(SINGLE_SPAN, 2, json!([100]), "tall"),
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "a"),
        ]),
        row(vec![cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "b")]),
        row(vec![
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([100]), "f1"),
            cell_with(SINGLE_SPAN, SINGLE_SPAN, json!([140]), "f2"),
        ]),
    ])]);
    assert_eq!(
        to_prosemirror_json(&imported, &schema()),
        to_prosemirror_json(&expected, &schema()),
        "thead/tbody/tfoot must be transparent, th must keep its header role, and \
         data-colwidth must decode to the colwidth array",
    );

    let exported = to_html(&imported, &schema());
    assert!(
        exported.starts_with("<table><tbody><tr>"),
        "exported table rows must sit in a tbody: {exported}",
    );
    assert!(
        exported.contains("data-colwidth=\"100,140\""),
        "the colwidth array must export as a standard data-colwidth list: {exported}",
    );
    assert_eq!(
        to_prosemirror_json(&import(&exported), &schema()),
        to_prosemirror_json(&imported, &schema()),
        "exported table HTML must reimport to the same document",
    );
}

#[test]
fn an_imported_row_never_gains_fabricated_block_content() {
    let imported = import("<table><tbody><tr></tr><tr><td></td></tr></tbody></table>");
    let json = to_prosemirror_json(&imported, &schema());
    let rows = json["content"][0]["content"].as_array().expect("rows");
    assert_eq!(rows.len(), 2, "both rows survive: {json}");
    assert!(
        rows[0].get("content").is_none(),
        "an empty row must stay empty instead of gaining a paragraph: {json}",
    );
    assert_eq!(
        rows[1]["content"][0]["content"][0]["type"], PARAGRAPH_NODE,
        "an empty cell still gains the schema's required block content: {json}",
    );
}

#[test]
fn a_nested_table_survives_import_and_export_as_cell_content() {
    let source = concat!(
        "<table><tbody><tr><td>",
        "<table><tbody><tr><td><p>inner</p></td></tr></tbody></table>",
        "</td></tr></tbody></table>"
    );
    let imported = import(source);
    let json = to_prosemirror_json(&imported, &schema());
    let inner = &json["content"][0]["content"][0]["content"][0]["content"][0];
    assert_eq!(
        inner["type"], TABLE_NODE,
        "the inner table must be preserved as cell content: {json}",
    );
    assert_eq!(
        inner["content"][0]["content"][0]["content"][0]["content"][0]["text"], "inner",
        "the inner table's text must be preserved: {json}",
    );
    let exported = to_html(&imported, &schema());
    assert_eq!(
        exported,
        concat!(
            "<table><tbody><tr><td colspan=\"1\" rowspan=\"1\">",
            "<table><tbody><tr><td colspan=\"1\" rowspan=\"1\"><p>inner</p></td></tr></tbody></table>",
            "</td></tr></tbody></table>"
        ),
        "a nested table must export as a nested table",
    );
}

#[test]
fn a_column_group_carries_no_model_state() {
    let imported = import(
        "<table><colgroup><col><col></colgroup><tbody><tr><td><p>x</p></td></tr></tbody></table>",
    );
    assert_eq!(
        to_prosemirror_json(&imported, &schema()),
        to_prosemirror_json(
            &import("<table><tbody><tr><td><p>x</p></td></tr></tbody></table>"),
            &schema()
        ),
        "a colgroup must not reach the model at all",
    );
}

#[test]
fn copying_a_rectangle_over_a_projected_hole_mints_a_cell_only_in_the_copy() {
    let document = document_with(vec![table(vec![
        row(vec![cell("a"), cell("b")]),
        row(vec![cell("c")]),
    ])]);
    let before = to_prosemirror_json(&document, &schema());
    let index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &limits());
    let projected = index.table_at(0).expect("the fixture projects");
    assert!(
        projected.irregular,
        "the fixture must actually hold a projected hole",
    );
    let openings: Vec<u32> = projected.cells.iter().map(|cell| cell.source_pos).collect();
    let fragment = table_clipboard_fragment(
        &document,
        &Selection::cell(openings[1], openings[2]),
        &index,
        &schema(),
    )
    .expect("a cell rectangle copies");

    let copied = Document::new(crate::model::Node::element(
        document.root().node_type().into(),
        Default::default(),
        fragment,
    ));
    let json = to_prosemirror_json(&copied, &schema());
    let rows = json["content"][0]["content"].as_array().expect("rows");
    assert_eq!(rows.len(), 2, "the copy spans both rows: {json}");
    assert_eq!(
        rows[1]["content"].as_array().expect("second row").len(),
        2,
        "the projected hole must become a real cell in the copy: {json}",
    );
    assert_eq!(
        rows[1]["content"][1]["content"][0]["type"], PARAGRAPH_NODE,
        "the minted cell carries the schema's default block: {json}",
    );
    assert!(
        rows[1]["content"][1].get("content").is_some()
            && rows[1]["content"][1]["content"][0].get("content").is_none(),
        "the minted cell is empty: {json}",
    );
    assert_eq!(
        to_prosemirror_json(&document, &schema()),
        before,
        "a read-only copy must never mutate the source document",
    );
}

#[test]
fn a_text_selection_is_never_a_table_clipboard_fragment() {
    let document = document_with(vec![table(vec![row(vec![cell("a")])])]);
    let index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &limits());
    assert_eq!(
        table_clipboard_fragment(&document, &Selection::text(3, 4), &index, &schema()),
        Err(InterchangeFailure::NotACellRectangle),
        "only a cell rectangle produces a table clipboard fragment",
    );
}

#[test]
fn copying_a_merged_rectangle_keeps_its_spans_and_header_roles() {
    let document = document_with(vec![table(vec![
        row(vec![header_cell("h1"), header_cell("h2")]),
        row(vec![cell_with(2, SINGLE_SPAN, json!([100, 140]), "wide")]),
    ])]);
    let index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &limits());
    let openings: Vec<u32> = index
        .table_at(0)
        .expect("the fixture projects")
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .collect();
    let fragment = table_clipboard_fragment(
        &document,
        &Selection::cell(openings[0], openings[2]),
        &index,
        &schema(),
    )
    .expect("a cell rectangle copies");
    let copied = Document::new(crate::model::Node::element(
        document.root().node_type().into(),
        Default::default(),
        fragment,
    ));
    let json = to_prosemirror_json(&copied, &schema());
    assert_eq!(
        json["content"][0]["content"][0]["content"][0]["type"], HEADER_CELL_NODE,
        "header cells stay header cells in the copy: {json}",
    );
    assert_eq!(
        json["content"][0]["content"][1]["content"][0]["attrs"]["colspan"], 2,
        "a merged cell keeps its colspan in the copy: {json}",
    );
    assert_eq!(
        json["content"][0]["content"][1]["content"]
            .as_array()
            .expect("merged row")
            .len(),
        1,
        "a merged cell is copied once, not once per covered slot: {json}",
    );
    assert_eq!(
        to_html(&copied, &schema()),
        concat!(
            "<table><tbody>",
            "<tr><th colspan=\"1\" rowspan=\"1\"><p>h1</p></th><th colspan=\"1\" rowspan=\"1\"><p>h2</p></th></tr>",
            "<tr><td colspan=\"2\" data-colwidth=\"100,140\" rowspan=\"1\"><p>wide</p></td></tr>",
            "</tbody></table>"
        ),
        "the copied rectangle serializes as a self-contained table",
    );
}

#[test]
fn an_authored_replacement_normalizes_its_outer_tables_and_a_restore_keeps_them_raw() {
    let irregular = json!({ "type": "doc", "content": [table(vec![
        row(vec![cell("a"), cell("b")]),
        row(vec![nesting_cell()]),
    ])] })
    .to_string();

    for (history, expected_second_row_cells, reason) in [
        (
            ReplacementHistory::UndoableBoundary,
            2,
            "a user-authored replacement owns its new outer table and normalizes it",
        ),
        (
            ReplacementHistory::ResetAndClear,
            1,
            "a restore carries geometry this engine did not author and must stay raw",
        ),
    ] {
        let mut engine = replacement_engine();
        engine
            .prepare_root_replacement_json(REPLACEMENT_REQUEST_ID, &irregular, history)
            .expect("the replacement commits");
        let json = engine.document_json().expect("the engine is ready");
        let second_row = &json["content"][0]["content"][1];
        assert_eq!(
            second_row["content"]
                .as_array()
                .expect("the second row holds cells")
                .len(),
            expected_second_row_cells,
            "{reason}: {json}",
        );
        assert_eq!(
            second_row["content"][0]["content"][1]["content"][0]["content"]
                .as_array()
                .expect("the nested row holds cells")
                .len(),
            1,
            "a nested descendant is never normalized by its owner's replacement: {json}",
        );
    }
}

#[test]
fn a_malformed_column_width_list_is_distinguishable_from_a_deliberately_unset_one() {
    let malformed = format!(
        "<table><tbody><tr><td data-colwidth=\"{MALFORMED_COLUMN_WIDTHS}\"><p>x</p></td></tr></tbody></table>"
    );
    let unset = format!(
        "<table><tbody><tr><td data-colwidth=\"{UNSET_COLUMN_WIDTHS}\"><p>x</p></td></tr></tbody></table>"
    );

    let strict = FromHtmlOptions {
        strict: true,
        ..FromHtmlOptions::default()
    };
    let refusal = from_html_with_limits(&malformed, &schema(), &strict, &ResourceLimits::default())
        .expect_err("a malformed column width list must be refused in strict mode");
    assert!(
        matches!(
            &refusal,
            crate::serialize::html_in::ParseError::InvalidAttribute { attr, value }
                if attr == "data-colwidth" && value == MALFORMED_COLUMN_WIDTHS
        ),
        "the refusal must name the attribute and the value it could not decode: {refusal:?}",
    );
    assert!(
        from_html_with_limits(&unset, &schema(), &strict, &ResourceLimits::default()).is_ok(),
        "a deliberately unset list is not malformed and must still import in strict mode",
    );

    let cell_attrs = |html: &str| {
        to_prosemirror_json(&import(html), &schema())["content"][0]["content"][0]["content"][0]
            .get("attrs")
            .cloned()
    };
    assert_eq!(
        cell_attrs(&malformed),
        cell_attrs(&unset),
        "outside strict mode both drop the attribute rather than inventing a width",
    );
    let declared = format!(
        "<table><tbody><tr><td data-colwidth=\"{DECLARED_COLUMN_WIDTH}\"><p>x</p></td></tr></tbody></table>"
    );
    assert_eq!(
        cell_attrs(&declared),
        Some(json!({ "colwidth": [DECLARED_COLUMN_WIDTH] })),
        "a well-formed list is kept, so dropping the malformed one is a real distinction",
    );
}

#[test]
fn a_partially_specified_column_width_keeps_every_slot() {
    let cell_with_widths = |widths: serde_json::Value| {
        json!({
            "type": HEADER_CELL_NODE,
            "attrs": { "colspan": 2, "rowspan": SINGLE_SPAN, "colwidth": widths },
            "content": [{ "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "h" }] }],
        })
    };
    for (widths, expected) in [
        (json!([0, 140]), "0,140"),
        (json!([100, 0]), "100,0"),
        (json!([Value::Null, 140]), "0,140"),
        (json!([100, Value::Null]), "100,0"),
        (json!([100, 140]), "100,140"),
    ] {
        let document = document_with(vec![table(vec![row(vec![cell_with_widths(
            widths.clone(),
        )])])]);
        let exported = to_html(&document, &schema());
        assert!(
            exported.contains(&format!("data-colwidth=\"{expected}\"")),
            "{widths} must export every slot as {expected}: {exported}",
        );
        let slices: Vec<&str> = expected.split(TABLE_COLWIDTH_SEPARATOR).collect();
        assert_eq!(
            slices.len(),
            DOUBLE_SPAN as usize,
            "the pinned parser keeps a width list only when its length equals colspan: {exported}",
        );
        assert!(
            slices.iter().all(|slice| slice.parse::<u32>().is_ok()),
            "the pinned parser rejects any list with a non digit slice: {exported}",
        );
        assert_eq!(
            to_html(&import(&exported), &schema()),
            exported,
            "{widths} must reimport to a document that exports the same slots",
        );
    }
}

#[test]
fn copying_a_cell_whose_declared_span_exceeds_the_grid_yields_the_effective_span() {
    let document = document_with(vec![table(vec![row(vec![cell_with(
        SINGLE_SPAN,
        OVERSIZED_ROWSPAN,
        Value::Null,
        "a",
    )])])]);
    let before = to_prosemirror_json(&document, &schema());
    let index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &limits());
    let projected = index.table_at(0).expect("the fixture projects");
    assert_eq!(
        projected.cells[0].rect.rowspan, SINGLE_SPAN,
        "the projection clamps the declared rowspan to the one row that exists",
    );
    let opening = projected.cells[0].source_pos;

    let fragment = table_clipboard_fragment(
        &document,
        &Selection::cell(opening, opening),
        &index,
        &schema(),
    )
    .expect("a cell rectangle copies");
    let copied = Document::new(crate::model::Node::element(
        document.root().node_type().into(),
        Default::default(),
        fragment,
    ));
    let json = to_prosemirror_json(&copied, &schema());
    let copied_index = TableProjectionIndex::derive_or_fallback(&copied, &schema(), &limits());
    let copied_table = copied_index.table_at(0).expect("the copy projects");
    assert_eq!(
        copied_table.cells[0].rect.rowspan, SINGLE_SPAN,
        "the copy must carry the effective span, not the raw one that overruns it: {json}",
    );
    assert!(
        !copied_table.irregular,
        "a copied rectangle must be a valid table on its own: {json}",
    );
    assert_eq!(
        to_prosemirror_json(&document, &schema()),
        before,
        "the source keeps its raw attributes untouched",
    );
}

#[test]
fn a_clipboard_refusal_reports_the_kind_of_refusal_it_is() {
    let document = document_with(vec![table(vec![row(vec![cell("a")])])]);
    let index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &limits());
    assert_eq!(
        crate::clipboard::export_cells(&document, &Selection::text(3, 4), &index, &schema()),
        Err(InterchangeFailure::NotACellRectangle),
        "a selection that is not a rectangle is refused as such, not as an unreadable grid",
    );
}

fn schema_admitting_a_stray_table_child() -> Schema {
    let cell_attrs = json!({
        "colspan": { "type": "number", "default": SINGLE_SPAN, "min": SINGLE_SPAN },
        "rowspan": { "type": "number", "default": SINGLE_SPAN, "min": SINGLE_SPAN },
        "colwidth": { "default": Value::Null }
    });
    Schema::from_json(&json!({ "nodes": [
        { "name": "doc", "content": "block+", "role": "doc" },
        { "name": PARAGRAPH_NODE, "content": "inline*", "group": "block", "role": "textBlock", "htmlTag": "p" },
        { "name": "text", "content": "", "group": "inline", "role": "text" },
        {
            "name": TABLE_NODE,
            "content": format!("({ROW_NODE} | {PARAGRAPH_NODE})+"),
            "group": "block",
            "role": "block",
            "tableRole": "table",
            "htmlTag": "table"
        },
        { "name": ROW_NODE, "content": format!("({CELL_NODE} | {HEADER_CELL_NODE})*"), "role": "block", "tableRole": "row", "htmlTag": "tr" },
        { "name": CELL_NODE, "content": "block+", "role": "block", "tableRole": "cell", "htmlTag": "td", "attrs": cell_attrs },
        { "name": HEADER_CELL_NODE, "content": "block+", "role": "block", "tableRole": "header_cell", "htmlTag": "th", "attrs": cell_attrs },
    ], "marks": [] }))
    .expect("a table that also accepts a paragraph child resolves its roles")
}

#[test]
fn a_stray_table_child_reports_an_unreadable_grid_to_the_host() {
    let schema = schema_admitting_a_stray_table_child();
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema: schema.clone(),
        fragment_name: FRAGMENT_NAME.into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: limits(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .expect("the engine initializes");
    engine
        .import_json(
            &json!({ "type": "doc", "content": [{
                "type": TABLE_NODE,
                "content": [
                    { "type": ROW_NODE, "content": [cell("a"), cell("b")] },
                    { "type": PARAGRAPH_NODE, "content": [{ "type": "text", "text": "stray" }] },
                ],
            }] })
            .to_string(),
            TransactionOrigin::DocumentImport,
        )
        .expect("a table holding a stray paragraph is admitted by this schema");

    let document = engine.document().expect("the engine is ready");
    let index = TableProjectionIndex::derive_or_fallback(document, &schema, &limits());
    let openings: Vec<u32> = index
        .table_at(0)
        .expect("the table still projects")
        .cells
        .iter()
        .map(|cell| cell.source_pos)
        .collect();
    let map = engine.position_map().expect("the engine is ready");
    let inside = |opening: u32| crate::yrs_engine::RevisionedPosition {
        offset: map.doc_to_scalar(opening + CELL_INTERIOR, document),
        kind: crate::yrs_engine::EditorOffsetKind::Scalar,
        affinity: crate::yrs_engine::Affinity::Before,
    };
    let (anchor, head) = (inside(openings[0]), inside(openings[1]));

    engine
        .apply_typed_transaction(crate::yrs_engine::TypedTransaction {
            request_id: REPLACEMENT_REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: crate::yrs_engine::SelectionIntent::Set(
                crate::yrs_engine::SelectionInput::Cell { anchor, head },
            ),
            history_policy: crate::yrs_engine::HistoryPolicy::Skip,
        })
        .expect("the cell rectangle is admitted");

    assert_eq!(
        engine.clipboard(),
        Some(json!({
            crate::clipboard::CLIPBOARD_UNSUPPORTED_KEY:
                crate::clipboard::CLIPBOARD_UNSUPPORTED_TABLE_GRID
        })),
        "a table the clipboard cannot read must say so, not blame the selection",
    );
}
