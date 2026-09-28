use super::*;
use crate::boundary::{drop_json_value_stack_safe, serialize_json_value_stack_safe};
use crate::serialize::json_out::node_to_json;

#[test]
fn content_keys_hash_the_concatenated_child_json() {
    let schema = crate::schema::presets::prosemirror_table_schema();
    let mut inputs = super::position_free::fixtures();
    let original = inputs.last().unwrap().1.clone();
    for (kind, change) in [
        ("text", json!({"type":"text","text":"changed"})),
        (
            "mark",
            json!({"type":"text","text":"R0000C0000XY","marks":[{"type":"bold"}]}),
        ),
    ] {
        let mut input = original.clone();
        input["content"][1]["content"][0]["content"][0]["content"][0]["content"][0] = change;
        inputs.push((kind, input));
    }
    for (name, href) in [
        ("attr-base", "https://example.com/old"),
        ("attr", "https://example.com/new"),
    ] {
        let mut input = original.clone();
        input["content"][1]["content"][0]["content"][0]["content"][0]["content"][0]["marks"] =
            json!([{"type":"link", "attrs":{"href":href}}]);
        inputs.push((name, input));
    }
    let mut first_keys = std::collections::HashSet::new();
    for (name, input) in inputs {
        let document = crate::serialize::from_prosemirror_json(
            &input,
            &schema,
            crate::serialize::UnknownTypeMode::Preserve,
        )
        .unwrap();
        let cache =
            CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
        let mut records = Vec::new();
        cache.visit_table_records(&mut records);
        let mut nodes = std::collections::BTreeMap::new();
        let mut pending = vec![(document.root(), 0)];
        while let Some((node, pos)) = pending.pop() {
            if matches!(node.node_type(), "table_cell" | "table_header") {
                nodes.insert(pos, node);
            }
            let mut child_pos = pos + u32::from(node.node_type() != "doc");
            if let Some(content) = node.content() {
                for child in content.iter() {
                    pending.push((child, child_pos));
                    child_pos += child.node_size();
                }
            }
        }
        for (table_pos, table) in &records {
            for (pos, cell) in crate::tables::render::absolute_cell_starts(table, *table_pos)
                .into_iter()
                .zip(&table.cells)
            {
                let node = nodes[&pos];
                let mut hash = Sha256::new();
                hash.update(crate::schema::schema_fingerprint(&schema).as_bytes());
                for child in node.content().unwrap().iter() {
                    let value = node_to_json(child, &schema);
                    let bytes = serialize_json_value_stack_safe(&value, 0);
                    let mut streamed = Vec::new();
                    crate::serialize::json_out::write_node_json(&mut streamed, child, &schema)
                        .unwrap();
                    assert_eq!(streamed, bytes, "{name}: cell {pos} streamed bytes");
                    hash.update(bytes);
                    drop_json_value_stack_safe(value);
                }
                assert_eq!(
                    cell.content_key,
                    format!("{:x}", hash.finalize()),
                    "{name}: cell {pos} canonical concatenated children"
                );
            }
        }
        if matches!(
            name,
            "multi-paragraph" | "text" | "mark" | "attr-base" | "attr"
        ) {
            assert!(
                first_keys.insert(records[0].1.cells[0].content_key.clone()),
                "{name}: changed child must change the key"
            );
        }
        let again =
            CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
        assert_eq!(
            cache.materialize(),
            again.materialize(),
            "{name}: keys are stable"
        );
    }
}

