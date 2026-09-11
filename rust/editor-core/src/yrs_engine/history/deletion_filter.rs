const PROTECTED_UNDO_ELEMENT_TAGS: [&str; 1] = ["paragraph"];
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
    reachable: &IdSet,
    removable: &IdSet,
) -> OperationResult<Vec<ID>> {
    if reachable.is_empty() {
        return Ok(Vec::new());
    }
    let BranchID::Root(fragment_name) = AsRef::<Branch>::as_ref(fragment).id() else {
        return Ok(Vec::new());
    };
    let (candidates, containers) = {
        let txn = doc.transact();
        let candidates = if txn.state_vector().len() > 1 {
            reachable
        } else {
            removable
        };
        let containers = flatten_reverted_containers(&txn, fragment, candidates)
            .into_iter()
            .filter_map(|node| node.reverted)
            .collect::<Vec<ID>>();
        (candidates.clone(), containers)
    };
    if containers.is_empty() {
        return Ok(Vec::new());
    }
    let removal = id_set_difference(removable, &id_set_of(&containers));
    let Some(projection) = project_document_without(request_id, doc, &fragment_name, &removal)?
    else {
        return Ok(Vec::new());
    };
    let txn = projection.transact();
    let Some(projected_fragment) = txn.get_xml_fragment(fragment_name.as_ref()) else {
        return Ok(Vec::new());
    };
    let mut nodes = flatten_reverted_containers(&txn, &projected_fragment, &candidates);
    Ok(surviving_container_ids(&mut nodes))
}

impl YrsHistory {
    fn redone_reach(&self, insertions: &IdSet) -> (IdSet, IdSet) {
        let mut reachable = insertions.clone();
        let mut removable = insertions.clone();
        for _ in 0..=self.redone_chains.len() {
            let mut grew = false;
            for chain in &self.redone_chains {
                if id_sets_intersect(&reachable, &chain.originals)
                    && !id_set_contains_all(&reachable, &chain.copies)
                {
                    reachable.merge_with(chain.copies.clone());
                    grew = true;
                }
                if id_set_contains_all(&removable, &chain.originals)
                    && !id_set_contains_all(&removable, &chain.copies)
                {
                    removable.merge_with(chain.copies.clone());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        (reachable, removable)
    }

    fn redone_origins_of(&self, protected: &IdSet) -> IdSet {
        let mut blocked = protected.clone();
        for _ in 0..=self.redone_chains.len() {
            let mut grew = false;
            for chain in &self.redone_chains {
                if id_sets_intersect(&blocked, &chain.copies)
                    && !id_set_contains_all(&blocked, &chain.originals)
                {
                    blocked.merge_with(chain.originals.clone());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        blocked
    }

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
        let (reachable, removable) = self.redone_reach(top.insertions());
        let protected = protected_container_ids(request_id, doc, fragment, &reachable, &removable)?;
        if protected.is_empty() {
            return Ok(false);
        }
        let blocked = self.redone_origins_of(&id_set_of(&protected));
        let stack = match action {
            HistoryAction::Undo => self.manager.undo_stack(),
            HistoryAction::Redo => self.manager.redo_stack(),
        };
        let top = stack
            .last()
            .expect("filtered history stack retains its top item");
        let filtered_insertions = id_set_difference(&removable, &blocked);
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
