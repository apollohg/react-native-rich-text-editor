use super::insert_admission::LocalizedTextblockEditPlan;
use crate::boundary::ResourceLimits;
use crate::model::{Document, Node};
use crate::schema::Schema;
use crate::transform::{
    DocumentStats, DocumentValidationMetrics, DocumentValidationReport, DocumentValidator,
};
use crate::yrs_engine;
use crate::yrs_engine::canonical::CanonicalArtifact;
use crate::yrs_engine::compiler::CompiledDocumentDerivations;
use std::sync::Arc;
use yrs::branch::{Branch, BranchID};
use yrs::types::xml::XmlFragmentRef;
use yrs::ReadTxn;

/// Reusable document-validation evidence with a controlled state-revision
/// reseal. Document, schema, limits, canonical, and epoch facts never mutate.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedDocumentEvidence {
    pub(super) document_root: Node,
    pub(super) canonical_artifact: CanonicalArtifact,
    pub(super) canonical_format_version: u8,
    pub(super) validation: DocumentValidationReport,
    pub(super) validation_report_seal: [usize; 4],
    pub(super) resource_limits: ResourceLimits,
    pub(super) editing_limits: yrs_engine::EditingLimits,
    pub(super) max_length: Option<u32>,
    pub(super) schema_fingerprint: Arc<str>,
    pub(super) canonical_schema: yrs_engine::canonical::CanonicalSchemaContext,
    pub(super) fragment_name: Arc<str>,
    pub(super) store_token: usize,
    pub(super) fragment_id: BranchID,
    pub(super) engine_epoch: u64,
    pub(super) target_document_revision: u64,
    pub(super) target_state_revision: u64,
    pub(super) target_yrs_state_epoch: u64,
}

pub(crate) struct ValidatedCandidateContext<'a> {
    pub evidence: &'a ValidatedDocumentEvidence,
    pub table_projection: Option<&'a crate::tables::admission::AdmittedTableProjection>,
    pub canonical_schema: &'a yrs_engine::canonical::CanonicalSchemaContext,
    pub fragment_name: &'a str,
    pub engine_epoch: u64,
}

