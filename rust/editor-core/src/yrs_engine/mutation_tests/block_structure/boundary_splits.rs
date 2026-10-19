const BOUNDARY_SPLIT_REQUEST: u64 = 73_200;
const BOUNDARY_SPLIT_MAX_PARAGRAPHS: usize = 5;

fn boundary_split(at: u32) -> TypedOperation {
    TypedOperation::SplitBlock {
        at: point_for_test(at),
        node_type: "paragraph".into(),
        attrs: HashMap::new(),
    }
}

fn compile_boundary_split_transaction(
    source: &Value,
    operations: Vec<TypedOperation>,
) -> Result<(), crate::yrs_engine::OperationError> {
    let (doc, schema, limits, editing_limits, document) = diagnostic_doc(source);
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    compile_transaction_with_yrs(
        CompilationContext {
            document: &document,
            selection: None,
            schema: &schema,
            resource_limits: &limits,
            editing_limits: &editing_limits,
            document_revision: 0,
            max_length: None,
            validation_certificate: None,
        },
        TypedTransaction {
            request_id: BOUNDARY_SPLIT_REQUEST,
            base_document_revision: 0,
            origin: TransactionOrigin::LocalCommand,
            operations,
            selection_intent: SelectionIntent::UseOperationResult,
            history_policy: HistoryPolicy::Auto,
        },
        &txn,
        &fragment,
    )
    .map(|_| ())
}

#[test]
fn a_split_at_a_block_start_keeps_later_blocks_tracked_for_a_cross_block_delete() {
    const LONG_TEXT: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";
    let paragraph =
        |text: &str| json!({"type": "paragraph", "content": [{"type": "text", "text": text}]});
    let source =
        json!({"type": "doc", "content": [paragraph("a"), paragraph(LONG_TEXT), paragraph("c")]});
    let schema = tiptap_schema();
    let long_start = rendered_scalar_offset(&source, &schema, LONG_TEXT);
    let end = rendered_scalar_offset(&source, &schema, "c") + 1;
    let (actual, expected, _, _, _) = compile_and_execute(
        source,
        vec![
            boundary_split(0),
            TypedOperation::DeleteRange {
                range: range_for_test(long_start, end),
            },
        ],
    );
    assert_eq!(actual, expected);
    assert_eq!(
        actual["content"],
        json!([{"type": "paragraph"}, paragraph("a"), {"type": "paragraph"}]),
        "the split creates an empty first block and the delete removes the long block's text \
         through the end of the last block"
    );
}

#[derive(Debug, Clone)]
struct BoundaryParagraph {
    head: Option<usize>,
    tail: Option<usize>,
}

#[derive(Debug, Clone)]
enum BoundaryFollowUp {
    Delete { from: u32, to: u32 },
    Replace { from: u32, to: u32 },
    Insert { at: u32 },
    InsertBreak { at: u32 },
    Split { at: u32 },
    Join { paragraph: usize },
}

const BOUNDARY_SPLIT_MAX_FOLLOW_UPS: usize = 3;
const BOUNDARY_SPLIT_CASES: u32 = 768;
const BOUNDARY_HEAD_PROBABILITY: f64 = 0.85;
const HARD_BREAK_NODE_SIZE: u32 = 1;

fn boundary_paragraph() -> impl Strategy<Value = BoundaryParagraph> {
    (
        prop::option::weighted(BOUNDARY_HEAD_PROBABILITY, 1..5usize),
        prop::option::of(1..4usize),
    )
        .prop_map(|(head, tail)| BoundaryParagraph { head, tail })
}

fn boundary_follow_up() -> impl Strategy<Value = BoundaryFollowUp> {
    prop_oneof![
        3 => (any::<u32>(), any::<u32>()).prop_map(|(from, to)| BoundaryFollowUp::Delete { from, to }),
        2 => (any::<u32>(), any::<u32>()).prop_map(|(from, to)| BoundaryFollowUp::Replace { from, to }),
        1 => any::<u32>().prop_map(|at| BoundaryFollowUp::Insert { at }),
        1 => any::<u32>().prop_map(|at| BoundaryFollowUp::InsertBreak { at }),
        1 => any::<u32>().prop_map(|at| BoundaryFollowUp::Split { at }),
        2 => any::<usize>().prop_map(|paragraph| BoundaryFollowUp::Join { paragraph }),
    ]
}

fn boundary_head_text(index: usize, head: usize) -> String {
    format!("h{index}{}", "x".repeat(head))
}

fn boundary_split_source(paragraphs: &[BoundaryParagraph]) -> Value {
    let content = paragraphs
        .iter()
        .enumerate()
        .map(|(index, paragraph)| {
            let mut inline = Vec::new();
            if let Some(head) = paragraph.head {
                inline.push(json!({"type": "text", "text": boundary_head_text(index, head)}));
            }
            if let Some(tail) = paragraph.tail {
                inline.push(json!({"type": "hardBreak"}));
                inline
                    .push(json!({"type": "text", "text": format!("t{index}{}", "y".repeat(tail))}));
            }
            if inline.is_empty() {
                json!({"type": "paragraph"})
            } else {
                json!({"type": "paragraph", "content": inline})
            }
        })
        .collect::<Vec<_>>();
    json!({"type": "doc", "content": content})
}

fn boundary_shape_source(shape: &[(Option<usize>, Option<usize>)]) -> Value {
    boundary_split_source(
        &shape
            .iter()
            .map(|&(head, tail)| BoundaryParagraph { head, tail })
            .collect::<Vec<_>>(),
    )
}

