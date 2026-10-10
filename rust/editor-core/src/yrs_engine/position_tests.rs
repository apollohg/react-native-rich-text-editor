use std::collections::HashMap;

use serde_json::{json, Value};
use yrs::branch::{Branch, BranchPtr};
use yrs::types::xml::{XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut, XmlTextRef};
use yrs::{Assoc, ReadTxn, StickyIndex};

use super::{
    boundary_anchors_at, boundary_chunks_at_doc_positions, is_table_cell_element, scalar_len,
    scalar_offset_to_utf16, sticky_at, xml_out_pm_size, xml_text_plain_string,
    BOUNDARY_SORT_TARGETS, BOUNDARY_WALK_NODE_VISITS,
};
use crate::model::Node;
use crate::position_epoch::BoundaryAnchors;
use crate::schema::content_rule::ContentRule;
use crate::schema::presets::prosemirror_table_schema;
use crate::schema::{AttrSpec, NodeRole, NodeSpec, Schema};
use crate::tables::commands::{TableCommand, NODE_OPENING_TOKENS};
use crate::tables::commands_tests::engine_with;
use crate::yrs_engine::{
    Affinity, EditorOffsetKind, HistoryPolicy, RevisionedPosition, SelectionInput, SelectionIntent,
    TransactionOrigin, TypedCommand, TypedTransaction, YrsDocumentEngine,
};

const MENTION_NODE: &str = "mention";
const TASK_LIST_NODE: &str = "task_list";
const TASK_ITEM_NODE: &str = "task_item";
const TABLE_NODE: &str = "table";
const HEADING_NODE: &str = "heading";
const DIRECTION_ATTR: &str = "dir";
const RIGHT_TO_LEFT: &str = "rtl";
const LONG_PROSE_PARAGRAPHS: usize = 40;
const LONG_PROSE_WORDS: usize = 12;
const TYPED_TEXT: &str = "q😀z";
const FIRST_EDIT_REQUEST: u64 = 1;
const INSIDE_WORD: u32 = 1;

#[test]
fn compact_text_epochs_preserve_every_dense_anchor_pin_and_retained_charge() {
    struct DenseConstruction;
    impl Drop for DenseConstruction {
        fn drop(&mut self) {
            super::BOUNDARY_COMPACTION_DISABLED.set(false);
        }
    }
    let mut saved_anchors = 0;
    for (label, engine) in corpus() {
        let dense = {
            super::BOUNDARY_COMPACTION_DISABLED.set(true);
            let _reset = DenseConstruction;
            engine.build_position_epoch_snapshot().unwrap()
        };
        let compact = engine.build_position_epoch_snapshot().unwrap();
        assert_eq!(
            dense.boundary_count(),
            compact.boundary_count(),
            "{label}: scalar domain"
        );
        assert_eq!(
            dense.retained_bytes, compact.retained_bytes,
            "{label}: unchanged admission charge"
        );
        for scalar in 0..dense.boundary_count() as u32 {
            let expected = dense.boundary(scalar).unwrap();
            let actual = compact.boundary(scalar).unwrap();
            assert_eq!(
                expected.anchors, actual.anchors,
                "{label}: exact anchor at {scalar}"
            );
            assert_eq!(
                expected.pinned_cell.map(|pin| (pin.cell, pin.point)),
                actual.pinned_cell.map(|pin| (pin.cell, pin.point)),
                "{label}: cell pin at {scalar}"
            );
        }
        saved_anchors += dense.boundary_count()
            - compact
                .chunks
                .iter()
                .map(|chunk| chunk.stored_anchor_count())
                .sum::<usize>();
    }
    assert!(
        saved_anchors > 0,
        "Certified plain text must avoid constructing per-character anchor objects"
    );
}

#[test]
fn splitting_one_text_into_ordered_chunks_keeps_scalar_traversal_linear() {
    const REPEATS: usize = 128;
    const CHUNK_BOUNDARIES: usize = 32;
    let text = "café😀".repeat(REPEATS);
    let engine = engine_with(
        corpus_schema(),
        vec![json!({"type":"paragraph", "content":[{"type":"text","text":text}]})],
    );
    let positions = scalar_doc_positions(&engine);
    let chunks: Vec<_> = positions
        .chunks(CHUNK_BOUNDARIES)
        .map(<[_]>::to_vec)
        .collect();
    engine
        .read_fragment_for_test(|txn, fragment| {
            super::BOUNDARY_TEXT_SCALAR_VISITS.set(0);
            let actual =
                boundary_chunks_at_doc_positions(txn, fragment, &chunks, engine.schema()).unwrap();
            assert_eq!(
                actual
                    .iter()
                    .map(|chunk| chunk.anchors.len())
                    .sum::<usize>(),
                positions.len()
            );
            assert_eq!(
                super::BOUNDARY_TEXT_SCALAR_VISITS.get(),
                text.chars().count(),
                "Split chunks must not rescan earlier UTF-16 prefixes"
            );
            for (chunk, positions) in actual.iter().zip(&chunks) {
                for (anchor, position) in chunk.anchors.iter().zip(positions) {
                    assert_eq!(
                        DescentAnchors::batched(&anchor),
                        anchors_by_descent(txn, fragment, *position, engine.schema()).unwrap()
                    );
                }
            }
        })
        .unwrap();
}

