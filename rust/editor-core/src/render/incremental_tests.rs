use std::collections::HashMap;
use std::sync::Arc;

use proptest::prelude::*;

use crate::boundary::ResourceLimits;
use crate::model::{Document, Fragment, Mark, Node};
use crate::render::incremental::{
    render_blocks, try_render_blocks, CachedRenderBlocks, CachedRenderTransitionUpdate,
};
use crate::render::RenderElement;
use crate::{prosemirror_schema, tiptap_schema};

fn text(value: &str) -> Node {
    Node::text(value.to_string(), vec![])
}

fn paragraph(children: Vec<Node>) -> Node {
    Node::element(
        "paragraph".to_string(),
        HashMap::new(),
        Fragment::from(children),
    )
}

fn doc(children: Vec<Node>) -> Document {
    Document::new(Node::element(
        "doc".to_string(),
        HashMap::new(),
        Fragment::from(children),
    ))
}

fn replace_top_level(document: &Document, index: usize, replacement: Node) -> Document {
    let mut children = (0..document.root().child_count())
        .map(|child_index| document.root().child(child_index).unwrap().clone())
        .collect::<Vec<_>>();
    children[index] = replacement;
    doc(children)
}

fn inline_atom(label: &str) -> Node {
    Node::void(
        "__opaque_json".to_string(),
        HashMap::from([
            (
                "opaque_placement".to_string(),
                serde_json::Value::String("inline".to_string()),
            ),
            (
                "label".to_string(),
                serde_json::Value::String(label.to_string()),
            ),
        ]),
    )
}

fn opaque_block_atom(label: &str) -> Node {
    Node::void(
        "__opaque_json".to_string(),
        HashMap::from([
            (
                "opaque_placement".to_string(),
                serde_json::Value::String("block".to_string()),
            ),
            (
                "label".to_string(),
                serde_json::Value::String(label.to_string()),
            ),
        ]),
    )
}

fn hard_break() -> Node {
    Node::void("hardBreak".to_string(), HashMap::new())
}

fn horizontal_rule() -> Node {
    Node::void("horizontalRule".to_string(), HashMap::new())
}

fn bullet_list(children: Vec<Node>) -> Node {
    Node::element(
        "bulletList".to_string(),
        HashMap::new(),
        Fragment::from(children),
    )
}

fn ordered_list(start: u32, children: Vec<Node>) -> Node {
    ordered_list_with_start(Some(serde_json::Value::Number(start.into())), children)
}

fn ordered_list_with_start(start: Option<serde_json::Value>, children: Vec<Node>) -> Node {
    let mut attrs = HashMap::new();
    if let Some(start) = start {
        attrs.insert("start".to_string(), start);
    }
    Node::element("orderedList".to_string(), attrs, Fragment::from(children))
}

fn list_item(children: Vec<Node>) -> Node {
    Node::element(
        "listItem".to_string(),
        HashMap::new(),
        Fragment::from(children),
    )
}

fn assert_update_reconstructs(
    old_render: Vec<Vec<RenderElement>>,
    transition: &super::CachedRenderTransition,
    expected: &[Vec<RenderElement>],
) {
    let reconstructed = match &transition.update {
        CachedRenderTransitionUpdate::None => old_render,
        CachedRenderTransitionUpdate::Patch(patch) => {
            let mut blocks = old_render;
            let end = patch
                .start_index
                .checked_add(patch.delete_count)
                .expect("test patch range should not overflow");
            blocks.splice(patch.start_index..end, patch.blocks.clone());
            blocks
        }
        CachedRenderTransitionUpdate::Full(blocks) => blocks.clone(),
    };
    assert_eq!(reconstructed, expected);
    assert_eq!(transition.cache.materialize(), expected);
}

#[test]
fn legacy_safe_patch_counter_counts_only_its_old_and_new_full_render_passes() {
    let schema = tiptap_schema();
    let old_doc = doc(vec![paragraph(vec![text("old")])]);
    let new_doc = doc(vec![paragraph(vec![text("new")])]);
    super::reset_cached_render_counts_for_test();

    super::safe_contiguous_render_blocks_patch(&old_doc, &new_doc, &schema, &[0])
        .expect("valid hint should produce a safe patch");

    assert_eq!(super::take_cached_render_counts_for_test(), (0, 0, 0, 0, 2));
}

#[test]
fn cached_render_slow_invariant_detects_private_block_tampering() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let document = doc(vec![
        paragraph(vec![text("first")]),
        paragraph(vec![text("second")]),
    ]);
    let mut cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();

    assert!(cache.verify_slow_invariant(&document, &schema));
    let sealed_schema = Arc::clone(&cache.schema_fingerprint);
    cache.schema_fingerprint = Arc::<str>::from("tampered-schema");
    assert!(!cache.verify_slow_invariant(&document, &schema));
    cache.schema_fingerprint = sealed_schema;
    let sealed_root = cache.document_root_seal.clone();
    let foreign = doc(vec![
        paragraph(vec![text("first")]),
        paragraph(vec![text("second")]),
    ]);
    cache.document_root_seal = foreign.root().clone();
    assert!(!cache.verify_slow_invariant(&document, &schema));
    cache.document_root_seal = sealed_root;
    let removed_block = cache.blocks.pop().unwrap();
    assert!(!cache.verify_slow_invariant(&document, &schema));
    cache.blocks.push(removed_block);
    let sealed_node = Arc::clone(&cache.blocks[0].node);
    cache.blocks[0].node = Arc::new(paragraph(vec![text("tampered")]));
    assert!(!cache.verify_slow_invariant(&document, &schema));
    cache.blocks[0].node = sealed_node;
    let sealed_node_size = cache.blocks[0].node_size;
    cache.blocks[0].node_size = cache.blocks[0].node_size.saturating_add(1);
    assert!(!cache.verify_slow_invariant(&document, &schema));
    cache.blocks[0].node_size = sealed_node_size;
    cache.blocks[1].start_pos = cache.blocks[1].start_pos.saturating_add(1);
    assert!(!cache.verify_slow_invariant(&document, &schema));
}