impl ValidatedDocumentEvidence {
    pub(super) fn validation_report_seal(validation: DocumentValidationReport) -> [usize; 4] {
        [
            validation.stats.node_count,
            validation.stats.max_depth,
            validation.metrics.metadata_bytes,
            validation.metrics.validation_work,
        ]
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mint<T: ReadTxn>(
        document: &Document,
        validation_source_root: &Node,
        canonical_artifact: &CanonicalArtifact,
        validation: DocumentValidationReport,
        resource_limits: &ResourceLimits,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
        schema_fingerprint: &str,
        canonical_schema: &yrs_engine::canonical::CanonicalSchemaContext,
        fragment_name: &str,
        txn: &T,
        fragment: &XmlFragmentRef,
        engine_epoch: u64,
        target_document_revision: u64,
        target_state_revision: u64,
        target_yrs_state_epoch: u64,
    ) -> Option<Self> {
        if !validation_source_root.shares_storage_with(document.root())
            || !canonical_artifact.matches_exact_source_document(document)
            || canonical_artifact.format_version()
                != yrs_engine::canonical::CANONICAL_ARTIFACT_FORMAT_VERSION
            || !canonical_artifact.schema_context().ptr_eq(canonical_schema)
            || canonical_artifact.schema_fingerprint() != schema_fingerprint
            || canonical_schema.schema_fingerprint() != schema_fingerprint
            || validation.stats.node_count > resource_limits.max_document_nodes
            || validation.stats.max_depth > resource_limits.max_document_depth
            || validation.metrics.metadata_bytes > resource_limits.max_input_bytes
            || validation.metrics.validation_work
                > resource_limits.max_document_nodes.saturating_mul(128)
            || max_length
                .is_some_and(|limit| canonical_artifact.text_scalar_len() > u64::from(limit))
        {
            return None;
        }
        #[cfg(test)]
        yrs_engine::observability::record_validated_evidence_construction();
        Some(Self {
            document_root: document.root().clone(),
            canonical_artifact: canonical_artifact.clone(),
            canonical_format_version: canonical_artifact.format_version(),
            validation,
            validation_report_seal: Self::validation_report_seal(validation),
            resource_limits: resource_limits.clone(),
            editing_limits: editing_limits.clone(),
            max_length,
            schema_fingerprint: schema_fingerprint.into(),
            canonical_schema: canonical_schema.clone(),
            fragment_name: fragment_name.into(),
            store_token: txn.store() as *const _ as usize,
            fragment_id: AsRef::<Branch>::as_ref(fragment).id(),
            engine_epoch,
            target_document_revision,
            target_state_revision,
            target_yrs_state_epoch,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admitted_validation_report<T: ReadTxn>(
        &self,
        document: &Document,
        canonical_artifact: &CanonicalArtifact,
        resource_limits: &ResourceLimits,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
        schema_fingerprint: &str,
        canonical_schema: &yrs_engine::canonical::CanonicalSchemaContext,
        fragment_name: &str,
        txn: &T,
        fragment: &XmlFragmentRef,
        engine_epoch: u64,
        target_document_revision: u64,
        target_state_revision: u64,
        target_yrs_state_epoch: u64,
    ) -> Option<DocumentValidationReport> {
        (self.document_root.shares_storage_with(document.root())
            && self.canonical_artifact.ptr_eq(canonical_artifact)
            && self
                .canonical_artifact
                .matches_exact_source_document(document)
            && self.canonical_format_version == canonical_artifact.format_version()
            && self.canonical_format_version
                == yrs_engine::canonical::CANONICAL_ARTIFACT_FORMAT_VERSION
            && self.validation_report_seal == Self::validation_report_seal(self.validation)
            && self.resource_limits == *resource_limits
            && self.editing_limits == *editing_limits
            && self.max_length == max_length
            && self.schema_fingerprint.as_ref() == schema_fingerprint
            && self.canonical_schema.ptr_eq(canonical_schema)
            && self
                .canonical_artifact
                .schema_context()
                .ptr_eq(canonical_schema)
            && self.fragment_name.as_ref() == fragment_name
            && self.store_token == txn.store() as *const _ as usize
            && self.fragment_id == AsRef::<Branch>::as_ref(fragment).id()
            && self.engine_epoch == engine_epoch
            && self.target_document_revision == target_document_revision
            && self.target_state_revision == target_state_revision
            && self.target_yrs_state_epoch == target_yrs_state_epoch)
            .then_some(self.validation)
    }

    #[cfg(test)]
    pub(super) fn tampered_for_test(&self, schema: &Schema) -> Vec<(&'static str, Self)> {
        let foreign_document = crate::serialize::from_prosemirror_json(
            &serde_json::json!({
                "type": schema.doc_node_type(),
                "content": [{"type": "paragraph", "content": [{"type": "text", "text": "foreign"}]}]
            }),
            schema,
            crate::serialize::UnknownTypeMode::Preserve,
        )
        .expect("foreign evidence fixture should parse");
        let foreign_artifact = self
            .canonical_schema
            .derive(&foreign_document)
            .expect("foreign evidence fixture should canonicalize");
        let mut variants = Vec::new();
        macro_rules! tamper {
            ($name:literal, $field:ident, $value:expr) => {{
                let mut value = self.clone();
                value.$field = $value;
                variants.push(($name, value));
            }};
        }
        tamper!(
            "documentRoot",
            document_root,
            foreign_document.root().clone()
        );
        tamper!("canonicalArtifact", canonical_artifact, foreign_artifact);
        tamper!(
            "canonicalFormat",
            canonical_format_version,
            self.canonical_format_version.wrapping_add(1)
        );
        tamper!(
            "schemaContext",
            canonical_schema,
            yrs_engine::canonical::CanonicalSchemaContext::new(schema)
        );
        tamper!(
            "schemaFingerprint",
            schema_fingerprint,
            Arc::<str>::from("tampered")
        );
        tamper!("fragment", fragment_name, Arc::<str>::from("tampered"));
        tamper!("store", store_token, self.store_token.wrapping_add(1));
        let foreign_doc = yrs::Doc::new();
        let foreign_fragment = foreign_doc.get_or_insert_xml_fragment("foreign");
        tamper!(
            "fragmentIdentity",
            fragment_id,
            AsRef::<Branch>::as_ref(&foreign_fragment).id()
        );
        tamper!(
            "engineEpoch",
            engine_epoch,
            self.engine_epoch.wrapping_add(1)
        );
        tamper!(
            "documentRevision",
            target_document_revision,
            self.target_document_revision.wrapping_add(1)
        );
        tamper!(
            "stateRevision",
            target_state_revision,
            self.target_state_revision.wrapping_add(1)
        );
        tamper!(
            "targetEpoch",
            target_yrs_state_epoch,
            self.target_yrs_state_epoch.wrapping_add(1)
        );
        let mut resource_limits = self.resource_limits.clone();
        resource_limits.max_document_nodes = resource_limits.max_document_nodes.saturating_add(1);
        tamper!("resourceLimits", resource_limits, resource_limits);
        let mut editing_limits = self.editing_limits.clone();
        editing_limits.max_derived_output_bytes =
            editing_limits.max_derived_output_bytes.saturating_add(1);
        tamper!("editingLimits", editing_limits, editing_limits);
        tamper!(
            "maxLength",
            max_length,
            self.max_length.map(|value| value + 1)
        );
        let mut validation = self.validation;
        validation.stats.node_count = validation.stats.node_count.saturating_add(1);
        tamper!("validationReport", validation, validation);
        variants
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DocumentValidationCertificate {
    pub(super) stats: DocumentStats,
    pub(super) depth_counts: Arc<[usize]>,
    pub(super) metrics: DocumentValidationMetrics,
    pub(super) resource_limits: ResourceLimits,
    pub(super) schema_fingerprint: Arc<str>,
    pub(super) canonical_artifact: CanonicalArtifact,
    pub(super) canonical_fingerprint: [u8; 32],
    pub(super) canonical_serialized_len: usize,
    pub(super) canonical_fingerprint_materialized: bool,
    pub(super) raw_text_scalars: u64,
    pub(super) raw_text_utf8_bytes: usize,
    pub(super) document_revision: u64,
    pub(super) state_revision: u64,
    pub(super) yrs_state_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootChildWindow {
    from: usize,
    old_to: usize,
    new_to: usize,
}

impl RootChildWindow {
    pub(crate) fn between(old_root: &Node, new_root: &Node) -> Option<Self> {
        let old_children = old_root.content()?.children();
        let new_children = new_root.content()?.children();
        let from = old_children
            .iter()
            .zip(new_children)
            .take_while(|(old, new)| old.shares_storage_with(new))
            .count();
        let suffix = old_children[from..]
            .iter()
            .rev()
            .zip(new_children[from..].iter().rev())
            .take_while(|(old, new)| old.shares_storage_with(new))
            .count();
        Some(Self {
            from,
            old_to: old_children.len().checked_sub(suffix)?,
            new_to: new_children.len().checked_sub(suffix)?,
        })
    }

    pub(crate) fn from(&self) -> usize {
        self.from
    }

    pub(crate) fn old_to(&self) -> usize {
        self.old_to
    }

    pub(crate) fn new_to(&self) -> usize {
        self.new_to
    }
}

struct ValidationContribution {
    report: DocumentValidationReport,
    depth_counts: Vec<usize>,
}

impl ValidationContribution {
    fn subtree(
        node: &Node,
        schema: &Schema,
        limits: &ResourceLimits,
        depth: usize,
    ) -> Option<Self> {
        Some(Self {
            report: DocumentValidator::validate_subtree_report(node, schema, limits, depth).ok()?,
            depth_counts: DocumentValidationCertificate::subtree_depth_counts(node, depth),
        })
    }

    fn node_itself(
        node: &Node,
        schema: &Schema,
        limits: &ResourceLimits,
        depth: usize,
    ) -> Option<Self> {
        let mut depth_counts = vec![0; depth.checked_add(1)?];
        depth_counts[depth] = 1;
        Some(Self {
            report: DocumentValidator::validate_node_itself_report(node, schema, limits, depth)
                .ok()?,
            depth_counts,
        })
    }
}

impl PartialEq for DocumentValidationCertificate {
    fn eq(&self, other: &Self) -> bool {
        let canonical_identity_matches = if self.canonical_fingerprint_materialized
            && other.canonical_fingerprint_materialized
        {
            self.canonical_fingerprint == other.canonical_fingerprint
                && self.canonical_serialized_len == other.canonical_serialized_len
        } else if !self.canonical_fingerprint_materialized
            && !other.canonical_fingerprint_materialized
        {
            self.canonical_artifact.ptr_eq(&other.canonical_artifact)
        } else {
            false
        };
        self.stats == other.stats
            && self.depth_counts == other.depth_counts
            && self.metrics == other.metrics
            && self.resource_limits == other.resource_limits
            && self.schema_fingerprint == other.schema_fingerprint
            && canonical_identity_matches
            && self.raw_text_scalars == other.raw_text_scalars
            && self.raw_text_utf8_bytes == other.raw_text_utf8_bytes
            && self.document_revision == other.document_revision
            && self.state_revision == other.state_revision
            && self.yrs_state_epoch == other.yrs_state_epoch
    }
}

impl Eq for DocumentValidationCertificate {}

#[allow(dead_code)]
impl DocumentValidationCertificate {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_report(
        document: &Document,
        validation: DocumentValidationReport,
        canonical_artifact: &CanonicalArtifact,
        resource_limits: &ResourceLimits,
        schema_fingerprint: &str,
        document_revision: u64,
        state_revision: u64,
        yrs_state_epoch: u64,
    ) -> Self {
        #[cfg(test)]
        yrs_engine::observability::record_validation_certificate_construction();
        Self {
            stats: validation.stats,
            depth_counts: Self::subtree_depth_counts(
                document.root(),
                crate::transform::DOCUMENT_ROOT_DEPTH,
            )
            .into(),
            metrics: validation.metrics,
            resource_limits: resource_limits.clone(),
            schema_fingerprint: Arc::from(schema_fingerprint),
            canonical_artifact: canonical_artifact.clone(),
            canonical_fingerprint: [0; 32],
            canonical_serialized_len: 0,
            canonical_fingerprint_materialized: false,
            raw_text_scalars: canonical_artifact.text_scalar_len(),
            raw_text_utf8_bytes: canonical_artifact.text_utf8_bytes(),
            document_revision,
            state_revision,
            yrs_state_epoch,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn mint(
        document: &Document,
        canonical_artifact: &CanonicalArtifact,
        schema: &Schema,
        resource_limits: &ResourceLimits,
        schema_fingerprint: &str,
        document_revision: u64,
        state_revision: u64,
        yrs_state_epoch: u64,
    ) -> Option<Self> {
        if crate::schema::schema_fingerprint(schema) != schema_fingerprint
            || canonical_artifact.schema_fingerprint() != schema_fingerprint
            || !canonical_artifact.matches_document(document)
        {
            return None;
        }
        let validation =
            DocumentValidator::validate_report(document, schema, resource_limits).ok()?;
        crate::transform::validate_canonical_marks(document, schema).ok()?;
        let mut certificate = Self::from_report(
            document,
            validation,
            canonical_artifact,
            resource_limits,
            schema_fingerprint,
            document_revision,
            state_revision,
            yrs_state_epoch,
        );
        certificate.canonical_fingerprint = canonical_artifact.sha256();
        certificate.canonical_serialized_len = canonical_artifact.serialized_len();
        certificate.canonical_fingerprint_materialized = true;
        Some(certificate)
    }

    pub(crate) fn depth_slots(&self) -> usize {
        self.depth_counts.len()
    }

    pub(crate) fn stats(&self) -> DocumentStats {
        self.stats
    }

    pub(crate) fn document_revision(&self) -> u64 {
        self.document_revision
    }

    pub(crate) fn state_revision(&self) -> u64 {
        self.state_revision
    }

    pub(crate) fn yrs_state_epoch(&self) -> u64 {
        self.yrs_state_epoch
    }

    pub(crate) fn canonical_fingerprint(&self) -> [u8; 32] {
        if self.canonical_fingerprint_materialized {
            self.canonical_fingerprint
        } else {
            self.canonical_artifact.sha256()
        }
    }

    // Keep every sealed identity dimension explicit so exact certificate matching stays auditable.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn matches_materialized_identity(
        &self,
        canonical_artifact: &CanonicalArtifact,
        canonical_fingerprint: [u8; 32],
        canonical_serialized_len: usize,
        resource_limits: &ResourceLimits,
        schema_fingerprint: &str,
        document_revision: u64,
        state_revision: u64,
        yrs_state_epoch: u64,
    ) -> bool {
        self.resource_limits == *resource_limits
            && self.schema_fingerprint.as_ref() == schema_fingerprint
            && self.document_revision == document_revision
            && self.state_revision == state_revision
            && self.yrs_state_epoch == yrs_state_epoch
            && self.raw_text_scalars == canonical_artifact.text_scalar_len()
            && self.raw_text_utf8_bytes == canonical_artifact.text_utf8_bytes()
            && self.canonical_artifact.ptr_eq(canonical_artifact)
            && if self.canonical_fingerprint_materialized {
                self.canonical_fingerprint == canonical_fingerprint
                    && self.canonical_serialized_len == canonical_serialized_len
            } else {
                canonical_artifact.sha256() == canonical_fingerprint
                    && canonical_artifact.serialized_len() == canonical_serialized_len
            }
    }

    #[cfg(test)]
    pub(crate) fn canonical_fingerprint_materialized_for_test(&self) -> bool {
        self.canonical_fingerprint_materialized
    }

    #[cfg(test)]
    pub(super) fn materialize_canonical_artifact(&mut self) {
        if !self.canonical_fingerprint_materialized {
            self.canonical_fingerprint = self.canonical_artifact.sha256();
            self.canonical_serialized_len = self.canonical_artifact.serialized_len();
            self.canonical_fingerprint_materialized = true;
        }
    }

    pub(super) fn reseal_state_revision(&mut self, state_revision: u64) {
        self.state_revision = state_revision;
    }

    pub(crate) fn subtree_depth_counts(node: &Node, depth: usize) -> Vec<usize> {
        let mut counts = Vec::<usize>::new();
        let mut pending = vec![(node, depth)];
        while let Some((node, depth)) = pending.pop() {
            if counts.len() <= depth {
                counts.resize(depth + 1, 0);
            }
            counts[depth] += 1;
            if let Some(content) = node.content() {
                pending.extend(content.iter().map(|child| (child, depth + 1)));
            }
        }
        counts
    }

    pub(crate) fn mint_localized(
        previous: &Self,
        block_path: &[u32],
        old_block: &Node,
        new_block: &Node,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> Option<Self> {
        let depth = crate::transform::DOCUMENT_ROOT_DEPTH.checked_add(block_path.len())?;
        let removed = ValidationContribution::subtree(old_block, schema, limits, depth)?;
        let added = ValidationContribution::subtree(new_block, schema, limits, depth)?;
        crate::transform::validate_subtree_marks(new_block, schema).ok()?;
        previous.with_contributions_replaced(&[removed], &[added], limits)
    }

    pub(crate) fn certifies_root_window(
        &self,
        base: &Document,
        candidate: &Document,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> bool {
        let certified =
            RootChildWindow::between(base.root(), candidate.root()).is_some_and(|window| {
                Self::mint_root_window(self, base.root(), candidate.root(), window, schema, limits)
                    .is_some()
            });
        #[cfg(test)]
        if certified {
            super::observability::ROOT_WINDOW_CERTIFICATION_COUNT.set(
                super::observability::ROOT_WINDOW_CERTIFICATION_COUNT
                    .get()
                    .saturating_add(1),
            );
        }
        certified
    }

    pub(super) fn mint_root_window(
        previous: &Self,
        old_root: &Node,
        new_root: &Node,
        window: RootChildWindow,
        schema: &Schema,
        limits: &ResourceLimits,
    ) -> Option<Self> {
        let root_depth = crate::transform::DOCUMENT_ROOT_DEPTH;
        let child_depth = root_depth.checked_add(1)?;
        if old_root.node_type() != new_root.node_type() {
            return None;
        }
        let old_children = old_root.content()?;
        let new_children = new_root.content()?;
        let mut removed = vec![ValidationContribution::node_itself(
            old_root, schema, limits, root_depth,
        )?];
        for index in window.from..window.old_to {
            removed.push(ValidationContribution::subtree(
                old_children.child(index)?,
                schema,
                limits,
                child_depth,
            )?);
        }
        let mut added = vec![ValidationContribution::node_itself(
            new_root, schema, limits, root_depth,
        )?];
        for index in window.from..window.new_to {
            let child = new_children.child(index)?;
            crate::transform::validate_subtree_marks(child, schema).ok()?;
            added.push(ValidationContribution::subtree(
                child,
                schema,
                limits,
                child_depth,
            )?);
        }
        previous.with_contributions_replaced(&removed, &added, limits)
    }

    fn with_contributions_replaced(
        &self,
        removed: &[ValidationContribution],
        added: &[ValidationContribution],
        limits: &ResourceLimits,
    ) -> Option<Self> {
        if self.resource_limits != *limits {
            return None;
        }
        let mut depth_counts = self.depth_counts.to_vec();
        let mut node_count = self.stats.node_count;
        let mut metadata_bytes = self.metrics.metadata_bytes;
        let mut validation_work = self.metrics.validation_work;
        for contribution in removed {
            node_count = node_count.checked_sub(contribution.report.stats.node_count)?;
            metadata_bytes =
                metadata_bytes.checked_sub(contribution.report.metrics.metadata_bytes)?;
            validation_work =
                validation_work.checked_sub(contribution.report.metrics.validation_work)?;
            for (depth, count) in contribution.depth_counts.iter().enumerate() {
                let slot = depth_counts.get_mut(depth)?;
                *slot = slot.checked_sub(*count)?;
            }
        }
        for contribution in added {
            node_count = node_count.checked_add(contribution.report.stats.node_count)?;
            metadata_bytes =
                metadata_bytes.checked_add(contribution.report.metrics.metadata_bytes)?;
            validation_work =
                validation_work.checked_add(contribution.report.metrics.validation_work)?;
            if depth_counts.len() < contribution.depth_counts.len() {
                depth_counts.resize(contribution.depth_counts.len(), 0);
            }
            for (depth, count) in contribution.depth_counts.iter().enumerate() {
                depth_counts[depth] = depth_counts[depth].checked_add(*count)?;
            }
        }
        let max_depth = depth_counts.iter().rposition(|count| *count > 0)?;
        depth_counts.truncate(max_depth.checked_add(1)?);
        let stats = DocumentStats {
            node_count,
            max_depth,
        };
        let metrics = DocumentValidationMetrics {
            metadata_bytes,
            validation_work,
        };
        if stats.node_count > limits.max_document_nodes
            || stats.max_depth > limits.max_document_depth
            || metrics.metadata_bytes > limits.max_input_bytes
            || metrics.validation_work > crate::transform::document_validation_work_limit(limits)
        {
            return None;
        }
        Some(Self {
            stats,
            depth_counts: depth_counts.into(),
            metrics,
            ..self.clone()
        })
    }

    pub(super) fn promote_localized_canonical(
        self,
        canonical_artifact: &CanonicalArtifact,
        derivations: &CompiledDocumentDerivations,
        plan: &LocalizedTextblockEditPlan,
    ) -> Option<Self> {
        if canonical_artifact.schema_fingerprint() != self.schema_fingerprint.as_ref()
            || canonical_artifact.format_version()
                != yrs_engine::canonical::CANONICAL_ARTIFACT_FORMAT_VERSION
            || canonical_artifact.serialized_len() != plan.next_canonical_serialized_len
            || canonical_artifact.text_scalar_len() != plan.next_raw_text_scalars
            || canonical_artifact.text_utf8_bytes() != plan.next_raw_text_utf8_bytes
            || derivations.document_node_count != self.stats.node_count
            || derivations.document_text_bytes != plan.next_raw_text_utf8_bytes
            || derivations.rendered_scalars != plan.next_rendered_scalars
        {
            return None;
        }
        Some(self.with_unmaterialized_canonical(canonical_artifact))
    }

    pub(super) fn with_unmaterialized_canonical(
        self,
        canonical_artifact: &CanonicalArtifact,
    ) -> Self {
        Self {
            canonical_artifact: canonical_artifact.clone(),
            canonical_fingerprint: [0; 32],
            canonical_serialized_len: canonical_artifact.serialized_len(),
            canonical_fingerprint_materialized: false,
            raw_text_scalars: canonical_artifact.text_scalar_len(),
            raw_text_utf8_bytes: canonical_artifact.text_utf8_bytes(),
            ..self
        }
    }

    pub(super) fn report(&self) -> DocumentValidationReport {
        DocumentValidationReport {
            stats: self.stats,
            metrics: self.metrics,
        }
    }

    pub(super) fn resealed(
        self,
        document_revision: u64,
        state_revision: u64,
        yrs_state_epoch: u64,
    ) -> Self {
        Self {
            document_revision,
            state_revision,
            yrs_state_epoch,
            ..self
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn matches(
        &self,
        canonical_artifact: &CanonicalArtifact,
        resource_limits: &ResourceLimits,
        schema_fingerprint: &str,
        document_revision: u64,
        state_revision: u64,
        yrs_state_epoch: u64,
    ) -> bool {
        self.resource_limits == *resource_limits
            && self.schema_fingerprint.as_ref() == schema_fingerprint
            && (self.canonical_artifact.ptr_eq(canonical_artifact)
                || (self.canonical_fingerprint() == canonical_artifact.sha256()
                    && self.canonical_serialized_len == canonical_artifact.serialized_len()))
            && self.raw_text_scalars == canonical_artifact.text_scalar_len()
            && self.raw_text_utf8_bytes == canonical_artifact.text_utf8_bytes()
            && self.document_revision == document_revision
            && self.state_revision == state_revision
            && self.yrs_state_epoch == yrs_state_epoch
    }
}
