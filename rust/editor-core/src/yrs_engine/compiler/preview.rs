use crate::model::Document;
use crate::position::update::UpdateMode;
use crate::position::PositionMap;
use crate::render::RENDERED_BLOCK_SEPARATOR;
use crate::transform::{DocumentValidator, Step, StepMap};
use crate::yrs_engine;
use crate::yrs_engine::canonical::{CanonicalArtifact, CanonicalSchemaContext};
use crate::yrs_engine::compiler::admission::PreparedSemanticAdmission;
#[cfg(test)]
use crate::yrs_engine::compiler::observability::FORCE_LOCALIZED_SEMANTIC_ALLOCATION_FAILURE;
use crate::yrs_engine::compiler::{
    document_text_bytes, node_text_bytes, CachedCompilationView, CompilationContext,
    CompiledDocumentDerivations, PreparedSemanticContext,
};
use crate::yrs_engine::derived_state::ValidatedLocalizedTextblockEditAdmission;
use crate::yrs_engine::editing_limits::CheckedWork;
use crate::yrs_engine::{
    scalar_offset_to_byte, OperationError, OperationResult, TypedOperation, TypedTransaction,
};
use std::sync::Arc;

pub(super) struct LocalizedSemanticCompilation {
    pub(super) steps: std::collections::VecDeque<LocalizedSemanticStep>,
}

pub(super) struct LocalizedSemanticStep {
    pub(super) position: u32,
    pub(super) preview: Document,
    pub(super) step_map: StepMap,
    pub(super) derivations: LocalizedSemanticDerivations,
}

