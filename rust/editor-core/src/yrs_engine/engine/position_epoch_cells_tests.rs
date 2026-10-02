use serde_json::{json, Value};
use std::hash::{DefaultHasher, Hash, Hasher};

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

    let doc_positions: Vec<_> = (0..state.position_map.block_count())
        .map(|index| {
            state
                .position_map
                .block_doc_positions(index, &state.document)
                .unwrap()
        })
        .collect();
    super::PINNED_CELL_SERIALIZATIONS.set(0);
    let spans = pinning.spans(&doc_positions);
    assert_eq!(
        super::PINNED_CELL_SERIALIZATIONS.get(),
        0,
        "The exact document render cache already fingerprinted every cell child"
    );

    let mismatched_schema = super::CellPinning {
        schema_fingerprint: "a different schema identity",
        ..engine.cell_pinning(state)
    };
    super::PINNED_CELL_SERIALIZATIONS.set(0);
    let fallback_spans = mismatched_schema.spans(&doc_positions);
    assert!(
        super::PINNED_CELL_SERIALIZATIONS.get() > 0,
        "A mismatched schema seal must compute independent fingerprints"
    );
    assert_eq!(
        spans, fallback_spans,
        "Reusing render fingerprints preserves every cell, point, and block range"
    );
    let other = engine_with(
        tabled_schema_with_lists(PROSEMIRROR_TABLE_NAMES),
        vec![table(vec![row(vec![cell_of(vec![paragraph(
            "different",
        )])])])],
    );
    let mismatched_document = super::CellPinning {
        render_blocks: &other.derived_state.as_ref().unwrap().render_blocks,
        ..engine.cell_pinning(state)
    };
    super::PINNED_CELL_SERIALIZATIONS.set(0);
    assert_eq!(mismatched_document.spans(&doc_positions), spans);
    assert!(
        super::PINNED_CELL_SERIALIZATIONS.get() > 0,
        "A cache from another document must compute independent fingerprints"
    );

    let mut different_geometry = engine.document_json().unwrap()["content"]
        .as_array()
        .unwrap()
        .clone();
    different_geometry[1]["content"][0]["content"][0]["attrs"]["colspan"] = json!(2);
    let different_geometry = engine_with(
        tabled_schema_with_lists(PROSEMIRROR_TABLE_NAMES),
        different_geometry,
    );
    let mismatched_index = super::CellPinning {
        index: &different_geometry
            .derived_state
            .as_ref()
            .unwrap()
            .table_projection_index,
        ..engine.cell_pinning(state)
    };
    assert!(
        mismatched_index.render_blocks.matches_identity(
            mismatched_index.document,
            mismatched_index.schema_fingerprint,
        ),
        "The projection mismatch must not be hidden by a cache identity mismatch"
    );
    super::PINNED_CELL_SERIALIZATIONS.set(0);
    let mismatched_spans = mismatched_index.spans(&doc_positions);
    assert!(
        super::PINNED_CELL_SERIALIZATIONS.get() > 0,
        "Mismatched source geometry requires independent cell hashes"
    );
    let independent_index = super::CellPinning {
        schema_fingerprint: "force independent hash",
        ..mismatched_index
    };
    assert_eq!(mismatched_spans, independent_index.spans(&doc_positions));

    assert_eq!(spans.len(), cells.len(), "every cell is pinned");
    for (target, span) in spans.iter().enumerate() {
        let node =
            crate::tables::commands::node_starting_at(&state.document, cells[target].1.source_pos)
                .unwrap();
        let mut expected_fingerprint = DefaultHasher::new();
        for child in node.content().unwrap().iter() {
            crate::serialize::node_to_prosemirror_json(child, &engine.schema)
                .to_string()
                .hash(&mut expected_fingerprint);
        }
        assert_eq!(
            span.cell.content_fingerprint,
            expected_fingerprint.finish(),
            "cell {target}: streamed fingerprint preserves the materialized JSON hash"
        );
        assert_eq!(
            pinning.cell_text_points(&cells, target).as_ref(),
            Some(
                &span
                    .points
                    .iter()
                    .map(|(scalar, point)| (
                        scalar
                            + state
                                .position_map
                                .effective_scalar_start(span.block_range.start),
                        *point
                    ))
                    .collect::<Vec<_>>()
            ),
            "cell {target} at doc position {} resolves to the points it was pinned with",
            cells[target].1.source_pos
        );
    }
}

