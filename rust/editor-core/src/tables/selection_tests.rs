use serde_json::{json, Value};

use crate::boundary::ResourceLimits;
use crate::model::Document;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::serialize::json_in::{from_prosemirror_json, UnknownTypeMode};
use crate::tables::admission::{validate_table_shapes, ProjectionFailure, TableProjectionIndex};
use crate::tables::selection::{
    admit_cell_opening, admit_cell_pair, cell_pair_is_usable, resolve_cell_rect,
    snap_cell_selection, CellAdmission, CellSelectionOrigin, CELL_SELECTION_ANCHOR_FIELD,
};
use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};
use crate::yrs_engine::cell_admission_error;

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
const STARVED_TABLE_GRID_SLOTS: usize = 1;
const REQUEST_ID: u64 = 1;

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

fn cell_with_attrs(attrs: Value) -> Value {
    json!({
        "type": CELL_NODE,
        "attrs": attrs,
        "content": [{ "type": PARAGRAPH_NODE, "content": [] }],
    })
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

#[test]
fn a_starved_projection_is_distinguished_from_a_document_without_cells() {
    let document = document_of(vec![table(vec![row(vec![plain_cell(), plain_cell()])])]);
    let starved = ResourceLimits {
        max_table_grid_slots: STARVED_TABLE_GRID_SLOTS,
        ..ResourceLimits::default()
    };
    let starved_index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &starved);
    let table_free = index_of(&document_of(vec![paragraph("only text")]));

    assert!(starved_index.projection_failed());
    assert_eq!(
        admit_cell_pair(&starved_index, FIRST_CELL, SECOND_CELL),
        CellAdmission::ProjectionUnavailable(ProjectionFailure::ResourceExhausted)
    );
    assert_eq!(
        admit_cell_pair(&table_free, FIRST_CELL, SECOND_CELL),
        CellAdmission::NotCells
    );
    assert_eq!(
        admit_cell_opening(&starved_index, FIRST_CELL + 2),
        Err(CellAdmission::ProjectionUnavailable(
            ProjectionFailure::ResourceExhausted
        ))
    );
    assert_eq!(
        admit_cell_opening(&table_free, FIRST_CELL + 2),
        Err(CellAdmission::NotCells)
    );
}

#[test]
fn only_a_cell_selection_withholds_a_text_range() {
    let document = document_of(vec![table(vec![row(vec![plain_cell(), plain_cell()])])]);
    let content_size = document.content_size();

    for selection in [
        Selection::text(1, 3),
        Selection::cursor(1),
        Selection::node(FIRST_CELL),
        Selection::all(),
    ] {
        assert!(
            selection.text_range(&document).is_some(),
            "{selection:?} must keep its text range"
        );
        assert!(selection.from(&document).is_some());
        assert!(selection.to(&document).is_some());
    }

    let rectangle = Selection::cell(FIRST_CELL, SECOND_CELL);
    assert_eq!(rectangle.text_range(&document), None);
    assert_eq!(rectangle.from(&document), None);
    assert_eq!(rectangle.to(&document), None);
    assert_eq!(rectangle.anchor(&document), FIRST_CELL);
    assert_eq!(rectangle.head(&document), SECOND_CELL);
    assert_ne!(content_size, 0);
}

#[test]
fn an_invalid_table_is_reported_as_structural_not_as_starvation() {
    let invalid = document_of(vec![table(vec![row(vec![cell_with_attrs(json!({
        "colspan": 0,
        "rowspan": 1,
        "colwidth": null,
    }))])])]);
    let index =
        TableProjectionIndex::derive_or_fallback(&invalid, &schema(), &ResourceLimits::default());

    assert_eq!(
        index.projection_failure(),
        Some(ProjectionFailure::Structural)
    );
    assert_eq!(
        admit_cell_pair(&index, FIRST_CELL, FIRST_CELL),
        CellAdmission::ProjectionUnavailable(ProjectionFailure::Structural)
    );
    assert_eq!(
        cell_admission_error(
            REQUEST_ID,
            CELL_SELECTION_ANCHOR_FIELD,
            admit_cell_pair(&index, FIRST_CELL, FIRST_CELL),
        )
        .code,
        "DOCUMENT_INVALID"
    );
}

