const PROSE_CARET_SCALAR: u32 = 2;
const PROSE_DOCUMENT_POSITION: u64 = 2;
const CELL_TEXT_DEPTH: u64 = 2;
const FIRST_CELL: usize = 0;
const SECOND_CELL: usize = 1;
const DELETION_REQUEST_ID: u64 = 2201;
const NOT_APPLICABLE_OUTCOME: &str = "notApplicable";
const TRANSACTION_OUTCOME: &str = "transaction";

fn table_deletion_cell(text: &str) -> Value {
    json!({ "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
}

fn table_deletion_prose(text: &str) -> Value {
    json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
}

fn regular_deletion_table() -> Value {
    json!({ "type": "table", "content": [{ "type": "table_row", "content": [
        table_deletion_cell("left"), table_deletion_cell("right")
    ] }] })
}

fn empty_deletion_table() -> Value {
    json!({ "type": "table" })
}

fn cellless_deletion_table() -> Value {
    json!({ "type": "table", "content": [{ "type": "table_row" }] })
}

fn nested_deletion_table() -> Value {
    json!({ "type": "table", "content": [{ "type": "table_row", "content": [
        { "type": "table_cell", "content": [regular_deletion_table()] }
    ] }] })
}

fn table_deletion_editor(blocks: Vec<Value>) -> String {
    create_handle(json!({
        "schema": crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        "initialization": { "type": "localJson", "json": { "type": "doc", "content": blocks } },
    }))
}

fn render_of(id: &str) -> Value {
    ok_json(&v2_render::editor_v2_render_update(
        id.to_string(),
        None,
        None,
    ))
}

fn table_records_by_position(render: &Value) -> Vec<Value> {
    let mut records: Vec<Value> = render["tableRecords"]
        .as_object()
        .expect("the render publishes table records")
        .values()
        .cloned()
        .collect();
    records.sort_by_key(|record| record["tablePos"].as_u64().expect("tablePos"));
    records
}

fn delete_table_at(id: &str, table_pos: u64) -> Value {
    ok_json(&v2::editor_v2_apply_command(
        id.to_string(),
        command_envelope(
            DELETION_REQUEST_ID,
            revision_of(id),
            json!({ "type": "deleteTable", "tablePos": table_pos }),
        ),
    ))
}

fn place_prose_caret(id: &str) {
    ok_json(&v2::editor_v2_set_selection(
        id.to_string(),
        selection_envelope(
            DELETION_REQUEST_ID,
            revision_of(id),
            PROSE_CARET_SCALAR,
            PROSE_CARET_SCALAR,
        ),
    ));
}

fn holds_a_table(document: &Value) -> bool {
    document.to_string().contains("\"type\":\"table\"")
}

#[test]
fn explicit_table_deletion_removes_regular_empty_and_cellless_frames_and_keeps_the_prose_caret() {
    for (label, frame) in [
        ("regular table", regular_deletion_table()),
        ("empty table frame", empty_deletion_table()),
        ("cell-less table frame", cellless_deletion_table()),
    ] {
        let id = table_deletion_editor(vec![
            table_deletion_prose("before"),
            frame,
            table_deletion_prose("after"),
        ]);
        place_prose_caret(&id);
        let before_document = document_json_of(&id);
        let before_render = render_of(&id);
        let records = table_records_by_position(&before_render);
        assert_eq!(records.len(), 1, "{label}: {records:?}");
        let record = &records[0];
        assert_eq!(
            record["readOnlyDescendants"], false,
            "{label}: an outer frame is published as mutable: {record}"
        );
        assert_eq!(
            before_render["activeState"]["commands"]["deleteTable"], false,
            "{label}: a prose caret never enables the anchored delete"
        );
        assert_eq!(state_of(&id)["canUndo"], false, "{label}");

        let outcome = delete_table_at(&id, record["tablePos"].as_u64().unwrap());

        assert_eq!(outcome["type"], TRANSACTION_OUTCOME, "{label}: {outcome}");
        assert_eq!(outcome["changed"], true, "{label}: {outcome}");
        let after_document = document_json_of(&id);
        assert!(
            !holds_a_table(&after_document),
            "{label}: the frame is gone: {after_document}"
        );
        assert_eq!(
            after_document["content"],
            json!([
                table_deletion_prose("before"),
                table_deletion_prose("after")
            ]),
            "{label}"
        );
        let after_render = render_of(&id);
        assert_eq!(
            after_render["selection"], before_render["selection"],
            "{label}: a caret outside the table stays where it was"
        );
        assert_eq!(state_of(&id)["canUndo"], true, "{label}");

        assert_eq!(
            ok_json(&v2::editor_v2_undo(
                id.clone(),
                history_envelope(DELETION_REQUEST_ID + 1)
            )),
            json!({ "changed": true }),
            "{label}"
        );
        assert_eq!(
            document_json_of(&id),
            before_document,
            "{label}: undo restores the frame"
        );
        assert_eq!(
            render_of(&id)["selection"],
            before_render["selection"],
            "{label}: undo restores the prose caret"
        );
        assert_eq!(
            state_of(&id)["canUndo"],
            false,
            "{label}: the deletion was exactly one history entry"
        );
        destroy_handle(&id);
    }
}

fn scalar_of(id: &str, doc_pos: u64) -> u32 {
    ok_json(&v2_render::editor_v2_doc_to_scalar(
        id.to_string(),
        u32::try_from(doc_pos).expect("fixture positions fit u32"),
    ))["scalar"]
        .as_u64()
        .and_then(|scalar| u32::try_from(scalar).ok())
        .expect("doc_to_scalar returns a u32 scalar")
}

fn cell_interior(record: &Value, cell: usize) -> u64 {
    record["cells"][cell]["sourcePos"].as_u64().unwrap() + CELL_TEXT_DEPTH
}

fn select_cell_rectangle(id: &str, record: &Value) {
    let cell = record["cells"][SECOND_CELL]["sourcePos"].clone();
    ok_json(&v2::editor_v2_set_selection(
        id.to_string(),
        exact_cell_request(
            revision_of(id),
            document_cell_point(cell.as_u64().unwrap()),
            document_cell_point(cell.as_u64().unwrap()),
        )
        .to_string(),
    ));
}

fn select_prose_into_cell(id: &str, record: &Value) {
    let head = scalar_of(id, cell_interior(record, SECOND_CELL));
    ok_json(&v2::editor_v2_set_selection(
        id.to_string(),
        selection_envelope(
            DELETION_REQUEST_ID,
            revision_of(id),
            PROSE_CARET_SCALAR,
            head,
        ),
    ));
}

fn select_cell_into_prose(id: &str, record: &Value) {
    let anchor = scalar_of(id, cell_interior(record, FIRST_CELL));
    ok_json(&v2::editor_v2_set_selection(
        id.to_string(),
        selection_envelope(
            DELETION_REQUEST_ID,
            revision_of(id),
            anchor,
            PROSE_CARET_SCALAR,
        ),
    ));
}

#[test]
fn explicit_table_deletion_lands_like_the_anchored_delete_when_any_endpoint_is_inside() {
    let blocks = vec![table_deletion_prose("before"), regular_deletion_table()];
    let anchored = table_deletion_editor(blocks.clone());
    let record = table_records_by_position(&render_of(&anchored))[0].clone();
    select_cell_rectangle(&anchored, &record);
    assert_eq!(
        render_of(&anchored)["activeState"]["commands"]["deleteTable"],
        true
    );
    let anchored_outcome = ok_json(&v2::editor_v2_apply_command(
        anchored.clone(),
        command_envelope(
            DELETION_REQUEST_ID,
            revision_of(&anchored),
            json!({ "type": "deleteTable" }),
        ),
    ));
    assert_eq!(
        anchored_outcome["type"], TRANSACTION_OUTCOME,
        "{anchored_outcome}"
    );
    let expected_document = document_json_of(&anchored);
    let expected_selection = render_of(&anchored)["selection"].clone();
    assert_eq!(expected_selection["type"], "text", "{expected_selection}");
    destroy_handle(&anchored);

    let selections: [(&str, fn(&str, &Value)); 3] = [
        ("cell rectangle", select_cell_rectangle),
        ("text from prose into a cell", select_prose_into_cell),
        ("text from a cell into prose", select_cell_into_prose),
    ];
    for (label, select) in selections {
        let id = table_deletion_editor(blocks.clone());
        select(&id, &record);
        let selected = render_of(&id)["selection"].clone();
        assert_ne!(
            selected, expected_selection,
            "{label}: the fixture must start elsewhere"
        );

        let outcome = delete_table_at(&id, record["tablePos"].as_u64().unwrap());

        assert_eq!(outcome["type"], TRANSACTION_OUTCOME, "{label}: {outcome}");
        assert_eq!(document_json_of(&id), expected_document, "{label}");
        assert_eq!(
            render_of(&id)["selection"],
            expected_selection,
            "{label}: a selection {selected} touching the deleted table lands where the anchored \
             delete puts it"
        );
        destroy_handle(&id);
    }
}

#[test]
fn explicit_table_deletion_declines_nested_tables_and_positions_without_an_outer_table() {
    let id = table_deletion_editor(vec![
        table_deletion_prose("before"),
        nested_deletion_table(),
    ]);
    place_prose_caret(&id);
    let records = table_records_by_position(&render_of(&id));
    assert_eq!(records.len(), 2, "{records:?}");
    let outer = &records[0];
    let nested = &records[1];
    assert_eq!(outer["readOnlyDescendants"], false, "{outer}");
    assert_eq!(nested["readOnlyDescendants"], true, "{nested}");
    let outer_pos = outer["tablePos"].as_u64().unwrap();
    let document_end = outer["sourceEnd"].as_u64().unwrap();
    let before_document = document_json_of(&id);
    let before_revision = revision_of(&id);

    for (label, table_pos) in [
        ("nested table", nested["tablePos"].as_u64().unwrap()),
        ("prose paragraph", PROSE_DOCUMENT_POSITION),
        (
            "inside the outer table",
            outer_pos + u64::from(crate::tables::commands::NODE_OPENING_TOKENS),
        ),
        ("document end", document_end),
        ("beyond the document", u64::from(u32::MAX)),
    ] {
        let outcome = delete_table_at(&id, table_pos);
        assert_eq!(
            outcome["type"], NOT_APPLICABLE_OUTCOME,
            "{label} at {table_pos}: {outcome}"
        );
        assert_eq!(document_json_of(&id), before_document, "{label}");
        assert_eq!(revision_of(&id), before_revision, "{label}");
    }
    assert_eq!(state_of(&id)["canUndo"], false);
    destroy_handle(&id);
}

#[test]
fn published_outer_table_records_are_exactly_the_explicitly_deletable_tables() {
    let blocks = vec![
        table_deletion_prose("before"),
        regular_deletion_table(),
        empty_deletion_table(),
        cellless_deletion_table(),
        nested_deletion_table(),
    ];
    let probe = table_deletion_editor(blocks.clone());
    let records = table_records_by_position(&render_of(&probe));
    destroy_handle(&probe);
    assert_eq!(records.len(), 5, "{records:?}");

    for record in records {
        let id = table_deletion_editor(blocks.clone());
        place_prose_caret(&id);
        let outcome = delete_table_at(&id, record["tablePos"].as_u64().unwrap());
        let deletable = outcome["type"] == TRANSACTION_OUTCOME;
        assert_eq!(
            record["readOnlyDescendants"] == false,
            deletable,
            "the published record must predict explicit deletion: {record} -> {outcome}"
        );
        destroy_handle(&id);
    }
}
