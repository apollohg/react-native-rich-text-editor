const VISITOR_LOCAL_CLIENT: u64 = 9_101;
const VISITOR_REMOTE_CLIENT: u64 = 9_102;
const VISITOR_TEXT: &str = "visitor-text";
const VISITOR_MAP: &str = "visitor-map";
const VISITOR_FRAGMENT: &str = "visitor-fragment";
const VISITOR_ELEMENT_TAG: &str = "node";
const VISITOR_MAP_KEY: &str = "entry";
const VISITOR_FORMAT_KEYS: [&str; 2] = ["bold", "italic"];
const VISITOR_ATTRIBUTE_KEYS: [&str; 2] = ["kind", "level"];
const VISITOR_CHUNKS: [&str; 4] = ["a", "bc", "é", "xyz"];
const VISITOR_MAX_RUN: u32 = 6;
const VISITOR_MAX_NESTED: u8 = 3;
const VISITOR_SCAN_REQUEST: u64 = 73_100;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ClockCoverage {
    units: u64,
    clients: std::collections::BTreeSet<yrs::ClientID>,
}

impl ClockCoverage {
    fn record(&mut self, client: yrs::ClientID, len: u32) {
        self.units += u64::from(len);
        self.clients.insert(client);
    }

    fn covers(&self, tombstoned: &ClockCoverage) -> bool {
        self.units >= tombstoned.units && tombstoned.clients.is_subset(&self.clients)
    }
}

fn visitor_doc(client: u64) -> Doc {
    Doc::with_options(Options {
        client_id: yrs::ClientID::new(client),
        offset_kind: OffsetKind::Utf16,
        skip_gc: true,
        ..Options::default()
    })
}

fn sync_visitor_docs(left: &Doc, right: &Doc) {
    let left_update = left
        .transact()
        .encode_state_as_update_v1(&right.transact().state_vector());
    let right_update = right
        .transact()
        .encode_state_as_update_v1(&left.transact().state_vector());
    right
        .transact_mut()
        .apply_update(Update::decode_v1(&left_update).unwrap())
        .unwrap();
    left.transact_mut()
        .apply_update(Update::decode_v1(&right_update).unwrap())
        .unwrap();
}

fn tombstoned_by(doc: &Doc, edit: impl FnOnce(&mut yrs::TransactionMut)) -> ClockCoverage {
    let before = doc.transact().state_vector();
    let mut txn = doc.transact_mut();
    edit(&mut txn);
    let mut coverage = ClockCoverage::default();
    for (client, ranges) in txn.delete_set().iter() {
        let existing = before.get(client);
        for range in ranges {
            let end = range.end.min(existing);
            if end > range.start {
                coverage.record(*client, end - range.start);
            }
        }
    }
    coverage
}

fn clamp_visitor_range(at: u32, len: u32, total: u32) -> (u32, u32) {
    let from = at % (total + 1);
    (from, len.min(total - from))
}

fn visitor_format_attrs(key: usize, value: Option<bool>) -> Attrs {
    Attrs::from([(
        Arc::<str>::from(VISITOR_FORMAT_KEYS[key]),
        value.map_or(Any::Null, Any::Bool),
    )])
}

#[derive(Debug, Clone)]
enum TextSetupStep {
    Insert {
        at: u32,
        chunk: usize,
        remote: bool,
    },
    SharedEmbed {
        at: u32,
        nested: u8,
        remote: bool,
    },
    PrimitiveEmbed {
        at: u32,
        remote: bool,
    },
    Format {
        at: u32,
        len: u32,
        key: usize,
        value: Option<bool>,
        remote: bool,
    },
    Remove {
        at: u32,
        len: u32,
        remote: bool,
    },
    Sync,
}

#[derive(Debug, Clone)]
enum TextRangeEdit {
    Remove {
        at: u32,
        len: u32,
    },
    Format {
        at: u32,
        len: u32,
        key: usize,
        value: Option<bool>,
    },
}

