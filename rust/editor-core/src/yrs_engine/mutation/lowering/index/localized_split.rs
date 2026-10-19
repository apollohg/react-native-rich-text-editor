impl LocalizedSplitCompiler {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_new<T: ReadTxn>(
        request_id: u64,
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
        action_limit: usize,
        scan_limit: usize,
        scan_work: usize,
        locator: LocalizedSplitLocator<'_>,
        seed: &MutationLookupSeed,
        resource_limits: &ResourceLimits,
        editing_limits: &EditingLimits,
        max_length: Option<u32>,
        schema_fingerprint: &str,
        yrs_state_epoch: u64,
        document_revision: u64,
    ) -> OperationResult<Option<Self>> {
        #[cfg(test)]
        if FORCE_EAGER_MUTATION_LOWERING.get() {
            return Ok(None);
        }
        if !seed.matches(
            txn,
            fragment,
            locator.document,
            resource_limits,
            editing_limits,
            max_length,
            schema_fingerprint,
            yrs_state_epoch,
            document_revision,
        ) {
            return Ok(None);
        }
        let Some(seed_payload) = seed.ready_payload() else {
            return Ok(None);
        };
        let Ok(resolved) = locator.document.resolve(locator.position) else {
            return Ok(None);
        };
        let Some(&child_index) = resolved.node_path.first() else {
            return Ok(None);
        };
        let Some(semantic_root) = locator.document.root().content() else {
            return Ok(None);
        };
        let Ok(root_child) = usize::try_from(child_index) else {
            return Ok(None);
        };
        let Some(semantic_child) = semantic_root
            .child(root_child)
            .filter(|child| child.is_element())
        else {
            return Ok(None);
        };
        let Some(child_start) = semantic_root
            .iter()
            .take(root_child)
            .try_fold(0u32, |offset, child| offset.checked_add(child.node_size()))
        else {
            return Ok(None);
        };
        let document_guard = capture_document_guard(request_id, txn)?;
        if fragment
            .children(txn)
            .any(|child| !matches!(child, XmlOut::Element(_)))
        {
            return Ok(None);
        }
        let mut root_traversal_work = 0usize;
        let (root, _) = structural_parent_target(
            request_id,
            txn,
            XmlParentRef::Fragment(fragment.clone()),
            &[],
            &mut root_traversal_work,
            &HashMap::new(),
        )?;
        if root.storage_children.len() != semantic_root.child_count() {
            return Ok(None);
        }
        let Some(StorageChildKind::Element {
            target: element, ..
        }) = root.storage_children.get(root_child).cloned()
        else {
            return Ok(None);
        };
        if wire_element_is_semantic_void(&element, txn, schema) {
            return Ok(None);
        }
        let root_id = root.signature.parent.clone();
        let mut located = Vec::new();
        let mut materialized_texts = HashMap::new();
        let mut traversal_work = 0usize;
        let child_end = collect_text_targets(
            &TextTargetContext {
                request_id,
                txn,
                schema,
            },
            std::iter::once((child_index, XmlOut::Element(element.clone()))),
            TextTargetParent {
                id: root_id.clone(),
                ancestors: &[],
            },
            child_start,
            &mut traversal_work,
            &mut materialized_texts,
            &mut located,
        )?;
        if child_start.checked_add(semantic_child.node_size()) != Some(child_end) {
            return Ok(None);
        }
        let mut structural_parents = HashMap::new();
        collect_structural_parents(
            request_id,
            txn,
            XmlParentRef::Element(element),
            vec![child_index],
            vec![(root_id, child_index)],
            schema,
            &mut traversal_work,
            &materialized_texts,
            &mut structural_parents,
        )?;
        structural_parents.insert(Vec::new(), root);
        if structural_parents.values().any(|parent| {
            seed_payload
                .path_parent_widths
                .get(&parent.signature.parent)
                .copied()
                != Some(parent.signature.children.len())
        }) {
            return Ok(None);
        }
        let targets = resolve_located_targets(request_id, located, &structural_parents)?;
        if targets.iter().any(|target| match &target.kind {
            ResolvedTargetKind::Existing { signature, .. } => {
                seed_payload
                    .target_materialization_work
                    .get(&signature.target)
                    .copied()
                    != Some(signature.capture_work)
            }
            ResolvedTargetKind::Missing { .. } => false,
            ResolvedTargetKind::Prepared { .. } => true,
        }) {
            return Ok(None);
        }
        let Some(complement) = structural_window_complement(
            locator.document,
            root_child,
            &structural_parents,
            targets.len(),
            seed_payload,
        ) else {
            return Ok(None);
        };
        #[cfg(test)]
        LOCALIZED_SPLIT_HIT_COUNT.set(LOCALIZED_SPLIT_HIT_COUNT.get().saturating_add(1));
        Ok(Some(Self {
            compiler: MutationCompiler {
                request_id,
                document_guard,
                targets,
                structural_parents,
                actions: Vec::new(),
                prepared_inserts: Vec::new(),
                prepared_elements: HashMap::new(),
                charged_work: 0,
                pending_traversal_work: seed_payload.pending_traversal_work,
                localized_position_target_count: None,
                explicit_path_parent_widths: None,
                structural_window_complement: Some(complement),
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
            },
            document: locator.document.clone(),
            position: locator.position,
        }))
    }

    pub(crate) fn split_block(
        mut self,
        operation_index: usize,
        before: &Document,
        after: &Document,
        position: u32,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> OperationResult<YrsMutationPlan> {
        if !before.shares_root_storage_with(&self.document) || position != self.position {
            return Err(OperationError::engine_invariant_failed(
                self.compiler.request_id,
                Some(operation_index),
                "localized split candidate is stale",
            ));
        }
        self.compiler
            .split_block(operation_index, before, after, position, schema, limits)?;
        self.compiler.finish(Some(operation_index))
    }
}

fn structural_window_complement(
    document: &Document,
    root_child: usize,
    window_parents: &HashMap<Vec<u32>, StructuralParentTarget>,
    window_targets: usize,
    seed: &MutationLookupPayload,
) -> Option<StructuralWindowComplement> {
    let root = document.root().content()?;
    let mut element_count = 0usize;
    let mut semantic_layout_work = 0usize;
    let mut content_position_work = 0usize;
    let mut pending = root
        .iter()
        .enumerate()
        .filter(|(index, child)| *index != root_child && child.is_element())
        .map(|(_, child)| (child, 1usize))
        .collect::<Vec<_>>();
    while let Some((node, depth)) = pending.pop() {
        element_count = element_count.checked_add(1)?;
        semantic_layout_work = semantic_layout_work
            .checked_add(depth)?
            .checked_add(node.child_count())?;
        content_position_work = content_position_work.checked_add(depth.checked_add(1)?)?;
        let child_depth = depth.checked_add(1)?;
        pending.extend(
            node.content()?
                .iter()
                .filter(|child| child.is_element())
                .map(|child| (child, child_depth)),
        );
    }
    let parent_count = seed
        .path_parent_widths
        .len()
        .checked_sub(window_parents.len())?;
    if parent_count != element_count {
        return None;
    }
    let seed_widths = seed
        .path_parent_widths
        .values()
        .try_fold(0usize, |sum, width| sum.checked_add(*width))?;
    let window_widths = window_parents.values().try_fold(0usize, |sum, parent| {
        sum.checked_add(parent.signature.children.len())
    })?;
    Some(StructuralWindowComplement {
        root_child,
        root_children: root.child_count().checked_sub(1)?,
        parent_count,
        target_count: seed.target_count.checked_sub(window_targets)?,
        parent_layout_work: seed_widths
            .checked_sub(window_widths)?
            .checked_add(semantic_layout_work)?,
        content_position_work,
    })
}
