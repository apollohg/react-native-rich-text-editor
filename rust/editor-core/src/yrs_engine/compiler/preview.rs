use crate::model::Document;
use crate::position::update::UpdateMode;
use crate::position::PositionMap;
use crate::transform::{DocumentValidator, Step, StepMap};
use crate::yrs_engine;
use crate::yrs_engine::canonical::{CanonicalArtifact, CanonicalSchemaContext};
use crate::yrs_engine::compiler::admission::PreparedSemanticAdmission;
#[cfg(test)]
use crate::yrs_engine::compiler::observability::FORCE_LOCALIZED_SEMANTIC_ALLOCATION_FAILURE;
use crate::yrs_engine::compiler::{
    document_text_bytes, CompilationContext, CompiledDocumentDerivations, PreparedSemanticContext,
};
use crate::yrs_engine::derived_state::ValidatedLocalizedTextblockEditAdmission;
use crate::yrs_engine::editing_limits::CheckedWork;
use crate::yrs_engine::{OperationError, OperationResult, TypedOperation, TypedTransaction};
use std::sync::Arc;

pub(super) struct LocalizedSemanticCompilation {
    pub(super) position: u32,
    pub(super) preview: Document,
    pub(super) step_map: StepMap,
    pub(super) derivations: LocalizedSemanticDerivations,
}

pub(super) struct LocalizedSemanticDerivations {
    pub(super) affected_top_level_blocks: Vec<usize>,
    pub(super) rendered_text: String,
    pub(super) rendered_scalars: u32,
    pub(super) document_text_bytes: usize,
    pub(super) document_node_count: usize,
    pub(super) raw_text_scalars: u64,
    pub(super) raw_text_utf8_bytes: usize,
}

