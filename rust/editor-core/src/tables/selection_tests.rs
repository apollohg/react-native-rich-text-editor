use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::model::Document;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::serialize::json_in::{from_prosemirror_json, UnknownTypeMode};
use crate::tables::admission::{validate_table_shapes, TableProjectionIndex};
use crate::tables::selection::{resolve_cell_rect, snap_cell_selection};
use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};

const TABLE_NODE: &str = "table";
const ROW_NODE: &str = "table_row";
const CELL_NODE: &str = "table_cell";
const PARAGRAPH_NODE: &str = "paragraph";
const TEXT_NODE: &str = "text";
const SINGLE_SPAN: u32 = 1;
const FIRST_CELL: u32 = 2;
const SECOND_CELL: u32 = 6;
const THIRD_CELL: u32 = 10;
const SECOND_ROW_FIRST_CELL: u32 = 16;
const SECOND_ROW_SECOND_CELL: u32 = 20;

fn schema() -> Schema {
    tabled_schema(PROSEMIRROR_TABLE_NAMES)
}

fn cell(colspan: u32, rowspan: u32, text: Option<&str>) -> Value {
    let content = match text {
        None => json!([{ "type": PARAGRAPH_NODE, "content": [] }]),
        Some(text) => json!([{
            "type": PARAGRAPH_NODE,
            "content": [{ "type": TEXT_NODE, "text": text }],
        }]),
    };
    json!({
        "type": CELL_NODE,
        "attrs": { "colspan": colspan, "rowspan": rowspan, "colwidth": null },
        "content": content,
    })
}

fn plain_cell() -> Value {
    cell(SINGLE_SPAN, SINGLE_SPAN, None)
}

fn row(cells: Vec<Value>) -> Value {
    json!({ "type": ROW_NODE, "content": cells })
}

fn table(rows: Vec<Value>) -> Value {
    json!({ "type": TABLE_NODE, "content": rows })
}

fn paragraph(text: &str) -> Value {
    json!({
        "type": PARAGRAPH_NODE,
        "content": [{ "type": TEXT_NODE, "text": text }],
    })
}

fn document_of(content: Vec<Value>) -> Document {
    from_prosemirror_json(
        &json!({ "type": "doc", "content": content }),
        &schema(),
        UnknownTypeMode::Preserve,
    )
    .expect("the table fixture parses")
}

fn index_of(document: &Document) -> TableProjectionIndex {
    validate_table_shapes(document, &schema(), &ResourceLimits::default())
        .expect("the fixture document projects")
}

fn two_by_three() -> TableProjectionIndex {
    index_of(&document_of(vec![table(vec![
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
    ])]))
}

#[test]
fn a_single_cell_resolves_to_exactly_that_cell() {
    let index = two_by_three();

    let rect = resolve_cell_rect(&index, FIRST_CELL, FIRST_CELL).expect("the cell resolves");

    assert_eq!(rect.cells, vec![FIRST_CELL]);
    assert_eq!((rect.top, rect.left, rect.bottom, rect.right), (0, 0, 1, 1));
}

#[test]
fn a_rectangle_collects_every_covered_real_cell() {
    let index = two_by_three();

    let rect =
        resolve_cell_rect(&index, FIRST_CELL, SECOND_ROW_SECOND_CELL).expect("the pair resolves");

    assert_eq!(
        rect.cells,
        vec![
            FIRST_CELL,
            SECOND_CELL,
            SECOND_ROW_FIRST_CELL,
            SECOND_ROW_SECOND_CELL
        ]
    );
}

#[test]
fn direction_does_not_change_the_covered_cell_set() {
    let index = two_by_three();

    let forward = resolve_cell_rect(&index, FIRST_CELL, SECOND_ROW_SECOND_CELL)
        .expect("the forward pair resolves");
    let backward = resolve_cell_rect(&index, SECOND_ROW_SECOND_CELL, FIRST_CELL)
        .expect("the backward pair resolves");

    assert_eq!(forward, backward);
}

#[test]
fn a_position_inside_cell_content_is_not_a_cell_opening() {
    let index = two_by_three();

    assert_eq!(resolve_cell_rect(&index, FIRST_CELL + 1, FIRST_CELL), None);
}

