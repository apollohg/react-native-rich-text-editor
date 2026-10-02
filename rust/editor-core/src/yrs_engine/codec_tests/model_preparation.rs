#[test]
fn model_preparation_matches_json_nodes_work_limits_and_wire_output() {
    use super::prepare_model_nodes;
    use crate::model::{Fragment, Mark, Node};
    use crate::serialize::node_to_prosemirror_json;
    use std::collections::HashMap;

    const FIXTURE_CLIENT_ID: u64 = 7;
    let marked = Node::text(
        "hé🙂\n雪".into(),
        vec![
            Mark::new("bold".into(), HashMap::new()),
            Mark::new("link".into(), HashMap::from([("href".into(), json!("/é"))])),
        ],
    );
    let mut deep = Node::element(
        "paragraph".into(),
        HashMap::new(),
        Fragment::from(vec![marked.clone()]),
    );
    for _ in 0..64 {
        deep = Node::element(
            "blockquote".into(),
            HashMap::new(),
            Fragment::from(vec![deep]),
        );
    }
    let nodes = vec![
        Node::element(
            "h2".into(),
            HashMap::from([("level".into(), json!(99))]),
            Fragment::from(vec![marked.clone()]),
        ),
        Node::element(
            "orderedList".into(),
            HashMap::from([("start".into(), json!(1))]),
            Fragment::empty(),
        ),
        Node::element(
            "paragraph".into(),
            HashMap::from([("metadata".into(), json!({"a":[null, true, 1.5, "雪"]}))]),
            Fragment::empty(),
        ),
        Node::element(
            "__opaque_json".into(),
            HashMap::from([(
                "original_json".into(),
                json!({"type":"alien","attrs":{"nested":[1,true]},"content":[{"type":"text","text":"opaque","marks":[{"type":"italic"}]}]}),
            )]),
            Fragment::from(vec![marked.clone()]),
        ),
        Node::element(
            "__opaque_json".into(),
            HashMap::new(),
            Fragment::from(vec![marked.clone()]),
        ),
        marked,
        deep,
    ];
    let custom = Schema::from_json(&json!({"nodes":[
        {"name":"doc","role":"doc","content":"block+"},
        {"name":"h2","role":"textBlock","content":"inline*","group":"block","attrs":{"native":{"default":0}},"json":{"type":"heading","attrs":{"level":2,"tone":"info"}}},
        {"name":"paragraph","role":"textBlock","content":"inline*","group":"block"},
        {"name":"text","role":"text","group":"inline"}
    ],"marks":[]})).unwrap();
    for schema in [
        tiptap_schema(),
        crate::schema::presets::prosemirror_schema(),
        custom,
    ] {
        let json: Vec<_> = nodes
            .iter()
            .map(|node| node_to_prosemirror_json(node, &schema))
            .collect();
        let compare = |limits: &ResourceLimits| {
            let expected = prepare_xml_nodes(&json, limits, 2);
            let actual = prepare_model_nodes(&nodes, &schema, limits, 2);
            match (expected, actual) {
                (Ok(expected), Ok(actual)) => {
                    assert_eq!(
                        actual.nodes, expected.nodes,
                        "prepared node parity at {limits:?}"
                    );
                    assert_eq!(actual.work, expected.work, "work parity at {limits:?}");
                }
                (Err(expected), Err(actual)) => {
                    assert_eq!(actual, expected, "first error parity at {limits:?}")
                }
                (expected, actual) => panic!(
                    "admission mismatch at {limits:?}: expected {expected:?}, actual {actual:?}"
                ),
            }
        };
        compare(&ResourceLimits::default());
        for limit in 0..=128 {
            compare(&ResourceLimits {
                max_document_nodes: limit,
                ..ResourceLimits::default()
            });
            compare(&ResourceLimits {
                max_document_depth: limit,
                ..ResourceLimits::default()
            });
        }
        for limit in 0..=512 {
            compare(&ResourceLimits {
                max_input_bytes: limit,
                ..ResourceLimits::default()
            });
        }
        compare(&ResourceLimits {
            max_document_nodes: 2,
            max_document_depth: 1,
            max_input_bytes: 0,
            ..ResourceLimits::default()
        });

        let wire = |batch: super::PreparedXmlBatch| {
            let doc = Doc::with_options(Options {
                client_id: yrs::ClientID::new(FIXTURE_CLIENT_ID),
                offset_kind: OffsetKind::Utf16,
                ..Options::default()
            });
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment("prosemirror");
            for child in batch.nodes {
                insert_prepared_node(&fragment, &mut txn, child.index, child.node);
            }
            let value = YrsDocumentCodec::new(&schema, &ResourceLimits::default())
                .read_json(&fragment, &txn)
                .unwrap();
            (
                value,
                txn.encode_state_as_update_v1(&yrs::StateVector::default()),
            )
        };
        let limits = ResourceLimits::default();
        let expected = wire(prepare_xml_nodes(&json, &limits, 2).unwrap());
        let actual = wire(prepare_model_nodes(&nodes, &schema, &limits, 2).unwrap());
        assert_eq!(actual.0, expected.0, "Yrs projection parity");
        // Multiple marks use Yrs' randomized Attrs iteration; compare exact bytes
        // on single-mark text where both paths have deterministic item ordering.
        let single_mark = vec![Node::element(
            "h2".into(),
            HashMap::new(),
            Fragment::from(vec![
                Node::text(
                    "hé🙂".into(),
                    vec![Mark::new("bold".into(), HashMap::new())],
                ),
                Node::text(
                    "雪".into(),
                    vec![Mark::new(
                        "link".into(),
                        HashMap::from([("href".into(), json!("/é"))]),
                    )],
                ),
            ]),
        )];
        let single_json: Vec<_> = single_mark
            .iter()
            .map(|node| node_to_prosemirror_json(node, &schema))
            .collect();
        let expected = wire(prepare_xml_nodes(&single_json, &limits, 2).unwrap());
        let actual = wire(prepare_model_nodes(&single_mark, &schema, &limits, 2).unwrap());
        assert_eq!(
            actual.1, expected.1,
            "fixed-client single-mark Yjs update bytes"
        );
    }
}