pub(super) fn charge_preview_output(
    work: &mut CheckedWork,
    request_id: u64,
    operation_index: usize,
    preview: &Document,
    canonical_schema: &CanonicalSchemaContext,
    context: CompilationContext<'_>,
) -> OperationResult<CanonicalArtifact> {
    let artifact = canonical_schema.derive(preview).map_err(|error| {
        OperationError::engine_invariant_failed(
            request_id,
            Some(operation_index),
            format!("preview serialization failed: {error}"),
        )
    })?;
    charge_canonical_output(work, request_id, operation_index, &artifact, context)?;
    Ok(artifact)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn charge_localized_preview_output(
    work: &mut CheckedWork,
    request_id: u64,
    operation_index: usize,
    preview: &Document,
    canonical_schema: &CanonicalSchemaContext,
    context: CompilationContext<'_>,
    raw_text_scalars: u64,
    raw_text_utf8_bytes: usize,
) -> OperationResult<CanonicalArtifact> {
    let artifact = canonical_schema
        .derive_with_known_text_metrics(preview, raw_text_scalars, raw_text_utf8_bytes)
        .map_err(|error| {
            OperationError::engine_invariant_failed(
                request_id,
                Some(operation_index),
                format!("preview serialization failed: {error}"),
            )
        })?;
    charge_canonical_output(work, request_id, operation_index, &artifact, context)?;
    Ok(artifact)
}

pub(super) fn charge_canonical_output(
    work: &mut CheckedWork,
    request_id: u64,
    operation_index: usize,
    artifact: &CanonicalArtifact,
    context: CompilationContext<'_>,
) -> OperationResult<()> {
    work.charge_output_bytes(
        request_id,
        operation_index,
        artifact.serialized_len(),
        context.editing_limits.max_derived_output_bytes,
    )
}

pub(super) fn charge_prepared_preview_output(
    work: &mut CheckedWork,
    request_id: u64,
    operation_index: usize,
    admission: &PreparedSemanticAdmission,
    context: CompilationContext<'_>,
) -> OperationResult<CanonicalArtifact> {
    let artifact = admission.canonical_artifact();
    charge_canonical_output(work, request_id, operation_index, artifact, context)?;
    Ok(artifact.clone())
}

pub(super) fn prepared_candidate_matches(
    prepared: Option<PreparedSemanticContext<'_>>,
    operation_count: usize,
    operation_index: usize,
    candidate: &Document,
    context: CompilationContext<'_>,
    canonical_schema: &CanonicalSchemaContext,
) -> bool {
    operation_count == 1
        && operation_index == 0
        && prepared.is_some_and(|prepared| {
            *candidate == *prepared.expected_preview
                && prepared
                    .admission
                    .candidate_validation_ref()
                    .is_some_and(|validation| {
                        validation.admits_context(
                            prepared.expected_preview,
                            prepared.admission.canonical_artifact(),
                            context.resource_limits,
                            context.editing_limits,
                            context.max_length,
                            prepared.schema_fingerprint,
                            canonical_schema,
                        )
                    })
        })
}

pub(super) fn validate_preview(
    request_id: u64,
    operation_index: Option<usize>,
    preview: &Document,
    context: CompilationContext<'_>,
) -> OperationResult<()> {
    DocumentValidator::validate(preview, context.schema, context.resource_limits).map_err(
        |error| {
            let field = if error.code() == "DOCUMENT_LIMIT_EXCEEDED" {
                "document"
            } else {
                "content"
            };
            if error.code() == "DOCUMENT_LIMIT_EXCEEDED" {
                OperationError::document_limit_exceeded(
                    request_id,
                    operation_index,
                    field,
                    error.limit.unwrap_or(0) as u64,
                    error.actual.unwrap_or(0) as u64,
                )
            } else {
                OperationError::document_invalid(
                    request_id,
                    operation_index,
                    field,
                    error.to_string(),
                )
            }
        },
    )?;
    if let Some(limit) = context.max_length {
        let actual = preview.root().text_content().chars().count() as u64;
        if actual > limit as u64 {
            return Err(OperationError::document_limit_exceeded(
                request_id,
                operation_index,
                "maxLength",
                limit as u64,
                actual,
            ));
        }
    }
    Ok(())
}

pub(super) fn scalar_byte_offset(text: &str, scalar_offset: u32) -> Option<usize> {
    let mut scalars = 0u32;
    for (byte, _) in text.char_indices() {
        if scalars == scalar_offset {
            return Some(byte);
        }
        scalars = scalars.checked_add(1)?;
    }
    (scalars == scalar_offset).then_some(text.len())
}

pub(super) fn try_localized_semantic_compilation(
    context: CompilationContext<'_>,
    transaction: &TypedTransaction,
    validated: &ValidatedLocalizedTextblockEditAdmission<'_>,
) -> Option<LocalizedSemanticCompilation> {
    #[cfg(test)]
    if FORCE_LOCALIZED_SEMANTIC_ALLOCATION_FAILURE.get() {
        return None;
    }
    let [TypedOperation::InsertText { text, marks, .. }] = transaction.operations.as_slice() else {
        return None;
    };
    let position = validated.document_position();
    let (preview, step_map) = crate::transform::apply_step(
        context.document,
        &Step::InsertText {
            pos: position,
            text: text.clone(),
            marks: marks.clone(),
        },
        context.schema,
    )
    .ok()?;
    let rendered_byte = scalar_byte_offset(
        validated.rendered_text(),
        validated.rendered_scalar_position(),
    )?;
    let prefix_end = if validated.creates_leaf() {
        let placeholder = crate::render::empty_text_block_placeholder_string();
        if !validated.rendered_text()[..rendered_byte].ends_with(placeholder.as_str()) {
            return None;
        }
        rendered_byte.checked_sub(placeholder.len())?
    } else {
        rendered_byte
    };
    let rendered_capacity = validated
        .rendered_text()
        .len()
        .checked_sub(rendered_byte.checked_sub(prefix_end)?)?
        .checked_add(text.len())?;
    let mut rendered_text = String::new();
    rendered_text.try_reserve_exact(rendered_capacity).ok()?;
    rendered_text.push_str(&validated.rendered_text()[..prefix_end]);
    rendered_text.push_str(text);
    rendered_text.push_str(&validated.rendered_text()[rendered_byte..]);

    let top_level_count = context.document.root().child_count();
    let affected_start = validated.affected_top_level_index().saturating_sub(1);
    if affected_start >= top_level_count {
        return None;
    }
    let affected_len = top_level_count.checked_sub(affected_start)?;
    let mut affected_top_level_blocks = Vec::new();
    affected_top_level_blocks
        .try_reserve_exact(affected_len)
        .ok()?;
    affected_top_level_blocks.extend(affected_start..top_level_count);

    Some(LocalizedSemanticCompilation {
        position,
        preview,
        step_map,
        derivations: LocalizedSemanticDerivations {
            affected_top_level_blocks,
            rendered_text,
            rendered_scalars: validated.next_rendered_scalars(),
            document_text_bytes: validated.next_raw_text_utf8_bytes(),
            document_node_count: validated
                .document_node_count()
                .checked_add(usize::from(validated.creates_leaf()))?,
            raw_text_scalars: validated.next_raw_text_scalars(),
            raw_text_utf8_bytes: validated.next_raw_text_utf8_bytes(),
        },
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn derive_preview_document(
    request_id: u64,
    context: CompilationContext<'_>,
    base_position_map: &PositionMap,
    preview: &Document,
    composed_map: &StepMap,
    position_update_mode: UpdateMode,
    affected_top_level_blocks: &[usize],
) -> OperationResult<CompiledDocumentDerivations> {
    yrs_engine::derived_state::record_preview_position_map_derivation();
    #[cfg(test)]
    yrs_engine::observability::record_position_map_clone();
    let mut position_map = base_position_map.clone();
    let update_mode = if affected_top_level_blocks.is_empty() && preview != context.document {
        UpdateMode::Rebuild
    } else {
        position_update_mode
    };
    position_map.update(
        composed_map,
        context.document,
        preview,
        update_mode,
        context.schema,
    );
    #[cfg(test)]
    yrs_engine::observability::record_position_map_compaction();
    position_map.compact();
    yrs_engine::derived_state::record_preview_rendered_text_derivation();
    let rendered_text = crate::render::rendered_text(preview, context.schema);
    let rendered_scalars = u32::try_from(rendered_text.chars().count()).map_err(|_| {
        OperationError::engine_invariant_failed(
            request_id,
            None,
            "preview rendered scalar count exceeds the position domain",
        )
    })?;
    if rendered_scalars != position_map.total_scalars() {
        return Err(OperationError::engine_invariant_failed(
            request_id,
            None,
            "preview rendered text and position map have different scalar lengths",
        ));
    }
    let document_text_bytes = document_text_bytes(preview).ok_or_else(|| {
        OperationError::engine_invariant_failed(
            request_id,
            None,
            "preview document text byte metric overflowed",
        )
    })?;
    #[cfg(test)]
    yrs_engine::observability::record_document_node_count_scan();
    let document_node_count = crate::editor_state::document_node_count(preview.root());
    Ok(CompiledDocumentDerivations {
        identity_seal: Arc::new(()),
        position_map,
        rendered_text,
        rendered_scalars,
        document_text_bytes,
        document_node_count,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn derive_localized_preview_document(
    request_id: u64,
    context: CompilationContext<'_>,
    base_position_map: &PositionMap,
    preview: &Document,
    composed_map: &StepMap,
    position_update_mode: UpdateMode,
    affected_top_level_blocks: &[usize],
    localized: LocalizedSemanticDerivations,
) -> OperationResult<CompiledDocumentDerivations> {
    yrs_engine::derived_state::record_preview_position_map_derivation();
    #[cfg(test)]
    yrs_engine::observability::record_position_map_clone();
    let mut position_map = base_position_map.clone();
    let update_mode = if affected_top_level_blocks.is_empty() && preview != context.document {
        UpdateMode::Rebuild
    } else {
        position_update_mode
    };
    position_map.update(
        composed_map,
        context.document,
        preview,
        update_mode,
        context.schema,
    );
    #[cfg(test)]
    yrs_engine::observability::record_position_map_compaction();
    position_map.compact();
    if localized.rendered_scalars != position_map.total_scalars() {
        return Err(OperationError::engine_invariant_failed(
            request_id,
            None,
            "localized preview rendered text and position map have different scalar lengths",
        ));
    }
    Ok(CompiledDocumentDerivations {
        identity_seal: Arc::new(()),
        position_map,
        rendered_text: localized.rendered_text,
        rendered_scalars: localized.rendered_scalars,
        document_text_bytes: localized.document_text_bytes,
        document_node_count: localized.document_node_count,
    })
}

pub(super) fn affected_top_level_blocks(before: &Document, after: &Document) -> Vec<usize> {
    #[cfg(test)]
    yrs_engine::observability::record_affected_top_level_scan();
    if before == after {
        return Vec::new();
    }
    let before_children = before
        .root()
        .content()
        .map(|content| content.children())
        .unwrap_or(&[]);
    let after_children = after
        .root()
        .content()
        .map(|content| content.children())
        .unwrap_or(&[]);
    let mut prefix = 0usize;
    while prefix < before_children.len()
        && prefix < after_children.len()
        && before_children[prefix] == after_children[prefix]
    {
        prefix += 1;
    }
    let start = prefix.saturating_sub(1);
    let end = before_children.len().max(after_children.len());
    (start..end).collect()
}
