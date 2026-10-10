use super::*;
use crate::model::{Fragment, Node};
use crate::yrs_engine::observability::{
    reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
};

const ENCODED_BOUND_SEEDED_STEPS: usize = 500;
const FIXTURE_REPETITIONS: usize = 1024;
const FIXTURE_CLIENT_ID: u64 = 991;
const RANDOM_SEED: u64 = 0x7ab1e;
const RANDOM_MULTIPLIER: u64 = 6364136223846793005;
const RANDOM_INCREMENT: u64 = 1;

fn encoded_bound_engine() -> YrsDocumentEngine {
    let mut engine = transaction_engine();
    let value = json!({"type":"doc","content":[{"type":"paragraph","content":[{
        "type":"text","text":"ab🦀cd".repeat(FIXTURE_REPETITIONS)
    }]}]});
    let document =
        from_prosemirror_json(&value, &engine.schema, UnknownTypeMode::Preserve).unwrap();
    let source = ValidatedImportDocument::new(
        document,
        &engine.schema,
        &engine.canonical_schema,
        &engine.resource_limits,
        Some(value.to_string().len()),
    )
    .unwrap();
    let doc = Doc::with_options(Options {
        client_id: ClientID::new(FIXTURE_CLIENT_ID),
        offset_kind: OffsetKind::Utf16,
        skip_gc: true,
        ..Options::default()
    });
    let candidate = engine
        .build_candidate_from_document_in_doc(source, TransactionOrigin::DocumentImport, doc)
        .unwrap();
    engine
        .commit_candidate(candidate, TransactionOrigin::DocumentImport)
        .unwrap();
    engine
}

fn point(offset: u32) -> RevisionedPosition {
    RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    }
}

#[test]
fn the_encoded_bound_never_under_counts() {
    let mut engine = encoded_bound_engine();
    let mut random = RANDOM_SEED;
    let mut text: Vec<char> = "ab🦀cd".repeat(FIXTURE_REPETITIONS).chars().collect();
    for step in 0..ENCODED_BOUND_SEEDED_STEPS {
        random = random
            .wrapping_mul(RANDOM_MULTIPLIER)
            .wrapping_add(RANDOM_INCREMENT);
        let start = random as usize % (text.len() - 1) + 1;
        let end = (start + (random >> 32) as usize % 3 + 1).min(text.len());
        let range = RevisionedRange {
            from: point(start as u32),
            to: point(end as u32),
        };
        let replacement = if step % 3 == 2 { "" } else { "x🦀" };
        let operation = match step % 3 {
            0 => TypedOperation::InsertText {
                at: point(start as u32),
                text: replacement.into(),
                marks: vec![],
            },
            1 => TypedOperation::ReplaceRange {
                range,
                content: Fragment::from(vec![Node::text(replacement.into(), vec![])]),
            },
            _ => TypedOperation::DeleteRange { range },
        };
        let mut transaction = insert_transaction(&engine, step as u64);
        transaction.operations = vec![operation];
        engine
            .apply_typed_transaction(transaction)
            .unwrap_or_else(|error| panic!("step {step} at {start}..{end}: {error:?}"));
        text.splice(
            start..if step % 3 == 0 { start } else { end },
            replacement.chars(),
        );
        let exact = engine.encoded_state().unwrap().len();
        assert!(
            exact <= engine.encoded_state_upper_bound,
            "step {step}: exact {exact}, bound {}",
            engine.encoded_state_upper_bound
        );
        assert_eq!(
            engine.document_json().unwrap()["content"][0]["content"][0]["text"],
            text.iter().collect::<String>(),
            "step {step}"
        );
        let replay_doc = engine.new_history_candidate_doc();
        engine
            .history
            .seed_candidate(step as u64, &replay_doc)
            .unwrap();
        let fragment = replay_doc.get_or_insert_xml_fragment(engine.fragment_name.as_str());
        let replay_history = engine
            .history
            .replay_into(step as u64, &replay_doc, &fragment)
            .unwrap_or_else(|error| panic!("history replay at step {step}: {error:?}"));
        assert_eq!(
            replay_doc
                .transact()
                .encode_state_as_update_v1(&StateVector::default()),
            engine.encoded_state().unwrap(),
            "history delta parity at step {step}"
        );
        drop(replay_history);
    }
}

#[test]
fn the_encoded_size_limit_decision_equals_the_exact_encoding_at_the_boundary() {
    let mut probe = encoded_bound_engine();
    let transaction = insert_transaction(&probe, 1);
    probe.apply_typed_transaction(transaction).unwrap();
    let exact = probe.encoded_state().unwrap().len();
    for limit in [exact - 1, exact, exact + 1] {
        let mut engine = encoded_bound_engine();
        engine.resource_limits.max_encoded_state_bytes = limit;
        let before = atomic_audit(&engine);
        let bound_before = engine.encoded_state_upper_bound;
        let transaction = insert_transaction(&engine, 1);
        reset_full_pass_counts_for_test();
        let result = engine.apply_typed_transaction(transaction);
        assert_eq!(
            take_full_pass_counts_for_test().whole_state_encodings,
            1,
            "limit {limit}: exact candidate admission is measured"
        );
        if limit < exact {
            let error = result.expect_err("one byte under exact must reject");
            assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
            assert_eq!(atomic_audit(&engine), before);
            assert_eq!(engine.encoded_state_upper_bound, bound_before);
        } else {
            result.unwrap_or_else(|error| panic!("limit {limit}, exact {exact}: {error:?}"));
            assert_eq!(engine.encoded_state().unwrap().len(), exact);
            assert_eq!(
                engine.encoded_state_upper_bound, exact,
                "near-boundary admission resets to exact size"
            );
        }
    }
}