fn text_setup_step() -> impl Strategy<Value = TextSetupStep> {
    let value = prop_oneof![Just(None), any::<bool>().prop_map(Some)];
    prop_oneof![
        4 => (any::<u32>(), 0..VISITOR_CHUNKS.len(), any::<bool>())
            .prop_map(|(at, chunk, remote)| TextSetupStep::Insert { at, chunk, remote }),
        1 => (any::<u32>(), 0..=VISITOR_MAX_NESTED, any::<bool>())
            .prop_map(|(at, nested, remote)| TextSetupStep::SharedEmbed { at, nested, remote }),
        1 => (any::<u32>(), any::<bool>())
            .prop_map(|(at, remote)| TextSetupStep::PrimitiveEmbed { at, remote }),
        3 => (any::<u32>(), 0..VISITOR_MAX_RUN, 0..VISITOR_FORMAT_KEYS.len(), value, any::<bool>())
            .prop_map(|(at, len, key, value, remote)| TextSetupStep::Format { at, len, key, value, remote }),
        1 => (any::<u32>(), 1..VISITOR_MAX_RUN, any::<bool>())
            .prop_map(|(at, len, remote)| TextSetupStep::Remove { at, len, remote }),
        1 => Just(TextSetupStep::Sync),
    ]
}

fn text_range_edit() -> impl Strategy<Value = TextRangeEdit> {
    let value = prop_oneof![Just(None), any::<bool>().prop_map(Some)];
    prop_oneof![
        (any::<u32>(), 1..VISITOR_MAX_RUN).prop_map(|(at, len)| TextRangeEdit::Remove { at, len }),
        (
            any::<u32>(),
            0..VISITOR_MAX_RUN,
            0..VISITOR_FORMAT_KEYS.len(),
            value
        )
            .prop_map(|(at, len, key, value)| TextRangeEdit::Format {
                at,
                len,
                key,
                value
            }),
    ]
}

fn insert_shared_embed(text: &yrs::TextRef, txn: &mut yrs::TransactionMut, at: u32, nested: u8) {
    let array = text.insert_embed(txn, at, ArrayPrelim::default());
    for index in 0..nested {
        yrs::Array::push_back(&array, txn, Any::from(u32::from(index)));
    }
    if nested > 0 {
        let map = yrs::Array::push_back(&array, txn, MapPrelim::default());
        yrs::Map::insert(&map, txn, VISITOR_MAP_KEY, Any::Bool(true));
    }
}

fn apply_text_setup(local: &Doc, remote: &Doc, step: &TextSetupStep) {
    let doc_for = |is_remote: bool| if is_remote { remote } else { local };
    match step {
        TextSetupStep::Sync => sync_visitor_docs(local, remote),
        TextSetupStep::Insert { at, chunk, remote } => {
            let doc = doc_for(*remote);
            let text = doc.get_or_insert_text(VISITOR_TEXT);
            let mut txn = doc.transact_mut();
            let at = at % (text.len(&txn) + 1);
            text.insert(&mut txn, at, VISITOR_CHUNKS[*chunk]);
        }
        TextSetupStep::SharedEmbed { at, nested, remote } => {
            let doc = doc_for(*remote);
            let text = doc.get_or_insert_text(VISITOR_TEXT);
            let mut txn = doc.transact_mut();
            let at = at % (text.len(&txn) + 1);
            insert_shared_embed(&text, &mut txn, at, *nested);
        }
        TextSetupStep::PrimitiveEmbed { at, remote } => {
            let doc = doc_for(*remote);
            let text = doc.get_or_insert_text(VISITOR_TEXT);
            let mut txn = doc.transact_mut();
            let at = at % (text.len(&txn) + 1);
            text.insert_embed(&mut txn, at, Any::Bool(true));
        }
        TextSetupStep::Format {
            at,
            len,
            key,
            value,
            remote,
        } => {
            let doc = doc_for(*remote);
            let text = doc.get_or_insert_text(VISITOR_TEXT);
            let mut txn = doc.transact_mut();
            let (from, len) = clamp_visitor_range(*at, *len, text.len(&txn));
            text.format(&mut txn, from, len, visitor_format_attrs(*key, *value));
        }
        TextSetupStep::Remove { at, len, remote } => {
            let doc = doc_for(*remote);
            let text = doc.get_or_insert_text(VISITOR_TEXT);
            let mut txn = doc.transact_mut();
            let (from, len) = clamp_visitor_range(*at, *len, text.len(&txn));
            if len > 0 {
                text.remove_range(&mut txn, from, len);
            }
        }
    }
}

