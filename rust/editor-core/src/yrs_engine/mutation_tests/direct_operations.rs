use super::*;
use crate::yrs_engine::{SelectionInput, TypedCommand};

fn engine() -> YrsDocumentEngine {
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema: tiptap_schema(),
        fragment_name: "prosemirror".into(),
        initialization_mode: InitializationMode::LocalEmpty,
        resource_limits: ResourceLimits::default(),
        editing_limits: EditingLimits::default(),
        max_length: None,
        scope: None,
    })
    .unwrap();
    engine
        .import_json(TWO_PARAGRAPHS, TransactionOrigin::DocumentImport)
        .unwrap();
    engine
}

#[test]
fn one_character_insert_compiles_a_direct_mutation_action() {
    let engine = engine();
    let compiled = engine
        .compile_typed_transaction(TypedTransaction {
            request_id: 51,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalInput,
            operations: vec![TypedOperation::InsertText {
                at: RevisionedPosition {
                    offset: 1,
                    kind: EditorOffsetKind::Scalar,
                    affinity: Affinity::After,
                },
                text: "!".into(),
                marks: vec![],
            }],
            selection_intent: SelectionIntent::UseOperationResult,
            history_policy: HistoryPolicy::Auto,
        })
        .unwrap();

    assert_eq!(compiled.preview.root().text_content(), "a!lphaomega");
    assert_eq!(compiled.mutation_plan.actions.len(), 1);
    assert!(compiled.encoded_growth_bound > 0);
}

#[test]
fn every_text_and_mark_operation_executes_to_its_exact_preview() {
    let plain = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{ "type": "text", "text": "Hello" }]
        }]
    });
    let bold = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{
                "type": "text",
                "text": "Hello",
                "marks": [{ "type": "bold" }]
            }]
        }]
    });
    let link = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{
                "type": "text",
                "text": "Hello",
                "marks": [{ "type": "link", "attrs": { "href": "old" } }]
            }]
        }]
    });
    let cases = vec![
        (
            plain.clone(),
            TypedOperation::InsertText {
                at: point_for_test(2),
                text: "🙂".into(),
                marks: vec![Mark::new("bold".into(), HashMap::new())],
            },
        ),
        (
            plain.clone(),
            TypedOperation::DeleteRange {
                range: range_for_test(1, 4),
            },
        ),
        (
            plain.clone(),
            TypedOperation::ReplaceRange {
                range: range_for_test(1, 4),
                content: Fragment::from(vec![Node::text(
                    "XY".into(),
                    vec![Mark::new("italic".into(), HashMap::new())],
                )]),
            },
        ),
        (
            plain.clone(),
            TypedOperation::AddMark {
                range: range_for_test(1, 4),
                mark: Mark::new("bold".into(), HashMap::new()),
            },
        ),
        (
            bold,
            TypedOperation::RemoveMark {
                range: range_for_test(1, 4),
                mark_type: "bold".into(),
            },
        ),
        (
            link,
            TypedOperation::ReplaceMark {
                range: range_for_test(1, 4),
                mark: Mark::new(
                    "link".into(),
                    HashMap::from([("href".into(), Value::String("new".into()))]),
                ),
            },
        ),
    ];

    for (source, operation) in cases {
        compile_and_execute(source, vec![operation]);
    }
}