#[test]
fn cells_from_two_tables_do_not_resolve() {
    let first = table(vec![row(vec![plain_cell(), plain_cell()])]);
    let second = table(vec![row(vec![plain_cell(), plain_cell()])]);
    let document = document_of(vec![first, second]);
    let index = index_of(&document);
    let positions: Vec<u32> = index.positions().collect();
    let second_table_first_cell = positions[1] + FIRST_CELL;

    assert!(resolve_cell_rect(&index, FIRST_CELL, FIRST_CELL).is_some());
    assert!(resolve_cell_rect(&index, second_table_first_cell, second_table_first_cell).is_some());
    assert_eq!(
        resolve_cell_rect(&index, FIRST_CELL, second_table_first_cell),
        None
    );
}

#[test]
fn a_synthetic_slot_is_never_an_anchor() {
    let document = document_of(vec![table(vec![
        row(vec![cell(SINGLE_SPAN, 2, None), plain_cell()]),
        row(vec![plain_cell()]),
    ])]);
    let index = index_of(&document);
    let table = index.table_at(0).expect("the table projects");
    let real: Vec<u32> = table.cells.iter().map(|cell| cell.source_pos).collect();
    let covered_by_rowspan = 1;

    assert_eq!(table.slots.len(), (table.rows * table.columns) as usize);
    assert_eq!(real.len(), table.slots.len() - covered_by_rowspan);
    for position in 0..40u32 {
        if real.contains(&position) {
            continue;
        }
        assert_eq!(
            resolve_cell_rect(&index, position, position),
            None,
            "position {position} is not a real cell opening"
        );
    }
}

#[test]
fn a_merged_cell_touched_at_one_corner_pulls_in_its_whole_span() {
    let document = document_of(vec![table(vec![
        row(vec![plain_cell(), cell(2, SINGLE_SPAN, None)]),
        row(vec![plain_cell(), plain_cell(), plain_cell()]),
    ])]);
    let index = index_of(&document);
    let merged = SECOND_CELL;

    let rect = resolve_cell_rect(&index, FIRST_CELL, merged).expect("the pair resolves");

    assert_eq!((rect.left, rect.right), (0, 3));
    assert_eq!(rect.cells, vec![FIRST_CELL, merged]);
}

#[test]
fn an_unrelated_earlier_insertion_preserves_the_same_real_cell_set() {
    let before = two_by_three();
    let selected = resolve_cell_rect(&before, FIRST_CELL, SECOND_ROW_SECOND_CELL)
        .expect("the pair resolves before the insertion");

    let after = index_of(&document_of(vec![
        paragraph("inserted"),
        table(vec![
            row(vec![plain_cell(), plain_cell(), plain_cell()]),
            row(vec![plain_cell(), plain_cell(), plain_cell()]),
        ]),
    ]));
    let shift = after
        .positions()
        .next()
        .expect("the shifted table projects");
    let shifted = resolve_cell_rect(&after, FIRST_CELL + shift, SECOND_ROW_SECOND_CELL + shift)
        .expect("the shifted pair resolves");

    assert_ne!(shift, 0);
    assert_eq!(
        shifted.cells,
        selected
            .cells
            .iter()
            .map(|position| position + shift)
            .collect::<Vec<u32>>()
    );
}

#[test]
fn emoji_and_combining_marks_do_not_move_cell_openings() {
    let plain = index_of(&document_of(vec![table(vec![row(vec![
        plain_cell(),
        plain_cell(),
    ])])]));
    let decorated = index_of(&document_of(vec![table(vec![row(vec![
        cell(SINGLE_SPAN, SINGLE_SPAN, Some("\u{1f600}e\u{0301}")),
        plain_cell(),
    ])])]));

    let plain_second = plain
        .table_at(0)
        .expect("the plain table projects")
        .cells
        .last()
        .expect("the plain table has cells")
        .source_pos;
    let decorated_second = decorated
        .table_at(0)
        .expect("the decorated table projects")
        .cells
        .last()
        .expect("the decorated table has cells")
        .source_pos;

    assert_eq!(plain_second, SECOND_CELL);
    assert_eq!(decorated_second, SECOND_CELL + 3);
    assert!(resolve_cell_rect(&decorated, FIRST_CELL, decorated_second).is_some());
}

#[test]
fn a_deleted_anchor_snaps_forward_to_the_nearest_surviving_cell() {
    let survivor = index_of(&document_of(vec![table(vec![row(vec![
        plain_cell(),
        plain_cell(),
    ])])]));

    let snapped = snap_cell_selection(&survivor, THIRD_CELL, FIRST_CELL)
        .expect("the deleted anchor snaps to a surviving cell");

    assert_eq!(
        snapped,
        Selection::Cell {
            anchor: SECOND_CELL,
            head: FIRST_CELL,
        }
    );
}

