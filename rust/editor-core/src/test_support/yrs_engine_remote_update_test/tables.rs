const TABLE_NODE: &str = "table";
const ROW_NODE: &str = "table_row";
const CELL_NODE: &str = "table_cell";
const HEADER_NODE: &str = "table_header";
const PARAGRAPH_NODE: &str = "paragraph";
const IRREGULAR_TABLE_POSITION: u32 = 0;
const TWO_BY_TWO_SLOTS: usize = 4;
const ONE_REMOTE_COMMIT: u64 = 1;

fn tabled_engine(mode: InitializationMode) -> YrsDocumentEngine {
    engine_with(
        crate::tables::tests::tabled_schema(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        mode,
        ResourceLimits::default(),
        EditingLimits::default(),
        None,
    )
}

fn roleless_table_schema() -> Schema {
    Schema::from_json(&serde_json::json!({
        "nodes": [
            {"name":"doc","content":"block+","role":"doc"},
            {"name":PARAGRAPH_NODE,"content":"inline*","group":"block","role":"textBlock"},
            {"name":"text","content":"","group":"inline","role":"text"},
            {"name":TABLE_NODE,"content":format!("{ROW_NODE}*"),"group":"block","role":"block"},
            {"name":ROW_NODE,"content":format!("({CELL_NODE} | {HEADER_NODE})*"),"role":"block"},
            {"name":CELL_NODE,"content":"block+","role":"block","attrs":{
                "colspan":{"default":1},"rowspan":{"default":1},"colwidth":{"default":null}}},
            {"name":HEADER_NODE,"content":"block+","role":"block","attrs":{
                "colspan":{"default":1},"rowspan":{"default":1},"colwidth":{"default":null}}}
        ],
        "marks": []
    }))
    .unwrap()
}

fn table_cell(attrs: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": CELL_NODE,
        "attrs": attrs,
        "content": [{"type": PARAGRAPH_NODE, "content": []}],
    })
}

fn plain_table_cell() -> serde_json::Value {
    table_cell(serde_json::json!({"colspan":1,"rowspan":1,"colwidth":null}))
}

fn table_row(cells: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({"type": ROW_NODE, "content": cells})
}

fn table_document(tables: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({"type": "doc", "content": tables})
}

fn irregular_table() -> serde_json::Value {
    serde_json::json!({
        "type": TABLE_NODE,
        "content": [
            table_row(vec![table_cell(
                serde_json::json!({"colspan":2,"rowspan":1,"colwidth":null}),
            )]),
            table_row(vec![plain_table_cell(), plain_table_cell(), plain_table_cell()]),
        ],
    })
}

fn regular_table() -> serde_json::Value {
    serde_json::json!({
        "type": TABLE_NODE,
        "content": [
            table_row(vec![plain_table_cell(), plain_table_cell()]),
            table_row(vec![plain_table_cell(), plain_table_cell()]),
        ],
    })
}

fn source_holding(schema: Schema, document: serde_json::Value) -> YrsDocumentEngine {
    let mut source = engine_with(
        schema,
        InitializationMode::LocalEmpty,
        ResourceLimits::default(),
        EditingLimits::default(),
        None,
    );
    source
        .import_json(&document.to_string(), TransactionOrigin::DocumentImport)
        .expect("the peer fixture imports its own document");
    source
}

