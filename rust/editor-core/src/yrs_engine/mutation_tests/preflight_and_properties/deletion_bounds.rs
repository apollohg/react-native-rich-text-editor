#[derive(Debug, Clone)]
enum BoundedEdit {
    Delete { from: u32, len: u32 },
    SelectAllDelete,
    Format { from: u32, len: u32, add: bool },
}

const BOUNDED_EDIT_MAX_LEN: u32 = 24;
const BOUNDED_EDIT_REQUEST: u64 = 73_000;
const REMOTE_EDITING_CLIENT: u64 = 7_001;

fn bounded_edit_strategy() -> impl Strategy<Value = BoundedEdit> {
    prop_oneof![
        3 => (any::<u32>(), 1..BOUNDED_EDIT_MAX_LEN)
            .prop_map(|(from, len)| BoundedEdit::Delete { from, len }),
        1 => Just(BoundedEdit::SelectAllDelete),
        3 => (any::<u32>(), 1..BOUNDED_EDIT_MAX_LEN, any::<bool>())
            .prop_map(|(from, len, add)| BoundedEdit::Format { from, len, add }),
    ]
}

fn bounded_operation(edit: &BoundedEdit, total_scalars: u32) -> TypedOperation {
    let clamp = |from: u32, len: u32| {
        let from = from % (total_scalars + 1);
        (from, from.saturating_add(len).min(total_scalars))
    };
    match edit {
        BoundedEdit::Delete { from, len } => {
            let (from, to) = clamp(*from, *len);
            TypedOperation::DeleteRange {
                range: range_for_test(from, to),
            }
        }
        BoundedEdit::SelectAllDelete => TypedOperation::DeleteRange {
            range: range_for_test(0, total_scalars),
        },
        BoundedEdit::Format { from, len, add } => {
            let (from, to) = clamp(*from, *len);
            if *add {
                TypedOperation::AddMark {
                    range: range_for_test(from, to),
                    mark: Mark::new("bold".into(), HashMap::new()),
                }
            } else {
                TypedOperation::RemoveMark {
                    range: range_for_test(from, to),
                    mark_type: "italic".into(),
                }
            }
        }
    }
}

fn compile_bounded_edit(
    doc: &Doc,
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
    editing_limits: &EditingLimits,
    operation: TypedOperation,
) -> crate::yrs_engine::OperationResult<CompiledTransaction> {
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    compile_transaction_with_yrs(
        CompilationContext {
            document,
            selection: None,
            schema,
            resource_limits: limits,
            editing_limits,
            document_revision: 0,
            max_length: None,
            validation_certificate: None,
        },
        TypedTransaction {
            request_id: BOUNDED_EDIT_REQUEST,
            base_document_revision: 0,
            origin: TransactionOrigin::LocalInput,
            operations: vec![operation],
            selection_intent: SelectionIntent::UseOperationResult,
            history_policy: HistoryPolicy::Auto,
        },
        &txn,
        &fragment,
    )
}

