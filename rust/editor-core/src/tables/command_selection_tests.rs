use serde_json::{json, Value};

use crate::tables::commands::{TableCommand, TableEdge};
use crate::tables::commands_tests::{
    applied, cell_openings, document_of, place_caret, resolved_caret, resolved_cells, seeded,
    select_cells, table_of,
};
use crate::tables::normalize_tests::{cell, cell_with, row, table};
use crate::yrs_engine::YrsDocumentEngine;

const CELL_TEXT_OFFSET: u32 = 2;
const CELL_TEXT_LENGTH: u32 = 2;
const ONE_CHARACTER: u32 = 1;
const GRID_ROWS: [&str; 3] = ["a", "b", "c"];
const GRID_COLUMNS: u32 = 3;
const MIDDLE_CELL: usize = 4;
const LAST_CELL_OF_MIDDLE_ROW: usize = 5;
const LAST_CELL: usize = 8;
const NARROWED_MIDDLE_ROW_LAST: usize = 3;
const SPANNING_CELL: usize = 0;
const SPAN: u32 = 2;
const TRAILING_TEXT: &str = "after";
const TABLE_INDEX: u32 = 0;

fn grid() -> Value {
    table(
        GRID_ROWS
            .iter()
            .map(|name| {
                row((0..GRID_COLUMNS)
                    .map(|column| cell(&format!("{name}{column}")))
                    .collect())
            })
            .collect(),
    )
}

fn trailing_paragraph() -> Value {
    json!({ "type": "paragraph", "content": [{ "type": "text", "text": TRAILING_TEXT }] })
}

fn text_start(engine: &YrsDocumentEngine, index: usize) -> u32 {
    cell_openings(engine)[index] + CELL_TEXT_OFFSET
}

fn caret_at(position: u32) -> Option<(u32, u32)> {
    Some((position, position))
}

fn cell_text(engine: &YrsDocumentEngine, index: usize) -> String {
    let table = table_of(engine);
    let texts = table["content"]
        .as_array()
        .expect("the table holds rows")
        .iter()
        .flat_map(|row| row["content"].as_array().expect("a row holds cells").iter())
        .map(|cell| {
            cell["content"][0]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();
    texts[index].clone()
}

fn add_row(side: TableEdge) -> TableCommand {
    TableCommand::AddTableRow { side }
}

fn add_column(side: TableEdge) -> TableCommand {
    TableCommand::AddTableColumn { side }
}

#[test]
fn row_and_column_insertions_map_the_caret_like_prosemirror_tables() {
    for (command, name, cell_after) in [
        (add_row(TableEdge::Before), "addRowBefore", 7),
        (add_row(TableEdge::After), "addRowAfter", 4),
        (add_column(TableEdge::Before), "addColumnBefore", 6),
        (add_column(TableEdge::After), "addColumnAfter", 5),
    ] {
        let mut engine = seeded(vec![grid()]);
        place_caret(&mut engine, MIDDLE_CELL);

        applied(&mut engine, command);

        assert_eq!(cell_text(&engine, cell_after), "b1", "{name} fixture index");
        assert_eq!(
            resolved_caret(&engine),
            caret_at(text_start(&engine, cell_after) + ONE_CHARACTER),
            "{name} must keep the caret after the first character of b1, as a mapped TextSelection",
        );
    }
}

#[test]
fn row_and_column_insertions_map_a_cell_rectangle_onto_the_same_cells() {
    for (command, name, anchor, head) in [
        (add_row(TableEdge::Before), "addRowBefore", 3, 7),
        (add_row(TableEdge::After), "addRowAfter", 0, 4),
        (add_column(TableEdge::Before), "addColumnBefore", 1, 6),
        (add_column(TableEdge::After), "addColumnAfter", 0, 5),
    ] {
        let mut engine = seeded(vec![grid()]);
        select_cells(&mut engine, 0, MIDDLE_CELL);

        applied(&mut engine, command);

        let openings = cell_openings(&engine);
        assert_eq!(
            resolved_cells(&engine),
            Some((openings[anchor], openings[head])),
            "{name} must keep the a0..b1 rectangle as a mapped CellSelection, now over {} and {}",
            cell_text(&engine, anchor),
            cell_text(&engine, head),
        );
    }
}

#[test]
fn insertions_stretch_whole_row_and_column_selections_over_the_new_cells() {
    for (selected, command, name, anchor, head) in [
        (
            (3, 5),
            add_column(TableEdge::After),
            "row addColumnAfter",
            4,
            7,
        ),
        (
            (3, 5),
            add_column(TableEdge::Before),
            "row addColumnBefore",
            4,
            7,
        ),
        (
            (1, 7),
            add_row(TableEdge::After),
            "column addRowAfter",
            1,
            10,
        ),
        (
            (1, 7),
            add_row(TableEdge::Before),
            "column addRowBefore",
            1,
            10,
        ),
    ] {
        let mut engine = seeded(vec![grid()]);
        select_cells(&mut engine, selected.0, selected.1);

        applied(&mut engine, command);

        let openings = cell_openings(&engine);
        assert_eq!(
            resolved_cells(&engine),
            Some((openings[anchor], openings[head])),
            "{name} must stretch the whole selection like CellSelection.rowSelection/colSelection",
        );
    }
}

#[test]
fn deleting_a_row_leaves_the_caret_where_prosemirror_tables_maps_it() {
    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, MIDDLE_CELL);
    applied(&mut engine, TableCommand::DeleteTableRows);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, 3)),
        "a caret in b1 must land at the start of c0, the next text position after the removed row",
    );

    let mut engine = seeded(vec![grid()]);
    select_cells(&mut engine, MIDDLE_CELL, MIDDLE_CELL);
    applied(&mut engine, TableCommand::DeleteTableRows);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, 3)),
        "a cell selection on b1 cannot survive its row, so it must become a caret at the start of c0",
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, LAST_CELL);
    applied(&mut engine, TableCommand::DeleteTableRows);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, LAST_CELL_OF_MIDDLE_ROW) + CELL_TEXT_LENGTH),
        "removing the last row of a trailing table must search back to the end of b2",
    );

    let mut engine = seeded(vec![grid(), trailing_paragraph()]);
    place_caret(&mut engine, LAST_CELL);
    applied(&mut engine, TableCommand::DeleteTableRows);
    let table_size = document_of(&engine)
        .node_at(&[TABLE_INDEX])
        .expect("the table survives")
        .node_size();
    assert_eq!(
        resolved_caret(&engine),
        caret_at(table_size + ONE_CHARACTER),
        "removing the last row must move the caret forward into the paragraph after the table",
    );
}