#[test]
fn cached_render_identity_accepts_only_the_sealed_root_and_schema() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let document = doc(vec![paragraph(vec![text("same")])]);
    let shared = document.clone();
    let foreign = doc(vec![paragraph(vec![text("same")])]);
    let schema_fingerprint = crate::schema::schema_fingerprint(&schema);
    let cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();

    assert_eq!(document, foreign);
    assert!(!document.root().shares_storage_with(foreign.root()));
    assert!(cache.matches_identity(&shared, &schema_fingerprint));
    assert!(!cache.matches_identity(&foreign, &schema_fingerprint));
    assert!(!cache.matches_identity(&shared, "foreign-schema"));
}

#[test]
fn cached_render_build_transition_and_full_fallback_propagate_identity_seals() {
    let schema = tiptap_schema();
    let schema_fingerprint = crate::schema::schema_fingerprint(&schema);
    let limits = ResourceLimits::default();
    let old_document = doc(vec![paragraph(vec![text("old")])]);
    let new_document = doc(vec![paragraph(vec![text("new")])]);
    let foreign_new = doc(vec![paragraph(vec![text("new")])]);
    let cache = CachedRenderBlocks::build(&old_document, &schema, &limits).unwrap();

    assert!(cache.matches_identity(&old_document, &schema_fingerprint));
    let transition = cache
        .transition(&old_document, &new_document, &schema, &[0], &limits)
        .unwrap();
    assert!(transition
        .cache
        .matches_identity(&new_document, &schema_fingerprint));
    assert!(!transition
        .cache
        .matches_identity(&foreign_new, &schema_fingerprint));

    let fallback = cache
        .transition(&old_document, &new_document, &schema, &[1], &limits)
        .unwrap();
    assert!(matches!(
        fallback.update,
        CachedRenderTransitionUpdate::Full(_)
    ));
    assert!(fallback
        .cache
        .matches_identity(&new_document, &schema_fingerprint));
    assert!(!fallback
        .cache
        .matches_identity(&foreign_new, &schema_fingerprint));

    let shared_old = old_document.clone();
    let unchanged = cache
        .transition(&old_document, &shared_old, &schema, &[], &limits)
        .unwrap();
    assert!(unchanged
        .cache
        .matches_identity(&shared_old, &schema_fingerprint));

    let deep_equal_old = doc(vec![paragraph(vec![text("old")])]);
    assert_eq!(old_document, deep_equal_old);
    assert!(!old_document
        .root()
        .shares_storage_with(deep_equal_old.root()));
    let resealed_unchanged = cache
        .transition(&old_document, &deep_equal_old, &schema, &[], &limits)
        .unwrap();
    assert!(resealed_unchanged
        .cache
        .matches_identity(&deep_equal_old, &schema_fingerprint));
    assert!(!resealed_unchanged
        .cache
        .matches_identity(&old_document, &schema_fingerprint));

    let new_schema = prosemirror_schema();
    let new_schema_fingerprint = crate::schema::schema_fingerprint(&new_schema);
    let schema_fallback = cache
        .transition(&old_document, &old_document, &new_schema, &[], &limits)
        .unwrap();
    assert!(schema_fallback
        .cache
        .matches_identity(&old_document, &new_schema_fingerprint));
    assert!(!schema_fallback
        .cache
        .matches_identity(&old_document, &schema_fingerprint));
}

#[test]
fn cached_render_build_and_every_transition_run_the_slow_debug_verifier() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let old_document = doc(vec![paragraph(vec![text("old")])]);
    let new_document = doc(vec![paragraph(vec![text("new")])]);

    super::reset_slow_invariant_checks_for_test();
    let cache = CachedRenderBlocks::build(&old_document, &schema, &limits).unwrap();
    assert_eq!(super::take_slow_invariant_checks_for_test(), 1);

    super::reset_slow_invariant_checks_for_test();
    cache
        .transition(&old_document, &new_document, &schema, &[0], &limits)
        .unwrap();
    assert_eq!(super::take_slow_invariant_checks_for_test(), 1);

    super::reset_slow_invariant_checks_for_test();
    let foreign_old = doc(vec![paragraph(vec![text("old")])]);
    cache
        .transition(&foreign_old, &new_document, &schema, &[0], &limits)
        .unwrap();
    assert_eq!(
        super::take_slow_invariant_checks_for_test(),
        2,
        "full fallback verifies both its rebuilt cache and transition result"
    );

    super::reset_slow_invariant_checks_for_test();
    cache
        .transition(&old_document, &old_document, &schema, &[], &limits)
        .unwrap();
    assert_eq!(super::take_slow_invariant_checks_for_test(), 1);
}

