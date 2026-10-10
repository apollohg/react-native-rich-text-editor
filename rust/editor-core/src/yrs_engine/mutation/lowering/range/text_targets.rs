struct TextTargetContext<'a, T> {
    request_id: u64,
    txn: &'a T,
    schema: &'a Schema,
}

struct TextTargetParent<'a> {
    id: BranchID,
    ancestors: &'a [(BranchID, u32)],
}

fn drive_lookup_materialization_collector<T: ReadTxn>(
    txn: &T,
    schema: &Schema,
    children: impl Iterator<Item = XmlOut>,
    collector: &mut ImportLookupMaterializationCollector,
    branch_index: &mut Option<crate::yrs_engine::block_branch_index::BlockBranchIndexCapture>,
) {
    for (ordinal, child) in children.enumerate() {
        if collector.has_failed() {
            break;
        }
        match child {
            XmlOut::Text(text) => {
                let mut capture = ImportTextCaptureWork::new();
                for diff in text.diff(txn, YChange::identity) {
                    let yrs::Out::Any(Any::String(value)) = diff.insert else {
                        collector
                            .invalidate("Yrs XML text materialization contains a non-string value");
                        break;
                    };
                    capture.observe(&value, diff.attributes.as_deref());
                    if let Some(message) = capture.failure() {
                        collector.invalidate(message);
                        break;
                    }
                }
                if collector.has_failed() {
                    break;
                }
                if let Some(index) = branch_index {
                    index.text(AsRef::<Branch>::as_ref(&text).id(), capture.scalar_len);
                }
                collector.observe_text(AsRef::<Branch>::as_ref(&text).id(), capture);
            }
            XmlOut::Element(element) => {
                let mut attributes = ImportElementAttributeWork::new();
                for (key, value) in element.attributes(txn) {
                    let yrs::Out::Any(value) = value else {
                        collector
                            .invalidate("XML element attribute resolved to a non-Any shared value");
                        break;
                    };
                    attributes.observe(key, &value);
                    if let Some(message) = attributes.failure() {
                        collector.invalidate(message);
                        break;
                    }
                }
                if collector.has_failed() {
                    break;
                }
                let (is_void, is_textblock) = wire_element_semantics(&element, txn, schema);
                if branch_index
                    .as_mut()
                    .is_some_and(|index| !index.enter(ordinal, &element, txn, schema, is_void))
                {
                    *branch_index = None;
                }
                let observe_children = collector.begin_element(
                    AsRef::<Branch>::as_ref(&element).id(),
                    attributes,
                    is_void,
                    is_textblock,
                );
                if collector.has_failed() {
                    break;
                }
                if observe_children {
                    drive_lookup_materialization_collector(
                        txn,
                        schema,
                        element.children(txn),
                        collector,
                        branch_index,
                    );
                    if !collector.has_failed() {
                        collector.end_container();
                        if let Some(index) = branch_index {
                            index.exit();
                        }
                    }
                }
            }
            XmlOut::Fragment(fragment) => {
                *branch_index = None;
                collector.begin_fragment();
                if collector.has_failed() {
                    break;
                }
                drive_lookup_materialization_collector(
                    txn,
                    schema,
                    fragment.children(txn),
                    collector,
                    branch_index,
                );
                if !collector.has_failed() {
                    collector.end_container();
                }
            }
        }
    }
}

fn build_lookup_seed_payload<T: ReadTxn>(
    request_id: u64,
    txn: &T,
    fragment: &XmlFragmentRef,
    schema: &Schema,
    target_capacity_hint: Option<usize>,
) -> OperationResult<MutationLookupPayload> {
    build_lookup_seed_payload_with_branch_index(
        request_id,
        txn,
        fragment,
        schema,
        target_capacity_hint,
        None,
    )
    .map(|(payload, _)| payload)
}

fn build_lookup_seed_payload_with_branch_index<T: ReadTxn>(
    request_id: u64,
    txn: &T,
    fragment: &XmlFragmentRef,
    schema: &Schema,
    target_capacity_hint: Option<usize>,
    position_map: Option<&crate::position::PositionMap>,
) -> OperationResult<(
    MutationLookupPayload,
    Option<Option<crate::yrs_engine::block_branch_index::BlockBranchIndex>>,
)> {
    let root_width = usize::try_from(fragment.len(txn)).map_err(|_| {
        OperationError::engine_invariant_failed(
            request_id,
            None,
            "Yrs fragment child count exceeds usize",
        )
    })?;
    let mut collector = ImportLookupMaterializationCollector::new(
        request_id,
        AsRef::<Branch>::as_ref(fragment).id(),
        root_width,
        target_capacity_hint,
    );
    let mut branch_index = position_map
        .filter(|_| !collector.has_failed())
        .map(crate::yrs_engine::block_branch_index::BlockBranchIndexCapture::new);
    drive_lookup_materialization_collector(
        txn,
        schema,
        fragment.children(txn),
        &mut collector,
        &mut branch_index,
    );
    let payload = collector.finish_payload()?;
    Ok((payload, branch_index.map(|index| index.finish())))
}

