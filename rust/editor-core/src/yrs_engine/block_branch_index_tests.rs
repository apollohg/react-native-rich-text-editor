use super::block_branch_index::BlockBranchIndex;
use crate::test_support::large_table_fixture::{
    multi_paragraph_cell_document, plain_table_document, session_with_document,
};
use serde_json::json;
use yrs::Assoc;

#[test]
fn index_conversions_equal_root_walks_at_every_position() {
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
                    assert!(
                        index.block_branches(block).is_some(),
                        "{name}: block {block}"
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
