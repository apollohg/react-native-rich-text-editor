const REPLAY_DETERMINISM_RUNS: usize = 64;
const REPLAY_CONVERGENCE_RUNS: usize = 32;

fn imported_pair() -> (YrsDocumentEngine, YrsDocumentEngine) {
    let mut local = engine(InitializationMode::LocalEmpty);
    local
        .import_json(
            r#"{"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"aaa"}]},
                {"type":"paragraph","content":[{"type":"text","text":"bbb"}]}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let mut remote = engine(InitializationMode::AwaitRemote);
    remote
        .apply_remote_update_v1(200, &local.encoded_state().unwrap())
        .unwrap();
    (local, remote)
}

fn delete_across_paragraph_boundary(local: &mut YrsDocumentEngine, request_id: u64, to: u32) {
    local
        .apply_command(
            request_id,
            TypedCommand::DeleteRange {
                range: RevisionedRange {
                    from: point(1),
                    to: point(to),
                },
            },
        )
        .unwrap()
        .expect("delete must apply");
}

fn drain_outbox_into(
    outbox: &mut CollaborationOutbox,
    peer: &mut YrsDocumentEngine,
    request_id: u64,
) {
    while let Some(leased) = outbox.lease_next().unwrap() {
        if let crate::collaboration_runtime::outbox::OutboundLeasePayload::DocumentUpdate(bytes) =
            &leased.payload
        {
            peer.apply_remote_update_v1(request_id, bytes).unwrap();
        }
        outbox.ack_lease(leased.lease_id).unwrap();
    }
}

#[test]
fn redo_after_a_remote_insert_into_a_restored_paragraph_is_deterministic() {
    let mut outcomes = std::collections::BTreeMap::<String, usize>::new();
    for _ in 0..REPLAY_DETERMINISM_RUNS {
        let (mut local, mut remote) = imported_pair();
        delete_across_paragraph_boundary(&mut local, 201, 6);
        local.undo(202).unwrap().expect("undo must apply");

        remote
            .apply_remote_update_v1(203, &local.encoded_state().unwrap())
            .unwrap();
        select_text(&mut remote, 204, 6, 6);
        remote
            .apply_command(205, TypedCommand::InsertText { text: "R".into() })
            .unwrap()
            .unwrap();
        local
            .apply_remote_update_v1(206, &remote.encoded_state().unwrap())
            .unwrap();
        assert_eq!(
            local.document().unwrap().root().text_content(),
            "aaabbRb",
            "the peer's character lands in the restored second paragraph",
        );

        let redone = local.redo(207).unwrap();
        let after = match redone {
            Some(_) => local.document_json().unwrap().to_string(),
            None => format!("NOCHANGE {}", local.document_json().unwrap()),
        };
        *outcomes.entry(after).or_default() += 1;
    }
    println!("POST-REDO DOCUMENTS ({REPLAY_DETERMINISM_RUNS} runs):");
    for (document, count) in &outcomes {
        println!("  {count:>4}  {document}");
    }
    assert_eq!(
        outcomes.len(),
        1,
        "replayed redo must reproduce one document across runs",
    );
}

#[test]
fn a_second_undo_after_a_remote_insert_into_a_restored_paragraph_is_deterministic() {
    let mut outcomes = std::collections::BTreeMap::<String, usize>::new();
    for _ in 0..REPLAY_DETERMINISM_RUNS {
        let (mut local, mut remote) = imported_pair();
        local
            .apply_command(201, TypedCommand::InsertText { text: "Z".into() })
            .unwrap()
            .unwrap();
        delete_across_paragraph_boundary(&mut local, 202, 7);
        local.undo(203).unwrap().expect("undo must apply");
        remote
            .apply_remote_update_v1(204, &local.encoded_state().unwrap())
            .unwrap();
        select_text(&mut remote, 205, 7, 7);
        remote
            .apply_command(206, TypedCommand::InsertText { text: "R".into() })
            .unwrap()
            .unwrap();
        local
            .apply_remote_update_v1(207, &remote.encoded_state().unwrap())
            .unwrap();

        let before = local.document_json().unwrap().to_string();
        let second = local.undo(208).unwrap();
        let after = match second {
            Some(_) => local.document_json().unwrap().to_string(),
            None => format!("NOCHANGE {}", local.document_json().unwrap()),
        };
        *outcomes.entry(format!("{before} -> {after}")).or_default() += 1;
    }
    println!("SECOND-UNDO OUTCOMES ({REPLAY_DETERMINISM_RUNS} runs):");
    for (line, count) in &outcomes {
        println!("  {count:>4}  {line}");
    }
    assert_eq!(
        outcomes.len(),
        1,
        "a second replayed undo must reproduce one document across runs",
    );
}

#[test]
fn undo_then_redo_with_a_peer_converges_after_bidirectional_synchronization() {
    let mut outcomes = std::collections::BTreeMap::<String, usize>::new();
    for _ in 0..REPLAY_CONVERGENCE_RUNS {
        let (mut local, mut remote) = imported_pair();
        delete_across_paragraph_boundary(&mut local, 201, 6);

        let mut outbox = CollaborationOutbox::with_ceilings(64, 1 << 20);
        local.undo_with_outbox(202, Some(&mut outbox)).unwrap();
        drain_outbox_into(&mut outbox, &mut remote, 203);
        remote
            .apply_remote_update_v1(204, &local.encoded_state().unwrap())
            .unwrap();

        select_text(&mut remote, 205, 6, 6);
        remote
            .apply_command(206, TypedCommand::InsertText { text: "R".into() })
            .unwrap()
            .unwrap();
        local
            .apply_remote_update_v1(207, &remote.encoded_state().unwrap())
            .unwrap();

        local.redo_with_outbox(208, Some(&mut outbox)).unwrap();
        drain_outbox_into(&mut outbox, &mut remote, 209);
        remote
            .apply_remote_update_v1(210, &local.encoded_state().unwrap())
            .unwrap();
        local
            .apply_remote_update_v1(211, &remote.encoded_state().unwrap())
            .unwrap();

        let local_document = local.document_json().unwrap().to_string();
        let remote_document = remote.document_json().unwrap().to_string();
        let verdict = if local_document == remote_document {
            "CONVERGED"
        } else {
            "DIVERGED"
        };
        *outcomes
            .entry(format!(
                "{verdict}\n        local ={local_document}\n        remote={remote_document}"
            ))
            .or_default() += 1;
    }
    println!("CONVERGENCE OUTCOMES ({REPLAY_CONVERGENCE_RUNS} runs):");
    for (line, count) in &outcomes {
        println!("  {count:>4}  {line}");
    }
    assert!(
        outcomes.keys().all(|line| line.starts_with("CONVERGED")),
        "every replica pair must converge after an undo and a redo",
    );
    assert_eq!(
        outcomes.len(),
        1,
        "convergence must settle on one document across runs",
    );
}

#[test]
fn repeated_undo_and_redo_over_multiple_restored_blocks_replays_faithfully() {
    let mut local = engine(InitializationMode::LocalEmpty);
    local
        .import_json(
            r#"{"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"aaa"}]},
                {"type":"paragraph","content":[{"type":"text","text":"bbb"}]},
                {"type":"paragraph","content":[{"type":"text","text":"ccc"}]},
                {"type":"paragraph","content":[{"type":"text","text":"ddd"}]}]}"#,
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    delete_across_paragraph_boundary(&mut local, 301, 13);
    let deleted_document = local.document_json().unwrap();
    assert_eq!(local.document().unwrap().root().text_content(), "add");

    local.undo(302).unwrap().expect("undo must apply");
    let restored_document = local.document_json().unwrap();
    assert_eq!(
        local.document().unwrap().root().text_content(),
        "aaabbbcccddd",
        "the undo independently restores three deleted blocks",
    );

    for round in 0..16u64 {
        local
            .redo(400 + round * 2)
            .unwrap_or_else(|error| panic!("redo round {round} must replay: {}", error.message))
            .unwrap_or_else(|| panic!("redo round {round} must change the document"));
        assert_eq!(
            local.document_json().unwrap(),
            deleted_document,
            "redo round {round} must reproduce the deleted document",
        );

        local
            .undo(401 + round * 2)
            .unwrap_or_else(|error| panic!("undo round {round} must replay: {}", error.message))
            .unwrap_or_else(|| panic!("undo round {round} must change the document"));
        assert_eq!(
            local.document_json().unwrap(),
            restored_document,
            "undo round {round} must reproduce the restored document",
        );
    }
}
