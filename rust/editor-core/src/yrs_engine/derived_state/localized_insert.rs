use super::insert_admission::{
    LocalizedTextblockEdit, LocalizedTextblockEditAdmission,
    LocalizedTextblockEditAdmissionRequest, LocalizedTextblockEditPlan,
};
use super::localized_index::{
    canonical_marks_sha256, node_path_sha256, LocalizedTextLeafCertificate,
    LEAVES_MEETING_AT_A_POSITION,
};
#[cfg(test)]
use super::observability::LOCALIZED_INSERT_ADMISSION_WORK;
use super::DerivedStateCache;
use crate::boundary::ResourceLimits;
#[cfg(test)]
use crate::model::Mark;
use crate::model::{Fragment, Node};
use crate::schema::{NodeRole, Schema};
use crate::transform::Step;
use crate::transform::{DocumentValidator, DOCUMENT_ROOT_DEPTH};
use crate::yrs_engine;
use crate::yrs_engine::{scalar_offset_to_utf16, ResolvedPoint, ResolvedSelection};
use sha2::Digest;
use std::sync::Arc;
use yrs::types::xml::XmlFragmentRef;
use yrs::ReadTxn;

struct TextblockEditTarget {
    leaf: LocalizedTextLeafCertificate,
    scalar_at: u32,
    utf16_at: u32,
    creates_leaf: bool,
    canonical_growth_bytes: isize,
    rendered_scalar_delta: i32,
    rendered_utf16_delta: i32,
    removed_scalars: u32,
    removed_utf8_bytes: usize,
    removed_utf16: u32,
    empty_after: bool,
}