#[test]
fn edited_and_retained_cell_fingerprints_match_independent_hashes_through_history() {
    use crate::yrs_engine::{
        Affinity, EditorOffsetKind, HistoryPolicy, RevisionedPosition, SelectionInput,
        SelectionIntent, TransactionOrigin, TypedCommand, TypedTransaction,
    };
    let mut engine = engine_with(
        tabled_schema_with_lists(PROSEMIRROR_TABLE_NAMES),
        vec![
            table(vec![row(vec![cell_of(vec![paragraph("old")])])]),
            table(vec![row(vec![cell_of(vec![
                paragraph("outer"),
                table(vec![row(vec![cell_of(vec![paragraph("inner")])])]),
            ])])]),
        ],
    );
    let state = engine.derived_state.as_ref().unwrap();
    let retained_document = state.document.clone();
    let retained_index = state.table_projection_index.clone();
    let retained_positions = state.position_map.clone();
    let retained_render = state.render_blocks.clone();
    let retained_schema = engine.schema.clone();
    let retained_schema_fingerprint = engine.schema_fingerprint.clone();
    let retained = super::CellPinning {
        document: &retained_document,
        index: &retained_index,
        position_map: &retained_positions,
        render_blocks: &retained_render,
        schema: &retained_schema,
        schema_fingerprint: &retained_schema_fingerprint,
    };
    let checked_spans = |pinning: super::CellPinning<'_>, stage: &str| {
        let positions: Vec<_> = (0..pinning.position_map.block_count())
            .map(|index| {
                pinning
                    .position_map
                    .block_doc_positions(index, pinning.document)
                    .unwrap()
            })
            .collect();
        super::PINNED_CELL_SERIALIZATIONS.set(0);
        let fast = pinning.spans(&positions);
        assert_eq!(
            super::PINNED_CELL_SERIALIZATIONS.get(),
            0,
            "{stage}: reuse current render"
        );
        let slow = super::CellPinning {
            schema_fingerprint: "force independent hash",
            ..pinning
        };
        assert_eq!(
            fast,
            slow.spans(&positions),
            "{stage}: exact independent oracle"
        );
        fast
    };
    let original = checked_spans(super::CellPinning { ..retained }, "original");
    let point = |offset| RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    for (request, text, head, stage) in [
        (10, "n", 1, "same-sized edit"),
        (20, "prefix", 0, "nested relocation"),
    ] {
        let document = engine.document().unwrap();
        let opening = crate::tables::commands_tests::cell_openings(&engine)[0];
        let interior = crate::tables::interchange::first_editable_position_in_cell(
            document,
            &engine.schema,
            opening,
        )
        .unwrap()
        .unwrap();
        let start = engine
            .position_map()
            .unwrap()
            .doc_to_scalar(interior + 1, document);
        engine
            .apply_typed_transaction(TypedTransaction {
                request_id: request,
                base_document_revision: engine.revision(),
                origin: TransactionOrigin::LocalApi,
                operations: vec![],
                selection_intent: SelectionIntent::Set(SelectionInput::Text {
                    anchor: point(start),
                    head: point(start + head),
                }),
                history_policy: HistoryPolicy::Skip,
            })
            .unwrap();
        let commit = engine
            .apply_command(
                request + 1,
                TypedCommand::ReplaceSelectionText { text: text.into() },
            )
            .unwrap();
        assert!(commit.is_some(), "{stage}: edit must commit");
        let edited = checked_spans(
            engine.cell_pinning(engine.derived_state.as_ref().unwrap()),
            stage,
        );
        assert_ne!(
            edited[0].cell.content_fingerprint, original[0].cell.content_fingerprint,
            "{stage}: outer content changed"
        );
        assert_eq!(
            checked_spans(super::CellPinning { ..retained }, "retained original"),
            original
        );
        engine.undo(request + 2).unwrap();
        checked_spans(
            engine.cell_pinning(engine.derived_state.as_ref().unwrap()),
            "undo",
        );
        engine.redo(request + 3).unwrap();
        assert_eq!(
            checked_spans(
                engine.cell_pinning(engine.derived_state.as_ref().unwrap()),
                "redo"
            ),
            edited
        );
    }
}