#[test]
fn scalar_to_utf16_preserves_mixed_text_boundaries() {
    const ASCII_PREFIX_LENGTH: usize = 257;
    let prefix = "a".repeat(ASCII_PREFIX_LENGTH);
    let corpus = [
        String::new(),
        prefix.clone(),
        format!("{prefix}é中🙂e\u{301}\r\n{prefix}"),
        format!("🙂{prefix}🧑\u{200d}💻"),
    ];
    for text in corpus {
        let mut expected_utf16 = 0;
        for (scalar, character) in text.chars().enumerate() {
            assert_eq!(
                scalar_offset_to_utf16(&text, scalar as u32),
                Some(expected_utf16),
                "incorrect UTF-16 boundary at scalar {scalar} in {text:?}"
            );
            expected_utf16 += character.encode_utf16(&mut [0; 2]).len() as u32;
        }
        let end = text.chars().count() as u32;
        assert_eq!(scalar_offset_to_utf16(&text, end), Some(expected_utf16));
        assert_eq!(scalar_offset_to_utf16(&text, end + 1), None);
        assert_eq!(scalar_offset_to_utf16(&text, u32::MAX), None);
    }
}

fn corpus_schema() -> Schema {
    let base = prosemirror_table_schema();
    let mut nodes: Vec<NodeSpec> = base
        .all_nodes()
        .cloned()
        .map(|mut node| {
            if node.name == TABLE_NODE {
                node.attrs.insert(
                    DIRECTION_ATTR.into(),
                    AttrSpec {
                        default: Some(Value::Null),
                        has_default: true,
                        ..AttrSpec::default()
                    },
                );
            }
            node
        })
        .collect();
    let spec = |name: &str, content: &str, group: Option<&str>, role: NodeRole, is_void| NodeSpec {
        name: name.into(),
        content: ContentRule::parse(content).expect("the corpus content rule parses"),
        group: group.map(Into::into),
        attrs: HashMap::new(),
        role,
        html_tag: None,
        html_rules: None,
        json_projection: None,
        is_void,
        deletable_on_backspace: None,
        allow_undeclared_attrs: true,
        table_role: None,
    };
    nodes.push(spec(
        MENTION_NODE,
        "",
        Some("inline"),
        NodeRole::Inline,
        true,
    ));
    nodes.push(spec(
        TASK_LIST_NODE,
        &format!("{TASK_ITEM_NODE}+"),
        Some("block"),
        NodeRole::List { ordered: false },
        false,
    ));
    nodes.push(spec(
        TASK_ITEM_NODE,
        "paragraph block*",
        None,
        NodeRole::ListItem,
        false,
    ));
    Schema::new(nodes, base.all_marks().cloned().collect())
}

fn text(value: &str) -> Value {
    json!({ "type": "text", "text": value })
}

fn marked(value: &str, mark: &str) -> Value {
    json!({ "type": "text", "text": value, "marks": [{ "type": mark }] })
}

fn linked(value: &str) -> Value {
    json!({ "type": "text", "text": value, "marks": [{ "type": "link", "attrs": { "href": "https://example.com" } }] })
}

fn hard_break() -> Value {
    json!({ "type": "hard_break" })
}

fn mention(label: &str) -> Value {
    json!({ "type": MENTION_NODE, "attrs": { "label": label } })
}

fn paragraph(content: Vec<Value>) -> Value {
    json!({ "type": "paragraph", "content": content })
}

fn empty_paragraph() -> Value {
    json!({ "type": "paragraph" })
}

fn heading(level: u8, content: Vec<Value>) -> Value {
    json!({ "type": HEADING_NODE, "attrs": { "level": level }, "content": content })
}

fn list(list_type: &str, item_type: &str, items: Vec<Vec<Value>>) -> Value {
    let items: Vec<Value> = items
        .into_iter()
        .map(|content| json!({ "type": item_type, "content": content }))
        .collect();
    json!({ "type": list_type, "content": items })
}

fn cell(content: Vec<Value>) -> Value {
    json!({ "type": "table_cell", "content": content })
}

fn spanned_cell(colspan: u32, rowspan: u32, content: Vec<Value>) -> Value {
    json!({ "type": "table_cell", "attrs": { "colspan": colspan, "rowspan": rowspan }, "content": content })
}

fn header(content: Vec<Value>) -> Value {
    json!({ "type": "table_header", "content": content })
}

fn row(cells: Vec<Value>) -> Value {
    json!({ "type": "table_row", "content": cells })
}

fn table(rows: Vec<Value>) -> Value {
    json!({ "type": TABLE_NODE, "content": rows })
}

fn rtl_table(rows: Vec<Value>) -> Value {
    json!({ "type": TABLE_NODE, "attrs": { DIRECTION_ATTR: RIGHT_TO_LEFT }, "content": rows })
}

fn rich_prose() -> Vec<Value> {
    vec![
        heading(1, vec![text("Title "), marked("bold", "bold")]),
        paragraph(vec![
            text("Plain "),
            marked("emphasis", "italic"),
            hard_break(),
            linked("link"),
            text(" and "),
            mention("Ada"),
            text(" A😀e\u{301} tail"),
        ]),
        empty_paragraph(),
        json!({ "type": "horizontal_rule" }),
        json!({ "type": "image", "attrs": { "src": "https://example.com/a.png" } }),
        json!({ "type": "blockquote", "content": [paragraph(vec![text("quoted")]), empty_paragraph()] }),
        json!({ "type": "codeBlock", "content": [text("let x = 1;\nlet y = 2;")] }),
        list(
            "bullet_list",
            "list_item",
            vec![
                vec![paragraph(vec![text("one")])],
                vec![
                    paragraph(vec![marked("two", "bold")]),
                    list(
                        "ordered_list",
                        "list_item",
                        vec![
                            vec![paragraph(vec![text("nested")])],
                            vec![empty_paragraph()],
                        ],
                    ),
                ],
            ],
        ),
        list(
            TASK_LIST_NODE,
            TASK_ITEM_NODE,
            vec![
                vec![paragraph(vec![text("todo")])],
                vec![paragraph(vec![mention("Bob")])],
            ],
        ),
        heading(3, vec![]),
        paragraph(vec![hard_break(), hard_break()]),
    ]
}

