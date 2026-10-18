pub(crate) fn crdt_envelope<T: ReadTxn>(
    request_id: u64,
    _read_lock: &T,
    plan: &YrsMutationPlan,
    snapshot_scan_reservation: usize,
) -> OperationResult<CrdtEnvelope> {
    if snapshot_scan_reservation == 0 {
        return Err(OperationError::operation_limit_exceeded(
            request_id,
            None,
            "maxEncodedStateBytes",
            0,
            1,
        ));
    }
    let mut text_actions = HashMap::<BranchID, usize>::new();
    let mut child_actions = HashMap::<BranchID, usize>::new();
    for action in &plan.actions {
        match action {
            YrsMutationAction::InsertText { target, .. }
            | YrsMutationAction::DeleteText { target, .. }
            | YrsMutationAction::FormatText { target, .. } => {
                *text_actions
                    .entry(AsRef::<Branch>::as_ref(target).id())
                    .or_default() += 1;
            }
            YrsMutationAction::DeleteXmlChildren { parent, .. }
            | YrsMutationAction::InsertXmlChildren { parent, .. } => {
                *child_actions.entry(parent.id()).or_default() += 1;
            }
            YrsMutationAction::CreateText { parent, .. } => {
                *child_actions
                    .entry(AsRef::<Branch>::as_ref(parent).id())
                    .or_default() += 1;
            }
            YrsMutationAction::SetXmlAttribute { .. }
            | YrsMutationAction::RemoveXmlAttribute { .. } => {}
        }
    }
    let mut clients = HashSet::new();
    let mut live_clock_units = 0u64;
    let mut overflowed = false;
    let mut visit = |client, len: u32| {
        clients.insert(client);
        match live_clock_units.checked_add(u64::from(len)) {
            Some(units) => live_clock_units = units,
            None => overflowed = true,
        }
    };
    let mut whole_targets = HashSet::new();
    let mut scan_work = 0usize;
    for action in &plan.actions {
        let action_work = match action {
            YrsMutationAction::DeleteText {
                target,
                index_utf16,
                len_utf16,
                ..
            }
            | YrsMutationAction::FormatText {
                target,
                index_utf16,
                len_utf16,
                ..
            } => {
                let branch = AsRef::<Branch>::as_ref(target);
                if text_actions.get(&branch.id()) == Some(&SINGLE_SCOPED_ACTION) {
                    branch.visit_text_range_deletion_scope(
                        OffsetKind::Utf16,
                        *index_utf16,
                        *len_utf16,
                        &mut visit,
                    )
                } else if whole_targets.insert(branch.id()) {
                    branch.visit_text_range_deletion_scope(
                        OffsetKind::Utf16,
                        0,
                        u32::MAX,
                        &mut visit,
                    )
                } else {
                    0
                }
            }
            YrsMutationAction::DeleteXmlChildren {
                parent,
                child_index,
                child_count,
                ..
            } => {
                let branch = parent.branch();
                if child_actions.get(&branch.id()) == Some(&SINGLE_SCOPED_ACTION) {
                    branch.visit_child_range_deletion_scope(
                        *child_index,
                        *child_count,
                        &mut visit,
                    )
                } else if whole_targets.insert(branch.id()) {
                    branch.visit_child_range_deletion_scope(0, u32::MAX, &mut visit)
                } else {
                    0
                }
            }
            YrsMutationAction::RemoveXmlAttribute { target, key, .. } => {
                AsRef::<Branch>::as_ref(target).visit_map_entry_deletion_scope(key, &mut visit)
            }
            YrsMutationAction::SetXmlAttribute {
                target,
                key,
                signature,
                ..
            } if signature
                .attrs
                .binary_search_by(|(candidate, _)| candidate.as_ref().cmp(key.as_ref()))
                .is_ok() =>
            {
                AsRef::<Branch>::as_ref(target).visit_map_entry_deletion_scope(key, &mut visit)
            }
            YrsMutationAction::SetXmlAttribute { .. }
            | YrsMutationAction::InsertText { .. }
            | YrsMutationAction::InsertXmlChildren { .. }
            | YrsMutationAction::CreateText { .. } => 0,
        };
        scan_work = scan_work
            .checked_add(action_work)
            .ok_or_else(|| snapshot_metric_overflow(request_id))?;
    }
    if overflowed {
        return Err(snapshot_metric_overflow(request_id));
    }
    if scan_work > snapshot_scan_reservation {
        return Err(OperationError::operation_limit_exceeded(
            request_id,
            None,
            "maxEncodedStateBytes",
            u64::try_from(snapshot_scan_reservation).unwrap_or(u64::MAX),
            u64::try_from(scan_work).unwrap_or(u64::MAX),
        ));
    }
    Ok(CrdtEnvelope {
        live_clock_units,
        client_count: clients.len(),
        scan_work,
    })
}

