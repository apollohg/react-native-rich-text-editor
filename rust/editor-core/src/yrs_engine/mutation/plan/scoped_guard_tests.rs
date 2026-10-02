use super::*;
use crate::yrs_engine::compiler::CompilationReadTransaction;
use crate::yrs_engine::{Affinity, EditorOffsetKind, RevisionedPosition, TypedOperation};
use yrs::updates::decoder::Decode;
use yrs::{Doc, Transact, Update};

const REQUEST_ID: u64 = 122;
const FRAGMENT_NAME: &str = "prosemirror";
const SNAPSHOT_MISMATCH: &str = "Yrs document snapshot changed before mutation preflight";

fn fixture() -> (Doc, YrsMutationPlan) {
    let source = serde_json::json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"scope"}]}]});
    let (doc, _, _, compiled) = crate::yrs_engine::mutation_tests::compile_operations_with_schema(
        &source,
        vec![TypedOperation::InsertText {
            at: RevisionedPosition {
                offset: 1,
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::After,
            },
            text: "!".into(),
            marks: vec![],
        }],
        crate::schema::presets::tiptap_schema(),
    );
    (doc, compiled.mutation_plan)
}

fn candidate(owner: &CompilationReadTransaction<'_>) -> Doc {
    let doc = Doc::with_options(yrs::Options {
        offset_kind: yrs::OffsetKind::Utf16,
        ..Default::default()
    });
    let update = owner.encode_state_as_update_v1(&StateVector::default());
    doc.transact_mut()
        .apply_update(Update::decode_v1(&update).unwrap())
        .unwrap();
    doc
}

#[test]
fn held_guard_requires_the_original_live_scope_at_capture_and_preflight() {
    let (doc, mut plan) = fixture();
    let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
    let other_doc = Doc::new();
    let foreign = CompilationReadTransaction::for_immediate_commit(other_doc.transact());
    let error =
        capture_document_guard_with_read_scope(REQUEST_ID, &owner, foreign.scope()).unwrap_err();
    assert_eq!(
        error.message.as_ref(),
        "Yrs mutation guard read scope belongs to a different document store"
    );
    plan.document_guard =
        Some(capture_document_guard_with_read_scope(REQUEST_ID, &owner, owner.scope()).unwrap());
    preflight_mutation_plan_with_read_scope(REQUEST_ID, &plan, &owner, owner.scope()).unwrap();
    assert_eq!(
        preflight_mutation_plan(REQUEST_ID, &plan, &owner)
            .unwrap_err()
            .message
            .as_ref(),
        SNAPSHOT_MISMATCH,
        "generic preflight must not silently authorize a scoped guard"
    );
    let other_scope = CompilationReadTransaction::for_immediate_commit(doc.transact());
    assert_eq!(owner.state_vector(), other_scope.state_vector());
    assert_eq!(
        preflight_mutation_plan_with_read_scope(
            REQUEST_ID,
            &plan,
            &other_scope,
            other_scope.scope()
        )
        .unwrap_err()
        .message
        .as_ref(),
        SNAPSHOT_MISMATCH,
        "the same store and state vector do not establish scope identity"
    );
    let mut over_budget = plan.clone();
    over_budget.work_limit = over_budget.compilation_work + over_budget.expected_preflight_work - 1;
    assert_eq!(
        preflight_mutation_plan_with_read_scope(
            REQUEST_ID,
            &over_budget,
            &other_scope,
            other_scope.scope()
        )
        .unwrap_err()
        .code,
        "OPERATION_LIMIT_EXCEEDED",
        "work rejection must still precede scope rejection"
    );
    let same_state = candidate(&owner);
    let candidate_txn = same_state.transact();
    let before = candidate_txn.encode_state_as_update_v1(&StateVector::default());
    assert_eq!(
        plan.clone()
            .rebind_and_preflight_equivalent_store(REQUEST_ID, &candidate_txn, other_scope.scope())
            .unwrap_err()
            .message
            .as_ref(),
        SNAPSHOT_MISMATCH,
        "candidate rebinding must authenticate the original scope before changing stores"
    );
    plan.rebind_and_preflight_equivalent_store(REQUEST_ID, &candidate_txn, owner.scope())
        .unwrap();
    assert_eq!(
        candidate_txn.encode_state_as_update_v1(&StateVector::default()),
        before
    );
}