fn tables() -> Vec<Value> {
    let nested = table(vec![row(vec![
        cell(vec![list(
            "bullet_list",
            "list_item",
            vec![vec![paragraph(vec![text("inner")])]],
        )]),
        cell(vec![paragraph(vec![
            text("side"),
            hard_break(),
            mention("Cy"),
        ])]),
    ])]);
    vec![
        paragraph(vec![text("before")]),
        table(vec![
            row(vec![
                header(vec![paragraph(vec![marked("Head", "bold")])]),
                header(vec![empty_paragraph()]),
                header(vec![heading(2, vec![text("H")])]),
            ]),
            row(vec![
                spanned_cell(2, 1, vec![paragraph(vec![text("wide")])]),
                spanned_cell(1, 2, vec![paragraph(vec![text("tall")]), empty_paragraph()]),
            ]),
            row(vec![
                cell(vec![list(
                    TASK_LIST_NODE,
                    TASK_ITEM_NODE,
                    vec![vec![paragraph(vec![text("task")])]],
                )]),
                cell(vec![
                    paragraph(vec![text("outer")]),
                    nested,
                    paragraph(vec![text("after")]),
                ]),
            ]),
        ]),
        empty_paragraph(),
        rtl_table(vec![
            row(vec![
                cell(vec![paragraph(vec![text("مرحبا")])]),
                cell(vec![paragraph(vec![
                    text("שלום"),
                    hard_break(),
                    text("עולם"),
                ])]),
            ]),
            row(vec![
                cell(vec![empty_paragraph()]),
                cell(vec![
                    json!({ "type": "horizontal_rule" }),
                    paragraph(vec![text("😀")]),
                ]),
            ]),
        ]),
        paragraph(vec![text("after")]),
    ]
}

fn long_prose() -> Vec<Value> {
    (0..LONG_PROSE_PARAGRAPHS)
        .map(|index| {
            let words = (0..LONG_PROSE_WORDS)
                .map(|word| format!("w{index}_{word} "))
                .collect::<String>();
            paragraph(vec![
                text(&words),
                marked(&words, "bold"),
                hard_break(),
                marked(&words, "italic"),
                text(&words),
            ])
        })
        .collect()
}

fn place_caret(engine: &mut YrsDocumentEngine, request_id: u64, offset: u32) {
    let point = RevisionedPosition {
        offset,
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::After,
    };
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![],
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point,
                head: point,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap_or_else(|error| panic!("placing the caret at {offset} must apply: {error:?}"));
}

fn apply(engine: &mut YrsDocumentEngine, request_id: u64, command: TypedCommand) {
    let description = format!("{command:?}");
    engine
        .apply_command(request_id, command)
        .unwrap_or_else(|error| panic!("{description} must apply: {error:?}"))
        .unwrap_or_else(|| panic!("{description} must not be a no-op"));
}

fn doc_position_inside_word(parent: &Node, content_start: u32, word: &str) -> Option<u32> {
    let mut position = content_start;
    for child in parent.content()?.iter() {
        if let Some(byte_index) = child.text_str().and_then(|value| value.find(word)) {
            let chars_before = child.text_str()?[..byte_index].chars().count();
            return position.checked_add(u32::try_from(chars_before).ok()? + INSIDE_WORD);
        }
        if child.is_element() {
            if let Some(found) =
                doc_position_inside_word(child, position + NODE_OPENING_TOKENS, word)
            {
                return Some(found);
            }
        }
        position += child.node_size();
    }
    None
}

fn caret_inside_word(engine: &YrsDocumentEngine, word: &str) -> u32 {
    let document = engine.document().expect("the engine is ready");
    let doc_pos = doc_position_inside_word(document.root(), 0, word)
        .unwrap_or_else(|| panic!("the corpus contains {word:?}"));
    engine
        .position_map()
        .expect("the engine is ready")
        .doc_to_scalar(doc_pos, document)
}

fn edited(content: Vec<Value>, words: &[&str]) -> YrsDocumentEngine {
    let mut engine = engine_with(corpus_schema(), content);
    let mut request_id = FIRST_EDIT_REQUEST;
    for word in words {
        let caret = caret_inside_word(&engine, word);
        place_caret(&mut engine, request_id, caret);
        for command in [
            TypedCommand::InsertText {
                text: TYPED_TEXT.into(),
            },
            TypedCommand::SplitBlock,
            TypedCommand::DeleteBackward,
            TypedCommand::DeleteBackward,
        ] {
            request_id += 1;
            apply(&mut engine, request_id, command);
        }
        request_id += 1;
    }
    engine
}

fn corpus() -> Vec<(&'static str, YrsDocumentEngine)> {
    vec![
        ("rich prose", engine_with(corpus_schema(), rich_prose())),
        ("tables", engine_with(corpus_schema(), tables())),
        (
            "multi-paragraph and nested cells",
            engine_with(
                corpus_schema(),
                crate::test_support::large_table_fixture::multi_paragraph_cell_document()
                    ["content"]
                    .as_array()
                    .unwrap()
                    .clone(),
            ),
        ),
        ("long prose", engine_with(corpus_schema(), long_prose())),
        (
            "edited rich prose",
            edited(rich_prose(), &["emphasis", "quoted", "nested", "todo"]),
        ),
        (
            "edited tables",
            edited(tables(), &["wide", "task", "outer", "עולם"]),
        ),
        (
            "edited long prose",
            edited(long_prose(), &["w0_3", "w17_5", "w39_11"]),
        ),
    ]
}