pub(super) struct LocalizedSemanticDerivations {
    pub(super) affected_top_level_blocks: Vec<usize>,
    pub(super) rendered_text: Arc<str>,
    pub(super) rendered_scalars: u32,
    pub(super) document_text_bytes: usize,
    pub(super) document_node_count: usize,
    pub(super) canonical_artifact: CanonicalArtifact,
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
    let certified = context.validation_certificate.is_some_and(|certificate| {
        certificate.certifies_root_window(
            context.document,
            preview,
            context.schema,
            context.resource_limits,
        )
    });
    if !certified {
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
    }
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

pub(super) fn try_localized_semantic_compilation(
    context: CompilationContext<'_>,
    transaction: &TypedTransaction,
    validated: &ValidatedLocalizedTextblockEditAdmission<'_>,
) -> Option<LocalizedSemanticCompilation> {
    #[cfg(test)]
    if FORCE_LOCALIZED_SEMANTIC_ALLOCATION_FAILURE.get() {
        return None;
    }
    let position = validated.document_position();
    let old_block = context.document.node_at(validated.block_path())?;
    let old_text = old_block.text_content();
    let original_start_scalar = validated.rendered_scalar_position();
    let rendered_byte = scalar_offset_to_byte(validated.rendered_text(), original_start_scalar)?;
    let placeholder = crate::render::empty_text_block_placeholder_string();
    let placeholder_scalars = u32::try_from(placeholder.chars().count()).ok()?;
    let prefix_end = if validated.creates_leaf() {
        if !validated.rendered_text()[..rendered_byte].ends_with(placeholder.as_str()) {
            return None;
        }
        rendered_byte.checked_sub(placeholder.len())?
    } else {
        rendered_byte
    };
    let suffix_start =
        scalar_offset_to_byte(validated.rendered_text(), validated.range_end_scalar())?;
    let removed_rendered_scalars = if validated.creates_leaf() {
        placeholder_scalars
    } else {
        validated
            .range_end_scalar()
            .checked_sub(original_start_scalar)?
    };
    let top_level_count = context.document.root().child_count();
    let affected_start = validated.affected_top_level_index().saturating_sub(1);
    if affected_start >= top_level_count {
        return None;
    }
    let mut steps = std::collections::VecDeque::new();
    steps.try_reserve(transaction.operations.len()).ok()?;
    let mut current_document = context.document.clone();
    let mut current_artifact = validated.canonical_artifact().clone();
    for operation in &transaction.operations {
        let (step, text) = match operation {
            TypedOperation::InsertText { text, marks, .. } => (
                Step::InsertText {
                    pos: position,
                    text: text.clone(),
                    marks: marks.clone(),
                },
                text.as_str(),
            ),
            TypedOperation::DeleteRange { .. } => (
                Step::DeleteRange {
                    from: position,
                    to: validated.range_end(),
                },
                "",
            ),
            TypedOperation::ReplaceRange { content, .. } => (
                Step::ReplaceRange {
                    from: position,
                    to: validated.range_end(),
                    content: content.clone(),
                },
                content.child(0)?.text_str()?,
            ),
            _ => return None,
        };
        let (preview, step_map) =
            crate::transform::apply_step(&current_document, &step, context.schema).ok()?;
        let block = preview.node_at(validated.block_path())?;
        let inserted = if block.child_count() == 0 {
            placeholder.as_str()
        } else {
            text
        };
        let rendered_capacity = validated
            .rendered_text()
            .len()
            .checked_sub(suffix_start.checked_sub(prefix_end)?)?
            .checked_add(inserted.len())?;
        let mut rendered_text = String::new();
        rendered_text.try_reserve_exact(rendered_capacity).ok()?;
        rendered_text.push_str(&validated.rendered_text()[..prefix_end]);
        rendered_text.push_str(inserted);
        rendered_text.push_str(&validated.rendered_text()[suffix_start..]);
        let block_text = block.text_content();
        let raw_text_utf8_bytes = validated
            .base_raw_text_utf8_bytes()
            .checked_sub(old_text.len())?
            .checked_add(block_text.len())?;
        let document_node_count = validated
            .base_document_node_count()
            .checked_sub(old_block.child_count())?
            .checked_add(block.child_count())?;
        let mut affected_top_level_blocks = Vec::new();
        affected_top_level_blocks
            .try_reserve_exact(top_level_count.checked_sub(affected_start)?)
            .ok()?;
        affected_top_level_blocks.extend(affected_start..top_level_count);
        current_artifact = CanonicalArtifact::derive_localized(
            &current_artifact,
            &preview,
            validated.block_path(),
        )?;
        current_document = preview.clone();
        steps.push_back(LocalizedSemanticStep {
            position,
            preview,
            step_map,
            derivations: LocalizedSemanticDerivations {
                affected_top_level_blocks,
                rendered_text: rendered_text.into(),
                rendered_scalars: validated
                    .base_rendered_scalars()
                    .checked_sub(removed_rendered_scalars)?
                    .checked_add(u32::try_from(inserted.chars().count()).ok()?)?,
                document_text_bytes: raw_text_utf8_bytes,
                document_node_count,
                canonical_artifact: current_artifact.clone(),
            },
        });
    }
    Some(LocalizedSemanticCompilation { steps })
}

struct PreviewTextMetrics {
    rendered_text: String,
    rendered_scalars: u32,
    document_text_bytes: usize,
    document_node_count: usize,
}

fn root_window_preview_text_metrics(
    schema: &crate::schema::Schema,
    base: CachedCompilationView<'_>,
    preview: &Document,
) -> Option<PreviewTextMetrics> {
    let window =
        yrs_engine::derived_state::RootChildWindow::between(base.document.root(), preview.root())?;
    let old_window = base
        .document
        .root()
        .content()?
        .children()
        .get(window.from()..window.old_to())?;
    let new_window = preview
        .root()
        .content()?
        .children()
        .get(window.from()..window.new_to())?;
    let map = base.position_map;
    let blocks = map.block_range_for_root_children(
        u32::try_from(window.from()).ok()?..u32::try_from(window.old_to()).ok()?,
    );
    let window_text = crate::render::rendered_root_children_text(
        preview,
        schema,
        window.from()..window.new_to(),
    )?;
    let prefix_scalars = match blocks.start.checked_sub(1) {
        Some(last_before) => {
            let block = map.block(last_before)?;
            Some(
                map.effective_scalar_start(last_before)
                    .checked_add(block.scalar_prefix_len)?
                    .checked_add(block.scalar_len)?,
            )
        }
        None => None,
    };
    let suffix_start =
        (blocks.end < map.block_count()).then(|| map.effective_scalar_start(blocks.end));
    let prefix_byte = prefix_scalars
        .map(|scalars| scalar_offset_to_byte(base.rendered_text, scalars))
        .unwrap_or(Some(0))?;
    let suffix_byte = match suffix_start {
        Some(scalars) => prefix_byte.checked_add(scalar_offset_to_byte(
            base.rendered_text.get(prefix_byte..)?,
            scalars.checked_sub(prefix_scalars.unwrap_or(0))?,
        )?)?,
        None => base.rendered_text.len(),
    };
    let segments = [
        prefix_scalars.map(|_| base.rendered_text.get(..prefix_byte)),
        window_text.as_deref().map(Some),
        suffix_start.map(|_| base.rendered_text.get(suffix_byte..)),
    ]
    .into_iter()
    .flatten()
    .collect::<Option<Vec<&str>>>()?;
    let mut rendered_text = String::new();
    rendered_text
        .try_reserve_exact(
            segments
                .iter()
                .try_fold(segments.len().saturating_sub(1), |total, segment| {
                    total.checked_add(segment.len())
                })?,
        )
        .ok()?;
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            rendered_text.push(RENDERED_BLOCK_SEPARATOR);
        }
        rendered_text.push_str(segment);
    }
    let window_text_bytes = |nodes: &[crate::model::Node]| {
        nodes.iter().try_fold(0usize, |total, node| {
            total.checked_add(node_text_bytes(node)?)
        })
    };
    let window_node_count = |nodes: &[crate::model::Node]| {
        nodes.iter().try_fold(0usize, |total, node| {
            total.checked_add(crate::editor_state::document_node_count(node))
        })
    };
    Some(PreviewTextMetrics {
        rendered_scalars: u32::try_from(segments.len().saturating_sub(1))
            .ok()?
            .checked_add(prefix_scalars.unwrap_or(0))?
            .checked_add(
                u32::try_from(
                    window_text
                        .as_deref()
                        .map_or(0, |text| text.chars().count()),
                )
                .ok()?,
            )?
            .checked_add(
                suffix_start
                    .map(|start| base.rendered_scalars.checked_sub(start))
                    .unwrap_or(Some(0))?,
            )?,
        rendered_text,
        document_text_bytes: base
            .document_text_bytes
            .checked_sub(window_text_bytes(old_window)?)?
            .checked_add(window_text_bytes(new_window)?)?,
        document_node_count: base
            .document_node_count
            .checked_sub(window_node_count(old_window)?)?
            .checked_add(window_node_count(new_window)?)?,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn derive_preview_document(
    request_id: u64,
    context: CompilationContext<'_>,
    base_position_map: &PositionMap,
    cached_view: Option<CachedCompilationView<'_>>,
    preview: &Document,
    composed_map: &StepMap,
    position_update_mode: UpdateMode,
    affected_top_level_blocks: &[usize],
) -> OperationResult<CompiledDocumentDerivations> {
    yrs_engine::derived_state::record_preview_position_map_derivation();
    #[cfg(test)]
    yrs_engine::observability::record_position_map_clone();
    let update_mode = if affected_top_level_blocks.is_empty() && preview != context.document {
        UpdateMode::Rebuild
    } else {
        position_update_mode
    };
    let position_map = base_position_map.clone_updated_and_compacted(
        composed_map,
        context.document,
        preview,
        update_mode,
        context.schema,
    );
    #[cfg(test)]
    yrs_engine::observability::record_position_map_compaction();
    yrs_engine::derived_state::record_preview_rendered_text_derivation();
    let PreviewTextMetrics {
        rendered_text,
        rendered_scalars,
        document_text_bytes,
        document_node_count,
    } = match cached_view
        .and_then(|base| root_window_preview_text_metrics(context.schema, base, preview))
    {
        Some(spliced) => spliced,
        None => {
            let rendered_text = crate::render::rendered_text(preview, context.schema);
            let rendered_scalars = u32::try_from(rendered_text.chars().count()).map_err(|_| {
                OperationError::engine_invariant_failed(
                    request_id,
                    None,
                    "preview rendered scalar count exceeds the position domain",
                )
            })?;
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
            PreviewTextMetrics {
                rendered_text,
                rendered_scalars,
                document_text_bytes,
                document_node_count,
            }
        }
    };
    if rendered_scalars != position_map.total_scalars() {
        return Err(OperationError::engine_invariant_failed(
            request_id,
            None,
            "preview rendered text and position map have different scalar lengths",
        ));
    }
    Ok(CompiledDocumentDerivations {
        identity_seal: Arc::new(()),
        position_map,
        rendered_text: rendered_text.into(),
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
    let update_mode = if affected_top_level_blocks.is_empty() && preview != context.document {
        UpdateMode::Rebuild
    } else {
        position_update_mode
    };
    let position_map = base_position_map.clone_updated_and_compacted(
        composed_map,
        context.document,
        preview,
        update_mode,
        context.schema,
    );
    #[cfg(test)]
    yrs_engine::observability::record_position_map_compaction();
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
