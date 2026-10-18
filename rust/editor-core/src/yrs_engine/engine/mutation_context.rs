use super::YrsDocumentEngine;
use crate::model::Document;
use crate::yrs_engine;
use crate::yrs_engine::compiler::MutationLookupTransition;
use std::sync::Arc;
use yrs::types::xml::XmlFragmentRef;
use yrs::{ReadTxn, Transact};

impl YrsDocumentEngine {
    pub(super) fn prepare_mutation_lookup_seed(
        &self,
        request_id: u64,
    ) -> yrs_engine::OperationResult<yrs_engine::prepared_admission::PreparedMutationContext> {
        let state = self
            .derived_state
            .as_ref()
            .ok_or_else(|| yrs_engine::OperationError::engine_not_ready(request_id))?;
        if state.document_revision != self.revision
            || state.state_revision != self.state_revision
            || state.schema_fingerprint != self.schema_fingerprint
            || state.canonical_artifact.schema_fingerprint() != self.schema_fingerprint
        {
            return Err(yrs_engine::OperationError::engine_invariant_failed(
                request_id,
                None,
                "installed derived state does not match the live engine context",
            ));
        }
        let txn = self.doc.transact();
        let fragment = txn
            .get_xml_fragment(self.fragment_name.as_str())
            .ok_or_else(|| {
                yrs_engine::OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "ready engine lost its mutation lookup fragment",
                )
            })?;
        let installed_seed_matches = state
            .mutation_lookup_seed
            .matches_canonical_artifact(&state.canonical_artifact)
            && state.mutation_lookup_seed.matches(
                &txn,
                &fragment,
                &state.document,
                &self.resource_limits,
                &self.editing_limits,
                self.max_length,
                &self.schema_fingerprint,
                self.yrs_state_epoch,
                self.revision,
            );
        let lookup_seed = if installed_seed_matches {
            Arc::clone(&state.mutation_lookup_seed)
        } else {
            let target_capacity_hint = state
                .localized_text_index
                .as_ref()
                .map_or(0, |index| index.leaf_count());
            let hydrated = state
                .mutation_lookup_seed
                .hydrate_with_target_capacity_hint(
                    request_id,
                    &txn,
                    &fragment,
                    &self.schema,
                    &state.document,
                    &self.resource_limits,
                    &self.editing_limits,
                    self.max_length,
                    &self.schema_fingerprint,
                    self.yrs_state_epoch,
                    self.revision,
                    target_capacity_hint,
                )?
                .with_canonical_artifact(&state.canonical_artifact)
                .try_publish_hydrated(request_id)?;
            #[cfg(test)]
            yrs_engine::observability::record_staged_seed_preparation();
            hydrated
        };
        // `authority` below performs the one exact live-store validation of
        // whichever seed source won. Avoid repeating the same binding walk
        // here; no prepared context escapes unless that authority check passes.
        let context = yrs_engine::prepared_admission::PreparedMutationContext::new(
            request_id,
            state.document.clone(),
            state.canonical_artifact.clone(),
            self.revision,
            self.state_revision,
            self.yrs_state_epoch,
            self.schema_fingerprint.clone().into_boxed_str(),
            self.fragment_name.clone().into_boxed_str(),
            self.resource_limits.clone(),
            self.editing_limits.clone(),
            self.max_length,
            lookup_seed,
        );
        {
            context.authority(
                yrs_engine::prepared_admission::LiveMutationAuthorityContext {
                    request_id,
                    installed: state,
                    txn: &txn,
                    fragment: &fragment,
                    fragment_name: &self.fragment_name,
                    schema_fingerprint: &self.schema_fingerprint,
                    resource_limits: &self.resource_limits,
                    editing_limits: &self.editing_limits,
                    max_length: self.max_length,
                    document_revision: self.revision,
                    state_revision: self.state_revision,
                    yrs_state_epoch: self.yrs_state_epoch,
                },
            )?;
        }
        Ok(context)
    }

    pub(super) fn prepare_mutation_identity(
        &self,
        context: &mut yrs_engine::prepared_admission::PreparedMutationContext,
    ) -> yrs_engine::OperationResult<()> {
        if context.materialized_identity().is_some() {
            return Ok(());
        }
        let state = self
            .derived_state
            .as_ref()
            .ok_or_else(|| yrs_engine::OperationError::engine_not_ready(context.request_id()))?;
        let txn = self.doc.transact();
        let fragment = txn
            .get_xml_fragment(self.fragment_name.as_str())
            .ok_or_else(|| {
                yrs_engine::OperationError::engine_invariant_failed(
                    context.request_id(),
                    None,
                    "ready engine lost its mutation lookup fragment",
                )
            })?;
        {
            context.authority(
                yrs_engine::prepared_admission::LiveMutationAuthorityContext {
                    request_id: context.request_id(),
                    installed: state,
                    txn: &txn,
                    fragment: &fragment,
                    fragment_name: &self.fragment_name,
                    schema_fingerprint: &self.schema_fingerprint,
                    resource_limits: &self.resource_limits,
                    editing_limits: &self.editing_limits,
                    max_length: self.max_length,
                    document_revision: self.revision,
                    state_revision: self.state_revision,
                    yrs_state_epoch: self.yrs_state_epoch,
                },
            )?;
        }
        let canonical_fingerprint = context.canonical_artifact().sha256();
        let canonical_serialized_len = context.canonical_artifact().serialized_len();
        if !state.matches_materialized_mutation_identity(
            context.canonical_artifact(),
            canonical_fingerprint,
            canonical_serialized_len,
            &self.resource_limits,
            &self.schema_fingerprint,
            self.revision,
            self.state_revision,
            self.yrs_state_epoch,
        ) {
            // Identity is optional cached evidence. Runtime limit changes can
            // legitimately make the installed validation certificate
            // ineligible for reuse; leave identity absent so compilation uses
            // its full validation path and preserves established error order.
            return Ok(());
        }
        context.set_materialized_identity(
            yrs_engine::prepared_admission::MaterializedMutationIdentity {
                canonical_fingerprint,
                canonical_serialized_len,
            },
        );
        #[cfg(test)]
        yrs_engine::observability::record_staged_identity_materialization();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn root_window_lookup_splice<C: ReadTxn, N: ReadTxn>(
        &self,
        request_id: u64,
        current_txn: &C,
        current_fragment: &XmlFragmentRef,
        next_txn: &N,
        next_fragment: &XmlFragmentRef,
        preview: &Document,
        derivations: Option<&yrs_engine::compiler::CompiledDocumentDerivations>,
        next_yrs_state_epoch: u64,
        next_document_revision: u64,
    ) -> yrs_engine::OperationResult<
        Option<(
            yrs_engine::mutation::MutationLookupSeed,
            Option<Option<yrs_engine::block_branch_index::BlockBranchIndex>>,
        )>,
    > {
        let Some(state) = self.derived_state.as_ref() else {
            return Ok(None);
        };
        let (Some(derivations), Some(index)) = (derivations, state.block_branch_index.as_ref())
        else {
            return Ok(None);
        };
        if state.mutation_lookup_seed.is_unavailable() {
            return Ok(None);
        }
        let Some(window) = yrs_engine::derived_state::RootChildWindow::between(
            state.document.root(),
            preview.root(),
        ) else {
            return Ok(None);
        };
        let (Ok(from), Ok(old_to), Ok(new_to)) = (
            u32::try_from(window.from()),
            u32::try_from(window.old_to()),
            u32::try_from(window.new_to()),
        ) else {
            return Ok(None);
        };
        let old_blocks = state
            .position_map
            .block_range_for_root_children(from..old_to);
        let new_blocks = derivations
            .position_map
            .block_range_for_root_children(from..new_to);
        let start_position = preview.root().content().and_then(|children| {
            children
                .children()
                .get(..window.from())?
                .iter()
                .try_fold(0u32, |position, child| {
                    position.checked_add(child.node_size())
                })
        });
        let Some(start_position) = start_position.filter(|_| old_blocks.start == new_blocks.start)
        else {
            return Ok(None);
        };
        let mut capture = Some(
            yrs_engine::block_branch_index::BlockBranchIndexCapture::for_root_window(
                &derivations.position_map,
                new_blocks.clone(),
                from,
                start_position,
            ),
        );
        let seed = state.mutation_lookup_seed.with_root_window_replaced(
            request_id,
            current_txn,
            current_fragment,
            next_txn,
            next_fragment,
            &window,
            &self.schema,
            &mut capture,
            preview,
            &self.resource_limits,
            &self.editing_limits,
            self.max_length,
            &self.schema_fingerprint,
            next_yrs_state_epoch,
            next_document_revision,
        )?;
        let branch_index = capture
            .and_then(|capture| capture.finish_root_window(new_blocks))
            .and_then(|branches| index.with_root_window_replaced(old_blocks, &window, branches))
            .map(Some);
        Ok(Some((seed, branch_index)))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_mutation_lookup_transition_with_authority<T: ReadTxn>(
        &self,
        request_id: u64,
        authority: &dyn yrs_engine::prepared_admission::DerivedStateAuthority,
        transition: &MutationLookupTransition,
        txn: &T,
        fragment: &XmlFragmentRef,
        preview: &Document,
        canonical_artifact: &yrs_engine::canonical::CanonicalArtifact,
        next_yrs_state_epoch: u64,
        next_document_revision: u64,
    ) -> yrs_engine::OperationResult<Arc<yrs_engine::mutation::MutationLookupSeed>> {
        let current = authority.installed();
        let seed = authority.lookup_seed(request_id)?;
        let prepared = match transition {
            MutationLookupTransition::Promote(promotion) => seed.prepare_promotion(
                txn,
                fragment,
                promotion,
                &current.document,
                preview,
                &self.resource_limits,
                &self.editing_limits,
                self.max_length,
                &self.schema_fingerprint,
                self.yrs_state_epoch,
                self.revision,
                next_yrs_state_epoch,
                next_document_revision,
            )?,
            MutationLookupTransition::Invalidate {
                request_id: transition_request_id,
            } => {
                if *transition_request_id != request_id {
                    return Err(yrs_engine::OperationError::engine_invariant_failed(
                        request_id,
                        None,
                        "localized mutation lookup invalidation request is stale",
                    ));
                }
                seed.prepare_unavailable_transition(
                    request_id,
                    txn,
                    fragment,
                    &current.document,
                    preview,
                    &self.resource_limits,
                    &self.editing_limits,
                    self.max_length,
                    &self.schema_fingerprint,
                    self.yrs_state_epoch,
                    self.revision,
                    next_yrs_state_epoch,
                    next_document_revision,
                )?
            }
        };
        Ok(Arc::new(
            prepared.with_canonical_artifact(canonical_artifact),
        ))
    }

    #[cfg(test)]
    pub(super) fn finalize_deferred_for_test(
        &self,
        deferred: yrs_engine::prepared_admission::DeferredCommandAdmission,
        context: &yrs_engine::prepared_admission::PreparedMutationContext,
        transaction: &yrs_engine::TypedTransaction,
        expected_preview: &crate::model::Document,
    ) -> yrs_engine::OperationResult<yrs_engine::compiler::PreparedSemanticAdmission> {
        let state = self
            .derived_state
            .as_ref()
            .ok_or_else(|| yrs_engine::OperationError::engine_not_ready(transaction.request_id))?;
        let txn = self.doc.transact();
        let fragment = txn
            .get_xml_fragment(self.fragment_name.as_str())
            .ok_or_else(|| {
                yrs_engine::OperationError::engine_invariant_failed(
                    transaction.request_id,
                    None,
                    "ready engine lost its deferred-finalization fragment",
                )
            })?;
        let staged = context.authority(
            yrs_engine::prepared_admission::LiveMutationAuthorityContext {
                request_id: transaction.request_id,
                installed: state,
                txn: &txn,
                fragment: &fragment,
                fragment_name: &self.fragment_name,
                schema_fingerprint: &self.schema_fingerprint,
                resource_limits: &self.resource_limits,
                editing_limits: &self.editing_limits,
                max_length: self.max_length,
                document_revision: self.revision,
                state_revision: self.state_revision,
                yrs_state_epoch: self.yrs_state_epoch,
            },
        )?;
        yrs_engine::compiler::finalize_deferred_admission(
            &staged,
            deferred,
            yrs_engine::compiler::PreparedSemanticLiveContext {
                transaction,
                expected_preview,
                canonical_schema: &self.canonical_schema,
            },
        )
    }

    #[cfg(test)]
    pub(super) fn ensure_mutation_lookup_seed(
        &mut self,
        request_id: u64,
    ) -> yrs_engine::OperationResult<()> {
        let context = self.prepare_mutation_lookup_seed(request_id)?;
        let prepared = Arc::clone(context.lookup_seed());
        let state = self.derived_state.as_mut().ok_or_else(|| {
            yrs_engine::OperationError::engine_invariant_failed(
                request_id,
                None,
                "ready engine lost derived state during lookup hydration",
            )
        })?;
        if Arc::ptr_eq(&state.mutation_lookup_seed, &prepared) {
            return Ok(());
        }
        state.mutation_lookup_seed = prepared;
        #[cfg(test)]
        yrs_engine::observability::record_installed_base_seed_publication();
        Ok(())
    }
}