#[test]
fn localized_render_transition_matches_generic_for_supported_insert_shapes() {
    fn assert_parity(old: Document, new: Document, target: usize, inserted_scalars: u32) {
        let schema = tiptap_schema();
        let limits = ResourceLimits::default();
        let cache = CachedRenderBlocks::build(&old, &schema, &limits).unwrap();
        let affected = target.saturating_sub(1)..old.root().child_count();
        let affected = affected.collect::<Vec<_>>();
        let specialized = cache
            .transition_localized_textblock(
                &old,
                &new,
                &schema,
                target,
                i32::try_from(inserted_scalars).unwrap(),
                &limits,
            )
            .unwrap();
        let generic = cache.transition(&old, &new, &schema, &[], &limits).unwrap();
        assert_eq!(specialized.update, generic.update);
        assert_eq!(specialized.cache.materialize(), generic.cache.materialize());
        assert_eq!(specialized.rerendered_new_blocks, 1);
        if old.root().child_count() == 160 {
            let CachedRenderTransitionUpdate::Patch(patch) = &specialized.update else {
                panic!("wide localized insert must retain the generic patch contract");
            };
            assert_eq!(patch.start_index, target);
            assert_eq!(patch.delete_count, 1);
            assert_eq!(patch.blocks.len(), 1);
            let conservative =
                super::classify_cached_transition(&cache, &specialized.cache, &affected, true);
            assert_ne!(conservative, specialized.update);
            let CachedRenderTransitionUpdate::Patch(conservative) = conservative else {
                panic!("conservative range should widen the patch for this fixture");
            };
            assert!(conservative.delete_count > patch.delete_count);
            assert!(conservative.blocks.len() > patch.blocks.len());
        }
    }

    let three = doc(vec![
        paragraph(vec![text("first")]),
        paragraph(vec![text("middle")]),
        paragraph(vec![text("last")]),
    ]);
    for (target, replacement) in [(0, "firstx"), (1, "middlex"), (2, "lastx")] {
        assert_parity(
            three.clone(),
            replace_top_level(&three, target, paragraph(vec![text(replacement)])),
            target,
            1,
        );
    }

    let bold = Mark::new("bold".to_string(), HashMap::new());
    let fragmented = doc(vec![paragraph(vec![
        Node::text("ab".to_string(), vec![bold.clone()]),
        Node::text("cd".to_string(), vec![]),
    ])]);
    assert_parity(
        fragmented.clone(),
        replace_top_level(
            &fragmented,
            0,
            paragraph(vec![
                Node::text("ab".to_string(), vec![bold]),
                Node::text("c🙂\\\"\n\u{1}d".to_string(), vec![]),
            ]),
        ),
        0,
        5,
    );

    let nested = doc(vec![bullet_list(vec![
        list_item(vec![paragraph(vec![text("one")])]),
        list_item(vec![paragraph(vec![text("two")])]),
    ])]);
    assert_parity(
        nested.clone(),
        replace_top_level(
            &nested,
            0,
            bullet_list(vec![
                list_item(vec![paragraph(vec![text("one")])]),
                list_item(vec![paragraph(vec![text("twox")])]),
            ]),
        ),
        0,
        1,
    );

    let positioned_suffix = doc(vec![
        paragraph(vec![text("edit")]),
        paragraph(vec![text("later"), hard_break(), inline_atom("mention")]),
        horizontal_rule(),
        opaque_block_atom("trailing"),
    ]);
    assert_parity(
        positioned_suffix.clone(),
        replace_top_level(
            &positioned_suffix,
            0,
            paragraph(vec![text("edit expanded")]),
        ),
        0,
        9,
    );

    let wide = doc((0..160)
        .map(|index| paragraph(vec![text(&format!("block {index}"))]))
        .collect());
    assert_parity(
        wide.clone(),
        replace_top_level(&wide, 80, paragraph(vec![text("block 80x")])),
        80,
        1,
    );
}

#[test]
fn localized_render_transition_accepts_exact_element_capacity_and_rejects_one_under() {
    let schema = tiptap_schema();
    let default_limits = ResourceLimits::default();
    let old = doc(vec![paragraph(vec![
        text("a"),
        hard_break(),
        inline_atom("mention"),
        text("b"),
        hard_break(),
        inline_atom("emoji"),
        text("c"),
        hard_break(),
        inline_atom("mention"),
    ])]);
    let new = replace_top_level(
        &old,
        0,
        paragraph(vec![
            text("ax"),
            hard_break(),
            inline_atom("mention"),
            text("b"),
            hard_break(),
            inline_atom("emoji"),
            text("c"),
            hard_break(),
            inline_atom("mention"),
        ]),
    );
    let cache = CachedRenderBlocks::build(&old, &schema, &default_limits).unwrap();
    let new_cache = CachedRenderBlocks::build(&new, &schema, &default_limits).unwrap();
    let old_materialized = cache.materialize();
    let new_materialized = new_cache.materialize();
    let required_elements = old_materialized
        .iter()
        .chain(new_materialized.iter())
        .map(Vec::len)
        .max()
        .unwrap();
    const TABLE_GRID_ALLOWANCE: usize = 1;
    let exact_nodes = (required_elements - TABLE_GRID_ALLOWANCE).div_ceil(3);
    assert!(
        exact_nodes > 1,
        "fixture must make one-under resource-bound"
    );
    let exact = ResourceLimits {
        max_document_nodes: exact_nodes,
        max_table_grid_slots: TABLE_GRID_ALLOWANCE,
        ..default_limits.clone()
    };
    let one_under = ResourceLimits {
        max_document_nodes: exact_nodes - 1,
        ..exact.clone()
    };

    assert!(cache
        .transition_localized_textblock(&old, &new, &schema, 0, 1, &exact)
        .is_ok());
    assert!(matches!(
        cache.transition_localized_textblock(&old, &new, &schema, 0, 1, &one_under),
        Err(super::CachedRenderError::ResourceLimitExceeded)
    ));
}

#[test]
fn localized_render_transition_rejects_unsealed_shape_and_delta_facts() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let old = doc(vec![
        paragraph(vec![text("first")]),
        paragraph(vec![text("middle")]),
        paragraph(vec![text("last")]),
    ]);
    let new = replace_top_level(&old, 1, paragraph(vec![text("middlex")]));
    let cache = CachedRenderBlocks::build(&old, &schema, &limits).unwrap();

    assert!(matches!(
        cache.transition_localized_textblock(&old, &new, &schema, 1, 2, &limits),
        Err(super::CachedRenderError::CacheInvariantViolation)
    ));
    assert!(matches!(
        cache.transition_localized_textblock(&old, &new, &schema, 3, 1, &limits),
        Err(super::CachedRenderError::CacheInvariantViolation)
    ));

    let changed_cardinality = doc(vec![
        paragraph(vec![text("first")]),
        paragraph(vec![text("middlex")]),
        paragraph(vec![text("last")]),
        paragraph(vec![text("extra")]),
    ]);
    assert!(matches!(
        cache.transition_localized_textblock(&old, &changed_cardinality, &schema, 1, 1, &limits,),
        Err(super::CachedRenderError::CacheInvariantViolation)
    ));

    let foreign_unchanged_blocks = doc(vec![
        paragraph(vec![text("first")]),
        paragraph(vec![text("middlex")]),
        paragraph(vec![text("last")]),
    ]);
    assert!(matches!(
        cache.transition_localized_textblock(
            &old,
            &foreign_unchanged_blocks,
            &schema,
            1,
            1,
            &limits,
        ),
        Err(super::CachedRenderError::CacheInvariantViolation)
    ));
}

