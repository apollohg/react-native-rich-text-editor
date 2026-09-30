#[derive(Debug, Clone)]
struct ResolvedText {
    kind: ResolvedTargetKind,
    gap_before: u32,
    text: String,
    scalar_len: u32,
    base_runs: Vec<PreparedTextRun>,
    current_runs: Vec<PreparedTextRun>,
    action_slots: Vec<usize>,
}

#[derive(Debug, Clone)]
enum ResolvedTargetKind {
    Existing {
        target: XmlTextRef,
        signature: TargetSignature,
    },
    Missing {
        parent: XmlElementRef,
        child_index: u32,
        signature: ParentSignature,
        create_action: Option<usize>,
    },
    Prepared {
        handle: PreparedHandle,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PreparedHandle {
    insert_id: usize,
    ordinal_path: Box<[usize]>,
}

#[derive(Debug, Clone)]
struct PendingPreparedInsert {
    parent: XmlParentRef,
    child_index: u32,
    nodes: Vec<PreparedXmlChild>,
    signature: Arc<StructuralParentSignature>,
    operation_index: usize,
    semantic_parent_path: Vec<u32>,
    first_semantic_index: u32,
}

#[derive(Debug, Clone)]
enum ActionSlot {
    Concrete(Box<YrsMutationAction>),
    PreparedInsert(usize),
    Tombstone,
}

impl ActionSlot {
    fn concrete(action: YrsMutationAction) -> Self {
        Self::Concrete(Box::new(action))
    }

    fn concrete_mut(&mut self) -> Option<&mut YrsMutationAction> {
        match self {
            Self::Concrete(action) => Some(action.as_mut()),
            Self::PreparedInsert(_) | Self::Tombstone => None,
        }
    }
}

enum LocatedTarget {
    Existing {
        start: u32,
        target: XmlTextRef,
        text: String,
        scalar_len: u32,
        signature: TargetSignature,
    },
    Missing {
        start: u32,
        parent: XmlElementRef,
        child_index: u32,
        signature: ParentSignature,
    },
}

#[derive(Debug, Clone)]
struct MaterializedText {
    text: String,
    scalar_len: u32,
    utf16_len: u32,
    signature_runs: Vec<TextSignatureRun>,
    prepared_runs: Vec<PreparedTextRun>,
    work: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct MutationDocumentContext<'a> {
    pub(crate) before: &'a Document,
    pub(crate) after: &'a Document,
    pub(crate) schema: &'a Schema,
    pub(crate) limits: &'a ResourceLimits,
}

#[derive(Clone, Copy)]
pub(crate) struct ReplacementInput<'a> {
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) boundaries: &'a [u32],
    pub(crate) content: &'a Fragment,
}

#[derive(Debug, Clone)]
struct StructuralParentTarget {
    parent: XmlParentRef,
    signature: Arc<StructuralParentSignature>,
    storage_children: Vec<StorageChildKind>,
}

#[derive(Debug, Clone)]
struct PendingElementAttrs {
    target: XmlElementRef,
    signature: Arc<ElementSignature>,
    desired: Vec<(Arc<str>, Any)>,
    operation_index: usize,
    first_order: usize,
}

#[derive(Debug, Clone)]
struct VirtualStateCheckpoint {
    document: Document,
    targets: Vec<ResolvedText>,
    structural_parents: HashMap<Vec<u32>, StructuralParentTarget>,
    actions: Vec<ActionSlot>,
    prepared_inserts: Vec<Option<PendingPreparedInsert>>,
    prepared_elements: HashMap<Vec<u32>, PreparedHandle>,
    created_gap_shifts: HashMap<BranchID, Vec<u32>>,
    pending_element_attrs: HashMap<BranchID, PendingElementAttrs>,
}

#[derive(Debug, Clone)]
enum StorageChildKind {
    Text {
        scalar_len: u32,
        target: XmlTextRef,
        signature: TargetSignature,
        runs: Vec<PreparedTextRun>,
    },
    Element {
        target: XmlElementRef,
        signature: Arc<ElementSignature>,
    },
    PreparedElement,
}

#[derive(Debug, Clone)]
enum StorageInsertion {
    Boundary(u32),
    InsideText {
        child_index: u32,
        local_scalar: u32,
        target: XmlTextRef,
        signature: TargetSignature,
        runs: Vec<PreparedTextRun>,
    },
}

