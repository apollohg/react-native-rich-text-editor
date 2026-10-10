const MAX_RETAINED_REDONE_CHAINS: usize = 64;

pub(crate) struct RedoneChain {
    originals: IdSet,
    copies: IdSet,
}

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
            XmlOut::Element(_) => ContainerProtection::ProtectedElement,
            XmlOut::Fragment(_) => ContainerProtection::Unprotected,
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

fn id_set_of(ids: &[ID]) -> IdSet {
    let mut set = IdSet::new();
    for id in ids {
        set.insert(*id, 1);
    }
    set
}

fn id_sets_intersect(left: &IdSet, right: &IdSet) -> bool {
    left.iter().any(|(client, ranges)| {
        right.iter().any(|(other_client, other_ranges)| {
            other_client == client
                && ranges.iter().any(|range| {
                    other_ranges
                        .iter()
                        .any(|other| range.start < other.end && other.start < range.end)
                })
        })
    })
}

fn id_set_contains_all(outer: &IdSet, inner: &IdSet) -> bool {
    inner.iter().all(|(client, ranges)| {
        ranges.iter().all(|range| {
            outer.iter().any(|(outer_client, outer_ranges)| {
                outer_client == client
                    && outer_ranges
                        .iter()
                        .any(|outer| outer.start <= range.start && range.end <= outer.end)
            })
        })
    })
}

fn id_set_difference(source: &IdSet, removed: &IdSet) -> IdSet {
    let retained: Vec<(ClientID, Vec<Range<u32>>)> = source
        .iter()
        .filter_map(|(client, ranges)| {
            let removed_ranges: Vec<&Range<u32>> = removed
                .iter()
                .filter(|(removed_client, _)| *removed_client == client)
                .flat_map(|(_, removed_ranges)| removed_ranges.iter())
                .collect();
            let mut kept: Vec<Range<u32>> = Vec::new();
            for range in ranges.iter() {
                let mut boundaries: Vec<&Range<u32>> = removed_ranges
                    .iter()
                    .copied()
                    .filter(|removed| removed.start < range.end && range.start < removed.end)
                    .collect();
                boundaries.sort_by_key(|removed| removed.start);
                let mut start = range.start;
                for removed in boundaries {
                    if removed.start > start {
                        kept.push(start..removed.start);
                    }
                    start = start.max(removed.end);
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

fn project_document_without(
    request_id: u64,
    doc: &Doc,
    fragment_name: &str,
    removed: &IdSet,
) -> OperationResult<Option<Doc>> {
    let state = encode_full_state(doc);
    if state.is_empty() {
        return Ok(None);
    }
    let projection = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        skip_gc: true,
        ..Options::default()
    });
    projection.get_or_insert_xml_fragment(fragment_name);
    for update in [state, delete_set_update(removed)] {
        let decoded = Update::decode_v1(&update).map_err(|error| {
            OperationError::engine_invariant_failed(
                request_id,
                None,
                format!("history projection cannot decode its own update: {error}"),
            )
        })?;
        projection
            .transact_mut()
            .apply_update(decoded)
            .map_err(|error| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    format!("history projection cannot apply its own update: {error}"),
                )
            })?;
    }
    Ok(Some(projection))
}

fn protected_container_ids(
    request_id: u64,
    doc: &Doc,
    fragment: &XmlFragmentRef,
    deletions: &IdSet,
) -> OperationResult<Vec<ID>> {
    if deletions.is_empty() {
        return Ok(Vec::new());
    }
    let BranchID::Root(fragment_name) = AsRef::<Branch>::as_ref(fragment).id() else {
        return Ok(Vec::new());
    };
    let containers = {
        let txn = doc.transact();
        flatten_reverted_containers(&txn, fragment, deletions)
            .into_iter()
            .filter_map(|node| node.reverted)
            .collect::<Vec<ID>>()
    };
    if containers.is_empty() {
        return Ok(Vec::new());
    }
    let removal = id_set_difference(deletions, &id_set_of(&containers));
    let Some(projection) = project_document_without(request_id, doc, &fragment_name, &removal)?
    else {
        return Ok(Vec::new());
    };
    let txn = projection.transact();
    let Some(projected_fragment) = txn.get_xml_fragment(fragment_name.as_ref()) else {
        return Ok(Vec::new());
    };
    let mut nodes = flatten_reverted_containers(&txn, &projected_fragment, deletions);
    Ok(surviving_container_ids(&mut nodes))
}

impl YrsHistory {
    pub(crate) fn record_redone_chain(&mut self, originals: IdSet, copies: IdSet) {
        if originals.is_empty() || copies.is_empty() {
            return;
        }
        self.redone_chains.push(RedoneChain { originals, copies });
        if self.redone_chains.len() > MAX_RETAINED_REDONE_CHAINS {
            self.retain_anchored_redone_chains();
        }
    }

    fn retain_anchored_redone_chains(&mut self) {
        let mut anchors = IdSet::new();
        for item in self
            .manager
            .undo_stack()
            .iter()
            .chain(self.manager.redo_stack())
        {
            anchors.merge_with(item.insertions().clone());
        }
        let mut copies = IdSet::new();
        for chain in &self.redone_chains {
            copies.merge_with(chain.copies.clone());
        }
        self.redone_chains.retain(|chain| {
            id_sets_intersect(&chain.originals, &anchors)
                || id_sets_intersect(&chain.originals, &copies)
        });
    }

    fn exclude_protected_containers(
        &mut self,
        request_id: u64,
        doc: &Doc,
        fragment: &XmlFragmentRef,
        action: HistoryAction,
    ) -> OperationResult<bool> {
        let stack = match action {
            HistoryAction::Undo => self.manager.undo_stack(),
            HistoryAction::Redo => self.manager.redo_stack(),
        };
        let Some(top) = stack.last() else {
            return Ok(false);
        };
        let resolved = self.manager.resolved_deletions(&mut doc.transact_mut(), top)
            .ok_or_else(|| OperationError::engine_invariant_failed(request_id, None,
                "history deletion targets cannot be resolved"))?;
        let mut deletions = IdSet::new();
        for mapping in &resolved {
            deletions.insert(mapping.target, mapping.target_len);
        }
        let protected = protected_container_ids(request_id, doc, fragment, &deletions)?;
        if protected.is_empty() {
            return Ok(false);
        }
        let protected = id_set_of(&protected);
        let mut blocked = IdSet::new();
        for mapping in resolved {
            if protected.contains(&mapping.target) {
                blocked.insert(mapping.source, mapping.source_len);
            }
        }
        let stack = match action {
            HistoryAction::Undo => self.manager.undo_stack(),
            HistoryAction::Redo => self.manager.redo_stack(),
        };
        let top = stack
            .last()
            .expect("filtered history stack retains its top item");
        let filtered_insertions = id_set_difference(top.insertions(), &blocked);
        if &filtered_insertions == top.insertions() {
            return Ok(false);
        }
        let filtered = StackItem::with_meta(
            doc.guid(),
            top.deletions().clone(),
            filtered_insertions,
            top.meta().clone(),
        );
        let (mut undo_stack, mut redo_stack) = self.cloned_stacks();
        let replaced = match action {
            HistoryAction::Undo => undo_stack.last_mut(),
            HistoryAction::Redo => redo_stack.last_mut(),
        };
        *replaced.expect("filtered history stack retains its top item") = filtered;
        self.install_stacks(doc, fragment, undo_stack, redo_stack);
        Ok(true)
    }
}

fn ids_of(set: &IdSet) -> impl Iterator<Item = ID> + '_ {
    set.iter().flat_map(|(client, ranges)| {
        ranges
            .iter()
            .flat_map(move |range| range.clone().map(move |clock| ID::new(*client, clock)))
    })
}