include!("legacy_lookup.rs");

#[cfg(test)]
fn measure_text_capture_work<T: ReadTxn>(
    request_id: u64,
    target: &XmlTextRef,
    txn: &T,
) -> OperationResult<usize> {
    let mut work = 0usize;
    let mut scalar_len = 0u32;
    let mut utf16_len = 0u32;
    for diff in target.diff(txn, YChange::identity) {
        let yrs::Out::Any(Any::String(value)) = diff.insert else {
            return Err(OperationError::engine_invariant_failed(
                request_id,
                None,
                "Yrs XML text materialization contains a non-string value",
            ));
        };
        if value.is_empty() {
            continue;
        }
        if value.is_ascii() {
            let len = u32::try_from(value.len()).map_err(|_| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "Yrs XML text length exceeds u32",
                )
            })?;
            scalar_len = scalar_len.checked_add(len).ok_or_else(|| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "Yrs XML text scalar length overflow",
                )
            })?;
            utf16_len = utf16_len.checked_add(len).ok_or_else(|| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "Yrs XML text UTF-16 length overflow",
                )
            })?;
        } else {
            for scalar in value.chars() {
                scalar_len = scalar_len.checked_add(1).ok_or_else(|| {
                    OperationError::engine_invariant_failed(
                        request_id,
                        None,
                        "Yrs XML text scalar length overflow",
                    )
                })?;
                let scalar_utf16_len = if scalar.len_utf16() == 1 { 1 } else { 2 };
                utf16_len = utf16_len.checked_add(scalar_utf16_len).ok_or_else(|| {
                    OperationError::engine_invariant_failed(
                        request_id,
                        None,
                        "Yrs XML text UTF-16 length overflow",
                    )
                })?;
            }
        }
        let attrs = diff.attributes.as_deref();
        let attrs_len = attrs.map_or(0, |attrs| attrs.len());
        work = work.checked_add(attrs_len).ok_or_else(|| {
            OperationError::engine_invariant_failed(
                request_id,
                None,
                "Yrs XML text materialization work overflow",
            )
        })?;
        let mut key_bytes = 0usize;
        if let Some(attrs) = attrs {
            for (key, value) in attrs.iter() {
                key_bytes = key_bytes.checked_add(key.len()).ok_or_else(|| {
                    OperationError::engine_invariant_failed(
                        request_id,
                        None,
                        "Yrs XML text materialization work overflow",
                    )
                })?;
                work = work
                    .checked_add(key.len())
                    .and_then(|work| work.checked_add(super::plan::any_preflight_work(value)?))
                    .ok_or_else(|| {
                        OperationError::engine_invariant_failed(
                            request_id,
                            None,
                            "Yrs XML text materialization work overflow",
                        )
                    })?;
            }
        }
        let partitions = binary_partition_work(attrs_len);
        work = work
            .checked_add(
                attrs_len
                    .checked_mul(partitions)
                    .and_then(|work| work.checked_add(key_bytes.checked_mul(partitions)?))
                    .ok_or_else(|| {
                        OperationError::engine_invariant_failed(
                            request_id,
                            None,
                            "Yrs XML text materialization work overflow",
                        )
                    })?,
            )
            .and_then(|work| work.checked_add(value.len()))
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "Yrs XML text materialization work overflow",
                )
            })?;
    }
    let _ = (scalar_len, utf16_len);
    Ok(work)
}

fn collect_text_targets<'a, T: ReadTxn>(
    context: &TextTargetContext<'_, T>,
    children: impl Iterator<Item = (u32, XmlOut)> + 'a,
    parent: TextTargetParent<'_>,
    position: u32,
    traversal_work: &mut usize,
    materialized_texts: &mut HashMap<BranchID, MaterializedText>,
    output: &mut Vec<LocatedTarget>,
) -> OperationResult<u32> {
    crate::boundary::with_document_stack(|| {
        collect_text_targets_inner(
            context,
            children,
            parent,
            position,
            traversal_work,
            materialized_texts,
            output,
        )
    })
}