#[test]
fn same_parent_structural_delete_accepts_immediate_insert_into_the_surviving_paragraph() {
    const LEFT: &str = "L😀eft";
    const RIGHT: &str = "R😀ight";
    const INSERTED: &str = "again";
    const NEIGHBOR: &str = "untouched";
    let source = json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [
            {"type": "text", "text": LEFT, "marks": [{"type": "bold"}]},
            {"type": "hardBreak"},
            {"type": "text", "text": RIGHT, "marks": [{"type": "italic"}]}
        ]},
        {"type": "paragraph", "content": [{"type": "text", "text": NEIGHBOR}]}
    ]});
    let schema = tiptap_schema();
    let left = rendered_scalar_offset(&source, &schema, LEFT);
    let right = rendered_scalar_offset(&source, &schema, RIGHT);
    let end = right + u32::try_from(RIGHT.chars().count()).unwrap();
    let left_end = left + u32::try_from(LEFT.chars().count()).unwrap();
    for (label, from, to, expected_text) in [
        ("all inline children", left, end, INSERTED.to_owned()),
        (
            "prefix through hard break",
            left,
            right,
            format!("{INSERTED}{RIGHT}"),
        ),
        (
            "hard break through suffix",
            left_end,
            end,
            format!("{LEFT}{INSERTED}"),
        ),
        (
            "hard break only",
            left_end,
            right,
            format!("{LEFT}{INSERTED}{RIGHT}"),
        ),
    ] {
        let (actual, expected, _, _, _) = compile_and_execute(
            source.clone(),
            vec![
                TypedOperation::DeleteRange {
                    range: range_for_test(from, to),
                },
                TypedOperation::InsertText {
                    at: point_for_test(from),
                    text: INSERTED.into(),
                    marks: vec![],
                },
            ],
        );
        assert_eq!(
            actual, expected,
            "{label}: live XML must match the compiled preview"
        );
        let document = from_prosemirror_json(&actual, &schema, UnknownTypeMode::Preserve).unwrap();
        assert_eq!(
            document
                .root()
                .content()
                .unwrap()
                .child(0)
                .unwrap()
                .text_content(),
            expected_text,
            "{label}"
        );
        assert_eq!(
            actual["content"][1], source["content"][1],
            "{label}: adjacent paragraph changed"
        );
    }
}

#[test]
fn same_parent_structural_replacement_accepts_follow_up_insert_and_split() {
    const LEFT: &str = "left";
    const RIGHT: &str = "right";
    const INSERTED: &str = "again";
    const FOLLOW_UP: &str = "!";
    let source = json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [
            {"type": "text", "text": LEFT},
            {"type": "hardBreak"},
            {"type": "text", "text": RIGHT}
        ]},
        {"type": "paragraph", "content": [{"type": "text", "text": "untouched"}]}
    ]});
    let schema = tiptap_schema();
    let from = rendered_scalar_offset(&source, &schema, LEFT);
    let to = rendered_scalar_offset(&source, &schema, RIGHT)
        + u32::try_from(RIGHT.chars().count()).unwrap();
    let replacement = || TypedOperation::ReplaceRange {
        range: range_for_test(from, to),
        content: Fragment::from(vec![Node::text(INSERTED.into(), vec![])]),
    };
    let (inserted, _, _, _, _) = compile_and_execute(
        source.clone(),
        vec![
            replacement(),
            TypedOperation::InsertText {
                at: point_for_test(to),
                text: FOLLOW_UP.into(),
                marks: vec![],
            },
        ],
    );
    assert_eq!(
        inserted["content"][0]["content"][0]["text"],
        format!("{INSERTED}{FOLLOW_UP}")
    );
    assert_eq!(inserted["content"][1], source["content"][1]);
    let (split, _, _, _, _) = compile_and_execute(
        source.clone(),
        vec![
            replacement(),
            TypedOperation::SplitBlock {
                at: point_for_test(to),
                node_type: "paragraph".into(),
                attrs: HashMap::new(),
            },
        ],
    );
    assert_eq!(
        split["content"],
        json!([
            {"type": "paragraph", "content": [{"type": "text", "text": INSERTED}]},
            {"type": "paragraph"},
            source["content"][1].clone()
        ])
    );
}

