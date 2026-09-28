fn table_mapping_snapshot(
    document: serde_json::Value,
    schema: serde_json::Value,
) -> serde_json::Value {
    let editor_id = create_editor(serde_json::json!({
        "schema": schema,
        "initialization": { "type": "localJson", "json": document },
    }));
    let result = super::render::editor_v2_render_update(editor_id.clone(), None, None);
    assert_eq!(
        super::editor::editor_v2_destroy(editor_id).value,
        Some(true)
    );
    serde_json::from_str(result.value.as_deref().expect("render snapshot")).expect("snapshot JSON")
}

#[test]
fn native_table_source_identity_survives_preceding_and_cell_text_edits() {
    let mut schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    schema["nodes"]
        .as_array_mut()
        .expect("nodes")
        .push(serde_json::json!({
            "name": "card", "content": "", "group": "block", "role": "block", "isVoid": true
        }));
    let editor_id = create_editor(serde_json::json!({
        "schema": schema,
        "initialization": { "type": "localJson", "json": {
            "type": "doc", "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "before" }] },
                { "type": "table", "content": [{ "type": "table_row", "content": [{
                    "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "inside" }] }]
                }] }] },
                { "type": "card" }
            ]
        } },
    }));
    let render = || {
        let result =
            super::render::editor_v2_render_native(editor_id.clone(), "22".into(), None, None);
        serde_json::from_str::<serde_json::Value>(result.value.as_deref().expect("native render"))
            .expect("snapshot JSON")
    };
    let full_render = || {
        let result = super::render::editor_v2_render_update(editor_id.clone(), None, None);
        serde_json::from_str::<serde_json::Value>(result.value.as_deref().expect("full render"))
            .expect("full snapshot JSON")
    };
    let schema_fingerprint = super::editor::with_editor(&editor_id, |session| {
        Ok(session.engine.schema_fingerprint().to_owned())
    })
    .expect("initial schema fingerprint");
    let initial_document: serde_json::Value = serde_json::from_str(
        super::editor::editor_v2_get_document_json(editor_id.clone())
            .value
            .as_deref()
            .expect("initial document"),
    )
    .expect("initial document JSON");
    let first = render();
    let atom_id = full_render()["renderBlocks"][2][0]["atomId"].clone();
    assert!(atom_id.as_str().is_some_and(|id| id.starts_with('y')));
    let (first_key, first_record) = first["tableRecords"]
        .as_object()
        .expect("first table records")
        .iter()
        .next()
        .expect("first table");
    let source_id = first_record["sourceId"].clone();
    assert!(source_id.as_str().is_some_and(|id| id.starts_with('y')));

    let select = |request_id: &str, revision: &serde_json::Value, scalar: u64| {
        super::editor::editor_v2_set_selection(
            editor_id.clone(),
            serde_json::json!({
                "version": 1, "requestId": request_id, "baseDocumentRevision": revision,
                "selection": {
                    "type": "text",
                    "anchor": { "offset": scalar, "kind": "scalar", "affinity": "after" },
                    "head": { "offset": scalar, "kind": "scalar", "affinity": "after" },
                }
            })
            .to_string(),
        )
    };
    let insert = |request_id: &str, revision: &serde_json::Value, text: &str| {
        super::editor::editor_v2_apply_command(
            editor_id.clone(),
            serde_json::json!({
                "version": 1, "requestId": request_id, "baseDocumentRevision": revision,
                "command": { "type": "insertText", "text": text }
            })
            .to_string(),
        )
    };
    let selected = select("1", &first["documentVersion"], 0);
    assert!(
        selected.error.is_none(),
        "select preceding text: {:?}",
        selected.error
    );
    let inserted = insert("2", &first["documentVersion"], "x");
    assert!(
        inserted.error.is_none(),
        "edit preceding text: {:?}",
        inserted.error
    );
    let after_prefix: serde_json::Value = serde_json::from_str(
        super::editor::editor_v2_get_document_json(editor_id.clone())
            .value
            .as_deref()
            .expect("document after preceding edit"),
    )
    .expect("document JSON");
    assert_eq!(after_prefix["content"][0]["content"][0]["text"], "xbefore");
    let second = render();
    assert!(second["renderPatch"].is_object());
    assert_ne!(second["documentVersion"], first["documentVersion"]);
    let (second_key, second_record) = second["tableRecords"]
        .as_object()
        .expect("second table records")
        .iter()
        .next()
        .expect("second table");
    assert_ne!(first_key, second_key);
    assert_eq!(source_id, second_record["sourceId"]);
    assert_eq!(full_render()["renderBlocks"][2][0]["atomId"], atom_id);

    let scalar_start = second["tableInputMappings"]["tables"][second_key]["extent"]["scalarStart"]
        .as_u64()
        .expect("cell scalar start");
    let selected = select("3", &second["documentVersion"], scalar_start + 1);
    assert!(
        selected.error.is_none(),
        "select cell text: {:?}",
        selected.error
    );
    let inserted = insert("4", &second["documentVersion"], "z");
    assert!(
        inserted.error.is_none(),
        "edit cell text: {:?}",
        inserted.error
    );
    let after_cell: serde_json::Value = serde_json::from_str(
        super::editor::editor_v2_get_document_json(editor_id.clone())
            .value
            .as_deref()
            .expect("document after cell edit"),
    )
    .expect("document JSON");
    assert_eq!(
        after_cell["content"][1]["content"][0]["content"][0]["content"][0]["content"][0]["text"],
        "iznside",
        "after cell: {after_cell}"
    );
    let third = render();
    assert!(third["renderPatch"].is_object());
    assert_ne!(third["documentVersion"], second["documentVersion"]);
    let third_record = third["tableRecords"][second_key]
        .as_object()
        .expect("retained table");
    assert_eq!(third_record["sourceId"], source_id);
    assert_eq!(third_record["tablePos"], second_record["tablePos"]);
    assert_eq!(full_render()["renderBlocks"][2][0]["atomId"], atom_id);
    assert_eq!(
        after_cell["content"][1].get("attrs"),
        initial_document["content"][1].get("attrs")
    );
    assert!(!after_cell.to_string().contains("\"sourceId\""));
    let final_schema_fingerprint = super::editor::with_editor(&editor_id, |session| {
        Ok(session.engine.schema_fingerprint().to_owned())
    })
    .expect("final schema fingerprint");
    assert_eq!(final_schema_fingerprint, schema_fingerprint);
    assert_eq!(
        super::editor::editor_v2_destroy(editor_id).value,
        Some(true)
    );
}

