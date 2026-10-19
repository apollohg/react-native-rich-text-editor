impl MutationCompiler {
    pub(crate) fn join_blocks(
        &mut self,
        operation_index: usize,
        context: MutationDocumentContext<'_>,
        position: u32,
    ) -> OperationResult<()> {
        let document = context.before;
        let resolved = document.resolve(position).map_err(|message| {
            OperationError::operation_invalid(self.request_id, operation_index, "at", message)
        })?;
        let parent = resolved.parent(document);
        let content = parent.content().ok_or_else(|| {
            OperationError::operation_invalid(
                self.request_id,
                operation_index,
                "at",
                "join parent has no content",
            )
        })?;
        let mut offset = 0u32;
        let mut boundary = None;
        for (index, child) in content.iter().enumerate() {
            if offset == resolved.parent_offset && index > 0 {
                boundary = Some(index);
                break;
            }
            offset = offset
                .checked_add(child.node_size())
                .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        }
        let boundary = boundary.ok_or_else(|| {
            OperationError::operation_invalid(
                self.request_id,
                operation_index,
                "at",
                "join position is not between adjacent blocks",
            )
        })?;
        let right_node = content
            .child(boundary)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let suffix = right_node
            .content()
            .cloned()
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let retained_end = position
            .checked_sub(crate::tables::commands::NODE_CLOSING_TOKENS)
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        let removed_end = position
            .checked_add(right_node.node_size())
            .ok_or_else(|| invalid_action_range(self.request_id, operation_index))?;
        self.merge_following_blocks_into_retained(
            operation_index,
            context,
            retained_end,
            removed_end,
            &suffix,
        )
    }

    pub(crate) fn finish(
        mut self,
        operation_index: Option<usize>,
    ) -> OperationResult<YrsMutationPlan> {
        let mut pending = std::mem::take(&mut self.pending_element_attrs)
            .into_values()
            .collect::<Vec<_>>();
        pending.sort_by_key(|entry| entry.first_order);
        for entry in pending {
            let mut concrete = Vec::new();
            append_attribute_diff(&mut concrete, entry);
            self.actions
                .extend(concrete.into_iter().map(ActionSlot::concrete));
        }
        self.charged_work = self
            .charged_work
            .checked_add(self.pending_traversal_work)
            .ok_or_else(|| {
                OperationError::operation_limit_exceeded(
                    self.request_id,
                    operation_index,
                    "maxActionsPerTransaction",
                    u64::try_from(self.action_limit).unwrap_or(u64::MAX),
                    u64::MAX,
                )
            })?;
        self.pending_traversal_work = 0;
        if self.charged_work > self.action_limit {
            return Err(OperationError::operation_limit_exceeded(
                self.request_id,
                operation_index,
                "maxActionsPerTransaction",
                u64::try_from(self.action_limit).unwrap_or(u64::MAX),
                u64::try_from(self.charged_work).unwrap_or(u64::MAX),
            ));
        }
        let path_parent_widths = self.explicit_path_parent_widths.take().unwrap_or_else(|| {
            self.structural_parents
                .values()
                .map(|parent| {
                    (
                        parent.signature.parent.clone(),
                        parent.signature.children.len(),
                    )
                })
                .collect::<HashMap<_, _>>()
        });
        let mut actions = Vec::new();
        for slot in self.actions {
            match slot {
                ActionSlot::Concrete(action) => actions.push(*action),
                ActionSlot::PreparedInsert(insert_id) => {
                    let insert = self
                        .prepared_inserts
                        .get_mut(insert_id)
                        .and_then(Option::take)
                        .ok_or_else(|| {
                            OperationError::engine_invariant_failed(
                                self.request_id,
                                operation_index,
                                "prepared insertion action has no owned blueprint",
                            )
                        })?;
                    actions.push(YrsMutationAction::InsertXmlChildren {
                        parent: insert.parent,
                        child_index: insert.child_index,
                        nodes: insert.nodes,
                        signature: insert.signature,
                        operation_index: insert.operation_index,
                    });
                }
                ActionSlot::Tombstone => {}
            }
        }
        let actions = merge_same_index_insertions(self.request_id, operation_index, actions)?;
        let expected_preflight_work =
            expected_preflight_work(self.request_id, &actions, &path_parent_widths)?;
        Ok(YrsMutationPlan {
            actions,
            compilation_work: self.charged_work,
            expected_preflight_work,
            work_limit: self.action_limit,
            document_guard: Some(self.document_guard),
            prepared_metrics: Vec::new(),
            scan_work: self.scan_work,
            #[cfg(test)]
            position_resolver_work: self.position_resolver_work,
        })
    }
}

fn merge_same_index_insertions(
    request_id: u64,
    operation_index: Option<usize>,
    actions: Vec<YrsMutationAction>,
) -> OperationResult<Vec<YrsMutationAction>> {
    let invalid = || {
        OperationError::engine_invariant_failed(
            request_id,
            operation_index,
            "merged prepared insertion index overflow",
        )
    };
    let mut merged = Vec::with_capacity(actions.len());
    let mut last_child_list_action = HashMap::<BranchID, usize>::new();
    for action in actions {
        let parent_id = match &action {
            YrsMutationAction::InsertXmlChildren { parent, .. }
            | YrsMutationAction::DeleteXmlChildren { parent, .. } => Some(parent.id()),
            YrsMutationAction::CreateText { parent, .. } => {
                Some(AsRef::<Branch>::as_ref(parent).id())
            }
            _ => None,
        };
        let Some(parent_id) = parent_id else {
            merged.push(action);
            continue;
        };
        if let YrsMutationAction::InsertXmlChildren {
            child_index, nodes, ..
        } = &action
        {
            if let Some(YrsMutationAction::InsertXmlChildren {
                child_index: earlier_index,
                nodes: earlier_nodes,
                ..
            }) = last_child_list_action
                .get(&parent_id)
                .and_then(|&earlier| merged.get_mut(earlier))
            {
                if earlier_index == child_index {
                    let mut combined = nodes.clone();
                    combined.append(earlier_nodes);
                    for (ordinal, child) in combined.iter_mut().enumerate() {
                        child.index = u32::try_from(ordinal)
                            .ok()
                            .and_then(|ordinal| earlier_index.checked_add(ordinal))
                            .ok_or_else(invalid)?;
                    }
                    *earlier_nodes = combined;
                    continue;
                }
            }
        }
        last_child_list_action.insert(parent_id, merged.len());
        merged.push(action);
    }
    Ok(merged)
}