fn collect_text_targets_inner<'a, T: ReadTxn>(
    context: &TextTargetContext<'_, T>,
    children: impl Iterator<Item = (u32, XmlOut)> + 'a,
    parent: TextTargetParent<'_>,
    mut position: u32,
    traversal_work: &mut usize,
    materialized_texts: &mut HashMap<BranchID, MaterializedText>,
    output: &mut Vec<LocatedTarget>,
) -> OperationResult<u32> {
    let request_id = context.request_id;
    let txn = context.txn;
    let schema = context.schema;
    let parent_id = parent.id;
    let ancestor_path = parent.ancestors;
    for (child_index, child) in children {
        *traversal_work = traversal_work.checked_add(1).ok_or_else(|| {
            OperationError::engine_invariant_failed(
                request_id,
                None,
                "Yrs mutation target traversal work overflow",
            )
        })?;
        let mut child_path = Vec::with_capacity(ancestor_path.len() + 1);
        child_path.push((parent_id.clone(), child_index));
        child_path.extend_from_slice(ancestor_path);
        *traversal_work = traversal_work
            .checked_add(child_path.len())
            .ok_or_else(|| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "Yrs mutation path capture work overflow",
                )
            })?;
        match child {
            XmlOut::Text(text) => {
                *traversal_work =
                    traversal_work
                        .checked_add(child_path.len())
                        .ok_or_else(|| {
                            OperationError::engine_invariant_failed(
                                request_id,
                                None,
                                "Yrs text signature preflight work overflow",
                            )
                        })?;
                let materialized = materialize_text(request_id, &text, txn)?;
                *traversal_work =
                    traversal_work
                        .checked_add(materialized.work)
                        .ok_or_else(|| {
                            OperationError::engine_invariant_failed(
                                request_id,
                                None,
                                "Yrs text materialization work overflow",
                            )
                        })?;
                let target_id = <XmlTextRef as AsRef<Branch>>::as_ref(&text).id();
                if materialized_texts
                    .insert(target_id.clone(), materialized.clone())
                    .is_some()
                {
                    return Err(OperationError::engine_invariant_failed(
                        request_id,
                        None,
                        "duplicate Yrs text materialization",
                    ));
                }
                output.push(LocatedTarget::Existing {
                    start: position,
                    signature: TargetSignature {
                        target: target_id,
                        path: child_path,
                        initial_len_utf16: materialized.utf16_len,
                        runs: materialized.signature_runs.clone(),
                        capture_work: materialized.work,
                    },
                    target: text,
                    text: materialized.text.clone(),
                    scalar_len: materialized.scalar_len,
                });
                position = position
                    .checked_add(materialized.scalar_len)
                    .ok_or_else(|| {
                        OperationError::engine_invariant_failed(
                            request_id,
                            None,
                            "Yrs mutation target position overflow",
                        )
                    })?;
            }
            XmlOut::Element(element) => {
                let (is_void, is_textblock) = wire_element_semantics(&element, txn, schema);
                if is_void {
                    position = position.checked_add(1).ok_or_else(|| {
                        OperationError::engine_invariant_failed(
                            request_id,
                            None,
                            "Yrs void target position overflow",
                        )
                    })?;
                } else {
                    position = position.checked_add(1).ok_or_else(|| {
                        OperationError::engine_invariant_failed(
                            request_id,
                            None,
                            "Yrs element target position overflow",
                        )
                    })?;
                    if is_textblock {
                        let children = element.children(txn).collect::<Vec<_>>();
                        *traversal_work = traversal_work
                            .checked_add(children.len())
                            .and_then(|work| work.checked_add(children.len()))
                            .and_then(|work| work.checked_add(child_path.len()))
                            .ok_or_else(|| {
                                OperationError::engine_invariant_failed(
                                    request_id,
                                    None,
                                    "Yrs textblock materialization work overflow",
                                )
                            })?;
                        let child_count = u32::try_from(children.len()).map_err(|_| {
                            OperationError::engine_invariant_failed(
                                request_id,
                                None,
                                "Yrs textblock child count exceeds u32",
                            )
                        })?;
                        let mut previous_was_text = false;
                        for (index, child) in children.iter().cloned().enumerate() {
                            let index = u32::try_from(index).map_err(|_| {
                                OperationError::engine_invariant_failed(
                                    request_id,
                                    None,
                                    "Yrs textblock child index exceeds u32",
                                )
                            })?;
                            let child_is_text = matches!(child, XmlOut::Text(_));
                            if !previous_was_text && !child_is_text {
                                *traversal_work = traversal_work
                                    .checked_add(child_path.len())
                                    .ok_or_else(|| {
                                        OperationError::engine_invariant_failed(
                                            request_id,
                                            None,
                                            "Yrs missing-gap signature work overflow",
                                        )
                                    })?;
                                output.push(LocatedTarget::Missing {
                                    start: position,
                                    parent: element.clone(),
                                    child_index: index,
                                    signature: parent_signature_from_children(
                                        &element,
                                        &child_path,
                                        &children,
                                        child_count,
                                        index,
                                    ),
                                });
                            }
                            position = collect_text_targets(
                                context,
                                std::iter::once((index, child)),
                                TextTargetParent {
                                    id: <XmlElementRef as AsRef<Branch>>::as_ref(&element).id(),
                                    ancestors: &child_path,
                                },
                                position,
                                traversal_work,
                                materialized_texts,
                                output,
                            )?;
                            previous_was_text = child_is_text;
                        }
                        if !previous_was_text {
                            *traversal_work = traversal_work
                                .checked_add(child_path.len())
                                .ok_or_else(|| {
                                    OperationError::engine_invariant_failed(
                                        request_id,
                                        None,
                                        "Yrs missing-gap signature work overflow",
                                    )
                                })?;
                            output.push(LocatedTarget::Missing {
                                start: position,
                                parent: element.clone(),
                                child_index: child_count,
                                signature: parent_signature_from_children(
                                    &element,
                                    &child_path,
                                    &children,
                                    child_count,
                                    child_count,
                                ),
                            });
                        }
                    } else {
                        position = collect_text_targets(
                            context,
                            (0u32..).zip(element.children(txn)),
                            TextTargetParent {
                                id: <XmlElementRef as AsRef<Branch>>::as_ref(&element).id(),
                                ancestors: &child_path,
                            },
                            position,
                            traversal_work,
                            materialized_texts,
                            output,
                        )?;
                    }
                    position = position.checked_add(1).ok_or_else(|| {
                        OperationError::engine_invariant_failed(
                            request_id,
                            None,
                            "Yrs element target position overflow",
                        )
                    })?;
                }
            }
            XmlOut::Fragment(fragment) => {
                position = collect_text_targets(
                    context,
                    (0u32..).zip(fragment.children(txn)),
                    TextTargetParent {
                        id: <XmlFragmentRef as AsRef<Branch>>::as_ref(&fragment).id(),
                        ancestors: &child_path,
                    },
                    position,
                    traversal_work,
                    materialized_texts,
                    output,
                )?;
            }
        }
    }
    Ok(position)
}