fn irregular_source() -> YrsDocumentEngine {
    source_holding(
        crate::tables::tests::tabled_schema(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        table_document(vec![irregular_table()]),
    )
}

fn projected_irregular_positions(engine: &YrsDocumentEngine) -> Vec<u32> {
    engine
        .table_projection_index()
        .expect("a ready engine carries a table projection index")
        .irregular_positions()
        .collect()
}

#[test]
fn an_irregular_remote_table_lands_byte_exact_without_any_local_repair() {
    let source = irregular_source();
    let update = source.encoded_state().unwrap();
    let mut target = tabled_engine(InitializationMode::AwaitRemote);
    let before = audit(&target);

    let commit = target.apply_remote_update_v1(400, &update).unwrap();

    assert!(commit.changed);
    assert_eq!(
        target.document_json().unwrap(),
        source.document_json().unwrap(),
        "the irregular geometry is admitted exactly as the peer sent it",
    );
    assert_eq!(
        target.encoded_state().unwrap(),
        update,
        "admitting the update must add no local structs of its own",
    );
    assert_eq!(
        target.revision(),
        before.revision + ONE_REMOTE_COMMIT,
        "one remote commit and no follow-up local document update",
    );
    assert_eq!(
        projected_irregular_positions(&target),
        vec![IRREGULAR_TABLE_POSITION],
    );
}

#[test]
fn a_nested_irregular_remote_table_is_reported_at_its_own_position() {
    let nested = serde_json::json!({
        "type": TABLE_NODE,
        "content": [table_row(vec![serde_json::json!({
            "type": CELL_NODE,
            "attrs": {"colspan":1,"rowspan":1,"colwidth":null},
            "content": [irregular_table()],
        })])],
    });
    let source = source_holding(
        crate::tables::tests::tabled_schema(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        table_document(vec![nested]),
    );
    let update = source.encoded_state().unwrap();
    let mut target = tabled_engine(InitializationMode::AwaitRemote);

    target.apply_remote_update_v1(401, &update).unwrap();

    assert_eq!(target.encoded_state().unwrap(), update);
    assert_eq!(projected_irregular_positions(&target), vec![3]);
}

#[test]
fn a_full_state_reconnect_import_keeps_irregular_geometry() {
    let source = irregular_source();
    let update = source.encoded_state().unwrap();
    let mut target = tabled_engine(InitializationMode::AwaitRemote);
    target.apply_remote_update_v1(402, &update).unwrap();
    let seeded = audit(&target);

    let replayed = target.apply_remote_update_v1(403, &update).unwrap();

    assert!(!replayed.changed);
    assert_eq!(audit(&target), seeded);
    assert_eq!(
        projected_irregular_positions(&target),
        vec![IRREGULAR_TABLE_POSITION],
    );
}

#[test]
fn undo_restores_irregular_geometry_without_repairing_it() {
    let source = irregular_source();
    let mut target = tabled_engine(InitializationMode::AwaitRemote);
    target
        .apply_remote_update_v1(404, &source.encoded_state().unwrap())
        .unwrap();
    let admitted = target.document_json().unwrap();
    select_text(&mut target, 405, 3, 3);
    target
        .apply_command(406, TypedCommand::InsertText { text: "x".into() })
        .unwrap()
        .expect("typing into the irregular table applies");
    assert_ne!(target.document_json().unwrap(), admitted);

    target.undo(407).unwrap().expect("undo must apply");

    assert_eq!(
        target.document_json().unwrap(),
        admitted,
        "history restores the irregular geometry verbatim",
    );
    assert_eq!(
        projected_irregular_positions(&target),
        vec![IRREGULAR_TABLE_POSITION],
    );
}

#[test]
fn table_shape_rejections_leave_encoded_state_revisions_history_and_outbox_intact() {
    let mut target = tabled_engine(InitializationMode::AwaitRemote);
    target
        .apply_remote_update_v1(410, &irregular_source().encoded_state().unwrap())
        .unwrap();
    select_text(&mut target, 411, 3, 3);
    target
        .apply_command(412, TypedCommand::InsertText { text: "y".into() })
        .unwrap()
        .expect("a local edit gives history something to protect");

    let bad_spans = [
        serde_json::json!(0),
        serde_json::json!(-1),
        serde_json::json!(1.5),
    ];
    let mut rejected: Vec<serde_json::Value> = Vec::new();
    for span in bad_spans {
        rejected.push(table_document(vec![serde_json::json!({
            "type": TABLE_NODE,
            "content": [table_row(vec![table_cell(
                serde_json::json!({"colspan": span, "rowspan": 1, "colwidth": null}),
            )])],
        })]));
    }
    rejected.push(table_document(vec![serde_json::json!({
        "type": TABLE_NODE,
        "content": [table_row(vec![table_cell(
            serde_json::json!({"colspan": 1, "rowspan": 1, "colwidth": ["120"]}),
        )])],
    })]));

    let mut request_id = 420;
    for document in rejected {
        let source = source_holding(roleless_table_schema(), document.clone());
        let before = audit(&target);
        let outbox = CollaborationOutbox::with_ceilings(4, 1024);
        let pending_before = outbox.pending_document_update_count();

        let error = target
            .apply_remote_update_v1(request_id, &source.encoded_state().unwrap())
            .unwrap_err();

        assert_eq!(error.code, "DOCUMENT_INVALID", "{document}");
        assert_eq!(audit(&target), before, "{document}");
        assert_eq!(outbox.pending_document_update_count(), pending_before);
        assert!(target.can_undo());
        request_id += 1;
    }
}

#[test]
fn the_aggregate_grid_ceiling_rejects_atomically_and_reports_its_field() {
    let source = source_holding(
        crate::tables::tests::tabled_schema(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        table_document(vec![regular_table(), regular_table()]),
    );
    let update = source.encoded_state().unwrap();
    let mut generous = tabled_engine(InitializationMode::AwaitRemote);
    generous.apply_remote_update_v1(430, &update).unwrap();
    assert_eq!(generous.encoded_state().unwrap(), update);

    let starved_limits = ResourceLimits {
        max_table_grid_slots: TWO_BY_TWO_SLOTS * 2 - 1,
        ..ResourceLimits::default()
    };
    let mut starved = engine_with(
        crate::tables::tests::tabled_schema(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
        InitializationMode::AwaitRemote,
        starved_limits,
        EditingLimits::default(),
        None,
    );
    let before = audit(&starved);

    let error = starved.apply_remote_update_v1(431, &update).unwrap_err();

    assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
    assert_eq!(error.details.as_ref().unwrap()["field"], "update");
    assert_eq!(error.details.as_ref().unwrap()["phase"], "tableGrid");
    assert_eq!(error.limit, Some((TWO_BY_TWO_SLOTS * 2 - 1) as u64));
    assert_eq!(error.actual, Some((TWO_BY_TWO_SLOTS * 2) as u64));
    assert_eq!(audit(&starved), before);
}

#[test]
fn a_remote_document_that_breaks_the_table_content_expression_is_still_rejected() {
    let mut target = tabled_engine(InitializationMode::AwaitRemote);
    let before = audit(&target);
    let permissive = Schema::from_json(&serde_json::json!({
        "nodes": [
            {"name":"doc","content":"block+","role":"doc"},
            {"name":PARAGRAPH_NODE,"content":"inline*","group":"block","role":"textBlock"},
            {"name":"text","content":"","group":"inline","role":"text"},
            {"name":ROW_NODE,"content":"block*","group":"block","role":"block"}
        ],
        "marks": []
    }))
    .unwrap();
    let foreign = source_holding(
        permissive,
        serde_json::json!({"type":"doc","content":[
            {"type":ROW_NODE,"content":[{"type":PARAGRAPH_NODE,"content":[]}]}]}),
    );

    let error = target
        .apply_remote_update_v1(440, &foreign.encoded_state().unwrap())
        .unwrap_err();

    assert_eq!(error.code, "DOCUMENT_INVALID");
    assert_eq!(audit(&target), before);
}

fn scoped_tabled_engine(schema: Schema) -> YrsDocumentEngine {
    YrsDocumentEngine::new(YrsEngineConfig {
        schema,
        fragment_name: "prosemirror".into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: Some(crate::yrs_engine::DocumentScope {
            document_id: "table-document".into(),
            lineage_id: "table-lineage".into(),
        }),
    })
    .unwrap()
}

#[test]
fn a_tabled_snapshot_restored_into_a_table_free_engine_reports_a_schema_mismatch() {
    let source = scoped_tabled_engine(crate::tables::tests::tabled_schema(
        crate::tables::tests::PROSEMIRROR_TABLE_NAMES,
    ));
    let snapshot = source.export_snapshot().unwrap();
    let mut target = scoped_tabled_engine(tiptap_schema());
    let before = audit(&target);

    let error = target.restore_snapshot(&snapshot).unwrap_err();

    assert_eq!(error.code, "SNAPSHOT_SCHEMA_MISMATCH");
    assert_eq!(audit(&target), before);
}