#[test]
fn deleting_a_column_leaves_the_selection_where_prosemirror_tables_maps_it() {
    let mut engine = seeded(vec![grid()]);
    select_cells(&mut engine, MIDDLE_CELL, MIDDLE_CELL);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_cells(&engine),
        Some((
            openings[NARROWED_MIDDLE_ROW_LAST],
            openings[NARROWED_MIDDLE_ROW_LAST]
        )),
        "a cell selection on b1 must map onto {}, the cell that slid into its place",
        cell_text(&engine, NARROWED_MIDDLE_ROW_LAST),
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, MIDDLE_CELL);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, NARROWED_MIDDLE_ROW_LAST)),
        "a caret in b1 must land at the start of b2",
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, LAST_CELL_OF_MIDDLE_ROW);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, MIDDLE_CELL)),
        "a caret in b2 must search forward to the start of c0, now cell {MIDDLE_CELL}",
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, LAST_CELL);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, LAST_CELL_OF_MIDDLE_ROW) + CELL_TEXT_LENGTH),
        "a caret in the last cell must search back to the end of c1",
    );

    let mut engine = seeded(vec![grid()]);
    select_cells(&mut engine, 1, 7);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_cells(&engine),
        Some((openings[1], openings[5])),
        "a whole column selection must map onto the next column, {} to {}",
        cell_text(&engine, 1),
        cell_text(&engine, 5),
    );
}

fn spanning_grid() -> Value {
    table(vec![
        row(vec![cell_with(SPAN, SPAN, Value::Null, "big"), cell("a2")]),
        row(vec![cell("b2")]),
        row(vec![cell("c0"), cell("c1"), cell("c2")]),
    ])
}

#[test]
fn splitting_a_selected_cell_selects_the_whole_split_rectangle() {
    let mut engine = seeded(vec![spanning_grid()]);
    select_cells(&mut engine, SPANNING_CELL, SPANNING_CELL);

    applied(&mut engine, TableCommand::SplitTableCell);

    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_cells(&engine),
        Some((openings[SPANNING_CELL], openings[4])),
        "splitCell from a CellSelection selects anchor..lastCell, the bottom right fresh cell",
    );
}

#[test]
fn splitting_from_a_caret_keeps_the_caret() {
    let mut engine = seeded(vec![spanning_grid()]);
    place_caret(&mut engine, SPANNING_CELL);

    applied(&mut engine, TableCommand::SplitTableCell);

    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, SPANNING_CELL) + ONE_CHARACTER),
        "splitCell from a TextSelection only maps the caret",
    );
}
