use serde_json::json;

use crate::boundary::ResourceLimits;
use crate::render::incremental::render_blocks;
use crate::render::incremental::CachedRenderBlocks;
use crate::render::RenderElement;
use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};

fn cell(text: &str) -> serde_json::Value {
    json!({ "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
}

#[test]
fn raised_depth_table_transport_has_bounded_json_container_depth() {
    crate::boundary::with_document_stack(|| {
        let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
        let limits = ResourceLimits {
            max_document_depth: 1024,
            ..ResourceLimits::default()
        };
        let mut child = json!({"type": "paragraph", "content": [{"type": "text", "text": "deep"}]});
        for _ in 0..110 {
            child = json!({"type": "table", "content": [{"type": "table_row", "content": [{"type": "table_cell", "content": [child]}]}]});
        }
        let input = json!({"type": "doc", "content": [child]});
        let document = crate::serialize::from_prosemirror_json_with_limits(
            &input,
            &schema,
            crate::serialize::UnknownTypeMode::Preserve,
            &limits,
        )
        .unwrap();
        let cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();
        let wire = crate::ffi_v2::render::serialize_render_cache_for_test(&cache);
        let mut depth = 0usize;
        let mut maximum = 0usize;
        let mut quoted = false;
        let mut escaped = false;
        for byte in wire.bytes() {
            if escaped {
                escaped = false;
                continue;
            }
            if quoted && byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                quoted = !quoted;
                continue;
            }
            if !quoted {
                match byte {
                    b'{' | b'[' => {
                        depth += 1;
                        maximum = maximum.max(depth);
                    }
                    b'}' | b']' => depth -= 1,
                    _ => {}
                }
            }
        }
        crate::boundary::drop_json_value_stack_safe(input);
        assert!(
            maximum < 64,
            "admitted 110-table/333-node-depth fixture has wire JSON depth {maximum}"
        );
    });
}

fn fixture(first: &str) -> crate::model::Document {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    crate::serialize::from_prosemirror_json(&json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [cell(first), cell("unchanged")] }]
    }] }), &schema, crate::serialize::UnknownTypeMode::Preserve).unwrap()
}