#[test]
fn partial_text_endpoints_around_an_atom_accept_replacement_continuation() {
    const LEFT: &str = "L😀eft";
    const RIGHT: &str = "R😀ight";
    const PREFIX: &str = "L😀";
    const SUFFIX: &str = "ight";
    const REPLACEMENT: &str = "again";
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": LEFT, "marks": [{"type": "bold"}]},
        {"type": "hardBreak"},
        {"type": "text", "text": RIGHT, "marks": [{"type": "italic"}]}
    ]}, {"type": "paragraph", "content": [{"type": "text", "text": "untouched"}]}]});
    let schema = tiptap_schema();
    let from = rendered_scalar_offset(&source, &schema, LEFT)
        + u32::try_from(PREFIX.chars().count()).unwrap();
    let to = rendered_scalar_offset(&source, &schema, SUFFIX);
    for replacement in [false, true] {
        let mut operations = vec![if replacement {
            TypedOperation::ReplaceRange {
                range: range_for_test(from, to),
                content: Fragment::from(vec![Node::text(REPLACEMENT.into(), vec![])]),
            }
        } else {
            TypedOperation::DeleteRange {
                range: range_for_test(from, to),
            }
        }];
        operations.push(TypedOperation::InsertText {
            at: point_for_test(to),
            text: "!".into(),
            marks: vec![],
        });
        let (actual, _, _, _, _) = compile_and_execute(source.clone(), operations);
        let document = from_prosemirror_json(&actual, &schema, UnknownTypeMode::Preserve).unwrap();
        assert_eq!(
            document
                .root()
                .content()
                .unwrap()
                .child(0)
                .unwrap()
                .text_content(),
            format!(
                "{PREFIX}{}!{SUFFIX}",
                if replacement { REPLACEMENT } else { "" }
            )
        );
        assert_eq!(actual["content"][1], source["content"][1]);
    }
}

#[test]
fn mixed_inline_replacement_accepts_typing_in_its_prepared_suffix() {
    const LEFT: &str = "left";
    const RIGHT: &str = "right";
    const REPLACED_SUFFIX: &str = "new";
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": LEFT}, {"type": "hardBreak"}, {"type": "text", "text": RIGHT}
    ]}, {"type": "paragraph", "content": [{"type": "text", "text": "untouched"}]}]});
    let schema = tiptap_schema();
    let from = rendered_scalar_offset(&source, &schema, LEFT);
    let to = rendered_scalar_offset(&source, &schema, RIGHT)
        + u32::try_from(RIGHT.chars().count()).unwrap();
    let (actual, _, _, _, _) = compile_and_execute(
        source.clone(),
        vec![
            TypedOperation::ReplaceRange {
                range: range_for_test(from, to),
                content: Fragment::from(vec![
                    Node::void("hardBreak".into(), HashMap::new()),
                    Node::text(REPLACED_SUFFIX.into(), vec![]),
                ]),
            },
            TypedOperation::InsertText {
                at: point_for_test(to),
                text: "!".into(),
                marks: vec![],
            },
        ],
    );
    assert_eq!(
        actual["content"][0]["content"],
        json!([
            {"type": "hardBreak"}, {"type": "text", "text": format!("{REPLACED_SUFFIX}!")}
        ])
    );
    assert_eq!(actual["content"][1], source["content"][1]);
}

#[test]
fn mixed_partial_replacement_accepts_typing_and_split_before_the_surviving_suffix() {
    const LEFT: &str = "left";
    const RIGHT: &str = "right";
    const PREFIX: &str = "le";
    const SUFFIX: &str = "ght";
    const REPLACEMENT: &str = "new";
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": LEFT}, {"type": "hardBreak"}, {"type": "text", "text": RIGHT}
    ]}, {"type": "paragraph", "content": [{"type": "text", "text": "untouched"}]}]});
    let schema = tiptap_schema();
    let from = rendered_scalar_offset(&source, &schema, LEFT)
        + u32::try_from(PREFIX.chars().count()).unwrap();
    let to = rendered_scalar_offset(&source, &schema, SUFFIX);
    for split in [false, true] {
        let mut operations = vec![
            TypedOperation::ReplaceRange {
                range: range_for_test(from, to),
                content: Fragment::from(vec![
                    Node::void("hardBreak".into(), HashMap::new()),
                    Node::text(REPLACEMENT.into(), vec![]),
                ]),
            },
            if split {
                TypedOperation::SplitBlock {
                    at: point_for_test(to),
                    node_type: "paragraph".into(),
                    attrs: HashMap::new(),
                }
            } else {
                TypedOperation::InsertText {
                    at: point_for_test(to),
                    text: "!".into(),
                    marks: vec![],
                }
            },
        ];
        if split {
            operations.push(TypedOperation::InsertText {
                at: point_for_test(to),
                text: "!".into(),
                marks: vec![],
            });
        }
        let (actual, _, _, _, _) = compile_and_execute(source.clone(), operations);
        let first = if split {
            REPLACEMENT.to_owned()
        } else {
            format!("{REPLACEMENT}!{SUFFIX}")
        };
        assert_eq!(
            actual["content"][0]["content"],
            json!([
                {"type": "text", "text": PREFIX}, {"type": "hardBreak"}, {"type": "text", "text": first}
            ])
        );
        if split {
            assert_eq!(
                actual["content"][1],
                json!({"type": "paragraph", "content": [{"type": "text", "text": format!("!{SUFFIX}")}]})
            );
        }
        assert_eq!(
            actual["content"][if split { 2 } else { 1 }],
            source["content"][1]
        );
    }
}

