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
        serde_json::from_str(&super::render::serialize_render_cache_for_test(&cache)).unwrap();
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