fn redone_copy_survives<T: ReadTxn>(txn: &T, container: ID) -> bool {
    StickyIndex::new(IndexScope::Nested(container), Assoc::After)
        .get_offset(txn)
        .is_some_and(|offset| {
            offset.branch.id() != BranchID::Nested(container) && !offset.branch.is_deleted()
        })
}

fn redo_can_restore<T: ReadTxn>(
    txn: &T,
    fragment_name: &str,
    item: &StackItem<HistoryMetadata>,
    id: ID,
) -> bool {
    let mut current = id;
    loop {
        let Some(offset) =
            StickyIndex::new(IndexScope::Relative(current), Assoc::After).get_offset(txn)
        else {
            return true;
        };
        match offset.branch.id() {
            BranchID::Root(name) => return name.as_ref() == fragment_name,
            BranchID::Nested(parent) => {
                if !offset.branch.is_deleted() || redone_copy_survives(txn, parent) {
                    return true;
                }
                if !item.deletions().contains(&parent) || item.insertions().contains(&parent) {
                    return false;
                }
                current = parent;
            }
        }
    }
}

impl YrsHistory {
    pub(crate) fn drop_unrevertible_stack_tops(&mut self, doc: &Doc, fragment: &XmlFragmentRef) {
        for action in [HistoryAction::Undo, HistoryAction::Redo] {
            while self.top_is_unrevertible(doc, fragment, action) {
                self.drop_top_stack_item(doc, fragment, action);
            }
        }
    }

    fn top_is_unrevertible(
        &self,
        doc: &Doc,
        fragment: &XmlFragmentRef,
        action: HistoryAction,
    ) -> bool {
        let Some(top) = self.acting_stack(action).last() else {
            return false;
        };
        let BranchID::Root(fragment_name) = AsRef::<Branch>::as_ref(fragment).id() else {
            return false;
        };
        if self.redone_chains.iter().any(|chain| {
            id_sets_intersect(&chain.originals, top.insertions())
                || id_sets_intersect(&chain.originals, top.deletions())
        }) {
            return false;
        }
        let txn = doc.transact();
        (top.insertions().is_empty()
            || id_set_contains_all(&txn.snapshot().delete_set, top.insertions()))
            && !ids_of(top.deletions())
                .filter(|id| !top.insertions().contains(id))
                .any(|id| redo_can_restore(&txn, &fragment_name, top, id))
    }
}