fn text_range_visit(doc: &Doc, from: u32, len: u32) -> (ClockCoverage, usize) {
    let text = doc.get_or_insert_text(VISITOR_TEXT);
    let _txn = doc.transact();
    let mut visited = ClockCoverage::default();
    let walked = AsRef::<Branch>::as_ref(&text).visit_text_range_deletion_scope(
        OffsetKind::Utf16,
        from,
        len,
        &mut |client, units| visited.record(client, units),
    );
    (visited, walked)
}

fn text_items_debug(doc: &Doc) -> String {
    let text = doc.get_or_insert_text(VISITOR_TEXT);
    let txn = doc.transact();
    format!("{:?}", text.diff(&txn, yrs::types::text::YChange::identity))
}

fn check_text_range_edit(local: &Doc, edit: &TextRangeEdit) -> Result<ClockCoverage, String> {
    let text = local.get_or_insert_text(VISITOR_TEXT);
    let total = text.len(&local.transact());
    let (at, len) = match edit {
        TextRangeEdit::Remove { at, len } | TextRangeEdit::Format { at, len, .. } => (*at, *len),
    };
    let (from, len) = clamp_visitor_range(at, len, total);
    if len == 0 && matches!(edit, TextRangeEdit::Remove { .. }) {
        return Ok(ClockCoverage::default());
    }
    let (visited, _) = text_range_visit(local, from, len);
    let before = text_items_debug(local);
    let tombstoned = tombstoned_by(local, |txn| match edit {
        TextRangeEdit::Remove { .. } => text.remove_range(txn, from, len),
        TextRangeEdit::Format { key, value, .. } => {
            text.format(txn, from, len, visitor_format_attrs(*key, *value))
        }
    });
    if visited.covers(&tombstoned) {
        Ok(tombstoned)
    } else {
        Err(format!(
            "{edit:?} over [{from}, {}) of {before}: visited {visited:?} does not cover \
             tombstoned {tombstoned:?}",
            from + len
        ))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(768))]

    #[test]
    fn text_range_deletion_scope_covers_every_yrs_tombstone(
        steps in prop::collection::vec(text_setup_step(), 0..28),
        edit in text_range_edit(),
    ) {
        let local = visitor_doc(VISITOR_LOCAL_CLIENT);
        let remote = visitor_doc(VISITOR_REMOTE_CLIENT);
        for step in &steps {
            apply_text_setup(&local, &remote, step);
        }
        sync_visitor_docs(&local, &remote);
        if let Err(violation) = check_text_range_edit(&local, &edit) {
            prop_assert!(false, "after {:?}: {}", steps, violation);
        }
    }
}

#[test]
fn text_range_deletion_scope_counts_nested_embedded_subtrees() {
    let local = visitor_doc(VISITOR_LOCAL_CLIENT);
    let text = local.get_or_insert_text(VISITOR_TEXT);
    {
        let mut txn = local.transact_mut();
        text.insert(&mut txn, 0, "ab");
        insert_shared_embed(&text, &mut txn, 1, VISITOR_MAX_NESTED);
    }
    let tombstoned = check_text_range_edit(&local, &TextRangeEdit::Remove { at: 1, len: 1 })
        .unwrap_or_else(|violation| panic!("{violation}"));
    assert_eq!(
        tombstoned.units,
        2 + u64::from(VISITOR_MAX_NESTED) + 1,
        "removing the embed tombstones the array item, its {VISITOR_MAX_NESTED} values, the nested \
         map and the map entry"
    );
}