fn shared_default_fixture() -> (crate::model::Document, crate::schema::Schema, String) {
    let payload = "shared-attribute-payload".repeat(4096);
    let mut config = crate::tables::tests::tabled_schema_json(PROSEMIRROR_TABLE_NAMES);
    for node in config["nodes"].as_array_mut().unwrap() {
        if node["tableRole"] == "cell" {
            node["attrs"]["opaque"] =
                json!({ "default": { "large": payload, "nested": [true, null, 7] } });
        }
    }
    let schema = crate::schema::Schema::from_json(&config).unwrap();
    let mut wide = cell("wide");
    wide["attrs"] = json!({ "colspan": 12 });
    let document = crate::serialize::from_prosemirror_json(
        &json!({ "type": "doc", "content": [{
        "type": "table", "content": [
            { "type": "table_row", "content": [wide] },
            { "type": "table_row", "content": [cell("narrow")] }
        ]
    }] }),
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    (document, schema, payload)
}

#[test]
fn shared_synthetic_attributes_are_retained_and_serialized_once() {
    let (document, schema, payload) = shared_default_fixture();
    crate::tables::render::ATTRIBUTE_SERIALIZED_BYTES.set(0);
    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let json = crate::ffi_v2::render::serialize_render_cache_for_test(&cache);
    assert_eq!(
        json.matches(&payload).count(),
        1,
        "one pool entry must serve all repeated defaults"
    );
    assert!(
        json.len() < payload.len() + 16_384,
        "wire growth must be unique bytes plus records"
    );
    assert!(crate::tables::render::ATTRIBUTE_SERIALIZED_BYTES.get() < 4 * payload.len());
    assert!(
        cache
            .table_attributes
            .values()
            .map(|value| value.len())
            .sum::<usize>()
            < payload.len() + 256
    );
}

#[test]
fn many_gap_rows_construct_one_default_and_preserve_effective_attrs() {
    let (document, schema, _) = shared_default_fixture();
    let mut json = crate::serialize::to_prosemirror_json(&document, &schema);
    let rows = json["content"][0]["content"].as_array_mut().unwrap();
    let narrow = rows[1].clone();
    rows.extend(std::iter::repeat_n(narrow, 20));
    let document = crate::serialize::from_prosemirror_json(
        &json,
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    crate::tables::reference_grid::DEFAULT_CELL_CONSTRUCTIONS.set(0);
    let index = crate::tables::admission::TableProjectionIndex::derive_or_fallback(
        &document,
        &schema,
        &ResourceLimits::default(),
    );
    let projected = index.table_at(0).unwrap();
    assert_eq!(projected.synthetic.len(), 231);
    assert_eq!(
        crate::tables::reference_grid::DEFAULT_CELL_CONSTRUCTIONS.get(),
        1
    );
    for region in &projected.synthetic {
        let effective = region.effective_node();
        assert_eq!(effective.attrs()["opaque"], region.node.attrs()["opaque"]);
        assert_eq!(effective.attrs()["colspan"], region.rect.colspan);
        assert_eq!(effective.attrs()["rowspan"], region.rect.rowspan);
        assert_eq!(effective.attrs()["colwidth"], serde_json::Value::Null);
        assert_eq!(effective.node_type(), region.node.node_type());
    }
}

#[test]
fn synthetic_projection_retains_shared_defaults_without_materializing_each_gap() {
    let (document, schema, _) = shared_default_fixture();
    let index = crate::tables::admission::TableProjectionIndex::derive_or_fallback(
        &document,
        &schema,
        &ResourceLimits::default(),
    );
    let table = index.table_at(0).unwrap();
    assert_eq!(table.synthetic.len(), 11);
    assert!(
        table
            .synthetic
            .windows(2)
            .all(|regions| regions[0].node.shares_storage_with(&regions[1].node)),
        "synthetic regions must share their default payload storage"
    );
}

#[test]
fn changing_one_cell_reuses_unchanged_content_and_rebases_source_positions() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let limits = ResourceLimits::default();
    let old = fixture("one");
    let new = fixture("one longer");
    let cache = CachedRenderBlocks::build(&old, &schema, &limits).unwrap();
    let old_blocks = cache.materialize();
    crate::tables::render::CELL_CONTENT_GENERATIONS.set(0);
    let transition = cache
        .transition(&old, &new, &schema, &[0], &limits)
        .unwrap();
    assert_eq!(crate::tables::render::CELL_CONTENT_GENERATIONS.get(), 1);
    let new_blocks = transition.cache.materialize();
    let RenderElement::Table { table: old_table } = &old_blocks[0][0] else {
        panic!("table");
    };
    let RenderElement::Table { table: new_table } = &new_blocks[0][0] else {
        panic!("table");
    };
    assert_eq!(
        old_table.cells[1].content_key,
        new_table.cells[1].content_key
    );
    assert_eq!(
        old_table.cells[1].source_pos + 7,
        new_table.cells[1].source_pos
    );
    assert_eq!(new_blocks, render_blocks(&new, &schema));
    assert_eq!(
        transition.cache.rendered_text(&schema),
        "one longer\nunchanged"
    );
}

#[test]
fn unchanged_cell_at_the_same_position_reuses_its_render_arc() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let limits = ResourceLimits::default();
    let document = |right: &str| {
        crate::serialize::from_prosemirror_json(&json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [cell("left"), cell(right)] }]
    }] }), &schema, crate::serialize::UnknownTypeMode::Preserve).unwrap()
    };
    let old = document("before");
    let new = document("after");
    let cache = CachedRenderBlocks::build(&old, &schema, &limits).unwrap();
    let old_blocks = cache.materialize();
    let RenderElement::Table { table: old_table } = &old_blocks[0][0] else {
        panic!("old table");
    };
    let transition = cache
        .transition(&old, &new, &schema, &[0], &limits)
        .unwrap();
    let new_blocks = transition.cache.materialize();
    let RenderElement::Table { table: new_table } = &new_blocks[0][0] else {
        panic!("new table");
    };
    assert_eq!(
        old_table.cells[0].content_key,
        new_table.cells[0].content_key
    );
    assert_eq!(old_table.cells[0].source_pos, new_table.cells[0].source_pos);
    assert!(std::sync::Arc::ptr_eq(
        &old_table.cells[0].elements,
        &new_table.cells[0].elements
    ));
    assert_ne!(
        old_table.cells[1].content_key,
        new_table.cells[1].content_key
    );
    assert_eq!(new_blocks, render_blocks(&new, &schema));
}

