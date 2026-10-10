use super::super::{tests::initialize_test_document, DerivedStateCache};
use super::*;
use crate::schema::presets::tiptap_schema;
use serde_json::json;

fn fixture(count: usize) -> DerivedStateCache {
    initialize_test_document(
        &tiptap_schema(),
        json!({
            "type": "doc", "content": (0..count).map(|index| json!({
                "type": "paragraph", "content": [{"type":"text", "text":format!("leaf-{index}🙂")}]
            })).collect::<Vec<_>>()
        }),
    )
    .unwrap()
}

fn edit(
    state: &DerivedStateCache,
    slot: usize,
    text: &str,
) -> (LocalizedTextblockEditAdmission, Vec<u32>, DerivedStateCache) {
    let schema = tiptap_schema();
    let leaf = state
        .localized_text_index
        .as_ref()
        .unwrap()
        .leaf(slot)
        .unwrap();
    let admission = state
        .localized_insert_admission_for_test(
            leaf.doc_end,
            text,
            &[],
            &schema,
            &ResourceLimits::default(),
            None,
            0,
        )
        .unwrap();
    let path = state
        .position_map
        .block(leaf.block_index)
        .unwrap()
        .node_path
        .clone();
    let (preview, _) = crate::transform::apply_step_canonical_marks(
        &state.document,
        &crate::transform::Step::InsertText {
            pos: leaf.doc_end,
            text: text.into(),
            marks: Vec::new(),
        },
        &schema,
    )
    .unwrap();
    let artifact = state
        .canonical_artifact
        .schema_context()
        .derive(&preview)
        .unwrap();
    let next = initialize_test_document(&schema, artifact.value().clone()).unwrap();
    (admission, path.to_vec(), next)
}

fn carry(
    state: &DerivedStateCache,
    admission: &LocalizedTextblockEditAdmission,
    path: &[u32],
    next: &DerivedStateCache,
    budget: usize,
) -> Option<LocalizedTextLeafIndex> {
    state
        .localized_text_index
        .as_ref()?
        .carry_after_textblock_edit(
            &state.validation_certificate,
            admission,
            path,
            &next.document,
            &next.canonical_artifact,
            budget,
            &next.position_map,
            &next.rendered_text,
        )
}

#[test]
fn repeated_unicode_edits_share_bounded_base_and_match_fresh_certificates() {
    for (count, target) in [(1, 0), (9, 0), (9, 4), (9, 8)] {
        let mut state = fixture(count);
        // Preserve the original spare-capacity charge as well as the packed case.
        if target == 4 {
            let index = state.localized_text_index.as_mut().unwrap();
            index.leaves.try_reserve_exact(count).unwrap();
            index.retained_bytes =
                index.leaves.capacity() * std::mem::size_of::<LocalizedTextLeafCertificate>();
        }
        for (step, text) in ["x", "🙂", "e\u{301}", "\u{200d}", "漢字", "\\\""]
            .into_iter()
            .enumerate()
        {
            let original = state.localized_text_index.as_ref().unwrap().clone();
            let budget = original.promotion_transient_budget_for_test().unwrap();
            let (admission, path, mut next) = edit(&state, target, text);
            assert!(
                carry(&state, &admission, &path, &next, budget - 1).is_none(),
                "one under, {count}/{target}/{step}"
            );
            LocalizedTextLeafIndex::take_carried_leaf_copies_for_test();
            let mut promoted = carry(&state, &admission, &path, &next, budget).unwrap();
            assert_eq!(
                LocalizedTextLeafIndex::take_carried_leaf_copies_for_test(),
                if step == 0 { count - 1 } else { 0 },
                "copies {count}/{target}/{step}"
            );
            assert_eq!(
                promoted.leaves(),
                next.localized_text_index.as_ref().unwrap().leaves(),
                "fresh oracle {count}/{target}/{step}"
            );
            assert_eq!(
                state.localized_text_index.as_ref().unwrap(),
                &original,
                "preparation changed installed authority"
            );
            let overlay = promoted.overlay.as_ref().unwrap();
            assert!(overlay.heap_bytes().unwrap() <= promoted.retained_bytes);
            assert_eq!(overlay.base.len(), count - 1);
            if let Some(previous) = original.overlay.as_ref() {
                assert!(Arc::ptr_eq(&overlay.base, &previous.base));
            }
            let clone_budget = promoted.promotion_transient_budget_for_test().unwrap();
            assert!(promoted.try_clone(clone_budget - 1).is_none());
            let cloned = promoted.try_clone(clone_budget).unwrap();
            assert_eq!(cloned, promoted);
            assert!(Arc::ptr_eq(
                &cloned.overlay.as_ref().unwrap().base,
                &overlay.base
            ));
            promoted.materialize_canonical_fingerprint(&next.validation_certificate);
            next.localized_text_index = Some(promoted);
            state = next;
        }
        if count > 1 {
            let retarget = (target + 1) % count;
            let (admission, path, next) = edit(&state, retarget, "新");
            let promoted = carry(
                &state,
                &admission,
                &path,
                &next,
                ResourceLimits::default().max_input_bytes,
            )
            .unwrap();
            assert!(
                promoted.overlay.is_none(),
                "retarget uses the existing destination"
            );
            assert_eq!(
                promoted.leaves(),
                next.localized_text_index.as_ref().unwrap().leaves()
            );
        }
    }
}

