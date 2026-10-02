use super::*;
use crate::model::node::NODE_HANDLE_CLONES;
use crate::test_support::large_table_fixture::{plain_table_document, session_with_document};

fn delete_column(document: &Document) -> Transaction {
    let table = document.root().child(0).unwrap();
    let mut position = 1;
    let mut steps = Vec::new();
    for row in table.content().unwrap().iter() {
        let cell = row.child(0).unwrap();
        steps.push(Step::ReplaceRange {
            from: position + 1,
            to: position + 1 + cell.node_size(),
            content: Fragment::empty(),
        });
        position += row.node_size();
    }
    let mut transaction = Transaction::new();
    for step in steps.into_iter().rev() {
        transaction.add_step(step);
    }
    transaction
}

fn borrowed_steps(
    document: &Document,
    transaction: &Transaction,
    schema: &Schema,
) -> Result<(Document, StepMap), TransformError> {
    let mut current = document.clone();
    let mut mapping = StepMap::empty();
    for step in &transaction.steps {
        let (next, map) = apply_step(&current, step, schema)?;
        current = next;
        mapping.append(&map);
    }
    Ok((current, mapping))
}

#[test]
fn column_preview_reuses_intermediate_ancestor_children() {
    const ROWS: usize = 64;
    const COLUMNS: usize = 4;
    let session = session_with_document(&plain_table_document(ROWS, COLUMNS));
    let document = session.engine.document().unwrap();
    let schema = session.engine.schema();
    let original = document.clone();
    let transaction = delete_column(&document);
    let expected = borrowed_steps(document, &transaction, schema).unwrap();
    NODE_HANDLE_CLONES.set(0);
    let actual = transaction.apply_steps_unchecked(document, schema).unwrap();
    let copies = NODE_HANDLE_CLONES.replace(0);
    assert_eq!(actual.0, expected.0);
    assert_eq!(actual.1.ranges(), expected.1.ranges());
    assert_eq!(
        actual.0.history_snapshot_retained_bytes(),
        expected.0.history_snapshot_retained_bytes()
    );
    assert!(document.shares_root_storage_with(&original));
    assert_eq!(
        document
            .root()
            .child(0)
            .unwrap()
            .child(0)
            .unwrap()
            .child_count(),
        COLUMNS
    );
    assert!(copies <= ROWS * COLUMNS * 4,
        "column preview copied {copies} node handles for {ROWS} rows; intermediate ancestors should be reused");
}

fn spare_element(name: &str, children: Vec<Node>) -> Node {
    const SPARE_CAPACITY: usize = 32;
    const ATTRIBUTE_DEPTH: usize = 256;
    let mut node_type = String::with_capacity(name.len() + SPARE_CAPACITY);
    node_type.push_str(name);
    let mut key = String::with_capacity(SPARE_CAPACITY);
    key.push_str("metadata");
    let mut text = String::with_capacity(SPARE_CAPACITY);
    text.push_str("retained");
    let mut value = serde_json::Value::String(text);
    for _ in 0..ATTRIBUTE_DEPTH {
        let mut items = Vec::with_capacity(SPARE_CAPACITY);
        items.push(value);
        value = serde_json::Value::Array(items);
    }
    let mut attrs = HashMap::with_capacity(SPARE_CAPACITY);
    attrs.insert(key, value);
    let mut content = Vec::with_capacity(children.len() + SPARE_CAPACITY);
    content.extend(children);
    Node::element(node_type, attrs, Fragment::from(content))
}

#[test]
fn owned_replacement_matches_mixed_steps_capacities_and_failures() {
    use crate::schema::presets::tiptap_schema;
    let schema = tiptap_schema();
    let paragraph =
        |text: &str| spare_element("paragraph", vec![Node::text(text.into(), Vec::new())]);
    let document = Document::new(spare_element(
        "doc",
        vec![spare_element(
            "blockquote",
            vec![paragraph("abc"), paragraph("def")],
        )],
    ));
    let original_json = crate::boundary::StackSafeJsonValue::new(
        crate::serialize::to_prosemirror_json(&document, &schema),
    );
    let original_fee = document.history_snapshot_retained_bytes();
    let mut transaction = Transaction::new();
    let replacement = |from, to, text: &str| Step::ReplaceRange {
        from,
        to,
        content: Fragment::from(vec![Node::text(text.into(), Vec::new())]),
    };
    let steps = [
        Step::ReplaceRange {
            from: 1,
            to: 1,
            content: Fragment::from(vec![paragraph("insert")]),
        },
        replacement(2, 3, "XY"),
        Step::SplitBlock {
            pos: 3,
            node_type: "paragraph".into(),
            attrs: HashMap::new(),
        },
        replacement(2, 3, "Z"),
        Step::ReplaceRange {
            from: 1,
            to: 1,
            content: Fragment::empty(),
        },
        Step::ReplaceRange {
            from: 2,
            to: 6,
            content: Fragment::empty(),
        },
        Step::ReplaceRange {
            from: u32::MAX,
            to: u32::MAX,
            content: Fragment::empty(),
        },
    ];
    let invalid_step_index = steps.len() - 1;
    for (index, step) in steps.into_iter().enumerate() {
        transaction.add_step(step);
        let expected = borrowed_steps(&document, &transaction, &schema);
        let actual = transaction.apply_steps_unchecked(&document, &schema);
        match (expected, actual) {
            (Ok((expected, expected_map)), Ok((actual, actual_map))) => {
                assert!(
                    index < invalid_step_index,
                    "out-of-bounds step unexpectedly succeeded"
                );
                assert_eq!(actual, expected, "document after prefix {index}");
                assert_eq!(
                    actual_map.ranges(),
                    expected_map.ranges(),
                    "mapping after prefix {index}"
                );
                assert_eq!(
                    actual.history_snapshot_retained_bytes(),
                    expected.history_snapshot_retained_bytes(),
                    "history fee after prefix {index}"
                );
            }
            (Err(expected), Err(actual)) => {
                assert_eq!(
                    index, invalid_step_index,
                    "unexpected failure before deliberately invalid final step: {expected}"
                );
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
            }
            (expected, actual) => panic!("prefix {index}: borrowed={expected:?}, owned={actual:?}"),
        }
        assert_eq!(document.history_snapshot_retained_bytes(), original_fee);
        let unchanged = crate::boundary::StackSafeJsonValue::new(
            crate::serialize::to_prosemirror_json(&document, &schema),
        );
        assert_eq!(
            unchanged, original_json,
            "original document changed after prefix {index}"
        );
    }
}
