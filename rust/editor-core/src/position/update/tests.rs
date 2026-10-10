use super::*;
use crate::model::{fragment::Fragment, node::Node};
use crate::schema::presets::tiptap_schema;
use std::collections::HashMap;

fn element(kind: &str, children: Vec<Node>) -> Node {
    Node::element(kind.to_owned(), HashMap::new(), Fragment::from(children))
}

fn document(texts: &[&str], depth: usize, list: bool) -> Document {
    let mut children = texts
        .iter()
        .map(|text| {
            let paragraph = element(
                "paragraph",
                if text.is_empty() {
                    vec![]
                } else {
                    vec![Node::text((*text).to_owned(), vec![])]
                },
            );
            if list {
                element("listItem", vec![paragraph])
            } else {
                paragraph
            }
        })
        .collect::<Vec<_>>();
    if list {
        children = vec![element("orderedList", children)];
    }
    for _ in 0..depth {
        children = vec![element("blockquote", children)];
    }
    Document::new(element("doc", children))
}

fn compare(
    source: &PositionMap,
    old: &Document,
    new: &Document,
    step: &StepMap,
    mode: UpdateMode,
    schema: &Schema,
) -> PositionMap {
    let before = format!("{source:?}");
    let charge = source.history_snapshot_clone_retained_bytes();
    let mut expected = source.clone();
    expected.blocks = super::super::Blocks::Dense(
        source
            .blocks
            .iter()
            .map(|block| block.into_owned())
            .collect(),
    );
    expected.update(step, old, new, mode, schema);
    expected.compact();
    let actual = source.clone_updated_and_compacted(step, old, new, mode, schema);
    assert_eq!(
        format!("{actual:?}"),
        format!("{expected:?}"),
        "mode={mode:?}, step={step:?}"
    );
    assert_eq!(actual.blocks.capacity(), expected.blocks.capacity());
    assert_eq!(
        actual.prefix_deltas.history_snapshot_clone_retained_bytes(),
        expected
            .prefix_deltas
            .history_snapshot_clone_retained_bytes()
    );
    assert_eq!(
        actual.history_snapshot_clone_retained_bytes(),
        expected.history_snapshot_clone_retained_bytes()
    );
    for (a, b) in actual.blocks.iter().zip(expected.blocks.iter()) {
        assert_eq!(a.node_path.capacity(), b.node_path.capacity());
        assert_eq!(a.node_path.spilled(), b.node_path.spilled());
    }
    for pos in 0..=new.content_size() {
        assert_eq!(
            actual.doc_to_scalar(pos, new),
            expected.doc_to_scalar(pos, new),
            "doc pos={pos}"
        );
    }
    for scalar in 0..=expected.total_scalars() {
        assert_eq!(
            actual.scalar_to_doc(scalar, new),
            expected.scalar_to_doc(scalar, new),
            "scalar={scalar}"
        );
    }
    assert_eq!(format!("{source:?}"), before, "source must stay immutable");
    assert_eq!(source.history_snapshot_clone_retained_bytes(), charge);
    actual
}