#[test]
fn cloned_guard_expires_with_its_owner_and_releases_the_write_lock() {
    let (doc, mut plan) = fixture();
    let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
    plan.document_guard =
        Some(capture_document_guard_with_read_scope(REQUEST_ID, &owner, owner.scope()).unwrap());
    let clone = plan.clone();
    let state = owner.state_vector();
    assert!(doc.try_transact_mut().is_err());
    drop(owner);
    assert!(
        doc.try_transact_mut().is_ok(),
        "guard clones must not retain the transaction lock"
    );
    let replacement = CompilationReadTransaction::for_immediate_commit(doc.transact());
    assert_eq!(replacement.state_vector(), state);
    for escaped in [plan, clone] {
        assert_eq!(
            preflight_mutation_plan_with_read_scope(
                REQUEST_ID,
                &escaped,
                &replacement,
                replacement.scope()
            )
            .unwrap_err()
            .message
            .as_ref(),
            SNAPSHOT_MISMATCH
        );
        assert!(
            !escaped.matches_sealed_import_state(&state, &replacement, replacement.scope()),
            "an expired scope must not authenticate encoded import bytes"
        );
    }
}

#[test]
fn held_candidate_evidence_still_checks_exact_text_targets() {
    let (doc, mut plan) = fixture();
    let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
    plan.document_guard =
        Some(capture_document_guard_with_read_scope(REQUEST_ID, &owner, owner.scope()).unwrap());
    let candidate = candidate(&owner);
    let target = plan
        .actions
        .iter()
        .find_map(|action| match action {
            YrsMutationAction::InsertText { target, .. } => {
                Some(AsRef::<Branch>::as_ref(target).id())
            }
            _ => None,
        })
        .expect("the real compiled insertion has a text target");
    {
        let mut txn = candidate.transact_mut();
        let text = XmlTextRef::from(target.get_branch(&txn).unwrap());
        text.remove_range(&mut txn, 0, 1);
    }
    let txn = candidate.transact();
    assert_eq!(
        txn.state_vector(),
        owner.state_vector(),
        "deletion-only corruption preserves the state vector"
    );
    let before = txn.encode_state_as_update_v1(&StateVector::default());
    let error = plan
        .rebind_and_preflight_equivalent_store(REQUEST_ID, &txn, owner.scope())
        .unwrap_err();
    assert!(
        error
            .message
            .starts_with("resolved Yrs XML text target signature changed before mutation"),
        "{error:?}"
    );
    assert_eq!(
        txn.encode_state_as_update_v1(&StateVector::default()),
        before
    );
}

#[test]
fn held_import_seal_uses_real_delete_set_evidence() {
    let (doc, mut plan) = fixture();
    {
        let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
        plan.document_guard = Some(
            capture_document_guard_with_read_scope(REQUEST_ID, &owner, owner.scope()).unwrap(),
        );
        assert!(plan.matches_sealed_import_state(&owner.state_vector(), &owner, owner.scope()));
    }
    let state = doc.transact().state_vector();
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_xml_fragment(FRAGMENT_NAME).unwrap();
        fragment.remove_range(&mut txn, 0, 1);
    }
    let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
    assert_eq!(owner.state_vector(), state);
    plan.document_guard =
        Some(capture_document_guard_with_read_scope(REQUEST_ID, &owner, owner.scope()).unwrap());
    assert!(
        !plan.matches_sealed_import_state(&state, &owner, owner.scope()),
        "a valid held scope cannot prove that the delete set is empty"
    );
}

fn observe_ancestors(owner: &CompilationReadTransaction<'_>, plan: &YrsMutationPlan) {
    let signature = plan.actions[0].signature();
    let scope = owner.scope().unwrap();
    assert!(scope.begin_observed_path(owner, signature.path.len()));
    for (parent, index) in signature.path.iter().rev() {
        let children: Vec<_> = match parent {
            BranchID::Root(name) => owner
                .get_xml_fragment(name.clone())
                .unwrap()
                .children(owner)
                .collect(),
            BranchID::Nested(_) => {
                match XmlOut::try_from(parent.get_branch(owner).unwrap()).unwrap() {
                    XmlOut::Element(element) => element.children(owner).collect(),
                    XmlOut::Fragment(fragment) => fragment.children(owner).collect(),
                    XmlOut::Text(_) => panic!("a text node cannot be an ancestor"),
                }
            }
        };
        scope.observe_path_child(owner, parent.clone(), *index, &children);
    }
}