fn scalar_doc_positions(engine: &YrsDocumentEngine) -> Vec<u32> {
    let map = engine.position_map().expect("the engine is ready");
    let document = engine.document().expect("the engine is ready");
    (0..=map.total_scalars())
        .map(|scalar| map.scalar_to_doc(scalar, document))
        .collect()
}

fn every_doc_position(engine: &YrsDocumentEngine) -> Vec<u32> {
    let document = engine.document().expect("the engine is ready");
    (0..=document.content_size()).collect()
}

#[derive(Debug, PartialEq)]
struct DescentAnchors {
    before: StickyIndex,
    after: StickyIndex,
    ancestor_before: Vec<StickyIndex>,
    ancestor_after: Vec<StickyIndex>,
    table_cell_ancestors: Option<usize>,
}

impl DescentAnchors {
    fn leaf(anchors: BoundaryAnchors) -> Self {
        Self {
            before: anchors.before,
            after: anchors.after,
            ancestor_before: Vec::new(),
            ancestor_after: Vec::new(),
            table_cell_ancestors: None,
        }
    }

    fn batched(anchors: &BoundaryAnchors) -> Self {
        let chain = anchors.ancestor_chain();
        Self {
            before: anchors.before.clone(),
            after: anchors.after.clone(),
            ancestor_before: chain
                .clone()
                .map(|ancestor| ancestor.before.clone())
                .collect(),
            ancestor_after: chain
                .clone()
                .map(|ancestor| ancestor.after.clone())
                .collect(),
            table_cell_ancestors: chain.clone().position(|ancestor| ancestor.table_cell),
        }
    }
}

fn anchors_by_descent<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    doc_pos: u32,
    schema: &Schema,
) -> Option<DescentAnchors> {
    anchors_by_descent_in_sequence(
        txn,
        fragment.children(txn),
        doc_pos,
        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(fragment)),
        schema,
    )
}

fn anchors_by_descent_in_sequence<'a, T: ReadTxn>(
    txn: &T,
    children: impl Iterator<Item = XmlOut> + 'a,
    doc_pos: u32,
    branch: BranchPtr,
    schema: &Schema,
) -> Option<DescentAnchors> {
    let mut branch_index = 0u32;
    let mut consumed_pm = 0u32;
    let mut children = children.peekable();

    while let Some(child) = children.next() {
        match &child {
            XmlOut::Text(text) => {
                let text_value = xml_text_plain_string(text, txn)?;
                let text_scalar_len = scalar_len(&text_value);
                let mut retry_adjacent_text = false;
                if doc_pos <= consumed_pm + text_scalar_len {
                    let utf16_offset = scalar_offset_to_utf16(&text_value, doc_pos - consumed_pm)?;
                    let text_branch = BranchPtr::from(<XmlTextRef as AsRef<Branch>>::as_ref(text));
                    let before = sticky_at(txn, text_branch, utf16_offset, Assoc::Before)
                        .or_else(|| sticky_at(txn, branch, branch_index, Assoc::Before));
                    let after =
                        sticky_at(txn, text_branch, utf16_offset, Assoc::After).or_else(|| {
                            sticky_at(txn, branch, branch_index.checked_add(1)?, Assoc::After)
                        });
                    if let (Some(before), Some(after)) = (before, after) {
                        return Some(DescentAnchors {
                            before,
                            after,
                            ancestor_before: Vec::new(),
                            ancestor_after: Vec::new(),
                            table_cell_ancestors: None,
                        });
                    }
                    if doc_pos < consumed_pm + text_scalar_len {
                        return None;
                    }
                    retry_adjacent_text = true;
                }
                branch_index = branch_index.checked_add(1)?;
                consumed_pm = consumed_pm.checked_add(text_scalar_len)?;
                if retry_adjacent_text && !matches!(children.peek(), Some(XmlOut::Text(_))) {
                    return None;
                }
            }
            XmlOut::Element(element) => {
                let child_size = xml_out_pm_size(txn, &child, schema)?;
                if doc_pos == consumed_pm {
                    return boundary_anchors_at(txn, branch, branch_index)
                        .map(DescentAnchors::leaf);
                }
                if doc_pos < consumed_pm.checked_add(child_size)? {
                    let mut anchors = anchors_by_descent_in_sequence(
                        txn,
                        element.children(txn),
                        doc_pos.checked_sub(consumed_pm)?.checked_sub(1)?,
                        BranchPtr::from(<XmlElementRef as AsRef<Branch>>::as_ref(element)),
                        schema,
                    )?;
                    if anchors.table_cell_ancestors.is_none()
                        && is_table_cell_element(element, txn, schema)
                    {
                        anchors.table_cell_ancestors = Some(anchors.ancestor_before.len());
                    }
                    anchors.ancestor_before.push(sticky_at(
                        txn,
                        branch,
                        branch_index,
                        Assoc::Before,
                    )?);
                    anchors.ancestor_after.push(sticky_at(
                        txn,
                        branch,
                        branch_index.checked_add(1)?,
                        Assoc::After,
                    )?);
                    return Some(anchors);
                }
                branch_index = branch_index.checked_add(1)?;
                consumed_pm = consumed_pm.checked_add(child_size)?;
            }
            XmlOut::Fragment(nested) => {
                let child_size = xml_out_pm_size(txn, &child, schema)?;
                if doc_pos == consumed_pm {
                    return boundary_anchors_at(txn, branch, branch_index)
                        .map(DescentAnchors::leaf);
                }
                if doc_pos < consumed_pm.checked_add(child_size)? {
                    let mut anchors = anchors_by_descent_in_sequence(
                        txn,
                        nested.children(txn),
                        doc_pos.checked_sub(consumed_pm)?,
                        BranchPtr::from(<XmlFragmentRef as AsRef<Branch>>::as_ref(nested)),
                        schema,
                    )?;
                    anchors.ancestor_before.push(sticky_at(
                        txn,
                        branch,
                        branch_index,
                        Assoc::Before,
                    )?);
                    anchors.ancestor_after.push(sticky_at(
                        txn,
                        branch,
                        branch_index.checked_add(1)?,
                        Assoc::After,
                    )?);
                    return Some(anchors);
                }
                branch_index = branch_index.checked_add(1)?;
                consumed_pm = consumed_pm.checked_add(child_size)?;
            }
        }
    }

    (doc_pos == consumed_pm)
        .then(|| boundary_anchors_at(txn, branch, branch_index))
        .flatten()
        .map(DescentAnchors::leaf)
}

