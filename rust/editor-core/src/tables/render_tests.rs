use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::boundary::ResourceLimits;
use crate::render::incremental::render_blocks;
use crate::render::incremental::CachedRenderBlocks;
use crate::render::RenderElement;
use crate::tables::tests::{tabled_schema, PROSEMIRROR_TABLE_NAMES};

const REKEYED_CELLS_PER_TABLE_TRANSITION: usize = 2;
const CHANGED_CELLS_PER_TRANSITION: usize = 1;

fn cell(text: &str) -> serde_json::Value {
    json!({ "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
}

#[test]
fn attribute_pool_collision_keeps_canonical_key_and_exact_json() {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let document = fixture("collision");
    let index = Arc::new(
        crate::tables::admission::TableProjectionIndex::derive_or_fallback(
            &document,
            &schema,
            &ResourceLimits::default(),
        ),
    );
    let table = document.root().child(0).unwrap();
    let mut original = crate::tables::render::TableRenderContext::new(
        Arc::clone(&index),
        &crate::schema::schema_fingerprint(&schema),
    );
    let expected =
        crate::tables::render::generate_table(table, &schema, 0, &mut original, false).unwrap();
    let json = original.attributes[&expected.structure.attrs_key].clone();
    let digest = format!("{:x}", Sha256::digest(json.as_bytes()));
    assert_eq!(expected.structure.attrs_key, digest);

    let mut context = crate::tables::render::TableRenderContext::new(
        index,
        &crate::schema::schema_fingerprint(&schema),
    );
    context
        .attributes
        .insert(digest, Arc::from("{\"different\":true}"));
    let record =
        crate::tables::render::generate_table(table, &schema, 0, &mut context, false).unwrap();

    assert_eq!(record.structure.attrs_key.len(), 64);
    assert!(record
        .structure
        .attrs_key
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert_eq!(
        context.attributes[&record.structure.attrs_key].as_ref(),
        json.as_ref()
    );
    assert_ne!(record.structure.attrs_key, expected.structure.attrs_key);
    assert_eq!(
        context.attributes[&expected.structure.attrs_key].as_ref(),
        "{\"different\":true}"
    );

    let identities = context.attribute_identity_count_for_test();
    let duplicate = fixture("collision");
    let reused = crate::tables::render::generate_table(
        duplicate.root().child(0).unwrap(),
        &schema,
        0,
        &mut context,
        false,
    )
    .unwrap();
    assert_eq!(reused.structure.attrs_key, record.structure.attrs_key);
    assert_eq!(
        context.attribute_identity_count_for_test(),
        identities,
        "equal attribute values must not retain another node identity for every cell"
    );
    assert_eq!(
        context.attributes[&reused.structure.attrs_key].as_ref(),
        json.as_ref()
    );
}

#[test]
fn raised_depth_table_transport_keeps_flat_table_records() {
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
        let (elements, records) = crate::viewer::lower_cached_tables_for_test(&cache);
        assert_eq!(records.len(), 110, "every admitted table is transported");
        assert_eq!(elements.len(), 1);
        assert!(matches!(
            &elements[0],
            crate::viewer::FfiViewerElement::Table { .. }
        ));
        for (index, table) in records.iter().enumerate() {
            assert_eq!(table.cells.len(), 1, "table {index}");
            if index + 1 < records.len() {
                assert!(
                    matches!(
                        table.cells[0].elements.as_slice(),
                        [crate::viewer::FfiViewerElement::Table { .. }]
                    ),
                    "nested tables remain shallow references at depth {index}"
                );
            } else {
                assert!(table.cells[0]
                    .elements
                    .iter()
                    .any(|element| matches!(element,
                    crate::viewer::FfiViewerElement::TextRun { text, .. } if text == "deep")));
            }
        }
        crate::boundary::drop_json_value_stack_safe(input);
    });
}

fn fixture(first: &str) -> crate::model::Document {
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    crate::serialize::from_prosemirror_json(&json!({ "type": "doc", "content": [{
        "type": "table", "content": [{ "type": "table_row", "content": [cell(first), cell("unchanged")] }]
    }] }), &schema, crate::serialize::UnknownTypeMode::Preserve).unwrap()
}

#[test]
fn admitted_projection_reuses_only_the_exact_root_schema_and_limits() {
    use crate::tables::admission::AdmittedTableProjection;
    use crate::transform::DocumentValidator;
    use crate::yrs_engine::observability::{
        reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
    };

    let document = fixture("admitted");
    let schema = tabled_schema(PROSEMIRROR_TABLE_NAMES);
    let limits = ResourceLimits::default();
    let proof = AdmittedTableProjection::admit(&document, &schema, &limits).unwrap();
    let admitted_index = proof
        .matching_index(
            &document,
            &crate::schema::schema_fingerprint(&schema),
            &limits,
        )
        .unwrap();
    assert!(Arc::ptr_eq(
        &admitted_index,
        &proof
            .matching_index(
                &document.clone(),
                &crate::schema::schema_fingerprint(&schema),
                &limits
            )
            .unwrap()
    ));
    let distinct_root = fixture("admitted");
    assert_eq!(document, distinct_root);
    assert!(
        proof
            .matching_index(
                &distinct_root,
                &crate::schema::schema_fingerprint(&schema),
                &limits
            )
            .is_none(),
        "equal content does not certify distinct root storage"
    );
    let changed_document = fixture("changed");
    assert!(proof
        .matching_index(
            &changed_document,
            &crate::schema::schema_fingerprint(&schema),
            &limits
        )
        .is_none());
    let mut changed_schema_json = crate::tables::tests::tabled_schema_json(PROSEMIRROR_TABLE_NAMES);
    changed_schema_json["nodes"][1]["htmlTag"] = json!("section");
    let changed_schema = crate::schema::Schema::from_json(&changed_schema_json).unwrap();
    assert_ne!(
        crate::schema::schema_fingerprint(&schema),
        crate::schema::schema_fingerprint(&changed_schema)
    );
    assert!(proof
        .matching_index(
            &document,
            &crate::schema::schema_fingerprint(&changed_schema),
            &limits
        )
        .is_none());
    let restricted_limits = ResourceLimits {
        max_table_grid_slots: 1,
        ..limits.clone()
    };
    assert!(proof
        .matching_index(
            &document,
            &crate::schema::schema_fingerprint(&schema),
            &restricted_limits
        )
        .is_none());

    for (label, candidate, candidate_schema, candidate_limits, expected_derivations) in [
        ("exact", &document, &schema, &limits, 0),
        ("distinct root", &distinct_root, &schema, &limits, 1),
        ("changed document", &changed_document, &schema, &limits, 1),
        ("changed schema", &document, &changed_schema, &limits, 1),
        ("restricted grid", &document, &schema, &restricted_limits, 1),
    ] {
        let report =
            DocumentValidator::validate_report(candidate, candidate_schema, candidate_limits)
                .unwrap();
        reset_full_pass_counts_for_test();
        let reused = CachedRenderBlocks::build_validated(
            candidate,
            candidate_schema,
            candidate_limits,
            &crate::schema::schema_fingerprint(candidate_schema),
            report.stats.node_count,
            report.stats.max_depth,
            Some(&proof),
            None,
        )
        .unwrap();
        let passes = take_full_pass_counts_for_test();
        assert_eq!(
            passes.table_projection_derivations, expected_derivations,
            "{label}: {passes:#?}"
        );
        assert_eq!(
            Arc::ptr_eq(&admitted_index, &reused.table_projection_index),
            expected_derivations == 0,
            "{label}"
        );
        let fresh =
            CachedRenderBlocks::build(candidate, candidate_schema, candidate_limits).unwrap();
        assert_eq!(
            reused.materialize(),
            fresh.materialize(),
            "{label}: complete render output"
        );
        assert_eq!(
            reused.table_projection_index, fresh.table_projection_index,
            "{label}: projection including failures"
        );
        assert_eq!(
            reused.history_snapshot_retained_bytes(),
            fresh.history_snapshot_retained_bytes(),
            "{label}: retained charge"
        );
    }
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
    let index = Arc::new(
        crate::tables::admission::TableProjectionIndex::derive_or_fallback(
            &document,
            &schema,
            &ResourceLimits::default(),
        ),
    );
    let mut context = crate::tables::render::TableRenderContext::new(
        index,
        &crate::schema::schema_fingerprint(&schema),
    );
    let table = document.root().child(0).unwrap();
    crate::tables::render::generate_table(table, &schema, 0, &mut context, false).unwrap();
    for row in table.content().unwrap().iter() {
        for cell in row.content().unwrap().iter() {
            assert!(context.has_attribute_identity_for_test(cell, true),
                "equal complex attributes must retain identity reuse to avoid repeated deep hashing");
        }
    }
    let distinct_attrs = 2;
    crate::yrs_engine::observability::reset_full_pass_counts_for_test();
    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let attributes: std::collections::BTreeMap<_, _> = cache
        .table_attributes
        .iter()
        .map(|(key, json)| (key, json.as_ref()))
        .collect();
    let json = serde_json::to_string(&attributes).unwrap();
    let (_, records) = crate::viewer::lower_cached_tables_for_test(&cache);
    assert!(records
        .iter()
        .flat_map(|table| &table.synthetic_regions)
        .all(|region| attributes.contains_key(&region.attrs_key)));
    assert_eq!(
        json.matches(&payload).count(),
        1,
        "one pool entry must serve all repeated defaults"
    );
    assert!(
        json.len() < payload.len() + 16_384,
        "wire growth must be unique bytes plus records"
    );
    let serializations =
        crate::yrs_engine::observability::take_full_pass_counts_for_test().attribute_serializations;
    assert_eq!(
        serializations, distinct_attrs,
        "each distinct attrs value, including the shared synthetic default, must be serialized exactly once"
    );
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
    crate::yrs_engine::observability::reset_full_pass_counts_for_test();
    let transition = cache
        .transition(&old, &new, &schema, &[0], &limits)
        .unwrap();
    let passes = crate::yrs_engine::observability::take_full_pass_counts_for_test();
    assert_eq!(
        passes.cell_content_keys, REKEYED_CELLS_PER_TABLE_TRANSITION,
        "every cell of the transitioned table is keyed: {passes:#?}"
    );
    assert_eq!(
        passes.cell_content_generations, CHANGED_CELLS_PER_TRANSITION,
        "only the changed cell generates content; the unchanged cell is reused and rebased: {passes:#?}"
    );
    let new_blocks = transition.cache.materialize();
    let RenderElement::Table {
        table: old_table, ..
    } = &old_blocks[0][0]
    else {
        panic!("table");
    };
    let RenderElement::Table {
        table: new_table, ..
    } = &new_blocks[0][0]
    else {
        panic!("table");
    };
    assert_eq!(
        old_table.cells[1].content_key,
        new_table.cells[1].content_key
    );
    assert_eq!(
        crate::tables::render::absolute_cell_starts(old_table, 0)[1] + 7,
        crate::tables::render::absolute_cell_starts(new_table, 0)[1]
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
    let RenderElement::Table {
        table: old_table, ..
    } = &old_blocks[0][0]
    else {
        panic!("old table");
    };
    let transition = cache
        .transition(&old, &new, &schema, &[0], &limits)
        .unwrap();
    let new_blocks = transition.cache.materialize();
    let RenderElement::Table {
        table: new_table, ..
    } = &new_blocks[0][0]
    else {
        panic!("new table");
    };
    assert_eq!(
        old_table.cells[0].content_key,
        new_table.cells[0].content_key
    );
    assert_eq!(
        crate::tables::render::absolute_cell_starts(old_table, 0)[0],
        crate::tables::render::absolute_cell_starts(new_table, 0)[0]
    );
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
    let RenderElement::Table { table, .. } = &blocks[0][0] else {
        panic!("table");
    };
    assert_eq!(
        table.structure.doc_size,
        document.root().child(0).unwrap().node_size()
    );
    assert_eq!(table.cells.len(), 2);
    assert!(
        crate::tables::render::absolute_cell_starts(table, 0)[1] + table.cells[1].doc_size
            < table.structure.doc_size - 2
    );
    let (nested_offset, nested) = table.cells[0]
        .elements
        .iter()
        .find_map(|element| match element {
            RenderElement::Table { table, doc_offset } => Some((*doc_offset, table)),
            _ => None,
        })
        .unwrap();
    assert!(nested.structure.read_only_descendants);
    assert!(nested_offset > 0);
    assert!(nested_offset + nested.structure.doc_size < table.cells[0].doc_size);
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
    let RenderElement::Table { table: safe, .. } = &blocks[0][0] else {
        panic!("safe table");
    };
    let RenderElement::Table {
        table: collision, ..
    } = &blocks[1][0]
    else {
        panic!("collision table");
    };
    assert_eq!(safe.structure.compatibility_diagnostic, None);
    assert_eq!(safe.cells.len(), 4);
    assert_eq!(
        crate::tables::render::absolute_cell_starts(safe, 0)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
    assert!(safe.cells.iter().all(|cell| cell.doc_size > 0));
    assert!(safe.cells[0].elements.iter().any(
        |element| matches!(element, RenderElement::Table { table, .. } if table.structure.read_only_descendants)
    ));
    assert_eq!(
        collision.structure.compatibility_diagnostic,
        Some(crate::tables::render::TableCompatibilityDiagnostic::OverlappingReferenceCells)
    );
    assert_eq!(collision.structure.failure, None);
    assert_eq!(
        collision.cells.len(),
        3,
        "fallback retains every authored source cell"
    );
    assert!(collision
        .structure
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
    assert_eq!(cache.rendered_text(&schema), "source survives");
    let blocks = cache.materialize();
    let RenderElement::Table { table, doc_offset } = &blocks[0][0] else {
        panic!("failed table");
    };
    assert_eq!(*doc_offset, 0);
    assert_eq!(
        table.structure.doc_size,
        document.root().child(0).unwrap().node_size()
    );
    assert_eq!(
        table.structure.failure,
        Some(crate::tables::render::TableRenderFailure::GridLimit)
    );
    assert_eq!(table.structure.compatibility_diagnostic, None);
    assert!(table.cells.is_empty());
    assert!(table.structure.source_rows.is_empty());
    assert!(table.structure.synthetic_regions.is_empty());
    let (elements, records) = crate::viewer::lower_cached_tables_for_test(&cache);
    assert_eq!(
        elements,
        vec![crate::viewer::FfiViewerElement::Table {
            table_id: "t0".into()
        }]
    );
    let record = &records[0];
    assert_eq!(record.table_pos, 0);
    assert_eq!(record.source_end, table.structure.doc_size);
    assert_eq!(
        record.failure,
        Some(crate::tables::render::TableRenderFailure::GridLimit)
    );
    assert_eq!(record.compatibility_diagnostic, None);
    assert!(record.cells.is_empty());
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
    let RenderElement::Table { table, .. } = &cache.materialize()[0][0] else {
        panic!("failed table");
    };
    assert_eq!(
        table.structure.doc_size,
        document.root().child(0).unwrap().node_size()
    );
    assert_eq!(
        table.structure.failure,
        Some(crate::tables::render::TableRenderFailure::InvalidAttributes)
    );
    assert_eq!(table.structure.compatibility_diagnostic, None);
    assert!(table.cells.is_empty());
    assert!(table.structure.source_rows.is_empty());
    assert!(table.structure.synthetic_regions.is_empty());
}

#[test]
fn unrepresentable_table_sources_keep_text_order_without_exporting_cells() {
    let schema = crate::tables::interchange_tests::schema_admitting_a_stray_table_child();
    let paragraph = |text: &str| json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] });
    let row = |cells: Vec<serde_json::Value>| json!({ "type": "table_row", "content": cells });
    let nested = json!({ "type": "table_cell", "content": [{ "type": "table", "content": [row(vec![cell("nested🙂")])] }] });
    for (children, expected) in [
        (vec![paragraph("before"), row(vec![cell("a")])], "before\na"),
        (vec![row(vec![cell("a")]), paragraph("between"), row(vec![cell("b")])], "a\nbetween\nb"),
        (vec![row(vec![cell("a"), paragraph("inside🙂"), cell("b")])], "a\ninside🙂\nb"),
        (vec![paragraph("before"), row(vec![nested])], "before\nnested🙂"),
    ] {
        let source = json!({ "type": "doc", "content": [{ "type": "table", "content": children }] });
        let document = crate::serialize::from_prosemirror_json(&source, &schema,
            crate::serialize::UnknownTypeMode::Error).unwrap();
        let canonical = crate::serialize::to_prosemirror_json(&document, &schema);
        let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
        assert_eq!(cache.rendered_text(&schema), expected, "source order: {source}");
        assert_eq!(crate::render::rendered_text(&document, &schema), expected);
        assert_eq!(crate::serialize::to_prosemirror_json(&document, &schema), canonical);
        let (_, records) = crate::viewer::lower_cached_tables_for_test(&cache);
        assert_eq!(records.len(), 1, "source-only descendants must not export orphan nested tables");
        assert_eq!(records[0].failure, Some(crate::tables::render::TableRenderFailure::InvalidStructure));
        assert!(records[0].cells.is_empty(), "an unreadable grid must not invent native cells");
        assert_eq!(records[0].source_end, document.root().child(0).unwrap().node_size());
    }
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
    let RenderElement::Table { table, .. } = &transition.cache.materialize()[0][0] else {
        panic!("failed table");
    };
    assert_eq!(
        table.structure.failure,
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

mod position_free;

mod streamed_keys;