fn build_visitor_tree(
    txn: &mut yrs::TransactionMut,
    fragment: &yrs::XmlFragmentRef,
    shape: &[TreeShape],
) {
    fn insert_shape<P: XmlFragment>(txn: &mut yrs::TransactionMut, parent: &P, shape: &TreeShape) {
        let index = parent.len(txn);
        match shape {
            TreeShape::Text { chunk, bold_len } => {
                let text = parent.insert(txn, index, XmlTextPrelim::new(VISITOR_CHUNKS[*chunk]));
                let len = (*bold_len).min(text.len(txn));
                text.format(txn, 0, len, visitor_format_attrs(0, Some(true)));
            }
            TreeShape::Element { rewrites, children } => {
                let element =
                    parent.insert(txn, index, XmlElementPrelim::empty(VISITOR_ELEMENT_TAG));
                for rewrite in 0..*rewrites {
                    element.insert_attribute(
                        txn,
                        VISITOR_ATTRIBUTE_KEYS[usize::from(rewrite) % VISITOR_ATTRIBUTE_KEYS.len()],
                        rewrite.to_string(),
                    );
                }
                for child in children {
                    insert_shape(txn, &element, child);
                }
            }
        }
    }
    for child in shape {
        insert_shape(txn, fragment, child);
    }
}

#[derive(Debug, Clone)]
enum TreeShape {
    Text {
        chunk: usize,
        bold_len: u32,
    },
    Element {
        rewrites: u8,
        children: Vec<TreeShape>,
    },
}

fn tree_shape() -> impl Strategy<Value = TreeShape> {
    let leaf = (0..VISITOR_CHUNKS.len(), 0..VISITOR_MAX_RUN)
        .prop_map(|(chunk, bold_len)| TreeShape::Text { chunk, bold_len });
    leaf.prop_recursive(3, 24, 4, |inner| {
        (0..4u8, prop::collection::vec(inner, 0..4))
            .prop_map(|(rewrites, children)| TreeShape::Element { rewrites, children })
    })
}

fn child_range_visit<P: AsRef<Branch>>(
    doc: &Doc,
    parent: &P,
    from: u32,
    count: u32,
) -> ClockCoverage {
    let _txn = doc.transact();
    let mut visited = ClockCoverage::default();
    parent
        .as_ref()
        .visit_child_range_deletion_scope(from, count, &mut |client, units| {
            visited.record(client, units)
        });
    visited
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn child_range_deletion_scope_equals_yrs_tombstones(
        shape in prop::collection::vec(tree_shape(), 1..6),
        tombstone in (any::<u32>(), 0..3u32),
        nested_target in any::<bool>(),
        at in any::<u32>(),
        count in 1..4u32,
    ) {
        let doc = visitor_doc(VISITOR_LOCAL_CLIENT);
        let fragment = doc.get_or_insert_xml_fragment(VISITOR_FRAGMENT);
        {
            let mut txn = doc.transact_mut();
            build_visitor_tree(&mut txn, &fragment, &shape);
            let (from, len) = clamp_visitor_range(tombstone.0, tombstone.1, fragment.len(&txn));
            if len > 0 {
                fragment.remove_range(&mut txn, from, len);
            }
        }
        let element = {
            let txn = doc.transact();
            fragment.children(&txn).find_map(|child| match child {
                XmlOut::Element(element) if nested_target => Some(element),
                _ => None,
            })
        };
        let (visited, tombstoned, from, count) = match &element {
            Some(element) => {
                let total = element.len(&doc.transact());
                let (from, count) = clamp_visitor_range(at, count, total);
                let visited = child_range_visit(&doc, element, from, count);
                let tombstoned = tombstoned_by(&doc, |txn| element.remove_range(txn, from, count));
                (visited, tombstoned, from, count)
            }
            None => {
                let total = fragment.len(&doc.transact());
                let (from, count) = clamp_visitor_range(at, count, total);
                let visited = child_range_visit(&doc, &fragment, from, count);
                let tombstoned = tombstoned_by(&doc, |txn| fragment.remove_range(txn, from, count));
                (visited, tombstoned, from, count)
            }
        };
        prop_assert_eq!(
            &visited,
            &tombstoned,
            "removing children [{}, {}) of {} in {:?}",
            from,
            from + count,
            if element.is_some() { "the first element" } else { "the fragment" },
            shape
        );
    }
}

fn map_entry_visit<P: AsRef<Branch>>(doc: &Doc, parent: &P, key: &str) -> ClockCoverage {
    let _txn = doc.transact();
    let mut visited = ClockCoverage::default();
    parent
        .as_ref()
        .visit_map_entry_deletion_scope(key, &mut |client, units| visited.record(client, units));
    visited
}

