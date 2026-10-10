use super::active_state::ActiveStateStructuralSeal;
use super::localized_index::{canonical_marks_sha256, LocalizedTextLeafCertificate};
#[cfg(test)]
use super::observability::{
    FORCE_LOCALIZED_INDEX_BUDGET, LOCALIZED_INDEX_PROMOTION_ATTEMPTS,
    LOCALIZED_INDEX_PROMOTION_DROPS, LOCALIZED_INDEX_PROMOTION_SUCCESSES,
};
use super::render_evidence::{
    LocalizedRenderOperationKind, LocalizedRenderTransitionProof, PreparedDerivedEvidence,
};
use super::validation::DocumentValidationCertificate;
use super::DerivedStateCache;
use crate::boundary::ResourceLimits;
use crate::model::Document;
use crate::schema::Schema;
use crate::yrs_engine;
use crate::yrs_engine::canonical::CanonicalArtifact;
use crate::yrs_engine::compiler::CompiledDocumentDerivations;
use crate::yrs_engine::prepared_admission::DerivedStateAuthority;
use crate::yrs_engine::{RelativeSelection, ResolvedSelection};
use sha2::Digest;
use std::sync::Arc;
use yrs::types::xml::XmlFragmentRef;
use yrs::ReadTxn;

#[derive(Debug, Clone)]
pub(crate) struct LocalizedTextblockEdit<'edit> {
    pub(crate) block_path: &'edit [u32],
    pub(crate) replaced: std::ops::Range<u32>,
    pub(crate) text: &'edit str,
    pub(crate) marks: &'edit [crate::model::Mark],
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct LocalizedTextblockEditPlan {
    pub(super) leaf: LocalizedTextLeafCertificate,
    pub(super) creates_leaf: bool,
    pub(super) removed_scalars: u32,
    pub(super) range_end: u32,
    pub(super) block_path_len: usize,
    pub(super) block_path_sha256: [u8; 32],
    pub(super) affected_top_level_index: usize,
    pub(super) inserted_scalars: u32,
    pub(super) inserted_utf8_bytes: usize,
    pub(super) inserted_utf16: u32,
    pub(super) canonical_growth_bytes: isize,
    pub(super) rendered_scalar_delta: i32,
    pub(super) rendered_utf16_delta: i32,
    pub(super) next_raw_text_scalars: u64,
    pub(super) next_raw_text_utf8_bytes: usize,
    pub(super) next_canonical_serialized_len: usize,
    pub(super) next_rendered_scalars: u32,
    pub(super) operation_result: ResolvedSelection,
    pub(super) history_undo_units: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalizedTextblockEditAdmission {
    pub(super) plan: LocalizedTextblockEditPlan,
    pub(super) document_revision: u64,
    pub(super) state_revision: u64,
    pub(super) yrs_state_epoch: u64,
    pub(super) selection: ResolvedSelection,
    pub(super) relative_selection: RelativeSelection,
    pub(super) stored_marks_sha256: Option<[u8; 32]>,
    pub(super) canonical_fingerprint: [u8; 32],
    pub(super) validation_certificate: DocumentValidationCertificate,
    pub(super) request_id: u64,
    pub(super) base_document_revision: u64,
    pub(super) origin: yrs_engine::TransactionOrigin,
    pub(super) inserted_at: yrs_engine::RevisionedPosition,
    pub(super) inserted_document_position: u32,
    pub(super) inserted_text_sha256: [u8; 32],
    pub(super) inserted_marks_sha256: [u8; 32],
    pub(super) selection_intent: yrs_engine::SelectionIntent,
    pub(super) history_policy: yrs_engine::HistoryPolicy,
    pub(super) max_length: Option<u32>,
    pub(super) max_operations_per_transaction: usize,
    pub(super) max_undo_groups: usize,
    pub(super) max_derived_output_bytes: usize,
    pub(super) max_undo_retained_units: u64,
    pub(super) render_seal: Arc<crate::render::incremental::CachedRenderBlocks>,
    pub(super) lookup_seal: Arc<yrs_engine::mutation::MutationLookupSeed>,
}

pub(super) struct LocalizedTextblockEditAdmissionRequest<'a> {
    pub(super) request_id: u64,
    pub(super) base_document_revision: u64,
    pub(super) origin: yrs_engine::TransactionOrigin,
    pub(super) inserted_at: yrs_engine::RevisionedPosition,
    pub(super) edit: LocalizedTextblockEdit<'a>,
    pub(super) selection_intent: yrs_engine::SelectionIntent,
    pub(super) history_policy: yrs_engine::HistoryPolicy,
}

