fn hard_break() -> Node {
    Node::void("hardBreak".into(), HashMap::new())
}

fn delete_for_test(from: u32, to: u32) -> TypedOperation {
    TypedOperation::DeleteRange {
        range: range_for_test(from, to),
    }
}

fn join_for_test(at: u32) -> TypedOperation {
    TypedOperation::JoinBlocks {
        at: point_for_test(at),
    }
}

fn hard_break_insert_for_test(at: u32) -> TypedOperation {
    TypedOperation::InsertNode {
        at: point_for_test(at),
        node: hard_break(),
    }
}

fn type_for_test(at: u32, text: &str) -> TypedOperation {
    TypedOperation::InsertText {
        at: point_for_test(at),
        text: text.into(),
        marks: vec![],
    }
}

fn assert_composition_matches_preview(
    label: &str,
    source: Value,
    operations: Vec<TypedOperation>,
    content: Value,
) {
    let context = format!("{label}: {operations:?} on {source}");
    let (actual, expected, _, _, _) = compile_and_execute(source, operations);
    assert_eq!(actual, expected, "{context}");
    assert_eq!(actual["content"], content, "{context}");
}

fn text_json(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn hard_break_json() -> Value {
    json!({"type": "hardBreak"})
}

fn paragraph_json(inline: &[Value]) -> Value {
    if inline.is_empty() {
        json!({"type": "paragraph"})
    } else {
        json!({"type": "paragraph", "content": inline})
    }
}

#[test]
fn a_join_merges_blocks_created_earlier_in_the_transaction_through_their_storage() {
    assert_composition_matches_preview(
        "a join into the block a boundary split created",
        boundary_shape_source(&[(Some(2), None), (Some(1), None)]),
        vec![boundary_split(0), join_for_test(4)],
        json!([paragraph_json(&[]), paragraph_json(&[text_json("h0xxh1x")])]),
    );
    assert_composition_matches_preview(
        "a delete after the join targets the shifted blocks",
        boundary_shape_source(&[(Some(2), Some(3)), (Some(2), None), (Some(1), Some(3))]),
        vec![
            boundary_split(16),
            join_for_test(15),
            delete_for_test(5, 11),
        ],
        json!([
            paragraph_json(&[text_json("h0xx"), hard_break_json(), text_json("h1xx")]),
            paragraph_json(&[text_json("h2x"), hard_break_json(), text_json("t2yyy")])
        ]),
    );
}

#[test]
fn a_cross_block_delete_keeps_the_retained_text_current_for_an_inline_node_insert() {
    assert_composition_matches_preview(
        "a hard break splits the retained text that received the joined suffix",
        boundary_shape_source(&[(Some(2), None), (Some(1), None)]),
        vec![delete_for_test(3, 5), hard_break_insert_for_test(1)],
        json!([paragraph_json(&[
            text_json("h"),
            hard_break_json(),
            text_json("0xh1x")
        ])]),
    );
}

#[test]
fn text_typed_into_a_textless_block_stays_laid_out_for_later_structural_operations() {
    let source = boundary_shape_source(&[(None, None), (Some(3), None)]);
    assert_composition_matches_preview(
        "a split in the next block",
        source.clone(),
        vec![type_for_test(0, "a"), boundary_split(4)],
        json!([
            paragraph_json(&[text_json("a")]),
            paragraph_json(&[text_json("h1")]),
            paragraph_json(&[text_json("xxx")])
        ]),
    );
    assert_composition_matches_preview(
        "a delete from the typed text into the next block",
        source,
        vec![type_for_test(0, "a"), delete_for_test(1, 3)],
        json!([paragraph_json(&[text_json("a1xxx")])]),
    );
}

#[test]
fn an_emptied_prepared_text_does_not_widen_its_batch_for_a_later_merge() {
    assert_composition_matches_preview(
        "a delete empties the trailing prepared text before merging the next block",
        boundary_shape_source(&[
            (Some(2), None),
            (None, Some(2)),
            (Some(1), Some(2)),
            (Some(2), Some(1)),
            (Some(1), None),
        ]),
        vec![
            boundary_split(25),
            TypedOperation::ReplaceRange {
                range: range_for_test(0, 11),
                content: Fragment::from(vec![Node::text("r".into(), vec![])]),
            },
            delete_for_test(15, 20),
        ],
        json!([
            paragraph_json(&[
                text_json("rh2x"),
                hard_break_json(),
                text_json("h3xx"),
                hard_break_json()
            ]),
            paragraph_json(&[text_json("t3y")]),
            paragraph_json(&[text_json("h4x")])
        ]),
    );
}

#[test]
fn a_second_inline_node_before_the_first_in_one_text_folds_into_its_batch() {
    assert_composition_matches_preview(
        "two hard breaks split one text right to left",
        boundary_shape_source(&[(Some(4), None)]),
        vec![hard_break_insert_for_test(4), hard_break_insert_for_test(1)],
        json!([paragraph_json(&[
            text_json("h"),
            hard_break_json(),
            text_json("0xx"),
            hard_break_json(),
            text_json("xx")
        ])]),
    );
}

#[test]
fn an_emptied_prepared_text_keeps_later_prepared_siblings_on_their_semantic_paths() {
    assert_composition_matches_preview(
        "a hard break inserted before an emptied text in a created block",
        boundary_shape_source(&[(Some(1), Some(3)), (Some(4), None)]),
        vec![
            boundary_split(0),
            delete_for_test(0, 3),
            hard_break_insert_for_test(0),
        ],
        json!([
            paragraph_json(&[]),
            paragraph_json(&[hard_break_json(), hard_break_json(), text_json("t0yyy")]),
            paragraph_json(&[text_json("h1xxxx")])
        ]),
    );
}

#[test]
fn a_mixed_suffix_merges_its_leading_text_into_the_retained_text() {
    assert_composition_matches_preview(
        "a second join appends after a mixed suffix",
        boundary_shape_source(&[(Some(3), None), (Some(2), Some(2)), (None, Some(3))]),
        vec![join_for_test(5), join_for_test(15)],
        json!([paragraph_json(&[
            text_json("h0xxxh1xx"),
            hard_break_json(),
            text_json("t1yy"),
            hard_break_json(),
            text_json("t2yyy")
        ])]),
    );
    assert_composition_matches_preview(
        "a hard break at the merge point of a mixed suffix",
        boundary_shape_source(&[
            (Some(1), None),
            (Some(2), None),
            (Some(1), None),
            (Some(4), Some(3)),
        ]),
        vec![
            boundary_split(0),
            delete_for_test(5, 13),
            hard_break_insert_for_test(5),
        ],
        json!([
            paragraph_json(&[]),
            paragraph_json(&[text_json("h0x")]),
            paragraph_json(&[
                text_json("h"),
                hard_break_json(),
                text_json("h3xxxx"),
                hard_break_json(),
                text_json("t3yyy")
            ])
        ]),
    );
}

#[test]
fn a_boundary_split_reads_the_current_length_of_refilled_text() {
    assert_composition_matches_preview(
        "a split at the start of text refilled by a cross-block delete",
        boundary_shape_source(&[
            (Some(4), Some(2)),
            (Some(4), None),
            (None, None),
            (Some(1), None),
            (Some(3), None),
        ]),
        vec![
            boundary_split(20),
            delete_for_test(0, 12),
            boundary_split(0),
        ],
        json!([
            paragraph_json(&[]),
            paragraph_json(&[text_json("h1xxxx")]),
            paragraph_json(&[]),
            paragraph_json(&[]),
            paragraph_json(&[text_json("h3x")]),
            paragraph_json(&[text_json("h4xxx")])
        ]),
    );
}

#[test]
fn actions_inside_a_deleted_block_are_dropped_even_after_their_text_moved() {
    let source = boundary_shape_source(&[
        (Some(4), Some(2)),
        (None, None),
        (Some(3), None),
        (Some(4), None),
        (Some(2), Some(2)),
    ]);
    let splits = [boundary_split(0), boundary_split(33), boundary_split(31)];
    let merged = json!([
        paragraph_json(&[]),
        paragraph_json(&[text_json("h0xxxx"), hard_break_json(), text_json("t0yy")]),
        paragraph_json(&[]),
        paragraph_json(&[text_json("h2xxx")]),
        paragraph_json(&[text_json("h3xxxxh4xx")]),
        paragraph_json(&[hard_break_json(), text_json("t")]),
        paragraph_json(&[text_json("4yy")])
    ]);
    for (label, merge) in [
        ("a join", join_for_test(26)),
        ("a cross-block delete", delete_for_test(26, 27)),
    ] {
        let operations = splits.iter().cloned().chain([merge]).collect::<Vec<_>>();
        assert_composition_matches_preview(
            &format!("{label} removes a block whose split-off text was edited earlier"),
            source.clone(),
            operations,
            merged.clone(),
        );
    }
}

#[test]
fn a_delete_that_merges_texts_keeps_the_prepared_batch_it_merges_into() {
    assert_composition_matches_preview(
        "a delete joins the retained text with the first text of a merged suffix",
        boundary_shape_source(&[
            (Some(3), Some(3)),
            (Some(2), Some(3)),
            (Some(1), None),
            (Some(1), Some(1)),
        ]),
        vec![
            boundary_split(31),
            delete_for_test(17, 27),
            delete_for_test(13, 17),
        ],
        json!([
            paragraph_json(&[text_json("h0xxx"), hard_break_json(), text_json("t0yyy")]),
            paragraph_json(&[text_json("hh3x"), hard_break_json()]),
            paragraph_json(&[text_json("t3y")])
        ]),
    );
}

#[test]
fn a_structural_edit_after_text_created_in_the_same_block_is_refused_cleanly() {
    let cases = [
        (
            "a hard break before text typed into a textless block",
            boundary_shape_source(&[(None, None), (Some(1), None)]),
            vec![type_for_test(0, "z"), hard_break_insert_for_test(0)],
        ),
        (
            "a join into a textless block after typing into it",
            boundary_shape_source(&[(None, None), (Some(2), Some(1))]),
            vec![boundary_split(7), type_for_test(0, "z"), join_for_test(1)],
        ),
    ];
    for (label, source, operations) in cases {
        let error =
            compile_boundary_split_transaction(&source, operations.clone()).expect_err(label);
        assert_eq!(
            error.code, "OPERATION_INVALID",
            "{label}: {operations:?} on {source}: {error:?}"
        );
    }
}

#[test]
fn a_gap_moved_by_an_earlier_structural_edit_refuses_text_creation_cleanly() {
    let cases = [
        (
            "typing after a hard break whose earlier sibling was deleted",
            json!({"type": "doc", "content": [
                paragraph_json(&[hard_break_json(), hard_break_json()])
            ]}),
            vec![delete_for_test(0, 1), type_for_test(2, "z")],
        ),
        (
            "typing after a mixed suffix joined into a textless block",
            json!({"type": "doc", "content": [
                paragraph_json(&[]),
                paragraph_json(&[text_json("h1x"), hard_break_json()])
            ]}),
            vec![join_for_test(0), type_for_test(6, "z")],
        ),
    ];
    for (label, source, operations) in cases {
        let error =
            compile_boundary_split_transaction(&source, operations.clone()).expect_err(label);
        assert_eq!(
            error.code, "POSITION_INVALID",
            "{label}: {operations:?} on {source}: {error:?}"
        );
    }
}

#[test]
fn a_split_inside_a_created_block_shifts_later_actions_from_their_own_index_frame() {
    assert_composition_matches_preview(
        "a split in a created block after joins removed earlier siblings",
        boundary_shape_source(&[(None, None), (None, None), (None, None)]),
        vec![
            boundary_split(3),
            join_for_test(1),
            join_for_test(3),
            boundary_split(2),
        ],
        json!([
            paragraph_json(&[]),
            paragraph_json(&[]),
            paragraph_json(&[])
        ]),
    );
}

#[test]
fn a_merged_suffix_folds_into_a_pending_batch_at_the_same_storage_index() {
    let replace = |from, to| TypedOperation::ReplaceRange {
        range: range_for_test(from, to),
        content: Fragment::from(vec![Node::text("r".into(), vec![])]),
    };
    assert_composition_matches_preview(
        "a third replace merges a mixed suffix where an emptied batch remains",
        boundary_shape_source(&[
            (Some(3), None),
            (Some(1), None),
            (Some(4), Some(3)),
            (Some(2), Some(3)),
            (Some(1), Some(3)),
        ]),
        vec![replace(1, 10), replace(0, 17), replace(0, 34)],
        json!([paragraph_json(&[
            text_json("rrh4x"),
            hard_break_json(),
            text_json("t4yyy")
        ])]),
    );
}

#[test]
fn a_storage_only_splice_keeps_a_pending_batch_it_falls_inside() {
    let source = boundary_shape_source(&[(Some(1), Some(1)), (Some(2), Some(3))]);
    let merged = json!([paragraph_json(&[
        text_json("h0xh1xx"),
        hard_break_json(),
        text_json("t1yyy")
    ])]);
    assert_composition_matches_preview(
        "a delete keeps an emptied retained text after a joined mixed suffix",
        source.clone(),
        vec![join_for_test(7), delete_for_test(3, 7)],
        merged.clone(),
    );
    assert_composition_matches_preview(
        "a delete removes an inserted hard break before a joined mixed suffix",
        source,
        vec![
            hard_break_insert_for_test(7),
            join_for_test(7),
            delete_for_test(3, 7),
        ],
        merged,
    );
}

#[test]
fn insertions_that_end_up_at_one_index_merge_into_a_single_yrs_insert() {
    let replace = |from, to| TypedOperation::ReplaceRange {
        range: range_for_test(from, to),
        content: Fragment::from(vec![Node::text("r".into(), vec![])]),
    };
    assert_composition_matches_preview(
        "a replace removes the hard break that separated two pending insertions",
        boundary_shape_source(&[
            (None, Some(3)),
            (None, Some(2)),
            (Some(4), Some(3)),
            (Some(1), None),
        ]),
        vec![join_for_test(6), replace(7, 13), replace(0, 7)],
        json!([
            paragraph_json(&[text_json("rh2xxxx"), hard_break_json(), text_json("t2yyy")]),
            paragraph_json(&[text_json("h3x")])
        ]),
    );
}

#[test]
fn a_mixed_inline_insert_keeps_the_trailing_text_gap_out_of_the_shifted_layout() {
    let source = json!({"type": "doc", "content": [
        paragraph_json(&[text_json("bc"), hard_break_json()])
    ]});
    let operations = vec![TypedOperation::ReplaceStructure(
        crate::yrs_engine::StructuralReplacement::new(
            vec![0],
            0,
            0,
            Fragment::from(vec![Node::text("x".into(), vec![]), hard_break()]),
            crate::selection::Selection::cursor(3),
        ),
    )];
    assert_composition_matches_preview(
        "text and a hard break pasted before text that ends in a hard break",
        source,
        operations,
        json!([paragraph_json(&[
            text_json("x"),
            hard_break_json(),
            text_json("bc"),
            hard_break_json()
        ])]),
    );
}

fn mention_json() -> Value {
    json!({"type": "mention", "attrs": {"id": "user-1", "label": "Ann"}})
}

fn marked_text_json(text: &str, marks: Value) -> Value {
    json!({"type": "text", "text": text, "marks": marks})
}

#[test]
fn one_mark_operation_across_a_textblock_skips_its_hard_breaks_and_atoms() {
    let bold = || json!([{"type": "bold"}]);
    let link = || json!([{"type": "link", "attrs": {"href": "https://example.com"}}]);
    let link_mark = || {
        Mark::new(
            "link".into(),
            HashMap::from([("href".to_string(), json!("https://example.com"))]),
        )
    };
    assert_composition_matches_preview(
        "bold over the whole text extent of cd<br>ef",
        json!({"type": "doc", "content": [
            paragraph_json(&[text_json("cd"), hard_break_json(), text_json("ef")])
        ]}),
        vec![TypedOperation::AddMark {
            range: range_for_test(0, 5),
            mark: Mark::new("bold".into(), HashMap::new()),
        }],
        json!([paragraph_json(&[
            marked_text_json("cd", bold()),
            hard_break_json(),
            marked_text_json("ef", bold())
        ])]),
    );
    let mixed = || {
        json!({"type": "doc", "content": [paragraph_json(&[
            text_json("ab"),
            hard_break_json(),
            text_json("cd"),
            mention_json(),
            text_json("ef")
        ])]})
    };
    let mention_width = rendered_scalar_offset(&mixed(), &tiptap_schema(), "ef")
        - rendered_scalar_offset(&mixed(), &tiptap_schema(), "cd")
        - 2;
    let end = 7 + mention_width;
    assert_composition_matches_preview(
        "a link over text containing a hard break and a mention",
        mixed(),
        vec![TypedOperation::AddMark {
            range: range_for_test(1, end - 1),
            mark: link_mark(),
        }],
        json!([paragraph_json(&[
            text_json("a"),
            marked_text_json("b", link()),
            hard_break_json(),
            marked_text_json("cd", link()),
            mention_json(),
            marked_text_json("e", link()),
            text_json("f")
        ])]),
    );
    let linked = json!({"type": "doc", "content": [paragraph_json(&[
        marked_text_json("ab", link()),
        hard_break_json(),
        marked_text_json("cd", link()),
        mention_json(),
        marked_text_json("ef", link())
    ])]});
    assert_composition_matches_preview(
        "removing a link across a hard break and a mention",
        linked,
        vec![TypedOperation::RemoveMark {
            range: range_for_test(0, end),
            mark_type: "link".into(),
        }],
        json!([paragraph_json(&[
            text_json("ab"),
            hard_break_json(),
            text_json("cd"),
            mention_json(),
            text_json("ef")
        ])]),
    );
}