#[test]
fn cached_transition_rerenders_early_text_and_rebases_later_atoms() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let old_doc = doc(vec![
        paragraph(vec![text("one")]),
        paragraph(vec![text("middle")]),
        paragraph(vec![text("before "), inline_atom("mention")]),
    ]);
    let new_doc = doc(vec![
        paragraph(vec![text("one expanded")]),
        paragraph(vec![text("middle")]),
        paragraph(vec![text("before "), inline_atom("mention")]),
    ]);
    let old_render = render_blocks(&old_doc, &schema);
    let cache = CachedRenderBlocks::build(&old_doc, &schema, &limits)
        .expect("old document should be cacheable");

    let transition = cache
        .transition(&old_doc, &new_doc, &schema, &[0], &limits)
        .expect("transition should be cacheable");
    let new_render = render_blocks(&new_doc, &schema);

    assert_eq!(cache.materialize(), old_render);
    assert_eq!(transition.cache.materialize(), new_render);
    assert_eq!(transition.rerendered_new_blocks, 1);
    let CachedRenderTransitionUpdate::Patch(patch) = transition.update else {
        panic!("expected an exact contiguous patch");
    };
    let mut reconstructed = old_render;
    reconstructed.splice(
        patch.start_index..patch.start_index + patch.delete_count,
        patch.blocks,
    );
    assert_eq!(reconstructed, new_render);
}

#[test]
fn cached_transition_rebases_every_position_bearing_render_variant() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let old_doc = doc(vec![
        paragraph(vec![text("a")]),
        paragraph(vec![text("later"), hard_break(), inline_atom("inline")]),
        horizontal_rule(),
        opaque_block_atom("block"),
    ]);
    let new_doc = doc(vec![
        paragraph(vec![text("a much longer prefix")]),
        paragraph(vec![text("later"), hard_break(), inline_atom("inline")]),
        horizontal_rule(),
        opaque_block_atom("block"),
    ]);
    let old_render = render_blocks(&old_doc, &schema);
    let cache = CachedRenderBlocks::build(&old_doc, &schema, &limits).unwrap();
    let transition = cache
        .transition(&old_doc, &new_doc, &schema, &[0], &limits)
        .unwrap();
    let expected = render_blocks(&new_doc, &schema);

    assert_eq!(transition.rerendered_new_blocks, 1);
    assert_update_reconstructs(old_render.clone(), &transition, &expected);

    let reverse = transition
        .cache
        .transition(&new_doc, &old_doc, &schema, &[0], &limits)
        .unwrap();
    assert_eq!(reverse.rerendered_new_blocks, 1);
    assert_update_reconstructs(expected, &reverse, &old_render);
}

#[test]
fn cached_transition_handles_mark_only_change() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let old_doc = doc(vec![paragraph(vec![text("marked")])]);
    let mark = Mark::new("bold".to_string(), HashMap::new());
    let new_doc = doc(vec![paragraph(vec![Node::text(
        "marked".to_string(),
        vec![mark],
    )])]);
    let old_render = render_blocks(&old_doc, &schema);
    let cache = CachedRenderBlocks::build(&old_doc, &schema, &limits).unwrap();
    let transition = cache
        .transition(&old_doc, &new_doc, &schema, &[0], &limits)
        .unwrap();
    let expected = render_blocks(&new_doc, &schema);

    assert_eq!(transition.rerendered_new_blocks, 1);
    assert!(matches!(
        transition.update,
        CachedRenderTransitionUpdate::Patch(_)
    ));
    assert_update_reconstructs(old_render, &transition, &expected);
}

#[test]
fn cached_transition_handles_top_level_insert_and_delete() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let initial = doc(vec![
        paragraph(vec![text("one")]),
        paragraph(vec![text("three")]),
    ]);
    let inserted = doc(vec![
        paragraph(vec![text("one")]),
        paragraph(vec![text("two")]),
        paragraph(vec![text("three")]),
    ]);

    let initial_render = render_blocks(&initial, &schema);
    let initial_cache = CachedRenderBlocks::build(&initial, &schema, &limits).unwrap();
    let insertion = initial_cache
        .transition(&initial, &inserted, &schema, &[1], &limits)
        .unwrap();
    let inserted_render = render_blocks(&inserted, &schema);
    assert_eq!(insertion.rerendered_new_blocks, 1);
    assert_update_reconstructs(initial_render, &insertion, &inserted_render);

    let deletion = insertion
        .cache
        .transition(&inserted, &initial, &schema, &[1], &limits)
        .unwrap();
    assert_eq!(deletion.rerendered_new_blocks, 0);
    assert_update_reconstructs(
        inserted_render,
        &deletion,
        &render_blocks(&initial, &schema),
    );
}

#[test]
fn cached_transition_handles_lists_and_rebases_later_atom() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let list = |first: &str| {
        bullet_list(vec![
            list_item(vec![paragraph(vec![text(first)])]),
            list_item(vec![paragraph(vec![text("second")])]),
        ])
    };
    let old_doc = doc(vec![list("first"), paragraph(vec![inline_atom("later")])]);
    let new_doc = doc(vec![
        list("first item expanded"),
        paragraph(vec![inline_atom("later")]),
    ]);
    let old_render = render_blocks(&old_doc, &schema);
    let cache = CachedRenderBlocks::build(&old_doc, &schema, &limits).unwrap();
    let transition = cache
        .transition(&old_doc, &new_doc, &schema, &[0], &limits)
        .unwrap();
    let expected = render_blocks(&new_doc, &schema);

    assert_eq!(transition.rerendered_new_blocks, 1);
    assert_update_reconstructs(old_render, &transition, &expected);
}

#[test]
fn cached_transition_classifies_net_zero_as_none_even_with_invalid_hint() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let document = doc(vec![paragraph(vec![text("same")])]);
    let cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();
    let transition = cache
        .transition(&document, &document, &schema, &[usize::MAX], &limits)
        .unwrap();

    assert_eq!(transition.rerendered_new_blocks, 0);
    assert_eq!(transition.update, CachedRenderTransitionUpdate::None);
}