#[test]
fn mixed_replacement_can_be_deleted_by_base_affinities_and_typed_into_again() {
    const LEFT: &str = "left";
    const RIGHT: &str = "right";
    const REPLACEMENT: &str = "new";
    const FINAL_TEXT: &str = "again";
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": LEFT}, {"type": "hardBreak"}, {"type": "text", "text": RIGHT}
    ]}, {"type": "paragraph", "content": [{"type": "text", "text": "untouched"}]}]});
    let schema = tiptap_schema();
    let from = rendered_scalar_offset(&source, &schema, LEFT);
    let to = rendered_scalar_offset(&source, &schema, RIGHT)
        + u32::try_from(RIGHT.chars().count()).unwrap();
    let mut second_range = range_for_test(from, to);
    second_range.from.affinity = Affinity::Before;
    let (actual, _, _, _, _) = compile_and_execute(
        source.clone(),
        vec![
            TypedOperation::ReplaceRange {
                range: range_for_test(from, to),
                content: Fragment::from(vec![
                    Node::void("hardBreak".into(), HashMap::new()),
                    Node::text(REPLACEMENT.into(), vec![]),
                ]),
            },
            TypedOperation::DeleteRange {
                range: second_range,
            },
            TypedOperation::InsertText {
                at: point_for_test(from),
                text: FINAL_TEXT.into(),
                marks: vec![],
            },
        ],
    );
    assert_eq!(
        actual["content"][0],
        json!({"type": "paragraph", "content": [{"type": "text", "text": FINAL_TEXT}]})
    );
    assert_eq!(actual["content"][1], source["content"][1]);
}

#[test]
fn inserted_atom_suffix_accepts_a_later_split_and_typing_in_one_transaction() {
    const TEXT: &str = "abcdef";
    const PREFIX: &str = "ab";
    const RETAINED_SUFFIX: &str = "c";
    const RIGHT: &str = "def";
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": TEXT}
    ]}, {"type": "paragraph", "content": [{"type": "text", "text": "untouched"}]}]});
    let schema = tiptap_schema();
    let start = rendered_scalar_offset(&source, &schema, TEXT);
    let insert_at = start + u32::try_from(PREFIX.chars().count()).unwrap();
    let split_at = insert_at + u32::try_from(RETAINED_SUFFIX.chars().count()).unwrap();
    let (actual, _, _, _, _) = compile_and_execute(
        source.clone(),
        vec![
            TypedOperation::InsertNode {
                at: point_for_test(insert_at),
                node: Node::void("hardBreak".into(), HashMap::new()),
            },
            TypedOperation::SplitBlock {
                at: point_for_test(split_at),
                node_type: "paragraph".into(),
                attrs: HashMap::new(),
            },
            TypedOperation::InsertText {
                at: point_for_test(split_at),
                text: "!".into(),
                marks: vec![],
            },
        ],
    );
    assert_eq!(
        actual["content"],
        json!([
            {"type": "paragraph", "content": [{"type": "text", "text": PREFIX}, {"type": "hardBreak"}, {"type": "text", "text": RETAINED_SUFFIX}]},
            {"type": "paragraph", "content": [{"type": "text", "text": format!("!{RIGHT}")}]},
            source["content"][1].clone()
        ])
    );
}

