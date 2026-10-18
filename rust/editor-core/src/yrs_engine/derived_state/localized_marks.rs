use super::localized_index::LocalizedTextLeafIndex;
#[cfg(test)]
use super::observability::FORCE_LOCALIZED_INDEX_BUDGET;
use super::validation::DocumentValidationCertificate;
use super::DerivedStateCache;
use crate::boundary::ResourceLimits;
use crate::model::{Document, Node};
use crate::position::PositionMap;
use crate::render::incremental::{CachedRenderBlocks, CachedRenderTransitionUpdate};
use crate::schema::Schema;
use crate::yrs_engine::canonical::{replacement_node, CanonicalArtifact, CanonicalSchemaContext};
use std::sync::Arc;

const UNCHANGED_TEXTBLOCK_SIZE_DELTA: i32 = 0;

#[derive(Debug, Clone)]
pub(crate) struct LocalizedMarkEvidence {
    base_document_root: Node,
    base_render_blocks: Arc<CachedRenderBlocks>,
    render_update: CachedRenderTransitionUpdate,
    block_index: usize,
    validation_certificate: DocumentValidationCertificate,
}

pub(crate) struct LocalizedMarkCandidate {
    pub(crate) canonical_artifact: CanonicalArtifact,
    pub(crate) position_map: PositionMap,
    pub(crate) rendered_text: Arc<str>,
    pub(crate) rendered_scalars: u32,
    pub(crate) render_blocks: Arc<CachedRenderBlocks>,
    pub(crate) evidence: LocalizedMarkEvidence,
}

impl DerivedStateCache {
    pub(crate) fn localized_mark_candidate(
        &self,
        document: &Document,
        from: u32,
        to: u32,
        schema: &Schema,
        canonical_schema: &CanonicalSchemaContext,
        resource_limits: &ResourceLimits,
    ) -> Option<LocalizedMarkCandidate> {
        let block_index = self.textblock_index_at(from)?;
        if self.textblock_index_at(to)? != block_index
            || !self
                .canonical_artifact
                .schema_context()
                .ptr_eq(canonical_schema)
            || !self
                .validation_certificate
                .canonical_artifact
                .ptr_eq(&self.canonical_artifact)
        {
            return None;
        }
        let path = self.position_map.block_path(block_index)?;
        let base_block = self.document.node_at(path)?;
        let block = replacement_node(self.document.root(), document.root(), path)?;
        if base_block.node_type() != block.node_type()
            || base_block.node_size() != block.node_size()
            || base_block.text_content() != block.text_content()
        {
            return None;
        }
        let canonical_artifact =
            CanonicalArtifact::derive_localized(&self.canonical_artifact, document, path)?;
        let validation_certificate = DocumentValidationCertificate::mint_localized(
            &self.validation_certificate,
            path,
            base_block,
            block,
            schema,
            resource_limits,
        )?
        .with_unmaterialized_canonical(&canonical_artifact);
        let transition = self
            .render_blocks
            .transition_localized_textblock(
                &self.document,
                document,
                schema,
                usize::try_from(*path.first()?).ok()?,
                UNCHANGED_TEXTBLOCK_SIZE_DELTA,
                resource_limits,
            )
            .ok()?;
        Some(LocalizedMarkCandidate {
            canonical_artifact,
            position_map: self.position_map.clone(),
            rendered_text: self.rendered_text.clone(),
            rendered_scalars: self.rendered_scalars,
            render_blocks: Arc::new(transition.cache),
            evidence: LocalizedMarkEvidence {
                base_document_root: self.document.root().clone(),
                base_render_blocks: Arc::clone(&self.render_blocks),
                render_update: transition.update,
                block_index,
                validation_certificate,
            },
        })
    }
}

impl LocalizedMarkEvidence {
    pub(super) fn validation_certificate(&self) -> &DocumentValidationCertificate {
        &self.validation_certificate
    }

    pub(super) fn render_update_from(
        &self,
        render_blocks: &Arc<CachedRenderBlocks>,
    ) -> Option<CachedRenderTransitionUpdate> {
        Arc::ptr_eq(&self.base_render_blocks, render_blocks).then(|| self.render_update.clone())
    }

    pub(super) fn derives_from(&self, installed: &DerivedStateCache) -> bool {
        installed
            .document
            .root()
            .shares_storage_with(&self.base_document_root)
    }

    pub(super) fn carry_localized_text_index(
        &self,
        installed: &DerivedStateCache,
        document: &Document,
        validation: &DocumentValidationCertificate,
        position_map: &PositionMap,
    ) -> Option<LocalizedTextLeafIndex> {
        let path = position_map.block_path(self.block_index)?;
        let cache_budget = validation.resource_limits.max_input_bytes;
        #[cfg(test)]
        let cache_budget = FORCE_LOCALIZED_INDEX_BUDGET.get().unwrap_or(cache_budget);
        installed
            .localized_text_index
            .as_ref()?
            .carry_after_textblock_marks(
                &installed.validation_certificate,
                validation,
                self.block_index,
                installed.document.node_at(path)?,
                document.node_at(path)?,
                position_map,
                cache_budget,
            )
    }
}