#[test]
fn native_table_source_identity_follows_custom_schema_role_and_keeps_atom_ids() {
    let names = ["gridPanel", "gridRow", "gridCell", "gridHeader"];
    let mut schema = crate::tables::tests::tabled_schema_json(names);
    schema["nodes"]
        .as_array_mut()
        .expect("nodes")
        .push(serde_json::json!({
            "name": "card", "content": "", "group": "block", "role": "block", "isVoid": true
        }));
    let snapshot = table_mapping_snapshot(
        serde_json::json!({ "type": "doc", "content": [
            { "type": "card" },
            { "type": "gridPanel", "content": [{ "type": "gridRow", "content": [{
                "type": "gridCell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "cell" }] }]
            }] }] }
        ] }),
        schema,
    );
    let atom_id = snapshot["renderBlocks"][0][0]["atomId"]
        .as_str()
        .expect("existing atom ID");
    let table_id = snapshot["renderBlocks"][1][0]["tableId"]
        .as_str()
        .expect("custom role table");
    let source_id = snapshot["tableRecords"][table_id]["sourceId"]
        .as_str()
        .expect("custom role source ID");
    assert!(atom_id.starts_with('y'));
    assert!(source_id.starts_with('y'));
    assert_ne!(atom_id, source_id);
}

#[test]
fn table_input_mapping_snapshot_associates_middle_table_cell_blocks_with_effective_coordinates() {
    let schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    let snapshot = table_mapping_snapshot(
        serde_json::json!({
            "type": "doc",
            "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "before" }] },
                { "type": "table", "content": [{ "type": "table_row", "content": [
                    { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "left" }] }] },
                    { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "right" }] }] }
                ] }] },
                { "type": "paragraph", "content": [{ "type": "text", "text": "after" }] }
            ]
        }),
        schema,
    );

    assert_eq!(
        snapshot["tableInputMappings"]["version"],
        serde_json::json!(1)
    );
    assert_eq!(
        snapshot["tableInputMappings"]["tables"]
            .as_object()
            .map(|tables| tables.len()),
        Some(1)
    );
    assert_eq!(
        snapshot["tableInputMappings"]["tables"]["t8"]["extent"],
        serde_json::json!({ "scalarStart": 7, "scalarEnd": 17 })
    );
    assert!(snapshot["tableRecords"]["t8"]["sourceId"]
        .as_str()
        .is_some_and(|id| id.starts_with('y') && id.contains('-')));
    assert_eq!(
        snapshot["tableInputMappings"]["tables"]["t8"]["cells"][0]["blocks"][0],
        serde_json::json!({
            "elementIndex": 0,
            "docStart": 12,
            "docEnd": 16,
            "scalarStart": 7,
            "contentScalarStart": 7,
            "scalarEnd": 11,
            "breakScalarEnd": 11,
            "void": false,
        })
    );
}