#[test]
fn map_entry_deletion_scope_equals_yrs_tombstones_for_overwrites_and_removals() {
    let local = visitor_doc(VISITOR_LOCAL_CLIENT);
    let remote = visitor_doc(VISITOR_REMOTE_CLIENT);
    let map = remote.get_or_insert_map(VISITOR_MAP);
    {
        let mut txn = remote.transact_mut();
        let array = yrs::Map::insert(&map, &mut txn, VISITOR_MAP_KEY, ArrayPrelim::default());
        yrs::Array::push_back(&array, &mut txn, Any::Bool(true));
        let nested = yrs::Array::push_back(&array, &mut txn, MapPrelim::default());
        yrs::Map::insert(&nested, &mut txn, VISITOR_MAP_KEY, Any::Bool(false));
    }
    sync_visitor_docs(&local, &remote);
    let map = local.get_or_insert_map(VISITOR_MAP);
    let visited = map_entry_visit(&local, &map, VISITOR_MAP_KEY);
    let tombstoned = tombstoned_by(&local, |txn| {
        yrs::Map::insert(&map, txn, VISITOR_MAP_KEY, Any::Bool(true));
    });
    assert_eq!(
        tombstoned.units, 4,
        "overwriting the nested array tombstones the entry, its value, the nested map and its entry"
    );
    assert_eq!(visited, tombstoned, "a nested map-key overwrite");

    let visited = map_entry_visit(&local, &map, VISITOR_MAP_KEY);
    let tombstoned = tombstoned_by(&local, |txn| {
        yrs::Map::remove(&map, txn, VISITOR_MAP_KEY);
    });
    assert_eq!(tombstoned.units, 1);
    assert_eq!(visited, tombstoned, "removing a primitive map entry");

    let visited = map_entry_visit(&local, &map, VISITOR_MAP_KEY);
    assert_eq!(
        visited,
        ClockCoverage::default(),
        "a removed key has no live entry"
    );

    let fragment = local.get_or_insert_xml_fragment(VISITOR_FRAGMENT);
    let element = {
        let mut txn = local.transact_mut();
        let element = fragment.insert(&mut txn, 0, XmlElementPrelim::empty(VISITOR_ELEMENT_TAG));
        for rewrite in 0..3 {
            element.insert_attribute(&mut txn, VISITOR_ATTRIBUTE_KEYS[0], rewrite.to_string());
        }
        element
    };
    let visited = map_entry_visit(&local, &element, VISITOR_ATTRIBUTE_KEYS[0]);
    let tombstoned = tombstoned_by(&local, |txn| {
        element.insert_attribute(txn, VISITOR_ATTRIBUTE_KEYS[0], "final");
    });
    assert_eq!(
        tombstoned.units, 1,
        "only the live attribute value is tombstoned"
    );
    assert_eq!(
        visited, tombstoned,
        "an XML attribute overwrite after earlier rewrites"
    );
}