fn assert_batch_matches_descent(
    label: &str,
    engine: &YrsDocumentEngine,
    doc_positions: &[u32],
    scalar_snapshot: bool,
) {
    let snapshot = scalar_snapshot.then(|| {
        engine
            .build_position_epoch_snapshot()
            .expect("scalar snapshot builds")
    });
    let schema = engine.schema();
    engine
        .read_fragment_for_test(|txn, fragment| {
            let mut unresolved = Vec::new();
            let mut expected = Vec::with_capacity(doc_positions.len());
            for &doc_pos in doc_positions {
                match anchors_by_descent(txn, fragment, doc_pos, schema) {
                    Some(anchors) => expected.push(anchors),
                    None => unresolved.push(doc_pos),
                }
            }
            assert!(
                unresolved.is_empty(),
                "{label}: per-position descent cannot anchor doc positions {unresolved:?}"
            );
            let batched = snapshot.as_ref().map(|snapshot| snapshot.chunks.to_vec()).unwrap_or_else(||
                boundary_chunks_at_doc_positions(txn, fragment, &[doc_positions.to_vec()], schema)
                    .unwrap_or_else(|| panic!("{label}: the batched walk failed to anchor every position")));
            let flattened: Vec<_> = batched.iter().flat_map(|chunk| chunk.anchors.iter()).collect();
            assert_eq!(flattened.len(), expected.len(), "{label}: one boundary per position");
            for (index, (actual, wanted)) in flattened.iter().zip(&expected).enumerate() {
                assert_eq!(
                    &DescentAnchors::batched(actual),
                    wanted,
                    "{label}: boundary {index} at doc position {} differs from per-position descent",
                    doc_positions[index]
                );
            }
        })
        .expect("the engine fragment exists");
}

#[test]
fn ordered_boundary_chunks_borrow_targets_without_changing_anchors_or_fees() {
    const BOUNDARIES_PER_CHUNK: usize = 3;
    const CHUNKS_PER_GROUP: usize = 4;
    for (label, engine) in corpus() {
        let positions = scalar_doc_positions(&engine);
        let chunks: Vec<_> = positions
            .chunks(BOUNDARIES_PER_CHUNK)
            .flat_map(|chunk| {
                [
                    Vec::new(),
                    chunk.to_vec(),
                    Vec::new(),
                    vec![*chunk.last().unwrap()],
                ]
            })
            .collect();
        engine
            .read_fragment_for_test(|txn, fragment| {
                BOUNDARY_SORT_TARGETS.set(0);
                let actual =
                    boundary_chunks_at_doc_positions(txn, fragment, &chunks, engine.schema())
                        .unwrap_or_else(|| panic!("{label}: ordered chunks failed"));
                assert_eq!(
                    BOUNDARY_SORT_TARGETS.get(),
                    0,
                    "{label}: ordered targets need no sorting buffer"
                );
                assert_eq!(
                    actual.len(),
                    chunks.len(),
                    "{label}: empty chunks must survive"
                );
                for group in actual.chunks_exact(CHUNKS_PER_GROUP) {
                    let source = group[1].anchors.iter().next_back().unwrap();
                    let repeated = group[3].anchors.get(0).unwrap();
                    assert_eq!(source, repeated, "{label}: duplicate across an empty chunk");
                    match (&source.ancestor, &repeated.ancestor) {
                        (Some(source), Some(repeated)) => assert!(
                            std::sync::Arc::ptr_eq(source, repeated),
                            "{label}: duplicate boundaries share the same ancestor chain"
                        ),
                        (None, None) => {}
                        _ => panic!("{label}: duplicate ancestor ownership differs"),
                    }
                }
                let reversed: Vec<_> = chunks
                    .iter()
                    .rev()
                    .map(|chunk| chunk.iter().rev().copied().collect())
                    .collect();
                let fallback =
                    boundary_chunks_at_doc_positions(txn, fragment, &reversed, engine.schema())
                        .expect("the sorted fallback must resolve the same boundaries");
                for (index, ((chunk, positions), reversed_chunk)) in actual
                    .iter()
                    .zip(&chunks)
                    .zip(fallback.iter().rev())
                    .enumerate()
                {
                    assert_eq!(
                        chunk.anchors.len(),
                        positions.len(),
                        "{label}: chunk {index}"
                    );
                    assert_eq!(
                        chunk.anchors.capacity(),
                        reversed_chunk.anchors.capacity(),
                        "{label}: chunk {index} capacity"
                    );
                    assert_eq!(
                        chunk.retained_bytes, reversed_chunk.retained_bytes,
                        "{label}: chunk {index} charge"
                    );
                    assert!(
                        chunk.anchors.iter().eq(reversed_chunk.anchors.iter().rev()),
                        "{label}: chunk {index} sorted fallback anchors"
                    );
                    for (anchor, position) in chunk.anchors.iter().zip(positions) {
                        let expected =
                            anchors_by_descent(txn, fragment, *position, engine.schema())
                                .expect("scalar boundary must be resolvable");
                        assert_eq!(
                            DescentAnchors::batched(&anchor),
                            expected,
                            "{label}: chunk {index} position {position}"
                        );
                    }
                }
            })
            .expect("the engine fragment exists");
    }
}