#[test]
fn cached_transition_falls_back_when_schema_fingerprint_changes() {
    let old_schema = tiptap_schema();
    let new_schema = prosemirror_schema();
    let limits = ResourceLimits::default();
    let document = doc(vec![paragraph(vec![text("same document")])]);
    let cache = CachedRenderBlocks::build(&document, &old_schema, &limits).unwrap();

    let transition = cache
        .transition(&document, &document, &new_schema, &[], &limits)
        .unwrap();

    assert_eq!(
        transition.update,
        CachedRenderTransitionUpdate::Full(render_blocks(&document, &new_schema))
    );
}

#[test]
fn cached_transition_uses_full_fallback_for_invalid_hint() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let old_doc = doc(vec![paragraph(vec![text("old")])]);
    let new_doc = doc(vec![paragraph(vec![text("new")])]);
    let cache = CachedRenderBlocks::build(&old_doc, &schema, &limits).unwrap();
    let transition = cache
        .transition(&old_doc, &new_doc, &schema, &[1], &limits)
        .unwrap();

    assert_eq!(
        transition.update,
        CachedRenderTransitionUpdate::Full(render_blocks(&new_doc, &schema))
    );
}

#[test]
fn cached_transition_uses_full_fallback_when_changed_document_renders_identically() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let ignored = |flag: bool| {
        Node::element(
            "unrecognisedContainer".to_string(),
            HashMap::from([("flag".to_string(), serde_json::Value::Bool(flag))]),
            Fragment::from(vec![]),
        )
    };
    let old_doc = doc(vec![ignored(false)]);
    let new_doc = doc(vec![ignored(true)]);
    let cache = CachedRenderBlocks::build(&old_doc, &schema, &limits).unwrap();
    let transition = cache
        .transition(&old_doc, &new_doc, &schema, &[0], &limits)
        .unwrap();

    assert_eq!(
        transition.update,
        CachedRenderTransitionUpdate::Full(render_blocks(&new_doc, &schema))
    );
}

#[test]
fn cached_render_build_obeys_document_node_limit() {
    let schema = tiptap_schema();
    let limits = ResourceLimits {
        max_document_nodes: 2,
        ..ResourceLimits::default()
    };
    let document = doc(vec![paragraph(vec![text("too many nodes")])]);

    assert!(matches!(
        CachedRenderBlocks::build(&document, &schema, &limits),
        Err(super::CachedRenderError::ResourceLimitExceeded)
    ));
}

#[test]
fn cached_render_build_uses_canonical_root_depth_one() {
    let schema = tiptap_schema();
    let exact_limits = ResourceLimits {
        max_document_depth: 3,
        ..ResourceLimits::default()
    };
    let over_limits = ResourceLimits {
        max_document_depth: 2,
        ..ResourceLimits::default()
    };
    let document = doc(vec![paragraph(vec![text("depth three")])]);

    assert!(CachedRenderBlocks::build(&document, &schema, &exact_limits).is_ok());
    assert!(matches!(
        CachedRenderBlocks::build(&document, &schema, &over_limits),
        Err(super::CachedRenderError::ResourceLimitExceeded)
    ));
}

#[test]
fn cached_render_build_rejects_root_width_over_remaining_node_budget() {
    let schema = tiptap_schema();
    let limits = ResourceLimits {
        max_document_nodes: 3,
        ..ResourceLimits::default()
    };
    let document = doc(vec![
        paragraph(vec![]),
        paragraph(vec![]),
        paragraph(vec![]),
    ]);

    assert!(matches!(
        CachedRenderBlocks::build(&document, &schema, &limits),
        Err(super::CachedRenderError::ResourceLimitExceeded)
    ));
}

#[test]
fn cached_render_build_rejects_ordered_list_number_overflow() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let document = doc(vec![ordered_list(
        u32::MAX,
        vec![
            list_item(vec![paragraph(vec![text("one")])]),
            list_item(vec![paragraph(vec![text("two")])]),
        ],
    )]);

    assert!(matches!(
        CachedRenderBlocks::build(&document, &schema, &limits),
        Err(super::CachedRenderError::PositionOverflow)
    ));
}

#[test]
fn incremental_ordered_list_indices_are_exact_or_structured_overflow() {
    let schema = tiptap_schema();
    let exact = doc(vec![ordered_list(
        u32::MAX,
        vec![list_item(vec![paragraph(vec![text("last")])])],
    )]);

    let exact_blocks = try_render_blocks(&exact, &schema).expect("u32::MAX must render");
    let RenderElement::BlockStart {
        list_context: Some(context),
        ..
    } = &exact_blocks[0][0]
    else {
        panic!("ordered-list item must carry a list context");
    };
    assert_eq!(context.index, u32::MAX);

    let overflow = doc(vec![ordered_list(
        u32::MAX,
        vec![
            list_item(vec![paragraph(vec![text("last")])]),
            list_item(vec![paragraph(vec![text("overflow")])]),
        ],
    )]);
    assert!(matches!(
        try_render_blocks(&overflow, &schema),
        Err(super::CachedRenderError::PositionOverflow)
    ));
}