#[test]
fn source_anchoring_and_nested_records_survive_rich_cell_content() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let mut outer = cell("outer");
    outer["content"].as_array_mut().unwrap().push(json!({
        "type": "table", "content": [{ "type": "table_row", "content": [cell("nested")] }]
    }));
    let json = json!({ "type": "doc", "content": [{ "type": "table", "content": [
        { "type": "table_row", "content": [outer, cell("last")] },
        { "type": "table_row", "content": [] }
    ] }] });
    let document = crate::serialize::from_prosemirror_json(
        &json,
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    let before = crate::serialize::to_prosemirror_json(&document, &schema);
    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let blocks = cache.materialize();
    let RenderElement::Table { table } = &blocks[0][0] else {
        panic!("table");
    };
    assert_eq!(
        table.source_end,
        document.root().child(0).unwrap().node_size()
    );
    assert_eq!(table.cells.len(), 2);
    assert!(table.cells[1].source_end < table.source_end - 2);
    let nested = table.cells[0]
        .elements
        .iter()
        .find_map(|element| match element {
            RenderElement::Table { table } => Some(table),
            _ => None,
        })
        .unwrap();
    assert!(nested.read_only_descendants);
    assert!(nested.table_pos > table.cells[0].source_pos);
    assert!(nested.source_end < table.cells[0].source_end);
    assert_eq!(cache.rendered_text(&schema), "outer\nnested\nlast");
    assert_eq!(
        crate::serialize::to_prosemirror_json(&document, &schema),
        before
    );
}

#[test]
fn combined_fixture_keeps_real_cells_bijective_and_marks_only_overlap_fallbacks() {
    let mut schema_json = crate::tables::tests::tabled_schema_json(PROSEMIRROR_TABLE_NAMES);
    schema_json["marks"] = json!([{ "name": "strong" }]);
    schema_json["nodes"].as_array_mut().unwrap().push(json!({
        "name": "hard_break", "content": "", "group": "inline", "role": "hardBreak", "isVoid": true
    }));
    let schema = crate::schema::Schema::from_json(&schema_json).unwrap();
    let rich_cell = json!({
        "type": "table_header", "attrs": { "colspan": 2 }, "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "marked", "marks": [{ "type": "strong" }] },
                { "type": "hard_break" },
                { "type": "text", "text": "atom" }
            ] },
            { "type": "paragraph", "content": [{ "type": "text", "text": "second block" }] },
            { "type": "table", "content": [{ "type": "table_row", "content": [cell("nested")] }] }
        ]
    });
    let document = crate::serialize::from_prosemirror_json(&json!({ "type": "doc", "content": [
        { "type": "table", "content": [
            { "type": "table_row", "content": [rich_cell, cell("safe")] },
            { "type": "table_row", "content": [cell("tail"), cell("end")] }
        ] },
        { "type": "table", "content": [
            { "type": "table_row", "content": [
                cell("collision base"),
                { "type": "table_cell", "attrs": { "rowspan": 2, "colspan": 1 }, "content": [{ "type": "paragraph" }] }
            ] },
            { "type": "table_row", "content": [
                { "type": "table_cell", "attrs": { "rowspan": 3, "colspan": 2 }, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "overlaps" }] }] }
            ] },
            { "type": "table_row", "content": [] }
        ] }
    ] }), &schema, crate::serialize::UnknownTypeMode::Preserve).unwrap();

    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let blocks = cache.materialize();
    let RenderElement::Table { table: safe } = &blocks[0][0] else {
        panic!("safe table");
    };
    let RenderElement::Table { table: collision } = &blocks[1][0] else {
        panic!("collision table");
    };
    assert_eq!(safe.compatibility_diagnostic, None);
    assert_eq!(safe.cells.len(), 4);
    assert_eq!(
        safe.cells
            .iter()
            .map(|cell| cell.source_pos)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
    assert!(safe
        .cells
        .iter()
        .all(|cell| cell.source_pos < cell.source_end));
    assert!(safe.cells[0].elements.iter().any(
        |element| matches!(element, RenderElement::Table { table } if table.read_only_descendants)
    ));
    assert_eq!(
        collision.compatibility_diagnostic,
        Some(crate::tables::render::TableCompatibilityDiagnostic::OverlappingReferenceCells)
    );
    assert_eq!(collision.failure, None);
    assert_eq!(
        collision.cells.len(),
        3,
        "fallback retains every authored source cell"
    );
    assert!(collision
        .synthetic_regions
        .iter()
        .all(|region| region.rowspan > 0 && region.colspan > 0));
}

