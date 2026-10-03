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
const GRID_WIDTH: usize = 3;
const WIDENED_WIDTH: usize = 4;
const NARROWED_WIDTH: usize = 2;
const FIRST_ROW: usize = 0;
const MIDDLE_ROW: usize = 1;
const LAST_ROW: usize = 2;
const GROWN_LAST_ROW: usize = 3;
const FIRST_COLUMN: usize = 0;
const MIDDLE_COLUMN: usize = 1;
const LAST_COLUMN: usize = 2;
const WIDENED_LAST_COLUMN: usize = 3;
const NARROWED_LAST_COLUMN: usize = 1;
const SHIFTED: usize = 1;
const A0: usize = FIRST_ROW * GRID_WIDTH + FIRST_COLUMN;
const A1: usize = FIRST_ROW * GRID_WIDTH + MIDDLE_COLUMN;
const A2: usize = FIRST_ROW * GRID_WIDTH + LAST_COLUMN;
const B0: usize = MIDDLE_ROW * GRID_WIDTH + FIRST_COLUMN;
const B1: usize = MIDDLE_ROW * GRID_WIDTH + MIDDLE_COLUMN;
const B2: usize = MIDDLE_ROW * GRID_WIDTH + LAST_COLUMN;
const C1: usize = LAST_ROW * GRID_WIDTH + MIDDLE_COLUMN;
const C2: usize = LAST_ROW * GRID_WIDTH + LAST_COLUMN;
const SPANNING_CELL: usize = 0;
const SPAN: u32 = 2;
const TRAILING_TEXT: &str = "after";
const TABLE_INDEX: u32 = 0;

fn at(row: usize, column: usize, width: usize) -> usize {
    row * width + column
}

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
        (
            add_row(TableEdge::Before),
            "addRowBefore",
            at(MIDDLE_ROW + SHIFTED, MIDDLE_COLUMN, GRID_WIDTH),
        ),
        (
            add_row(TableEdge::After),
            "addRowAfter",
            at(MIDDLE_ROW, MIDDLE_COLUMN, GRID_WIDTH),
        ),
        (
            add_column(TableEdge::Before),
            "addColumnBefore",
            at(MIDDLE_ROW, MIDDLE_COLUMN + SHIFTED, WIDENED_WIDTH),
        ),
        (
            add_column(TableEdge::After),
            "addColumnAfter",
            at(MIDDLE_ROW, MIDDLE_COLUMN, WIDENED_WIDTH),
        ),
    ] {
        let mut engine = seeded(vec![grid()]);
        place_caret(&mut engine, B1);

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
        (
            add_row(TableEdge::Before),
            "addRowBefore",
            at(FIRST_ROW + SHIFTED, FIRST_COLUMN, GRID_WIDTH),
            at(MIDDLE_ROW + SHIFTED, MIDDLE_COLUMN, GRID_WIDTH),
        ),
        (add_row(TableEdge::After), "addRowAfter", A0, B1),
        (
            add_column(TableEdge::Before),
            "addColumnBefore",
            at(FIRST_ROW, FIRST_COLUMN + SHIFTED, WIDENED_WIDTH),
            at(MIDDLE_ROW, MIDDLE_COLUMN + SHIFTED, WIDENED_WIDTH),
        ),
        (
            add_column(TableEdge::After),
            "addColumnAfter",
            at(FIRST_ROW, FIRST_COLUMN, WIDENED_WIDTH),
            at(MIDDLE_ROW, MIDDLE_COLUMN, WIDENED_WIDTH),
        ),
    ] {
        let mut engine = seeded(vec![grid()]);
        select_cells(&mut engine, A0, B1);

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
    let widened_row = (
        at(MIDDLE_ROW, FIRST_COLUMN, WIDENED_WIDTH),
        at(MIDDLE_ROW, WIDENED_LAST_COLUMN, WIDENED_WIDTH),
    );
    let grown_column = (A1, at(GROWN_LAST_ROW, MIDDLE_COLUMN, GRID_WIDTH));
    for (selected, command, name, stretched) in [
        (
            (B0, B2),
            add_column(TableEdge::After),
            "row addColumnAfter",
            widened_row,
        ),
        (
            (B0, B2),
            add_column(TableEdge::Before),
            "row addColumnBefore",
            widened_row,
        ),
        (
            (A1, C1),
            add_row(TableEdge::After),
            "column addRowAfter",
            grown_column,
        ),
        (
            (A1, C1),
            add_row(TableEdge::Before),
            "column addRowBefore",
            grown_column,
        ),
    ] {
        let mut engine = seeded(vec![grid()]);
        select_cells(&mut engine, selected.0, selected.1);

        applied(&mut engine, command);

        let openings = cell_openings(&engine);
        assert_eq!(
            resolved_cells(&engine),
            Some((openings[stretched.0], openings[stretched.1])),
            "{name} must stretch the whole selection like CellSelection.rowSelection/colSelection",
        );
    }
}