fn total_scalars(document: &Document, schema: &Schema) -> u32 {
    PositionMap::build(document, schema).total_scalars()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(160))]

    #[test]
    fn deletion_bounds_cover_recorded_clocks_and_encoded_growth(
        source in crate::test_support::document_strategy::document_strategy(),
        history in prop::collection::vec(bounded_edit_strategy(), 0..4),
        hidden_gaps in prop::collection::vec(any::<u32>(), 0..4),
        edit in bounded_edit_strategy(),
    ) {
        let (base, schema, limits, editing_limits, mut document) =
            diagnostic_doc_with_schema(&source, crate::schema::presets::tiptap_table_schema());
        let doc = Doc::with_options(Options {
            client_id: yrs::ClientID::new(REMOTE_EDITING_CLIENT),
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        let base_update = base.transact().encode_state_as_update_v1(&StateVector::default());
        doc.transact_mut()
            .apply_update(Update::decode_v1(&base_update).unwrap())
            .unwrap();
        for prior in &history {
            let operation = bounded_operation(prior, total_scalars(&document, &schema));
            if let Ok(compiled) =
                compile_bounded_edit(&doc, &document, &schema, &limits, &editing_limits, operation)
            {
                let mut txn = doc.transact_mut();
                execute_mutation_plan(compiled.mutation_plan, &mut txn);
                txn.commit();
                drop(txn);
                document = compiled.preview;
            }
        }
        inject_hidden_bold_pairs(&doc, &hidden_gaps);
        let operation = bounded_operation(&edit, total_scalars(&document, &schema));
        let context = format!(
            "{operation:?} after {history:?} on {}",
            crate::serialize::to_prosemirror_json(&document, &schema)
        );
        let Ok(compiled) =
            compile_bounded_edit(&doc, &document, &schema, &limits, &editing_limits, operation)
        else {
            return Ok(());
        };
        if compiled.mutation_plan.is_empty() {
            return Ok(());
        }
        if let Some(violation) = execute_measured_edit(&doc, compiled) {
            prop_assert!(false, "{}: {}", context, violation);
        }
    }
}

fn first_text_node<T: ReadTxn>(txn: &T, fragment: &yrs::XmlFragmentRef) -> Option<yrs::XmlTextRef> {
    let mut pending = fragment.children(txn).collect::<Vec<_>>();
    pending.reverse();
    while let Some(node) = pending.pop() {
        match node {
            XmlOut::Text(text) => return Some(text),
            XmlOut::Element(element) => {
                let mut children = element.children(txn).collect::<Vec<_>>();
                children.reverse();
                pending.extend(children);
            }
            XmlOut::Fragment(_) => {}
        }
    }
    None
}

fn inject_hidden_bold_pairs(doc: &Doc, gaps: &[u32]) {
    let mut txn = doc.transact_mut();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    let Some(text) = first_text_node(&txn, &fragment) else {
        return;
    };
    for gap in gaps {
        let boundaries = std::iter::once(0)
            .chain(yrs::GetString::get_string(&text, &txn).chars().scan(
                0u32,
                |offset, character| {
                    *offset += u32::try_from(character.len_utf16()).unwrap();
                    Some(*offset)
                },
            ))
            .collect::<Vec<_>>();
        let boundary = boundaries[usize::try_from(*gap).unwrap() % boundaries.len()];
        text.format(
            &mut txn,
            boundary,
            0,
            Attrs::from([(Arc::<str>::from("bold"), Any::Bool(true))]),
        );
    }
}

fn execute_measured_edit(doc: &Doc, compiled: CompiledTransaction) -> Option<String> {
    let fragment = doc.transact().get_xml_fragment("prosemirror").unwrap();
    let mut undo = UndoManager::<()>::new();
    undo.expand_scope(doc, &fragment);
    let before_full_len = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default())
        .len();
    let undo_units_bound = compiled.undo_units_bound;
    let encoded_growth_bound = compiled.encoded_growth_bound;
    let update = {
        let mut txn = doc.transact_mut();
        execute_mutation_plan(compiled.mutation_plan, &mut txn);
        txn.commit();
        txn.encode_update_v1()
    };
    let after_full_len = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default())
        .len();
    let clocks = |set: &yrs::IdSet| {
        set.iter()
            .flat_map(|(_, ranges)| ranges.into_iter())
            .map(|range| u64::from(range.end - range.start))
            .sum::<u64>()
    };
    let recorded = undo.undo_stack().last().map_or(0, |item| {
        clocks(item.deletions()) + clocks(item.insertions())
    });
    if recorded > undo_units_bound {
        return Some(format!(
            "recorded {recorded} undo clocks exceed the bound {undo_units_bound}"
        ));
    }
    if update.len() > encoded_growth_bound {
        return Some(format!(
            "transaction update of {} bytes exceeds the growth bound {encoded_growth_bound}",
            update.len()
        ));
    }
    if after_full_len > before_full_len + encoded_growth_bound {
        return Some(format!(
            "encoded state grew from {before_full_len} to {after_full_len} beyond the bound \
             {encoded_growth_bound}"
        ));
    }
    None
}

