use super::*;
use crate::test_support::large_table_fixture::{
    ffi_editor_with_document, multi_paragraph_cell_document, plain_table_document,
};

const FIXTURE_ROWS: usize = 2;
const FIXTURE_COLUMNS: usize = 2;
const OWNER_ID: &str = "71";
const MULTI_PARAGRAPH_CELL_INDEX: usize = 4;
const OPEN_TOKEN_SIZE: u32 = 1;

fn fixtures() -> Vec<(&'static str, serde_json::Value)> {
    let regular = plain_table_document(FIXTURE_ROWS, FIXTURE_COLUMNS);
    let mut merged = regular.clone();
    merged["content"][0]["content"][0]["content"][0]["attrs"] = json!({"colspan": 2});
    merged["content"][0]["content"][0]["content"]
        .as_array_mut()
        .unwrap()
        .pop();
    let mut empty_row = regular.clone();
    empty_row["content"][0]["content"]
        .as_array_mut()
        .unwrap()
        .insert(1, json!({"type": "table_row"}));
    let mut fixtures = vec![
        ("regular", regular),
        ("merged", merged),
        ("empty-row", empty_row),
        ("multi-paragraph", multi_paragraph_cell_document()),
    ];
    for (_, document) in &mut fixtures {
        document["content"].as_array_mut().unwrap().insert(
            0,
            json!({
                "type": "paragraph", "content": [{"type": "text", "text": "before table"}],
            }),
        );
    }
    fixtures
}

#[test]
fn cell_elements_are_positioned_relative_to_their_cell() {
    let schema = crate::schema::presets::prosemirror_table_schema();
    let document = crate::serialize::from_prosemirror_json(
        &multi_paragraph_cell_document(),
        &schema,
        crate::serialize::UnknownTypeMode::Preserve,
    )
    .unwrap();
    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let blocks = cache.materialize();
    let RenderElement::Table { table, .. } = &blocks[0][0] else {
        panic!("outer table")
    };
    let cell = &table.cells[MULTI_PARAGRAPH_CELL_INDEX];
    let atom_position = cell
        .elements
        .iter()
        .find_map(|element| match element {
            RenderElement::VoidInline { doc_pos, .. }
            | RenderElement::OpaqueInlineAtom { doc_pos, .. } => Some(*doc_pos),
            _ => None,
        })
        .expect("rich cell has an inline atom");
    let text = document
        .root()
        .child(0)
        .unwrap()
        .child(1)
        .unwrap()
        .child(1)
        .unwrap()
        .child(0)
        .unwrap()
        .child(0)
        .unwrap();
    assert_eq!(
        atom_position,
        OPEN_TOKEN_SIZE + OPEN_TOKEN_SIZE + text.node_size(),
        "atom position is relative to the cell opening token"
    );
}

#[test]
fn the_legacy_snapshot_is_unchanged_by_position_free_records() {
    for (name, document) in fixtures() {
        let _clients = crate::test_support::deterministic_clients::DeterministicClients::new();
        let editor_id = ffi_editor_with_document(&document);
        let rendered = crate::ffi_v2::render::editor_v2_render_native(
            editor_id.clone(),
            OWNER_ID.into(),
            None,
            None,
        );
        let destroyed = crate::ffi_v2::editor::editor_v2_destroy(editor_id);
        assert!(destroyed.error.is_none());
        let snapshot = rendered
            .value
            .unwrap_or_else(|| panic!("{name}: {:?}", rendered.error));
        let expected = match name {
            "regular" => include_str!("../../test_support/fixtures/table-render-regular.json"),
            "merged" => include_str!("../../test_support/fixtures/table-render-merged.json"),
            "empty-row" => include_str!("../../test_support/fixtures/table-render-empty-row.json"),
            "multi-paragraph" => {
                include_str!("../../test_support/fixtures/table-render-multi-paragraph.json")
            }
            _ => unreachable!("fixture has a captured baseline"),
        };
        assert_eq!(
            snapshot.as_bytes(),
            expected.as_bytes(),
            "legacy snapshot {name}"
        );
    }
}