#[test]
fn observed_ancestor_reuse_preserves_work_and_rejects_wrong_edges_and_foreign_stores() {
    use crate::yrs_engine::observability::PREFLIGHT_CHILDREN_ENUMERATED;
    let (doc, plan) = fixture();
    let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
    observe_ancestors(&owner, &plan);
    let eager_work = preflight_mutation_plan_impl(REQUEST_ID, &plan, &owner, None).unwrap();
    PREFLIGHT_CHILDREN_ENUMERATED.set(0);
    let observed_work =
        preflight_mutation_plan_impl(REQUEST_ID, &plan, &owner, owner.scope()).unwrap();
    assert_eq!(
        observed_work, eager_work,
        "reuse must charge complete parent widths"
    );
    assert_eq!(
        PREFLIGHT_CHILDREN_ENUMERATED.get(),
        0,
        "the same immutable owner already observed every queried edge"
    );
    let mut altered = plan.clone();
    let YrsMutationAction::InsertText { signature, .. } = &mut altered.actions[0] else {
        panic!("expected insertion");
    };
    signature.path[0].1 += 1;
    let eager_error = preflight_mutation_plan_impl(REQUEST_ID, &altered, &owner, None).unwrap_err();
    let observed_error =
        preflight_mutation_plan_impl(REQUEST_ID, &altered, &owner, owner.scope()).unwrap_err();
    assert_eq!(
        format!("{observed_error:?}"),
        format!("{eager_error:?}"),
        "a different edge must retain the original failure"
    );
    let foreign = candidate(&owner);
    let foreign_owner = CompilationReadTransaction::for_immediate_commit(foreign.transact());
    let mut rebound = plan.clone();
    rebound.document_guard =
        Some(capture_document_guard_with_read_scope(REQUEST_ID, &owner, owner.scope()).unwrap());
    PREFLIGHT_CHILDREN_ENUMERATED.set(0);
    rebound
        .rebind_and_preflight_equivalent_store(REQUEST_ID, &foreign_owner, owner.scope())
        .unwrap();
    assert!(
        PREFLIGHT_CHILDREN_ENUMERATED.get() > 0,
        "candidate stores require complete traversal even with identical IDs"
    );
    assert!(!owner
        .scope()
        .unwrap()
        .begin_observed_path(&foreign_owner, 1));
    assert!(doc.try_transact_mut().is_err());
    drop(owner);
    assert!(doc.try_transact_mut().is_ok());
    let replacement = CompilationReadTransaction::for_immediate_commit(doc.transact());
    PREFLIGHT_CHILDREN_ENUMERATED.set(0);
    assert_eq!(
        preflight_mutation_plan_impl(REQUEST_ID, &plan, &replacement, replacement.scope()).unwrap(),
        eager_work
    );
    assert!(
        PREFLIGHT_CHILDREN_ENUMERATED.get() > 0,
        "a new read owner must not inherit observations"
    );
}