#[allow(dead_code)]
impl LocalizedTextblockEditAdmission {
    pub(crate) fn block_index(&self) -> usize {
        self.plan.leaf.block_index
    }

    pub(crate) fn lookup_seal_matches(
        &self,
        seed: &Arc<yrs_engine::mutation::MutationLookupSeed>,
    ) -> bool {
        Arc::ptr_eq(&self.lookup_seal, seed)
    }

    pub(crate) fn same_prewrite_selection_claims(&self, other: &Self) -> bool {
        self.plan == other.plan
            && self.document_revision == other.document_revision
            && self.state_revision == other.state_revision
            && self.yrs_state_epoch == other.yrs_state_epoch
            && self.selection == other.selection
            && self.relative_selection == other.relative_selection
            && self.stored_marks_sha256 == other.stored_marks_sha256
            && self.canonical_fingerprint == other.canonical_fingerprint
            && self.validation_certificate == other.validation_certificate
            && self.request_id == other.request_id
            && self.base_document_revision == other.base_document_revision
            && self.origin == other.origin
            && self.inserted_at == other.inserted_at
            && self.inserted_document_position == other.inserted_document_position
            && self.inserted_text_sha256 == other.inserted_text_sha256
            && self.inserted_marks_sha256 == other.inserted_marks_sha256
            && self.selection_intent == other.selection_intent
            && self.history_policy == other.history_policy
            && self.max_length == other.max_length
            && self.max_operations_per_transaction == other.max_operations_per_transaction
            && self.max_undo_groups == other.max_undo_groups
            && self.max_derived_output_bytes == other.max_derived_output_bytes
            && self.max_undo_retained_units == other.max_undo_retained_units
            && Arc::ptr_eq(&self.render_seal, &other.render_seal)
            && Arc::ptr_eq(&self.lookup_seal, &other.lookup_seal)
    }

    pub(crate) fn active_state_structural_seal(&self) -> ActiveStateStructuralSeal {
        ActiveStateStructuralSeal {
            block_index: self.plan.leaf.block_index,
            child_ordinal: self.plan.leaf.child_ordinal,
            leaf_doc_start: self.plan.leaf.doc_start,
            leaf_marks_sha256: self.plan.leaf.marks_sha256,
            block_path_len: self.plan.block_path_len,
            block_path_sha256: self.plan.block_path_sha256,
            affected_top_level_index: self.plan.affected_top_level_index,
        }
    }

    pub(crate) fn inserted_document_position(&self) -> u32 {
        self.inserted_document_position
    }

    pub(crate) fn inserted_scalars(&self) -> u32 {
        self.plan.inserted_scalars
    }

    pub(crate) fn inserted_utf16(&self) -> u32 {
        self.plan.inserted_utf16
    }