#[derive(Debug, Clone)]
struct VirtualStructuralSplice {
    parent_path: Vec<u32>,
    semantic_index: u32,
    semantic_delete: u32,
    semantic_insert: u32,
    storage_index: u32,
    storage_delete: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextRangeDisposition {
    Applied,
    Structural,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MutationCompilerBuild {
    Localized,
    EagerFallback,
}

#[derive(Clone, Copy)]
pub(crate) struct LocalizedInsertLocator<'a> {
    pub(crate) document: &'a Document,
    pub(crate) block_path: &'a [u32],
    pub(crate) position: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct LocalizedFormatLocator<'a> {
    document: &'a Document,
    block_path: &'a [u32],
    from: u32,
    to: u32,
    seed: &'a MutationLookupSeed,
}

pub(crate) struct LocalizedRootWindowLocator<'a> {
    document: &'a Document,
    expected_preview: &'a Document,
    from_child: u32,
    to_child: u32,
    expected_content: Fragment,
    seed: &'a MutationLookupSeed,
}

#[derive(Debug)]
pub(crate) struct MutationCompiler {
    request_id: u64,
    document_guard: DocumentGuard,
    targets: Vec<ResolvedText>,
    structural_parents: HashMap<Vec<u32>, StructuralParentTarget>,
    actions: Vec<ActionSlot>,
    prepared_inserts: Vec<Option<PendingPreparedInsert>>,
    prepared_elements: HashMap<Vec<u32>, PreparedHandle>,
    charged_work: usize,
    pending_traversal_work: usize,
    localized_position_target_count: Option<usize>,
    explicit_path_parent_widths: Option<HashMap<BranchID, usize>>,
    action_limit: usize,
    scan_work: usize,
    scan_limit: usize,
    #[cfg(test)]
    position_resolver_work: usize,
    created_gap_shifts: HashMap<BranchID, Vec<u32>>,
    pending_element_attrs: HashMap<BranchID, PendingElementAttrs>,
    wrap_checkpoints: HashMap<usize, VirtualStateCheckpoint>,
    #[cfg(test)]
    virtual_delete_visits: usize,
}

/// Authoritative, document-scoped metadata captured by the lifecycle that
/// owns the derived editor view. It contains only the global facts a
/// localized insert cannot recover without walking the complete Yrs tree.
#[derive(Debug, Clone)]
pub(crate) struct MutationLookupSeed {
    binding: MutationLookupBinding,
    state: MutationLookupSeedState,
}

#[derive(Debug, Clone)]
struct MutationLookupBinding {
    source_document: Document,
    canonical_artifact: Option<CanonicalArtifact>,
    resource_limits: ResourceLimits,
    editing_limits: EditingLimits,
    max_length: Option<u32>,
    store_token: usize,
    fragment_id: BranchID,
    schema_fingerprint: Arc<str>,
    yrs_state_epoch: u64,
    document_revision: u64,
    /// Exact Yrs state/delete-set evidence exists only while an unavailable
    /// history-candidate capability awaits one-shot publication.
    history_store_snapshot: Option<HistoryStoreSnapshotEvidence>,
}

#[derive(Debug, Clone)]
pub(crate) struct HistoryStoreSnapshotEvidence {
    snapshot: Arc<yrs::Snapshot>,
    /// Exact clock-scan work admitted against maxEncodedStateBytes before the
    /// proportional Yrs snapshot allocation was allowed to occur.
    admitted_clock_scan_work: usize,
}

#[derive(Debug, Clone)]
enum MutationLookupSeedState {
    Ready(MutationLookupPayload),
    Unavailable,
}

#[derive(Debug, Clone)]
struct MutationLookupPayload {
    target_count: usize,
    pending_traversal_work: usize,
    path_parent_widths: Arc<HashMap<BranchID, usize>>,
    target_materialization_work: TargetMaterializationWork,
}

#[derive(Debug, Clone)]
struct TargetMaterializationWork {
    base: Arc<HashMap<BranchID, usize>>,
    replacement: Option<(BranchID, usize)>,
}

impl TargetMaterializationWork {
    fn new(entries: HashMap<BranchID, usize>) -> Self {
        Self {
            base: Arc::new(entries),
            replacement: None,
        }
    }

    fn get(&self, key: &BranchID) -> Option<&usize> {
        if let Some((target, work)) = &self.replacement {
            if target == key {
                return Some(work);
            }
        }
        self.base.get(key)
    }

