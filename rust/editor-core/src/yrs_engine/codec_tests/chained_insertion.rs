const CHAINED_INSERT_CLIENT_ID: u64 = 4_242;
const CHAINED_INSERT_PEER_CLIENT_ID: u64 = 4_243;
const CHAINED_INSERT_FIRST_INDEX: u32 = 2;
const CHAINED_INSERT_GAP_INDEX: u32 = 9;
const CHAINED_INSERT_BASE_PARAGRAPHS: u32 = 6;
const CHAINED_INSERT_DELETED_PARAGRAPHS: u32 = 2;

fn chained_insert_doc(client_id: u64) -> Doc {
    Doc::with_options(Options {
        client_id: yrs::ClientID::new(client_id),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    })
}

fn decode_chained_insert_update(update: &[u8]) -> yrs::Update {
    use yrs::updates::decoder::Decode;
    yrs::Update::decode_v1(update).unwrap()
}

fn chained_insert_base(doc: &Doc) -> Vec<u8> {
    let limits = ResourceLimits::default();
    let base = (0..CHAINED_INSERT_BASE_PARAGRAPHS)
        .map(|index| json!({ "type": "paragraph", "content": [{ "type": "text", "text": format!("base {index}") }] }))
        .collect::<Vec<_>>();
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment("prosemirror");
        insert_prepared_nodes(
            &fragment,
            &mut txn,
            prepare_xml_nodes(&base, &limits, 2).unwrap().nodes,
        );
    }
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment("prosemirror");
        fragment.remove_range(&mut txn, 1, CHAINED_INSERT_DELETED_PARAGRAPHS);
    }
    doc.transact()
        .encode_state_as_update_v1(&yrs::StateVector::default())
}

fn chained_insert_batch() -> Vec<super::PreparedXmlChild> {
    let pasted = json!([
        { "type": "paragraph", "content": [
            { "type": "text", "text": "pasted " },
            { "type": "text", "text": "bold", "marks": [{ "type": "bold" }] }
        ]},
        { "type": "bulletList", "content": (0..4).map(|index| json!({
            "type": "listItem",
            "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": format!("item {index} 😀") }] }]
        })).collect::<Vec<_>>() },
        { "type": "paragraph", "content": [{ "type": "text", "text": "e\u{301} 東京" }] },
        { "type": "horizontalRule" },
        { "type": "paragraph" }
    ]);
    let mut nodes = prepare_xml_nodes(pasted.as_array().unwrap(), &ResourceLimits::default(), 2)
        .unwrap()
        .nodes;
    for child in &mut nodes {
        child.index += CHAINED_INSERT_FIRST_INDEX;
    }
    let mut gapped = prepare_xml_nodes(
        &[json!({ "type": "paragraph", "content": [{ "type": "text", "text": "after gap" }] })],
        &ResourceLimits::default(),
        2,
    )
    .unwrap()
    .nodes;
    gapped[0].index = CHAINED_INSERT_GAP_INDEX;
    nodes.extend(gapped);
    nodes
}

#[test]
fn chained_prepared_insertion_matches_indexed_insertion_bytes_and_converges() {
    let schema = tiptap_schema();
    let limits = ResourceLimits::default();
    let codec = YrsDocumentCodec::new(&schema, &limits);
    let chained = chained_insert_doc(CHAINED_INSERT_CLIENT_ID);
    let indexed = chained_insert_doc(CHAINED_INSERT_CLIENT_ID);
    let base_update = chained_insert_base(&chained);
    assert_eq!(chained_insert_base(&indexed), base_update);
    {
        let mut txn = chained.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment("prosemirror");
        insert_prepared_nodes(&fragment, &mut txn, chained_insert_batch());
    }
    {
        let mut txn = indexed.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment("prosemirror");
        for child in chained_insert_batch() {
            super::insert_prepared_node(
                &fragment,
                &mut txn,
                super::PreparedNodePosition::Index(child.index),
                child.node,
            );
        }
    }

    let full = |doc: &Doc| {
        doc.transact()
            .encode_state_as_update_v1(&yrs::StateVector::default())
    };
    assert_eq!(full(&chained), full(&indexed), "full Update-v1 state");

    let peer = chained_insert_doc(CHAINED_INSERT_PEER_CLIENT_ID);
    {
        let mut txn = peer.transact_mut();
        txn.get_or_insert_xml_fragment("prosemirror");
        txn.apply_update(decode_chained_insert_update(&base_update))
            .unwrap();
    }
    let base_vector = peer.transact().state_vector();
    let chained_delta = chained.transact().encode_state_as_update_v1(&base_vector);
    assert_eq!(
        chained_delta,
        indexed.transact().encode_state_as_update_v1(&base_vector),
        "incremental Update-v1 from the shared base"
    );
    peer.transact_mut()
        .apply_update(decode_chained_insert_update(&chained_delta))
        .unwrap();

    let json = |doc: &Doc| {
        let txn = doc.transact();
        let fragment = txn.get_xml_fragment("prosemirror").unwrap();
        codec.read_json(&fragment, &txn).unwrap()
    };
    let expected = json(&indexed);
    assert_eq!(json(&chained), expected);
    assert_eq!(json(&peer), expected, "a remote peer converges on the chained update");
    assert_eq!(
        peer.transact().state_vector(),
        indexed.transact().state_vector()
    );
    assert_eq!(
        expected["content"].as_array().unwrap().len(),
        (CHAINED_INSERT_BASE_PARAGRAPHS - CHAINED_INSERT_DELETED_PARAGRAPHS) as usize
            + chained_insert_batch().len(),
        "{expected}"
    );
}
