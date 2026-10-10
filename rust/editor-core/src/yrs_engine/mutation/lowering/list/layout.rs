impl MutationCompiler {
    fn apply_virtual_structural_splices(
        &mut self,
        operation_index: usize,
        after: &Document,
        splices: &[VirtualStructuralSplice],
        preserved_prepared_inserts: &[usize],
    ) -> OperationResult<()> {
        let traversal_work = splices
            .len()
            .checked_add(self.structural_parents.len())
            .and_then(|work| work.checked_add(self.prepared_elements.len()))
            .and_then(|work| work.checked_add(self.prepared_inserts.len()))
            .and_then(|work| work.checked_add(self.targets.len()))
            .and_then(|work| work.checked_add(self.actions.len()))
            .and_then(|work| work.checked_add(self.pending_element_attrs.len()))
            .ok_or_else(|| work_overflow(self.request_id, operation_index, self.action_limit))?;
        self.charge_operation_work(operation_index, traversal_work)?;

        let mut deleted_branch_ids = HashSet::new();
        let mut deleted_prepared_insert_ids = HashSet::new();
        for splice in splices {
            let storage_start = usize::try_from(splice.storage_index)
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?;
            let storage_delete = usize::try_from(splice.storage_delete)
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?;
            let storage_end = storage_start
                .checked_add(storage_delete)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            let parent = self
                .structural_parents
                .get_mut(&splice.parent_path)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            if storage_end > parent.storage_children.len() {
                return Err(invalid_action_range(self.request_id, operation_index));
            }
            for child in &parent.storage_children[storage_start..storage_end] {
                match child {
                    StorageChildKind::Text { target, .. } => {
                        deleted_branch_ids.insert(AsRef::<Branch>::as_ref(target).id());
                    }
                    StorageChildKind::Element { target, .. } => {
                        deleted_branch_ids.insert(AsRef::<Branch>::as_ref(target).id());
                    }
                    StorageChildKind::PreparedElement | StorageChildKind::PreparedChild { .. } => {}
                }
            }
            let inserted = splice.storage_insert.clone().unwrap_or_else(|| {
                (0..splice.semantic_insert)
                    .map(|_| StorageChildKind::PreparedElement)
                    .collect()
            });
            parent
                .storage_children
                .splice(storage_start..storage_end, inserted);

            let mut parents = HashMap::with_capacity(self.structural_parents.len());
            for (path, parent) in std::mem::take(&mut self.structural_parents) {
                match remap_semantic_path(&path, splice)? {
                    Some(path) => {
                        parents.insert(path, parent);
                    }
                    None => {
                        if let XmlParentRef::Element(element) = &parent.parent {
                            deleted_branch_ids.insert(AsRef::<Branch>::as_ref(element).id());
                        }
                        for child in &parent.storage_children {
                            match child {
                                StorageChildKind::Text { target, .. } => {
                                    deleted_branch_ids.insert(AsRef::<Branch>::as_ref(target).id());
                                }
                                StorageChildKind::Element { target, .. } => {
                                    deleted_branch_ids.insert(AsRef::<Branch>::as_ref(target).id());
                                }
                                StorageChildKind::PreparedElement
                                | StorageChildKind::PreparedChild { .. } => {}
                            }
                        }
                    }
                }
            }
            self.structural_parents = parents;

            let mut prepared_elements = HashMap::with_capacity(self.prepared_elements.len());
            for (path, handle) in std::mem::take(&mut self.prepared_elements) {
                match remap_semantic_path(&path, splice)? {
                    Some(path) => {
                        prepared_elements.insert(path, handle);
                    }
                    None => {
                        if !preserved_prepared_inserts.contains(&handle.insert_id) {
                            deleted_prepared_insert_ids.insert(handle.insert_id);
                        }
                    }
                }
            }
            self.prepared_elements = prepared_elements;

            for (insert_id, pending) in self.prepared_inserts.iter_mut().enumerate() {
                let Some(pending) = pending.as_mut() else {
                    continue;
                };
                if preserved_prepared_inserts.contains(&insert_id) {
                    if pending.semantic_parent_path != splice.parent_path {
                        match remap_semantic_path(&pending.semantic_parent_path, splice)? {
                            Some(path) => pending.semantic_parent_path = path,
                            None => {
                                deleted_prepared_insert_ids.insert(insert_id);
                            }
                        }
                    }
                    continue;
                }
                if pending.semantic_parent_path == splice.parent_path {
                    let batch_len = u32::try_from(pending.nodes.len())
                        .map_err(|_| invalid_action_range(self.request_id, operation_index))?;
                    let batch_end = pending
                        .first_semantic_index
                        .checked_add(batch_len)
                        .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
                    let delete_end = splice
                        .semantic_index
                        .checked_add(splice.semantic_delete)
                        .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
                    if pending.first_semantic_index < delete_end
                        && batch_end > splice.semantic_index
                    {
                        deleted_prepared_insert_ids.insert(insert_id);
                    } else if pending.first_semantic_index >= delete_end {
                        pending.first_semantic_index = shift_semantic_index(
                            pending.first_semantic_index,
                            splice.semantic_delete,
                            splice.semantic_insert,
                        )?;
                    }
                } else {
                    match remap_semantic_path(&pending.semantic_parent_path, splice)? {
                        Some(path) => pending.semantic_parent_path = path,
                        None => {
                            deleted_prepared_insert_ids.insert(insert_id);
                        }
                    }
                }
            }
        }

        self.pending_element_attrs
            .retain(|id, _| !deleted_branch_ids.contains(id));
        for target in &mut self.targets {
            let deleted = match &target.kind {
                ResolvedTargetKind::Existing { target, .. } => {
                    deleted_branch_ids.contains(&AsRef::<Branch>::as_ref(target).id())
                }
                ResolvedTargetKind::Missing { parent, .. } => {
                    deleted_branch_ids.contains(&AsRef::<Branch>::as_ref(parent).id())
                }
                ResolvedTargetKind::Prepared { handle } => {
                    deleted_prepared_insert_ids.contains(&handle.insert_id)
                }
            };
            if deleted {
                for slot in std::mem::take(&mut target.action_slots) {
                    self.actions[slot] = ActionSlot::Tombstone;
                }
            }
        }
        self.targets.retain(|target| match &target.kind {
            ResolvedTargetKind::Existing { target, .. } => {
                !deleted_branch_ids.contains(&AsRef::<Branch>::as_ref(target).id())
            }
            ResolvedTargetKind::Missing { parent, .. } => {
                !deleted_branch_ids.contains(&AsRef::<Branch>::as_ref(parent).id())
            }
            ResolvedTargetKind::Prepared { handle } => {
                !deleted_prepared_insert_ids.contains(&handle.insert_id)
            }
        });
        for slot in &mut self.actions {
            let deleted = match slot {
                ActionSlot::PreparedInsert(insert_id) => {
                    deleted_prepared_insert_ids.contains(insert_id)
                        || self
                            .prepared_inserts
                            .get(*insert_id)
                            .and_then(Option::as_ref)
                            .is_some_and(|insert| match &insert.parent {
                                XmlParentRef::Element(parent) => deleted_branch_ids
                                    .contains(&AsRef::<Branch>::as_ref(parent).id()),
                                XmlParentRef::Fragment(_) => false,
                            })
                }
                ActionSlot::Concrete(action) => {
                    mutation_action_touches_branches(action, &deleted_branch_ids)
                }
                ActionSlot::Tombstone => false,
            };
            if deleted {
                if let ActionSlot::PreparedInsert(insert_id) = slot {
                    self.prepared_inserts[*insert_id] = None;
                }
                *slot = ActionSlot::Tombstone;
            }
        }
        for insert_id in deleted_prepared_insert_ids {
            if let Some(insert) = self.prepared_inserts.get_mut(insert_id) {
                *insert = None;
            }
        }
        self.rebuild_target_layout(operation_index, after)
    }

    fn register_prepared_insert_state(
        &mut self,
        operation_index: usize,
        insert_id: usize,
        after: &Document,
    ) -> OperationResult<()> {
        let insert = self
            .prepared_inserts
            .get(insert_id)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let nodes = insert.nodes.clone();
        let semantic_parent_path = insert.semantic_parent_path.clone();
        let first_semantic_index = insert.first_semantic_index;
        self.charge_operation_work(
            operation_index,
            prepared_clone_work(&nodes).ok_or_else(|| {
                work_overflow(self.request_id, operation_index, self.action_limit)
            })?,
        )?;
        let mut elements = Vec::new();
        let mut texts = Vec::new();
        collect_prepared_child_handles(
            insert_id,
            &nodes,
            &semantic_parent_path,
            first_semantic_index,
            Some(after),
            &mut elements,
            &mut texts,
        )?;
        for (path, handle) in elements {
            if handle.ordinal_path.len() == 1
                && self
                    .structural_parents
                    .get(&semantic_parent_path)
                    .is_some_and(|parent| {
                        parent.storage_children.iter().any(|child| matches!(child,
                StorageChildKind::PreparedChild { handle: candidate, .. } if candidate == &handle
            ))
                    })
            {
                continue;
            }
            if self.prepared_elements.insert(path, handle).is_some() {
                return Err(OperationError::engine_invariant_failed(
                    self.request_id,
                    Some(operation_index),
                    "duplicate prepared insert element semantic path",
                ));
            }
        }
        for (_, handle, runs) in texts {
            let text = prepared_runs_text(&runs);
            let scalar_len = u32::try_from(text.chars().count())
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?;
            self.targets.push(ResolvedText {
                kind: ResolvedTargetKind::Prepared { handle },
                gap_before: 0,
                text,
                scalar_len,
                base_runs: Vec::new(),
                current_runs: runs,
                action_slots: Vec::new(),
            });
        }
        self.rebuild_target_layout(operation_index, after)
    }

    fn rebuild_target_layout(
        &mut self,
        operation_index: usize,
        after: &Document,
    ) -> OperationResult<()> {
        let (content_positions, mut layout_work) =
            collect_document_content_positions(self.request_id, after.root())?;
        let mut existing_starts = HashMap::new();
        let mut inline_prepared_starts = HashMap::new();
        let mut inline_prepared_elements = HashMap::new();
        let mut missing_starts = HashMap::<BranchID, Vec<u32>>::new();
        let current_text_lengths = self
            .targets
            .iter()
            .filter_map(|target| match &target.kind {
                ResolvedTargetKind::Existing { target: text, .. } => {
                    Some((AsRef::<Branch>::as_ref(text).id(), target.scalar_len))
                }
                ResolvedTargetKind::Missing { .. } | ResolvedTargetKind::Prepared { .. } => None,
            })
            .collect::<HashMap<_, _>>();
        let current_prepared_lengths = self
            .targets
            .iter()
            .filter_map(|target| match &target.kind {
                ResolvedTargetKind::Prepared { handle } => {
                    Some((handle.clone(), target.scalar_len))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        for (path, parent) in &self.structural_parents {
            let node = after
                .node_at(path)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            let content = node
                .content()
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            layout_work = layout_work
                .checked_add(path.len())
                .and_then(|work| work.checked_add(parent.storage_children.len()))
                .and_then(|work| work.checked_add(content.child_count()))
                .ok_or_else(|| {
                    work_overflow(self.request_id, operation_index, self.action_limit)
                })?;
            let mut cursor = content_positions
                .get(path)
                .copied()
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            let mut semantic_index = 0usize;
            let mut semantic_text_offset = 0u32;
            let mut boundaries = Vec::with_capacity(parent.storage_children.len() + 1);
            for storage in &parent.storage_children {
                boundaries.push(cursor);
                let text_length = match storage {
                    StorageChildKind::Text {
                        scalar_len, target, ..
                    } => {
                        let id = AsRef::<Branch>::as_ref(target).id();
                        existing_starts.insert(id.clone(), cursor);
                        Some(
                            current_text_lengths
                                .get(&id)
                                .copied()
                                .unwrap_or(*scalar_len),
                        )
                    }
                    StorageChildKind::PreparedChild {
                        handle,
                        scalar_len: Some(length),
                    } => {
                        inline_prepared_starts.insert(handle.clone(), cursor);
                        Some(
                            current_prepared_lengths
                                .get(handle)
                                .copied()
                                .unwrap_or(*length),
                        )
                    }
                    StorageChildKind::Element { .. }
                    | StorageChildKind::PreparedElement
                    | StorageChildKind::PreparedChild {
                        scalar_len: None, ..
                    } => None,
                };
                if let Some(mut remaining) = text_length {
                    while remaining > 0 {
                        let child = content.child(semantic_index).ok_or_else(|| {
                            invalid_action_range(self.request_id, operation_index)
                        })?;
                        if !child.is_text() || semantic_text_offset >= child.node_size() {
                            return Err(invalid_action_range(self.request_id, operation_index));
                        }
                        let consumed = remaining.min(child.node_size() - semantic_text_offset);
                        cursor = cursor.checked_add(consumed).ok_or_else(|| {
                            invalid_action_range(self.request_id, operation_index)
                        })?;
                        remaining -= consumed;
                        semantic_text_offset += consumed;
                        if semantic_text_offset == child.node_size() {
                            semantic_index += 1;
                            semantic_text_offset = 0;
                        }
                    }
                } else {
                    if semantic_text_offset != 0 {
                        return Err(invalid_action_range(self.request_id, operation_index));
                    }
                    let child = content
                        .child(semantic_index)
                        .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
                    if child.is_text() {
                        return Err(invalid_action_range(self.request_id, operation_index));
                    }
                    if let StorageChildKind::PreparedChild { handle, .. } = storage {
                        let mut semantic_path = path.clone();
                        semantic_path.push(
                            u32::try_from(semantic_index).map_err(|_| {
                                invalid_action_range(self.request_id, operation_index)
                            })?,
                        );
                        inline_prepared_elements.insert(handle.clone(), semantic_path);
                    }
                    cursor = cursor
                        .checked_add(child.node_size())
                        .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
                    semantic_index += 1;
                }
            }
            if semantic_text_offset != 0 || semantic_index != content.child_count() {
                return Err(invalid_action_range(self.request_id, operation_index));
            }
            boundaries.push(cursor);
            missing_starts.insert(parent.parent.id(), boundaries);
        }

        let mut prepared_starts = HashMap::new();
        for (insert_id, insert) in self.prepared_inserts.iter().enumerate() {
            let Some(insert) = insert else {
                continue;
            };
            layout_work = layout_work
                .checked_add(prepared_clone_work(&insert.nodes).ok_or_else(|| {
                    work_overflow(self.request_id, operation_index, self.action_limit)
                })?)
                .ok_or_else(|| {
                    work_overflow(self.request_id, operation_index, self.action_limit)
                })?;
            let mut elements = Vec::new();
            let mut texts = Vec::new();
            collect_prepared_child_handles(
                insert_id,
                &insert.nodes,
                &insert.semantic_parent_path,
                insert.first_semantic_index,
                Some(after),
                &mut elements,
                &mut texts,
            )?;
            for (path, handle, _) in texts {
                let start = inline_prepared_starts
                    .get(&handle)
                    .copied()
                    .or_else(|| first_text_doc_position(after.root(), &path))
                    .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
                prepared_starts.insert(handle, start);
            }
        }
        prepared_starts.extend(inline_prepared_starts);
        self.prepared_elements
            .retain(|_, handle| !inline_prepared_elements.contains_key(handle));
        for (handle, path) in inline_prepared_elements {
            self.prepared_elements.insert(path, handle);
        }

        let target_count = self.targets.len();
        let mut indexed = Vec::with_capacity(target_count);
        for (ordinal, target) in std::mem::take(&mut self.targets).into_iter().enumerate() {
            let start = match &target.kind {
                ResolvedTargetKind::Existing { target, .. } => existing_starts
                    .get(&AsRef::<Branch>::as_ref(target).id())
                    .copied(),
                ResolvedTargetKind::Missing {
                    parent,
                    child_index,
                    ..
                } => missing_starts
                    .get(&AsRef::<Branch>::as_ref(parent).id())
                    .and_then(|boundaries| boundaries.get(*child_index as usize))
                    .copied(),
                ResolvedTargetKind::Prepared { handle } => prepared_starts.get(handle).copied(),
            }
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            indexed.push((start, ordinal, target));
        }
        layout_work = layout_work
            .checked_add(target_count)
            .and_then(|work| {
                work.checked_add(target_count.checked_mul(binary_partition_work(target_count))?)
            })
            .ok_or_else(|| work_overflow(self.request_id, operation_index, self.action_limit))?;
        self.charge_operation_work(operation_index, layout_work)?;
        indexed.sort_by_key(|(start, ordinal, _)| (*start, *ordinal));
        let mut previous_end = 0u32;
        for (start, _, mut target) in indexed {
            target.gap_before = start
                .checked_sub(previous_end)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            previous_end = start
                .checked_add(target.scalar_len)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            self.targets.push(target);
        }
        Ok(())
    }
}