#[test]
fn boundary_target_cursor_preserves_duplicate_and_unordered_input_indices() {
    let cases = [
        vec![],
        vec![vec![], vec![]],
        vec![vec![], vec![0, 0], vec![], vec![0, 1], vec![]],
        vec![vec![1, 3], vec![], vec![0, 3]],
        vec![vec![3, 1, 0], vec![], vec![u32::MAX, u32::MAX]],
    ];
    for positions in cases {
        let mut expected: Vec<_> = positions
            .iter()
            .enumerate()
            .flat_map(|(block, positions)| {
                positions
                    .iter()
                    .enumerate()
                    .map(move |(offset, position)| (*position, block, offset))
            })
            .collect();
        expected.sort_unstable();
        let mut cursor = super::BoundaryTargets::new(&positions).expect("small cursor fits");
        for target in expected {
            assert_eq!(cursor.peek(), Some(target), "input {positions:?}");
            cursor.advance();
        }
        assert_eq!(
            cursor.peek(),
            None,
            "input {positions:?}: cursor must be exhausted"
        );
    }
}

#[test]
fn batched_boundary_anchors_match_per_position_descent_at_every_scalar() {
    for (label, engine) in corpus() {
        let doc_positions = scalar_doc_positions(&engine);
        eprintln!("{label}: {} scalar boundaries", doc_positions.len());
        assert_batch_matches_descent(label, &engine, &doc_positions, true);
    }
}

#[test]
fn batched_boundary_anchors_match_per_position_descent_at_every_document_position() {
    for (label, engine) in corpus() {
        let mut doc_positions = every_doc_position(&engine);
        eprintln!("{label}: {} document positions", doc_positions.len());
        assert_batch_matches_descent(label, &engine, &doc_positions, false);
        doc_positions.reverse();
        assert_batch_matches_descent(label, &engine, &doc_positions, false);
    }
}

#[test]
fn batched_boundary_anchors_match_descent_after_sibling_deletions() {
    const SIDE: usize = 5;
    let rows = (0..SIDE)
        .map(|r| {
            row((0..SIDE)
                .map(|c| cell(vec![paragraph(vec![text(&format!("cell_{r}_{c}"))])]))
                .collect())
        })
        .collect();
    let mut engine = engine_with(corpus_schema(), vec![table(rows)]);
    let mut request_id = FIRST_EDIT_REQUEST;
    for (word, command) in [
        ("cell_1_0", TableCommand::DeleteTableColumns),
        ("cell_1_2", TableCommand::DeleteTableColumns),
        ("cell_1_4", TableCommand::DeleteTableColumns),
        ("cell_0_1", TableCommand::DeleteTableRows),
        ("cell_2_1", TableCommand::DeleteTableRows),
        ("cell_4_1", TableCommand::DeleteTableRows),
    ] {
        let caret = caret_inside_word(&engine, word);
        place_caret(&mut engine, request_id, caret);
        request_id += 1;
        apply(&mut engine, request_id, TypedCommand::Table(command));
        request_id += 1;
        let positions = every_doc_position(&engine);
        assert_batch_matches_descent(word, &engine, &positions, false);
        let scalar_positions = scalar_doc_positions(&engine);
        assert_batch_matches_descent(word, &engine, &scalar_positions, true);
    }
}

fn yrs_node_count<T: ReadTxn>(txn: &T, children: impl Iterator<Item = XmlOut>) -> usize {
    children
        .map(|child| {
            let nested = match &child {
                XmlOut::Element(element) => yrs_node_count(txn, element.children(txn)),
                XmlOut::Fragment(fragment) => yrs_node_count(txn, fragment.children(txn)),
                XmlOut::Text(_) => 0,
            };
            nested + 1
        })
        .sum()
}

#[test]
fn pinning_visits_every_yrs_node_once_however_many_boundaries_it_anchors() {
    for (label, engine) in corpus() {
        let node_count = engine
            .read_fragment_for_test(|txn, fragment| yrs_node_count(txn, fragment.children(txn)))
            .expect("the engine fragment exists");
        BOUNDARY_WALK_NODE_VISITS.set(0);

        let boundaries = engine
            .build_position_epoch_snapshot()
            .unwrap_or_else(|| panic!("{label}: the position epoch builds"));

        let visits = BOUNDARY_WALK_NODE_VISITS.replace(0);
        eprintln!(
            "{label}: {} boundaries anchored with {visits} node visits over {node_count} Yrs nodes",
            boundaries.boundary_count()
        );
        assert_eq!(
            visits, node_count,
            "{label}: pinning walks the document once instead of descending per boundary"
        );
        engine
            .read_fragment_for_test(|txn, fragment| {
                for positions in [
                    vec![],
                    vec![vec![]],
                    vec![vec![0]],
                    vec![vec![u32::MAX], vec![0]],
                ] {
                    BOUNDARY_WALK_NODE_VISITS.set(0);
                    let chunks = boundary_chunks_at_doc_positions(
                        txn,
                        fragment,
                        &positions,
                        engine.schema(),
                    );
                    assert_eq!(
                        chunks.is_some(),
                        !positions
                            .iter()
                            .flatten()
                            .any(|position| *position == u32::MAX),
                        "{label}: invalid targets must fail, empty targets remain valid"
                    );
                    assert_eq!(
                        BOUNDARY_WALK_NODE_VISITS.get(),
                        node_count,
                        "{label}: exhausted targets must not stop XML validation for {positions:?}"
                    );
                }
            })
            .expect("the engine fragment exists");
    }
}