#[test]
fn ordered_list_start_defaults_when_absent_or_null_and_rejects_malformed_values() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let missing = doc(vec![ordered_list_with_start(
        None,
        vec![list_item(vec![paragraph(vec![text("first")])])],
    )]);

    let null_start = doc(vec![ordered_list_with_start(
        Some(serde_json::Value::Null),
        vec![list_item(vec![paragraph(vec![text("first")])])],
    )]);

    for (document, label) in [(missing, "missing"), (null_start, "null")] {
        let blocks = try_render_blocks(&document, &schema)
            .unwrap_or_else(|error| panic!("{label} start defaults to one, got {error:?}"));
        let RenderElement::BlockStart {
            list_context: Some(context),
            ..
        } = &blocks[0][0]
        else {
            panic!("ordered-list item must carry a list context");
        };
        assert_eq!(context.index, 1, "{label} start must default to one");
    }

    for start in [
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!("1"),
        serde_json::json!(u64::from(u32::MAX) + 1),
    ] {
        let malformed = doc(vec![ordered_list_with_start(
            Some(start),
            vec![list_item(vec![paragraph(vec![text("bad")])])],
        )]);
        assert!(matches!(
            CachedRenderBlocks::build(&malformed, &schema, &limits),
            Err(super::CachedRenderError::InvalidOrderedListStart)
        ));
        assert!(matches!(
            try_render_blocks(&malformed, &schema),
            Err(super::CachedRenderError::InvalidOrderedListStart)
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn cached_transition_always_reconstructs_and_matches_full_render(
        values in prop::collection::vec("[a-z]{0,12}", 1..8),
        replacement in "[a-z]{0,12}",
        raw_index in any::<usize>(),
    ) {
        let schema = tiptap_schema();
        let limits = ResourceLimits::default();
        let index = raw_index % values.len();
        let old_doc = doc(values.iter().map(|value| paragraph(vec![text(value)])).collect());
        let mut new_values = values;
        new_values[index] = replacement;
        let new_doc = doc(
            new_values
                .iter()
                .map(|value| paragraph(vec![text(value)]))
                .collect(),
        );
        let old_render = render_blocks(&old_doc, &schema);
        let cache = CachedRenderBlocks::build(&old_doc, &schema, &limits).unwrap();
        let transition = cache
            .transition(&old_doc, &new_doc, &schema, &[index], &limits)
            .unwrap();
        let expected = render_blocks(&new_doc, &schema);

        assert_update_reconstructs(old_render, &transition, &expected);
    }
}

fn web_authored_ordered_list(start: serde_json::Value, labels: &[&str]) -> Document {
    let items = labels
        .iter()
        .map(|label| {
            serde_json::json!({
                "type": "listItem",
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": label }],
                }],
            })
        })
        .collect::<Vec<_>>();
    let json = serde_json::json!({
        "type": "doc",
        "content": [{
            "type": "orderedList",
            "attrs": { "start": start },
            "content": items,
        }],
    });
    crate::serialize::from_prosemirror_json(
        &json,
        &tiptap_schema(),
        crate::serialize::UnknownTypeMode::Error,
    )
    .expect("web-authored ordered list must import")
}

#[test]
fn cached_render_accepts_a_web_authored_float_ordered_list_start() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let document = web_authored_ordered_list(serde_json::json!(3.0), &["Three", "Four"]);

    let blocks = try_render_blocks(&document, &schema)
        .expect("a float-valued start of 3.0 must render instead of raising PositionOverflow");
    CachedRenderBlocks::build(&document, &schema, &limits)
        .expect("a float-valued start of 3.0 must build cached render blocks");

    let indexes = blocks
        .iter()
        .flatten()
        .filter_map(|element| match element {
            RenderElement::BlockStart {
                list_context: Some(context),
                ..
            } => Some(context.index),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![3, 4],
        "a float-valued start of 3.0 must number the cached render from 3"
    );
}

#[test]
fn localized_table_edits_preserve_source_correspondence_and_fresh_render_parity() {
    const ROWS: usize = 3;
    const COLUMNS: usize = 3;
    const INSERTION: &str = "changed🙂";
    const CELL_TEXT_OFFSET: u32 = 2;
    let schema = crate::schema::presets::prosemirror_table_schema();
    let limits = ResourceLimits::default();
    for irregular in [false, true] {
        let mut source =
            crate::test_support::large_table_fixture::plain_table_document(ROWS, COLUMNS);
        for row in source["content"][0]["content"].as_array_mut().unwrap() {
            for cell in row["content"].as_array_mut().unwrap() {
                cell["content"][0]["content"][0]["text"] = serde_json::json!("same");
            }
        }
        if irregular {
            source["content"][0]["content"][0]["content"][0]["type"] =
                serde_json::json!("table_header");
            source["content"][0]["content"][0]["content"][0]["attrs"]["colspan"] =
                serde_json::json!(2);
            source["content"][0]["content"][1]["content"]
                .as_array_mut()
                .unwrap()
                .pop();
        }
        let old = crate::serialize::from_prosemirror_json(
            &source,
            &schema,
            crate::serialize::UnknownTypeMode::Error,
        )
        .unwrap();
        let cache = CachedRenderBlocks::build(&old, &schema, &limits).unwrap();
        let mut records = Vec::new();
        cache.visit_table_records(&mut records);
        let (table_pos, table) = records[0];
        let starts = crate::tables::render::absolute_cell_starts(table, table_pos);
        for index in [0, starts.len() / 2, starts.len() - 1] {
            let step = crate::transform::Step::InsertText {
                pos: starts[index] + CELL_TEXT_OFFSET,
                text: INSERTION.into(),
                marks: Vec::new(),
            };
            let (new, _) = crate::transform::apply_step(&old, &step, &schema).unwrap();
            let transition = cache
                .transition_localized_textblock(
                    &old,
                    &new,
                    &schema,
                    0,
                    INSERTION.chars().count() as i32,
                    &limits,
                )
                .unwrap();
            let fresh = CachedRenderBlocks::build(&new, &schema, &limits).unwrap();
            assert_eq!(
                transition.cache.materialize(),
                fresh.materialize(),
                "irregular={irregular}, edited cell={index}"
            );
            let mut updated = Vec::new();
            transition.cache.visit_table_records(&mut updated);
            for (other, prior) in table.cells.iter().enumerate() {
                assert_eq!(Arc::ptr_eq(prior, &updated[0].1.cells[other]), other != index,
                    "irregular={irregular}, edit={index}, source cell={other}: equal text does not exchange cell identities");
            }
        }
    }
}