const SINGLE_SCOPED_ACTION: usize = 1;

pub(crate) fn crdt_clock_scan_reservation<T: ReadTxn>(
    request_id: u64,
    txn: &T,
    reservation: usize,
) -> OperationResult<usize> {
    if txn.store().pending_update().is_some() || txn.store().pending_ds().is_some() {
        return Err(OperationError::engine_not_ready(request_id));
    }
    let state = txn.state_vector();
    let work = state.iter().try_fold(0usize, |work, (_, clock)| {
        work.checked_add(usize::try_from(*clock).ok()?)?
            .checked_add(1)
    });
    let work = work.ok_or_else(|| snapshot_metric_overflow(request_id))?;
    if work > reservation {
        return Err(OperationError::operation_limit_exceeded(
            request_id,
            None,
            "maxEncodedStateBytes",
            u64::try_from(reservation).unwrap_or(u64::MAX),
            u64::try_from(work).unwrap_or(u64::MAX),
        ));
    }
    Ok(work)
}

pub(super) fn capture_document_guard<T: ReadTxn>(
    request_id: u64,
    txn: &T,
) -> OperationResult<DocumentGuard> {
    capture_document_guard_with_read_scope(request_id, txn, None)
}

pub(super) fn capture_document_guard_with_read_scope<T: ReadTxn>(
    request_id: u64,
    txn: &T,
    scope: Option<crate::yrs_engine::compiler::CompilationReadScope<'_>>,
) -> OperationResult<DocumentGuard> {
    if txn.store().pending_update().is_some() || txn.store().pending_ds().is_some() {
        return Err(OperationError::engine_not_ready(request_id));
    }
    let evidence = if let Some(scope) = scope {
        if !scope.matches_store(txn) {
            return Err(OperationError::engine_invariant_failed(
                request_id,
                None,
                "Yrs mutation guard read scope belongs to a different document store",
            ));
        }
        DocumentGuardEvidence::Held {
            state: txn.state_vector(),
            scope: scope.stamp(),
        }
    } else {
        #[cfg(test)]
        crate::yrs_engine::observability::record_mutation_guard_snapshot_request();
        DocumentGuardEvidence::Snapshot(txn.snapshot())
    };
    let state_clock_work = snapshot_state_clock_work(request_id, evidence.state())?;
    Ok(DocumentGuard {
        store_token: txn.store() as *const _ as usize,
        evidence,
        state_clock_work,
    })
}

pub(super) fn snapshot_state_clock_work(
    request_id: u64,
    state: &yrs::StateVector,
) -> OperationResult<usize> {
    state
        .iter()
        .try_fold(0usize, |work, (_, clock)| {
            work.checked_add(usize::try_from(*clock).ok()?)?
                .checked_add(1)
        })
        .ok_or_else(|| snapshot_metric_overflow(request_id))
}

fn snapshot_metric_overflow(request_id: u64) -> OperationError {
    OperationError::engine_invariant_failed(request_id, None, "Yrs snapshot metric overflow")
}
