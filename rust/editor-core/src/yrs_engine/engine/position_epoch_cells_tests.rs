use serde_json::{json, Value};

use crate::tables::commands_tests::engine_with;
use crate::tables::tests::{
    list_block, tabled_schema_with_lists, BULLET_LIST_NODE, LIST_ITEM_NODE,
    PROSEMIRROR_TABLE_NAMES, TASK_ITEM_NODE, TASK_LIST_NODE,
};

const CELL_NODE: &str = "table_cell";

fn cell_of(content: Vec<Value>) -> Value {
    json!({ "type": CELL_NODE, "content": content })
}

fn paragraph(text: &str) -> Value {
    json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
}

fn row(cells: Vec<Value>) -> Value {
    json!({ "type": "table_row", "content": cells })
}

fn table(rows: Vec<Value>) -> Value {
    json!({ "type": "table", "content": rows })
}

fn bullets(texts: &[&str]) -> Value {
    list_block(BULLET_LIST_NODE, LIST_ITEM_NODE, texts)
}

#[test]
fn pin_and_resolution_walks_give_every_cell_the_same_text_points() {
    let nested = table(vec![row(vec![
        cell_of(vec![bullets(&["Inner"])]),
        cell_of(vec![paragraph("Side")]),
    ])]);
    let engine = engine_with(
        tabled_schema_with_lists(PROSEMIRROR_TABLE_NAMES),
        vec![
            paragraph("before"),
            table(vec![
                row(vec![
                    cell_of(vec![bullets(&["Alpha", "Beta"])]),
                    cell_of(vec![paragraph("Prose")]),
                ]),
                row(vec![
                    cell_of(vec![list_block(TASK_LIST_NODE, TASK_ITEM_NODE, &["Todo"])]),
                    cell_of(vec![bullets(&["Gamma"])]),
                ]),
                row(vec![
                    cell_of(vec![bullets(&["Delta"])]),
                    cell_of(vec![paragraph("Outer"), nested, paragraph("After")]),
                ]),
            ]),
        ],
    );
    let state = engine.derived_state.as_ref().expect("the engine is ready");
    let pinning = engine.cell_pinning(state);
    let cells = pinning.cells_in_document_order();

    let spans = pinning.spans();

    assert_eq!(spans.len(), cells.len(), "every cell is pinned");
    for (target, span) in spans.iter().enumerate() {
        assert_eq!(
            pinning.cell_text_points(&cells, target).as_ref(),
            Some(&span.points),
            "cell {target} at doc position {} resolves to the points it was pinned with",
            cells[target].1.source_pos
        );
    }
}