#[test]
fn a_minted_rectangle_needs_positive_admission_and_a_preserved_one_survives() {
    let document = document_of(vec![table(vec![row(vec![plain_cell(), plain_cell()])])]);
    let starved = ResourceLimits {
        max_table_grid_slots: STARVED_TABLE_GRID_SLOTS,
        ..ResourceLimits::default()
    };
    let starved_index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &starved);
    let healthy = index_of(&document);
    let table_free = index_of(&document_of(vec![paragraph("only text")]));

    assert!(!cell_pair_is_usable(
        &starved_index,
        FIRST_CELL,
        SECOND_CELL,
        CellSelectionOrigin::Minted
    ));
    assert!(cell_pair_is_usable(
        &starved_index,
        FIRST_CELL,
        SECOND_CELL,
        CellSelectionOrigin::Preserved
    ));
    for origin in [CellSelectionOrigin::Minted, CellSelectionOrigin::Preserved] {
        assert!(cell_pair_is_usable(
            &healthy,
            FIRST_CELL,
            SECOND_CELL,
            origin
        ));
        assert!(!cell_pair_is_usable(
            &table_free,
            FIRST_CELL,
            SECOND_CELL,
            origin
        ));
    }
}

#[test]
fn a_starved_projection_refuses_with_a_distinct_code() {
    let unavailable = cell_admission_error(
        REQUEST_ID,
        CELL_SELECTION_ANCHOR_FIELD,
        CellAdmission::ProjectionUnavailable(ProjectionFailure::ResourceExhausted),
    );
    let structural = cell_admission_error(
        REQUEST_ID,
        CELL_SELECTION_ANCHOR_FIELD,
        CellAdmission::ProjectionUnavailable(ProjectionFailure::Structural),
    );
    let not_cells = cell_admission_error(
        REQUEST_ID,
        CELL_SELECTION_ANCHOR_FIELD,
        CellAdmission::NotCells,
    );

    assert_eq!(unavailable.code, "OPERATION_RESOURCE_EXHAUSTED");
    assert_eq!(structural.code, "DOCUMENT_INVALID");
    assert_eq!(not_cells.code, "POSITION_INVALID");
    assert_ne!(unavailable.message, structural.message);
    assert_ne!(unavailable.message, not_cells.message);
}

