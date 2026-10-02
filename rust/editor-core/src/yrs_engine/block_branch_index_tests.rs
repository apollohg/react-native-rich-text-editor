use super::block_branch_index::BlockBranchIndex;
use crate::test_support::large_table_fixture::{
    multi_paragraph_cell_document, plain_table_document, session_with_document,
};
use serde_json::json;
use yrs::Assoc;

#[test]
fn index_conversions_equal_root_walks_at_every_position() {
    use yrs::branch::Branch;
    use yrs::types::xml::{XmlElementRef, XmlFragment, XmlOut};
    let fixtures = [
        (
            "prose",
            json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a🦀b"}]},{"type":"paragraph"}]}),
        ),
        (
            "lists",
            json!({"type":"doc","content":[{"type":"bullet_list","content":[{"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"list"}]}]}]}]}),
        ),
        (
            "atoms",
            json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"},{"type":"mention","attrs":{"id":"atom","label":"A"}},{"type":"text","text":"after"}]},{"type":"horizontal_rule"}]}),
        ),
        ("table", plain_table_document(3, 3)),
        ("nested-rich", multi_paragraph_cell_document()),
    ];
    for (name, input) in fixtures {
        let session = session_with_document(&input);
        let document = session.engine.document().unwrap();
        let map = session.engine.position_map().unwrap();
        let schema = session.engine.schema();
        session
            .engine
            .read_fragment_for_test(|txn, fragment| {
                let index = BlockBranchIndex::build(txn, fragment, schema, map).expect(name);
                for block in 0..map.block_count() {
                    let path = &map.block(block).unwrap().node_path;
                    let mut node = fragment.get(txn, path[0]).unwrap();
                    for &child in &path[1..] {
                        let XmlOut::Element(element) = node else {
                            panic!("{name}: container path {path:?}")
                        };
                        node = element.get(txn, child).unwrap();
                    }
                    let XmlOut::Element(element) = node else {
                        panic!("{name}: block path {path:?}")
                    };
                    let branches = index.block_branches(block).unwrap();
                    assert_eq!(
                        branches.element,
                        <XmlElementRef as AsRef<Branch>>::as_ref(&element).id(),
                        "{name}: block {block} identity at {path:?}"
                    );
                    let expected_texts: Vec<_> = element
                        .children(txn)
                        .filter_map(|child| match child {
                            XmlOut::Text(text) => Some(AsRef::<Branch>::as_ref(&text).id()),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(
                        branches.texts.as_slice(),
                        expected_texts,
                        "{name}: block {block} text identities"
                    );
                }
                for position in 0..=document.root().content().unwrap().size() {
                    for assoc in [Assoc::Before, Assoc::After] {
                        let walked = super::position::doc_pos_to_sticky_index(
                            txn, fragment, position, assoc, schema,
                        );
                        let indexed = index.sticky_at_doc_pos(txn, position, assoc, map, document);
                        if let Some(sticky) = &indexed {
                            let offset = sticky.get_offset(txn).unwrap();
                            assert_eq!(
                                index.doc_pos_of_offset(txn, &offset, map, document),
                                super::position::sticky_index_to_doc_pos(
                                    txn, fragment, sticky, schema
                                ),
                                "{name}: indexed reverse {position}, {assoc:?}"
                            );
                            assert_eq!(
                                Some(sticky),
                                walked.as_ref(),
                                "{name}: position {position}, {assoc:?}"
                            );
                        }
                        let is_content = (0..map.block_count()).any(|block| {
                            !map.block(block).unwrap().is_void_block
                                && (map.effective_doc_start(block)..=map.effective_doc_end(block))
                                    .contains(&position)
                        });
                        if is_content {
                            assert_eq!(
                                indexed, walked,
                                "{name}: content {position}, {assoc:?} must be indexed"
                            );
                        }
                        if let Some(sticky) = walked {
                            let offset = sticky.get_offset(txn).unwrap();
                            if let Some(actual) =
                                index.doc_pos_of_offset(txn, &offset, map, document)
                            {
                                assert_eq!(
                                    Some(actual),
                                    super::position::sticky_index_to_doc_pos(
                                        txn, fragment, &sticky, schema
                                    ),
                                    "{name}: reverse {position}, {assoc:?}"
                                );
                            }
                        }
                    }
                }
            })
            .unwrap();
    }
}

#[test]
fn path_keyed_identities_equal_position_keyed_identities() {
    let _clients = crate::test_support::deterministic_clients::DeterministicClients::new();
    let mut input = multi_paragraph_cell_document();
    input["content"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"type":"horizontal_rule"}));
    let session = session_with_document(&input);
    let index = session.engine.block_branch_index_for_test().unwrap();
    assert_eq!(index.table_key(&[1]), Some("y1-2"));
    assert_eq!(index.table_key(&[1, 2, 2, 0]), Some("y1-163"));
    assert_eq!(index.atom_id(&[0]), Some("y1-1"));
    assert_eq!(index.table_key(&[0]), None);
    assert_eq!(index.atom_id(&[1]), None);
}

#[test]
fn building_branch_index_does_not_remeasure_ancestor_subtrees() {
    use yrs::types::xml::{XmlFragment, XmlOut};
    fn node_count(txn: &impl yrs::ReadTxn, nodes: impl Iterator<Item = XmlOut>) -> usize {
        nodes
            .map(|node| {
                1 + match node {
                    XmlOut::Element(element) => node_count(txn, element.children(txn)),
                    XmlOut::Fragment(fragment) => node_count(txn, fragment.children(txn)),
                    XmlOut::Text(_) => 0,
                }
            })
            .sum()
    }
    for (label, document) in [
        ("nested table", multi_paragraph_cell_document()),
        ("tall table", plain_table_document(100, 20)),
    ] {
        let session = session_with_document(&document);
        session
            .engine
            .read_fragment_for_test(|txn, fragment| {
                let nodes = node_count(txn, fragment.children(txn));
                super::position::XML_SIZE_NODE_VISITS.set(0);
                let index = BlockBranchIndex::build(
                    txn,
                    fragment,
                    session.engine.schema(),
                    session.engine.position_map().unwrap(),
                );
                assert!(
                    index.is_some(),
                    "{label}: branch identities must remain available"
                );
                let size_visits = super::position::XML_SIZE_NODE_VISITS.replace(0);
                assert!(
                    size_visits <= nodes,
                    "{label}: {size_visits} subtree-size visits for only {nodes} XML nodes"
                );
            })
            .unwrap();
    }
}

#[test]
fn void_descendant_collision_keeps_first_source_branch_and_restores_sibling_position() {
    use yrs::branch::Branch;
    use yrs::{Transact, XmlElementPrelim, XmlFragment, XmlTextPrelim};
    let model = session_with_document(
        &json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]}),
    );
    let schema = model.engine.schema();
    const COLLIDING_CONTENT_START: u32 = 2;
    for tag in ["unknown-void", "__opaque", "horizontal_rule"] {
        let doc = yrs::Doc::new();
        let root = doc.get_or_insert_xml_fragment("branches");
        let mut txn = doc.transact_mut();
        let void = root.push_back(&mut txn, XmlElementPrelim::empty(tag));
        let nested = void.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        nested.push_back(&mut txn, XmlTextPrelim::new("nested"));
        let following = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        following.push_back(&mut txn, XmlTextPrelim::new("following"));
        let mut block = model
            .engine
            .position_map()
            .unwrap()
            .block(0)
            .unwrap()
            .into_owned();
        block.doc_start = COLLIDING_CONTENT_START;
        let map = crate::position::PositionMap::from_blocks(vec![block], schema);
        let index = BlockBranchIndex::build(&txn, &root, schema, &map).unwrap();
        assert_eq!(index.block_branches(0).unwrap().element, AsRef::<Branch>::as_ref(&nested).id(),
            "{tag}: the void's earlier descendant must win over the following sibling at the same position");
    }
}

#[test]
fn nested_fragments_flatten_identity_paths_and_unsupported_text_still_refuses_index() {
    use yrs::branch::{Branch, BranchID};
    use yrs::{
        Any, Text, Transact, XmlElementPrelim, XmlFragment, XmlFragmentPrelim, XmlTextPrelim,
    };
    let model = session_with_document(
        &json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]}),
    );
    let schema = model.engine.schema();
    let empty_map = crate::position::PositionMap::from_blocks(Vec::new(), schema);
    let doc = yrs::Doc::new();
    let root = doc.get_or_insert_xml_fragment("branches");
    let mut txn = doc.transact_mut();
    let yrs::XmlOut::Fragment(nested) = root.push_back(
        &mut txn,
        yrs::types::xml::XmlIn::Fragment(XmlFragmentPrelim::default()),
    ) else {
        panic!("nested XML fragment")
    };
    let table = nested.push_back(&mut txn, XmlElementPrelim::empty("table"));
    let yrs::XmlOut::Fragment(deeper) = nested.push_back(
        &mut txn,
        yrs::types::xml::XmlIn::Fragment(XmlFragmentPrelim::default()),
    ) else {
        panic!("deeper XML fragment")
    };
    let atom = deeper.push_back(&mut txn, XmlElementPrelim::empty("horizontal_rule"));
    let following = root.push_back(&mut txn, XmlElementPrelim::empty("horizontal_rule"));
    let key = |element: &yrs::XmlElementRef| {
        let BranchID::Nested(id) = AsRef::<Branch>::as_ref(element).id() else {
            panic!("integrated element")
        };
        format!("y{}-{}", id.client, id.clock)
    };
    let index = BlockBranchIndex::build(&txn, &root, schema, &empty_map).unwrap();
    assert_eq!(index.table_key(&[0]), Some(key(&table).as_str()));
    assert_eq!(
        index.atom_id(&[1]),
        Some(key(&atom).as_str()),
        "flattened nested atom must retain first-source precedence over sibling {}",
        key(&following)
    );
    let text = root.push_back(&mut txn, XmlTextPrelim::new(""));
    text.insert_embed(&mut txn, 0, Any::Bool(true));
    assert!(
        BlockBranchIndex::build(&txn, &root, schema, &empty_map).is_none(),
        "unmapped text must still be validated even after all block targets are exhausted"
    );
}