    pub(crate) fn operation_result_selection(&self) -> &ResolvedSelection {
        &self.plan.operation_result
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_current<'a, T: ReadTxn>(
        &'a self,
        state: &'a DerivedStateCache,
        transaction: &yrs_engine::TypedTransaction,
        document_position: u32,
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
        resource_limits: &ResourceLimits,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
        yrs_state_epoch: u64,
    ) -> Option<ValidatedLocalizedTextblockEditAdmission<'a>> {
        let authority = yrs_engine::prepared_admission::InstalledDerivedStateAuthority::new(state);
        let lookup_seed =
            DerivedStateAuthority::lookup_seed(&authority, transaction.request_id).ok()?;
        self.validate_current_with_authority(
            state,
            transaction,
            document_position,
            txn,
            fragment,
            lookup_seed,
            authority.materialized_identity(),
            schema,
            resource_limits,
            editing_limits,
            max_length,
            yrs_state_epoch,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_current_with_authority<'a, T: ReadTxn>(
        &'a self,
        state: &'a DerivedStateCache,
        transaction: &yrs_engine::TypedTransaction,
        document_position: u32,
        txn: &T,
        fragment: &XmlFragmentRef,
        lookup_seed: &Arc<yrs_engine::mutation::MutationLookupSeed>,
        identity: Option<&yrs_engine::prepared_admission::MaterializedMutationIdentity>,
        schema: &Schema,
        resource_limits: &ResourceLimits,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
        yrs_state_epoch: u64,
    ) -> Option<ValidatedLocalizedTextblockEditAdmission<'a>> {
        let (at, edit) = state.textblock_edit_for_transaction(transaction)?;
        if edit.replaced.start != document_position {
            return None;
        }
        let text = edit.text;
        let marks = edit.marks;
        let canonical_serialized_len = identity.map_or(
            state.validation_certificate.canonical_serialized_len,
            |identity| identity.canonical_serialized_len,
        );
        let canonical_fingerprint = identity.map_or_else(
            || state.validation_certificate.canonical_fingerprint,
            |identity| identity.canonical_fingerprint,
        );
        let plan = state.plan_localized_textblock_edit(
            &edit,
            schema,
            editing_limits,
            max_length,
            canonical_serialized_len,
        )?;
        let stored_marks_sha256 = match state.stored_marks.as_deref() {
            Some(stored_marks) => Some(canonical_marks_sha256(stored_marks)?),
            None => None,
        };
        let claims_match = self.plan == plan
            && self.request_id == transaction.request_id
            && self.base_document_revision == transaction.base_document_revision
            && self.origin == transaction.origin
            && self.inserted_at == at
            && self.inserted_document_position == document_position
            && self.inserted_text_sha256 == <[u8; 32]>::from(sha2::Sha256::digest(text.as_bytes()))
            && self.inserted_marks_sha256 == canonical_marks_sha256(marks)?
            && self.selection_intent == transaction.selection_intent
            && self.history_policy == transaction.history_policy
            && self.max_length == max_length
            && self.max_operations_per_transaction == editing_limits.max_operations_per_transaction
            && self.max_undo_groups == editing_limits.max_undo_groups
            && self.max_derived_output_bytes == editing_limits.max_derived_output_bytes
            && self.max_undo_retained_units == editing_limits.max_undo_retained_units
            && self.validation_certificate == state.validation_certificate
            && identity.is_none_or(|identity| {
                state.matches_materialized_mutation_identity(
                    &state.canonical_artifact,
                    identity.canonical_fingerprint,
                    identity.canonical_serialized_len,
                    resource_limits,
                    &state.schema_fingerprint,
                    state.document_revision,
                    state.state_revision,
                    yrs_state_epoch,
                )
            })
            && (identity.is_some()
                || self.validation_certificate.matches(
                    &state.canonical_artifact,
                    resource_limits,
                    &state.schema_fingerprint,
                    state.document_revision,
                    state.state_revision,
                    yrs_state_epoch,
                ))
            && self.selection == state.resolved_selection
            && self.relative_selection == state.relative_selection
            && self.stored_marks_sha256 == stored_marks_sha256
            && self.document_revision == state.document_revision
            && self.state_revision == state.state_revision
            && self.yrs_state_epoch == yrs_state_epoch
            && self.canonical_fingerprint == canonical_fingerprint
            && Arc::ptr_eq(&self.render_seal, &state.render_blocks)
            && self
                .render_seal
                .matches_identity(&state.document, &state.schema_fingerprint)
            && Arc::ptr_eq(&self.lookup_seal, lookup_seed)
            && self.lookup_seal.matches(
                txn,
                fragment,
                &state.document,
                resource_limits,
                editing_limits,
                max_length,
                &state.schema_fingerprint,
                yrs_state_epoch,
                state.document_revision,
            );
        claims_match.then_some(ValidatedLocalizedTextblockEditAdmission {
            admission: self,
            state,
        })
    }

    #[cfg(test)]
    pub(crate) fn tampered_claims_for_test(&self) -> Vec<(&'static str, Self)> {
        let mut cases = Vec::new();
        macro_rules! tamper {
            ($name:literal, $body:expr) => {{
                let mut proof = self.clone();
                $body(&mut proof);
                cases.push(($name, proof));
            }};
        }
        tamper!("leaf.docStart", |proof: &mut Self| proof
            .plan
            .leaf
            .doc_start =
            proof.plan.leaf.doc_start.saturating_add(1));
        tamper!("leaf.textDigest", |proof: &mut Self| proof
            .plan
            .leaf
            .text_sha256[0] ^=
            1);
        tamper!("leaf.markDigest", |proof: &mut Self| proof
            .plan
            .leaf
            .marks_sha256[0] ^=
            1);
        tamper!("createsLeaf", |proof: &mut Self| proof.plan.creates_leaf =
            !proof.plan.creates_leaf);
        tamper!("blockPathLength", |proof: &mut Self| proof
            .plan
            .block_path_len =
            proof.plan.block_path_len.saturating_add(1));
        tamper!("blockPathDigest", |proof: &mut Self| proof
            .plan
            .block_path_sha256[0] ^=
            1);
        tamper!("topLevelIndex", |proof: &mut Self| proof
            .plan
            .affected_top_level_index =
            proof.plan.affected_top_level_index.saturating_add(1));
        tamper!("insertedScalars", |proof: &mut Self| proof
            .plan
            .inserted_scalars =
            proof.plan.inserted_scalars.saturating_add(1));
        tamper!("insertedUtf8", |proof: &mut Self| proof
            .plan
            .inserted_utf8_bytes =
            proof.plan.inserted_utf8_bytes.saturating_add(1));
        tamper!("insertedUtf16", |proof: &mut Self| proof
            .plan
            .inserted_utf16 =
            proof.plan.inserted_utf16.saturating_add(1));
        tamper!("canonicalGrowth", |proof: &mut Self| proof
            .plan
            .canonical_growth_bytes =
            proof.plan.canonical_growth_bytes.saturating_add(1));
        tamper!("renderedScalarDelta", |proof: &mut Self| proof
            .plan
            .rendered_scalar_delta =
            proof.plan.rendered_scalar_delta.saturating_add(1));
        tamper!("renderedUtf16Delta", |proof: &mut Self| proof
            .plan
            .rendered_utf16_delta =
            proof.plan.rendered_utf16_delta.saturating_add(1));
        tamper!("nextRawScalars", |proof: &mut Self| proof
            .plan
            .next_raw_text_scalars =
            proof.plan.next_raw_text_scalars.saturating_add(1));
        tamper!("nextRawUtf8", |proof: &mut Self| proof
            .plan
            .next_raw_text_utf8_bytes =
            proof.plan.next_raw_text_utf8_bytes.saturating_add(1));
        tamper!("nextCanonical", |proof: &mut Self| proof
            .plan
            .next_canonical_serialized_len =
            proof.plan.next_canonical_serialized_len.saturating_add(1));
        tamper!("nextRendered", |proof: &mut Self| proof
            .plan
            .next_rendered_scalars =
            proof.plan.next_rendered_scalars.saturating_add(1));
        tamper!("operationResult", |proof: &mut Self| proof
            .plan
            .operation_result =
            ResolvedSelection::All);
        tamper!("historyUnits", |proof: &mut Self| proof
            .plan
            .history_undo_units =
            proof.plan.history_undo_units.saturating_add(1));
        tamper!("documentRevision", |proof: &mut Self| proof
            .document_revision =
            proof.document_revision.saturating_add(1));
        tamper!("stateRevision", |proof: &mut Self| proof.state_revision =
            proof.state_revision.saturating_add(1));
        tamper!("epoch", |proof: &mut Self| proof.yrs_state_epoch =
            proof.yrs_state_epoch.saturating_add(1));
        tamper!("selection", |proof: &mut Self| proof.selection =
            ResolvedSelection::All);
        tamper!("relativeSelection", |proof: &mut Self| proof
            .relative_selection =
            RelativeSelection::All);
        tamper!("storedMarks", |proof: &mut Self| proof
            .stored_marks_sha256 =
            if proof.stored_marks_sha256.is_some() {
                None
            } else {
                Some([0; 32])
            });
        tamper!("canonicalFingerprint", |proof: &mut Self| proof
            .canonical_fingerprint[0] ^=
            1);
        tamper!("validationCertificate", |proof: &mut Self| proof
            .validation_certificate
            .stats
            .node_count =
            proof
                .validation_certificate
                .stats
                .node_count
                .saturating_add(1));
        tamper!("requestId", |proof: &mut Self| proof.request_id =
            proof.request_id.saturating_add(1));
        tamper!("baseDocumentRevision", |proof: &mut Self| proof
            .base_document_revision =
            proof.base_document_revision.saturating_add(1));
        tamper!("origin", |proof: &mut Self| proof.origin =
            yrs_engine::TransactionOrigin::RemoteSync);
        tamper!("insertedAt", |proof: &mut Self| proof.inserted_at.offset =
            proof.inserted_at.offset.saturating_add(1));
        tamper!("documentPosition", |proof: &mut Self| proof
            .inserted_document_position =
            proof.inserted_document_position.saturating_add(1));
        tamper!("textDigest", |proof: &mut Self| proof
            .inserted_text_sha256[0] ^=
            1);
        tamper!("marks", |proof: &mut Self| proof.inserted_marks_sha256
            [0] ^= 1);
        tamper!("selectionIntent", |proof: &mut Self| proof
            .selection_intent =
            yrs_engine::SelectionIntent::Preserve);
        tamper!("historyPolicy", |proof: &mut Self| proof.history_policy =
            yrs_engine::HistoryPolicy::Skip);
        tamper!("maxLength", |proof: &mut Self| proof.max_length =
            Some(proof.max_length.unwrap_or(u32::MAX).saturating_sub(1)));
        tamper!("maxOperations", |proof: &mut Self| proof
            .max_operations_per_transaction =
            proof.max_operations_per_transaction.saturating_add(1));
        tamper!("maxUndoGroups", |proof: &mut Self| proof.max_undo_groups =
            proof.max_undo_groups.saturating_add(1));
        tamper!("maxDerivedOutput", |proof: &mut Self| proof
            .max_derived_output_bytes =
            proof.max_derived_output_bytes.saturating_add(1));
        tamper!("maxUndo", |proof: &mut Self| proof
            .max_undo_retained_units =
            proof.max_undo_retained_units.saturating_add(1));
        tamper!("renderSeal", |proof: &mut Self| proof.render_seal =
            Arc::new((*proof.render_seal).clone()));
        tamper!("lookupSeal", |proof: &mut Self| proof.lookup_seal =
            Arc::new((*proof.lookup_seal).clone()));
        cases
    }
}

