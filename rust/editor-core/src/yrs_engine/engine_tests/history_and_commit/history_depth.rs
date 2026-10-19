const DEPTH_REQUEST_ID: u64 = 70_095_000;
const DEPTH_BOUNDED_INSERTS: usize = 10;
const DEPTH_INSERT_STRIDE: usize = 97;

const DEPTH_SELECTION_WIDTH: u32 = 5;

fn assert_bounded_edits_on_a_very_large_document_undo_and_redo_in_order(
    edit: impl Fn(usize) -> (u32, TypedCommand),
) {
    let mut engine = transaction_engine();
    engine
        .prepare_root_replacement_json(
            DEPTH_REQUEST_ID,
            &loaded_article(VERY_LARGE_LOADED_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let mut request = DEPTH_REQUEST_ID;
    let mut before_each = Vec::new();
    let mut after_each = Vec::new();
    for step in 0..DEPTH_BOUNDED_INSERTS {
        let block = engine
            .derived_state
            .as_ref()
            .unwrap()
            .position_map
            .block(step * DEPTH_INSERT_STRIDE)
            .unwrap()
            .into_owned();
        let caret = block.scalar_start + block.scalar_prefix_len;
        let (width, command) = edit(step);
        request += 1;
        select_text(&mut engine, request, caret, caret + width);
        before_each.push(engine.document_json().unwrap());
        request += 1;
        engine
            .apply_command(request, command.clone())
            .unwrap()
            .unwrap_or_else(|| panic!("edit {step} ({command:?}) must apply"));
        after_each.push(engine.document_json().unwrap());
    }
    for step in (0..DEPTH_BOUNDED_INSERTS).rev() {
        request += 1;
        assert!(
            engine.undo(request).unwrap().is_some(),
            "undo of edit {step} must be available"
        );
        assert_eq!(
            engine.document_json().unwrap(),
            before_each[step],
            "undoing edit {step} restores the document it was applied to"
        );
    }
    assert!(!engine.can_undo(), "the reset load is not undoable");
    for step in 0..DEPTH_BOUNDED_INSERTS {
        request += 1;
        assert!(
            engine.redo(request).unwrap().is_some(),
            "redo of edit {step} must be available"
        );
        assert_eq!(
            engine.document_json().unwrap(),
            after_each[step],
            "redoing edit {step} reapplies it"
        );
    }
    assert!(!engine.can_redo());
}

#[test]
fn ten_bounded_inserts_on_a_very_large_document_all_undo_and_redo_in_order() {
    assert_bounded_edits_on_a_very_large_document_undo_and_redo_in_order(|step| {
        (
            0,
            TypedCommand::InsertText {
                text: format!("insert {step}"),
            },
        )
    });
}

#[test]
fn ten_bounded_deletes_on_a_very_large_document_all_undo_and_redo_in_order() {
    assert_bounded_edits_on_a_very_large_document_undo_and_redo_in_order(|_| {
        (DEPTH_SELECTION_WIDTH, TypedCommand::DeleteBackward)
    });
}

#[test]
fn ten_bold_toggles_on_a_very_large_document_all_undo_and_redo_in_order() {
    assert_bounded_edits_on_a_very_large_document_undo_and_redo_in_order(|_| {
        (
            DEPTH_SELECTION_WIDTH,
            TypedCommand::ToggleMark {
                mark_type: "bold".into(),
            },
        )
    });
}

const RETENTION_REQUEST_ID: u64 = 70_096_000;
const RETENTION_EDITS: usize = 12;
const RETENTION_STRIDE: usize = 29;
const RETENTION_SELECTION_WIDTH: u32 = 4;

fn bounded_edit(engine: &mut YrsDocumentEngine, request: &mut u64, edit: usize) {
    let block = engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .block(edit * RETENTION_STRIDE)
        .unwrap()
        .into_owned();
    let start = block.scalar_start + block.scalar_prefix_len;
    let (anchor, head, command) = match edit % 3 {
        0 => (
            start,
            start,
            TypedCommand::InsertText {
                text: format!("edit {edit}"),
            },
        ),
        1 => (
            start,
            start + RETENTION_SELECTION_WIDTH,
            TypedCommand::ToggleMark {
                mark_type: "italic".into(),
            },
        ),
        _ => (
            start + RETENTION_SELECTION_WIDTH,
            start + RETENTION_SELECTION_WIDTH,
            TypedCommand::DeleteBackward,
        ),
    };
    *request += 1;
    select_text(engine, *request, anchor, head);
    *request += 1;
    engine
        .apply_command(*request, command)
        .unwrap()
        .unwrap_or_else(|| panic!("edit {edit} must apply"));
}

fn assert_cached_standalone_matches_a_full_charge(engine: &YrsDocumentEngine, context: &str) {
    let state = engine.derived_state.as_ref().unwrap();
    let full =
        crate::yrs_engine::derived_state::history_document_snapshot_standalone_retained_bytes(
            &state.history_snapshot_parts(),
            &engine.schema_fingerprint,
            &engine.fragment_name,
            engine.scope.as_ref(),
        )
        .unwrap();
    let cached = state.history_snapshot_standalone_bytes(|| None);
    assert_eq!(
        cached,
        Some(full),
        "{context}: the incrementally derived standalone charge equals a full charge"
    );
}

#[cfg(not(target_os = "android"))]
struct HistoryRetention {
    charged: usize,
    released_metadata: usize,
}

#[cfg(not(target_os = "android"))]
fn measure_history_retention(edits: usize) -> HistoryRetention {
    use crate::test_support::thread_allocation_meter::thread_live_bytes;

    let mut engine = transaction_engine();
    engine
        .prepare_root_replacement_json(
            RETENTION_REQUEST_ID,
            &loaded_article(LARGE_LOADED_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let mut request = RETENTION_REQUEST_ID;
    for edit in 0..edits {
        bounded_edit(&mut engine, &mut request, edit);
        assert_cached_standalone_matches_a_full_charge(&engine, &format!("edit {edit}"));
    }
    assert_eq!(
        engine.history.document_snapshot_identities_for_test().len(),
        edits,
        "every bounded edit is its own retained undo group"
    );
    let charged = engine.retained_history_for_test().1;
    let replay_bytes = engine.history.replay_bytes_retained_for_test();
    let live_with_history = thread_live_bytes();
    engine.history.release_for_test();
    let released = usize::try_from(live_with_history - thread_live_bytes()).unwrap();
    let released_metadata = released.saturating_sub(replay_bytes);
    eprintln!(
        "{edits} edits: history released {released} bytes, {replay_bytes} of them encoded \
         replay bytes and {released_metadata} snapshot and metadata bytes, against a \
         {charged}-byte charge"
    );
    HistoryRetention {
        charged,
        released_metadata,
    }
}

#[test]
#[cfg(not(target_os = "android"))]
fn history_snapshot_charges_bound_the_memory_history_actually_retains() {
    let shorter = measure_history_retention(RETENTION_EDITS / 2);
    let longer = measure_history_retention(RETENTION_EDITS);
    for retention in [&shorter, &longer] {
        assert!(
            retention.released_metadata <= retention.charged,
            "the metadata charge must bound the memory history actually retains: released {} > \
             charged {}",
            retention.released_metadata,
            retention.charged
        );
    }
    let released_growth = longer.released_metadata - shorter.released_metadata;
    let charged_growth = longer.charged - shorter.charged;
    eprintln!(
        "{} more edits retained {released_growth} more bytes against {charged_growth} more charged",
        RETENTION_EDITS - RETENTION_EDITS / 2
    );
    assert!(
        released_growth <= charged_growth,
        "each additional edit's unique snapshot charge must bound the memory it really adds: \
         released {released_growth} > charged {charged_growth}"
    );
}

#[test]
fn history_rolls_and_clears_when_unique_snapshot_bytes_exceed_the_budget() {
    const ROLLING_METADATA_LIMIT: usize = 3 * 1024 * 1024;
    let mut engine = transaction_engine_with_editing_limits(crate::yrs_engine::EditingLimits {
        max_derived_output_bytes: ROLLING_METADATA_LIMIT,
        ..crate::yrs_engine::EditingLimits::default()
    });
    engine
        .prepare_root_replacement_json(
            RETENTION_REQUEST_ID,
            &loaded_article(LARGE_LOADED_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    let mut request = RETENTION_REQUEST_ID;
    let mut before_each = Vec::new();
    let mut retained_groups = Vec::new();
    for edit in 0..RETENTION_EDITS {
        before_each.push(engine.document_json().unwrap());
        bounded_edit(&mut engine, &mut request, edit);
        let groups = engine.history.document_snapshot_identities_for_test().len();
        assert!(
            engine.retained_history_for_test().1 <= ROLLING_METADATA_LIMIT,
            "edit {edit}: retained metadata stays within the budget"
        );
        retained_groups.push(groups);
    }
    eprintln!("retained undo groups after each edit: {retained_groups:?}");
    let rolled = retained_groups.windows(2).any(|pair| pair[1] < pair[0]);
    assert!(
        rolled,
        "the budget must force at least one roll: retained groups {retained_groups:?}"
    );
    let depth = *retained_groups.last().unwrap();
    for undone in 0..depth {
        request += 1;
        assert!(
            engine.undo(request).unwrap().is_some(),
            "undo {undone} of {depth}"
        );
        assert_eq!(
            engine.document_json().unwrap(),
            before_each[RETENTION_EDITS - 1 - undone],
            "undo {undone} restores the document before its edit"
        );
    }
    assert!(
        !engine.can_undo(),
        "rolling cleared every group older than the retained {depth}"
    );
}

const LARGE_EDIT_REQUEST_ID: u64 = 70_097_000;
const ENCODED_STATE_MARGIN_BYTES: usize = 4096;

fn very_large_engine(editing_limits: crate::yrs_engine::EditingLimits) -> YrsDocumentEngine {
    let mut engine = transaction_engine_with_editing_limits(editing_limits);
    engine
        .prepare_root_replacement_json(
            LARGE_EDIT_REQUEST_ID,
            &loaded_article(VERY_LARGE_LOADED_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::ResetAndClear,
        )
        .unwrap();
    engine
}

fn delete_leading_scalars(engine: &mut YrsDocumentEngine, request: u64, scalars: u32) {
    select_text(engine, request, 0, scalars);
    engine
        .apply_command(request + 1, TypedCommand::DeleteBackward)
        .unwrap()
        .unwrap();
}

fn leading_half(engine: &YrsDocumentEngine) -> u32 {
    engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .total_scalars()
        / 2
}

fn leading_delete_undo_units(engine: &YrsDocumentEngine, request: u64, scalars: u32) -> u64 {
    let point = |offset| RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    };
    engine
        .compile_typed_transaction(TypedTransaction {
            request_id: request,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalInput,
            operations: vec![TypedOperation::DeleteRange {
                range: RevisionedRange {
                    from: point(0),
                    to: point(scalars),
                },
            }],
            selection_intent: SelectionIntent::UseOperationResult,
            history_policy: HistoryPolicy::Auto,
        })
        .unwrap()
        .undo_units_bound
}

#[test]
fn a_select_all_delete_still_exceeds_a_tight_undo_unit_limit() {
    let probe = very_large_engine(crate::yrs_engine::EditingLimits::default());
    let total = probe
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .total_scalars();
    let units = leading_delete_undo_units(&probe, LARGE_EDIT_REQUEST_ID + 1, total);
    let mut engine = very_large_engine(crate::yrs_engine::EditingLimits {
        max_undo_retained_units: units - 1,
        ..crate::yrs_engine::EditingLimits::default()
    });
    let before = engine.document_json().unwrap();
    select_text(&mut engine, LARGE_EDIT_REQUEST_ID + 2, 0, total);
    let error = engine
        .apply_command(LARGE_EDIT_REQUEST_ID + 3, TypedCommand::DeleteBackward)
        .unwrap_err();
    assert_eq!(
        (error.code, error.limit),
        ("OPERATION_LIMIT_EXCEEDED", Some(units - 1)),
        "a select-all delete must exceed an undo limit one below its {units}-unit range: {error:?}"
    );
    assert!(
        error.actual >= Some(units),
        "the reported charge covers the whole document: {error:?}"
    );
    assert_eq!(engine.document_json().unwrap(), before);
}

#[test]
fn two_half_document_deletes_roll_history_under_a_limit_that_fits_only_one() {
    let probe = very_large_engine(crate::yrs_engine::EditingLimits::default());
    let units = leading_delete_undo_units(&probe, LARGE_EDIT_REQUEST_ID + 10, leading_half(&probe));
    let mut engine = very_large_engine(crate::yrs_engine::EditingLimits {
        max_undo_retained_units: units + units / 2,
        ..crate::yrs_engine::EditingLimits::default()
    });
    let first_half = leading_half(&engine);
    delete_leading_scalars(&mut engine, LARGE_EDIT_REQUEST_ID + 11, first_half);
    let after_first = engine.document_json().unwrap();
    let second_half = leading_half(&engine);
    delete_leading_scalars(&mut engine, LARGE_EDIT_REQUEST_ID + 13, second_half);
    assert!(engine.undo(LARGE_EDIT_REQUEST_ID + 15).unwrap().is_some());
    assert_eq!(
        engine.document_json().unwrap(),
        after_first,
        "the surviving undo group restores the document between the two deletes"
    );
    assert!(
        !engine.can_undo(),
        "the first {units}-unit delete must roll out of a {}-unit history",
        units + units / 2
    );
}

#[test]
fn a_large_replacing_paste_still_exceeds_a_tight_encoded_state_limit() {
    const PASTED_BYTES: usize = 4 * ENCODED_STATE_MARGIN_BYTES;
    let mut engine = very_large_engine(crate::yrs_engine::EditingLimits::default());
    let encoded = engine
        .doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default())
        .len();
    let limit = encoded + ENCODED_STATE_MARGIN_BYTES;
    engine.resource_limits.max_encoded_state_bytes = limit;
    delete_leading_scalars(&mut engine, LARGE_EDIT_REQUEST_ID + 20, 1);
    let before = engine.document_json().unwrap();
    let half = leading_half(&engine);
    select_text(&mut engine, LARGE_EDIT_REQUEST_ID + 22, 0, half);
    let error = engine
        .apply_command(
            LARGE_EDIT_REQUEST_ID + 23,
            TypedCommand::Paste {
                fragment: None,
                html: None,
                text: Some("p".repeat(PASTED_BYTES)),
                plain_text: true,
                allow_base64_images: false,
                input_filter: None,
                cell_drop: None,
            },
        )
        .unwrap_err();
    assert_eq!(
        (error.code, error.limit),
        (
            "DOCUMENT_LIMIT_EXCEEDED",
            Some(u64::try_from(limit).unwrap())
        ),
        "replacing half the document with {PASTED_BYTES} bytes must exceed a state limit \
         {ENCODED_STATE_MARGIN_BYTES} bytes above the current encoding, while a one-character \
         delete fits: {error:?}"
    );
    assert_eq!(engine.document_json().unwrap(), before);
}