fn paragraph_content_start(document: &Document, paragraph: usize) -> u32 {
    let preceding = document
        .root()
        .content()
        .unwrap()
        .iter()
        .take(paragraph)
        .map(Node::node_size)
        .sum::<u32>();
    preceding + crate::tables::commands::NODE_OPENING_TOKENS
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(BOUNDARY_SPLIT_CASES))]

    #[test]
    fn storage_boundary_splits_compose_with_later_operations(
        paragraphs in prop::collection::vec(boundary_paragraph(), 2..=BOUNDARY_SPLIT_MAX_PARAGRAPHS),
        split_paragraph in any::<usize>(),
        split_after_break in any::<bool>(),
        follow_ups in prop::collection::vec(boundary_follow_up(), 1..=BOUNDARY_SPLIT_MAX_FOLLOW_UPS),
    ) {
        let source = boundary_split_source(&paragraphs);
        let schema = tiptap_schema();
        let document = from_prosemirror_json(&source, &schema, UnknownTypeMode::Preserve).unwrap();
        let position_map = PositionMap::build(&document, &schema);
        let scalar = |position: u32| position_map.doc_to_scalar(position, &document);
        let total = position_map.total_scalars();
        let split_index = split_paragraph % paragraphs.len();
        let split_paragraph = &paragraphs[split_index];
        let split_start = paragraph_content_start(&document, split_index);
        let split_at = match split_paragraph.tail {
            Some(_) if split_after_break => {
                let head_len = split_paragraph.head.map_or(0, |head| {
                    u32::try_from(boundary_head_text(split_index, head).chars().count()).unwrap()
                });
                scalar(split_start + head_len + HARD_BREAK_NODE_SIZE)
            }
            _ => scalar(split_start),
        };
        let range = |from: u32, to: u32| {
            let (from, to) = (from % (total + 1), to % (total + 1));
            range_for_test(from.min(to), from.max(to))
        };
        let operations = std::iter::once(boundary_split(split_at))
            .chain(follow_ups.iter().map(|follow_up| match *follow_up {
                BoundaryFollowUp::Delete { from, to } => TypedOperation::DeleteRange { range: range(from, to) },
                BoundaryFollowUp::Replace { from, to } => TypedOperation::ReplaceRange {
                    range: range(from, to),
                    content: Fragment::from(vec![Node::text("r".into(), vec![])]),
                },
                BoundaryFollowUp::Insert { at } => TypedOperation::InsertText {
                    at: point_for_test(at % (total + 1)),
                    text: "z".into(),
                    marks: vec![],
                },
                BoundaryFollowUp::InsertBreak { at } => TypedOperation::InsertNode {
                    at: point_for_test(at % (total + 1)),
                    node: Node::void("hardBreak".into(), HashMap::new()),
                },
                BoundaryFollowUp::Split { at } => boundary_split(at % (total + 1)),
                BoundaryFollowUp::Join { paragraph } => {
                    let left = paragraph % (paragraphs.len() - 1);
                    let left_end = paragraph_content_start(&document, left)
                        + document.root().child(left).unwrap().content_size();
                    TypedOperation::JoinBlocks { at: point_for_test(scalar(left_end)) }
                }
            }))
            .collect::<Vec<_>>();
        let context = format!("{operations:?} on {source}");
        match compile_boundary_split_transaction(&source, operations.clone()) {
            Ok(()) => {
                let (actual, expected, _, _, _) = compile_and_execute(source, operations);
                prop_assert_eq!(actual, expected, "{}", context);
            }
            Err(error) => prop_assert_ne!(error.code, "ENGINE_INVARIANT_FAILED", "{}: {:?}", context, error),
        }
    }
}

#[test]
fn boundary_split_compositions_match_their_previews() {
    let paragraphs = |shape: &[(usize, Option<usize>)]| {
        boundary_shape_source(
            &shape
                .iter()
                .map(|&(head, tail)| (Some(head), tail))
                .collect::<Vec<_>>(),
        )
    };
    let delete = |from, to| TypedOperation::DeleteRange {
        range: range_for_test(from, to),
    };
    let replace = |from, to| TypedOperation::ReplaceRange {
        range: range_for_test(from, to),
        content: Fragment::from(vec![Node::text("r".into(), vec![])]),
    };
    let cases = [
        (
            "a delete from a moved block into the next block keeps a hard break suffix",
            paragraphs(&[(4, None), (3, None), (2, Some(2))]),
            vec![boundary_split(7), delete(7, 13)],
        ),
        (
            "a delete from after a moved hard break into the next block leaves a zero-width tail",
            paragraphs(&[(1, None), (1, Some(2)), (1, None), (2, None), (1, None)]),
            vec![boundary_split(4), delete(8, 13)],
        ),
        (
            "a delete of a hard break inside a moved block",
            paragraphs(&[(1, Some(3)), (3, None)]),
            vec![boundary_split(0), delete(0, 4)],
        ),
        (
            "two splits in one block insert adjacent right blocks",
            paragraphs(&[(2, Some(3)), (3, Some(3)), (3, None), (2, None)]),
            vec![boundary_split(17), boundary_split(11)],
        ),
        (
            "a split inside a moved block after a cross-block replace shifts the later delete",
            paragraphs(&[(1, Some(1)), (4, Some(3))]),
            vec![boundary_split(0), replace(0, 8), boundary_split(9)],
        ),
    ];
    for (label, source, operations) in cases {
        let (actual, expected, _, _, _) = compile_and_execute(source.clone(), operations.clone());
        assert_eq!(actual, expected, "{label}: {operations:?} on {source}");
    }
}