#[test]
fn absolute_cell_starts_follow_the_source_row_formula() {
    use crate::tables::render::absolute_cell_starts;
    const FAILED_GRID_LIMIT: usize = 1;
    let schema = crate::schema::presets::prosemirror_table_schema();
    for (name, input) in fixtures() {
        let document = crate::serialize::from_prosemirror_json(
            &input,
            &schema,
            crate::serialize::UnknownTypeMode::Preserve,
        )
        .unwrap();
        for grid_limit in [
            ResourceLimits::default().max_table_grid_slots,
            FAILED_GRID_LIMIT,
        ] {
            let limits = ResourceLimits {
                max_table_grid_slots: grid_limit,
                ..ResourceLimits::default()
            };
            let cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();
            let mut source = std::collections::BTreeMap::new();
            let mut pending = vec![(document.root(), 0)];
            while let Some((node, pos)) = pending.pop() {
                if schema
                    .node(node.node_type())
                    .is_some_and(|spec| spec.table_role == Some(crate::tables::TableRole::Table))
                {
                    let mut cells = Vec::new();
                    let mut row_pos = pos + OPEN_TOKEN_SIZE;
                    for row in node.content().unwrap().iter() {
                        let mut cell_pos = row_pos + OPEN_TOKEN_SIZE;
                        for cell in row.content().unwrap().iter() {
                            cells.push(cell_pos);
                            cell_pos += cell.node_size();
                        }
                        row_pos += row.node_size();
                    }
                    source.insert(pos, (node.node_size(), cells));
                }
                let mut child_pos = if node.node_type() == "doc" {
                    pos
                } else {
                    pos + OPEN_TOKEN_SIZE
                };
                if let Some(content) = node.content() {
                    for child in content.iter() {
                        pending.push((child, child_pos));
                        child_pos += child.node_size();
                    }
                }
            }
            let mut records = Vec::new();
            cache.visit_table_records(&mut records);
            for (pos, table) in records {
                let (size, cells) = &source[&pos];
                assert_eq!(
                    table.structure.doc_size, *size,
                    "{name} size at {pos}, limit {grid_limit}"
                );
                let actual = absolute_cell_starts(table, pos);
                if table.structure.failure.is_some() {
                    assert!(
                        actual.is_empty(),
                        "failed table retains its size but has no cell anchors"
                    );
                } else {
                    assert_eq!(actual, *cells, "{name} source row positions at {pos}");
                    assert_eq!(
                        table.structure.doc_size,
                        2 * OPEN_TOKEN_SIZE * (1 + table.structure.source_rows.len() as u32)
                            + table.cells.iter().map(|cell| cell.doc_size).sum::<u32>()
                    );
                }
            }
        }
    }
}

#[test]
fn rich_cell_elements_survive_sibling_and_root_position_changes() {
    const TABLE_BLOCK: usize = 1;
    const ATOM_CELL: usize = 4;
    const NESTED_CELL: usize = 8;
    let schema = crate::schema::presets::prosemirror_table_schema();
    let limits = ResourceLimits::default();
    let (_, original) = fixtures().pop().unwrap();
    let parse = |input: &serde_json::Value| {
        crate::serialize::from_prosemirror_json(
            input,
            &schema,
            crate::serialize::UnknownTypeMode::Preserve,
        )
        .unwrap()
    };
    let old = parse(&original);
    let cache = CachedRenderBlocks::build(&old, &schema, &limits).unwrap();
    let old_blocks = cache.materialize();
    let RenderElement::Table {
        table: old_table, ..
    } = &old_blocks[TABLE_BLOCK][0]
    else {
        panic!("table after prose")
    };
    for changes_cell in [true, false] {
        let mut input = original.clone();
        if changes_cell {
            input["content"][TABLE_BLOCK]["content"][0]["content"][0]["content"][0]["content"][0]
                ["text"] = json!("the first cell grows substantially");
        } else {
            input["content"][0]["content"][0]["text"] =
                json!("the prose before the table grows substantially");
        }
        let next = parse(&input);
        let changed = if changes_cell { TABLE_BLOCK } else { 0 };
        let transition = cache
            .transition(&old, &next, &schema, &[changed], &limits)
            .unwrap();
        let blocks = transition.cache.materialize();
        let RenderElement::Table { table, .. } = &blocks[TABLE_BLOCK][0] else {
            panic!("shifted table")
        };
        for cell in [ATOM_CELL, NESTED_CELL] {
            assert!(
                Arc::ptr_eq(&old_table.cells[cell].elements, &table.cells[cell].elements),
                "cell {cell} keeps its relative elements, changed cell={changes_cell}"
            );
        }
        let fresh = CachedRenderBlocks::build(&next, &schema, &limits).unwrap();
        assert_eq!(
            crate::ffi_v2::render::serialize_render_cache_for_test(
                &transition.cache,
                &test_table_ids(&transition.cache)
            ),
            crate::ffi_v2::render::serialize_render_cache_for_test(&fresh, &test_table_ids(&fresh)),
            "absolute nested tables and atoms agree after changed cell={changes_cell}",
        );
    }
}