#[test]
fn compacted_clone_matches_legacy_text_edits_and_retained_storage() {
    const DEEP_PATH: usize = 12;
    const SPARE_PATH: usize = 32;
    let schema = tiptap_schema();
    for depth in [0, DEEP_PATH] {
        for list in [false, true] {
            for force_spill in [false, true] {
                for original in ["", "abc", "é🔥a"] {
                    let texts = [original; 3];
                    let old = document(&texts, depth, list);
                    let mut source = PositionMap::build(&old, &schema);
                    source.blocks.dense_mut().reserve(SPARE_PATH);
                    if force_spill {
                        for block in source.blocks.dense_mut() {
                            block.node_path.reserve(SPARE_PATH);
                        }
                    }
                    for index in 0..texts.len() {
                        for replacement in ["", "Z", "é🔥a", "longer"] {
                            let mut changed = texts;
                            changed[index] = replacement;
                            let new = document(&changed, depth, list);
                            let step = StepMap::from_replace(
                                source.blocks.get(index).unwrap().doc_start,
                                original.chars().count() as u32,
                                replacement.chars().count() as u32,
                            );
                            let result = compare(
                                &source,
                                &old,
                                &new,
                                &step,
                                UpdateMode::InlineTextOnly,
                                &schema,
                            );
                            assert_eq!(
                                format!("{:?}", result.blocks),
                                format!("{:?}", PositionMap::build(&new, &schema).blocks)
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn compacted_clone_preserves_pending_deltas_and_structural_fallbacks() {
    let schema = tiptap_schema();
    let old = document(&["abc", "def", "ghi"], 0, false);
    let source = PositionMap::build(&old, &schema);
    let changed = document(&["abcd", "def", "ghi"], 0, false);
    let insert = StepMap::from_insert(source.blocks.get(0).unwrap().doc_end, 1);
    let mut pending = source.clone();
    pending.update(&insert, &old, &changed, UpdateMode::InlineTextOnly, &schema);
    assert!(!pending.prefix_deltas.is_empty());
    let next = document(&["abcd", "dé🔥", "ghi"], 0, false);
    let replacement = StepMap::from_replace(pending.effective_doc_start(1), 3, 3);
    compare(
        &pending,
        &changed,
        &next,
        &replacement,
        UpdateMode::InlineTextOnly,
        &schema,
    );
    for mode in [
        UpdateMode::MarksOnly,
        UpdateMode::Rebuild,
        UpdateMode::InlineTextOnly,
    ] {
        compare(
            &pending,
            &changed,
            &changed,
            &StepMap::empty(),
            mode,
            &schema,
        );
        for new in [
            document(&["abcd", "changed neighbor", "ghi"], 0, false),
            document(&["ab", "cd", "def", "ghi"], 0, false),
            document(&["abdef", "ghi"], 0, false),
            document(&[], 0, false),
        ] {
            compare(&source, &old, &new, &insert, mode, &schema);
        }
    }
    let mut wrapping = source.clone();
    for block in &mut wrapping.blocks.dense_mut()[1..] {
        block.doc_start = u32::MAX;
        block.doc_end = u32::MAX;
        block.scalar_start = u32::MAX;
    }
    compare(
        &wrapping,
        &old,
        &changed,
        &insert,
        UpdateMode::InlineTextOnly,
        &schema,
    );
}

#[test]
fn compacted_clone_handles_void_labels_and_multiple_ranges() {
    let schema = tiptap_schema();
    let paragraph = |label: &str| {
        element(
            "paragraph",
            vec![
                Node::text("a".to_owned(), vec![]),
                Node::void(
                    "mention".to_owned(),
                    HashMap::from([(
                        "label".to_owned(),
                        serde_json::Value::String(label.to_owned()),
                    )]),
                ),
                Node::void("hardBreak".to_owned(), HashMap::new()),
                Node::text("z".to_owned(), vec![]),
            ],
        )
    };
    let make = |label: &str| {
        Document::new(element(
            "doc",
            vec![
                paragraph(label),
                Node::void("horizontalRule".to_owned(), HashMap::new()),
                paragraph("last"),
            ],
        ))
    };
    let old = make("x");
    let new = make("long 🔥 label");
    let source = PositionMap::build(&old, &schema);
    let step = StepMap::from_replace(source.blocks.get(0).unwrap().doc_start + 1, 1, 1);
    let result = compare(
        &source,
        &old,
        &new,
        &step,
        UpdateMode::InlineTextOnly,
        &schema,
    );
    assert_ne!(
        result.blocks.get(0).unwrap().scalar_len,
        source.blocks.get(0).unwrap().scalar_len
    );
    assert_eq!(
        result.blocks.get(1).unwrap().doc_start,
        source.blocks.get(1).unwrap().doc_start
    );
    assert_ne!(
        result.blocks.get(1).unwrap().scalar_start,
        source.blocks.get(1).unwrap().scalar_start
    );
    let void_step = StepMap::from_replace(source.blocks.get(1).unwrap().doc_start, 0, 0);
    compare(
        &source,
        &old,
        &old,
        &void_step,
        UpdateMode::InlineTextOnly,
        &schema,
    );
    let multi = step.compose(&void_step);
    compare(
        &source,
        &old,
        &new,
        &multi,
        UpdateMode::InlineTextOnly,
        &schema,
    );
    let empty = document(&[], 0, false);
    compare(
        &PositionMap::build(&empty, &schema),
        &empty,
        &new,
        &step,
        UpdateMode::InlineTextOnly,
        &schema,
    );
}

#[test]
fn repeated_large_inline_edits_copy_bounded_block_storage() {
    const BLOCK_COUNT: usize = 4096;
    const MAX_BLOCK_COPIES: usize = 256;
    let schema = tiptap_schema();
    let mut texts = vec!["abc"; BLOCK_COUNT];
    let old = document(&texts, 0, false);
    let source = PositionMap::build(&old, &schema);
    texts[0] = "abcd";
    let first = document(&texts, 0, false);
    let first_map = source.clone_updated_and_compacted(
        &StepMap::from_insert(source.block(0).unwrap().doc_end, 1),
        &old,
        &first,
        UpdateMode::InlineTextOnly,
        &schema,
    );
    texts[0] = "abcde";
    let second = document(&texts, 0, false);
    super::super::BLOCK_MAPPING_CLONES.set(0);
    let second_map = first_map.clone_updated_and_compacted(
        &StepMap::from_insert(first_map.block(0).unwrap().doc_end, 1),
        &first,
        &second,
        UpdateMode::InlineTextOnly,
        &schema,
    );
    let copies = super::super::BLOCK_MAPPING_CLONES.get()
        + second_map.blocks.unshared_block_count(&first_map.blocks);
    assert!(copies <= MAX_BLOCK_COPIES,
        "second inline edit cloned {copies} of {BLOCK_COUNT} block mappings; budget={MAX_BLOCK_COPIES}");
    assert_eq!(
        second_map.total_scalars(),
        PositionMap::build(&second, &schema).total_scalars()
    );
    assert_eq!(
        first_map.total_scalars(),
        PositionMap::build(&first, &schema).total_scalars()
    );
}

#[test]
fn paged_inline_edits_match_dense_oracle_across_boundaries_and_fallbacks() {
    const BLOCK_COUNT: usize = 4097;
    const LAST: usize = BLOCK_COUNT - 1;
    let schema = tiptap_schema();
    let mut texts = vec!["abc"; BLOCK_COUNT];
    let mut old = document(&texts, 0, false);
    let mut source = PositionMap::build(&old, &schema);
    source.blocks.dense_mut().reserve(BLOCK_COUNT);
    let original = source.clone();
    let original_debug = format!("{original:?}");
    for round in 0..3 {
        for index in [0, 127, 128, 129, 255, 256, LAST - 1, LAST] {
            let previous = texts[index];
            texts[index] = if round % 2 == 0 { "é🔥abcd" } else { "" };
            let new = document(&texts, 0, false);
            let step = StepMap::from_replace(
                source.block(index).unwrap().doc_start,
                previous.chars().count() as u32,
                texts[index].chars().count() as u32,
            );
            source = compare(
                &source,
                &old,
                &new,
                &step,
                UpdateMode::InlineTextOnly,
                &schema,
            );
            assert!(matches!(source.blocks, super::super::Blocks::Paged { .. }));
            old = new;
        }
    }
    let index = 128;
    let step = StepMap::from_insert(source.block(index).unwrap().doc_end, 1);
    texts[index] = "é🔥abcdZ";
    let new = document(&texts, 0, false);
    let mut pending = source.clone();
    pending.update(&step, &old, &new, UpdateMode::InlineTextOnly, &schema);
    compare(
        &pending,
        &new,
        &new,
        &StepMap::empty(),
        UpdateMode::MarksOnly,
        &schema,
    );
    compare(
        &pending,
        &new,
        &new,
        &StepMap::empty(),
        UpdateMode::InlineTextOnly,
        &schema,
    );
    texts.insert(index, "split");
    let structural = document(&texts, 0, false);
    let rebuilt = compare(
        &source,
        &old,
        &structural,
        &step,
        UpdateMode::Rebuild,
        &schema,
    );
    assert!(matches!(rebuilt.blocks, super::super::Blocks::Dense(_)));
    assert_eq!(format!("{original:?}"), original_debug);
}