#[test]
fn overlay_allocation_failures_leave_installed_index_unchanged() {
    let mut state = fixture(3);
    for iteration in 0..2 {
        let (admission, path, mut next) = edit(&state, 1, "🙂");
        let original = state.localized_text_index.as_ref().unwrap().clone();
        let budget = original.promotion_transient_budget_for_test().unwrap();
        for stage in [
            LocalizedIndexAllocationStage::PromotionClone,
            LocalizedIndexAllocationStage::PromotionGrowth,
            LocalizedIndexAllocationStage::PromotionUpdate,
        ] {
            super::super::observability::force_localized_index_allocation_stage_for_test(Some(
                stage,
            ));
            let failed = carry(&state, &admission, &path, &next, budget);
            super::super::observability::force_localized_index_allocation_stage_for_test(None);
            assert!(failed.is_none(), "{iteration}/{stage:?}");
            assert_eq!(state.localized_text_index.as_ref().unwrap(), &original);
        }
        super::super::observability::force_localized_index_allocation_stage_for_test(Some(
            LocalizedIndexAllocationStage::InitialLeafCapacity,
        ));
        let failed = original.try_clone(budget);
        super::super::observability::force_localized_index_allocation_stage_for_test(None);
        assert!(failed.is_none());
        let mut promoted = carry(&state, &admission, &path, &next, budget).unwrap();
        promoted.materialize_canonical_fingerprint(&next.validation_certificate);
        next.localized_text_index = Some(promoted);
        state = next;
    }
}

#[test]
fn suffix_maxima_check_start_and_end_overflow_in_every_coordinate() {
    for field in 0..6 {
        let mut state = fixture(3);
        let (admission, path, next) = edit(&state, 0, "x");
        let index = state.localized_text_index.as_mut().unwrap();
        let suffix = &mut index.leaves[2];
        match field {
            0 => suffix.doc_start = u32::MAX,
            1 => suffix.doc_end = u32::MAX,
            2 => suffix.scalar_start = u32::MAX,
            3 => suffix.scalar_end = u32::MAX,
            4 => suffix.utf16_start = u32::MAX,
            5 => suffix.utf16_end = u32::MAX,
            _ => unreachable!(),
        }
        let original = index.clone();
        assert!(
            carry(
                &state,
                &admission,
                &path,
                &next,
                ResourceLimits::default().max_input_bytes
            )
            .is_none(),
            "field {field}"
        );
        assert_eq!(state.localized_text_index.as_ref().unwrap(), &original);
    }
    assert!(LeafOffsets {
        document: u32::MAX,
        scalar: 0,
        utf16: 0
    }
    .checked_add(LeafOffsets {
        document: 1,
        scalar: 0,
        utf16: 0
    })
    .is_none());
}

#[test]
fn reused_overlay_rejects_cumulative_suffix_overflow_without_mutating_source() {
    for field in 0..6 {
        let state = fixture(3);
        let (admission, path, mut next) = edit(&state, 0, "x");
        let mut promoted = carry(
            &state,
            &admission,
            &path,
            &next,
            ResourceLimits::default().max_input_bytes,
        )
        .unwrap();
        let overlay = promoted.overlay.as_mut().unwrap();
        let base = Arc::get_mut(&mut overlay.base).unwrap();
        let leaf = base.last_mut().unwrap();
        match field {
            0 => leaf.doc_start = u32::MAX - 1,
            1 => leaf.doc_end = u32::MAX - 1,
            2 => leaf.scalar_start = u32::MAX - 1,
            3 => leaf.scalar_end = u32::MAX - 1,
            4 => leaf.utf16_start = u32::MAX - 1,
            5 => leaf.utf16_end = u32::MAX - 1,
            _ => unreachable!(),
        }
        let mut maxima = LeafOffsets::default();
        for leaf in base.iter() {
            maxima.include(leaf);
        }
        overlay.suffix_maxima = Some(maxima);
        assert!(
            promoted.leaf(2).is_some(),
            "one positive delta fits exactly at {field}"
        );
        promoted.materialize_canonical_fingerprint(&next.validation_certificate);
        next.localized_text_index = Some(promoted);
        let original = next.localized_text_index.clone();
        let (admission, path, after) = edit(&next, 0, "x");
        assert!(
            carry(
                &next,
                &admission,
                &path,
                &after,
                ResourceLimits::default().max_input_bytes
            )
            .is_none(),
            "second delta overflows at {field}"
        );
        assert_eq!(next.localized_text_index, original);
    }
}
