fn validated_json_import_candidate(engine: &YrsDocumentEngine) -> CandidateDocument {
    let value = serde_json::json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{"type": "text", "text": "abc"}]
        }]
    });
    let document =
        from_prosemirror_json(&value, &engine.schema, UnknownTypeMode::Preserve).unwrap();
    let source = ValidatedImportDocument::new(
        document,
        &engine.schema,
        &engine.canonical_schema,
        &engine.resource_limits,
        Some(serde_json::to_vec(&value).unwrap().len()),
    )
    .unwrap();
    engine
        .build_candidate_from_document(source, TransactionOrigin::DocumentImport)
        .unwrap()
}

fn equal_clock_divergent_valid_update(
    engine: &YrsDocumentEngine,
    candidate: &CandidateDocument,
) -> Vec<u8> {
    let divergent = equivalent_private_candidate_doc(&candidate.doc);
    let empty_json = json!({
        "type": engine.schema.doc_node_type(),
        "content": [],
    });
    let divergent_json = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{"type": "text", "text": "xyz"}]
        }]
    });
    let codec = super::YrsDocumentCodec::new(&engine.schema, &engine.resource_limits);
    {
        let mut txn =
            divergent.transact_mut_with(TransactionOrigin::DocumentImport.as_yrs_origin());
        let fragment = txn.get_or_insert_xml_fragment(engine.fragment_name.as_str());
        codec
            .apply_json(&fragment, &mut txn, &empty_json, &divergent_json)
            .unwrap();
    }
    let candidate_txn = candidate.doc.transact();
    let divergent_txn = divergent.transact();
    assert_eq!(
        divergent_txn.state_vector(),
        candidate_txn.state_vector(),
        "the tamper must keep identical client clocks"
    );
    let candidate_encoded = candidate_txn.encode_state_as_update_v1(&StateVector::default());
    let divergent_encoded = divergent_txn.encode_state_as_update_v1(&StateVector::default());
    assert_ne!(
        divergent_encoded, candidate_encoded,
        "the tamper must carry different valid content"
    );
    divergent_encoded
}

#[test]
fn tampered_import_receipt_falls_back_to_one_full_lookup_build() {
    for case in [
        "bytes",
        "sha256",
        "stateVector",
        "fragment",
        "clientId",
        "guid",
        "offsetKind",
        "skipGc",
        "deleteSetEligibility",
        "lookupSourceDocument",
        "lookupCanonicalArtifact",
        "lookupResourceLimits",
        "lookupEditingLimits",
        "lookupMaxLength",
        "lookupSchemaToken",
        "lookupStoreToken",
    ] {
        let mut engine = transaction_engine();
        let mut candidate = validated_json_import_candidate(&engine);
        let installed = engine.derived_state.as_ref().unwrap();
        let foreign_document = installed.document.clone();
        let foreign_artifact = installed.canonical_artifact.clone();
        let receipt = candidate
            .import_encoded_state_receipt
            .as_mut()
            .expect("validated JSON candidates carry one private encoded-state receipt");
        match case {
            "bytes" => receipt.encoded_state = vec![0xff],
            "sha256" => receipt.encoded_state_sha256[0] ^= 1,
            "stateVector" => receipt.state_vector = StateVector::default(),
            "fragment" => receipt.fragment_id = BranchID::Root(Arc::from("foreign")),
            "clientId" => receipt.client_id = ClientID::new(receipt.client_id.get() ^ 1),
            "guid" => receipt.guid = Arc::from("foreign-guid"),
            "offsetKind" => receipt.offset_kind = OffsetKind::Bytes,
            "skipGc" => receipt.skip_gc = !receipt.skip_gc,
            "deleteSetEligibility" => receipt.delete_set_is_empty = !receipt.delete_set_is_empty,
            "lookupSourceDocument" => {
                receipt
                    .lookup_materialization
                    .as_mut()
                    .unwrap()
                    .source_document = foreign_document
            }
            "lookupCanonicalArtifact" => {
                receipt
                    .lookup_materialization
                    .as_mut()
                    .unwrap()
                    .canonical_artifact = foreign_artifact
            }
            "lookupResourceLimits" => {
                receipt
                    .lookup_materialization
                    .as_mut()
                    .unwrap()
                    .resource_limits
                    .max_document_nodes ^= 1
            }
            "lookupEditingLimits" => {
                receipt
                    .lookup_materialization
                    .as_mut()
                    .unwrap()
                    .editing_limits
                    .max_operations_per_transaction ^= 1
            }
            "lookupMaxLength" => {
                receipt.lookup_materialization.as_mut().unwrap().max_length = Some(1)
            }
            "lookupSchemaToken" => {
                receipt
                    .lookup_materialization
                    .as_mut()
                    .unwrap()
                    .schema_token ^= 1
            }
            "lookupStoreToken" => receipt.lookup_materialization.as_mut().unwrap().store_token ^= 1,
            _ => unreachable!(),
        }
        crate::yrs_engine::mutation::reset_localized_lookup_counts_for_test();

        engine
            .commit_candidate(candidate, TransactionOrigin::DocumentImport)
            .unwrap();

        assert_eq!(
            crate::yrs_engine::mutation::take_localized_lookup_counts_for_test().0,
            1,
            "{case}: a tampered receipt must fall back to one full lookup build"
        );
        assert_imported_lookup_seed_is_ready_without_replica(&engine, case);
    }
}

#[test]
fn equal_clock_divergent_valid_receipt_bytes_fall_back_to_one_full_lookup_build() {
    let mut engine = transaction_engine();
    let mut candidate = validated_json_import_candidate(&engine);
    let divergent_encoded = equal_clock_divergent_valid_update(&engine, &candidate);
    candidate
        .import_encoded_state_receipt
        .as_mut()
        .unwrap()
        .encoded_state = divergent_encoded;
    crate::yrs_engine::mutation::reset_localized_lookup_counts_for_test();

    engine
        .commit_candidate(candidate, TransactionOrigin::DocumentImport)
        .unwrap();

    assert_eq!(
        crate::yrs_engine::mutation::take_localized_lookup_counts_for_test().0,
        1,
        "divergent receipt bytes must not vouch for the fused lookup"
    );
    assert_imported_lookup_seed_is_ready_without_replica(&engine, "divergent bytes");
}