#[test]
fn attribute_keys_are_interned_once_per_distinct_value() {
    const ROWS: usize = 1000;
    const COLUMNS: usize = 20;
    let input = crate::test_support::large_table_fixture::plain_table_document(ROWS, COLUMNS);
    let mut distinct_values = std::collections::BTreeSet::new();
    let mut pending = vec![&input];
    while let Some(node) = pending.pop() {
        let kind = node["type"].as_str().unwrap();
        if matches!(kind, "table" | "table_row" | "table_cell" | "table_header") {
            let cell = matches!(kind, "table_cell" | "table_header");
            let attrs: serde_json::Map<_, _> = node["attrs"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(key, _)| {
                    !cell || !matches!(key.as_str(), "colspan" | "rowspan" | "colwidth")
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            distinct_values.insert(serde_json::Value::Object(attrs).to_string());
        }
        if let Some(children) = node["content"].as_array() {
            pending.extend(children);
        }
    }
    let schema = crate::schema::presets::prosemirror_table_schema();
    let document = crate::serialize::from_prosemirror_json(
        &input,
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    crate::yrs_engine::observability::reset_full_pass_counts_for_test();
    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let counts = crate::yrs_engine::observability::take_full_pass_counts_for_test();
    assert_eq!(
        cache.table_attributes.len(),
        distinct_values.len(),
        "fixture has only empty filtered attrs"
    );
    assert_eq!(
        counts.attribute_serializations,
        distinct_values.len(),
        "serialize once per canonical value: {counts:#?}"
    );
}

#[test]
fn the_attribute_pool_holds_exactly_the_referenced_keys() {
    let mut config = crate::tables::tests::tabled_schema_json(PROSEMIRROR_TABLE_NAMES);
    for node in config["nodes"].as_array_mut().unwrap() {
        if node["tableRole"] == "cell" {
            node["attrs"]["background"] = json!({"default":null});
        }
    }
    let schema = crate::schema::Schema::from_json(&config).unwrap();
    let attributed_fixture = |text: &str, direction: &str| {
        let document = fixture(text);
        let mut input = crate::serialize::to_prosemirror_json(&document, &schema);
        input["content"][0]["content"][0]["content"][0]["attrs"] = json!({"background":direction});
        crate::serialize::from_prosemirror_json(
            &input,
            &schema,
            crate::serialize::UnknownTypeMode::Preserve,
        )
        .unwrap()
    };
    let old = attributed_fixture("old", "red");
    let cache = CachedRenderBlocks::build(&old, &schema, &ResourceLimits::default()).unwrap();
    let next = attributed_fixture("new", "blue");
    let transition = cache
        .transition(&old, &next, &schema, &[0], &ResourceLimits::default())
        .unwrap();
    assert_ne!(
        cache.table_attributes, transition.cache.table_attributes,
        "changed attributes replace pool entries"
    );
    let fresh = CachedRenderBlocks::build(&next, &schema, &ResourceLimits::default()).unwrap();
    assert_eq!(transition.cache.table_attributes, fresh.table_attributes);
    for cache in [&cache, &transition.cache] {
        let mut records = Vec::new();
        cache.visit_table_records(&mut records);
        let mut referenced = std::collections::BTreeSet::new();
        for (_, table) in records {
            referenced.insert(table.structure.attrs_key.clone());
            referenced.extend(
                table
                    .structure
                    .source_rows
                    .iter()
                    .map(|row| row.attrs_key.clone()),
            );
            referenced.extend(
                table
                    .structure
                    .synthetic_regions
                    .iter()
                    .map(|region| region.attrs_key.clone()),
            );
            referenced.extend(table.cells.iter().map(|cell| cell.attrs_key.clone()));
        }
        assert_eq!(referenced, cache.table_attributes.keys().cloned().collect());
    }
}

#[test]
fn write_node_json_is_stack_safe_for_deep_content() {
    const DEPTH: usize = 10_000;
    let schema = crate::schema::presets::prosemirror_table_schema();
    let mut node = crate::model::Node::text("deep 🦀".into(), Vec::new());
    for _ in 0..DEPTH {
        node = crate::model::Node::element(
            "blockquote".into(),
            Default::default(),
            crate::model::Fragment::from(vec![node]),
        );
    }
    let value = node_to_json(&node, &schema);
    let expected = serialize_json_value_stack_safe(&value, 0);
    let mut actual = Vec::new();
    crate::serialize::json_out::write_node_json(&mut actual, &node, &schema).unwrap();
    assert_eq!(actual, expected);
    drop_json_value_stack_safe(value);
}

#[test]
fn streamed_node_metadata_is_stack_safe_and_propagates_sink_failures() {
    const DEPTH: usize = 10_000;
    let schema = crate::schema::presets::prosemirror_table_schema();
    let mut value = json!({"quote":"\"\n🦀"});
    for _ in 0..DEPTH {
        value = serde_json::Value::Array(vec![value]);
    }
    let node = crate::model::Node::element(
        "__opaque_json".into(),
        std::collections::HashMap::from([("original_json".into(), value)]),
        crate::model::Fragment::from(Vec::new()),
    );
    let expected = serialize_json_value_stack_safe(&node.attrs()["original_json"], 0);
    let mut actual = Vec::new();
    crate::serialize::json_out::write_node_json(&mut actual, &node, &schema).unwrap();
    assert_eq!(actual, expected);
    struct FailedSink;
    impl std::io::Write for FailedSink {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let error =
        crate::serialize::json_out::write_node_json(&mut FailedSink, &node, &schema).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
}
