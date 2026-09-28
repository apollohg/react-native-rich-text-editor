use std::time::Instant;

use serde_json::{json, Value};

use super::large_table_fixture::{
    ffi_empty_editor, ffi_replace_request, ffi_value, fixture_cell_text, keystroke_cell,
    plain_table_document, session_with_document,
};
use crate::ffi_v2::editor as v2;
use crate::ffi_v2::render as v2_render;
use crate::tables::commands::{TableCommand, TableEdge};
use crate::yrs_engine::observability::{
    reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
};
use crate::yrs_engine::TypedCommand;

const STRUCTURAL_FIXTURE_SIZE: usize = 3;
const STRUCTURAL_COMMAND_REQUEST_ID: u64 = 2;
const LEDGER_EPOCH_OWNER: u64 = 7;
const PROBE_FIXTURES: [(usize, usize); 2] = [(1000, 20), (100, 200)];
const PROBE_WARMUP_KEYSTROKES: usize = 5;
const PROBE_MEASURED_KEYSTROKES: usize = 20;
const PROBE_KEYSTROKES: usize = PROBE_WARMUP_KEYSTROKES + PROBE_MEASURED_KEYSTROKES;
const PROBE_OWNER_ID: &str = "41";
const PROBE_KEYSTROKE_TEXT: &str = "x";
const PROBE_FIRST_KEYSTROKE_REQUEST_ID: usize = 2;
const CELL_BOUNDARY_SCALARS: usize = 1;
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;

#[test]
fn the_ledger_counts_every_document_wide_pass_kind_on_the_generic_structural_path() {
    let mut session = session_with_document(&plain_table_document(
        STRUCTURAL_FIXTURE_SIZE,
        STRUCTURAL_FIXTURE_SIZE,
    ));
    reset_full_pass_counts_for_test();

    let result = session
        .engine
        .apply_command(
            STRUCTURAL_COMMAND_REQUEST_ID,
            TypedCommand::Table(TableCommand::AddTableRow {
                side: TableEdge::After,
            }),
        )
        .expect("adding a row after the caret's row plans");
    assert!(result.is_some(), "adding a row produced no transaction");
    let passes = take_full_pass_counts_for_test();
    eprintln!("generic structural path: {passes:#?}");
    for (kind, count) in [
        (
            "table_projection_derivations",
            passes.table_projection_derivations,
        ),
        (
            "table_command_availability_plans",
            passes.table_command_availability_plans,
        ),
        ("yrs_tree_walks", passes.yrs_tree_walks),
        ("whole_state_encodings", passes.whole_state_encodings),
        ("cell_content_keys", passes.cell_content_keys),
        ("attribute_serializations", passes.attribute_serializations),
    ] {
        assert!(
            count >= 1,
            "the generic structural path must record {kind}, got {count}"
        );
    }

    session
        .pin_position_epoch(LEDGER_EPOCH_OWNER, session.engine.revision())
        .expect("the structural result pins an epoch");
    let pinned = take_full_pass_counts_for_test();
    let block_count = session
        .engine
        .position_map()
        .expect("the engine is ready")
        .block_count();
    assert_eq!(
        pinned.epoch_block_rebuilds, block_count,
        "a full pin builds the anchors of every position-map block once: {pinned:#?}"
    );
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * MILLISECONDS_PER_SECOND
}

fn median_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn probe_render(editor_id: &str) -> (Value, f64) {
    let start = Instant::now();
    let result =
        v2_render::editor_v2_render_native(editor_id.to_owned(), PROBE_OWNER_ID.into(), None, None);
    let elapsed = elapsed_ms(start);
    (ffi_value(&result), elapsed)
}

fn position_epoch(render: &Value) -> String {
    render["positionEpoch"]
        .as_str()
        .expect("a native render publishes its position epoch")
        .to_owned()
}

#[test]
#[ignore = "release-mode wall-clock probe"]
fn large_table_keystroke_budget_probe() {
    for (rows, columns) in PROBE_FIXTURES {
        let fixture = format!("{rows}x{columns}");
        let editor_id = ffi_empty_editor();
        let request = ffi_replace_request(&plain_table_document(rows, columns));

        let start = Instant::now();
        let replaced = v2::editor_v2_replace_document(editor_id.clone(), request);
        let import_ms = elapsed_ms(start);
        ffi_value(&replaced);
        println!("PROBE {fixture} import {import_ms:.3}");

        let (render, first_frame_ms) = probe_render(&editor_id);
        println!("PROBE {fixture} first_frame {first_frame_ms:.3}");

        let cell_scalars = fixture_cell_text(0, 0).chars().count();
        let content_end =
            keystroke_cell(rows, columns) * (cell_scalars + CELL_BOUNDARY_SCALARS) + cell_scalars;
        let mut epoch = position_epoch(&render);
        let mut apply_samples = Vec::with_capacity(PROBE_MEASURED_KEYSTROKES);
        let mut frame_samples = Vec::with_capacity(PROBE_MEASURED_KEYSTROKES);
        for keystroke in 0..PROBE_KEYSTROKES {
            let caret = content_end + keystroke * PROBE_KEYSTROKE_TEXT.chars().count();
            let intent = json!({
                "version": 1,
                "requestId": (PROBE_FIRST_KEYSTROKE_REQUEST_ID + keystroke).to_string(),
                "ownerId": PROBE_OWNER_ID,
                "positionEpoch": epoch,
                "intent": {
                    "type": "insertText",
                    "anchor": caret,
                    "head": caret,
                    "text": PROBE_KEYSTROKE_TEXT,
                },
            })
            .to_string();
            let start = Instant::now();
            let applied = v2::editor_v2_apply_native_intent(editor_id.clone(), intent);
            let apply_ms = elapsed_ms(start);
            ffi_value(&applied);
            let (render, frame_ms) = probe_render(&editor_id);
            epoch = position_epoch(&render);
            if keystroke >= PROBE_WARMUP_KEYSTROKES {
                apply_samples.push(apply_ms);
                frame_samples.push(frame_ms);
            }
        }
        println!("PROBE {fixture} apply {:.3}", median_ms(apply_samples));
        println!("PROBE {fixture} frame {:.3}", median_ms(frame_samples));
        let cell = keystroke_cell(rows, columns);
        let (row, column) = (cell / columns, cell % columns);
        let document = ffi_value(&v2::editor_v2_get_document_json(editor_id.clone()));
        assert_eq!(
            document["content"][0]["content"][row]["content"][column]["content"][0]["content"][0]
                ["text"],
            format!(
                "{}{}",
                fixture_cell_text(row, column),
                PROBE_KEYSTROKE_TEXT.repeat(PROBE_KEYSTROKES)
            ),
            "{fixture}: every probe keystroke lands at the end of the keystroke cell"
        );
        assert!(
            v2::editor_v2_destroy(editor_id).error.is_none(),
            "the probe editor is destroyed"
        );
    }
}