#[test]
fn deleting_an_inserted_atom_remaps_its_surviving_prepared_suffix_before_split() {
    const TEXT: &str = "abcdef";
    const PREFIX: &str = "ab";
    const RETAINED_SUFFIX: &str = "c";
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": TEXT}]}]});
    let insert_at = u32::try_from(PREFIX.chars().count()).unwrap();
    let split_at = insert_at + u32::try_from(RETAINED_SUFFIX.chars().count()).unwrap();
    let mut inserted_atom = range_for_test(insert_at, insert_at);
    inserted_atom.from.affinity = Affinity::Before;
    let (actual, _, _, _, _) = compile_and_execute(
        source,
        vec![
            TypedOperation::InsertNode {
                at: point_for_test(insert_at),
                node: Node::void("hardBreak".into(), HashMap::new()),
            },
            TypedOperation::DeleteRange {
                range: inserted_atom,
            },
            TypedOperation::SplitBlock {
                at: point_for_test(split_at),
                node_type: "paragraph".into(),
                attrs: HashMap::new(),
            },
            TypedOperation::InsertText {
                at: point_for_test(split_at),
                text: "!".into(),
                marks: vec![],
            },
        ],
    );
    assert_eq!(
        actual["content"],
        json!([
            {"type": "paragraph", "content": [{"type": "text", "text": format!("{PREFIX}{RETAINED_SUFFIX}")}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "!def"}]}
        ])
    );
}

#[test]
fn structural_delete_of_prepared_and_existing_children_keeps_physical_indices_live() {
    const PREFIX: &str = "ab";
    const LEFT: &str = "abc";
    const RIGHT: &str = "def";
    const DELETED_PREFIX: &str = "d";
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": LEFT}, {"type": "hardBreak"}, {"type": "text", "text": RIGHT}
    ]}, {"type": "paragraph", "content": [{"type": "text", "text": "untouched"}]}]});
    let schema = tiptap_schema();
    let insert_at = u32::try_from(PREFIX.chars().count()).unwrap();
    let right_start = rendered_scalar_offset(&source, &schema, RIGHT);
    let to = right_start + u32::try_from(DELETED_PREFIX.chars().count()).unwrap();
    let (actual, _, _, _, _) = compile_and_execute(
        source.clone(),
        vec![
            TypedOperation::InsertNode {
                at: point_for_test(insert_at),
                node: Node::void("hardBreak".into(), HashMap::new()),
            },
            TypedOperation::DeleteRange {
                range: range_for_test(insert_at, to),
            },
            TypedOperation::InsertText {
                at: point_for_test(to),
                text: "!".into(),
                marks: vec![],
            },
        ],
    );
    assert_eq!(
        actual["content"][0]["content"],
        json!([
            {"type": "text", "text": PREFIX}, {"type": "hardBreak"}, {"type": "text", "text": "!ef"}
        ])
    );
    assert_eq!(actual["content"][1], source["content"][1]);
}