    fn iter(&self) -> impl Iterator<Item = (&BranchID, &usize)> {
        self.base.iter().map(|(key, value)| {
            let value = match &self.replacement {
                Some((target, work)) if target == key => work,
                _ => value,
            };
            (key, value)
        })
    }

    fn materialize(&self) -> Result<HashMap<BranchID, usize>, std::collections::TryReserveError> {
        let mut entries = HashMap::new();
        entries.try_reserve(self.base.len())?;
        entries.extend(self.iter().map(|(key, value)| (key.clone(), *value)));
        Ok(entries)
    }

    fn effective_map(
        &self,
        request_id: u64,
    ) -> OperationResult<std::borrow::Cow<'_, HashMap<BranchID, usize>>> {
        if self.replacement.is_none() {
            Ok(std::borrow::Cow::Borrowed(&self.base))
        } else {
            self.materialize()
                .map(std::borrow::Cow::Owned)
                .map_err(|_| lookup_seed_allocation_error(request_id, "mapGrowth"))
        }
    }

    fn promoted(&self, promotion: &MutationLookupPromotion) -> OperationResult<Self> {
        let mismatch = || {
            OperationError::engine_invariant_failed(
                promotion.request_id,
                None,
                "localized mutation lookup promotion does not match its seed",
            )
        };
        if promotion.source == MutationLookupPromotionSource::ExistingInsert {
            if let [(target, old_work, new_work)] =
                promotion.materialization_work_updates.as_slice()
            {
                if self.get(target) != Some(old_work) {
                    return Err(mismatch());
                }
                if self
                    .replacement
                    .as_ref()
                    .is_none_or(|(previous, _)| previous == target)
                {
                    return Ok(Self {
                        base: Arc::clone(&self.base),
                        replacement: Some((target.clone(), *new_work)),
                    });
                }
            }
        }
        let allocation_error = || {
            OperationError::engine_invariant_failed(
                promotion.request_id,
                None,
                "localized mutation lookup promotion allocation failed",
            )
        };
        if lookup_seed_hydration_should_fail("promotionMapReservation") {
            return Err(allocation_error());
        }
        let mut entries = self.materialize().map_err(|_| allocation_error())?;
        for (target, old_work, new_work) in &promotion.materialization_work_updates {
            if entries.get(target) != Some(old_work) {
                return Err(mismatch());
            }
            entries.insert(target.clone(), *new_work);
        }
        Ok(Self::new(entries))
    }
}

impl PartialEq for TargetMaterializationWork {
    fn eq(&self, other: &Self) -> bool {
        self.base.len() == other.base.len()
            && self
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

/// Opaque, one-owner payload collected while the validated codec projection
/// is already walking the exact candidate store.
pub(crate) struct ImportLookupMaterialization(MutationLookupPayload);

impl ImportLookupMaterialization {
    fn new(payload: MutationLookupPayload) -> Self {
        Self(payload)
    }

    /// Whether retaining an exact import replica can accelerate at least one
    /// localized mutation target. Zero-target payloads still prove a valid
    /// traversal, but a later mutation would have to take the ordinary
    /// structural path regardless of whether the replica was retained.
    pub(crate) fn accelerates_localized_mutation(&self) -> bool {
        self.0.target_count != 0
    }
}

#[derive(Debug)]
pub(crate) struct LocalizedInsertCompiler {
    compiler: MutationCompiler,
}

/// A capability for formatting exactly one non-empty range inside one
/// existing, non-void textblock. Its surface deliberately cannot perform
/// insertions, deletions, or structural edits.
#[derive(Debug)]
pub(crate) struct LocalizedFormatCompiler {
    compiler: MutationCompiler,
    seed_pending_traversal_work: usize,
    seed_materialization_work: TargetMaterializationWork,
}

/// A capability for replacing exactly one complete child window of the
/// semantic/Yrs document root. It cannot address text or descendant parents.
#[derive(Debug)]
pub(crate) struct LocalizedRootWindowCompiler {
    compiler: MutationCompiler,
    document: Document,
    expected_preview: Document,
    from_child: u32,
    to_child: u32,
    expected_content: Fragment,
}

#[derive(Debug, Clone)]
pub(crate) struct MutationLookupPromotion {
    request_id: u64,
    source: MutationLookupPromotionSource,
    materialization_work_updates: Vec<(BranchID, usize, usize)>,
    next_pending_traversal_work: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MutationLookupPromotionSource {
    ExistingInsert,
    ExistingFormat,
}