pub(crate) struct ValidatedLocalizedTextblockEditAdmission<'a> {
    pub(super) admission: &'a LocalizedTextblockEditAdmission,
    pub(super) state: &'a DerivedStateCache,
}

impl ValidatedLocalizedTextblockEditAdmission<'_> {
    pub(crate) fn prepare_derived_evidence(
        &self,
        preview: &Document,
        canonical_artifact: &CanonicalArtifact,
        derivations: &CompiledDocumentDerivations,
        schema: &Schema,
    ) -> Option<PreparedDerivedEvidence> {
        let plan = &self.admission.plan;
        let block_path = self.block_path();
        let validation_certificate = DocumentValidationCertificate::mint_localized(
            &self.state.validation_certificate,
            block_path,
            self.state.document.node_at(block_path)?,
            preview.node_at(block_path)?,
            schema,
            &self.state.validation_certificate.resource_limits,
        )?
        .promote_localized_canonical(canonical_artifact, derivations, plan)?;
        #[cfg(test)]
        LOCALIZED_INDEX_PROMOTION_ATTEMPTS
            .set(LOCALIZED_INDEX_PROMOTION_ATTEMPTS.get().saturating_add(1));
        let cache_budget = self
            .state
            .validation_certificate
            .resource_limits
            .max_input_bytes;
        #[cfg(test)]
        let cache_budget = FORCE_LOCALIZED_INDEX_BUDGET.get().unwrap_or(cache_budget);
        let localized_text_index = self.state.localized_text_index.as_ref().and_then(|index| {
            index.carry_after_textblock_edit(
                &self.state.validation_certificate,
                self.admission,
                block_path,
                preview,
                canonical_artifact,
                cache_budget,
                &derivations.position_map,
                &derivations.rendered_text,
            )
        });
        #[cfg(test)]
        if localized_text_index.is_some() {
            LOCALIZED_INDEX_PROMOTION_SUCCESSES
                .set(LOCALIZED_INDEX_PROMOTION_SUCCESSES.get().saturating_add(1));
        } else {
            LOCALIZED_INDEX_PROMOTION_DROPS
                .set(LOCALIZED_INDEX_PROMOTION_DROPS.get().saturating_add(1));
        }
        Some(PreparedDerivedEvidence {
            request_id: self.admission.request_id,
            base_document_root: self.state.document.root().clone(),
            preview_root: preview.root().clone(),
            base_validation: self.state.validation_certificate.clone(),
            base_render_seal: Arc::clone(&self.admission.render_seal),
            base_lookup_seal: Arc::clone(&self.admission.lookup_seal),
            max_operations_per_transaction: self.admission.max_operations_per_transaction,
            max_undo_groups: self.admission.max_undo_groups,
            max_derived_output_bytes: self.admission.max_derived_output_bytes,
            max_undo_retained_units: self.admission.max_undo_retained_units,
            max_length: self.admission.max_length,
            derivation_identity_seal: Arc::clone(&derivations.identity_seal),
            preview_rendered_scalars: derivations.rendered_scalars,
            preview_document_text_bytes: derivations.document_text_bytes,
            preview_document_node_count: derivations.document_node_count,
            preview_position_total_scalars: derivations.position_map.total_scalars(),
            preview_position_block_count: derivations.position_map.block_count(),
            canonical_artifact: canonical_artifact.clone(),
            canonical_serialized_len: canonical_artifact.serialized_len(),
            validation_certificate,
            localized_text_index,
            localized_render_transition_proof: Some(LocalizedRenderTransitionProof {
                base_document_root: self.state.document.root().clone(),
                preview_root: preview.root().clone(),
                base_render_seal: Arc::clone(&self.admission.render_seal),
                resource_limits: self.state.validation_certificate.resource_limits.clone(),
                schema_fingerprint: Arc::clone(
                    &self.state.validation_certificate.schema_fingerprint,
                ),
                max_operations_per_transaction: self.admission.max_operations_per_transaction,
                max_undo_groups: self.admission.max_undo_groups,
                max_derived_output_bytes: self.admission.max_derived_output_bytes,
                max_undo_retained_units: self.admission.max_undo_retained_units,
                max_length: self.admission.max_length,
                derivation_identity_seal: Arc::clone(&derivations.identity_seal),
                target_top_level_index: plan.affected_top_level_index,
                document_scalar_delta: i32::try_from(plan.inserted_scalars)
                    .ok()?
                    .checked_sub(i32::try_from(plan.removed_scalars).ok()?)?,
                rendered_scalar_delta: plan.rendered_scalar_delta,
                top_level_cardinality: self.state.document.root().child_count(),
                operation_kind: if plan.removed_scalars == 0 {
                    LocalizedRenderOperationKind::TextblockInsert
                } else {
                    LocalizedRenderOperationKind::ReplaceText
                },
            }),
        })
    }

    pub(crate) fn document_position(&self) -> u32 {
        self.admission.inserted_document_position
    }

    pub(crate) fn range_end_scalar(&self) -> u32 {
        self.state
            .position_map
            .doc_to_scalar(self.admission.plan.range_end, &self.state.document)
    }

    pub(crate) fn range_end(&self) -> u32 {
        self.admission.plan.range_end
    }

    pub(crate) fn base_document_node_count(&self) -> usize {
        self.state.document_node_count
    }

    pub(crate) fn canonical_artifact(&self) -> &CanonicalArtifact {
        &self.state.canonical_artifact
    }

    pub(crate) fn base_raw_text_utf8_bytes(&self) -> usize {
        self.state.validation_certificate.raw_text_utf8_bytes
    }

    pub(crate) fn base_rendered_scalars(&self) -> u32 {
        self.state.rendered_scalars
    }

    pub(crate) fn creates_leaf(&self) -> bool {
        self.admission.plan.creates_leaf
    }

    pub(crate) fn block_path(&self) -> &[u32] {
        self.state
            .position_map
            .block_path(self.admission.plan.leaf.block_index)
            .expect("validated admission retains its position block")
    }

    pub(crate) fn affected_top_level_index(&self) -> usize {
        self.admission.plan.affected_top_level_index
    }

    pub(crate) fn rendered_scalar_position(&self) -> u32 {
        self.state.position_map.doc_to_scalar(
            self.admission.inserted_document_position,
            &self.state.document,
        )
    }

    pub(crate) fn rendered_text(&self) -> &str {
        &self.state.rendered_text
    }

    #[cfg(test)]
    pub(crate) fn next_raw_text_scalars(&self) -> u64 {
        self.admission.plan.next_raw_text_scalars
    }

    #[cfg(test)]
    pub(crate) fn next_raw_text_utf8_bytes(&self) -> usize {
        self.admission.plan.next_raw_text_utf8_bytes
    }

    #[cfg(test)]
    pub(crate) fn next_rendered_scalars(&self) -> u32 {
        self.admission.plan.next_rendered_scalars
    }

    #[cfg(test)]
    pub(crate) fn next_canonical_serialized_len(&self) -> usize {
        self.admission.plan.next_canonical_serialized_len
    }

    #[cfg(test)]
    pub(crate) fn history_undo_units(&self) -> u64 {
        self.admission.plan.history_undo_units
    }

    #[cfg(test)]
    pub(crate) fn operation_result(&self) -> &ResolvedSelection {
        &self.admission.plan.operation_result
    }

    #[cfg(test)]
    pub(crate) fn stored_marks(&self) -> Option<&[crate::model::Mark]> {
        self.state.stored_marks.as_deref()
    }
}
