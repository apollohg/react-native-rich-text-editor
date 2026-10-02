fn linear_block_lookup(map: &PositionMap, position: u32) -> Option<usize> {
    let mut previous = None;
    for index in 0..map.block_count() {
        let block = map.block(index).unwrap();
        let start = map.effective_doc_start(index);
        let end = map.effective_doc_end(index);
        if block.doc_start == block.doc_end {
            if position == start {
                return Some(index);
            }
            if position < start {
                break;
            }
        } else {
            if position >= start && position <= end {
                return Some(index);
            }
            if position < start {
                return previous
                    .filter(|prior| position - map.effective_doc_end(*prior) <= start - position)
                    .or(Some(index));
            }
        }
        previous = Some(index);
    }
    previous
}

fn assert_block_lookup_parity(map: &PositionMap, document: &Document, stage: &str) {
    for position in (0..=document.content_size()).chain([u32::MAX]) {
        assert_eq!(
            map.find_block_for_doc_pos(position),
            linear_block_lookup(map, position),
            "{stage}: position={position}, blocks={:?}",
            map.blocks().collect::<Vec<_>>()
        );
    }
}

#[test]
fn document_block_lookup_has_logarithmic_probe_count() {
    const BLOCK_COUNT: usize = 4096;
    let document = Document::new(doc(vec![paragraph(vec![text("x")]); BLOCK_COUNT]));
    let map = PositionMap::build(&document, &tiptap_schema());
    crate::position::DOCUMENT_BLOCK_LOOKUP_PROBES.set(0);
    assert_eq!(
        map.find_block_for_doc_pos(document.content_size()),
        Some(BLOCK_COUNT - 1)
    );
    let probes = crate::position::DOCUMENT_BLOCK_LOOKUP_PROBES.get();
    let bound = BLOCK_COUNT.ilog2() as usize + 2;
    assert!(
        probes <= bound,
        "late-block lookup visited {probes} blocks; logarithmic bound={bound}"
    );
}

#[test]
fn document_block_lookup_preserves_gaps_empty_blocks_and_pending_deltas() {
    let schema = tiptap_schema();
    let mut document = Document::new(doc(vec![
        paragraph(vec![]),
        blockquote(vec![paragraph(vec![text("alpha🙂")]), horizontal_rule()]),
        bullet_list(vec![list_item(vec![paragraph(vec![
            text("beta"),
            hard_break(),
            text("end"),
        ])])]),
        paragraph(vec![]),
        paragraph(vec![text("tail")]),
    ]));
    let mut map = PositionMap::build(&document, &schema);
    assert_block_lookup_parity(&map, &document, "initial mixed document");
    for (block, inserted) in [(1, "XY"), (3, "Z")] {
        let mut transaction = Transaction::new();
        transaction.add_step(Step::InsertText {
            pos: map.effective_doc_start(block) + 1,
            text: inserted.into(),
            marks: vec![],
        });
        let (next, change) = transaction.apply(&document, &schema).unwrap();
        map.update(
            &change,
            &document,
            &next,
            UpdateMode::InlineTextOnly,
            &schema,
        );
        document = next;
        assert_block_lookup_parity(&map, &document, "positive pending delta");
    }
    let mut transaction = Transaction::new();
    let start = map.effective_doc_start(1);
    transaction.add_step(Step::DeleteRange {
        from: start,
        to: start + 1,
    });
    let (next, change) = transaction.apply(&document, &schema).unwrap();
    map.update(
        &change,
        &document,
        &next,
        UpdateMode::InlineTextOnly,
        &schema,
    );
    document = next;
    assert_ne!(
        map.block(3).unwrap().doc_start,
        map.effective_doc_start(3),
        "the differential control must exercise unapplied prefix deltas"
    );
    assert_block_lookup_parity(&map, &document, "negative pending delta");
    map.compact();
    assert_block_lookup_parity(&map, &document, "compacted");
    let mut transaction = Transaction::new();
    transaction.add_step(Step::SplitBlock {
        pos: map.effective_doc_start(1) + 1,
        node_type: "paragraph".into(),
        attrs: HashMap::new(),
    });
    let (next, change) = transaction.apply(&document, &schema).unwrap();
    map.update(&change, &document, &next, UpdateMode::Rebuild, &schema);
    assert_block_lookup_parity(&map, &next, "structural rebuild");
    for empty in [
        Document::new(doc(vec![])),
        Document::new(doc(vec![horizontal_rule()])),
    ] {
        assert_block_lookup_parity(
            &PositionMap::build(&empty, &schema),
            &empty,
            "empty or void",
        );
    }
}

#[test]
fn document_block_lookup_preserves_nested_table_and_atom_boundaries() {
    let schema = crate::schema::presets::prosemirror_table_schema();
    for source in [
        crate::test_support::large_table_fixture::multi_paragraph_cell_document(),
        crate::test_support::large_table_fixture::two_table_document(),
    ] {
        let document = crate::serialize::from_prosemirror_json(
            &source,
            &schema,
            crate::serialize::UnknownTypeMode::Preserve,
        )
        .unwrap();
        let map = PositionMap::build(&document, &schema);
        assert_block_lookup_parity(
            &map,
            &document,
            "nested table, multiple paragraphs, atom and root gaps",
        );
    }
}