#[test]
fn table_input_mapping_keeps_empty_and_multiple_cell_blocks_separate() {
    let mut schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    schema["nodes"]
        .as_array_mut()
        .expect("nodes")
        .push(serde_json::json!({
            "name": "heading", "content": "inline*", "group": "block", "role": "textBlock"
        }));
    let snapshot = table_mapping_snapshot(
        serde_json::json!({
            "type": "doc",
            "content": [{ "type": "table", "content": [{ "type": "table_row", "content": [
                { "type": "table_cell", "content": [{ "type": "paragraph" }, { "type": "heading", "content": [{ "type": "text", "text": "two" }] }] }
            ] }] }]
        }),
        schema,
    );

    let cell = &snapshot["tableInputMappings"]["tables"]["t0"]["cells"][0];
    assert_eq!(cell["blocks"].as_array().map(Vec::len), Some(2));
    assert_eq!(cell["blocks"][0]["elementIndex"], serde_json::json!(0));
    assert_eq!(cell["blocks"][1]["elementIndex"], serde_json::json!(3));
    assert_ne!(
        cell["blocks"][0]["scalarEnd"].as_u64(),
        cell["blocks"][0]["breakScalarEnd"].as_u64()
    );
    assert_eq!(
        cell["blocks"][1]["scalarStart"].as_u64(),
        cell["blocks"][0]["breakScalarEnd"].as_u64()
    );
    assert_eq!(
        cell["blocks"][0]["scalarEnd"].as_u64().unwrap()
            - cell["blocks"][0]["contentScalarStart"].as_u64().unwrap(),
        1
    );
}