#[test]
fn direct_model_preparation_rejects_deep_attributes_on_a_small_stack() {
    const THREAD_STACK_BYTES: usize = 128 * 1024;
    const ATTRIBUTE_DEPTH: usize = 32 * 1024;
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            use crate::model::{Mark, Node};
            use std::collections::HashMap;
            let limits = ResourceLimits::default();
            let schema = tiptap_schema();
            for marked in [true, false] {
                let mut value = json!("leaf");
                for _ in 0..ATTRIBUTE_DEPTH {
                    value = Value::Array(vec![value]);
                }
                let attrs = HashMap::from([("deep".into(), value)]);
                let node = if marked {
                    Node::text("text".into(), vec![Mark::new("bold".into(), attrs)])
                } else {
                    Node::void("paragraph".into(), attrs)
                };
                let error = super::prepare_model_nodes(&[node], &schema, &limits, 2).unwrap_err();
                assert_eq!(error.code, "DOCUMENT_LIMIT_EXCEEDED");
                assert_eq!(error.limit, Some(limits.max_document_depth));
                assert_eq!(error.actual, Some(limits.max_document_depth + 1));
                assert_eq!(error.details.unwrap()["dimension"], "anyDepth");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn direct_model_preparation_obeys_projected_text_discriminators() {
    use crate::model::{Fragment, Mark, Node};
    use std::collections::HashMap;
    let marked = Node::text(
        "hidden".into(),
        vec![Mark::new("bold".into(), HashMap::new())],
    );
    for (text_name, block_projection, text_projection, node) in [
        (
            "glyph",
            Some(json!({"type":"text"})),
            None,
            Node::element(
                "paragraph".into(),
                HashMap::from([("ignored".into(), json!(1))]),
                Fragment::from(vec![marked.clone()]),
            ),
        ),
        ("text", None, Some(json!({"type":"character"})), marked),
    ] {
        let mut block =
            json!({"name":"paragraph","role":"textBlock","content":"inline*","group":"block"});
        if let Some(projection) = block_projection {
            block["json"] = projection;
        }
        let mut text = json!({"name":text_name,"role":"text","group":"inline"});
        if let Some(projection) = text_projection {
            text["json"] = projection;
        }
        let schema = Schema::from_json(&json!({"nodes":[{"name":"doc","role":"doc","content":"block+"}, block, text],"marks":[]})).unwrap();
        let projected = crate::serialize::node_to_prosemirror_json(&node, &schema);
        let limits = ResourceLimits::default();
        let expected = prepare_xml_nodes(&[projected], &limits, 2).unwrap();
        let actual = super::prepare_model_nodes(&[node], &schema, &limits, 2).unwrap();
        assert_eq!(
            actual.nodes, expected.nodes,
            "projected discriminator for {text_name}"
        );
        assert_eq!(actual.work, expected.work);
    }
}