impl DerivedStateCache {
    pub(crate) fn textblock_edit_for_transaction<'a>(
        &'a self,
        transaction: &'a yrs_engine::TypedTransaction,
    ) -> Option<(yrs_engine::RevisionedPosition, LocalizedTextblockEdit<'a>)> {
        let (at, end, text, marks) = match transaction.operations.as_slice() {
            [yrs_engine::TypedOperation::InsertText { at, text, marks }] => {
                (*at, *at, text.as_str(), marks.as_slice())
            }
            [yrs_engine::TypedOperation::DeleteRange { range }, yrs_engine::TypedOperation::InsertText { at, text, marks }]
                if *at == range.from =>
            {
                (range.from, range.to, text.as_str(), marks.as_slice())
            }
            [yrs_engine::TypedOperation::DeleteRange { range }] => {
                (range.from, range.to, "", &[][..])
            }
            [yrs_engine::TypedOperation::ReplaceRange { range, content }] => {
                let node = content.child(0)?;
                if content.child_count() != 1 {
                    return None;
                }
                (range.from, range.to, node.text_str()?, node.marks())
            }
            _ => return None,
        };
        let resolve = |point: yrs_engine::RevisionedPosition| {
            yrs_engine::editor_offset_to_doc_pos(
                point.offset,
                point.kind,
                &self.rendered_text,
                &self.position_map,
                &self.document,
            )
        };
        let from = resolve(at)?;
        let to = resolve(end)?;
        if from > to {
            return None;
        }
        Some((
            at,
            LocalizedTextblockEdit {
                block_path: self.localized_textblock_path(from)?,
                replaced: from..to,
                text,
                marks,
            },
        ))
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn localized_insert_admission_for_test(
        &self,
        document_position: u32,
        text: &str,
        marks: &[Mark],
        schema: &Schema,
        resource_limits: &ResourceLimits,
        max_length: Option<u32>,
        yrs_state_epoch: u64,
    ) -> Option<LocalizedTextblockEditAdmission> {
        let schema_fingerprint = crate::schema::schema_fingerprint(schema);
        self.build_localized_textblock_edit_admission(
            LocalizedTextblockEditAdmissionRequest {
                request_id: 0,
                base_document_revision: self.document_revision,
                origin: yrs_engine::TransactionOrigin::LocalInput,
                inserted_at: yrs_engine::RevisionedPosition {
                    offset: document_position,
                    kind: yrs_engine::EditorOffsetKind::Scalar,
                    affinity: yrs_engine::Affinity::After,
                },
                edit: LocalizedTextblockEdit {
                    block_path: self.localized_textblock_path(document_position)?,
                    replaced: document_position..document_position,
                    text,
                    marks,
                },
                selection_intent: yrs_engine::SelectionIntent::UseOperationResult,
                history_policy: yrs_engine::HistoryPolicy::Auto,
            },
            schema,
            &schema_fingerprint,
            resource_limits,
            &crate::yrs_engine::EditingLimits::default(),
            max_length,
            yrs_state_epoch,
            &self.mutation_lookup_seed,
            None,
        )
    }

    pub(crate) fn localized_textblock_path(&self, document_position: u32) -> Option<&[u32]> {
        let block_index = self.textblock_index_at(document_position)?;
        Some(self.position_map.block(block_index)?.node_path.as_slice())
    }

    pub(crate) fn admits_localized_textblock_edit(
        &self,
        edit: &LocalizedTextblockEdit<'_>,
        schema: &Schema,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
    ) -> bool {
        self.plan_localized_textblock_edit(
            edit,
            schema,
            editing_limits,
            max_length,
            self.validation_certificate.canonical_serialized_len,
        )
        .is_some()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit_textblock_edit_with_authority<T: ReadTxn>(
        &self,
        transaction: &yrs_engine::TypedTransaction,
        document_position: u32,
        txn: &T,
        fragment: &XmlFragmentRef,
        lookup_seed: &Arc<yrs_engine::mutation::MutationLookupSeed>,
        identity: Option<&yrs_engine::prepared_admission::MaterializedMutationIdentity>,
        schema: &Schema,
        schema_fingerprint: &str,
        resource_limits: &ResourceLimits,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
        yrs_state_epoch: u64,
    ) -> Option<LocalizedTextblockEditAdmission> {
        if transaction.base_document_revision != self.document_revision
            || transaction.selection_intent != yrs_engine::SelectionIntent::UseOperationResult
            || !matches!(
                transaction.history_policy,
                yrs_engine::HistoryPolicy::Auto | yrs_engine::HistoryPolicy::Boundary
            )
            || !matches!(
                transaction.origin,
                yrs_engine::TransactionOrigin::LocalInput
                    | yrs_engine::TransactionOrigin::LocalCommand
                    | yrs_engine::TransactionOrigin::LocalApi
            )
            || !self
                .render_blocks
                .matches_identity(&self.document, &self.schema_fingerprint)
            || !lookup_seed.matches(
                txn,
                fragment,
                &self.document,
                resource_limits,
                editing_limits,
                max_length,
                &self.schema_fingerprint,
                yrs_state_epoch,
                self.document_revision,
            )
        {
            return None;
        }
        let (at, edit) = self.textblock_edit_for_transaction(transaction)?;
        if edit.replaced.start != document_position {
            return None;
        }
        self.build_localized_textblock_edit_admission(
            LocalizedTextblockEditAdmissionRequest {
                request_id: transaction.request_id,
                base_document_revision: transaction.base_document_revision,
                origin: transaction.origin,
                inserted_at: at,
                edit,
                selection_intent: transaction.selection_intent.clone(),
                history_policy: transaction.history_policy,
            },
            schema,
            schema_fingerprint,
            resource_limits,
            editing_limits,
            max_length,
            yrs_state_epoch,
            lookup_seed,
            identity,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_localized_textblock_edit_admission(
        &self,
        request: LocalizedTextblockEditAdmissionRequest<'_>,
        schema: &Schema,
        schema_fingerprint: &str,
        resource_limits: &ResourceLimits,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
        yrs_state_epoch: u64,
        lookup_seed: &Arc<yrs_engine::mutation::MutationLookupSeed>,
        identity: Option<&yrs_engine::prepared_admission::MaterializedMutationIdentity>,
    ) -> Option<LocalizedTextblockEditAdmission> {
        #[cfg(test)]
        LOCALIZED_INSERT_ADMISSION_WORK
            .set(LOCALIZED_INSERT_ADMISSION_WORK.get().saturating_add(1));
        let LocalizedTextblockEditAdmissionRequest {
            request_id,
            base_document_revision,
            origin,
            inserted_at,
            edit,
            selection_intent,
            history_policy,
        } = request;
        let identity_matches = identity.is_none_or(|identity| {
            self.matches_materialized_mutation_identity(
                &self.canonical_artifact,
                identity.canonical_fingerprint,
                identity.canonical_serialized_len,
                resource_limits,
                &self.schema_fingerprint,
                self.document_revision,
                self.state_revision,
                yrs_state_epoch,
            )
        });
        if schema_fingerprint != self.schema_fingerprint
            || !identity_matches
            || (identity.is_none()
                && !self.validation_certificate.matches(
                    &self.canonical_artifact,
                    resource_limits,
                    &self.schema_fingerprint,
                    self.document_revision,
                    self.state_revision,
                    yrs_state_epoch,
                ))
            || (identity.is_none()
                && !self
                    .localized_text_index
                    .as_ref()?
                    .matches(&self.validation_certificate))
        {
            return None;
        }
        let canonical_serialized_len = identity.map_or(
            self.validation_certificate.canonical_serialized_len,
            |identity| identity.canonical_serialized_len,
        );
        let canonical_fingerprint = identity.map_or_else(
            || self.validation_certificate.canonical_fingerprint,
            |identity| identity.canonical_fingerprint,
        );
        let plan = self.plan_localized_textblock_edit(
            &edit,
            schema,
            editing_limits,
            max_length,
            canonical_serialized_len,
        )?;
        Some(LocalizedTextblockEditAdmission {
            plan,
            document_revision: self.document_revision,
            state_revision: self.state_revision,
            yrs_state_epoch,
            selection: self.resolved_selection.clone(),
            relative_selection: self.relative_selection.clone(),
            stored_marks_sha256: match self.stored_marks.as_deref() {
                Some(stored_marks) => Some(canonical_marks_sha256(stored_marks)?),
                None => None,
            },
            canonical_fingerprint,
            validation_certificate: self.validation_certificate.clone(),
            request_id,
            base_document_revision,
            origin,
            inserted_at,
            inserted_document_position: edit.replaced.start,
            inserted_text_sha256: sha2::Sha256::digest(edit.text.as_bytes()).into(),
            inserted_marks_sha256: canonical_marks_sha256(edit.marks)?,
            selection_intent,
            history_policy,
            max_length,
            max_operations_per_transaction: editing_limits.max_operations_per_transaction,
            max_undo_groups: editing_limits.max_undo_groups,
            max_derived_output_bytes: editing_limits.max_derived_output_bytes,
            max_undo_retained_units: editing_limits.max_undo_retained_units,
            render_seal: Arc::clone(&self.render_blocks),
            lookup_seal: Arc::clone(lookup_seed),
        })
    }

    pub(super) fn plan_localized_textblock_edit(
        &self,
        edit: &LocalizedTextblockEdit<'_>,
        schema: &Schema,
        editing_limits: &yrs_engine::EditingLimits,
        max_length: Option<u32>,
        canonical_serialized_len: usize,
    ) -> Option<LocalizedTextblockEditPlan> {
        let document_position = edit.replaced.start;
        if edit.text.is_empty() && edit.replaced.is_empty() {
            return None;
        }
        let block_index = self.textblock_index_at(document_position)?;
        let block = self.position_map.block(block_index)?;
        if block.node_path.as_slice() != edit.block_path {
            return None;
        }
        crate::tables::mutation_guard::admit_textblock_ancestry(
            &self.document,
            schema,
            edit.block_path,
        )
        .ok()?;
        let inserted_scalars = u32::try_from(edit.text.chars().count()).ok()?;
        let inserted_utf16 = u32::try_from(edit.text.encode_utf16().count()).ok()?;
        let escaped_limit = editing_limits.max_derived_output_bytes;
        let target = self.textblock_edit_target(
            block_index,
            edit,
            schema,
            inserted_scalars,
            inserted_utf16,
            escaped_limit,
        )?;
        let next_raw_text_scalars = self
            .validation_certificate
            .raw_text_scalars
            .checked_sub(u64::from(target.removed_scalars))?
            .checked_add(u64::from(inserted_scalars))?;
        if max_length.is_some_and(|limit| next_raw_text_scalars > u64::from(limit)) {
            return None;
        }
        let next_raw_text_utf8_bytes = self
            .validation_certificate
            .raw_text_utf8_bytes
            .checked_sub(target.removed_utf8_bytes)?
            .checked_add(edit.text.len())?;
        let next_canonical_serialized_len =
            canonical_serialized_len.checked_add_signed(target.canonical_growth_bytes)?;
        if next_canonical_serialized_len > editing_limits.max_derived_output_bytes {
            return None;
        }
        let history_undo_units =
            u64::from(inserted_utf16).checked_add(u64::from(target.removed_utf16))?;
        if history_undo_units > editing_limits.max_undo_retained_units {
            return None;
        }
        let next_point = ResolvedPoint {
            document: document_position.checked_add(inserted_scalars)?,
            scalar: target
                .scalar_at
                .checked_add(inserted_scalars)?
                .checked_add(u32::from(target.empty_after))?
                .checked_sub(u32::from(target.creates_leaf))?,
            utf16: target
                .utf16_at
                .checked_add(inserted_utf16)?
                .checked_add(u32::from(target.empty_after))?
                .checked_sub(u32::from(target.creates_leaf))?,
        };
        Some(LocalizedTextblockEditPlan {
            leaf: target.leaf,
            creates_leaf: target.creates_leaf,
            removed_scalars: target.removed_scalars,
            range_end: edit.replaced.end,
            block_path_len: edit.block_path.len(),
            block_path_sha256: node_path_sha256(edit.block_path),
            affected_top_level_index: usize::try_from(*edit.block_path.first()?).ok()?,
            inserted_scalars,
            inserted_utf8_bytes: edit.text.len(),
            inserted_utf16,
            canonical_growth_bytes: target.canonical_growth_bytes,
            rendered_scalar_delta: target.rendered_scalar_delta,
            rendered_utf16_delta: target.rendered_utf16_delta,
            next_raw_text_scalars,
            next_raw_text_utf8_bytes,
            next_canonical_serialized_len,
            next_rendered_scalars: self
                .rendered_scalars
                .checked_add_signed(target.rendered_scalar_delta)?,
            operation_result: ResolvedSelection::Text {
                anchor: next_point,
                head: next_point,
            },
            history_undo_units,
        })
    }

    fn textblock_index_at(&self, document_position: u32) -> Option<usize> {
        if !self.position_map.has_effective_stored_bounds() {
            return None;
        }
        let blocks = self.position_map.blocks();
        let block_index = blocks
            .partition_point(|block| block.doc_start <= document_position)
            .checked_sub(1)?;
        let block = blocks.get(block_index)?;
        (!block.is_void_block && document_position <= block.doc_end).then_some(block_index)
    }

    fn textblock_edit_target(
        &self,
        block_index: usize,
        edit: &LocalizedTextblockEdit<'_>,
        schema: &Schema,
        inserted_scalars: u32,
        inserted_utf16: u32,
        escaped_limit: usize,
    ) -> Option<TextblockEditTarget> {
        let document_position = edit.replaced.start;
        let index = self.localized_text_index.as_ref()?;
        let marks_sha256 = canonical_marks_sha256(edit.marks)?;
        let scalar_at = self
            .position_map
            .doc_to_scalar(document_position, &self.document);
        let utf16_at = scalar_offset_to_utf16(&self.rendered_text, scalar_at)?;
        if !edit.replaced.is_empty() {
            return self.textblock_range_target(
                block_index,
                edit,
                schema,
                scalar_at,
                utf16_at,
                inserted_scalars,
                inserted_utf16,
            );
        }
        if let Some(leaf_index) = index.joined_leaf(block_index, document_position, marks_sha256) {
            let leaf = *index.leaves().get(leaf_index)?;
            let live_leaf = leaf.resolve(&self.document, &self.position_map)?;
            let live_text = live_leaf.text_str()?;
            let live_matches = <[u8; 32]>::from(sha2::Sha256::digest(live_text.as_bytes()))
                == leaf.text_sha256
                && live_leaf.marks() == edit.marks
                && live_leaf.node_size() == leaf.text_scalars
                && u32::try_from(live_text.encode_utf16().count()).ok()? == leaf.text_utf16
                && live_text.len() == leaf.text_utf8_bytes;
            return live_matches.then_some(TextblockEditTarget {
                leaf,
                scalar_at,
                utf16_at,
                creates_leaf: false,
                canonical_growth_bytes: isize::try_from(checked_json_string_body_len(
                    edit.text,
                    escaped_limit,
                )?)
                .ok()?,
                rendered_scalar_delta: i32::try_from(inserted_scalars).ok()?,
                rendered_utf16_delta: i32::try_from(inserted_utf16).ok()?,
                removed_scalars: 0,
                removed_utf8_bytes: 0,
                removed_utf16: 0,
                empty_after: false,
            });
        }
        let block = self.position_map.block(block_index)?;
        let block_node = self.document.node_at(edit.block_path)?;
        if block.doc_start != document_position
            || block.doc_end != document_position
            || block_node.content()?.child_count() != 0
            || !schema
                .node(block_node.node_type())
                .is_some_and(|spec| matches!(spec.role, NodeRole::TextBlock))
            || crate::transform::validate_input_mark_set(edit.marks, schema).is_err()
            || super::canonical_marks(edit.marks, schema) != edit.marks
        {
            return None;
        }
        let created_block = Node::element(
            block_node.node_type().to_owned(),
            block_node.attrs().clone(),
            Fragment::from(vec![Node::text(edit.text.to_owned(), edit.marks.to_vec())]),
        );
        DocumentValidator::validate_subtree_report(
            &created_block,
            schema,
            &self.validation_certificate.resource_limits,
            DOCUMENT_ROOT_DEPTH.checked_add(edit.block_path.len())?,
        )
        .ok()?;
        let canonical_growth_bytes = canonical_json_len(&created_block, schema)
            .checked_sub(canonical_json_len(block_node, schema))?;
        if canonical_growth_bytes > escaped_limit {
            return None;
        }
        let placeholder = crate::render::empty_text_block_placeholder_string();
        let placeholder_scalars = u32::try_from(placeholder.chars().count()).ok()?;
        let placeholder_utf16 = u32::try_from(placeholder.encode_utf16().count()).ok()?;
        let scalar_start = scalar_at.checked_sub(placeholder_scalars)?;
        let utf16_start = utf16_at.checked_sub(placeholder_utf16)?;
        Some(TextblockEditTarget {
            leaf: LocalizedTextLeafCertificate {
                block_index,
                child_ordinal: 0,
                doc_start: document_position,
                doc_end: document_position,
                scalar_start,
                scalar_end: scalar_start,
                utf16_start,
                utf16_end: utf16_start,
                text_sha256: sha2::Sha256::digest([]).into(),
                text_scalars: 0,
                text_utf16: 0,
                text_utf8_bytes: 0,
                marks_sha256,
            },
            scalar_at,
            utf16_at,
            creates_leaf: true,
            canonical_growth_bytes: isize::try_from(canonical_growth_bytes).ok()?,
            rendered_scalar_delta: i32::try_from(
                inserted_scalars.checked_sub(placeholder_scalars)?,
            )
            .ok()?,
            rendered_utf16_delta: i32::try_from(inserted_utf16.checked_sub(placeholder_utf16)?)
                .ok()?,
            removed_scalars: 0,
            removed_utf8_bytes: 0,
            removed_utf16: 0,
            empty_after: false,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn textblock_range_target(
        &self,
        block_index: usize,
        edit: &LocalizedTextblockEdit<'_>,
        schema: &Schema,
        scalar_at: u32,
        utf16_at: u32,
        inserted_scalars: u32,
        inserted_utf16: u32,
    ) -> Option<TextblockEditTarget> {
        let block = self.position_map.block(block_index)?;
        if edit.replaced.end > block.doc_end {
            return None;
        }
        let old = self.document.node_at(edit.block_path)?;
        let mut position = block.doc_start;
        let mut removed = String::new();
        for child in old.content()?.iter() {
            let end = position.checked_add(child.node_size())?;
            let from = position.max(edit.replaced.start);
            let to = end.min(edit.replaced.end);
            if from < to {
                let text = child.text_str()?;
                removed.extend(
                    text.chars()
                        .skip(usize::try_from(from - position).ok()?)
                        .take(usize::try_from(to - from).ok()?),
                );
            }
            position = end;
        }
        let removed_scalars = edit.replaced.end.checked_sub(edit.replaced.start)?;
        if u32::try_from(removed.chars().count()).ok()? != removed_scalars {
            return None;
        }
        crate::transform::validate_input_mark_set(edit.marks, schema).ok()?;
        if super::canonical_marks(edit.marks, schema) != edit.marks {
            return None;
        }
        let step = if edit.text.is_empty() {
            Step::DeleteRange {
                from: edit.replaced.start,
                to: edit.replaced.end,
            }
        } else {
            Step::ReplaceRange {
                from: edit.replaced.start,
                to: edit.replaced.end,
                content: Fragment::from(vec![Node::text(
                    edit.text.to_owned(),
                    edit.marks.to_vec(),
                )]),
            }
        };
        let (preview, _) = crate::transform::apply_step(&self.document, &step, schema).ok()?;
        let new = preview.node_at(edit.block_path)?;
        if old.node_type() != new.node_type() || old.attrs() != new.attrs() {
            return None;
        }
        let depth = DOCUMENT_ROOT_DEPTH.checked_add(edit.block_path.len())?;
        let limits = &self.validation_certificate.resource_limits;
        DocumentValidator::validate_subtree_report(new, schema, limits, depth).ok()?;
        crate::transform::validate_subtree_marks(new, schema).ok()?;
        let empty_after = new.content()?.child_count() == 0;
        let removed_utf16 = u32::try_from(removed.encode_utf16().count()).ok()?;
        let index = self.localized_text_index.as_ref()?;
        let leaf = *index
            .leaves()
            .iter()
            .skip(index.leaf_slot(edit.replaced.start))
            .take(LEAVES_MEETING_AT_A_POSITION)
            .find(|leaf| {
                leaf.block_index == block_index
                    && leaf.doc_start <= edit.replaced.start
                    && edit.replaced.start < leaf.doc_end
            })?;
        Some(TextblockEditTarget {
            leaf,
            scalar_at,
            utf16_at,
            creates_leaf: false,
            canonical_growth_bytes: isize::try_from(canonical_json_len(new, schema))
                .ok()?
                .checked_sub(isize::try_from(canonical_json_len(old, schema)).ok()?)?,
            rendered_scalar_delta: i32::try_from(inserted_scalars)
                .ok()?
                .checked_sub(i32::try_from(removed_scalars).ok()?)?
                .checked_add(i32::from(empty_after))?,
            rendered_utf16_delta: i32::try_from(inserted_utf16)
                .ok()?
                .checked_sub(i32::try_from(removed_utf16).ok()?)?
                .checked_add(i32::from(empty_after))?,
            removed_scalars,
            removed_utf8_bytes: removed.len(),
            removed_utf16,
            empty_after,
        })
    }
}

fn canonical_json_len(node: &Node, schema: &Schema) -> usize {
    crate::boundary::serialize_json_value_stack_safe(
        &crate::serialize::node_to_prosemirror_json(node, schema),
        0,
    )
    .len()
}

fn checked_json_string_body_len(text: &str, limit: usize) -> Option<usize> {
    let mut bytes = 0usize;
    for character in text.chars() {
        let amount = match character {
            '"' | '\\' | '\u{0008}' | '\u{000c}' | '\n' | '\r' | '\t' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            other => other.len_utf8(),
        };
        bytes = bytes.checked_add(amount)?;
        if bytes > limit {
            return None;
        }
    }
    Some(bytes)
}