#[test]
fn table_input_mapping_excludes_nested_table_blocks_and_maps_the_nested_record() {
    let schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    let snapshot = table_mapping_snapshot(
        serde_json::json!({
            "type": "doc",
            "content": [{ "type": "table", "content": [{ "type": "table_row", "content": [
                { "type": "table_cell", "content": [{ "type": "table", "content": [{ "type": "table_row", "content": [
                    { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "nested" }] }] }
                ] }] }] }
            ] }] }]
        }),
        schema,
    );

    let tables = snapshot["tableInputMappings"]["tables"]
        .as_object()
        .expect("tables");
    assert_eq!(tables.len(), 2);
    let records = snapshot["tableRecords"].as_object().expect("table records");
    let source_ids: std::collections::HashSet<_> = records
        .values()
        .map(|record| record["sourceId"].as_str().expect("stable source identity"))
        .collect();
    assert_eq!(source_ids.len(), records.len());
    let outer = tables.get("t0").expect("outer table");
    assert!(outer["cells"][0]["blocks"]
        .as_array()
        .is_some_and(Vec::is_empty));
    let excluded = &outer["cells"][0]["excluded"][0];
    assert_eq!(excluded["elementIndex"], serde_json::json!(0));
    assert!(excluded["tableId"]
        .as_str()
        .is_some_and(|id| tables.contains_key(id)));
    assert!(excluded["extent"].is_object());
    let nested = tables
        .get(excluded["tableId"].as_str().expect("nested id"))
        .expect("nested mapping");
    assert_eq!(
        nested["cells"][0]["blocks"].as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn table_input_mapping_omits_the_sidecar_for_legacy_table_free_snapshots() {
    let snapshot = table_mapping_snapshot(
        serde_json::json!({ "type": "doc", "content": [{ "type": "paragraph" }] }),
        serde_json::json!({
            "nodes": [
                { "name": "doc", "content": "paragraph+", "role": "doc" },
                { "name": "paragraph", "content": "inline*", "group": "block", "role": "textBlock" },
                { "name": "text", "group": "inline", "role": "text" }
            ],
            "marks": []
        }),
    );
    assert!(snapshot.get("tableInputMappings").is_none());
}

#[test]
fn table_input_mapping_uses_custom_list_prefixes_and_void_block_elements() {
    let mut schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    let nodes = schema["nodes"].as_array_mut().expect("nodes");
    nodes.extend([
        serde_json::json!({ "name": "todoList", "content": "todoTask+", "group": "block", "role": "list" }),
        serde_json::json!({ "name": "todoTask", "content": "paragraph+", "role": "listItem" }),
        serde_json::json!({ "name": "inlineWidget", "content": "", "group": "inline", "role": "inline", "isVoid": true }),
        serde_json::json!({ "name": "rule", "content": "", "group": "block", "role": "block", "isVoid": true }),
    ]);
    let snapshot = table_mapping_snapshot(
        serde_json::json!({
            "type": "doc",
            "content": [{ "type": "table", "content": [{ "type": "table_row", "content": [
                { "type": "table_cell", "content": [{ "type": "todoList", "content": [{ "type": "todoTask", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "x" }, { "type": "inlineWidget" }] }] }] }] },
                { "type": "table_cell", "content": [{ "type": "rule" }] }
            ] }] }]
        }),
        schema,
    );

    let cells = &snapshot["tableInputMappings"]["tables"]["t0"]["cells"];
    assert!(
        cells[0]["blocks"][0]["contentScalarStart"].as_u64()
            > cells[0]["blocks"][0]["scalarStart"].as_u64()
    );
    assert_eq!(cells[1]["blocks"][0]["elementIndex"], serde_json::json!(0));
    assert_eq!(cells[1]["blocks"][0]["void"], serde_json::json!(true));
}

#[test]
fn table_input_mapping_excludes_the_separator_between_adjacent_root_tables() {
    let schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    let table = |text| {
        serde_json::json!({ "type": "table", "content": [{ "type": "table_row", "content": [
        { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] }
    ] }] })
    };
    let snapshot = table_mapping_snapshot(
        serde_json::json!({ "type": "doc", "content": [table("a"), table("b")] }),
        schema,
    );

    let tables = snapshot["tableInputMappings"]["tables"]
        .as_object()
        .expect("tables");
    let extents: Vec<_> = tables
        .values()
        .map(|table| table["extent"].clone())
        .collect();
    assert_eq!(extents.len(), 2);
    assert_eq!(
        extents[1]["scalarStart"].as_u64().unwrap() - extents[0]["scalarEnd"].as_u64().unwrap(),
        1
    );
}