#[test]
fn deletion_scope_walk_is_charged_and_exhausting_the_scan_budget_rejects() {
    const HIDDEN_TEXT: &str = "abcdefgh";
    const DELETE_FROM: u32 = 1;
    const DELETE_TO: u32 = 7;
    let (doc, schema, limits, editing_limits, document) =
        reversed_single_item_paragraph(HIDDEN_TEXT);
    insert_hidden_bold_pairs(&doc, 1..8);
    let compiled = compile_measured_operations(
        &doc,
        &document,
        &schema,
        &limits,
        &editing_limits,
        vec![TypedOperation::DeleteRange {
            range: range_for_test(DELETE_FROM, DELETE_TO),
        }],
    );
    let txn = doc.transact();
    let (target, index, len) = match compiled.mutation_plan.actions.as_slice() {
        [YrsMutationAction::DeleteText {
            target,
            index_utf16,
            len_utf16,
            ..
        }] => (target.clone(), *index_utf16, *len_utf16),
        actions => panic!("a delete inside one paragraph lowers to one DeleteText: {actions:?}"),
    };
    let walked = AsRef::<Branch>::as_ref(&target).visit_text_range_deletion_scope(
        OffsetKind::Utf16,
        index,
        len,
        &mut |_, _| {},
    );
    let hidden_markers = 2 * 7;
    assert!(
        walked > hidden_markers,
        "the walk crosses every hidden marker and character, walked {walked}"
    );
    let envelope = crdt_envelope(
        VISITOR_SCAN_REQUEST,
        &txn,
        &compiled.mutation_plan,
        usize::MAX,
    )
    .unwrap();
    assert_eq!(
        envelope.scan_work, walked,
        "the envelope charges exactly the items walked"
    );
    let exhausted = crdt_envelope(
        VISITOR_SCAN_REQUEST,
        &txn,
        &compiled.mutation_plan,
        walked - 1,
    )
    .unwrap_err();
    assert_eq!(
        (exhausted.code, exhausted.limit, exhausted.actual),
        (
            "OPERATION_LIMIT_EXCEEDED",
            Some(u64::try_from(walked - 1).unwrap()),
            Some(u64::try_from(walked).unwrap())
        ),
        "{exhausted:?}"
    );
    drop(txn);
    let exact_budget = compiled.mutation_plan.scan_work + walked;
    let compile_with_budget = |max_input_bytes: usize| {
        let mut budget = limits.clone();
        budget.max_input_bytes = max_input_bytes;
        let txn = doc.transact();
        let fragment = txn.get_xml_fragment("prosemirror").unwrap();
        compile_transaction_with_yrs(
            CompilationContext {
                document: &document,
                selection: None,
                schema: &schema,
                resource_limits: &budget,
                editing_limits: &editing_limits,
                document_revision: 0,
                max_length: None,
                validation_certificate: None,
            },
            TypedTransaction {
                request_id: VISITOR_SCAN_REQUEST,
                base_document_revision: 0,
                origin: TransactionOrigin::LocalInput,
                operations: vec![TypedOperation::DeleteRange {
                    range: range_for_test(DELETE_FROM, DELETE_TO),
                }],
                selection_intent: SelectionIntent::UseOperationResult,
                history_policy: HistoryPolicy::Auto,
            },
            &txn,
            &fragment,
        )
    };
    let fits = compile_with_budget(exact_budget).unwrap();
    assert_eq!(fits.undo_units_bound, compiled.undo_units_bound);
    let over = compile_with_budget(exact_budget - 1).unwrap_err();
    assert_eq!(
        (over.code, over.limit, over.actual),
        (
            "OPERATION_LIMIT_EXCEEDED",
            Some(u64::try_from(exact_budget - 1).unwrap()),
            Some(u64::try_from(exact_budget).unwrap())
        ),
        "{over:?}"
    );
}

#[test]
fn a_child_deletion_after_an_insertion_into_the_same_parent_is_bounded() {
    const LONG_PARAGRAPH: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";
    let source = json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": "a" }] },
        { "type": "paragraph", "content": [{ "type": "text", "text": LONG_PARAGRAPH }] },
        { "type": "paragraph", "content": [{ "type": "text", "text": "c" }] }
    ]});
    let (doc, schema, limits, editing_limits, document) = diagnostic_doc(&source);
    let long_end = 2 + u32::try_from(LONG_PARAGRAPH.len()).unwrap() + 1;
    let compiled = compile_measured_operations(
        &doc,
        &document,
        &schema,
        &limits,
        &editing_limits,
        vec![
            TypedOperation::InsertNode {
                at: point_for_test(0),
                node: Node::element(
                    "paragraph".into(),
                    HashMap::new(),
                    Fragment::from(vec![Node::text("zz".into(), vec![])]),
                ),
            },
            TypedOperation::DeleteRange {
                range: range_for_test(1, long_end),
            },
        ],
    );
    let child_actions = compiled
        .mutation_plan
        .actions
        .iter()
        .filter_map(|action| match action {
            YrsMutationAction::InsertXmlChildren { .. } => Some("insert"),
            YrsMutationAction::DeleteXmlChildren { .. } => Some("delete"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        child_actions,
        ["insert", "delete"],
        "the deletion indices are shifted by an earlier insertion into the same parent"
    );
    assert_eq!(
        execute_measured_edit(&doc, compiled),
        None,
        "deleting the long paragraph after inserting before it"
    );
}