fn assert_plain_read_matches_diff(
    label: &str,
    text: &XmlTextRef,
    txn: &impl ReadTxn,
    expected_fallbacks: usize,
) -> Option<String> {
    use yrs::Text;
    let expected = text
        .diff(txn, yrs::types::text::YChange::identity)
        .into_iter()
        .try_fold(String::new(), |mut value, diff| {
            let yrs::Out::Any(yrs::Any::String(run)) = diff.insert else {
                return None;
            };
            value.push_str(&run);
            Some(value)
        });
    super::PLAIN_TEXT_DIFF_FALLBACKS.set(0);
    let actual = xml_text_plain_string(text, txn);
    assert_eq!(
        actual, expected,
        "{label}: plain text differs from the mark-aware reader"
    );
    assert_eq!(
        super::PLAIN_TEXT_DIFF_FALLBACKS.get(),
        expected_fallbacks,
        "{label}: only unsupported live item content needs mark diffs"
    );
    actual
}

fn plain_read_test_doc(client_id: u64) -> yrs::Doc {
    let mut options = yrs::Options::with_client_id(yrs::ClientID::new(client_id));
    options.offset_kind = yrs::OffsetKind::Utf16;
    yrs::Doc::with_options(options)
}

#[test]
fn single_text_item_certificate_matches_real_sticky_ids_and_rejects_fragmentation() {
    use yrs::{Any, Text, Transact, XmlTextPrelim};
    const CLIENT: u64 = 94;
    const FRAGMENT: &str = "single-item-certificate";
    for value in ["", "plain text", "a😀e\u{301} עברית"] {
        let doc = plain_read_test_doc(CLIENT);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let text = fragment.push_back(&mut txn, XmlTextPrelim::new(value));
        let certified = text.try_single_text_item(&txn);
        if value.is_empty() {
            assert!(certified.is_none(), "empty text has no item identity");
            continue;
        }
        let (id, copied) = certified.expect("one inserted text item is certifiable");
        assert_eq!(copied, value);
        let branch = BranchPtr::from(<XmlTextRef as AsRef<Branch>>::as_ref(&text));
        let length = text.len(&txn);
        for offset in 1..length {
            for assoc in [Assoc::Before, Assoc::After] {
                let clock = id.clock + offset - u32::from(assoc == Assoc::Before);
                assert_eq!(
                    StickyIndex::from_id(yrs::ID::new(id.client, clock), assoc),
                    StickyIndex::at(&txn, branch, offset, assoc).unwrap(),
                    "{value:?}, UTF-16 offset={offset}, association={assoc:?}"
                );
            }
        }
        text.format(
            &mut txn,
            0,
            length,
            yrs::types::Attrs::from([("bold".into(), Any::Bool(true))]),
        );
        assert!(
            text.try_single_text_item(&txn).is_none(),
            "format items invalidate the certificate"
        );
    }
    let doc = plain_read_test_doc(CLIENT);
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    let text = fragment.push_back(&mut txn, XmlTextPrelim::new("abc"));
    text.remove_range(&mut txn, 1, 1);
    text.insert(&mut txn, 1, "b");
    assert_eq!(text.try_plain_string(&txn).as_deref(), Some("abc"));
    assert!(
        text.try_single_text_item(&txn).is_none(),
        "equal visible text cannot certify contiguous IDs after delete/reinsert"
    );
}

#[test]
fn single_text_item_certificate_rejects_byte_offsets_and_merged_clients() {
    use yrs::{updates::decoder::Decode, StateVector, Text, Transact, Update, XmlTextPrelim};
    const LEFT_CLIENT: u64 = 95;
    const RIGHT_CLIENT: u64 = 96;
    const FRAGMENT: &str = "single-item-merged";
    let byte_doc = yrs::Doc::with_client_id(LEFT_CLIENT);
    let byte_root = byte_doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = byte_doc.transact_mut();
    let byte_text = byte_root.push_back(&mut txn, XmlTextPrelim::new("a😀"));
    assert!(
        byte_text.try_single_text_item(&txn).is_none(),
        "byte offsets cannot certify UTF-16 IDs"
    );
    drop(txn);

    let left = plain_read_test_doc(LEFT_CLIENT);
    let right = plain_read_test_doc(RIGHT_CLIENT);
    let left_root = left.get_or_insert_xml_fragment(FRAGMENT);
    let left_text = left_root.push_back(&mut left.transact_mut(), XmlTextPrelim::new("a😀b"));
    let base = left
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    right
        .transact_mut()
        .apply_update(Update::decode_v1(&base).unwrap())
        .unwrap();
    let right_root = right.get_or_insert_xml_fragment(FRAGMENT);
    let XmlOut::Text(right_text) = right_root.get(&right.transact(), 0).unwrap() else {
        panic!("replicated text child");
    };
    assert!(
        right_text.try_single_text_item(&right.transact()).is_some(),
        "a replicated single item retains its identity"
    );
    right_text.insert(&mut right.transact_mut(), 3, "右");
    let update = right
        .transact()
        .encode_state_as_update_v1(&left.transact().state_vector());
    left.transact_mut()
        .apply_update(Update::decode_v1(&update).unwrap())
        .unwrap();
    for (doc, text) in [(&left, &left_text), (&right, &right_text)] {
        assert_eq!(
            text.try_plain_string(&doc.transact()).as_deref(),
            Some("a😀右b")
        );
        assert!(
            text.try_single_text_item(&doc.transact()).is_none(),
            "merged client IDs must use the general anchor walk"
        );
    }
}