#[test]
fn table_input_mapping_is_complete_on_patch_snapshots_after_an_ordered_prefix_change() {
    let mut schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    let nodes = schema["nodes"].as_array_mut().expect("nodes");
    nodes.extend([
        serde_json::json!({ "name": "customOrderedList", "content": "customItem+", "group": "block", "role": "list", "attrs": { "start": { "type": "number", "default": 1, "min": 1 } } }),
        serde_json::json!({ "name": "customItem", "content": "paragraph+", "role": "listItem" }),
    ]);
    let document = |start| {
        serde_json::json!({
            "type": "doc",
            "content": [
                { "type": "customOrderedList", "attrs": { "start": start }, "content": [{ "type": "customItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "prefix" }] }] }] },
                { "type": "table", "content": [{ "type": "table_row", "content": [
                    { "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "target" }] }] }
                ] }] }
            ]
        })
    };
    let editor_id = create_editor(serde_json::json!({
        "schema": schema,
        "initialization": { "type": "localJson", "json": document(9) },
    }));
    let first = super::render::editor_v2_render_native(editor_id.clone(), "9".into(), None, None);
    let first: serde_json::Value =
        serde_json::from_str(first.value.as_deref().expect("first snapshot")).expect("first JSON");
    let replacement = super::editor::editor_v2_replace_document(
        editor_id.clone(),
        serde_json::json!({
            "version": 1,
            "requestId": "1",
            "history": "resetAndClear",
            "setJson": document(100),
        })
        .to_string(),
    );
    assert!(
        replacement.error.is_none(),
        "replace: {:?}",
        replacement.error
    );
    let second = super::render::editor_v2_render_native(editor_id.clone(), "9".into(), None, None);
    assert_eq!(
        super::editor::editor_v2_destroy(editor_id).value,
        Some(true)
    );
    let second: serde_json::Value =
        serde_json::from_str(second.value.as_deref().expect("second snapshot"))
            .expect("second JSON");

    assert!(second["renderPatch"].is_object());
    assert!(second["tableInputMappings"].is_object());
    let (table_id, first_mapping) = first["tableInputMappings"]["tables"]
        .as_object()
        .expect("first mappings")
        .iter()
        .next()
        .expect("target table");
    let second_mapping = &second["tableInputMappings"]["tables"][table_id];
    let first_block = &first_mapping["cells"][0]["blocks"][0];
    let second_block = &second_mapping["cells"][0]["blocks"][0];
    assert_eq!(first_block["docStart"], second_block["docStart"]);
    assert_eq!(
        second_mapping["extent"]["scalarStart"].as_u64().unwrap()
            - first_mapping["extent"]["scalarStart"].as_u64().unwrap(),
        2
    );
    assert_eq!(
        second_mapping["extent"]["scalarEnd"].as_u64().unwrap()
            - first_mapping["extent"]["scalarEnd"].as_u64().unwrap(),
        2
    );
    assert_eq!(
        first["tableRecords"][table_id]["cells"][0]["sourcePos"],
        second["tableRecords"][table_id]["cells"][0]["sourcePos"]
    );
    assert_eq!(
        first["tableRecords"][table_id]["cells"][0]["contentKey"],
        second["tableRecords"][table_id]["cells"][0]["contentKey"]
    );
    assert_ne!(
        first["tableRecords"][table_id]["sourceId"],
        second["tableRecords"][table_id]["sourceId"]
    );
}

#[test]
fn table_input_mapping_keeps_zero_leaf_nested_tables_explicit() {
    let snapshot = table_mapping_snapshot(
        serde_json::json!({ "type": "doc", "content": [{ "type": "table", "content": [{
            "type": "table_row", "content": [{ "type": "table_cell", "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "a" }] },
                { "type": "table", "content": [{ "type": "table_row" }] },
                { "type": "paragraph", "content": [{ "type": "text", "text": "b" }] }
            ] }]
        }] }] }),
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES),
    );
    let mappings = &snapshot["tableInputMappings"]["tables"];
    assert_eq!(mappings["t6"]["extent"], serde_json::Value::Null);
    assert_eq!(
        mappings["t0"]["extent"],
        serde_json::json!({ "scalarStart": 0, "scalarEnd": 3 })
    );
    assert_eq!(
        mappings["t0"]["cells"][0]["excluded"],
        serde_json::json!([{
            "elementIndex": 3, "tableId": "t6", "extent": null
        }])
    );
    assert_eq!(
        mappings["t0"]["cells"][0]["blocks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn table_input_mapping_preserves_extent_without_editable_cells_for_nested_failure() {
    let schema = crate::tables::tests::tabled_schema(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    let document = crate::serialize::from_prosemirror_json(
        &serde_json::json!({ "type": "doc", "content": [{ "type": "table", "content": [{
            "type": "table_row", "content": [{ "type": "table_cell", "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "a" }] },
                { "type": "table", "content": [{ "type": "table_row", "content": [{
                    "type": "table_cell", "attrs": { "colspan": 0 }, "content": [{
                        "type": "paragraph", "content": [{ "type": "text", "text": "XY" }]
                    }]
                }] }] },
                { "type": "paragraph", "content": [{ "type": "text", "text": "b" }] }
            ] }]
        }] }] }),
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    let cache = crate::render::incremental::CachedRenderBlocks::build(
        &document,
        &schema,
        &crate::boundary::ResourceLimits::default(),
    )
    .unwrap();
    let positions = crate::position::PositionMap::build(&document, &schema);
    let mapping = super::table_input_mapping::derive(&document, &positions, &cache)
        .unwrap()
        .unwrap();
    let records: serde_json::Value =
        serde_json::from_str(&super::render::serialize_render_cache_for_test(
            &cache,
            &document,
            &std::collections::HashMap::from([(0, "y0-0".to_owned())]),
        ))
        .unwrap();
    assert_eq!(
        records["tableRecords"]["t0"]["failure"],
        "invalidAttributes"
    );
    assert_eq!(mapping["tables"].as_object().unwrap().len(), 1);
    assert_eq!(mapping["tables"]["t0"]["cells"], serde_json::json!([]));
    assert_eq!(
        mapping["tables"]["t0"]["extent"],
        serde_json::json!({ "scalarStart": 0, "scalarEnd": 6 })
    );
}

#[test]
fn a_render_after_a_keystroke_does_not_walk_the_yrs_tree() {
    use crate::test_support::large_table_fixture::{
        ffi_editor_with_document, ffi_value, plain_table_document,
    };
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };
    const OWNER: &str = "92";
    const REQUEST: &str = "93";
    const SIDE: usize = 3;
    let editor_id = ffi_editor_with_document(&plain_table_document(SIDE, SIDE));
    let first = ffi_value(&super::render::editor_v2_render_native(
        editor_id.clone(),
        OWNER.into(),
        None,
        None,
    ));
    ffi_value(&super::editor::editor_v2_apply_native_intent(editor_id.clone(), json!({
        "version":1, "requestId":REQUEST, "ownerId":OWNER, "positionEpoch":first["positionEpoch"],
        "intent":{"type":"insertText","anchor":0,"head":0,"text":"x"}
    }).to_string()));
    reset_full_pass_counts_for_test();
    let rendered =
        super::render::editor_v2_render_native(editor_id.clone(), OWNER.into(), None, None);
    let counts = take_full_pass_counts_for_test();
    assert_eq!(
        counts.table_command_availability_plans, 0,
        "mirror-less render reuses authoritative availability"
    );
    assert_eq!(
        counts.active_applicability_passes, 0,
        "mirror-less render reuses authoritative active state"
    );
    assert!(super::editor::editor_v2_destroy(editor_id).error.is_none());
    let rendered = ffi_value(&rendered);
    assert!(rendered["renderPatch"].is_object());
    assert_eq!(
        counts.yrs_tree_walks, 0,
        "the edited epoch chunks use indexed branches: {counts:?}"
    );
}
