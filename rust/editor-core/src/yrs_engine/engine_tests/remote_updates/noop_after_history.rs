use super::*;
use crate::tables::commands::{TableCommand, TableEdge};
use yrs::{GetString, Text, Xml};

fn apply(doc: &Doc, bytes: &[u8]) {
    doc.transact_mut_with(TransactionOrigin::RemoteSync.as_yrs_origin())
        .apply_update(Update::decode_v1(bytes).unwrap())
        .unwrap();
}

fn full(doc: &Doc) -> Vec<u8> {
    doc.transact()
        .encode_state_as_update_v1(&StateVector::default())
}

fn roundtrip(bytes: &[u8]) -> Doc {
    let doc = utf16_doc();
    apply(&doc, bytes);
    doc
}

fn crossing_history() -> YrsDocumentEngine {
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema: crate::schema::presets::prosemirror_table_schema(),
        fragment_name: "prosemirror".into(),
        initialization_mode: crate::yrs_engine::InitializationMode::LocalEmpty,
        resource_limits: ResourceLimits::default(),
        editing_limits: crate::yrs_engine::EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap();
    let cell = |text: &str, rowspan: u32| {
        json!({
            "type": "table_cell", "attrs": {"colspan": 1, "rowspan": rowspan},
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}],
        })
    };
    engine
        .import_json(
            &json!({"type": "doc", "content": [{"type": "table", "content": [
                {"type": "table_row", "content": [cell("a", 2), cell("b", 1)]},
                {"type": "table_row", "content": [cell("c", 1)]},
            ]}]})
            .to_string(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let document = engine.document().unwrap();
    let projected = crate::tables::projection::project_table(
        document.node_at(&[0]).unwrap(),
        0,
        &engine.schema,
        &mut crate::tables::projection::TableGridBudget::new(
            ResourceLimits::default().max_table_grid_slots,
        ),
    )
    .unwrap();
    let interior = crate::tables::interchange::first_editable_position_in_cell(
        document,
        &engine.schema,
        projected.cells[1].source_pos,
    )
    .unwrap()
    .unwrap();
    let anchor = RevisionedPosition {
        offset: engine
            .position_map()
            .unwrap()
            .doc_to_scalar(interior, document),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: 800,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Cell {
                anchor: anchor.clone(),
                head: anchor,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    assert!(engine
        .apply_command(
            801,
            TypedCommand::Table(TableCommand::AddTableRow {
                side: TableEdge::After
            })
        )
        .unwrap()
        .is_some());
    assert!(engine.undo(802).unwrap().is_some());
    assert!(engine.redo(803).unwrap().is_some());
    assert_eq!(
        engine.document_json().unwrap()["content"][0]["content"][0]["content"][0]["attrs"]
            ["rowspan"]
            .as_f64(),
        Some(3.0)
    );
    assert_ne!(
        engine.encoded_state().unwrap(),
        full(&roundtrip(&engine.encoded_state().unwrap())),
        "fixture must retain history fragmentation"
    );
    engine
}

#[test]
fn empty_and_fullstate_after_redo_are_noops_on_both_remote_surfaces() {
    for prepared in [false, true] {
        let mut engine = crossing_history();
        let state = engine.encoded_state().unwrap();
        let peer_state = full(&roundtrip(&state));
        for bytes in [
            &[0, 0][..],
            &[0, 0][..],
            state.as_slice(),
            peer_state.as_slice(),
        ] {
            let before = atomic_audit(&engine);
            reset_import_state_encoding_counts_for_test();
            let commit = if prepared {
                let stage = engine.prepare_remote_update_v1(804, bytes).unwrap();
                assert_eq!(atomic_audit(&engine), before);
                engine.commit_prepared_remote_update(stage).unwrap()
            } else {
                engine.apply_remote_update_v1(804, bytes).unwrap()
            };
            assert!(
                !commit.changed,
                "redo history must not make a duplicate update change the document"
            );
            assert_eq!(take_import_state_encoding_counts_for_test(), (2, 0));
            assert_eq!(atomic_audit(&engine), before);
        }
        let redone = engine.document_json();
        assert!(engine.undo(805).unwrap().is_some());
        assert_eq!(
            engine.document_json().unwrap()["content"][0]["content"][0]["content"][0]["attrs"]
                ["rowspan"]
                .as_f64(),
            Some(2.0)
        );
        assert!(engine.redo(806).unwrap().is_some());
        assert_eq!(engine.document_json(), redone);
    }
}

fn first_cell<T: ReadTxn>(txn: &T) -> yrs::XmlElementRef {
    let root = txn.get_xml_fragment("prosemirror").unwrap();
    let table = root.get(txn, 0).unwrap().into_xml_element().unwrap();
    let row = table.get(txn, 0).unwrap().into_xml_element().unwrap();
    row.get(txn, 0).unwrap().into_xml_element().unwrap()
}

#[test]
fn real_remote_changes_after_history_are_not_hidden_by_equal_json_or_sv() {
    for split in [false, true] {
        for case in ["text", "attribute", "delete", "invisible"] {
            let mut engine = crossing_history();
            let source = roundtrip(&engine.encoded_state().unwrap());
            let before_sv = source.transact().state_vector();
            let before_json = engine.document_json();
            {
                let mut txn = source.transact_mut();
                let cell = first_cell(&txn);
                let paragraph = cell.get(&txn, 0).unwrap().into_xml_element().unwrap();
                let text = paragraph.get(&txn, 0).unwrap().into_xml_text().unwrap();
                match case {
                    "text" => text.insert(&mut txn, 0, "remote"),
                    "attribute" => {
                        cell.insert_attribute(
                            &mut txn,
                            "colwidth",
                            yrs::Any::Array(vec![yrs::Any::Number(120.0)].into()),
                        );
                    }
                    "delete" => text.remove_range(&mut txn, 0, 1),
                    "invisible" => {
                        text.insert(&mut txn, 0, "hidden");
                        text.remove_range(&mut txn, 0, 6);
                    }
                    _ => unreachable!(),
                }
            }
            let delta = source.transact().encode_state_as_update_v1(&before_sv);
            if case == "delete" {
                assert_eq!(source.transact().state_vector(), before_sv);
            }
            let before = atomic_audit(&engine);
            let commit = if split {
                let stage = engine.prepare_remote_update_v1(810, &delta).unwrap();
                assert_eq!(atomic_audit(&engine), before);
                engine.commit_prepared_remote_update(stage).unwrap()
            } else {
                engine.apply_remote_update_v1(810, &delta).unwrap()
            };
            assert!(commit.changed, "{case}");
            assert_eq!(commit.revision, before.revision + 1);
            assert_eq!(
                full(&roundtrip(&engine.encoded_state().unwrap())),
                full(&roundtrip(&full(&source)))
            );
            if case == "invisible" {
                assert_eq!(engine.document_json(), before_json);
            }
            let accepted = atomic_audit(&engine);
            assert!(!engine.apply_remote_update_v1(811, &delta).unwrap().changed);
            assert_eq!(atomic_audit(&engine), accepted);
            assert!(engine.undo(812).unwrap().is_some());
            assert!(engine.redo(813).unwrap().is_some());
        }
    }
}

fn candidate_pair(seed: &[u8], incoming: &[u8]) -> (Doc, Doc) {
    let joined = utf16_doc();
    {
        let mut txn = joined.transact_mut_with(TransactionOrigin::RemoteSync.as_yrs_origin());
        txn.apply_update(Update::decode_v1(seed).unwrap()).unwrap();
        txn.apply_update(Update::decode_v1(incoming).unwrap())
            .unwrap();
    }
    let split = roundtrip(seed);
    apply(&split, incoming);
    (joined, split)
}

fn assert_candidate_equivalence(left: &Doc, right: &Doc) {
    let a = full(left);
    let b = full(right);
    assert_eq!(
        left.transact().state_vector(),
        right.transact().state_vector()
    );
    assert_eq!(
        left.transact().has_missing_updates(),
        right.transact().has_missing_updates()
    );
    assert_eq!(
        Update::decode_v1(&a).unwrap().delete_set(),
        Update::decode_v1(&b).unwrap().delete_set()
    );
    assert_eq!(full(&roundtrip(&a)), full(&roundtrip(&b)));
    for doc in [left, right] {
        assert_eq!(
            doc.transact().state_vector().get(&doc.client_id()),
            0,
            "candidate must not author IDs"
        );
    }
}

#[test]
fn seed_commit_preserves_pending_delete_and_clock_hole_integration() {
    let source = utf16_doc();
    let text = source.get_or_insert_text("independent");
    text.insert(&mut source.transact_mut(), 0, "a");
    let first = full(&source);
    let first_sv = source.transact().state_vector();
    text.insert(&mut source.transact_mut(), 1, "b");
    let second = source.transact().encode_state_as_update_v1(&first_sv);
    let both = full(&source);
    let both_sv = source.transact().state_vector();
    text.remove_range(&mut source.transact_mut(), 0, 2);
    let deletion = source.transact().encode_state_as_update_v1(&both_sv);
    let deleted = full(&source);
    let history = crossing_history().encoded_state().unwrap();
    for (seed, incoming, completion, pending) in [
        (&first, &second, &both, false),
        (&both, &both, &both, false),
        (&both, &deletion, &deleted, false),
        (&vec![0, 0], &deleted, &deleted, false),
        (&second, &first, &both, false),
        (&vec![0, 0], &second, &first, true),
        (&vec![0, 0], &deletion, &both, true),
        (&deletion, &both, &both, false),
        (&history, &vec![0, 0], &history, false),
    ] {
        let (joined, split) = candidate_pair(seed, incoming);
        assert_candidate_equivalence(&joined, &split);
        assert_eq!(split.transact().has_missing_updates(), pending);
        apply(&joined, completion);
        apply(&split, completion);
        assert_candidate_equivalence(&joined, &split);
        assert!(!split.transact().has_missing_updates());
        let a = joined
            .get_or_insert_text("independent")
            .get_string(&joined.transact());
        let b = split
            .get_or_insert_text("independent")
            .get_string(&split.transact());
        assert_eq!(a, b);
    }

    use base64::Engine;
    // Same stock-Yjs fixture as the publication regressions: client 42 has a clock-3 hole.
    let [base, gap, suffix] = [
        "AQMqAAcBC3Byb3NlbWlycm9yAwlwYXJhZ3JhcGgHACoABgQAKgEBYQA=",
        "AQEqAygBC2luZGVwZW5kZW50AXgBfQEA",
        "AQEqBEQqAg1jb250aW51YXRpb24tAA==",
    ]
    .map(|s| base64::prelude::BASE64_STANDARD.decode(s).unwrap());
    let (joined, split) = candidate_pair(&base, &suffix);
    assert_candidate_equivalence(&joined, &split);
    assert!(!split.transact().has_missing_updates());
    assert_eq!(
        split.transact().state_vector(),
        roundtrip(&base).transact().state_vector()
    );
    assert_ne!(full(&split), base);
    apply(&joined, &gap);
    apply(&split, &gap);
    assert_candidate_equivalence(&joined, &split);
}

#[test]
fn history_noop_stages_preserve_quarantine_and_reject_stale_seals() {
    let mut engine = crossing_history();
    let source = utf16_doc();
    let text = source.get_or_insert_text("dependency");
    text.insert(&mut source.transact_mut(), 0, "a");
    let first = full(&source);
    let sv = source.transact().state_vector();
    text.insert(&mut source.transact_mut(), 1, "b");
    let second = source.transact().encode_state_as_update_v1(&sv);
    let before = atomic_audit(&engine);
    let stale = engine.prepare_remote_update_v1(820, &[0, 0]).unwrap();
    let dropped = engine.prepare_remote_update_v1(821, &[0, 0]).unwrap();
    drop(dropped);
    assert_eq!(atomic_audit(&engine), before);
    assert!(!engine.apply_remote_update_v1(822, &second).unwrap().changed);
    assert_eq!(atomic_audit(&engine), before);
    let retained = engine.quarantined_remote_update.clone();
    assert!(engine.commit_prepared_remote_update(stale).is_err());
    assert_eq!(engine.quarantined_remote_update, retained);
    for incoming in [&[0, 0][..], second.as_slice()] {
        let stage = engine.prepare_remote_update_v1(823, incoming).unwrap();
        assert!(stage.has_pending_dependencies());
        assert_eq!(engine.quarantined_remote_update, retained);
        assert!(!engine.commit_prepared_remote_update(stage).unwrap().changed);
        assert_eq!(atomic_audit(&engine), before);
    }
    let complete = engine.prepare_remote_update_v1(824, &first).unwrap();
    assert!(!complete.has_pending_dependencies());
    assert_eq!(atomic_audit(&engine), before);
    assert!(
        engine
            .commit_prepared_remote_update(complete)
            .unwrap()
            .changed
    );
    assert_eq!(engine.pending_remote_dependency_bytes(), 0);
    assert_eq!(
        engine
            .doc
            .get_or_insert_text("dependency")
            .get_string(&engine.doc.transact()),
        "ab"
    );
    let completed = atomic_audit(&engine);
    assert!(
        !engine
            .apply_remote_update_v1(825, &full(&source))
            .unwrap()
            .changed
    );
    assert_eq!(atomic_audit(&engine), completed);
    assert!(engine.undo(826).unwrap().is_some());
    assert!(engine.redo(827).unwrap().is_some());
}

#[test]
fn resolved_quarantine_can_clear_without_changing_history_state() {
    let mut engine = crossing_history();
    engine.quarantined_remote_update = Some(engine.encoded_state().unwrap());
    let before = atomic_audit(&engine);
    let stale = engine.prepare_remote_update_v1(830, &[0, 0]).unwrap();
    let clear = engine.prepare_remote_update_v1(831, &[0, 0]).unwrap();
    assert!(!clear.has_pending_dependencies());
    assert!(engine.pending_remote_dependency_bytes() > 0);
    assert!(!engine.commit_prepared_remote_update(clear).unwrap().changed);
    assert_eq!(engine.pending_remote_dependency_bytes(), 0);
    assert_eq!(atomic_audit(&engine), before);
    assert!(engine.commit_prepared_remote_update(stale).is_err());
    assert_eq!(atomic_audit(&engine), before);
}

#[test]
fn malformed_and_candidate_limit_rejections_preserve_history_state() {
    let mut engine = crossing_history();
    let before = atomic_audit(&engine);
    assert!(engine.prepare_remote_update_v1(840, &[255]).is_err());
    assert_eq!(atomic_audit(&engine), before);
    let source = roundtrip(&engine.encoded_state().unwrap());
    let sv = source.transact().state_vector();
    {
        let mut txn = source.transact_mut();
        let cell = first_cell(&txn);
        let paragraph = cell.get(&txn, 0).unwrap().into_xml_element().unwrap();
        let text = paragraph.get(&txn, 0).unwrap().into_xml_text().unwrap();
        text.insert(&mut txn, 0, &"x".repeat(128));
    }
    let delta = source.transact().encode_state_as_update_v1(&sv);
    engine.resource_limits.max_encoded_state_bytes = full(&source).len() - 1;
    assert!(
        engine.encoded_state().unwrap().len() <= engine.resource_limits.max_encoded_state_bytes
    );
    assert!(delta.len() <= engine.resource_limits.max_encoded_state_bytes);
    let error = engine.prepare_remote_update_v1(841, &delta).err().unwrap();
    assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
    assert_eq!(atomic_audit(&engine), before);
    assert_eq!(engine.pending_remote_dependency_bytes(), 0);
}
