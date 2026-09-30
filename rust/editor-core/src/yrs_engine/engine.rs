mod candidate;
mod candidate_cache;
mod commands;
mod commit;
mod commit_installation;
mod compilation;
mod history_state;
mod imports;
mod mutation_context;
mod outbound;
mod position_epoch_cells;
mod remote;
mod selection_commit;
mod snapshots;
#[cfg(test)]
mod test_hooks;
mod transaction_result;
mod transactions;
mod undo_redo;

use super::canonical::CanonicalSchemaContext;
#[cfg(test)]
use super::compiler::CompiledTransaction;
use super::derived_state::DerivedStateCache;
#[cfg(test)]
use super::YrsDocumentCodec;
use super::{
    DocumentScope, DocumentSnapshot, EditingLimits, TransactionOrigin, YrsEngineError,
    YrsEngineResult,
};
use crate::boundary::ResourceLimits;
use crate::model::Document;
use crate::position::PositionMap;
use crate::schema::Schema;
use crate::serialize::to_html;
use candidate::{
    admit_candidate_derived_output, build_await_remote_candidate,
    build_derived_state_for_candidate, build_local_empty_candidate,
};
#[cfg(test)]
use candidate::{CandidateDocument, EngineDocumentState};
use candidate_cache::{encode_state_bounded, PreparedCandidateCache};
#[cfg(test)]
use candidate_cache::{
    equivalent_private_candidate_doc, fresh_utf16_doc_excluding, fresh_utf16_doc_excluding_with,
    prepare_import_candidate_cache, retained_import_state_charge, seal_candidate_state_vector,
    utf16_doc,
};
#[cfg(test)]
use history_state::history_metadata_bytes;
pub(crate) use imports::admit_local_import_document;
pub(in crate::yrs_engine) use imports::ValidatedImportDocument;
#[cfg(test)]
use outbound::OutboundUpdateSink;
#[cfg(test)]
use remote::admit_max_encoded_state_len;
pub use remote::PreparedRemoteUpdate;
use serde_json::json;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
#[cfg(test)]
use test_hooks::{
    check_compiled_commit_preparation_stage_for_test, mark_compiled_commit_durable_write_for_test,
    reset_encoded_state_reuse_counts_for_test, reset_history_replay_guard_encodings_for_test,
    reset_import_receipt_sha256_counts_for_test, reset_import_receipt_state_decodings_for_test,
    reset_import_state_encoding_counts_for_test, reset_prepared_candidate_cache_counts_for_test,
    set_compiled_commit_stage_failpoint_for_test, set_outbound_staging_copy_failure_for_test,
    set_quarantined_update_reservation_failure_for_test,
    set_replay_candidate_perturbation_for_test, take_compiled_commit_authority_counts_for_test,
    take_encoded_state_reuse_counts_for_test, take_history_replay_guard_encodings_for_test,
    take_import_receipt_sha256_counts_for_test, take_import_receipt_state_decodings_for_test,
    take_import_state_encoding_counts_for_test, take_prepared_candidate_cache_counts_for_test,
    CompiledCommitPreparationStage,
};
use yrs::sync::time::{Clock, SystemClock};
use yrs::Doc;
use yrs::{ReadTxn, Transact};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitializationMode {
    LocalEmpty,
    AwaitRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineRenderState {
    Loading,
    Ready,
}

#[derive(Debug, Clone)]
pub struct YrsEngineConfig {
    pub schema: Schema,
    pub fragment_name: String,
    pub initialization_mode: InitializationMode,
    pub resource_limits: ResourceLimits,
    pub editing_limits: EditingLimits,
    pub max_length: Option<u32>,
    pub scope: Option<DocumentScope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineCommit {
    pub changed: bool,
    pub revision: u64,
}

const DOCUMENT_CHANGE_SCOPE_LOG_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DocumentChangeScope {
    Textblock { block_index: usize },
    Document,
}

pub struct YrsDocumentEngine {
    doc: Doc,
    fragment_name: String,
    schema: Schema,
    resource_limits: ResourceLimits,
    editing_limits: EditingLimits,
    max_length: Option<u32>,
    scope: Option<DocumentScope>,
    schema_fingerprint: String,
    canonical_schema: CanonicalSchemaContext,
    canonical_splice_cache: Option<super::canonical::CanonicalSpliceCache>,
    derived_state: Option<DerivedStateCache>,
    revision: u64,
    encoded_state_upper_bound: usize,
    document_scope_revision: u64,
    last_recorded_revision: u64,
    last_change_scope: DocumentChangeScope,
    change_scopes: VecDeque<(u64, DocumentChangeScope)>,
    #[cfg(test)]
    recorded_change_count: u64,
    state_revision: u64,
    yrs_state_epoch: u64,
    last_committed_origin: Option<TransactionOrigin>,
    document_origin: super::DocumentOrigin,
    durable_client_ids: HashSet<u64>,
    /// Dependency-pending standard updates are quarantined outside the live
    /// authoritative Doc until their complete merged state can be validated.
    quarantined_remote_update: Option<Vec<u8>>,
    /// Invalidates every outstanding [`PreparedRemoteUpdate`] seal on engine
    /// transitions that do NOT change revision/state-revision/epoch or the
    /// store handle: (a) a new dependency-pending payload entering quarantine
    /// (committing an older prepare would silently discard it), and (b) the
    /// unchanged fast paths of snapshot restore and canonical-equal imports,
    /// which clear the quarantine and rebind the bounded history replay chain
    /// (committing across that rebind could both resurrect intentionally
    /// discarded dependency bytes and violate the prepared replay-slot
    /// capacity invariants mid-install). Every other quarantine or history
    /// transition also changes a revision/epoch, which the seal covers.
    remote_seal_generation: u64,
    /// The engine-owned awareness codec: the sole `yrs::sync::Awareness`
    /// bound to the authoritative `Doc`, rebound on every store swap.
    awareness: Option<super::awareness::AwarenessCodec>,
    history: super::history::YrsHistory,
    /// An exact private replica used only to prove the next local commit. It is
    /// never exposed as editor authority and is consumed on use, so any
    /// recoverable preparation failure automatically drops it rather than
    /// publishing partially prepared state.
    prepared_candidate_cache: Option<PreparedCandidateCache>,
}

impl YrsDocumentEngine {
    pub fn new(config: YrsEngineConfig) -> YrsEngineResult<Self> {
        Self::new_with_history_clock(config, Arc::new(SystemClock))
    }

    pub fn new_with_snapshot(
        config: YrsEngineConfig,
        snapshot: &DocumentSnapshot,
    ) -> YrsEngineResult<Self> {
        if config.initialization_mode != InitializationMode::AwaitRemote {
            return Err(YrsEngineError::new(
                "CONFIG_INVALID",
                "snapshot initialization is only valid for an awaiting room document",
            )
            .with_details(json!({ "field": "initializationMode" })));
        }
        let mut engine = Self::new(config)?;
        engine.restore_snapshot(snapshot)?;
        Ok(engine)
    }

    pub fn new_with_history_clock(
        config: YrsEngineConfig,
        history_clock: Arc<dyn Clock>,
    ) -> YrsEngineResult<Self> {
        let YrsEngineConfig {
            schema,
            fragment_name,
            initialization_mode,
            resource_limits,
            editing_limits,
            max_length,
            scope,
        } = config;
        resource_limits.validate()?;
        editing_limits.validate()?;
        validate_config_metadata(&fragment_name, scope.as_ref(), &resource_limits)?;
        let canonical_schema = CanonicalSchemaContext::new(&schema);
        let schema_fingerprint = canonical_schema.schema_fingerprint().to_owned();
        let candidate = match initialization_mode {
            InitializationMode::LocalEmpty => build_local_empty_candidate(
                &schema,
                &canonical_schema,
                &fragment_name,
                &resource_limits,
            )?,
            InitializationMode::AwaitRemote => {
                build_await_remote_candidate(&fragment_name, &resource_limits)?
            }
        };
        admit_candidate_derived_output(&candidate, &editing_limits)?;
        let derived_state = build_derived_state_for_candidate(
            &candidate,
            &schema,
            &resource_limits,
            &editing_limits,
            max_length,
            &schema_fingerprint,
            &fragment_name,
            &canonical_schema,
            0,
            None,
            0,
            0,
            0,
        )?;
        let history_fragment = {
            let txn = candidate.doc.transact();
            txn.get_xml_fragment(fragment_name.as_str())
                .ok_or_else(|| {
                    YrsEngineError::new(
                        "CODEC_INVARIANT_FAILED",
                        "initialized Yrs fragment is missing while binding history",
                    )
                })?
        };
        let history = super::history::YrsHistory::new(
            &candidate.doc,
            &history_fragment,
            editing_limits.clone(),
            resource_limits.max_encoded_state_bytes,
            history_clock,
        );

        Ok(Self {
            encoded_state_upper_bound: candidate.encoded_state_bytes,
            doc: candidate.doc,
            fragment_name,
            schema,
            resource_limits,
            editing_limits,
            max_length,
            scope,
            schema_fingerprint,
            canonical_schema,
            canonical_splice_cache: None,
            derived_state,
            revision: 0,
            document_scope_revision: 0,
            last_recorded_revision: 0,
            last_change_scope: DocumentChangeScope::Document,
            change_scopes: VecDeque::with_capacity(DOCUMENT_CHANGE_SCOPE_LOG_CAPACITY),
            #[cfg(test)]
            recorded_change_count: 0,
            state_revision: 0,
            yrs_state_epoch: 0,
            last_committed_origin: None,
            document_origin: super::DocumentOrigin::Import,
            durable_client_ids: candidate.durable_client_ids,
            quarantined_remote_update: None,
            remote_seal_generation: 0,
            awareness: None,
            history,
            prepared_candidate_cache: None,
        })
    }

    fn record_document_change(&mut self, scope: DocumentChangeScope) {
        debug_assert!(self.revision > self.last_recorded_revision);
        self.last_recorded_revision = self.revision;
        self.last_change_scope = scope;
        if self.change_scopes.len() == DOCUMENT_CHANGE_SCOPE_LOG_CAPACITY {
            self.change_scopes.pop_front();
        }
        self.change_scopes.push_back((self.revision, scope));
        if scope == DocumentChangeScope::Document {
            self.document_scope_revision = self.revision;
        }
        #[cfg(test)]
        {
            self.recorded_change_count += 1;
        }
    }

    pub(crate) fn change_scopes_since(&self, revision: u64) -> Option<Vec<DocumentChangeScope>> {
        if revision > self.revision {
            return None;
        }
        let mut expected = revision;
        let mut scopes = Vec::new();
        for &(changed, scope) in self
            .change_scopes
            .iter()
            .filter(|(changed, _)| *changed > revision)
        {
            expected = expected.checked_add(1)?;
            if changed != expected {
                return None;
            }
            scopes.push(scope);
        }
        (expected == self.revision).then_some(scopes)
    }

    pub(crate) fn document_scope_revision(&self) -> u64 {
        self.document_scope_revision
    }

    pub fn is_ready(&self) -> bool {
        self.derived_state.is_some()
    }

    pub(crate) fn active_state(&self) -> Option<crate::editor_state::ActiveState> {
        let state = self.derived_state.as_ref()?;
        Some(state.render_active_state(
            &self.schema,
            &self.resource_limits,
            &self.editing_limits,
            self.document_scope_revision(),
        ))
    }

    pub fn render_state(&self) -> EngineRenderState {
        if self.is_ready() {
            EngineRenderState::Ready
        } else {
            EngineRenderState::Loading
        }
    }

    #[cfg(test)]
    fn prepared_candidate_cache_store_token_for_test(&self) -> Option<usize> {
        self.prepared_candidate_cache
            .as_ref()
            .map(PreparedCandidateCache::store_token)
    }

    /// Production surface: the engine-owned awareness codec, lazily bound to the
    /// authoritative `Doc`. The codec never exposes the document, a
    /// transaction, or the raw `Awareness` handle.
    pub fn awareness(&mut self) -> &mut super::awareness::AwarenessCodec {
        let doc = &self.doc;
        self.awareness
            .get_or_insert_with(|| super::awareness::AwarenessCodec::bind(doc))
    }

    /// Unresolvable sticky points return None so peer projections can omit the cursor.
    pub fn resolve_awareness_sticky_doc_pos(&self, sticky_json: &serde_json::Value) -> Option<u32> {
        let sticky: yrs::StickyIndex = serde_json::from_value(sticky_json.clone()).ok()?;
        let txn = self.doc.transact();
        let fragment = txn.get_xml_fragment(self.fragment_name.as_str())?;
        super::position::sticky_index_to_doc_pos(&txn, &fragment, &sticky, &self.schema)
    }

    /// Sealed awareness surface: materialize two valid document positions as
    /// sticky Yrs indices in this engine's current document context. Callers
    /// receive only the wire JSON; neither the document nor its transaction
    /// crosses the engine boundary.
    pub(crate) fn awareness_sticky_cursor(
        &self,
        anchor: u32,
        head: u32,
    ) -> Option<serde_json::Value> {
        let txn = self.doc.transact();
        let fragment = txn.get_xml_fragment(self.fragment_name.as_str())?;
        let collapsed = anchor == head;
        let anchor = super::cursor_sticky_index_from_doc_pos(
            &txn,
            &fragment,
            anchor,
            collapsed,
            &self.schema,
        )?;
        let head = super::cursor_sticky_index_from_doc_pos(
            &txn,
            &fragment,
            head,
            collapsed,
            &self.schema,
        )?;
        Some(serde_json::json!({ "anchor": anchor, "head": head }))
    }

    pub(crate) fn awareness_cell_rectangle(
        &self,
        anchor: u32,
        head: u32,
    ) -> Option<serde_json::Value> {
        crate::tables::selection::cell_pair_is_usable(
            self.table_projection_index()?,
            anchor,
            head,
            crate::tables::selection::CellSelectionOrigin::Minted,
        )
        .then_some(())?;
        let txn = self.doc.transact();
        let fragment = txn.get_xml_fragment(self.fragment_name.as_str())?;
        let sticky = |position| cell_opening_sticky_index(&txn, &fragment, position, &self.schema);
        Some(super::awareness::encode_relative_cell_rectangle(
            &super::awareness::RelativeCellRectangle {
                anchor: sticky(anchor)?,
                head: sticky(head)?,
            },
        ))
    }

    pub fn resolve_awareness_cell_rectangle(
        &self,
        state: &serde_json::Value,
    ) -> Option<(u32, u32)> {
        let rectangle = super::awareness::decode_relative_cell_rectangle(state)?;
        let txn = self.doc.transact();
        let fragment = txn.get_xml_fragment(self.fragment_name.as_str())?;
        let live_opening = |sticky: &yrs::StickyIndex| {
            let position =
                super::position::sticky_index_to_doc_pos(&txn, &fragment, sticky, &self.schema)?;
            let current = cell_opening_sticky_index(&txn, &fragment, position, &self.schema)?;
            (current.id() == sticky.id()).then_some(position)
        };
        let anchor = live_opening(&rectangle.anchor)?;
        let head = live_opening(&rectangle.head)?;
        crate::tables::selection::cell_pair_is_usable(
            self.table_projection_index()?,
            anchor,
            head,
            crate::tables::selection::CellSelectionOrigin::Minted,
        )
        .then_some((anchor, head))
    }

    pub(crate) fn clipboard(&self) -> Option<serde_json::Value> {
        let document = self.document()?;
        let selection = super::derived_state::resolved_to_legacy(self.resolved_selection()?);
        if let Some(reason) = crate::clipboard::unsupported_selection(&selection) {
            let index = self.table_projection_index()?;
            return Some(
                match crate::clipboard::export_cells(document, &selection, index, &self.schema) {
                    Ok(copied) => copied,
                    Err(crate::tables::interchange::InterchangeFailure::NotACellRectangle) => {
                        serde_json::json!({ crate::clipboard::CLIPBOARD_UNSUPPORTED_KEY: reason })
                    }
                    Err(crate::tables::interchange::InterchangeFailure::UnreadableGrid) => {
                        serde_json::json!({
                            crate::clipboard::CLIPBOARD_UNSUPPORTED_KEY:
                                crate::clipboard::CLIPBOARD_UNSUPPORTED_TABLE_GRID
                        })
                    }
                },
            );
        }
        Some(
            crate::clipboard::export(document, &selection, &self.schema).unwrap_or_else(
                || serde_json::json!({ crate::clipboard::CLIPBOARD_EMPTY_KEY: true }),
            ),
        )
    }

    pub fn document(&self) -> Option<&Document> {
        self.debug_assert_derived_revision_keys();
        let state = self.derived_state.as_ref()?;
        Some(&state.document)
    }

    pub(crate) fn cached_render_blocks(
        &self,
    ) -> Option<Arc<crate::render::incremental::CachedRenderBlocks>> {
        self.debug_assert_derived_revision_keys();
        self.derived_state
            .as_ref()
            .map(|state| Arc::clone(&state.render_blocks))
    }

    pub(crate) fn block_branch_index(
        &self,
    ) -> Option<&super::block_branch_index::BlockBranchIndex> {
        self.debug_assert_derived_revision_keys();
        self.derived_state.as_ref()?.block_branch_index.as_deref()
    }

    pub fn document_json(&self) -> Option<serde_json::Value> {
        self.debug_assert_derived_revision_keys();
        self.derived_state.as_ref().map(|state| {
            crate::boundary::clone_json_value_stack_safe(state.canonical_artifact.value())
        })
    }

    pub(crate) fn document_json_string(&self) -> Option<String> {
        self.debug_assert_derived_revision_keys();
        self.derived_state.as_ref().map(|state| {
            String::from_utf8(crate::boundary::serialize_json_value_stack_safe(
                state.canonical_artifact.value(),
                state.canonical_artifact.serialized_len(),
            ))
            .expect("serialized JSON is UTF-8")
        })
    }

    pub(crate) fn table_projection_index(
        &self,
    ) -> Option<&crate::tables::admission::TableProjectionIndex> {
        self.derived_state
            .as_ref()
            .map(|state| &state.table_projection_index)
    }

    pub fn document_html(&self) -> Option<String> {
        self.document()
            .map(|document| to_html(document, &self.schema))
    }

    #[allow(dead_code)]
    pub fn encoded_state(&self) -> YrsEngineResult<Vec<u8>> {
        encode_state_bounded(&self.doc, &self.resource_limits)
    }

    #[allow(dead_code)]
    pub fn has_document_state(&self) -> bool {
        !self.doc.transact().state_vector().is_empty()
    }

    pub fn revision(&self) -> u64 {
        self.debug_assert_derived_revision_keys();
        self.revision
    }

    pub fn state_revision(&self) -> u64 {
        self.debug_assert_derived_revision_keys();
        self.state_revision
    }

    #[cfg(feature = "table-interop")]
    pub(crate) fn availability_history_audit(
        &self,
    ) -> Option<super::history::AvailabilityHistoryAudit> {
        self.history.availability_audit()
    }

    #[cfg(feature = "table-interop")]
    pub(crate) fn availability_content_audit(&self) -> Option<Vec<u8>> {
        let state = self.derived_state.as_ref()?;
        if state.canonical_artifact.serialized_len() > crate::availability_audit::MAX_BYTES {
            return None;
        }
        crate::availability_audit::freeze_json(
            state.canonical_artifact.value(),
            crate::availability_audit::MAX_BYTES,
        )
    }

    #[cfg(feature = "table-interop")]
    pub(crate) fn availability_encoded_audit(&self) -> Option<Vec<u8>> {
        self.doc.transact().encode_state_for_history_audit(
            crate::availability_audit::MAX_BYTES,
            crate::availability_audit::MAX_ITEMS,
        )
    }

    #[cfg(feature = "table-interop")]
    pub(crate) fn availability_history_metadata_audit(
        &self,
    ) -> Option<Vec<yrs::HistoryMetadataAuditItem>> {
        self.doc.transact().store().history_metadata_audit(65_536)
    }

    /// Production audit surface: the Yrs state epoch, so full before/after
    /// session audits can pin epoch stability across atomic rejections.
    #[allow(dead_code)]
    pub fn yrs_state_epoch(&self) -> u64 {
        self.yrs_state_epoch
    }

    pub fn position_map(&self) -> Option<&PositionMap> {
        self.debug_assert_derived_revision_keys();
        self.derived_state.as_ref().map(|state| &state.position_map)
    }

    pub(crate) fn build_position_epoch_snapshot(
        &self,
    ) -> Option<crate::position_epoch::EpochSnapshot> {
        self.debug_assert_derived_revision_keys();
        let state = self.derived_state.as_ref()?;
        let txn = self.doc.transact();
        let fragment = txn.get_xml_fragment(self.fragment_name.as_str())?;
        let doc_positions = if state.position_map.block_count() == 0 {
            vec![vec![0]]
        } else {
            (0..state.position_map.block_count())
                .map(|index| {
                    state
                        .position_map
                        .block_doc_positions(index, &state.document)
                })
                .collect::<Option<Vec<_>>>()?
        };
        let mut chunks = super::position::boundary_chunks_at_doc_positions(
            &txn,
            &fragment,
            &doc_positions,
            &self.schema,
        )?;
        #[cfg(test)]
        (0..state.position_map.block_count())
            .for_each(|_| super::observability::record_epoch_block_rebuild());
        let spans = self.cell_pinning(state).spans(&doc_positions);
        let scalar_starts = crate::position_epoch::EpochSnapshot::scalar_starts(&chunks)?;
        crate::position_epoch::EpochSnapshot::attach_cells(
            &mut chunks,
            &scalar_starts,
            spans.iter().enumerate(),
        )?;
        crate::position_epoch::EpochSnapshot::new(
            self.yrs_state_epoch,
            self.revision,
            chunks,
            spans,
        )
    }

    pub(crate) fn update_position_epoch_snapshot(
        &self,
        previous: &crate::position_epoch::EpochSnapshot,
    ) -> Option<crate::position_epoch::EpochSnapshot> {
        use std::collections::BTreeSet;
        use yrs::types::xml::XmlElementRef;
        let scopes = self.change_scopes_since(previous.document_revision)?;
        if scopes.is_empty() {
            return None;
        }
        let state = self.derived_state.as_ref()?;
        if state.position_map.block_count() != previous.chunks.len() {
            return None;
        }
        let mut blocks = BTreeSet::new();
        let mut cells = BTreeSet::new();
        for scope in scopes {
            let DocumentChangeScope::Textblock { block_index } = scope else {
                return None;
            };
            let path = state.position_map.block(block_index)?.node_path.as_slice();
            let outer = (1..path.len()).find_map(|depth| {
                let prefix = &path[..depth];
                let node = state.document.node_at(prefix)?;
                matches!(
                    self.schema.node(node.node_type())?.table_role,
                    Some(crate::tables::TableRole::Cell | crate::tables::TableRole::HeaderCell)
                )
                .then_some(prefix)
            });
            if let Some(path) = outer {
                let first = previous
                    .cells
                    .partition_point(|span| span.node_path.as_slice() < path);
                let outer = previous
                    .cells
                    .get(first)
                    .filter(|span| span.node_path == path)?;
                blocks.extend(outer.block_range.clone());
                for index in first..previous.cells.len() {
                    if !previous.cells[index].node_path.starts_with(path) {
                        break;
                    }
                    cells.insert(index);
                }
            } else {
                blocks.insert(block_index);
            }
        }
        let blocks: Vec<_> = blocks.into_iter().collect();
        let cell_indexes: Vec<_> = cells.into_iter().collect();
        let positions = blocks
            .iter()
            .map(|index| {
                Some((
                    *index,
                    state
                        .position_map
                        .block_doc_positions(*index, &state.document)?,
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        let updated_cells = self.cell_pinning(state).rebuild_spans(
            &cell_indexes
                .iter()
                .map(|index| previous.cells[*index].as_ref())
                .collect::<Vec<_>>(),
            &positions,
        )?;
        let mut cells = previous.cells.to_vec();
        for (&index, span) in cell_indexes.iter().zip(updated_cells) {
            cells[index] = span;
        }
        let mut chunks = previous.chunks.to_vec();
        let txn = self.doc.transact();
        let branches = state.block_branch_index.as_ref()?;
        for (index, positions) in positions {
            if state.position_map.block(index)?.is_void_block {
                let mut anchors = previous.chunks[index].anchors.clone();
                for boundary in &mut anchors {
                    boundary.pinned_cell = None;
                }
                chunks[index] = Arc::new(crate::position_epoch::EpochBlockChunk::new(anchors)?);
                #[cfg(test)]
                super::observability::record_epoch_block_rebuild();
                continue;
            }
            let block = branches.block_branches(index)?;
            let element = XmlElementRef::from(block.element.get_branch(&txn)?);
            chunks[index] = super::position::boundary_chunk_for_block(
                &txn,
                &element,
                state.position_map.effective_doc_start(index),
                positions,
                &self.schema,
                previous.chunks[index].ancestor.clone(),
            )?;
            #[cfg(test)]
            super::observability::record_epoch_block_rebuild();
        }
        previous.with_rebuilt_chunks(
            self.yrs_state_epoch,
            self.revision,
            chunks,
            cells,
            &blocks,
            &cell_indexes,
        )
    }

    #[cfg(test)]
    pub(crate) fn mutation_lookup_matches_fresh_for_test(&self) -> bool {
        let Some(state) = self.derived_state.as_ref() else {
            return false;
        };
        self.read_fragment_for_test(|txn, fragment| {
            let fresh = super::mutation::MutationLookupSeed::build(
                0,
                txn,
                fragment,
                &self.schema,
                &state.document,
                &self.resource_limits,
                &self.editing_limits,
                self.max_length,
                &self.schema_fingerprint,
                self.yrs_state_epoch,
                self.revision,
            )
            .expect("ready document lookup builds");
            state
                .mutation_lookup_seed
                .has_same_ready_payload_for_test(&fresh)
        })
        .unwrap_or(false)
    }

    #[cfg(test)]
    pub(crate) fn block_branch_index_for_test(
        &self,
    ) -> Option<&super::block_branch_index::BlockBranchIndex> {
        self.derived_state.as_ref()?.block_branch_index.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn read_fragment_for_test<R>(
        &self,
        read: impl FnOnce(&yrs::Transaction<'_>, &yrs::XmlFragmentRef) -> R,
    ) -> Option<R> {
        let txn = self.doc.transact();
        let fragment = txn.get_xml_fragment(self.fragment_name.as_str())?;
        Some(read(&txn, &fragment))
    }

    #[cfg(test)]
    pub(crate) fn drop_localized_text_index_for_test(&mut self) {
        if let Some(state) = self.derived_state.as_mut() {
            state.localized_text_index = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_history_for_test(&self) -> (u64, usize) {
        (
            self.history
                .retained_units(0)
                .expect("retained history units fit their limit"),
            self.history.replay_metadata_bytes_for_test(),
        )
    }

    fn cell_pinning<'state>(
        &'state self,
        state: &'state super::derived_state::DerivedStateCache,
    ) -> position_epoch_cells::CellPinning<'state> {
        position_epoch_cells::CellPinning {
            document: &state.document,
            schema: &self.schema,
            index: &state.table_projection_index,
            position_map: &state.position_map,
        }
    }

    pub(crate) fn resolve_position_epoch_boundary(
        &self,
        boundary: &crate::position_epoch::EpochBoundary<'_>,
        affinity: super::Affinity,
        original_offset: u32,
    ) -> Option<crate::position_epoch::ResolvedBoundary> {
        self.debug_assert_derived_revision_keys();
        let state = self.derived_state.as_ref()?;
        let txn = self.doc.transact();
        let fragment = txn.get_xml_fragment(self.fragment_name.as_str())?;
        let anchors = boundary.anchors;
        let chain = boundary.ancestor_chain();
        let table_cell_ancestors = chain.clone().position(|ancestor| ancestor.table_cell);
        let side = |before: bool| {
            chain.clone().enumerate().map(move |(depth, ancestor)| {
                let sticky = if before {
                    &ancestor.before
                } else {
                    &ancestor.after
                };
                (true, Some(depth), sticky)
            })
        };
        let leading_before = matches!(affinity, super::Affinity::Before);
        let (leaf, opposite_leaf) = if leading_before {
            (&anchors.before, &anchors.after)
        } else {
            (&anchors.after, &anchors.before)
        };
        for (fallback, ancestor_depth, sticky) in std::iter::once((false, None, leaf))
            .chain(side(leading_before))
            .chain(std::iter::once((true, None, opposite_leaf)))
            .chain(side(!leading_before))
        {
            let Some(doc_pos) =
                super::position::sticky_index_to_doc_pos(&txn, &fragment, sticky, &self.schema)
            else {
                continue;
            };
            let pinning = self.cell_pinning(state);
            let resolved =
                |offset: u32, left_table_cell: bool| crate::position_epoch::ResolvedBoundary {
                    offset,
                    fallback,
                    left_table_cell,
                };
            let unmapped = state.position_map.doc_to_scalar(doc_pos, &state.document);
            let Some((depth, cell_depth)) = ancestor_depth.zip(table_cell_ancestors) else {
                return Some(resolved(unmapped, false));
            };
            if depth < cell_depth {
                let surviving = boundary.pinned_cell.and_then(|pinned| {
                    pinning.reanchor_in_surviving_cell(doc_pos, pinned, affinity)
                });
                return Some(resolved(surviving.unwrap_or(unmapped), false));
            }
            let retyped = boundary
                .pinned_cell
                .filter(|_| depth == cell_depth)
                .and_then(|pinned| pinning.reanchor_in_retyped_cell(doc_pos, pinned));
            return Some(
                retyped.map_or(resolved(unmapped, true), |offset| resolved(offset, false)),
            );
        }
        Some(crate::position_epoch::ResolvedBoundary {
            offset: original_offset.min(state.position_map.total_scalars()),
            fallback: true,
            left_table_cell: table_cell_ancestors.is_some(),
        })
    }

    pub fn relative_selection(&self) -> Option<&super::RelativeSelection> {
        self.debug_assert_derived_revision_keys();
        self.derived_state
            .as_ref()
            .map(|state| &state.relative_selection)
    }

    pub fn resolved_selection(&self) -> Option<&super::ResolvedSelection> {
        self.debug_assert_derived_revision_keys();
        self.derived_state
            .as_ref()
            .map(|state| &state.resolved_selection)
    }

    pub fn stored_marks(&self) -> Option<&[crate::model::Mark]> {
        self.debug_assert_derived_revision_keys();
        self.derived_state
            .as_ref()
            .and_then(|state| state.stored_marks.as_deref())
    }

    pub fn client_id(&self) -> u64 {
        self.doc.client_id().get()
    }

    #[allow(dead_code)]
    pub fn fragment_name(&self) -> &str {
        &self.fragment_name
    }

    #[allow(dead_code)]
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    #[allow(dead_code)]
    pub fn schema_fingerprint(&self) -> &str {
        &self.schema_fingerprint
    }

    #[allow(dead_code)]
    pub fn scope(&self) -> Option<&DocumentScope> {
        self.scope.as_ref()
    }

    #[allow(dead_code)]
    pub fn last_committed_origin(&self) -> Option<TransactionOrigin> {
        self.last_committed_origin
    }

    pub fn document_origin(&self) -> super::DocumentOrigin {
        self.document_origin
    }

    pub(crate) fn mark_document_origin_native_view(&mut self) {
        self.document_origin = super::DocumentOrigin::NativeView;
    }

    pub fn resource_limits(&self) -> &ResourceLimits {
        &self.resource_limits
    }

    #[allow(dead_code)]
    pub fn editing_limits(&self) -> &EditingLimits {
        &self.editing_limits
    }

    #[allow(dead_code)]
    pub fn max_length(&self) -> Option<u32> {
        self.max_length
    }

    fn debug_assert_derived_revision_keys(&self) {
        if let Some(state) = &self.derived_state {
            debug_assert_eq!(state.document_revision, self.revision);
            debug_assert_eq!(state.state_revision, self.state_revision);
            debug_assert!(state
                .render_blocks
                .matches_identity(&state.document, &state.schema_fingerprint));
            debug_assert_eq!(
                state.document_node_count,
                crate::editor_state::document_node_count(state.document.root())
            );
        }
    }

    fn next_revision(&self) -> YrsEngineResult<u64> {
        self.revision.checked_add(1).ok_or_else(|| {
            YrsEngineError::new(
                "REVISION_OVERFLOW",
                "document revision cannot be incremented",
            )
            .with_details(json!({ "field": "revision" }))
        })
    }

    fn reset_history_binding(&mut self) {
        self.canonical_splice_cache = None;
        let fragment = {
            let txn = self.doc.transact();
            txn.get_xml_fragment(self.fragment_name.as_str())
                .expect("ready Yrs document retains the history fragment")
        };
        self.encoded_state_upper_bound = self.history.rebind(&self.doc, &fragment);
        // Rebinding rebuilds the bounded replay chain (and, on the unchanged
        // restore/import fast paths, accompanies a quarantine clear) without
        // any revision/epoch change. Invalidate every outstanding prepared
        // remote update so a later commit can neither resurrect discarded
        // dependency bytes nor install against the reset replay chain.
        self.remote_seal_generation = self.remote_seal_generation.wrapping_add(1);
    }

    fn next_durable_revisions(&self) -> YrsEngineResult<(u64, u64, u64)> {
        let document_revision = self.next_revision()?;
        let state_revision = self.state_revision.checked_add(1).ok_or_else(|| {
            YrsEngineError::new("REVISION_OVERFLOW", "state revision cannot be incremented")
                .with_details(json!({ "field": "stateRevision" }))
        })?;
        let yrs_state_epoch = self.yrs_state_epoch.checked_add(1).ok_or_else(|| {
            YrsEngineError::new("REVISION_OVERFLOW", "Yrs state epoch cannot be incremented")
                .with_details(json!({ "field": "yrsStateEpoch" }))
        })?;
        Ok((document_revision, state_revision, yrs_state_epoch))
    }
}

fn checked_operation_increment(
    request_id: u64,
    value: u64,
    field: &'static str,
) -> super::OperationResult<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| super::OperationError::revision_overflow(request_id, field))
}

fn merge_operation_details(mapped: &mut super::OperationError, source: Option<serde_json::Value>) {
    let Some(serde_json::Value::Object(source)) = source else {
        return;
    };
    let target = mapped
        .details
        .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let serde_json::Value::Object(target) = target else {
        return;
    };
    for (key, value) in source {
        if key != "field" {
            target.insert(key, value);
        }
    }
}

fn validate_config_metadata(
    fragment_name: &str,
    scope: Option<&DocumentScope>,
    limits: &ResourceLimits,
) -> YrsEngineResult<()> {
    let fields = [
        ("fragmentName", fragment_name.len()),
        (
            "documentId",
            scope.map(|scope| scope.document_id.len()).unwrap_or(0),
        ),
        (
            "lineageId",
            scope.map(|scope| scope.lineage_id.len()).unwrap_or(0),
        ),
    ];
    for (field, actual) in fields {
        if actual > limits.max_input_bytes {
            return Err(YrsEngineError::limit(
                "INPUT_LIMIT_EXCEEDED",
                limits.max_input_bytes,
                actual,
            )
            .with_details(json!({ "field": field })));
        }
    }
    let total = fields
        .into_iter()
        .fold(0usize, |total, (_, bytes)| total.saturating_add(bytes));
    if total > limits.max_input_bytes {
        return Err(
            YrsEngineError::limit("INPUT_LIMIT_EXCEEDED", limits.max_input_bytes, total)
                .with_details(json!({ "field": "metadata" })),
        );
    }
    Ok(())
}

fn cell_opening_sticky_index<T: ReadTxn>(
    txn: &T,
    fragment: &yrs::XmlFragmentRef,
    position: u32,
    schema: &Schema,
) -> Option<yrs::StickyIndex> {
    super::position::doc_pos_to_sticky_index(txn, fragment, position, yrs::Assoc::After, schema)
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