#[test]
fn plain_text_reads_preserve_embed_and_raw_branch_fallbacks() {
    use yrs::{Any, Array, Text, Transact, XmlTextPrelim};
    const CLIENT: u64 = 91;
    const TEXT: &str = "a😀e\u{301} עברית";
    let doc = plain_read_test_doc(CLIENT);
    let fragment = doc.get_or_insert_xml_fragment("plain-text-read");
    let array = doc.get_or_insert_array("raw-array");
    let mut txn = doc.transact_mut();
    let text = fragment.push_back(&mut txn, XmlTextPrelim::new(TEXT));
    assert_eq!(
        assert_plain_read_matches_diff("unformatted unicode", &text, &txn, 0),
        Some(TEXT.into())
    );
    let length = text.len(&txn);
    text.format(
        &mut txn,
        0,
        length,
        yrs::types::Attrs::from([
            ("bold".into(), Any::Bool(true)),
            ("link".into(), Any::String("target".into())),
        ]),
    );
    assert_plain_read_matches_diff("formatted unicode", &text, &txn, 0);
    text.insert_embed(&mut txn, 0, Any::String("prefix".into()));
    assert_eq!(
        assert_plain_read_matches_diff("string embed", &text, &txn, 1),
        Some(format!("prefix{TEXT}"))
    );
    text.remove_range(&mut txn, 0, 1);
    assert_eq!(
        assert_plain_read_matches_diff("deleted string embed", &text, &txn, 0),
        Some(TEXT.into())
    );
    text.insert_embed(&mut txn, 0, Any::Bool(true));
    assert_eq!(
        assert_plain_read_matches_diff("non-string embed", &text, &txn, 1),
        None
    );
    text.remove_range(&mut txn, 0, 1);
    assert_plain_read_matches_diff("deleted non-string embed", &text, &txn, 0);
    text.insert_embed(&mut txn, 0, yrs::MapPrelim::default());
    assert_eq!(
        assert_plain_read_matches_diff("nested shared type", &text, &txn, 1),
        None
    );
    text.remove_range(&mut txn, 0, 1);
    assert_plain_read_matches_diff("deleted shared type", &text, &txn, 0);
    let length = text.len(&txn);
    text.remove_range(&mut txn, 0, length);
    assert_eq!(
        assert_plain_read_matches_diff("deleted formatted text", &text, &txn, 0),
        Some(String::new())
    );

    array.insert_range(
        &mut txn,
        0,
        [Any::String("ignored".into()), Any::Bool(true)],
    );
    let raw = XmlTextRef::from(BranchPtr::from(<yrs::ArrayRef as AsRef<Branch>>::as_ref(
        &array,
    )));
    assert_eq!(
        assert_plain_read_matches_diff("raw non-text array items", &raw, &txn, 1),
        Some(String::new())
    );
    array.insert(&mut txn, 0, yrs::MapPrelim::default());
    assert_eq!(
        assert_plain_read_matches_diff("raw array shared type", &raw, &txn, 1),
        None
    );
    let length = array.len(&txn);
    array.remove_range(&mut txn, 0, length);
    assert_eq!(
        assert_plain_read_matches_diff("deleted raw array items", &raw, &txn, 0),
        Some(String::new())
    );
}

#[test]
fn plain_text_reads_follow_merged_formatting_and_deleted_items() {
    use yrs::{updates::decoder::Decode, Any, StateVector, Text, Transact, Update, XmlTextPrelim};
    const LEFT_CLIENT: u64 = 92;
    const RIGHT_CLIENT: u64 = 93;
    const FRAGMENT: &str = "merged-plain-text";
    let left = plain_read_test_doc(LEFT_CLIENT);
    let right = plain_read_test_doc(RIGHT_CLIENT);
    let left_root = left.get_or_insert_xml_fragment(FRAGMENT);
    let right_root = right.get_or_insert_xml_fragment(FRAGMENT);
    let left_text = {
        let mut txn = left.transact_mut();
        let text = left_root.push_back(&mut txn, XmlTextPrelim::new("a😀bcdef"));
        text.format(
            &mut txn,
            0,
            3,
            yrs::types::Attrs::from([("bold".into(), Any::Bool(true))]),
        );
        text
    };
    let base = left
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    right
        .transact_mut()
        .apply_update(Update::decode_v1(&base).unwrap())
        .unwrap();
    let XmlOut::Text(right_text) = right_root.get(&right.transact(), 0).unwrap() else {
        panic!("text root child");
    };
    {
        let mut txn = left.transact_mut();
        left_text.insert(&mut txn, 0, "左");
        left_text.remove_range(&mut txn, 1, 1);
    }
    {
        let mut txn = right.transact_mut();
        right_text.format(
            &mut txn,
            0,
            3,
            yrs::types::Attrs::from([("italic".into(), Any::Bool(true))]),
        );
        let last = right_text.len(&txn) - 1;
        right_text.remove_range(&mut txn, last, 1);
        right_text.insert(&mut txn, last, "右");
    }
    let left_update = left
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    let right_update = right
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    left.transact_mut()
        .apply_update(Update::decode_v1(&right_update).unwrap())
        .unwrap();
    right
        .transact_mut()
        .apply_update(Update::decode_v1(&left_update).unwrap())
        .unwrap();
    assert_eq!(
        assert_plain_read_matches_diff("left merged formatting", &left_text, &left.transact(), 0),
        assert_plain_read_matches_diff(
            "right merged formatting",
            &right_text,
            &right.transact(),
            0
        )
    );
    {
        let mut txn = right.transact_mut();
        let length = right_text.len(&txn);
        right_text.remove_range(&mut txn, 0, length);
        right_text.insert(&mut txn, 0, "replacement🙂");
    }
    let update = right
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    left.transact_mut()
        .apply_update(Update::decode_v1(&update).unwrap())
        .unwrap();
    assert_eq!(
        assert_plain_read_matches_diff(
            "remote full text replacement",
            &left_text,
            &left.transact(),
            0
        ),
        Some("replacement🙂".into())
    );
}