#[test]
fn localized_textblock_nested_table_changes_rebuild_projection() {
    const CONTAINER: &str = "table_text_container";
    let mut config =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    config["nodes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name":CONTAINER,"content":"block*","group":"block","role":"textBlock"
        }));
    let schema = crate::schema::Schema::from_json(&config).unwrap();
    let nested = crate::serialize::from_prosemirror_json(
        &crate::test_support::large_table_fixture::plain_table_document(1, 1),
        &schema,
        crate::serialize::UnknownTypeMode::Error,
    )
    .unwrap()
    .root()
    .child(0)
    .unwrap()
    .clone();
    let limits = ResourceLimits::default();
    let empty = doc(vec![Node::element(
        CONTAINER.into(),
        HashMap::new(),
        Fragment::from(Vec::new()),
    )]);
    let populated = doc(vec![Node::element(
        CONTAINER.into(),
        HashMap::new(),
        Fragment::from(vec![nested]),
    )]);
    for (old, new) in [(&empty, &populated), (&populated, &empty)] {
        let cache = CachedRenderBlocks::build(old, &schema, &limits).unwrap();
        let delta = new.root().node_size() as i32 - old.root().node_size() as i32;
        let transition = cache
            .transition_localized_textblock(old, new, &schema, 0, delta, &limits)
            .unwrap();
        let fresh = CachedRenderBlocks::build(new, &schema, &limits).unwrap();
        assert_eq!(
            transition
                .cache
                .table_projection_index
                .positions()
                .collect::<Vec<_>>(),
            fresh.table_projection_index.positions().collect::<Vec<_>>(),
            "delta={delta}: added/removed table projection must match fresh derivation"
        );
        assert_eq!(
            transition.cache.materialize(),
            fresh.materialize(),
            "delta={delta}: nested content matches fresh rendering"
        );
        for block in &transition.cache.blocks {
            assert_eq!(
                block.element_count,
                crate::tables::render::element_count(&block.elements)
            );
        }
    }
}

#[test]
fn localized_table_element_counts_follow_split_merge_and_resource_boundaries() {
    const TABLE_INDEX: usize = 1;
    const INLINE_OFFSET: u32 = 3;
    const RENDER_ELEMENTS_PER_NODE: usize = 3;
    let schema = crate::schema::presets::prosemirror_table_schema();
    let limits = ResourceLimits::default();
    let table_document = crate::serialize::from_prosemirror_json(
        &crate::test_support::large_table_fixture::plain_table_document(3, 3),
        &schema,
        crate::serialize::UnknownTypeMode::Error,
    )
    .unwrap();
    let mut document = doc(vec![
        paragraph(vec![text("before")]),
        table_document.root().child(0).unwrap().clone(),
        paragraph(vec![text("after")]),
    ]);
    let mut cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();
    let mut records = Vec::new();
    cache.visit_table_records(&mut records);
    let (table_pos, table) = records[0];
    let starts = crate::tables::render::absolute_cell_starts(table, table_pos);
    let position = starts[starts.len() / 2] + INLINE_OFFSET;
    for iteration in 0..4 {
        let inserting = iteration % 2 == 0;
        let step = if inserting {
            crate::transform::Step::InsertText {
                pos: position,
                text: "x".into(),
                marks: vec![Mark::new("strong".into(), HashMap::new())],
            }
        } else {
            crate::transform::Step::DeleteRange {
                from: position,
                to: position + 1,
            }
        };
        let (next, _) = crate::transform::apply_step(&document, &step, &schema).unwrap();
        let fresh = CachedRenderBlocks::build(&next, &schema, &limits).unwrap();
        let required = fresh
            .blocks
            .iter()
            .map(|block| crate::tables::render::element_count(&block.elements))
            .sum::<usize>();
        let exact = ResourceLimits {
            max_document_nodes: fresh.blocks.len(),
            max_table_grid_slots: required - fresh.blocks.len() * RENDER_ELEMENTS_PER_NODE,
            ..limits.clone()
        };
        let one_under = ResourceLimits {
            max_table_grid_slots: exact.max_table_grid_slots - 1,
            ..exact.clone()
        };
        let delta = if inserting { 1 } else { -1 };
        assert!(
            matches!(
                cache.transition_localized_textblock(
                    &document,
                    &next,
                    &schema,
                    TABLE_INDEX,
                    delta,
                    &one_under
                ),
                Err(super::CachedRenderError::ResourceLimitExceeded)
            ),
            "iteration={iteration}: one fewer allowed element must reject the localized edit"
        );
        let transition = cache
            .transition_localized_textblock(&document, &next, &schema, TABLE_INDEX, delta, &exact)
            .unwrap();
        assert_eq!(
            transition.cache.materialize(),
            fresh.materialize(),
            "iteration={iteration}"
        );
        assert_eq!(transition.cache.table_attributes, fresh.table_attributes);
        for block in cache.blocks.iter().chain(&transition.cache.blocks) {
            assert_eq!(block.element_count, crate::tables::render::element_count(&block.elements),
                "iteration={iteration}: old and new snapshots retain exact counts, including rebased siblings");
        }
        assert_eq!(
            transition.cache.blocks[TABLE_INDEX].element_count as isize
                - cache.blocks[TABLE_INDEX].element_count as isize,
            if inserting { 2 } else { -2 },
            "Marked insertion splits a text run; deletion merges it again"
        );
        document = next;
        cache = transition.cache;
    }
    let removed = replace_top_level(&document, TABLE_INDEX, paragraph(vec![text("replacement")]));
    let transition = cache
        .transition(&document, &removed, &schema, &[TABLE_INDEX], &limits)
        .unwrap();
    let fresh = CachedRenderBlocks::build(&removed, &schema, &limits).unwrap();
    assert_eq!(transition.cache.materialize(), fresh.materialize());
    assert!(
        transition.cache.table_attributes.is_empty(),
        "Structural fallback must prune attribute keys retained by preceding local edits"
    );
    for block in &transition.cache.blocks {
        assert_eq!(
            block.element_count,
            crate::tables::render::element_count(&block.elements)
        );
    }
}