#[test]
fn deleting_a_row_leaves_the_caret_where_prosemirror_tables_maps_it() {
    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, B1);
    applied(&mut engine, TableCommand::DeleteTableRows);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, B0)),
        "a caret in b1 must land at the start of c0, the next text position after the removed row",
    );

    let mut engine = seeded(vec![grid()]);
    select_cells(&mut engine, B1, B1);
    applied(&mut engine, TableCommand::DeleteTableRows);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, B0)),
        "a cell selection on b1 cannot survive its row, so it must become a caret at the start of c0",
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, C2);
    applied(&mut engine, TableCommand::DeleteTableRows);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, B2) + CELL_TEXT_LENGTH),
        "removing the last row of a trailing table must search back to the end of b2",
    );

    let mut engine = seeded(vec![grid(), trailing_paragraph()]);
    place_caret(&mut engine, C2);
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
    let narrowed_b2 = at(MIDDLE_ROW, NARROWED_LAST_COLUMN, NARROWED_WIDTH);
    let narrowed_b0 = at(MIDDLE_ROW, FIRST_COLUMN, NARROWED_WIDTH);
    let narrowed_c0 = at(LAST_ROW, FIRST_COLUMN, NARROWED_WIDTH);
    let narrowed_c1 = at(LAST_ROW, NARROWED_LAST_COLUMN, NARROWED_WIDTH);
    let narrowed_a2 = at(FIRST_ROW, NARROWED_LAST_COLUMN, NARROWED_WIDTH);

    let mut engine = seeded(vec![grid()]);
    select_cells(&mut engine, B1, B1);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_cells(&engine),
        Some((openings[narrowed_b2], openings[narrowed_b2])),
        "a cell selection on b1 must map onto {}, the cell that slid into its place",
        cell_text(&engine, narrowed_b2),
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, B1);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, narrowed_b2)),
        "a caret in b1 must land at the start of b2",
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, B2);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, narrowed_c0)),
        "a caret in b2 must search forward to the start of c0",
    );

    let mut engine = seeded(vec![grid()]);
    place_caret(&mut engine, C2);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    assert_eq!(
        resolved_caret(&engine),
        caret_at(text_start(&engine, narrowed_c1) + CELL_TEXT_LENGTH),
        "a caret in the last cell must search back to the end of c1",
    );

    let mut engine = seeded(vec![grid()]);
    select_cells(&mut engine, A1, C1);
    applied(&mut engine, TableCommand::DeleteTableColumns);
    let openings = cell_openings(&engine);
    assert_eq!(
        resolved_cells(&engine),
        Some((openings[narrowed_a2], openings[narrowed_c1])),
        "a whole column selection must map onto the next column, {} to {}",
        cell_text(&engine, narrowed_a2),
        cell_text(&engine, narrowed_c1),
    );

    for (anchor, head, name) in [(A2, C2, "downward"), (C2, A2, "upward")] {
        let mut engine = seeded(vec![grid(), trailing_paragraph()]);
        select_cells(&mut engine, anchor, head);
        applied(&mut engine, TableCommand::DeleteTableColumns);
        let b0_start = text_start(&engine, narrowed_b0);
        let c1_end = text_start(&engine, narrowed_c1) + CELL_TEXT_LENGTH;
        let expected = if anchor < head {
            (b0_start, c1_end)
        } else {
            (c1_end, b0_start)
        };
        assert_eq!(
            resolved_caret(&engine),
            Some(expected),
            "a {name} last column selection falls out of the table and must resolve like TextSelection.between: \
             the head searches toward the anchor and the anchor toward the head, so both stay in the table",
        );
    }
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
        Some((
            openings[SPANNING_CELL],
            openings[at(MIDDLE_ROW, MIDDLE_COLUMN, GRID_WIDTH)]
        )),
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