fn reversed_single_item_paragraph(
    text: &str,
) -> (Doc, Schema, ResourceLimits, EditingLimits, Document) {
    let source = json!({ "type": "doc", "content": [{ "type": "paragraph" }] });
    let (doc, schema, limits, editing_limits, _) = diagnostic_doc(&source);
    for character in text.chars().rev() {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_xml_fragment("prosemirror").unwrap();
        let XmlOut::Element(paragraph) = fragment.get(&txn, 0).unwrap() else {
            panic!("the fixture paragraph must be an element");
        };
        let text_ref = match paragraph.get(&txn, 0) {
            Some(XmlOut::Text(text_ref)) => text_ref,
            _ => paragraph.insert(&mut txn, 0, XmlTextPrelim::new("")),
        };
        text_ref.insert(&mut txn, 0, &character.to_string());
    }
    let document = from_prosemirror_json(
        &json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] }),
        &schema,
        UnknownTypeMode::Preserve,
    )
    .unwrap();
    (doc, schema, limits, editing_limits, document)
}

fn insert_hidden_bold_pairs(doc: &Doc, gaps: std::ops::Range<u32>) {
    let mut txn = doc.transact_mut();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    let text = paragraph_text(&fragment, &txn, 0);
    for gap in gaps {
        text.format(
            &mut txn,
            gap,
            0,
            Attrs::from([(Arc::<str>::from("bold"), Any::Bool(true))]),
        );
    }
}

fn compile_measured_operations(
    doc: &Doc,
    document: &Document,
    schema: &Schema,
    limits: &ResourceLimits,
    editing_limits: &EditingLimits,
    operations: Vec<TypedOperation>,
) -> CompiledTransaction {
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    compile_transaction_with_yrs(
        CompilationContext {
            document,
            selection: None,
            schema,
            resource_limits: limits,
            editing_limits,
            document_revision: 0,
            max_length: None,
            validation_certificate: None,
        },
        TypedTransaction {
            request_id: BOUNDED_EDIT_REQUEST,
            base_document_revision: 0,
            origin: TransactionOrigin::LocalInput,
            operations,
            selection_intent: SelectionIntent::UseOperationResult,
            history_policy: HistoryPolicy::Auto,
        },
        &txn,
        &fragment,
    )
    .unwrap()
}

#[test]
fn scoped_deletion_bounds_cover_hidden_formats_shifted_targets_and_attribute_removals() {
    const SHORT_TEXT: &str = "abcdefgh";
    const LONG_TEXT: &str = "abcdefghijklmnop";
    const LONG_HALF: u32 = 8;
    const LONG_LEN: u32 = 16;
    let bold = || Mark::new("bold".into(), HashMap::new());

    let (doc, schema, limits, editing_limits, document) =
        reversed_single_item_paragraph(SHORT_TEXT);
    insert_hidden_bold_pairs(&doc, 1..8);
    let hidden = compile_measured_operations(
        &doc,
        &document,
        &schema,
        &limits,
        &editing_limits,
        vec![TypedOperation::AddMark {
            range: range_for_test(0, 8),
            mark: bold(),
        }],
    );
    assert_eq!(
        execute_measured_edit(&doc, hidden),
        None,
        "bolding across hidden bold pairs at every gap"
    );

    let (doc, schema, limits, editing_limits, document) = reversed_single_item_paragraph(LONG_TEXT);
    insert_hidden_bold_pairs(&doc, LONG_HALF + 1..LONG_LEN);
    let shifted = compile_measured_operations(
        &doc,
        &document,
        &schema,
        &limits,
        &editing_limits,
        vec![
            TypedOperation::DeleteRange {
                range: range_for_test(0, LONG_HALF),
            },
            TypedOperation::AddMark {
                range: range_for_test(LONG_HALF, LONG_LEN),
                mark: bold(),
            },
        ],
    );
    assert_eq!(
        execute_measured_edit(&doc, shifted),
        None,
        "a format whose shifted range covers hidden bold pairs after a delete"
    );

    let (doc, _, _, removal) = super::direct_operations::compile_attribute_operations(
        json!({ "type": "doc", "content": [{ "type": "customBlock", "attrs": { "old": true } }] }),
        vec![
            HashMap::from([("label".into(), Value::String("temporary".into()))]),
            HashMap::new(),
        ],
    );
    assert_eq!(
        execute_measured_edit(&doc, removal),
        None,
        "removing a stored attribute"
    );
}
