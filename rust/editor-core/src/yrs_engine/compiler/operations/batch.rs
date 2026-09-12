use crate::selection::Selection;
use crate::transform::Step;
use crate::yrs_engine::compiler::input_limits::{
    charge_undo_bound, checked_attrs_input_bytes, validate_fragment_marks, validate_operation_marks,
};
use crate::yrs_engine::compiler::operations::{OperationCompiler, OperationOutcome};
use crate::yrs_engine::compiler::positions::{
    resolve_child_window, resolve_content_offset, ChildWindowTarget,
};
use crate::yrs_engine::compiler::preview::validate_preview;
use crate::yrs_engine::compiler::text_boundaries::text_boundaries;
use crate::yrs_engine::compiler::{map_transform_error, merge_history_class, HistoryClass};
use crate::yrs_engine::mutation::{MutationDocumentContext, ReplacementInput};
use crate::yrs_engine::{OperationError, OperationResult, StructuralEdit, StructuralEditBatch};
use std::borrow::Cow;

const BATCH_FIELD: &str = "structure";
const PATCH_RANK: u8 = 0;
const CONTENT_TEXT_RANK: u8 = 1;
const SPLICE_RANK: u8 = 2;
const SINGLE_CHILD: u32 = 1;

struct PlannedEdit {
    edit_index: usize,
    from: u32,
    rank: u8,
}

fn batch_invalid(request_id: u64, operation_index: usize, message: &'static str) -> OperationError {
    OperationError::operation_invalid(request_id, operation_index, BATCH_FIELD, message)
}

fn parent_of(path: &[u32]) -> Option<(&[u32], u32)> {
    path.split_last().map(|(child, parent)| (parent, *child))
}

fn window_kills(parent_path: &[u32], from_child: u32, to_child: u32, target: &[u32]) -> bool {
    if from_child == to_child {
        return false;
    }
    let Some(index) = target.get(parent_path.len()).copied() else {
        return false;
    };
    target.starts_with(parent_path) && index >= from_child && index < to_child
}