#[cfg(test)]
pub(crate) fn assert_lookup_and_branch_index_parity_for_test<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    schema: &Schema,
    position_map: &crate::position::PositionMap,
    expected_capture: Option<bool>,
) {
    const REQUEST: u64 = 91_003;
    let ordinary = build_lookup_seed_payload(REQUEST, txn, fragment, schema, None);
    let combined = build_lookup_seed_payload_with_branch_index(
        REQUEST,
        txn,
        fragment,
        schema,
        None,
        Some(position_map),
    );
    let ((ordinary, combined), captured) = match (ordinary, combined) {
        (Ok(ordinary), Ok((combined, captured))) => ((ordinary, combined), captured),
        (Err(ordinary), Err(combined)) => {
            assert_eq!(
                ordinary, combined,
                "lookup errors must precede optional indexing"
            );
            return;
        }
        (ordinary, combined) => panic!(
            "lookup outcome changed: ordinary={:?}, combined={:?}",
            ordinary.err(),
            combined.err()
        ),
    };
    assert_eq!(ordinary.target_count, combined.target_count);
    assert_eq!(
        ordinary.pending_traversal_work,
        combined.pending_traversal_work
    );
    assert_eq!(ordinary.path_parent_widths, combined.path_parent_widths);
    assert_eq!(
        ordinary.target_materialization_work,
        combined.target_materialization_work
    );
    assert_eq!(
        ordinary.path_parent_widths.capacity(),
        combined.path_parent_widths.capacity()
    );
    if let Some(expected) = expected_capture {
        assert_eq!(
            captured.is_some(),
            expected,
            "capture must distinguish fallback from completed/unavailable"
        );
    }
    let walked = crate::yrs_engine::block_branch_index::BlockBranchIndex::build(
        txn,
        fragment,
        schema,
        position_map,
    );
    if let Some(captured) = captured {
        assert_eq!(
            captured, walked,
            "captured branch IDs, table paths and atom IDs must match the independent walker"
        );
        if let (Some(captured), Some(walked)) = (captured, walked) {
            captured.assert_same_allocations_for_test(&walked);
        }
    }
}
