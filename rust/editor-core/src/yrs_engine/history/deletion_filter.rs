const PROTECTED_UNDO_ELEMENT_TAGS: [&str; 1] = ["paragraph"];

#[derive(Clone, Copy)]
enum ContainerProtection {
    Text { empty: bool },
    ProtectedElement,
    Unprotected,
}

#[derive(Clone, Copy)]
struct ContainerNode {
    parent: Option<usize>,
    reverted: Option<ID>,
    protection: ContainerProtection,
    child_survives: bool,
}

fn flatten_reverted_containers<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    reverted: &IdSet,
) -> Vec<ContainerNode> {
    let mut nodes: Vec<ContainerNode> = Vec::new();
    let mut pending: Vec<(XmlOut, Option<usize>)> = fragment
        .children(txn)
        .map(|child| (child, None))
        .collect::<Vec<_>>();
    while let Some((node, parent)) = pending.pop() {
        let index = nodes.len();
        let reverted_id = match node.as_ref().id() {
            BranchID::Nested(id) if reverted.contains(&id) => Some(id),
            _ => None,
        };
        let protection = match &node {
            XmlOut::Text(text) => ContainerProtection::Text {
                empty: text.len(txn) == 0,
            },
            XmlOut::Element(element)
                if PROTECTED_UNDO_ELEMENT_TAGS.contains(&element.tag().as_ref()) =>
            {
                ContainerProtection::ProtectedElement
            }
            _ => ContainerProtection::Unprotected,
        };
        nodes.push(ContainerNode {
            parent,
            reverted: reverted_id,
            protection,
            child_survives: false,
        });
        match node {
            XmlOut::Element(element) => {
                pending.extend(element.children(txn).map(|child| (child, Some(index))));
            }
            XmlOut::Fragment(nested) => {
                pending.extend(nested.children(txn).map(|child| (child, Some(index))));
            }
            XmlOut::Text(_) => {}
        }
    }
    nodes
}

fn surviving_container_ids(nodes: &mut [ContainerNode]) -> Vec<ID> {
    let mut surviving = Vec::new();
    for index in (0..nodes.len()).rev() {
        let ContainerNode {
            parent,
            reverted,
            protection,
            child_survives,
        } = nodes[index];
        let survives = match reverted {
            None => true,
            Some(_) => match protection {
                ContainerProtection::Text { empty } => !empty,
                ContainerProtection::ProtectedElement => child_survives,
                ContainerProtection::Unprotected => false,
            },
        };
        if !survives {
            continue;
        }
        if let Some(id) = reverted {
            surviving.push(id);
        }
        if let Some(parent) = parent {
            nodes[parent].child_survives = true;
        }
    }
    surviving
}

fn id_set_without(source: &IdSet, removed: &[ID]) -> IdSet {
    let mut removed_clocks: HashMap<ClientID, Vec<u32>> = HashMap::new();
    for id in removed {
        removed_clocks.entry(id.client).or_default().push(id.clock);
    }
    for clocks in removed_clocks.values_mut() {
        clocks.sort_unstable();
    }
    let retained: Vec<(ClientID, Vec<Range<u32>>)> = source
        .iter()
        .filter_map(|(client, ranges)| {
            let clocks = removed_clocks
                .get(client)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mut kept: Vec<Range<u32>> = Vec::new();
            for range in ranges.iter() {
                let mut start = range.start;
                for &clock in clocks {
                    if clock < start || clock >= range.end {
                        continue;
                    }
                    if clock > start {
                        kept.push(start..clock);
                    }
                    start = clock.saturating_add(1);
                }
                if start < range.end {
                    kept.push(start..range.end);
                }
            }
            (!kept.is_empty()).then_some((*client, kept))
        })
        .collect();
    IdSet::from_iter(retained)
}

fn delete_set_update(removed: &IdSet) -> Vec<u8> {
    let mut encoder = EncoderV1::new();
    encoder.write_var(0u32);
    removed.encode(&mut encoder);
    encoder.to_vec()
}

fn project_document_without(doc: &Doc, fragment_name: &str, removed: &IdSet) -> Option<Doc> {
    let state = encode_full_state(doc);
    if state.is_empty() {
        return None;
    }
    let projection = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        skip_gc: true,
        ..Options::default()
    });
    projection.get_or_insert_xml_fragment(fragment_name);
    for update in [state, delete_set_update(removed)] {
        let decoded =
            Update::decode_v1(&update).expect("history projection re-decodes engine-owned updates");
        projection
            .transact_mut()
            .apply_update(decoded)
            .expect("history projection applies engine-owned updates");
    }
    Some(projection)
}

fn protected_container_ids(doc: &Doc, fragment: &XmlFragmentRef, reverted: &IdSet) -> Vec<ID> {
    if reverted.is_empty() {
        return Vec::new();
    }
    let BranchID::Root(fragment_name) = AsRef::<Branch>::as_ref(fragment).id() else {
        return Vec::new();
    };
    let containers: Vec<ID> = {
        let txn = doc.transact();
        flatten_reverted_containers(&txn, fragment, reverted)
            .into_iter()
            .filter_map(|node| node.reverted)
            .collect()
    };
    if containers.is_empty() {
        return Vec::new();
    }
    let Some(projection) =
        project_document_without(doc, &fragment_name, &id_set_without(reverted, &containers))
    else {
        return Vec::new();
    };
    let txn = projection.transact();
    let Some(projected_fragment) = txn.get_xml_fragment(fragment_name.as_ref()) else {
        return Vec::new();
    };
    let mut nodes = flatten_reverted_containers(&txn, &projected_fragment, reverted);
    surviving_container_ids(&mut nodes)
}

impl YrsHistory {
    fn exclude_protected_containers(
        &mut self,
        doc: &Doc,
        fragment: &XmlFragmentRef,
        action: HistoryAction,
    ) -> bool {
        let stack = match action {
            HistoryAction::Undo => self.manager.undo_stack(),
            HistoryAction::Redo => self.manager.redo_stack(),
        };
        let Some(top) = stack.last() else {
            return false;
        };
        let protected = protected_container_ids(doc, fragment, top.insertions());
        if protected.is_empty() {
            return false;
        }
        let filtered = StackItem::with_meta(
            doc.guid(),
            top.deletions().clone(),
            id_set_without(top.insertions(), &protected),
            top.meta().clone(),
        );
        let mut undo_stack = self.manager.undo_stack().to_vec();
        let mut redo_stack = self.manager.redo_stack().to_vec();
        let replaced = match action {
            HistoryAction::Undo => undo_stack.last_mut(),
            HistoryAction::Redo => redo_stack.last_mut(),
        };
        *replaced.expect("filtered history stack retains its top item") = filtered;
        self.manager = build_undo_manager(
            doc,
            fragment,
            self.clock.clone(),
            undo_stack,
            redo_stack,
            &self.pending_capture,
            &self.pending_pop,
            &self.popped,
        );
        true
    }
}
