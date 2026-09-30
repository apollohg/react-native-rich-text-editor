#[cfg(test)]
use crate::yrs_engine;

use crate::transform::StepMap;
use crate::yrs_engine::compiler::input_limits::{
    admit_transaction_envelope, admit_yrs_scan_work, input_limit_error,
};
#[cfg(test)]
use crate::yrs_engine::compiler::observability::{check_atomic_failpoint, AtomicFailpoint};
use crate::yrs_engine::compiler::positions::resolve_range;
use crate::yrs_engine::compiler::preview::try_localized_semantic_compilation;
use crate::yrs_engine::compiler::selection::planned_relative_selection;
use crate::yrs_engine::compiler::semantic::compile_transaction_impl;
use crate::yrs_engine::compiler::{
    base_document_text_bytes, CachedCompilationView, CompilationContext, CompiledTransaction,
    EngineCompilationView, PreparedSemanticContext, RelativeSelectionPlan, SelectionPlan,
    SemanticCompilationShortcuts, StoredMarksCompilationContext, TransactionMutationLowering,
};
use crate::yrs_engine::mutation::{
    crdt_envelope, preflight_mutation_plan, LocalizedFormatCompiler, LocalizedFormatLocator,
    LocalizedInsertCompiler, LocalizedInsertLocator, LocalizedRootWindowCompiler,
    LocalizedRootWindowLocator, MutationCompiler,
};
use crate::yrs_engine::{
    OperationError, OperationResult, SelectionIntent, TypedOperation, TypedTransaction,
};

// This cache cannot outlive the immutable read view held by one compilation.
struct CompilationReadView<'a, T> {
    txn: &'a T,
    snapshot: std::cell::OnceCell<yrs::Snapshot>,
}

impl<T: yrs::ReadTxn> yrs::ReadTxn for CompilationReadView<'_, T> {
    fn store(&self) -> &yrs::Store {
        self.txn.store()
    }

    fn snapshot(&self) -> yrs::Snapshot {
        #[cfg(test)]
        if self.snapshot.get().is_some() {
            crate::yrs_engine::observability::record_compilation_snapshot_reuse();
        }
        self.snapshot
            .get_or_init(|| {
                #[cfg(test)]
                crate::yrs_engine::observability::record_compilation_snapshot_scan();
                self.txn.snapshot()
            })
            .clone()
    }
}

