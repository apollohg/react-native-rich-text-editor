fn blockquote(children: Vec<Node>) -> Node {
    Node::element(
        "blockquote".to_string(),
        HashMap::new(),
        Fragment::from(children),
    )
}

fn delete_and_map(root: Node, from: u32, to: u32) -> (Document, crate::transform::StepMap) {
    let (document, schema) = doc_and_schema(root);
    let mut transaction = Transaction::new();
    transaction.add_step(Step::DeleteRange { from, to });
    transaction
        .apply(&document, &schema)
        .unwrap_or_else(|error| panic!("delete {from}..{to} must apply: {error:?}"))
}

#[test]
fn deleting_from_a_paragraph_into_a_list_item_keeps_the_later_list_items() {
    let (after, map) = delete_and_map(
        doc(vec![
            paragraph(vec![text("a")]),
            bullet_list(vec![
                list_item(vec![paragraph(vec![text("b")])]),
                list_item(vec![paragraph(vec![text("c")])]),
            ]),
        ]),
        2,
        6,
    );
    assert_eq!(
        after.root(),
        &doc(vec![
            paragraph(vec![text("ab")]),
            bullet_list(vec![list_item(vec![paragraph(vec![text("c")])])]),
        ]),
        "the paragraph joins the first item's text and the second item survives"
    );
    assert_eq!(
        map.map_pos(11),
        7,
        "the surviving item's text keeps its mapping"
    );
}

#[test]
fn deleting_from_a_list_item_into_a_later_paragraph_joins_into_the_item() {
    let (after, map) = delete_and_map(
        doc(vec![
            bullet_list(vec![list_item(vec![paragraph(vec![text("a")])])]),
            paragraph(vec![text("b")]),
            paragraph(vec![text("c")]),
        ]),
        4,
        8,
    );
    assert_eq!(
        after.root(),
        &doc(vec![
            bullet_list(vec![list_item(vec![paragraph(vec![text("ab")])])]),
            paragraph(vec![text("c")]),
        ]),
        "the later paragraph's text joins the list item and the next block survives"
    );
    assert_eq!(map.map_pos(8), 4, "the joined text follows the item's text");
    assert_eq!(
        map.map_pos(11),
        9,
        "the next block shifts by the removed structure"
    );
}

fn nested_block_strategy() -> impl Strategy<Value = Node> {
    let line = "[a-e]{1,3}".prop_map(|value| paragraph(vec![text(&value)]));
    prop_oneof![
        line.clone(),
        prop::collection::vec(line.clone().prop_map(|block| list_item(vec![block])), 1..4)
            .prop_map(bullet_list),
        prop::collection::vec(line, 1..3).prop_map(blockquote),
    ]
}

fn text_positions(node: &Node, content_start: u32, positions: &mut Vec<(u32, char)>) {
    let Some(content) = node.content() else {
        return;
    };
    let mut position = content_start;
    for child in content.iter() {
        if let Some(value) = child.text_str() {
            for (offset, character) in value.chars().enumerate() {
                positions.push((position + offset as u32, character));
            }
        } else {
            text_positions(child, position + 1, positions);
        }
        position += child.node_size();
    }
}

proptest::proptest! {
    #[test]
    fn cross_block_deletes_keep_every_character_outside_the_range(
        blocks in prop::collection::vec(nested_block_strategy(), 2..6),
        endpoints in (any::<prop::sample::Index>(), any::<prop::sample::Index>()),
    ) {
        let root = doc(blocks);
        let mut positions = Vec::new();
        text_positions(&root, 0, &mut positions);
        let mut caret_positions: Vec<u32> = positions
            .iter()
            .flat_map(|(position, _)| [*position, position + 1])
            .collect();
        caret_positions.sort_unstable();
        caret_positions.dedup();
        let first = *endpoints.0.get(&caret_positions);
        let second = *endpoints.1.get(&caret_positions);
        let (from, to) = (first.min(second), first.max(second));
        let survivors: String = positions
            .iter()
            .filter(|(position, _)| *position < from || *position >= to)
            .map(|(_, character)| *character)
            .collect();
        let (document, schema) = doc_and_schema(root.clone());
        let mut transaction = Transaction::new();
        transaction.add_step(Step::DeleteRange { from, to });
        let (after, _) = transaction.apply(&document, &schema).unwrap_or_else(|error| {
            panic!("delete {from}..{to} of {root:?} must apply: {error:?}")
        });
        proptest::prop_assert_eq!(
            after.root().text_content(),
            survivors,
            "delete {}..{} of {:?} produced {:?}",
            from,
            to,
            root,
            after.root()
        );
    }
}

#[test]
fn deleting_into_a_container_keeps_unjoinable_blocks_as_siblings() {
    let image = Node::void(
        "image".to_string(),
        HashMap::from([("src".to_string(), serde_json::json!("a.png"))]),
    );
    let (after, map) = delete_and_map(
        doc(vec![
            paragraph(vec![text("a")]),
            blockquote(vec![paragraph(vec![text("b")]), image.clone()]),
        ]),
        2,
        7,
    );
    assert_eq!(
        after.root(),
        &doc(vec![paragraph(vec![text("a")]), blockquote(vec![image])]),
        "the image cannot join the paragraph, so it stays in its quote"
    );
    assert_eq!(
        map.map_pos(8),
        5,
        "the position after the image keeps its mapping"
    );
}

#[test]
fn deleting_to_a_block_inside_the_next_item_moves_it_into_the_first_item() {
    let (after, map) = delete_and_map(
        doc(vec![bullet_list(vec![
            list_item(vec![paragraph(vec![text("a")])]),
            list_item(vec![
                paragraph(vec![text("b")]),
                blockquote(vec![paragraph(vec![text("c")])]),
            ]),
        ])]),
        4,
        10,
    );
    assert_eq!(
        after.root(),
        &doc(vec![bullet_list(vec![list_item(vec![
            paragraph(vec![text("a")]),
            blockquote(vec![paragraph(vec![text("c")])]),
        ])])]),
        "the quote cannot join the paragraph, so the two items join instead"
    );
    assert_eq!(map.map_pos(12), 7, "the quoted text keeps its mapping");
}

#[test]
fn deleting_a_list_items_leading_paragraph_text_keeps_a_paragraph_before_its_quote() {
    let (after, map) = delete_and_map(
        doc(vec![
            paragraph(vec![text("x")]),
            bullet_list(vec![list_item(vec![
                paragraph(vec![text("b")]),
                blockquote(vec![paragraph(vec![text("c")])]),
            ])]),
        ]),
        2,
        7,
    );
    assert_eq!(
        after.root(),
        &doc(vec![
            paragraph(vec![text("x")]),
            bullet_list(vec![list_item(vec![
                Node::element(
                    "paragraph".into(),
                    crate::command_planner::default_attrs(&tiptap_schema(), "paragraph").unwrap(),
                    Fragment::empty(),
                ),
                blockquote(vec![paragraph(vec![text("c")])]),
            ])]),
        ]),
        "a list item still starts with a paragraph once its first paragraph joins upward"
    );
    assert_eq!(map.map_pos(10), 9, "the quoted text keeps its mapping");
}