#[test]
fn partial_cross_paragraph_deletion_preserves_inline_suffixes_and_compound_continuations() {
    const LEFT: &str = "abc";
    const LEFT_TAIL: &str = "def";
    const RIGHT: &str = "ghi";
    const RIGHT_TAIL: &str = "jkl";
    const MIDDLE: &str = "selected middle";
    const NEIGHBOR: &str = "untouched";
    const REPLACEMENT: &str = "X";
    const PROBE: &str = "!";
    const PARTIAL_OFFSET: u32 = 1;
    let marked =
        |text: &str, mark: &str| json!({"type": "text", "text": text, "marks": [{"type": mark}]});
    let source = json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [marked(LEFT, "bold"), {"type": "hardBreak"}, marked(LEFT_TAIL, "italic")]},
        {"type": "paragraph", "content": [{"type": "text", "text": MIDDLE}]},
        {"type": "paragraph", "content": [marked(RIGHT, "bold"), {"type": "hardBreak"}, marked(RIGHT_TAIL, "italic")]},
        {"type": "paragraph", "content": [{"type": "text", "text": NEIGHBOR}]}
    ]});
    let schema = tiptap_schema();
    for (from_needle, left) in [
        (LEFT, vec![marked("a", "bold")]),
        (
            LEFT_TAIL,
            vec![
                marked(LEFT, "bold"),
                json!({"type": "hardBreak"}),
                marked("d", "italic"),
            ],
        ),
    ] {
        for (to_needle, right) in [
            (
                RIGHT,
                vec![
                    marked("hi", "bold"),
                    json!({"type": "hardBreak"}),
                    marked(RIGHT_TAIL, "italic"),
                ],
            ),
            (RIGHT_TAIL, vec![marked("kl", "italic")]),
        ] {
            let from = rendered_scalar_offset(&source, &schema, from_needle) + PARTIAL_OFFSET;
            let to = rendered_scalar_offset(&source, &schema, to_needle) + PARTIAL_OFFSET;
            for replace in [false, true] {
                for split in [false, true] {
                    let label =
                        format!("{from_needle}..{to_needle}/replace={replace}/split={split}");
                    let mut operations = vec![if replace {
                        TypedOperation::ReplaceRange {
                            range: range_for_test(from, to),
                            content: Fragment::from(vec![Node::text(REPLACEMENT.into(), vec![])]),
                        }
                    } else {
                        TypedOperation::DeleteRange {
                            range: range_for_test(from, to),
                        }
                    }];
                    if split {
                        operations.push(TypedOperation::SplitBlock {
                            at: point_for_test(to),
                            node_type: "paragraph".into(),
                            attrs: HashMap::new(),
                        });
                    }
                    operations.push(TypedOperation::InsertText {
                        at: point_for_test(to),
                        text: PROBE.into(),
                        marks: vec![],
                    });
                    let (actual, _, _, _, _) = compile_and_execute(source.clone(), operations);
                    let mut expected_left = left.clone();
                    let inserted = format!(
                        "{}{}",
                        if replace { REPLACEMENT } else { "" },
                        if split { "" } else { PROBE }
                    );
                    if !inserted.is_empty() {
                        expected_left.push(json!({"type": "text", "text": inserted}));
                    }
                    let mut expected = vec![json!({"type": "paragraph", "content": expected_left})];
                    if split {
                        let mut expected_right = vec![json!({"type": "text", "text": PROBE})];
                        expected_right.extend(right.clone());
                        expected.push(json!({"type": "paragraph", "content": expected_right}));
                    } else {
                        expected[0]["content"]
                            .as_array_mut()
                            .unwrap()
                            .extend(right.clone());
                    }
                    expected.push(source["content"][3].clone());
                    assert_eq!(
                        actual["content"],
                        json!(expected),
                        "{label}: selected content or retained inline structure differs"
                    );
                }
            }
        }
    }
}

#[test]
fn selected_marked_hard_break_content_accepts_repeated_prepared_block_splits() {
    const LEFT: &str = "left";
    const RIGHT: &str = "right";
    const NEIGHBOR: &str = "untouched";
    const START: u32 = 0;
    let source = json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [
            {"type": "text", "text": LEFT, "marks": [{"type": "bold"}]},
            {"type": "hardBreak"}, {"type": "text", "text": RIGHT}
        ]},
        {"type": "paragraph", "content": [{"type": "text", "text": NEIGHBOR}]}
    ]});
    let end = rendered_scalar_offset(&source, &tiptap_schema(), RIGHT)
        + u32::try_from(RIGHT.chars().count()).unwrap();
    let mut operations = vec![TypedOperation::DeleteRange {
        range: range_for_test(START, end),
    }];
    for (index, text) in ["one", "two", "three"].iter().enumerate() {
        if index > 0 {
            operations.push(TypedOperation::SplitBlock {
                at: point_for_test(START),
                node_type: "paragraph".into(),
                attrs: HashMap::new(),
            });
        }
        operations.push(TypedOperation::InsertText {
            at: point_for_test(START),
            text: (*text).into(),
            marks: vec![Mark::new("bold".into(), HashMap::new())],
        });
    }
    let (actual, _, _, _, _) = compile_and_execute(source.clone(), operations);
    assert_eq!(
        actual["content"],
        json!([
            {"type": "paragraph", "content": [{"type": "text", "text": "one", "marks": [{"type": "bold"}]}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "two", "marks": [{"type": "bold"}]}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "three", "marks": [{"type": "bold"}]}]},
            source["content"][1].clone()
        ])
    );
}