pub(super) fn compile_transaction_with_yrs_impl<T: yrs::ReadTxn>(
    context: CompilationContext<'_>,
    transaction: TypedTransaction,
    txn: &T,
    fragment: &yrs::types::xml::XmlFragmentRef,
    stored_marks: Option<StoredMarksCompilationContext<'_>>,
    prepared_semantics: Option<PreparedSemanticContext<'_>>,
    engine_view: Option<EngineCompilationView<'_>>,
) -> OperationResult<CompiledTransaction> {
    let read_view = CompilationReadView {
        txn,
        snapshot: std::cell::OnceCell::new(),
    };
    let txn = &read_view;
    let request_id = transaction.request_id;
    #[cfg(test)]
    check_atomic_failpoint(request_id, AtomicFailpoint::EnvelopeAdmission)?;
    let admitted_input_bytes = admit_transaction_envelope(context, &transaction)?;
    #[cfg(test)]
    check_atomic_failpoint(request_id, AtomicFailpoint::SemanticCompilation)?;
    let action_multiplier = context
        .editing_limits
        .max_operations_per_transaction
        .checked_add(context.resource_limits.max_document_depth)
        .and_then(|value| value.checked_add(2))
        .ok_or_else(|| {
            OperationError::operation_limit_exceeded(
                request_id,
                None,
                "maxActionsPerTransaction",
                u64::MAX,
                u64::MAX,
            )
        })?;
    let action_limit = action_multiplier
        .checked_mul(context.resource_limits.max_document_nodes)
        .ok_or_else(|| {
            OperationError::operation_limit_exceeded(
                request_id,
                None,
                "maxActionsPerTransaction",
                u64::MAX,
                u64::MAX,
            )
        })?;
    if let Some(prepared) = prepared_semantics {
        let canonical_schema = engine_view
            .map(|view| view.cached.canonical_artifact.schema_context())
            .ok_or_else(|| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "prepared semantic compilation has no live canonical schema context",
                )
            })?;
        prepared.admission.admit(
            &transaction,
            prepared.expected_preview,
            context.document_revision,
            prepared.state_revision,
            prepared.yrs_state_epoch,
            prepared.schema_fingerprint,
            context.resource_limits,
            context.editing_limits,
            context.max_length,
            canonical_schema,
        )?;
    }
    let cached_view = engine_view
        .map(|view| validate_cached_compilation_view(request_id, context, view))
        .transpose()?;
    let document_text_bytes = cached_view
        .map(|view| view.document_text_bytes)
        .or_else(|| base_document_text_bytes(context.document))
        .ok_or_else(|| {
            OperationError::operation_limit_exceeded(
                request_id,
                None,
                "maxInputBytes",
                u64::try_from(context.resource_limits.max_input_bytes).unwrap_or(u64::MAX),
                u64::MAX,
            )
        })?;
    let charged_scan_work = admit_yrs_scan_work(
        request_id,
        admitted_input_bytes,
        document_text_bytes,
        txn,
        context.resource_limits,
    )?;
    let localized_textblock_edit_admission = engine_view.and_then(|view| {
        let (_, edit) = view
            .authority
            .installed()
            .textblock_edit_for_transaction(&transaction)?;
        Some(edit.replaced.start).and_then(|document_position| {
            view.authority
                .installed()
                .admit_textblock_edit_with_authority(
                    &transaction,
                    document_position,
                    txn,
                    fragment,
                    view.authority.lookup_seed(request_id).ok()?,
                    view.authority.materialized_identity(),
                    context.schema,
                    view.schema_fingerprint,
                    context.resource_limits,
                    context.editing_limits,
                    context.max_length,
                    view.yrs_state_epoch,
                )
        })
    });
    let mut localized_compiler = None;
    if let Some(view) = engine_view {
        if !matches!(transaction.selection_intent, SelectionIntent::Set(_)) {
            if let Some((_, edit)) = view
                .authority
                .installed()
                .textblock_edit_for_transaction(&transaction)
                .filter(|(_, edit)| {
                    edit.replaced.is_empty() || localized_textblock_edit_admission.is_some()
                })
            {
                let position = edit.replaced.start;
                if let Some(block) = view
                    .cached
                    .position_map
                    .find_block_for_doc_pos(position)
                    .and_then(|index| view.cached.position_map.block(index))
                {
                    if let Ok(authority_lookup_seed) = view.authority.lookup_seed(request_id) {
                        if let Some(localized) = LocalizedInsertCompiler::try_new(
                            request_id,
                            txn,
                            fragment,
                            context.schema,
                            action_limit,
                            context.resource_limits.max_input_bytes,
                            charged_scan_work,
                            LocalizedInsertLocator {
                                document: context.document,
                                block_path: block.node_path.as_slice(),
                                position,
                            },
                            authority_lookup_seed.as_ref(),
                            context.resource_limits,
                            context.editing_limits,
                            context.max_length,
                            view.schema_fingerprint,
                            view.yrs_state_epoch,
                            context.document_revision,
                        )? {
                            localized_compiler = Some(localized);
                        }
                    }
                }
            }
        }
    }
    let mut localized_format_compiler = None;
    let mut localized_root_window_compiler = None;
    #[cfg(test)]
    let localized_format_candidate = prepared_semantics.is_some()
        && matches!(
            transaction.operations.as_slice(),
            [TypedOperation::AddMark { .. } | TypedOperation::RemoveMark { .. }]
        );
    #[cfg(test)]
    let localized_root_window_candidate = prepared_semantics.is_some()
        && matches!(
            transaction.operations.as_slice(),
            [TypedOperation::ReplaceStructure(replacement)] if replacement.parent_path().is_empty()
        );
    if prepared_semantics.is_some() {
        if let (Some(view), [operation]) = (engine_view, transaction.operations.as_slice()) {
            let range = match operation {
                TypedOperation::AddMark { range, .. }
                | TypedOperation::RemoveMark { range, .. } => Some(*range),
                _ => None,
            };
            if let Some(range) = range {
                if let Ok((from, to)) = resolve_range(
                    request_id,
                    0,
                    range,
                    view.cached.rendered_text,
                    view.cached.position_map,
                    view.cached.document,
                    &StepMap::empty(),
                ) {
                    if let Some(last) = to.checked_sub(1) {
                        let from_block = view.cached.position_map.find_block_for_doc_pos(from);
                        let to_block = view.cached.position_map.find_block_for_doc_pos(last);
                        if from < to && from_block == to_block {
                            if let Some(block) =
                                from_block.and_then(|index| view.cached.position_map.block(index))
                            {
                                if let Ok(authority_lookup_seed) =
                                    view.authority.lookup_seed(request_id)
                                {
                                    if let Some(locator) = LocalizedFormatLocator::mint(
                                        context.document,
                                        block.node_path.as_slice(),
                                        from,
                                        to,
                                        authority_lookup_seed.as_ref(),
                                        txn,
                                        fragment,
                                        context.resource_limits,
                                        context.editing_limits,
                                        context.max_length,
                                        view.schema_fingerprint,
                                        view.yrs_state_epoch,
                                        context.document_revision,
                                    ) {
                                        localized_format_compiler =
                                            LocalizedFormatCompiler::try_new(
                                                request_id,
                                                txn,
                                                fragment,
                                                context.schema,
                                                action_limit,
                                                context.resource_limits.max_input_bytes,
                                                charged_scan_work,
                                                locator,
                                                view.schema_fingerprint,
                                                view.yrs_state_epoch,
                                                context.document_revision,
                                            )?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let (Some(prepared), Some(view), [TypedOperation::ReplaceStructure(replacement)]) = (
        prepared_semantics,
        engine_view,
        transaction.operations.as_slice(),
    ) {
        if let Ok(authority_lookup_seed) = view.authority.lookup_seed(request_id) {
            if let Some(locator) = LocalizedRootWindowLocator::mint(
                request_id,
                context.document,
                prepared.expected_preview,
                replacement,
                authority_lookup_seed.as_ref(),
                txn,
                fragment,
                context.resource_limits,
                context.editing_limits,
                context.max_length,
                view.schema_fingerprint,
                view.yrs_state_epoch,
                context.document_revision,
            )? {
                localized_root_window_compiler = LocalizedRootWindowCompiler::try_new(
                    request_id,
                    txn,
                    fragment,
                    context.schema,
                    action_limit,
                    context.resource_limits.max_input_bytes,
                    charged_scan_work,
                    locator,
                )?;
            }
        }
    }
    let localized_semantic = if localized_compiler.is_some() {
        engine_view.and_then(|view| {
            let admission = localized_textblock_edit_admission.as_ref()?;
            let (_, edit) = view
                .authority
                .installed()
                .textblock_edit_for_transaction(&transaction)?;
            let document_position = edit.replaced.start;
            let validated = admission.validate_current_with_authority(
                view.authority.installed(),
                &transaction,
                document_position,
                txn,
                fragment,
                view.authority.lookup_seed(request_id).ok()?,
                view.authority.materialized_identity(),
                context.schema,
                context.resource_limits,
                context.editing_limits,
                context.max_length,
                view.yrs_state_epoch,
            )?;
            let localized = try_localized_semantic_compilation(context, &transaction, &validated)?;
            if let Some(prepared) = prepared_semantics {
                if localized.steps.back()?.preview != *prepared.expected_preview {
                    return None;
                }
            }
            Some(localized)
        })
    } else {
        None
    };
    let mutation_lowering = if let Some(localized) = localized_compiler {
        TransactionMutationLowering::LocalizedInsert(Box::new(localized))
    } else if let Some(localized) = localized_format_compiler {
        TransactionMutationLowering::LocalizedFormat(Box::new(localized))
    } else if let Some(localized) = localized_root_window_compiler {
        TransactionMutationLowering::LocalizedRootWindow(Box::new(localized))
    } else {
        #[cfg(test)]
        if localized_format_candidate {
            yrs_engine::mutation::record_range_format_eager_fallback_for_test();
        }
        #[cfg(test)]
        if localized_root_window_candidate {
            yrs_engine::mutation::record_root_window_eager_fallback_for_test();
        }
        TransactionMutationLowering::Eager(Box::new(MutationCompiler::new(
            request_id,
            txn,
            fragment,
            context.schema,
            action_limit,
            context.resource_limits.max_input_bytes,
            charged_scan_work,
        )?))
    };
    let mut load_crdt_envelope = |mutation_scan_work: usize| {
        let envelope = crdt_envelope(
            request_id,
            txn,
            context.resource_limits.max_encoded_state_bytes,
        )?;
        let reconciled = mutation_scan_work
            .checked_add(envelope.scan_work)
            .ok_or_else(|| {
                input_limit_error(
                    request_id,
                    None,
                    context.resource_limits.max_input_bytes,
                    usize::MAX,
                )
            })?;
        if reconciled > context.resource_limits.max_input_bytes {
            return Err(input_limit_error(
                request_id,
                None,
                context.resource_limits.max_input_bytes,
                reconciled,
            ));
        }
        Ok(envelope)
    };
    let mut compiled = compile_transaction_impl(
        context,
        &transaction,
        Some(mutation_lowering),
        Some(&mut load_crdt_envelope),
        stored_marks,
        cached_view,
        SemanticCompilationShortcuts {
            prepared: prepared_semantics,
            localized: localized_semantic,
        },
    )?;
    compiled.localized_textblock_edit_admission = localized_textblock_edit_admission;
    compiled.relative_selection_plan =
        match (&compiled.selection_plan, &transaction.selection_intent) {
            (SelectionPlan::Preserve, _) => RelativeSelectionPlan::Preserve,
            (SelectionPlan::Mapped(selection), _) => {
                RelativeSelectionPlan::PreserveWithFallback(selection.clone())
            }
            (SelectionPlan::Explicit(selection), SelectionIntent::Set(_)) => {
                RelativeSelectionPlan::Precomputed {
                    relative: planned_relative_selection(
                        context,
                        &transaction,
                        txn,
                        fragment,
                        cached_view,
                    )?
                    .ok_or_else(|| {
                        OperationError::engine_invariant_failed(
                            request_id,
                            None,
                            "explicit Set selection has no relative plan",
                        )
                    })?,
                    fallback: selection.clone(),
                }
            }
            (SelectionPlan::Explicit(_), SelectionIntent::UseOperationResult) => {
                RelativeSelectionPlan::OperationResult
            }
            (SelectionPlan::Explicit(_), SelectionIntent::Preserve) => {
                return Err(OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "Preserve selection unexpectedly compiled as explicit",
                ));
            }
        };
    // The server owns this read view through compilation and preflight. The
    // plan's document guard was captured only after the CRDT clock scan and
    // its input-work reservation above admitted full snapshot construction.
    // Preflight checks that sealed snapshot before any eager Yrs target reads.
    #[cfg(test)]
    check_atomic_failpoint(request_id, AtomicFailpoint::MutationPreflight)?;
    preflight_mutation_plan(request_id, &compiled.mutation_plan, txn)?;
    if compiled.localized_semantic_used {
        compiled.prepared_derived_evidence = engine_view.and_then(|view| {
            let admission = compiled.localized_textblock_edit_admission.as_ref()?;
            let (_, edit) = view
                .authority
                .installed()
                .textblock_edit_for_transaction(&transaction)?;
            let document_position = edit.replaced.start;
            let validated = admission.validate_current_with_authority(
                view.authority.installed(),
                &transaction,
                document_position,
                txn,
                fragment,
                view.authority.lookup_seed(request_id).ok()?,
                view.authority.materialized_identity(),
                context.schema,
                context.resource_limits,
                context.editing_limits,
                context.max_length,
                view.yrs_state_epoch,
            )?;
            validated.prepare_derived_evidence(
                &compiled.preview,
                compiled.canonical_artifact.as_ref()?,
                compiled.preview_derivations.as_ref()?,
                context.schema,
            )
        });
    }
    Ok(compiled)
}

pub(super) fn validate_cached_compilation_view<'a>(
    request_id: u64,
    context: CompilationContext<'_>,
    engine_view: EngineCompilationView<'a>,
) -> OperationResult<CachedCompilationView<'a>> {
    let cached = engine_view.cached;
    let selection_matches = context
        .selection
        .is_some_and(|selection| std::ptr::eq(selection, cached.selection));
    if !std::ptr::eq(context.document, cached.document)
        || !selection_matches
        || context.document_revision != cached.document_revision
        || engine_view.state_revision != cached.state_revision
        || engine_view.schema_fingerprint != cached.schema_fingerprint
        || cached.rendered_scalars != cached.position_map.total_scalars()
        || cached.document_node_count == 0
    {
        return Err(OperationError::engine_invariant_failed(
            request_id,
            None,
            "cached semantic compilation view does not match the live engine state",
        ));
    }
    Ok(cached)
}

#[cfg(test)]
mod snapshot_tests {
    use super::CompilationReadView;
    use std::cell::{Cell, OnceCell};
    use yrs::{Doc, ReadTxn, Text, Transact};

    struct CountingReadView<'a, T> {
        txn: &'a T,
        scans: Cell<usize>,
    }

    impl<T: ReadTxn> ReadTxn for CountingReadView<'_, T> {
        fn store(&self) -> &yrs::Store {
            self.txn.store()
        }
        fn snapshot(&self) -> yrs::Snapshot {
            self.scans.set(self.scans.get() + 1);
            self.txn.snapshot()
        }
    }

    #[test]
    fn compilation_snapshot_is_lazy_exact_and_limited_to_one_read_view() {
        let doc = Doc::new();
        let text = doc.get_or_insert_text("content");
        text.insert(&mut doc.transact_mut(), 0, "abcdef");
        let before = {
            let txn = doc.transact();
            let underlying = CountingReadView {
                txn: &txn,
                scans: Cell::new(0),
            };
            let view = CompilationReadView {
                txn: &underlying,
                snapshot: OnceCell::new(),
            };
            assert_eq!(
                underlying.scans.get(),
                0,
                "creating the view must not scan before admission"
            );
            let expected = txn.snapshot();
            for _ in 0..3 {
                assert_eq!(
                    view.snapshot(),
                    expected,
                    "both state and delete clocks must match the underlying snapshot"
                );
            }
            assert_eq!(
                underlying.scans.get(),
                1,
                "repeated reads must traverse the underlying store once"
            );
            expected
        };
        text.remove_range(&mut doc.transact_mut(), 1, 2);
        let txn = doc.transact();
        let underlying = CountingReadView {
            txn: &txn,
            scans: Cell::new(0),
        };
        let view = CompilationReadView {
            txn: &underlying,
            snapshot: OnceCell::new(),
        };
        let after = view.snapshot();
        assert_eq!(after, txn.snapshot());
        assert_eq!(
            before.state_map, after.state_map,
            "deletion must exercise unchanged state clocks"
        );
        assert_ne!(
            before.delete_set, after.delete_set,
            "a later read view must observe deletion-only changes"
        );
        assert_eq!(underlying.scans.get(), 1);
    }
}
