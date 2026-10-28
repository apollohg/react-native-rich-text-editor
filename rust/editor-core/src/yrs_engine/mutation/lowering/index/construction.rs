impl MutationCompiler {
    pub(crate) fn remaining_scan_work(&self) -> usize {
        self.scan_limit.saturating_sub(self.scan_work)
    }

    pub(crate) fn charge_position_resolver_work(
        &mut self,
        operation_index: usize,
        amount: usize,
    ) -> OperationResult<()> {
        self.charge_scan_work(operation_index, amount)?;
        #[cfg(test)]
        {
            self.position_resolver_work = self
                .position_resolver_work
                .checked_add(amount)
                .expect("successful resolver work is bounded by scan_limit");
        }
        Ok(())
    }

    fn push_action(&mut self, action: YrsMutationAction) -> usize {
        let slot = self.actions.len();
        self.actions.push(ActionSlot::concrete(action));
        slot
    }

    fn queue_prepared_insert(&mut self, insert: PendingPreparedInsert) -> usize {
        let insert_id = self.prepared_inserts.len();
        self.prepared_inserts.push(Some(insert));
        self.actions.push(ActionSlot::PreparedInsert(insert_id));
        insert_id
    }

    fn structural_action_follows_prepared_insert(
        &self,
        insert_id: usize,
        parent_id: &BranchID,
    ) -> bool {
        self.actions
            .iter()
            .skip_while(|slot| !matches!(slot, ActionSlot::PreparedInsert(id) if *id == insert_id))
            .skip(1)
            .any(|slot| match slot {
                ActionSlot::PreparedInsert(id) => self
                    .prepared_inserts
                    .get(*id)
                    .and_then(Option::as_ref)
                    .is_some_and(|pending| pending.parent.id() == *parent_id),
                ActionSlot::Concrete(action) => match action.as_ref() {
                    YrsMutationAction::DeleteXmlChildren { parent, .. }
                    | YrsMutationAction::InsertXmlChildren { parent, .. } => {
                        parent.id() == *parent_id
                    }
                    YrsMutationAction::CreateText { parent, .. } => {
                        AsRef::<Branch>::as_ref(parent).id() == *parent_id
                    }
                    _ => false,
                },
                ActionSlot::Tombstone => false,
            })
    }

    fn prepared_node_mut(
        &mut self,
        handle: &PreparedHandle,
        operation_index: usize,
    ) -> OperationResult<&mut PreparedXmlNode> {
        let insert = self
            .prepared_inserts
            .get_mut(handle.insert_id)
            .and_then(Option::as_mut)
            .ok_or_else(|| {
                OperationError::engine_invariant_failed(
                    self.request_id,
                    Some(operation_index),
                    "prepared insertion handle has no owned blueprint",
                )
            })?;
        let Some((&root, descendants)) = handle.ordinal_path.split_first() else {
            return Err(invalid_action_range(self.request_id, operation_index));
        };
        let mut node = &mut insert
            .nodes
            .get_mut(root)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?
            .node;
        for &ordinal in descendants {
            let PreparedXmlNode::Element { children, .. } = node else {
                return Err(invalid_action_range(self.request_id, operation_index));
            };
            node = &mut children
                .get_mut(ordinal)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?
                .node;
        }
        Ok(node)
    }

    fn drop_text_gaps_from(&mut self, parent_id: &BranchID, first_dropped_gap: u32) {
        self.targets.retain(|candidate| {
            !matches!(&candidate.kind,
                ResolvedTargetKind::Missing { parent, child_index: gap, .. }
                    if AsRef::<Branch>::as_ref(parent).id() == *parent_id && *gap >= first_dropped_gap
            )
        });
    }

    fn ensure_no_created_text(
        &self,
        operation_index: usize,
        field: &'static str,
        parent_id: &BranchID,
    ) -> OperationResult<()> {
        let has_created_text = self.targets.iter().any(|candidate| {
            matches!(
                &candidate.kind,
                ResolvedTargetKind::Missing {
                    parent,
                    create_action: Some(_),
                    ..
                } if AsRef::<Branch>::as_ref(parent).id() == *parent_id
            )
        });
        if has_created_text {
            return Err(OperationError::operation_invalid(
                self.request_id,
                operation_index,
                field,
                "a structural edit cannot follow text created in the same block within one transaction",
            )
            .refused_by_lowering(LoweringRefusal::CreatedTextBeforeStructuralEdit));
        }
        Ok(())
    }

    fn current_storage_text_len(&self, child: &StorageChildKind) -> Option<u32> {
        let StorageChildKind::Text {
            scalar_len, target, ..
        } = child
        else {
            return None;
        };
        let id = AsRef::<Branch>::as_ref(target).id();
        Some(
            self.targets
                .iter()
                .find_map(|candidate| match &candidate.kind {
                    ResolvedTargetKind::Existing { target, .. }
                        if AsRef::<Branch>::as_ref(target).id() == id =>
                    {
                        Some(candidate.scalar_len)
                    }
                    _ => None,
                })
                .unwrap_or(*scalar_len),
        )
    }

    fn foldable_pending_insert(
        &self,
        parent: &XmlParentRef,
        child_index: u32,
        semantic_parent_path: &[u32],
        following_semantic_index: u32,
    ) -> Option<usize> {
        let parent_id = parent.id();
        let (insert_id, position) =
            self.actions
                .iter()
                .enumerate()
                .rev()
                .find_map(|(position, slot)| match slot {
                    ActionSlot::PreparedInsert(insert_id) => {
                        let insert = self.prepared_inserts.get(*insert_id)?.as_ref()?;
                        (insert.parent.id() == parent_id).then_some((*insert_id, position))
                    }
                    ActionSlot::Concrete(_) | ActionSlot::Tombstone => None,
                })?;
        let insert = self.prepared_inserts.get(insert_id)?.as_ref()?;
        let child_list_untouched = self.actions[position + 1..].iter().all(|slot| match slot {
            ActionSlot::Concrete(action) => match action.as_ref() {
                YrsMutationAction::DeleteXmlChildren { parent, .. }
                | YrsMutationAction::InsertXmlChildren { parent, .. } => parent.id() != parent_id,
                YrsMutationAction::CreateText { parent, .. } => {
                    AsRef::<Branch>::as_ref(parent).id() != parent_id
                }
                _ => true,
            },
            ActionSlot::PreparedInsert(_) | ActionSlot::Tombstone => true,
        });
        (child_list_untouched
            && insert.child_index == child_index
            && insert.semantic_parent_path == semantic_parent_path
            && insert.first_semantic_index == following_semantic_index)
            .then_some(insert_id)
    }

    fn prepend_prepared_nodes(
        &mut self,
        operation_index: usize,
        insert_id: usize,
        nodes: Vec<PreparedXmlChild>,
        first_semantic_index: u32,
    ) -> OperationResult<()> {
        let request_id = self.request_id;
        let insert = self
            .prepared_inserts
            .get_mut(insert_id)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid_action_range(request_id, operation_index))?;
        insert.nodes.splice(0..0, nodes);
        insert.first_semantic_index = first_semantic_index;
        for (ordinal, child) in insert.nodes.iter_mut().enumerate() {
            child.index = u32::try_from(ordinal)
                .ok()
                .and_then(|ordinal| insert.child_index.checked_add(ordinal))
                .ok_or_else(|| invalid_action_range(request_id, operation_index))?;
        }
        self.prepared_elements
            .retain(|_, handle| handle.insert_id != insert_id);
        self.targets.retain(|target| {
            !matches!(&target.kind, ResolvedTargetKind::Prepared { handle } if handle.insert_id == insert_id)
        });
        Ok(())
    }

    fn replace_prepared_element_from_after(
        &mut self,
        operation_index: usize,
        handle: &PreparedHandle,
        path: &[u32],
        after: &Document,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<()> {
        let retained = after
            .node_at(path)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let insert_id = self.replace_prepared_element_from_preview(
            operation_index,
            handle,
            std::slice::from_ref(retained),
            path.len().saturating_add(1),
            schema,
            limits,
        )?;
        self.register_prepared_insert_state(operation_index, insert_id, after)
    }

    fn replace_prepared_element_from_preview(
        &mut self,
        operation_index: usize,
        handle: &PreparedHandle,
        replacement: &[Node],
        depth: usize,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<usize> {
        let mut batch =
            crate::yrs_engine::codec::prepare_model_nodes(replacement, schema, limits, depth)
                .map_err(|error| {
                    map_prepared_node_error(self.request_id, operation_index, error)
                })?;
        let empty_text_work = materialize_empty_prepared_textblocks(&mut batch.nodes, schema);
        let insert_id = handle.insert_id;
        self.replace_prepared_element_with_children(operation_index, handle, batch.nodes)?;
        self.prepared_elements
            .retain(|_, candidate| candidate.insert_id != insert_id);
        self.targets.retain(|target| {
            !matches!(&target.kind, ResolvedTargetKind::Prepared { handle } if handle.insert_id == insert_id)
        });
        self.charge_operation_work(
            operation_index,
            batch.work.checked_add(empty_text_work).ok_or_else(|| {
                work_overflow(self.request_id, operation_index, self.action_limit)
            })?,
        )?;
        Ok(insert_id)
    }

    fn release_prepared_storage_suffix(
        &mut self,
        operation_index: usize,
        block_path: &[u32],
        storage_start: usize,
    ) -> OperationResult<u32> {
        let request_id = self.request_id;
        let invalid = || invalid_action_range(request_id, operation_index);
        let block = self
            .structural_parents
            .get(block_path)
            .ok_or_else(invalid)?;
        let mut existing = 0u32;
        let mut released = HashMap::<usize, Vec<usize>>::new();
        for child in block
            .storage_children
            .get(storage_start..)
            .ok_or_else(invalid)?
        {
            match child {
                StorageChildKind::Text { .. } | StorageChildKind::Element { .. } => {
                    existing = existing.checked_add(1).ok_or_else(invalid)?;
                }
                StorageChildKind::PreparedChild { handle, .. } => {
                    let ordinal = *handle.ordinal_path.first().ok_or_else(invalid)?;
                    released.entry(handle.insert_id).or_default().push(ordinal);
                }
                StorageChildKind::PreparedElement => return Err(invalid()),
            }
        }
        for (insert_id, mut ordinals) in released {
            ordinals.sort_unstable();
            let insert = self
                .prepared_inserts
                .get_mut(insert_id)
                .and_then(Option::as_mut)
                .ok_or_else(invalid)?;
            let kept = ordinals[0];
            if !ordinals.iter().copied().eq(kept..insert.nodes.len()) {
                return Err(invalid());
            }
            let original_len = u32::try_from(insert.nodes.len()).map_err(|_| invalid())?;
            let following_index = insert
                .child_index
                .checked_add(original_len)
                .ok_or_else(invalid)?;
            let removed = u32::try_from(insert.nodes.len() - kept).map_err(|_| invalid())?;
            insert.nodes.truncate(kept);
            self.shift_child_actions_after_insert(
                operation_index,
                insert_id,
                following_index,
                removed,
                0,
            )?;
            if kept == 0 {
                for slot in &mut self.actions {
                    if matches!(slot, ActionSlot::PreparedInsert(id) if *id == insert_id) {
                        *slot = ActionSlot::Tombstone;
                    }
                }
                self.prepared_inserts[insert_id] = None;
            }
            let released_handle = |handle: &PreparedHandle| {
                handle.insert_id == insert_id
                    && handle
                        .ordinal_path
                        .first()
                        .is_some_and(|ordinal| *ordinal >= kept)
            };
            self.prepared_elements
                .retain(|_, handle| !released_handle(handle));
            for target in &mut self.targets {
                if matches!(&target.kind, ResolvedTargetKind::Prepared { handle } if released_handle(handle))
                {
                    for slot in std::mem::take(&mut target.action_slots) {
                        self.actions[slot] = ActionSlot::Tombstone;
                    }
                }
            }
            self.targets.retain(|target| {
                !matches!(&target.kind, ResolvedTargetKind::Prepared { handle } if released_handle(handle))
            });
        }
        Ok(existing)
    }

    fn shift_child_actions_after_insert(
        &mut self,
        operation_index: usize,
        insert_id: usize,
        following_index: u32,
        removed: u32,
        added: u32,
    ) -> OperationResult<()> {
        if removed == added {
            return Ok(());
        }
        let request_id = self.request_id;
        let invalid = || invalid_action_range(request_id, operation_index);
        let shift = |index: u32| -> OperationResult<u32> {
            index
                .checked_add(added)
                .and_then(|index| index.checked_sub(removed))
                .ok_or_else(invalid)
        };
        let renumber = |nodes: &mut [PreparedXmlChild], child_index: u32| -> OperationResult<()> {
            for (ordinal, child) in nodes.iter_mut().enumerate() {
                child.index = u32::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| child_index.checked_add(ordinal))
                    .ok_or_else(invalid)?;
            }
            Ok(())
        };
        let parent_id = self
            .prepared_inserts
            .get(insert_id)
            .and_then(Option::as_ref)
            .ok_or_else(invalid)?
            .parent
            .id();
        let position = self
            .actions
            .iter()
            .position(|slot| matches!(slot, ActionSlot::PreparedInsert(id) if *id == insert_id))
            .ok_or_else(invalid)?;
        let mut following = following_index;
        for slot in position + 1..self.actions.len() {
            match &mut self.actions[slot] {
                ActionSlot::Concrete(action) => match action.as_mut() {
                    YrsMutationAction::DeleteXmlChildren {
                        parent,
                        child_index,
                        child_count,
                        ..
                    } if parent.id() == parent_id => {
                        let end = child_index.checked_add(*child_count).ok_or_else(invalid)?;
                        if *child_index < following && end >= following {
                            return Err(invalid());
                        }
                        if end < following {
                            following = following.checked_sub(*child_count).ok_or_else(invalid)?;
                        } else {
                            *child_index = shift(*child_index)?;
                        }
                    }
                    YrsMutationAction::InsertXmlChildren {
                        parent,
                        child_index,
                        nodes,
                        ..
                    } if parent.id() == parent_id => {
                        if *child_index < following {
                            following = u32::try_from(nodes.len())
                                .ok()
                                .and_then(|count| following.checked_add(count))
                                .ok_or_else(invalid)?;
                        } else {
                            *child_index = shift(*child_index)?;
                            renumber(nodes, *child_index)?;
                        }
                    }
                    YrsMutationAction::CreateText {
                        parent,
                        child_index,
                        ..
                    } if AsRef::<Branch>::as_ref(parent).id() == parent_id => {
                        if *child_index < following {
                            following = following.checked_add(1).ok_or_else(invalid)?;
                        } else {
                            *child_index = shift(*child_index)?;
                        }
                    }
                    _ => {}
                },
                ActionSlot::PreparedInsert(other) => {
                    let other = *other;
                    let insert = self
                        .prepared_inserts
                        .get_mut(other)
                        .and_then(Option::as_mut)
                        .ok_or_else(invalid)?;
                    if insert.parent.id() == parent_id {
                        if insert.child_index < following {
                            following = u32::try_from(insert.nodes.len())
                                .ok()
                                .and_then(|count| following.checked_add(count))
                                .ok_or_else(invalid)?;
                        } else {
                            insert.child_index = shift(insert.child_index)?;
                            renumber(&mut insert.nodes, insert.child_index)?;
                        }
                    }
                }
                ActionSlot::Tombstone => {}
            }
        }
        Ok(())
    }

    fn replace_prepared_element_with_children(
        &mut self,
        operation_index: usize,
        handle: &PreparedHandle,
        replacement: Vec<PreparedXmlChild>,
    ) -> OperationResult<()> {
        let insert = self
            .prepared_inserts
            .get_mut(handle.insert_id)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let Some((&root_ordinal, descendants)) = handle.ordinal_path.split_first() else {
            return Err(invalid_action_range(self.request_id, operation_index));
        };
        if descendants.is_empty() {
            if root_ordinal >= insert.nodes.len() {
                return Err(invalid_action_range(self.request_id, operation_index));
            }
            let following_index = u32::try_from(root_ordinal)
                .ok()
                .and_then(|ordinal| insert.child_index.checked_add(ordinal))
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            let replacement_len = replacement.len();
            insert
                .nodes
                .splice(root_ordinal..=root_ordinal, replacement);
            for (ordinal, child) in insert.nodes.iter_mut().enumerate() {
                child.index = insert
                    .child_index
                    .checked_add(
                        u32::try_from(ordinal)
                            .map_err(|_| invalid_action_range(self.request_id, operation_index))?,
                    )
                    .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            }
            return self.shift_child_actions_after_insert(
                operation_index,
                handle.insert_id,
                following_index,
                REPLACED_PREPARED_NODES,
                u32::try_from(replacement_len)
                    .map_err(|_| invalid_action_range(self.request_id, operation_index))?,
            );
        }
        let mut node = &mut insert
            .nodes
            .get_mut(root_ordinal)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?
            .node;
        for ordinal in &descendants[..descendants.len() - 1] {
            let PreparedXmlNode::Element { children, .. } = node else {
                return Err(invalid_action_range(self.request_id, operation_index));
            };
            node = &mut children
                .get_mut(*ordinal)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?
                .node;
        }
        let PreparedXmlNode::Element { children, .. } = node else {
            return Err(invalid_action_range(self.request_id, operation_index));
        };
        let ordinal = descendants[descendants.len() - 1];
        if ordinal >= children.len() {
            return Err(invalid_action_range(self.request_id, operation_index));
        }
        children.splice(ordinal..=ordinal, replacement);
        for (ordinal, child) in children.iter_mut().enumerate() {
            child.index = u32::try_from(ordinal)
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?;
        }
        Ok(())
    }

    fn remove_prepared_children(
        &mut self,
        operation_index: usize,
        handles: &[PreparedHandle],
    ) -> OperationResult<Vec<usize>> {
        if handles.is_empty() {
            return Ok(Vec::new());
        }
        let mut removed = HashMap::<usize, Vec<usize>>::new();
        for handle in handles {
            let [ordinal] = handle.ordinal_path.as_ref() else {
                return Err(invalid_action_range(self.request_id, operation_index));
            };
            removed.entry(handle.insert_id).or_default().push(*ordinal);
        }
        let storage_work = self
            .structural_parents
            .values()
            .try_fold(0usize, |work, parent| {
                work.checked_add(parent.storage_children.len())
            })
            .ok_or_else(|| work_overflow(self.request_id, operation_index, self.action_limit))?;
        let work = self
            .targets
            .len()
            .checked_add(self.prepared_elements.len())
            .and_then(|work| work.checked_add(storage_work))
            .and_then(|work| work.checked_mul(removed.len()))
            .ok_or_else(|| work_overflow(self.request_id, operation_index, self.action_limit))?;
        self.charge_operation_work(operation_index, work)?;
        let mut preserved = Vec::with_capacity(removed.len());
        for (insert_id, mut ordinals) in removed {
            ordinals.sort_unstable();
            ordinals.dedup();
            let pending = self
                .prepared_inserts
                .get(insert_id)
                .and_then(Option::as_ref)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            let removal_work =
                pending
                    .nodes
                    .len()
                    .checked_mul(ordinals.len())
                    .ok_or_else(|| {
                        work_overflow(self.request_id, operation_index, self.action_limit)
                    })?;
            self.charge_operation_work(operation_index, removal_work)?;
            for &ordinal in ordinals.iter().rev() {
                self.replace_prepared_element_with_children(
                    operation_index,
                    &PreparedHandle {
                        insert_id,
                        ordinal_path: vec![ordinal].into_boxed_slice(),
                    },
                    Vec::new(),
                )?;
            }
            let remap = |handle: &mut PreparedHandle| {
                if handle.insert_id != insert_id {
                    return true;
                }
                let Some(root) = handle.ordinal_path.first_mut() else {
                    return false;
                };
                match ordinals.binary_search(root) {
                    Ok(_) => false,
                    Err(deleted_before) => {
                        *root -= deleted_before;
                        true
                    }
                }
            };
            self.targets.retain_mut(|target| match &mut target.kind {
                ResolvedTargetKind::Prepared { handle } => remap(handle),
                _ => true,
            });
            self.prepared_elements.retain(|_, handle| remap(handle));
            for parent in self.structural_parents.values_mut() {
                for child in &mut parent.storage_children {
                    if let StorageChildKind::PreparedChild { handle, .. } = child {
                        remap(handle);
                    }
                }
            }
            if self.prepared_inserts[insert_id]
                .as_ref()
                .is_some_and(|pending| pending.nodes.is_empty())
            {
                self.prepared_inserts[insert_id] = None;
                for slot in &mut self.actions {
                    if matches!(slot, ActionSlot::PreparedInsert(candidate) if *candidate == insert_id)
                    {
                        *slot = ActionSlot::Tombstone;
                    }
                }
            } else {
                preserved.push(insert_id);
            }
        }
        Ok(preserved)
    }

    fn prepared_storage_children(
        &self,
        operation_index: usize,
        insert_id: usize,
        nodes: &[PreparedXmlChild],
    ) -> OperationResult<Vec<StorageChildKind>> {
        nodes
            .iter()
            .enumerate()
            .map(|(ordinal, child)| {
                Ok(StorageChildKind::PreparedChild {
                    handle: PreparedHandle {
                        insert_id,
                        ordinal_path: vec![ordinal].into_boxed_slice(),
                    },
                    scalar_len: match &child.node {
                        PreparedXmlNode::Text { runs } => Some(checked_scalar_len(
                            self.request_id,
                            Some(operation_index),
                            &prepared_runs_text(runs),
                        )?),
                        PreparedXmlNode::Element { .. } => None,
                    },
                })
            })
            .collect()
    }

    pub(crate) fn new<T: ReadTxn>(
        request_id: u64,
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
        action_limit: usize,
        scan_limit: usize,
        scan_work: usize,
    ) -> OperationResult<Self> {
        let document_guard = capture_document_guard(request_id, txn)?;
        Self::new_eager_guarded(
            request_id,
            txn,
            fragment,
            schema,
            action_limit,
            scan_limit,
            scan_work,
            document_guard,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_eager_guarded<T: ReadTxn>(
        request_id: u64,
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
        action_limit: usize,
        scan_limit: usize,
        scan_work: usize,
        document_guard: DocumentGuard,
    ) -> OperationResult<Self> {
        let mut located = Vec::new();
        let mut materialized_texts = HashMap::new();
        let mut traversal_work = 0usize;
        let text_target_context = TextTargetContext {
            request_id,
            txn,
            schema,
        };
        #[cfg(test)]
        if !SUPPRESS_RANGE_FORMAT_LOWERING_COUNTS.get() {
            EAGER_RANGE_TEXT_COLLECTION_COUNT
                .set(EAGER_RANGE_TEXT_COLLECTION_COUNT.get().saturating_add(1));
        }
        collect_text_targets(
            &text_target_context,
            (0u32..).zip(fragment.children(txn)),
            TextTargetParent {
                id: <XmlFragmentRef as AsRef<Branch>>::as_ref(fragment).id(),
                ancestors: &[],
            },
            0,
            &mut traversal_work,
            &mut materialized_texts,
            &mut located,
        )?;
        let mut structural_parents = HashMap::new();
        #[cfg(test)]
        if !SUPPRESS_RANGE_FORMAT_LOWERING_COUNTS.get() {
            EAGER_RANGE_PARENT_COLLECTION_COUNT
                .set(EAGER_RANGE_PARENT_COLLECTION_COUNT.get().saturating_add(1));
        }
        collect_structural_parents(
            request_id,
            txn,
            XmlParentRef::Fragment(fragment.clone()),
            Vec::new(),
            Vec::new(),
            schema,
            &mut traversal_work,
            &materialized_texts,
            &mut structural_parents,
        )?;
        let targets = resolve_located_targets(request_id, located, &structural_parents)?;
        Ok(Self {
            request_id,
            document_guard,
            targets,
            structural_parents,
            actions: Vec::new(),
            prepared_inserts: Vec::new(),
            prepared_elements: HashMap::new(),
            charged_work: 0,
            pending_traversal_work: traversal_work,
            localized_position_target_count: None,
            explicit_path_parent_widths: None,
            structural_window_complement: None,
            action_limit,
            scan_work,
            scan_limit,
            #[cfg(test)]
            position_resolver_work: 0,
            created_gap_shifts: HashMap::new(),
            pending_element_attrs: HashMap::new(),
            wrap_checkpoints: HashMap::new(),
            #[cfg(test)]
            virtual_delete_visits: 0,
        })
    }

    /// Builds the existing eager compiler first, then replaces only its text
    /// target index with an independently resolved single-block target when
    /// that resolution is unambiguous. This is deliberately not wired into
    /// production yet: the eager result preserves today's guard and logical
    /// work accounting and is the ready fallback for every cold path.
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn new_localized_insert_or_eager<T: ReadTxn>(
        request_id: u64,
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
        action_limit: usize,
        scan_limit: usize,
        scan_work: usize,
        locator: LocalizedInsertLocator<'_>,
    ) -> OperationResult<(Self, MutationCompilerBuild)> {
        let eager = Self::new(
            request_id,
            txn,
            fragment,
            schema,
            action_limit,
            scan_limit,
            scan_work,
        )?;
        let Some(LocalizedTextblockTargets {
            targets,
            path_parent_widths,
        }) = localized_existing_textblock_targets(
            request_id,
            txn,
            fragment,
            schema,
            LocalizedTextblockLocator::Insert(locator),
            None,
        )?
        else {
            return Ok((eager, MutationCompilerBuild::EagerFallback));
        };

        let localized = Self {
            request_id,
            document_guard: eager.document_guard.clone(),
            targets,
            structural_parents: HashMap::new(),
            actions: Vec::new(),
            prepared_inserts: Vec::new(),
            prepared_elements: HashMap::new(),
            charged_work: 0,
            pending_traversal_work: eager.pending_traversal_work,
            localized_position_target_count: Some(eager.targets.len()),
            explicit_path_parent_widths: Some(path_parent_widths),
            structural_window_complement: None,
            action_limit,
            scan_work,
            scan_limit,
            #[cfg(test)]
            position_resolver_work: 0,
            created_gap_shifts: HashMap::new(),
            pending_element_attrs: HashMap::new(),
            wrap_checkpoints: HashMap::new(),
            #[cfg(test)]
            virtual_delete_visits: 0,
        };
        Ok((localized, MutationCompilerBuild::Localized))
    }
}

fn resolve_located_targets(
    request_id: u64,
    located: Vec<LocatedTarget>,
    structural_parents: &HashMap<Vec<u32>, StructuralParentTarget>,
) -> OperationResult<Vec<ResolvedText>> {
    let mut targets = Vec::with_capacity(located.len());
    let text_runs = structural_parents
        .values()
        .flat_map(|parent| parent.storage_children.iter())
        .filter_map(|child| match child {
            StorageChildKind::Text { target, runs, .. } => {
                Some((AsRef::<Branch>::as_ref(target).id(), runs.clone()))
            }
            StorageChildKind::Element { .. }
            | StorageChildKind::PreparedElement
            | StorageChildKind::PreparedChild { .. } => None,
        })
        .collect::<HashMap<_, _>>();
    let mut previous_end = 0u32;
    for located in located {
        let (start, scalar_len) = match &located {
            LocatedTarget::Existing {
                start, scalar_len, ..
            } => (*start, *scalar_len),
            LocatedTarget::Missing { start, .. } => (*start, 0),
        };
        let gap_before = start.checked_sub(previous_end).ok_or_else(|| {
            OperationError::engine_invariant_failed(
                request_id,
                None,
                "Yrs text targets overlap in document order",
            )
        })?;
        previous_end = start.checked_add(scalar_len).ok_or_else(|| {
            OperationError::engine_invariant_failed(
                request_id,
                None,
                "Yrs text target position overflow",
            )
        })?;
        let target = match located {
            LocatedTarget::Existing {
                target,
                text,
                signature,
                ..
            } => ResolvedText {
                base_runs: text_runs
                    .get(&AsRef::<Branch>::as_ref(&target).id())
                    .cloned()
                    .unwrap_or_else(|| {
                        vec![PreparedTextRun {
                            index_utf16: 0,
                            text: text.clone(),
                            attrs: Attrs::default(),
                        }]
                    }),
                current_runs: text_runs
                    .get(&AsRef::<Branch>::as_ref(&target).id())
                    .cloned()
                    .unwrap_or_else(|| {
                        vec![PreparedTextRun {
                            index_utf16: 0,
                            text: text.clone(),
                            attrs: Attrs::default(),
                        }]
                    }),
                action_slots: Vec::new(),
                kind: ResolvedTargetKind::Existing { signature, target },
                gap_before,
                text,
                scalar_len,
            },
            LocatedTarget::Missing {
                parent,
                child_index,
                signature,
                ..
            } => ResolvedText {
                base_runs: Vec::new(),
                current_runs: Vec::new(),
                action_slots: Vec::new(),
                kind: ResolvedTargetKind::Missing {
                    signature,
                    parent,
                    child_index,
                    create_action: None,
                },
                gap_before,
                text: String::new(),
                scalar_len: 0,
            },
        };
        targets.push(target);
    }
    Ok(targets)
}