#[test]
fn a_surviving_endpoint_keeps_its_own_cell() {
    let index = two_by_three();

    let snapped = snap_cell_selection(&index, SECOND_CELL, SECOND_ROW_FIRST_CELL)
        .expect("both endpoints survive");

    assert_eq!(
        snapped,
        Selection::Cell {
            anchor: SECOND_CELL,
            head: SECOND_ROW_FIRST_CELL,
        }
    );
}

#[test]
fn a_selection_with_no_surviving_table_has_no_cell_form() {
    let empty = index_of(&document_of(vec![paragraph("only text")]));

    assert_eq!(snap_cell_selection(&empty, FIRST_CELL, SECOND_CELL), None);
}

mod engine_round_trip {
    use serde_json::json;

    use crate::boundary::ResourceLimits;
    use crate::model::Fragment;
    use crate::schema::presets::tiptap_table_schema;
    use crate::selection::Selection;
    use crate::yrs_engine::{
        Affinity, EditingLimits, EditorOffsetKind, HistoryPolicy, InitializationMode,
        ResolvedSelection, RevisionedPosition, SelectionInput, SelectionIntent,
        StructuralReplacement, TransactionOrigin, TypedOperation, TypedTransaction,
        YrsDocumentEngine, YrsEngineConfig,
    };

    const FRAGMENT_NAME: &str = "prosemirror";
    const TIPTAP_TABLE: &str = "table";
    const TIPTAP_ROW: &str = "tableRow";
    const TIPTAP_CELL: &str = "tableCell";
    const PARAGRAPH: &str = "paragraph";
    const TEXT: &str = "text";
    const CELL_TEXT_OFFSET: u32 = 2;
    const CELL_TEXTS: [&str; 4] = ["alpha", "beta", "gamma", "delta"];
    const TOP_LEFT: usize = 0;
    const TOP_RIGHT: usize = 1;
    const BOTTOM_LEFT: usize = 2;
    const BOTTOM_RIGHT: usize = 3;
    const LEADING_PARAGRAPH_TEXT: &str = "before";