#[test]
fn selected_inline_children_accept_three_line_native_text_replacement() {
    const SELECT_REQUEST_ID: u64 = 751;
    const REPLACE_REQUEST_ID: u64 = 752;
    let source = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": "left"}, {"type": "hardBreak"}, {"type": "text", "text": "right"}
    ]}]});
    let mut editor = engine();
    editor
        .import_json(&source.to_string(), TransactionOrigin::DocumentImport)
        .unwrap();
    let end = editor.position_map().unwrap().total_scalars();
    let mut head = point_for_test(end);
    head.affinity = Affinity::Before;
    editor
        .apply_typed_transaction(TypedTransaction {
            request_id: SELECT_REQUEST_ID,
            base_document_revision: editor.revision(),
            origin: TransactionOrigin::LocalInput,
            operations: vec![],
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point_for_test(0),
                head,
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap();
    editor
        .apply_command(
            REPLACE_REQUEST_ID,
            TypedCommand::ReplaceSelectionText {
                text: "first\nsecond\nthird".into(),
            },
        )
        .unwrap();
    assert_eq!(
        editor.document_json().unwrap()["content"],
        json!([
            {"type": "paragraph", "content": [{"type": "text", "text": "first"}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "second"}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "third"}]}
        ])
    );
}

#[test]
fn empty_textblocks_create_direct_text_targets_for_insert_and_collapsed_replace() {
    let empty = json!({
        "type": "doc",
        "content": [{ "type": "paragraph" }]
    });
    let inserted = compile_and_execute(
        empty.clone(),
        vec![TypedOperation::InsertText {
            at: point_for_test(1),
            text: "hello".into(),
            marks: vec![Mark::new("bold".into(), HashMap::new())],
        }],
    );
    assert_eq!(inserted.0["content"][0]["content"][0]["text"], "hello");

    let replaced = compile_and_execute(
        empty,
        vec![TypedOperation::ReplaceRange {
            range: range_for_test(1, 1),
            content: Fragment::from(vec![Node::text("world".into(), vec![])]),
        }],
    );
    assert_eq!(replaced.0["content"][0]["content"][0]["text"], "world");
}

#[test]
fn created_text_target_executes_multi_piece_replacement_and_follow_up_edits() {
    let empty = json!({
        "type": "doc",
        "content": [{ "type": "paragraph" }]
    });
    let pieces = Fragment::from(vec![
        Node::text("ab".into(), vec![Mark::new("bold".into(), HashMap::new())]),
        Node::text(
            "cd".into(),
            vec![Mark::new("italic".into(), HashMap::new())],
        ),
    ]);
    let actual = compile_and_execute(
        empty,
        vec![TypedOperation::ReplaceRange {
            range: range_for_test(1, 1),
            content: pieces,
        }],
    );
    assert_eq!(actual.0["content"][0]["content"][0]["text"], "ab");
    assert_eq!(actual.0["content"][0]["content"][1]["text"], "cd");

    let source = json!({
        "type": "doc",
        "content": [{ "type": "paragraph" }]
    });
    let (doc, schema, limits, _editing_limits, document) = diagnostic_doc(&source);
    let plan = {
        let txn = doc.transact();
        let fragment = txn.get_xml_fragment("prosemirror").unwrap();
        let mut compiler = super::mutation::MutationCompiler::new(
            83,
            &txn,
            &fragment,
            &schema,
            1_000,
            limits.max_input_bytes,
            0,
        )
        .unwrap();
        compiler
            .insert(0, 1, "abcd", &[])
            .and_then(|_| compiler.insert(1, 3, "X", &[]))
            .and_then(|_| compiler.delete(2, 2, 3, &[]))
            .and_then(|_| {
                compiler.format(
                    3,
                    1,
                    5,
                    &[1, 5],
                    super::mutation::mark_attr(&Mark::new("bold".into(), HashMap::new())),
                )
            })
            .unwrap();
        let plan = compiler.finish(Some(3)).unwrap();
        preflight_mutation_plan(83, &plan, &txn).unwrap();
        plan
    };
    assert!(matches!(
        plan.actions.first(),
        Some(YrsMutationAction::CreateText { follow_up, .. }) if follow_up.len() == 3
    ));
    {
        let mut txn = doc.transact_mut();
        execute_mutation_plan(plan, &mut txn);
    }
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    let actual = YrsDocumentCodec::new(&schema, &limits)
        .read_json(&fragment, &txn)
        .unwrap();
    let actual = from_prosemirror_json(&actual, &schema, UnknownTypeMode::Preserve).unwrap();
    assert_eq!(actual.root().text_content(), "aXcd");
    assert_eq!(document.root().text_content(), "");
    let marks = actual
        .root()
        .content()
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .content()
        .unwrap();
    assert!(marks
        .iter()
        .all(|node| node.marks().iter().any(|mark| mark.mark_type() == "bold")));
}