#[test]
fn a_starved_projection_keeps_a_cell_selection_resolvable() {
    let document = document_of(vec![table(vec![row(vec![plain_cell(), plain_cell()])])]);
    let starved = ResourceLimits {
        max_table_grid_slots: STARVED_TABLE_GRID_SLOTS,
        ..ResourceLimits::default()
    };
    let starved_index = TableProjectionIndex::derive_or_fallback(&document, &schema(), &starved);

    assert_ne!(
        admit_cell_pair(&starved_index, FIRST_CELL, SECOND_CELL),
        CellAdmission::NotCells,
        "a transient projection failure must not read as a deleted table"
    );
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
        StructuralReplacement, TransactionOrigin, TypedCommand, TypedOperation, TypedTransaction,
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
    const PASTED_TEXT: &str = "pasted";
    const TIPTAP_BULLET_LIST: &str = "bulletList";
    const TIPTAP_LIST_ITEM: &str = "listItem";

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
            Some(json!({ "unsupported": "cellSelection" })),
            "a cell rectangle must refuse distinguishably, not look like an empty clipboard"
        );
    }

    fn legacy_selection(engine: &YrsDocumentEngine) -> crate::selection::Selection {
        match engine.resolved_selection().expect("the engine is ready") {
            ResolvedSelection::Text { anchor, head } => {
                crate::selection::Selection::text(anchor.document, head.document)
            }
            ResolvedSelection::Node { at } => crate::selection::Selection::node(at.document),
            ResolvedSelection::Cell { anchor, head } => {
                crate::selection::Selection::cell(anchor.document, head.document)
            }
            ResolvedSelection::All => crate::selection::Selection::all(),
        }
    }

    fn command_map(engine: &YrsDocumentEngine) -> std::collections::HashMap<String, bool> {
        crate::editor_state::command_applicability(
            engine.document().expect("the engine is ready"),
            &tiptap_table_schema(),
            &legacy_selection(engine),
            &ResourceLimits::default(),
        )
    }

    fn paste_text(text: &str) -> TypedCommand {
        TypedCommand::Paste {
            fragment: None,
            html: None,
            text: Some(text.into()),
            plain_text: true,
            allow_base64_images: false,
            input_filter: None,
        }
    }

    #[test]
    fn a_paste_over_a_cell_rectangle_does_not_mutate_the_document() {
        let mut engine = seeded();
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = inside_cell(&engine, BOTTOM_RIGHT);
        select_cells(&mut engine, 1, anchor, head).expect("the rectangle is admitted");
        let document_before = engine.document().expect("the engine is ready").clone();
        let revision_before = engine.revision();

        let outcome = engine.apply_command(2, paste_text(PASTED_TEXT));

        assert!(
            matches!(outcome, Ok(None)),
            "a paste over a cell rectangle must not apply: {outcome:?}"
        );
        assert_eq!(engine.revision(), revision_before);
        assert_eq!(
            engine.document().expect("the engine is ready"),
            &document_before,
            "a paste over a cell rectangle must leave the table intact"
        );
    }

    #[test]
    fn a_paste_over_a_text_selection_still_mutates_the_document() {
        let mut engine = seeded();
        let openings = cell_openings(&engine);
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = scalar_point(&engine, openings[TOP_LEFT] + CELL_TEXT_OFFSET + 1);
        apply(
            &mut engine,
            1,
            Vec::new(),
            SelectionIntent::Set(SelectionInput::Text { anchor, head }),
        )
        .expect("the text selection is admitted");
        let revision_before = engine.revision();

        engine
            .apply_command(2, paste_text(PASTED_TEXT))
            .expect("a paste over a text selection applies");

        assert_ne!(engine.revision(), revision_before);
    }

    const TABLE_COMMAND_PREFIX: &str = "Table";

    #[test]
    fn no_text_structural_command_is_offered_for_a_cell_rectangle() {
        let mut engine = seeded();
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = inside_cell(&engine, BOTTOM_RIGHT);
        let text_commands = command_map(&engine);

        select_cells(&mut engine, 1, anchor, head).expect("the rectangle is admitted");
        let cell_commands = command_map(&engine);

        assert!(
            text_commands.values().any(|applicable| *applicable),
            "the text baseline must offer at least one structural command"
        );
        assert_eq!(
            cell_commands
                .keys()
                .collect::<std::collections::BTreeSet<_>>(),
            text_commands
                .keys()
                .collect::<std::collections::BTreeSet<_>>(),
            "the command map must keep every key so a host reads a definite answer"
        );
        assert!(
            cell_commands
                .iter()
                .filter(|(name, _)| !name.contains(TABLE_COMMAND_PREFIX))
                .all(|(_, applicable)| !*applicable),
            "no text structural command may be offered for a cell rectangle: {cell_commands:?}"
        );
        assert!(
            cell_commands
                .iter()
                .any(|(name, applicable)| name.contains(TABLE_COMMAND_PREFIX) && *applicable),
            "a cell rectangle is exactly where a table command applies: {cell_commands:?}"
        );
    }

    #[test]
    fn a_structural_command_over_a_cell_rectangle_is_not_applicable() {
        let mut engine = seeded();
        let anchor = inside_cell(&engine, TOP_LEFT);
        let head = inside_cell(&engine, BOTTOM_RIGHT);
        select_cells(&mut engine, 1, anchor, head).expect("the rectangle is admitted");
        let revision_before = engine.revision();
        let document_before = engine.document().expect("the engine is ready").clone();

        for command in [
            TypedCommand::ToggleBlockquote,
            TypedCommand::WrapInList {
                list_type: TIPTAP_BULLET_LIST.into(),
                item_type: TIPTAP_LIST_ITEM.into(),
            },
            TypedCommand::ToggleHeading { level: 1 },
        ] {
            let outcome = engine.apply_command(2, command.clone());
            assert!(
                matches!(outcome, Ok(None)),
                "a structural command must refuse a cell rectangle: {command:?} -> {outcome:?}"
            );
        }

        assert_eq!(engine.revision(), revision_before);
        assert_eq!(
            engine.document().expect("the engine is ready"),
            &document_before
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
