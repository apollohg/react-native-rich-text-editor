use sha2::Digest;
use std::collections::BTreeSet;
use yrs::types::xml::XmlFragment;
use yrs::undo::{Options as UndoOptions, UndoManager};
use yrs::{Doc, GetString, Options, Origin, ReadTxn, StateVector, Text, Transact, XmlTextPrelim};

fn build_and_undo() -> (Vec<u8>, String) {
    let doc = Doc::with_options(Options {
        client_id: yrs::ClientID::new(1),
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment("prosemirror");
    let origin = Origin::from("local");
    let mut tracked = std::collections::HashSet::new();
    tracked.insert(origin.clone());
    let mut manager = UndoManager::<()>::with_options(UndoOptions {
        capture_timeout_millis: 0,
        tracked_origins: tracked,
        capture_transaction: None,
        timestamp: std::sync::Arc::new(yrs::sync::time::SystemClock),
        init_undo_stack: Vec::new(),
        init_redo_stack: Vec::new(),
    });
    manager.expand_scope(&doc, &fragment);
    {
        let mut txn = doc.transact_mut_with(origin.clone());
        let first = fragment.insert(&mut txn, 0, XmlTextPrelim::new("aaa"));
        let second = fragment.insert(&mut txn, 1, XmlTextPrelim::new("bbb"));
        let _ = (first, second);
    }
    manager.reset();
    {
        let mut txn = doc.transact_mut_with(origin.clone());
        let first = fragment.get(&txn, 0).unwrap();
        let second = fragment.get(&txn, 1).unwrap();
        if let (yrs::types::xml::XmlOut::Text(first), yrs::types::xml::XmlOut::Text(second)) =
            (first, second)
        {
            first.remove_range(&mut txn, 1, 2);
            second.remove_range(&mut txn, 0, 2);
        }
    }
    assert!(manager.undo_blocking());
    let txn = doc.transact();
    (
        txn.encode_state_as_update_v1(&StateVector::default()),
        fragment.get_string(&txn),
    )
}

#[test]
fn yrs_undo_reinsertion_ids_are_deterministic() {
    let mut states = BTreeSet::new();
    let mut texts = BTreeSet::new();
    for _ in 0..40 {
        let (state, text) = build_and_undo();
        states.insert(state);
        texts.insert(text);
    }
    println!("distinct rendered strings: {texts:?}");
    println!(
        "distinct encoded states from identical input: {}",
        states.len()
    );
    assert_eq!(
        states.len(),
        1,
        "identical local histories must produce identical Yrs ids after undo"
    );
    let digest = sha2::Sha256::digest(states.iter().next().expect("one state"));
    println!("UNDO-STATE-DIGEST {digest:x}");
}