#[test]
fn observed_parent_then_materialized_parent_is_charged_once() {
    use crate::yrs_engine::observability::PREFLIGHT_CHILDREN_ENUMERATED;
    let source = serde_json::json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"first"}]},
        {"type":"paragraph","content":[{"type":"text","text":"second"}]}
    ]});
    let operations = [1, 8]
        .into_iter()
        .map(|offset| TypedOperation::InsertText {
            at: RevisionedPosition {
                offset,
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::After,
            },
            text: "!".into(),
            marks: vec![],
        })
        .collect();
    let (doc, _, _, compiled) = crate::yrs_engine::mutation_tests::compile_operations_with_schema(
        &source,
        operations,
        crate::schema::presets::tiptap_schema(),
    );
    let plan = compiled.mutation_plan;
    assert_eq!(plan.actions.len(), 2);
    assert_ne!(
        plan.actions[0].signature().path.last(),
        plan.actions[1].signature().path.last()
    );
    let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
    observe_ancestors(&owner, &plan);
    PREFLIGHT_CHILDREN_ENUMERATED.set(0);
    let eager = preflight_mutation_plan_impl(REQUEST_ID, &plan, &owner, None).unwrap();
    let eager_scans = PREFLIGHT_CHILDREN_ENUMERATED.get();
    PREFLIGHT_CHILDREN_ENUMERATED.set(0);
    let observed = preflight_mutation_plan_impl(REQUEST_ID, &plan, &owner, owner.scope()).unwrap();
    assert_eq!(
        observed, eager,
        "materializing the second root edge must not charge the root twice"
    );
    assert!(
        PREFLIGHT_CHILDREN_ENUMERATED.get() < eager_scans,
        "the first target must consume observed edges before the second target falls back"
    );
    assert!(
        PREFLIGHT_CHILDREN_ENUMERATED.get() > 0,
        "the other root child was not observed"
    );
    assert!(!owner
        .scope()
        .unwrap()
        .begin_observed_path(&owner, usize::MAX));
    PREFLIGHT_CHILDREN_ENUMERATED.set(0);
    assert_eq!(
        preflight_mutation_plan_impl(REQUEST_ID, &plan, &owner, owner.scope()).unwrap(),
        eager
    );
    assert_eq!(
        PREFLIGHT_CHILDREN_ENUMERATED.get(),
        eager_scans,
        "failed optional observation admission must restore full traversal"
    );
}

#[test]
fn structural_and_mixed_actions_do_not_consume_observed_ancestor_edges() {
    use crate::yrs_engine::observability::PREFLIGHT_CHILDREN_ENUMERATED;
    let source = serde_json::json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"scope"}]}]});
    let position = |offset| RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    };
    for mixed in [false, true] {
        let mut operations = Vec::new();
        if mixed {
            operations.push(TypedOperation::InsertText {
                at: position(1),
                text: "!".into(),
                marks: vec![],
            });
        }
        operations.push(TypedOperation::SplitBlock {
            at: position(3),
            node_type: "paragraph".into(),
            attrs: HashMap::new(),
        });
        let (doc, _, _, compiled) =
            crate::yrs_engine::mutation_tests::compile_operations_with_schema(
                &source,
                operations,
                crate::schema::presets::tiptap_schema(),
            );
        let plan = compiled.mutation_plan;
        assert!(plan.actions.iter().any(|action| matches!(
            action,
            YrsMutationAction::InsertXmlChildren { .. }
                | YrsMutationAction::DeleteXmlChildren { .. }
                | YrsMutationAction::CreateText { .. }
        )));
        if mixed {
            assert!(plan
                .actions
                .iter()
                .any(|action| matches!(action, YrsMutationAction::InsertText { .. })));
        }
        let owner = CompilationReadTransaction::for_immediate_commit(doc.transact());
        let root = owner.get_xml_fragment(FRAGMENT_NAME).unwrap();
        let children: Vec<_> = root.children(&owner).collect();
        let scope = owner.scope().unwrap();
        assert!(scope.begin_observed_path(&owner, 1));
        scope.observe_path_child(&owner, AsRef::<Branch>::as_ref(&root).id(), 0, &children);
        PREFLIGHT_CHILDREN_ENUMERATED.set(0);
        let eager = preflight_mutation_plan_impl(REQUEST_ID, &plan, &owner, None).unwrap();
        let eager_scans = PREFLIGHT_CHILDREN_ENUMERATED.get();
        PREFLIGHT_CHILDREN_ENUMERATED.set(0);
        let observed =
            preflight_mutation_plan_impl(REQUEST_ID, &plan, &owner, owner.scope()).unwrap();
        assert_eq!(observed, eager, "mixed={mixed}");
        assert!(eager_scans > 0);
        assert_eq!(
            PREFLIGHT_CHILDREN_ENUMERATED.get(),
            eager_scans,
            "mixed={mixed}: structural/gap consumers must materialize complete children"
        );
    }
}