#[test]
fn table_output_meter_reuses_only_identical_ordered_cell_allocations() {
    use crate::render::output_bytes::render_element_bytes;
    use crate::tables::render::CELL_OUTPUT_METER_VISITS;
    let schema = crate::schema::presets::prosemirror_table_schema();
    let document = crate::serialize::from_prosemirror_json(
        &crate::test_support::large_table_fixture::plain_table_document(3, 3),
        &schema,
        crate::serialize::UnknownTypeMode::Error,
    )
    .unwrap();
    let cache = CachedRenderBlocks::build(&document, &schema, &ResourceLimits::default()).unwrap();
    let output = cache.materialize();
    let RenderElement::Table { table, .. } = &output[0][0] else {
        panic!("table fixture");
    };
    let expected = table.cells.iter().fold(0usize, |bytes, cell| {
        bytes.saturating_add(cell.retained_bytes(render_element_bytes))
    });
    CELL_OUTPUT_METER_VISITS.set(0);
    assert_eq!(cache.table_cell_output_bytes(0, table), Some(expected));
    assert_eq!(CELL_OUTPUT_METER_VISITS.replace(0), table.cells.len());
    assert_eq!(cache.table_cell_output_bytes(0, table), Some(expected));
    assert_eq!(
        CELL_OUTPUT_METER_VISITS.replace(0),
        0,
        "an unchanged public clone must not recursively meter its cells again"
    );
    assert_eq!(cache.table_cell_output_bytes(usize::MAX, table), None);
    let mut reordered = table.clone();
    reordered.edit_parts_for_testing(|_, cells| cells.swap(0, 1));
    assert_eq!(cache.table_cell_output_bytes(0, &reordered), None);
    let mut duplicated = table.clone();
    duplicated.edit_parts_for_testing(|_, cells| cells[1] = Arc::clone(&cells[0]));
    assert_eq!(cache.table_cell_output_bytes(0, &duplicated), None);
    let mut changed = table.clone();
    const EXTRA_CAPACITY: usize = 1024;
    changed.edit_parts_for_testing(|_, cells| {
        Arc::make_mut(&mut cells[0])
            .attrs_key
            .reserve(EXTRA_CAPACITY);
    });
    assert_eq!(
        cache.table_cell_output_bytes(0, &changed),
        None,
        "a detached key with different capacity cannot reuse the aggregate"
    );
    assert_eq!(
        cache.table_cell_output_bytes(0, table),
        Some(expected),
        "old snapshots remain unchanged"
    );
}

#[test]
fn table_output_meter_tracks_localized_edits_rebases_and_saturation() {
    use crate::render::output_bytes::render_element_bytes;
    use crate::tables::render::CELL_OUTPUT_METER_VISITS;
    const TABLE_INDEX: usize = 1;
    const INLINE_OFFSET: u32 = 3;
    let schema = crate::schema::presets::prosemirror_table_schema();
    let limits = ResourceLimits::default();
    let table_document = crate::serialize::from_prosemirror_json(
        &crate::test_support::large_table_fixture::plain_table_document(3, 3),
        &schema,
        crate::serialize::UnknownTypeMode::Error,
    )
    .unwrap();
    let mut document = doc(vec![
        paragraph(vec![text("before")]),
        table_document.root().child(0).unwrap().clone(),
    ]);
    let mut cache = CachedRenderBlocks::build(&document, &schema, &limits).unwrap();
    let old_snapshot = cache.clone();
    let mut records = Vec::new();
    cache.visit_table_records(&mut records);
    let (table_pos, table) = records[0];
    let position = crate::tables::render::absolute_cell_starts(table, table_pos)[0] + INLINE_OFFSET;
    let original = cache.table_cell_output_bytes(TABLE_INDEX, table).unwrap();
    for iteration in 0..4 {
        let inserting = iteration % 2 == 0;
        let step = if inserting {
            crate::transform::Step::InsertText {
                pos: position,
                text: "x".into(),
                marks: vec![Mark::new("strong".into(), HashMap::new())],
            }
        } else {
            crate::transform::Step::DeleteRange {
                from: position,
                to: position + 1,
            }
        };
        let (next, _) = crate::transform::apply_step(&document, &step, &schema).unwrap();
        let transition = cache
            .transition_localized_textblock(
                &document,
                &next,
                &schema,
                TABLE_INDEX,
                if inserting { 1 } else { -1 },
                &limits,
            )
            .unwrap();
        let output = transition.cache.materialize();
        let RenderElement::Table { table, .. } = &output[TABLE_INDEX][0] else {
            panic!("table fixture");
        };
        let expected = table.cells.iter().fold(0usize, |bytes, cell| {
            bytes.saturating_add(cell.retained_bytes(render_element_bytes))
        });
        CELL_OUTPUT_METER_VISITS.set(0);
        assert_eq!(
            transition.cache.table_cell_output_bytes(TABLE_INDEX, table),
            Some(expected),
            "edit {iteration}"
        );
        assert_eq!(
            CELL_OUTPUT_METER_VISITS.replace(0),
            0,
            "edit {iteration}: transferred aggregate avoids full scan"
        );
        document = next;
        cache = transition.cache;
    }
    let rebased = super::rebase_cached_block(
        &cache.blocks[TABLE_INDEX],
        document.root().child(TABLE_INDEX).unwrap(),
        cache.blocks[TABLE_INDEX].start_pos + 1,
    )
    .unwrap();
    assert_eq!(
        rebased.cell_output_bytes.get(),
        cache.blocks[TABLE_INDEX].cell_output_bytes.get()
    );
    let original_output = old_snapshot.materialize();
    let RenderElement::Table { table, .. } = &original_output[TABLE_INDEX][0] else {
        panic!("table fixture");
    };
    assert_eq!(
        old_snapshot.table_cell_output_bytes(TABLE_INDEX, table),
        Some(original)
    );
    cache.blocks[TABLE_INDEX].cell_output_bytes = std::sync::OnceLock::from(usize::MAX);
    let step = crate::transform::Step::InsertText {
        pos: position,
        text: "z".into(),
        marks: vec![],
    };
    let (next, _) = crate::transform::apply_step(&document, &step, &schema).unwrap();
    let transition = cache
        .transition_localized_textblock(&document, &next, &schema, TABLE_INDEX, 1, &limits)
        .unwrap();
    assert!(
        transition.cache.blocks[TABLE_INDEX]
            .cell_output_bytes
            .get()
            .is_none(),
        "saturated sum must be recomputed"
    );
    let output = transition.cache.materialize();
    let RenderElement::Table { table, .. } = &output[TABLE_INDEX][0] else {
        panic!("table fixture");
    };
    let expected = table.cells.iter().fold(0usize, |bytes, cell| {
        bytes.saturating_add(cell.retained_bytes(render_element_bytes))
    });
    assert_eq!(
        transition.cache.table_cell_output_bytes(TABLE_INDEX, table),
        Some(expected)
    );
}