impl OperationCompiler<'_> {
    fn plan_batch(
        &self,
        operation_index: usize,
        batch: &StructuralEditBatch,
    ) -> OperationResult<Vec<PlannedEdit>> {
        let request_id = self.request_id;
        let document = self.context.document;
        let limits = self.context.resource_limits;
        let mut planned = Vec::new();
        planned
            .try_reserve_exact(batch.edits().len())
            .map_err(|_| {
                batch_invalid(
                    request_id,
                    operation_index,
                    "sealed structural edit batch does not fit its allocation",
                )
            })?;
        for (edit_index, edit) in batch.edits().iter().enumerate() {
            let (from, rank) = match edit {
                StructuralEdit::SpliceChildren {
                    parent_path,
                    from_child,
                    to_child,
                    ..
                } => {
                    let (from, _) = resolve_child_window(
                        request_id,
                        operation_index,
                        document,
                        ChildWindowTarget {
                            parent_path,
                            from_child: *from_child,
                            to_child: *to_child,
                        },
                        limits,
                    )?;
                    (from, SPLICE_RANK)
                }
                StructuralEdit::PatchAttributes { path, .. } => {
                    let (parent_path, child) = parent_of(path).ok_or_else(|| {
                        batch_invalid(
                            request_id,
                            operation_index,
                            "an attribute patch cannot target the document root",
                        )
                    })?;
                    let to_child = child.checked_add(SINGLE_CHILD).ok_or_else(|| {
                        batch_invalid(
                            request_id,
                            operation_index,
                            "attribute patch child index overflowed",
                        )
                    })?;
                    let (from, _) = resolve_child_window(
                        request_id,
                        operation_index,
                        document,
                        ChildWindowTarget {
                            parent_path,
                            from_child: child,
                            to_child,
                        },
                        limits,
                    )?;
                    (from, PATCH_RANK)
                }
                StructuralEdit::InsertContentText {
                    parent_path,
                    parent_offset,
                    ..
                } => {
                    let position = resolve_content_offset(
                        request_id,
                        operation_index,
                        document,
                        parent_path,
                        *parent_offset,
                        limits,
                    )?;
                    (position, CONTENT_TEXT_RANK)
                }
            };
            planned.push(PlannedEdit {
                edit_index,
                from,
                rank,
            });
        }
        self.admit_batch_disjointness(operation_index, batch, &planned)?;
        planned.sort_by(|left, right| {
            right
                .from
                .cmp(&left.from)
                .then_with(|| left.rank.cmp(&right.rank))
        });
        Ok(planned)
    }

    fn admit_batch_disjointness(
        &self,
        operation_index: usize,
        batch: &StructuralEditBatch,
        planned: &[PlannedEdit],
    ) -> OperationResult<()> {
        let request_id = self.request_id;
        for (left_index, left) in batch.edits().iter().enumerate() {
            for (right_index, right) in batch.edits().iter().enumerate() {
                if left_index == right_index {
                    continue;
                }
                if let StructuralEdit::SpliceChildren {
                    parent_path,
                    from_child,
                    to_child,
                    ..
                } = left
                {
                    if window_kills(parent_path, *from_child, *to_child, right.target_path()) {
                        return Err(batch_invalid(
                            request_id,
                            operation_index,
                            "a sealed structural edit cannot survive under a replaced ancestor",
                        ));
                    }
                    if left_index < right_index && right.target_path() == parent_path.as_slice() {
                        match right {
                            StructuralEdit::SpliceChildren {
                                from_child: right_from,
                                to_child: right_to,
                                ..
                            } => {
                                if *to_child > *right_from && *right_to > *from_child {
                                    return Err(batch_invalid(
                                        request_id,
                                        operation_index,
                                        "a sealed structural edit batch cannot overlap two splices of one parent",
                                    ));
                                }
                            }
                            StructuralEdit::InsertContentText { .. } => {
                                return Err(batch_invalid(
                                    request_id,
                                    operation_index,
                                    "a sealed structural edit batch cannot splice and retext one parent",
                                ));
                            }
                            StructuralEdit::PatchAttributes { .. } => {}
                        }
                    }
                }
                if left_index < right_index
                    && matches!(left, StructuralEdit::PatchAttributes { .. })
                    && matches!(right, StructuralEdit::PatchAttributes { .. })
                    && left.target_path() == right.target_path()
                {
                    return Err(batch_invalid(
                        request_id,
                        operation_index,
                        "a sealed structural edit batch patches each node at most once",
                    ));
                }
            }
        }
        for (left_index, left) in planned.iter().enumerate() {
            for right in planned.iter().skip(left_index.saturating_add(1)) {
                if left.from == right.from && left.rank == right.rank {
                    return Err(batch_invalid(
                        request_id,
                        operation_index,
                        "a sealed structural edit batch cannot hold two edits at one coordinate",
                    ));
                }
            }
        }
        Ok(())
    }

    #[inline]
    pub(super) fn compile_edit_structure(
        self,
        operation_index: usize,
        batch: &StructuralEditBatch,
    ) -> OperationResult<(Self, OperationOutcome)> {
        let planned = self.plan_batch(operation_index, batch)?;
        let Self {
            context,
            transaction,
            prepared_semantics,
            localized_semantic,
            mut lowering,
            localized_insert,
            localized_format,
            localized_root_window,
            prelowered_plan,
            prelowered_lookup_transition,
            request_id,
            work,
            base_position_map,
            rendered_text,
            mut preview,
            mut composed_map,
            operation_result: _previous_operation_result,
            mut undo_units_bound,
            mut undo_limit_error,
            mut history_class,
            records_history,
            canonical_artifact,
            canonical_schema,
            stored_marks_state,
            split_at_caret_kept_stored_marks,
            tracked_caret,
            localized_derivations,
        } = self;
        if transaction.operations.len() != 1 {
            return Err(batch_invalid(
                request_id,
                operation_index,
                "a sealed structural edit batch must be the transaction's only operation",
            ));
        }
        if batch.edits().is_empty() {
            return Err(batch_invalid(
                request_id,
                operation_index,
                "a sealed structural edit batch must carry at least one edit",
            ));
        }
        if localized_insert.is_some()
            || localized_format.is_some()
            || localized_root_window.is_some()
        {
            return Err(OperationError::engine_invariant_failed(
                request_id,
                Some(operation_index),
                "a sealed structural edit batch cannot lower through a localized capability",
            ));
        }
        let mut operation_changed = false;
        for plan in planned {
            let edit = batch.edits().get(plan.edit_index).ok_or_else(|| {
                OperationError::engine_invariant_failed(
                    request_id,
                    Some(operation_index),
                    "sealed structural edit batch lost one of its planned edits",
                )
            })?;
            let next = match edit {
                StructuralEdit::SpliceChildren {
                    parent_path,
                    from_child,
                    to_child,
                    content,
                } => {
                    validate_fragment_marks(request_id, operation_index, content, context.schema)?;
                    let (from, to) = resolve_child_window(
                        request_id,
                        operation_index,
                        &preview,
                        ChildWindowTarget {
                            parent_path,
                            from_child: *from_child,
                            to_child: *to_child,
                        },
                        context.resource_limits,
                    )?;
                    if from != plan.from {
                        return Err(rebased_target(request_id, operation_index));
                    }
                    let step = Step::ReplaceRange {
                        from,
                        to,
                        content: content.clone(),
                    };
                    let (next, step_map) = crate::transform::apply_step_canonical_marks(
                        &preview,
                        &step,
                        context.schema,
                    )
                    .map_err(|error| {
                        map_transform_error(request_id, operation_index, BATCH_FIELD, error)
                    })?;
                    if lowering.is_some() {
                        validate_preview(request_id, Some(operation_index), &next, context)?;
                    }
                    if let Some(lowering) = (next != *preview).then_some(()).and(lowering.as_mut())
                    {
                        let boundaries = text_boundaries(
                            request_id,
                            operation_index,
                            &preview,
                            context.schema,
                            lowering,
                        )?;
                        lowering.replace_structural_range(
                            operation_index,
                            MutationDocumentContext {
                                before: &preview,
                                after: &next,
                                schema: context.schema,
                                limits: context.resource_limits,
                            },
                            ReplacementInput {
                                from,
                                to,
                                boundaries: &boundaries,
                                content,
                            },
                        )?;
                    }
                    if records_history {
                        charge_undo_bound(
                            &mut undo_units_bound,
                            &mut undo_limit_error,
                            u64::from(to - from).saturating_add(u64::from(content.size())),
                            request_id,
                            operation_index,
                            context.editing_limits.max_undo_retained_units,
                        );
                    }
                    composed_map = composed_map.compose(&step_map);
                    next
                }
                StructuralEdit::PatchAttributes { path, attrs } => {
                    let (parent_path, child) = parent_of(path).ok_or_else(|| {
                        batch_invalid(
                            request_id,
                            operation_index,
                            "an attribute patch cannot target the document root",
                        )
                    })?;
                    let to_child = child.checked_add(SINGLE_CHILD).ok_or_else(|| {
                        batch_invalid(
                            request_id,
                            operation_index,
                            "attribute patch child index overflowed",
                        )
                    })?;
                    let (from, to) = resolve_child_window(
                        request_id,
                        operation_index,
                        &preview,
                        ChildWindowTarget {
                            parent_path,
                            from_child: child,
                            to_child,
                        },
                        context.resource_limits,
                    )?;
                    if from != plan.from {
                        return Err(rebased_target(request_id, operation_index));
                    }
                    let step = Step::UpdateNodeAttrs {
                        pos: from,
                        attrs: attrs.clone(),
                    };
                    let (next, step_map) = crate::transform::apply_step_canonical_marks(
                        &preview,
                        &step,
                        context.schema,
                    )
                    .map_err(|error| {
                        map_transform_error(request_id, operation_index, BATCH_FIELD, error)
                    })?;
                    if lowering.is_some() {
                        validate_preview(request_id, Some(operation_index), &next, context)?;
                    }
                    if next != *preview {
                        if let Some(lowering) = &mut lowering {
                            lowering.update_node_attrs(
                                operation_index,
                                &preview,
                                from,
                                attrs,
                                context.schema,
                                context.resource_limits,
                            )?;
                        }
                    }
                    if records_history {
                        charge_undo_bound(
                            &mut undo_units_bound,
                            &mut undo_limit_error,
                            u64::try_from(checked_attrs_input_bytes(
                                request_id,
                                operation_index,
                                attrs,
                                context.resource_limits,
                                0,
                            )?)
                            .unwrap_or(u64::MAX),
                            request_id,
                            operation_index,
                            context.editing_limits.max_undo_retained_units,
                        );
                    }
                    composed_map = composed_map.compose(&step_map);
                    next
                }
                StructuralEdit::InsertContentText {
                    parent_path,
                    parent_offset,
                    text,
                    marks,
                } => {
                    validate_operation_marks(request_id, operation_index, marks, context.schema)?;
                    if text.is_empty() {
                        return Err(batch_invalid(
                            request_id,
                            operation_index,
                            "a sealed content text edit must not be empty",
                        ));
                    }
                    let position = resolve_content_offset(
                        request_id,
                        operation_index,
                        &preview,
                        parent_path,
                        *parent_offset,
                        context.resource_limits,
                    )?;
                    if position != plan.from {
                        return Err(rebased_target(request_id, operation_index));
                    }
                    let step = Step::InsertText {
                        pos: position,
                        text: text.clone(),
                        marks: marks.clone(),
                    };
                    let (next, step_map) = crate::transform::apply_step_canonical_marks(
                        &preview,
                        &step,
                        context.schema,
                    )
                    .map_err(|error| {
                        map_transform_error(request_id, operation_index, BATCH_FIELD, error)
                    })?;
                    if lowering.is_some() {
                        validate_preview(request_id, Some(operation_index), &next, context)?;
                    }
                    if let Some(lowering) = &mut lowering {
                        lowering.insert(operation_index, position, text, marks)?;
                    }
                    if records_history {
                        charge_undo_bound(
                            &mut undo_units_bound,
                            &mut undo_limit_error,
                            text.chars().count() as u64,
                            request_id,
                            operation_index,
                            context.editing_limits.max_undo_retained_units,
                        );
                    }
                    composed_map = composed_map.compose(&step_map);
                    next
                }
            };
            operation_changed = operation_changed || next != *preview;
            preview = Cow::Owned(next);
        }
        if operation_changed {
            history_class = merge_history_class(history_class, HistoryClass::Structural);
        }
        let operation_result: Option<Selection> = Some(batch.selection_after().clone());
        Ok((
            Self {
                context,
                transaction,
                prepared_semantics,
                localized_semantic,
                lowering,
                localized_insert,
                localized_format,
                localized_root_window,
                prelowered_plan,
                prelowered_lookup_transition,
                request_id,
                work,
                base_position_map,
                rendered_text,
                preview,
                composed_map,
                operation_result,
                undo_units_bound,
                undo_limit_error,
                history_class,
                records_history,
                canonical_artifact,
                canonical_schema,
                stored_marks_state,
                split_at_caret_kept_stored_marks,
                tracked_caret,
                localized_derivations,
            },
            OperationOutcome {
                stored_marks_input: None,
                inherited_marks: None,
                operation_changed,
                compatible_text_delete: false,
            },
        ))
    }
}

fn rebased_target(request_id: u64, operation_index: usize) -> OperationError {
    OperationError::engine_invariant_failed(
        request_id,
        Some(operation_index),
        "a sealed structural edit resolved away from its base document coordinates",
    )
}