#[test]
fn atom_only_textblock_supports_both_gaps_and_adjacent_opaque_atoms() {
    let source = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{ "type": "hardBreak" }]
        }]
    });
    let before = compile_and_execute(
        source.clone(),
        vec![TypedOperation::InsertText {
            at: point_for_test(0),
            text: "before".into(),
            marks: vec![],
        }],
    );
    assert_eq!(before.0["content"][0]["content"][0]["text"], "before");
    assert_eq!(before.0["content"][0]["content"][1]["type"], "hardBreak");

    let after = compile_and_execute(
        source.clone(),
        vec![TypedOperation::InsertText {
            at: point_for_test(1),
            text: "after".into(),
            marks: vec![],
        }],
    );
    assert_eq!(after.0["content"][0]["content"][0]["type"], "hardBreak");
    assert_eq!(after.0["content"][0]["content"][1]["text"], "after");

    let both = compile_and_execute(
        source.clone(),
        vec![
            TypedOperation::InsertText {
                at: point_for_test(0),
                text: "L".into(),
                marks: vec![],
            },
            TypedOperation::InsertText {
                at: point_for_test(1),
                text: "R".into(),
                marks: vec![],
            },
        ],
    );
    assert_eq!(both.0["content"][0]["content"][0]["text"], "L");
    assert_eq!(both.0["content"][0]["content"][1]["type"], "hardBreak");
    assert_eq!(both.0["content"][0]["content"][2]["text"], "R");

    let mention_source = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{
                "type": "mention",
                "attrs": { "id": "user-1", "label": "Alice" }
            }]
        }]
    });
    let (mention_doc, mention_schema, mention_limits, _, _) = diagnostic_doc(&mention_source);
    let mention_plan = {
        let txn = mention_doc.transact();
        let fragment = txn.get_xml_fragment("prosemirror").unwrap();
        let mut compiler = super::mutation::MutationCompiler::new(
            87,
            &txn,
            &fragment,
            &mention_schema,
            1_000,
            mention_limits.max_input_bytes,
            0,
        )
        .unwrap();
        let after_mention = compiler
            .target_positions_for_test()
            .unwrap()
            .last()
            .unwrap()
            .0;
        compiler.insert(0, after_mention, "tail", &[]).unwrap();
        let plan = compiler.finish(Some(0)).unwrap();
        preflight_mutation_plan(87, &plan, &txn).unwrap();
        plan
    };
    {
        let mut txn = mention_doc.transact_mut();
        execute_mutation_plan(mention_plan, &mut txn);
    }
    let txn = mention_doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror").unwrap();
    let mention = YrsDocumentCodec::new(&mention_schema, &mention_limits)
        .read_json(&fragment, &txn)
        .unwrap();
    assert_eq!(mention["content"][0]["content"][0]["type"], "mention");
    assert_eq!(mention["content"][0]["content"][1]["text"], "tail");
}

include!("direct_operations/structural_insert.rs");

include!("direct_operations/insert_positions.rs");

include!("direct_operations/opaque_validation.rs");

include!("direct_operations/opaque_limits.rs");

include!("direct_operations/attributes.rs");