#[test]
fn grid_limit_failure_keeps_the_real_table_extent_without_inventing_cells() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let document = crate::serialize::from_prosemirror_json(&json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [{
            "type": "table_cell", "attrs": { "colspan": 2 }, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "source survives" }] }]
        }] }]
    }] }), &schema, crate::serialize::UnknownTypeMode::Preserve).unwrap();
    let limits = ResourceLimits {
        max_table_grid_slots: 1,
        ..ResourceLimits::default()
    };
    let cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();
    let blocks = cache.materialize();
    let RenderElement::Table { table } = &blocks[0][0] else {
        panic!("failed table");
    };
    assert_eq!(table.table_pos, 0);
    assert_eq!(
        table.source_end,
        document.root().child(0).unwrap().node_size()
    );
    assert_eq!(
        table.failure,
        Some(crate::tables::render::TableRenderFailure::GridLimit)
    );
    assert_eq!(table.compatibility_diagnostic, None);
    assert!(table.cells.is_empty());
    assert!(table.source_rows.is_empty());
    assert!(table.synthetic_regions.is_empty());
    let wire: serde_json::Value = serde_json::from_str(
        &crate::ffi_v2::render::serialize_render_cache_for_test(&cache),
    )
    .unwrap();
    let id = wire["renderBlocks"][0][0]["tableId"].as_str().unwrap();
    assert_eq!(wire["tableRecords"][id]["tablePos"], json!(0));
    assert_eq!(
        wire["tableRecords"][id]["sourceEnd"],
        json!(table.source_end)
    );
    assert_eq!(wire["tableRecords"][id]["failure"], json!("gridLimit"));
    assert_eq!(
        wire["tableRecords"][id]["compatibilityDiagnostic"],
        serde_json::Value::Null
    );
    assert_eq!(wire["tableRecords"][id]["cells"], json!([]));
}

#[test]
fn structural_failure_keeps_the_real_table_extent_without_inventing_cells() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let document = crate::serialize::from_prosemirror_json(
        &json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [{
            "type": "table_cell", "attrs": { "colspan": 0 }, "content": [{ "type": "paragraph" }]
        }] }]
    }] }),
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let RenderElement::Table { table } = &cache.materialize()[0][0] else {
        panic!("failed table");
    };
    assert_eq!(
        table.source_end,
        document.root().child(0).unwrap().node_size()
    );
    assert_eq!(
        table.failure,
        Some(crate::tables::render::TableRenderFailure::InvalidAttributes)
    );
    assert_eq!(table.compatibility_diagnostic, None);
    assert!(table.cells.is_empty());
    assert!(table.source_rows.is_empty());
    assert!(table.synthetic_regions.is_empty());
}

#[test]
fn transition_rebuilds_table_metadata_when_projection_limits_change() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let document = crate::serialize::from_prosemirror_json(
        &json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [{
            "type": "table_cell", "attrs": { "colspan": 2 }, "content": [{ "type": "paragraph" }]
        }] }]
    }] }),
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    let permissive = ResourceLimits::default();
    let constrained = ResourceLimits {
        max_table_grid_slots: 1,
        ..ResourceLimits::default()
    };
    let cache = CachedRenderBlocks::build(&document, &schema, &permissive).unwrap();
    let transition = cache
        .transition(&document, &document, &schema, &[], &constrained)
        .unwrap();
    let RenderElement::Table { table } = &transition.cache.materialize()[0][0] else {
        panic!("failed table");
    };
    assert_eq!(
        table.failure,
        Some(crate::tables::render::TableRenderFailure::GridLimit)
    );
    assert!(table.cells.is_empty());
}

#[test]
fn table_is_one_outer_semantic_element() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let document = crate::serialize::from_prosemirror_json(
        &json!({
            "type": "doc",
            "content": [{
                "type": "table",
                "content": [{
                    "type": "table_row",
                    "content": [{
                        "type": "table_header",
                        "attrs": { "colspan": 2, "rowspan": 1 },
                        "content": [{
                            "type": "paragraph",
                            "content": [{ "type": "text", "text": "Header" }]
                        }]
                    }]
                }]
            }]
        }),
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    let blocks = render_blocks(&document, &schema);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].len(),
        1,
        "table descendants belong inside its semantic record"
    );
}