    fn engine() -> YrsDocumentEngine {
        YrsDocumentEngine::new(YrsEngineConfig {
            schema: tiptap_table_schema(),
            fragment_name: FRAGMENT_NAME.into(),
            initialization_mode: InitializationMode::LocalEmpty,
            resource_limits: ResourceLimits::default(),
            editing_limits: EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .expect("the tabled engine initializes")
    }

    fn cell(text: &str) -> serde_json::Value {
        json!({
            "type": TIPTAP_CELL,
            "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null },
            "content": [{
                "type": PARAGRAPH,
                "content": [{ "type": TEXT, "text": text }],
            }],
        })
    }

    fn paragraph(text: &str) -> serde_json::Value {
        json!({
            "type": PARAGRAPH,
            "content": [{ "type": TEXT, "text": text }],
        })
    }

    fn table() -> serde_json::Value {
        json!({
            "type": TIPTAP_TABLE,
            "content": [
                {
                    "type": TIPTAP_ROW,
                    "content": [cell(CELL_TEXTS[TOP_LEFT]), cell(CELL_TEXTS[TOP_RIGHT])],
                },
                {
                    "type": TIPTAP_ROW,
                    "content": [cell(CELL_TEXTS[BOTTOM_LEFT]), cell(CELL_TEXTS[BOTTOM_RIGHT])],
                },
            ],
        })
    }

    fn seeded_with(content: Vec<serde_json::Value>) -> YrsDocumentEngine {
        let mut engine = engine();
        engine
            .import_json(
                &json!({ "type": "doc", "content": content }).to_string(),
                TransactionOrigin::DocumentImport,
            )
            .expect("the tabled document imports");
        engine
    }

    fn seeded() -> YrsDocumentEngine {
        seeded_with(vec![table()])
    }

    fn cell_openings(engine: &YrsDocumentEngine) -> Vec<u32> {
        let index = engine
            .table_projection_index()
            .expect("the engine is ready");
        let table_position = index
            .positions()
            .next()
            .expect("the document holds a table");
        index
            .table_at(table_position)
            .expect("the table projects")
            .cells
            .iter()
            .map(|cell| cell.source_pos)
            .collect()
    }

    fn scalar_point(engine: &YrsDocumentEngine, document_position: u32) -> RevisionedPosition {
        let map = engine.position_map().expect("the engine is ready");
        let document = engine.document().expect("the engine is ready");
        RevisionedPosition {
            offset: map.doc_to_scalar(document_position, document),
            kind: EditorOffsetKind::Scalar,
            affinity: Affinity::Before,
        }
    }

    fn inside_cell(engine: &YrsDocumentEngine, cell_index: usize) -> RevisionedPosition {
        let opening = cell_openings(engine)[cell_index];
        scalar_point(engine, opening + CELL_TEXT_OFFSET)
    }

    fn apply(
        engine: &mut YrsDocumentEngine,
        request_id: u64,
        operations: Vec<TypedOperation>,
        selection_intent: SelectionIntent,
    ) -> crate::yrs_engine::OperationResult<()> {
        let transaction = TypedTransaction {
            request_id,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations,
            selection_intent,
            history_policy: HistoryPolicy::Skip,
        };
        engine.apply_typed_transaction(transaction).map(|_| ())
    }

    fn select_cells(
        engine: &mut YrsDocumentEngine,
        request_id: u64,
        anchor: RevisionedPosition,
        head: RevisionedPosition,
    ) -> crate::yrs_engine::OperationResult<()> {
        apply(
            engine,
            request_id,
            Vec::new(),
            SelectionIntent::Set(SelectionInput::Cell { anchor, head }),
        )
    }

    fn resolved_cells(engine: &YrsDocumentEngine) -> Option<(u32, u32)> {
        match engine.resolved_selection()? {
            ResolvedSelection::Cell { anchor, head } => Some((anchor.document, head.document)),
            _ => None,
        }
    }

    #[test]
    fn a_scalar_offset_inside_a_cell_addresses_that_cell() {
        let mut engine = seeded();
        let openings = cell_openings(&engine);
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = inside_cell(&engine, BOTTOM_RIGHT);

        select_cells(&mut engine, 1, anchor, head).expect("the cell rectangle is admitted");

        assert_eq!(
            resolved_cells(&engine),
            Some((openings[TOP_LEFT], openings[BOTTOM_RIGHT]))
        );
    }

    #[test]
    fn a_utf16_offset_inside_a_cell_addresses_the_same_cell() {
        let mut engine = seeded_with(vec![json!({
            "type": TIPTAP_TABLE,
            "content": [{
                "type": TIPTAP_ROW,
                "content": [cell("\u{1f600}\u{1f600}"), cell(CELL_TEXTS[TOP_RIGHT])],
            }],
        })]);
        let openings = cell_openings(&engine);
        let map = engine.position_map().expect("the engine is ready");
        let document = engine.document().expect("the engine is ready");
        let scalar = map.doc_to_scalar(openings[TOP_RIGHT] + CELL_TEXT_OFFSET, document);
        let rendered = crate::render::rendered_text(document, &tiptap_table_schema());
        let utf16 = crate::yrs_engine::scalar_offset_to_utf16(&rendered, scalar)
            .expect("the scalar offset is representable in UTF-16");
        let head = RevisionedPosition {
            offset: utf16,
            kind: EditorOffsetKind::Utf16,
            affinity: Affinity::Before,
        };
        let anchor = inside_cell(&engine, TOP_LEFT);

        assert_ne!(utf16, scalar);
        select_cells(&mut engine, 1, anchor, head).expect("the UTF-16 rectangle is admitted");

        assert_eq!(
            resolved_cells(&engine),
            Some((openings[TOP_LEFT], openings[TOP_RIGHT]))
        );
    }

    #[test]
    fn a_cell_rectangle_preserves_anchor_and_head_direction() {
        let mut engine = seeded();
        let openings = cell_openings(&engine);
        let anchor = inside_cell(&engine, BOTTOM_RIGHT);
        let head = inside_cell(&engine, TOP_LEFT);

        select_cells(&mut engine, 1, anchor, head).expect("the reversed rectangle is admitted");

        assert_eq!(
            resolved_cells(&engine),
            Some((openings[BOTTOM_RIGHT], openings[TOP_LEFT]))
        );
    }

    #[test]
    fn an_offset_outside_every_table_is_refused_as_a_cell_rectangle() {
        let mut engine = seeded_with(vec![paragraph(LEADING_PARAGRAPH_TEXT), table()]);
        let outside = scalar_point(&engine, 1);
        let inside = inside_cell(&engine, TOP_LEFT);

        let error = select_cells(&mut engine, 1, outside, inside)
            .expect_err("a position outside every table is not a cell");

        assert!(
            error.message.contains("real table cells"),
            "unexpected refusal: {error:?}"
        );
    }

    #[test]
    fn a_refused_cell_rectangle_leaves_the_previous_selection_untouched() {
        let mut engine = seeded_with(vec![paragraph(LEADING_PARAGRAPH_TEXT), table()]);
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = inside_cell(&engine, TOP_RIGHT);
        select_cells(&mut engine, 1, anchor, head).expect("the first rectangle is admitted");
        let before = engine.resolved_selection().cloned();
        let outside = scalar_point(&engine, 1);

        select_cells(&mut engine, 2, outside, outside)
            .expect_err("a position outside every table is not a cell");

        assert_eq!(engine.resolved_selection().cloned(), before);
    }

    #[test]
    fn an_unrelated_earlier_insertion_keeps_the_same_cells() {
        let mut engine = seeded();
        let openings = cell_openings(&engine);
        let anchor = inside_cell(&engine, TOP_RIGHT);
        let head = inside_cell(&engine, BOTTOM_LEFT);
        select_cells(&mut engine, 1, anchor, head).expect("the rectangle is admitted");
        let inside_first_cell = inside_cell(&engine, TOP_LEFT);
        let inserted = "xy";

        apply(
            &mut engine,
            2,
            vec![TypedOperation::InsertText {
                at: inside_first_cell,
                text: inserted.into(),
                marks: Vec::new(),
            }],
            SelectionIntent::Preserve,
        )
        .expect("the unrelated insertion applies");

        let shift = inserted.chars().count() as u32;
        assert_eq!(
            resolved_cells(&engine),
            Some((openings[TOP_RIGHT] + shift, openings[BOTTOM_LEFT] + shift))
        );
    }

    #[test]
    fn a_cell_rectangle_is_never_exported_as_a_clipboard_text_range() {
        let mut engine = seeded();
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = inside_cell(&engine, BOTTOM_RIGHT);
        let text_anchor = inside_cell(&engine, TOP_LEFT);
        let text_head = scalar_point(
            &engine,
            cell_openings(&engine)[TOP_LEFT] + CELL_TEXT_OFFSET + 1,
        );
        apply(
            &mut engine,
            1,
            Vec::new(),
            SelectionIntent::Set(SelectionInput::Text {
                anchor: text_anchor,
                head: text_head,
            }),
        )
        .expect("the text selection is admitted");
        let text_clipboard = engine.clipboard().expect("a text selection exports");

        select_cells(&mut engine, 2, anchor, head).expect("the rectangle is admitted");

        assert!(text_clipboard.get("fragment").is_some());
        assert_eq!(
            engine.clipboard(),
            Some(json!({ "empty": true })),
            "a cell rectangle must not export as a scalar text range"
        );
    }

    #[test]
    fn replacing_the_table_degrades_the_cell_selection_to_text() {
        let mut engine = seeded();
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = inside_cell(&engine, BOTTOM_RIGHT);
        select_cells(&mut engine, 1, anchor, head).expect("the rectangle is admitted");
        let replacement = crate::serialize::json_in::from_prosemirror_json(
            &json!({ "type": "doc", "content": [paragraph(LEADING_PARAGRAPH_TEXT)] }),
            &tiptap_table_schema(),
            crate::serialize::json_in::UnknownTypeMode::Preserve,
        )
        .expect("the replacement document parses")
        .root()
        .child(0)
        .expect("the replacement paragraph exists")
        .clone();

        apply(
            &mut engine,
            2,
            vec![TypedOperation::ReplaceStructure(
                StructuralReplacement::new(
                    Vec::new(),
                    0,
                    1,
                    Fragment::from(vec![replacement]),
                    Selection::cursor(1),
                ),
            )],
            SelectionIntent::Preserve,
        )
        .expect("the structural replacement applies");

        assert!(
            matches!(
                engine.resolved_selection(),
                Some(ResolvedSelection::Text { .. })
            ),
            "a vanished table must not leave a cell selection: {:?}",
            engine.resolved_selection()
        );
    }
}
