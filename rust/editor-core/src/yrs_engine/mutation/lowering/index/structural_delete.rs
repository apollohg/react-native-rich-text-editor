impl MutationCompiler {
    pub(crate) fn delete_structural_range(
        &mut self,
        operation_index: usize,
        context: MutationDocumentContext<'_>,
        from: u32,
        to: u32,
    ) -> OperationResult<()> {
        if from == to {
            return Ok(());
        }
        let MutationDocumentContext {
            before: document,
            after,
            schema,
            limits,
        } = context;
        let from_resolved = document.resolve(from).map_err(|message| {
            OperationError::operation_invalid(self.request_id, operation_index, "range", message)
        })?;
        let to_resolved = document.resolve(to).map_err(|message| {
            OperationError::operation_invalid(self.request_id, operation_index, "range", message)
        })?;
        if from_resolved.node_path != to_resolved.node_path {
            return self.delete_cross_parent_structural_range(
                operation_index,
                MutationDocumentContext {
                    before: document,
                    after,
                    schema,
                    limits,
                },
                &from_resolved,
                &to_resolved,
            );
        }
        let parent = from_resolved.parent(document);
        let content = parent.content().ok_or_else(|| {
            OperationError::operation_invalid(
                self.request_id,
                operation_index,
                "range",
                "structural deletion parent has no content",
            )
        })?;
        let path = from_resolved.node_path.iter().copied().collect::<Vec<_>>();
        if let Some(handle) = self.prepared_elements.get(&path).cloned() {
            return self.replace_prepared_element_from_after(
                operation_index,
                &handle,
                &path,
                after,
                schema,
                limits,
            );
        }
        let target = self.structural_parents.get(&path).cloned().ok_or_else(|| {
            OperationError::engine_invariant_failed(
                self.request_id,
                Some(operation_index),
                "semantic structural parent has no tracked Yrs branch",
            )
        })?;
        self.ensure_no_created_text(operation_index, "range", &target.parent.id())?;
        let start = self
            .current_storage_insertion(
                content.iter(),
                &target.storage_children,
                from_resolved.parent_offset,
            )
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let end = self
            .current_storage_insertion(
                content.iter(),
                &target.storage_children,
                to_resolved.parent_offset,
            )
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let starts_inside_text = matches!(
            start,
            StorageInsertion::InsideText { .. } | StorageInsertion::InsidePreparedText { .. }
        );
        let child_index = match &start {
            StorageInsertion::Boundary(index) => *index,
            StorageInsertion::InsideText { child_index, .. }
            | StorageInsertion::InsidePreparedText { child_index, .. } => child_index
                .checked_add(1)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?,
        };
        let child_end = match &end {
            StorageInsertion::Boundary(index) => *index,
            StorageInsertion::InsideText { child_index, .. }
            | StorageInsertion::InsidePreparedText { child_index, .. } => *child_index,
        };
        let child_count = child_end
            .checked_sub(child_index)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let semantic_index = semantic_insertion_index(content, from_resolved.parent_offset)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let after_children = after
            .node_at(&path)
            .and_then(Node::content)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let semantic_delete = content
            .child_count()
            .checked_sub(after_children.child_count())
            .and_then(|count| u32::try_from(count).ok())
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let positions = self.positions()?;
        for (insertion, delete_prefix) in [(&end, true), (&start, false)] {
            if matches!(
                insertion,
                StorageInsertion::InsideText { .. } | StorageInsertion::InsidePreparedText { .. }
            ) {
                let index = self
                    .targets
                    .iter()
                    .position(|candidate| match (insertion, &candidate.kind) {
                        (
                            StorageInsertion::InsideText { target: text, .. },
                            ResolvedTargetKind::Existing { target, .. },
                        ) => {
                            AsRef::<Branch>::as_ref(target).id()
                                == AsRef::<Branch>::as_ref(text).id()
                        }
                        (
                            StorageInsertion::InsidePreparedText { handle, .. },
                            ResolvedTargetKind::Prepared { handle: candidate },
                        ) => handle == candidate,
                        _ => false,
                    })
                    .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
                let (text_start, text_end) = positions[index];
                let (delete_from, delete_to) = if delete_prefix {
                    (text_start, to)
                } else {
                    (from, text_end)
                };
                if self.delete(operation_index, delete_from, delete_to, &[])?
                    != TextRangeDisposition::Applied
                {
                    return Err(invalid_action_range(self.request_id, operation_index));
                }
            }
        }
        self.charge_operation_work(
            operation_index,
            usize::try_from(child_count)
                .ok()
                .and_then(|count| count.checked_add(target.signature.children.len()))
                .ok_or_else(|| {
                    work_overflow(self.request_id, operation_index, self.action_limit)
                })?,
        )?;
        let retained = (!starts_inside_text)
            .then(|| {
                target.storage_children[child_index as usize..child_end as usize]
                    .iter()
                    .enumerate()
                    .find_map(|(index, child)| match child {
                        StorageChildKind::Text { target, .. } => {
                            Some((child_index + index as u32, target.clone()))
                        }
                        StorageChildKind::Element { .. }
                        | StorageChildKind::PreparedElement
                        | StorageChildKind::PreparedChild { .. } => None,
                    })
            })
            .flatten();
        let mut deletes = Vec::with_capacity(2);
        if let Some((retained_index, retained_text)) = retained {
            let retained_id = AsRef::<Branch>::as_ref(&retained_text).id();
            let virtual_index = self
                .targets
                .iter()
                .position(|candidate| {
                    matches!(
                        &candidate.kind, ResolvedTargetKind::Existing { target, .. }
                            if AsRef::<Branch>::as_ref(target).id() == retained_id
                    )
                })
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            self.normalize_existing_target_for_split(
                operation_index,
                virtual_index,
                0,
                Vec::new(),
            )?;
            let retained_target = &mut self.targets[virtual_index];
            retained_target.text.clear();
            retained_target.scalar_len = 0;
            retained_target.current_runs.clear();
            let structural = self
                .structural_parents
                .get_mut(&path)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            if let StorageChildKind::Text {
                scalar_len, runs, ..
            } = &mut structural.storage_children[retained_index as usize]
            {
                *scalar_len = 0;
                runs.clear();
            }
            deletes.push((retained_index + 1, child_end - retained_index - 1));
            deletes.push((child_index, retained_index - child_index));
        } else {
            deletes.push((child_index, child_count));
        }
        self.drop_text_gaps_from(&target.parent.id(), child_index + 1);
        let mut removed_prepared = vec![false; target.storage_children.len()];
        let mut removed_handles = Vec::new();
        for &(storage_index, storage_delete) in &deletes {
            for index in storage_index as usize..(storage_index + storage_delete) as usize {
                match &target.storage_children[index] {
                    StorageChildKind::PreparedChild { handle, .. } => {
                        removed_prepared[index] = true;
                        removed_handles.push(handle.clone());
                    }
                    StorageChildKind::PreparedElement => {
                        return Err(OperationError::operation_invalid(
                            self.request_id,
                            operation_index,
                            "range",
                            "structural deletion cannot remove an untracked prepared child",
                        ))
                    }
                    _ => {}
                }
            }
        }
        let preserved = self.remove_prepared_children(operation_index, &removed_handles)?;
        let mut physical_deletes = Vec::new();
        for &(storage_index, storage_delete) in &deletes {
            let end = storage_index + storage_delete;
            let mut cursor = storage_index;
            while cursor < end {
                if removed_prepared[cursor as usize] {
                    cursor += 1;
                    continue;
                }
                let start = cursor;
                while cursor < end && !removed_prepared[cursor as usize] {
                    cursor += 1;
                }
                let skipped = u32::try_from(
                    removed_prepared[..start as usize]
                        .iter()
                        .filter(|removed| **removed)
                        .count(),
                )
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?;
                physical_deletes.push((start - skipped, cursor - start));
            }
        }
        physical_deletes.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        for (child_index, child_count) in physical_deletes {
            self.push_action(YrsMutationAction::DeleteXmlChildren {
                parent: target.parent.clone(),
                child_index,
                child_count,
                signature: target.signature.clone(),
                operation_index,
            });
        }
        let mut splices = Vec::with_capacity(deletes.len());
        for (index, (storage_index, storage_delete)) in deletes.into_iter().enumerate() {
            splices.push(VirtualStructuralSplice {
                parent_path: path.clone(),
                semantic_index,
                semantic_delete: if index == 0 { semantic_delete } else { 0 },
                semantic_insert: 0,
                storage_index,
                storage_delete,
                storage_insert: None,
            });
        }
        self.apply_virtual_structural_splices(operation_index, after, &splices, &preserved)
    }

    fn delete_cross_parent_structural_range(
        &mut self,
        operation_index: usize,
        context: MutationDocumentContext<'_>,
        from_resolved: &crate::model::ResolvedPos,
        to_resolved: &crate::model::ResolvedPos,
    ) -> OperationResult<()> {
        let MutationDocumentContext {
            before: document,
            after,
            schema,
            limits,
        } = context;
        let common_depth = from_resolved
            .node_path
            .iter()
            .zip(to_resolved.node_path.iter())
            .take_while(|(left, right)| left == right)
            .count();
        if from_resolved.node_path.len() != common_depth + 1
            || to_resolved.node_path.len() != common_depth + 1
        {
            return Err(OperationError::operation_invalid(
                self.request_id,
                operation_index,
                "range",
                "cross-parent structural deletion endpoints must share a tracked direct parent",
            ));
        }
        let first_parent = from_resolved.parent(document);
        let last_parent = to_resolved.parent(document);
        let clone_work = semantic_node_clone_work(first_parent)
            .and_then(|work| work.checked_add(semantic_node_clone_work(last_parent)?))
            .ok_or_else(|| work_overflow(self.request_id, operation_index, self.action_limit))?;
        self.charge_operation_work(operation_index, clone_work)?;
        let from = from_resolved.pos;
        let first_end = from
            .checked_add(first_parent.content_size() - from_resolved.parent_offset)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let last_end = to_resolved
            .pos
            .checked_add(last_parent.content_size() - to_resolved.parent_offset)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let (_, suffix) =
            crate::transform::apply::split_children_at(last_parent, to_resolved.parent_offset);
        let suffix = Fragment::from(suffix);
        let (trimmed, _) = crate::transform::apply_step_canonical_marks(
            document,
            &crate::transform::Step::DeleteRange {
                from,
                to: first_end,
            },
            schema,
        )
        .map_err(|message| {
            OperationError::operation_invalid(
                self.request_id,
                operation_index,
                "range",
                message.to_string(),
            )
        })?;
        if self.delete(operation_index, from, first_end, &[])? == TextRangeDisposition::Structural {
            self.delete_structural_range(
                operation_index,
                MutationDocumentContext {
                    before: document,
                    after: &trimmed,
                    schema,
                    limits,
                },
                from,
                first_end,
            )?;
        } else {
            self.rebuild_target_layout(operation_index, &trimmed)?;
        }
        let remove_to = last_end
            .checked_add(crate::tables::commands::NODE_CLOSING_TOKENS)
            .and_then(|end| end.checked_sub(first_end - from))
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        self.merge_following_blocks_into_retained(
            operation_index,
            MutationDocumentContext {
                before: &trimmed,
                after,
                schema,
                limits,
            },
            from,
            remove_to,
            &suffix,
        )
    }

    fn merge_following_blocks_into_retained(
        &mut self,
        operation_index: usize,
        context: MutationDocumentContext<'_>,
        from: u32,
        remove_to: u32,
        suffix: &Fragment,
    ) -> OperationResult<()> {
        let MutationDocumentContext {
            before,
            after,
            schema,
            limits,
        } = context;
        let remove_from = from
            .checked_add(crate::tables::commands::NODE_CLOSING_TOKENS)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let (without_blocks, _) = crate::transform::apply_step_canonical_marks(
            before,
            &crate::transform::Step::DeleteRange {
                from: remove_from,
                to: remove_to,
            },
            schema,
        )
        .map_err(|message| {
            OperationError::operation_invalid(
                self.request_id,
                operation_index,
                "range",
                message.to_string(),
            )
        })?;
        let delete_action_start = self.actions.len();
        self.delete_structural_range(
            operation_index,
            MutationDocumentContext {
                before,
                after: &without_blocks,
                schema,
                limits,
            },
            remove_from,
            remove_to,
        )?;
        let sibling_deletes = self.actions.split_off(delete_action_start);
        let retained_parent = without_blocks.resolve(from).map_err(|message| {
            OperationError::operation_invalid(self.request_id, operation_index, "range", message)
        })?;
        self.insert_tracked_replacement(
            operation_index,
            MutationDocumentContext {
                before: &without_blocks,
                after,
                schema,
                limits,
            },
            from,
            &retained_parent,
            suffix,
        )?;
        // The suffix is inserted into the retained parent before its siblings are removed.
        self.actions.extend(sibling_deletes);
        self.rebuild_target_layout(operation_index, after)
    }

    fn current_storage_insertion<'a>(
        &self,
        children: impl Iterator<Item = &'a Node>,
        storage_children: &[StorageChildKind],
        position: u32,
    ) -> Option<StorageInsertion> {
        let targets = self
            .targets
            .iter()
            .filter_map(|candidate| match &candidate.kind {
                ResolvedTargetKind::Existing { target, .. } => {
                    Some((AsRef::<Branch>::as_ref(target).id(), candidate))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let prepared_targets = self
            .targets
            .iter()
            .filter_map(|target| match &target.kind {
                ResolvedTargetKind::Prepared { handle } => Some((handle.clone(), target)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let semantic_children = children.collect::<Vec<_>>();
        let mut semantic_index = 0usize;
        let mut semantic_text_offset = 0u32;
        let mut offset = 0u32;
        for (storage_index, storage) in storage_children.iter().enumerate() {
            if offset == position {
                return Some(StorageInsertion::Boundary(
                    u32::try_from(storage_index).ok()?,
                ));
            }
            let current = match storage {
                StorageChildKind::Text { target, .. } => {
                    Some(*targets.get(&AsRef::<Branch>::as_ref(target).id())?)
                }
                StorageChildKind::PreparedChild {
                    handle,
                    scalar_len: Some(_),
                } => prepared_targets.get(handle).copied(),
                _ => None,
            };
            let text_length = match (storage, current) {
                (_, Some(current)) => Some(current.scalar_len),
                (
                    StorageChildKind::PreparedChild {
                        scalar_len: Some(length),
                        ..
                    },
                    None,
                ) => Some(*length),
                _ => None,
            };
            if let Some(mut remaining) = text_length {
                let start = offset;
                while remaining > 0 {
                    let child = *semantic_children.get(semantic_index)?;
                    if !child.is_text() || semantic_text_offset >= child.node_size() {
                        return None;
                    }
                    let consumed = remaining.min(child.node_size() - semantic_text_offset);
                    offset = offset.checked_add(consumed)?;
                    remaining -= consumed;
                    semantic_text_offset += consumed;
                    if semantic_text_offset == child.node_size() {
                        semantic_index += 1;
                        semantic_text_offset = 0;
                    }
                }
                if position > start && position < offset {
                    let child_index = u32::try_from(storage_index).ok()?;
                    let local_scalar = position.checked_sub(start)?;
                    return match storage {
                        StorageChildKind::Text {
                            target, signature, ..
                        } => Some(StorageInsertion::InsideText {
                            child_index,
                            local_scalar,
                            target: target.clone(),
                            signature: signature.clone(),
                            runs: current?.current_runs.clone(),
                        }),
                        StorageChildKind::PreparedChild { handle, .. } => {
                            Some(StorageInsertion::InsidePreparedText {
                                child_index,
                                handle: handle.clone(),
                            })
                        }
                        _ => None,
                    };
                }
            } else {
                if semantic_text_offset != 0 {
                    return None;
                }
                let child = *semantic_children.get(semantic_index)?;
                if child.is_text() {
                    return None;
                }
                offset = offset.checked_add(child.node_size())?;
                semantic_index += 1;
            }
        }
        (offset == position).then(|| {
            u32::try_from(storage_children.len())
                .ok()
                .map(StorageInsertion::Boundary)
        })?
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_cross_parent_replacement(
        &mut self,
        operation_index: usize,
        before: &Document,
        after: &Document,
        from: u32,
        from_resolved: &crate::model::ResolvedPos,
        content: &Fragment,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<()> {
        if content.size() == 0 {
            return Ok(());
        }
        if content.iter().all(Node::is_text) {
            let pieces = inline_text_pieces(self.request_id, operation_index, content)?;
            let mut position = from;
            for (text, marks) in pieces {
                self.insert(operation_index, position, &text, &marks)?;
                position = position
                    .checked_add(checked_scalar_len(
                        self.request_id,
                        Some(operation_index),
                        &text,
                    )?)
                    .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            }
            return Ok(());
        }

        let ResolvedInsertion {
            target_index,
            scalar_index,
        } = self.resolve_insertion(operation_index, from)?;
        let retained_id = match &self.targets[target_index].kind {
            ResolvedTargetKind::Existing { target, .. } => AsRef::<Branch>::as_ref(target).id(),
            _ => return Err(invalid_action_range(self.request_id, operation_index)),
        };
        let cut_utf16 = prepared_runs_utf16_at_scalar(
            self.request_id,
            operation_index,
            &self.targets[target_index].current_runs,
            scalar_index,
        )?;
        let (desired_left, _) =
            split_runs_utf16(&self.targets[target_index].current_runs, cut_utf16)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        self.normalize_existing_target_for_split(
            operation_index,
            target_index,
            cut_utf16,
            desired_left.clone(),
        )?;
        self.targets[target_index].current_runs = desired_left;
        self.targets[target_index].text =
            prepared_runs_text(&self.targets[target_index].current_runs);
        self.targets[target_index].scalar_len = scalar_index;

        let mut position = from;
        for node in content.iter().take_while(|node| node.is_text()) {
            let text = node
                .text_str()
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            self.insert(operation_index, position, text, node.marks())?;
            position = position
                .checked_add(node.node_size())
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        }

        let parent_path = from_resolved.node_path.to_vec();
        let final_parent = after
            .node_at(&parent_path)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let final_children = final_parent
            .content()
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let first_structural_semantic = final_children
            .iter()
            .position(|node| !node.is_text())
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let json = final_children
            .iter()
            .skip(first_structural_semantic)
            .map(|node| crate::serialize::node_to_prosemirror_json(node, schema))
            .collect::<Vec<_>>();
        let mut batch = prepare_xml_nodes(&json, limits, parent_path.len().saturating_add(2))
            .map_err(|error| map_prepared_node_error(self.request_id, operation_index, error))?;
        let parent_target = self
            .structural_parents
            .get(&parent_path)
            .cloned()
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let retained_storage_index = parent_target
            .storage_children
            .iter()
            .position(|child| {
                matches!(
                    child,
                    StorageChildKind::Text { target, .. }
                        if AsRef::<Branch>::as_ref(target).id() == retained_id
                )
            })
            .and_then(|index| u32::try_from(index).ok())
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let child_index = retained_storage_index
            .checked_add(1)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        for child in &mut batch.nodes {
            child.index = child_index
                .checked_add(child.index)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        }
        let prepared_work = batch.work;
        let insert_id = self.queue_prepared_insert(PendingPreparedInsert {
            parent: parent_target.parent,
            child_index,
            nodes: batch.nodes,
            signature: parent_target.signature.clone(),
            operation_index,
            semantic_parent_path: parent_path.clone(),
            first_semantic_index: u32::try_from(first_structural_semantic)
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?,
        });
        self.register_prepared_children(
            operation_index,
            insert_id,
            &parent_path,
            u32::try_from(first_structural_semantic)
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?,
            target_index,
            after,
        )?;
        self.charge_operation_work(
            operation_index,
            parent_target
                .signature
                .children
                .len()
                .checked_add(prepared_work)
                .and_then(|work| work.checked_add(json.len()))
                .ok_or_else(|| {
                    work_overflow(self.request_id, operation_index, self.action_limit)
                })?,
        )?;
        let _ = before;
        Ok(())
    }

    fn register_prepared_children(
        &mut self,
        operation_index: usize,
        insert_id: usize,
        parent_path: &[u32],
        first_semantic_index: u32,
        after_target: usize,
        after: &Document,
    ) -> OperationResult<()> {
        let nodes = self
            .prepared_inserts
            .get(insert_id)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?
            .nodes
            .clone();
        let mut elements = Vec::new();
        let mut texts = Vec::new();
        collect_prepared_child_handles(
            insert_id,
            &nodes,
            parent_path,
            first_semantic_index,
            Some(after),
            &mut elements,
            &mut texts,
        )?;
        for (path, handle) in elements {
            if self.prepared_elements.insert(path, handle).is_some() {
                return Err(OperationError::engine_invariant_failed(
                    self.request_id,
                    Some(operation_index),
                    "duplicate prepared element semantic path",
                ));
            }
        }
        let positions = self.positions()?;
        let mut previous_end = positions
            .get(after_target)
            .map(|(_, end)| *end)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        for (insertion, (path, handle, runs)) in (after_target + 1..).zip(texts) {
            let start = first_text_doc_position(after.root(), &path, runs.is_empty())
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            let text = prepared_runs_text(&runs);
            let scalar_len = u32::try_from(text.chars().count())
                .map_err(|_| invalid_action_range(self.request_id, operation_index))?;
            let gap_before = start
                .checked_sub(previous_end)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
            self.targets.insert(
                insertion,
                ResolvedText {
                    kind: ResolvedTargetKind::Prepared { handle },
                    gap_before,
                    text,
                    scalar_len,
                    base_runs: Vec::new(),
                    current_runs: runs,
                    action_slots: Vec::new(),
                },
            );
            previous_end = start
                .checked_add(scalar_len)
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        }
        Ok(())
    }
}
