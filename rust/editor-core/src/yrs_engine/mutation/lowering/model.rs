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

const REPLACED_PREPARED_NODES: u32 = 1;

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
    PreparedChild {
        handle: PreparedHandle,
        scalar_len: Option<u32>,
    },
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
    InsidePreparedText {
        child_index: u32,
        handle: PreparedHandle,
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
    storage_insert: Option<Vec<StorageChildKind>>,
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

#[derive(Clone, Copy)]
pub(crate) struct LocalizedSplitLocator<'a> {
    pub(crate) document: &'a Document,
    pub(crate) position: u32,
}

#[derive(Debug, Clone, Copy)]
struct StructuralWindowComplement {
    root_child: usize,
    root_children: usize,
    parent_count: usize,
    target_count: usize,
    parent_layout_work: usize,
    content_position_work: usize,
}

pub(crate) struct LocalizedRootWindowLocator<'a> {
    document: &'a Document,
    expected_preview: Option<&'a Document>,
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
    structural_window_complement: Option<StructuralWindowComplement>,
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
    path_parent_widths: LookupEntries,
    target_materialization_work: LookupEntries,
}

#[derive(Debug, Clone)]
struct LookupEntries {
    base: Arc<HashMap<BranchID, usize>>,
    overrides: Arc<HashMap<BranchID, Option<usize>>>,
    len: usize,
}

const LOOKUP_OVERRIDE_COMPACTION_FLOOR: usize = 64;

impl LookupEntries {
    fn new(entries: HashMap<BranchID, usize>) -> Self {
        Self {
            len: entries.len(),
            base: Arc::new(entries),
            overrides: Arc::new(HashMap::new()),
        }
    }

    fn get(&self, key: &BranchID) -> Option<&usize> {
        match self.overrides.get(key) {
            Some(value) => value.as_ref(),
            None => self.base.get(key),
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn iter(&self) -> impl Iterator<Item = (&BranchID, &usize)> {
        self.base
            .iter()
            .filter(|(key, _)| !self.overrides.contains_key(*key))
            .chain(
                self.overrides
                    .iter()
                    .filter_map(|(key, value)| value.as_ref().map(|value| (key, value))),
            )
    }

    fn values(&self) -> impl Iterator<Item = &usize> {
        self.iter().map(|(_, value)| value)
    }

    fn materialize(&self) -> Result<HashMap<BranchID, usize>, std::collections::TryReserveError> {
        let mut entries = HashMap::new();
        entries.try_reserve(self.len)?;
        entries.extend(self.iter().map(|(key, value)| (key.clone(), *value)));
        Ok(entries)
    }

    fn compaction_limit(&self) -> usize {
        self.base
            .len()
            .isqrt()
            .max(LOOKUP_OVERRIDE_COMPACTION_FLOOR)
    }

    fn with_overrides(
        &self,
        request_id: u64,
        overrides: HashMap<BranchID, Option<usize>>,
        len: usize,
        compaction_stage: &'static str,
    ) -> OperationResult<Self> {
        let next = Self {
            base: Arc::clone(&self.base),
            overrides: Arc::new(overrides),
            len,
        };
        if next.overrides.len() <= self.compaction_limit() {
            return Ok(next);
        }
        if lookup_seed_hydration_should_fail(compaction_stage) {
            return Err(lookup_seed_allocation_error(request_id, compaction_stage));
        }
        next.materialize()
            .map(Self::new)
            .map_err(|_| lookup_seed_allocation_error(request_id, compaction_stage))
    }

    fn override_entries(
        &self,
        request_id: u64,
        additional: usize,
    ) -> OperationResult<HashMap<BranchID, Option<usize>>> {
        let capacity = self
            .overrides
            .len()
            .checked_add(additional)
            .ok_or_else(|| lookup_seed_allocation_error(request_id, "mapGrowth"))?;
        let mut overrides = HashMap::new();
        overrides
            .try_reserve(capacity)
            .map_err(|_| lookup_seed_allocation_error(request_id, "mapGrowth"))?;
        overrides.extend(
            self.overrides
                .iter()
                .map(|(key, value)| (key.clone(), *value)),
        );
        Ok(overrides)
    }

    fn set_override(
        &self,
        overrides: &mut HashMap<BranchID, Option<usize>>,
        key: BranchID,
        value: Option<usize>,
    ) {
        if self.base.get(&key).copied() == value {
            overrides.remove(&key);
        } else {
            overrides.insert(key, value);
        }
    }

    fn replaced(
        &self,
        request_id: u64,
        before: &LookupEntries,
        after: &LookupEntries,
    ) -> OperationResult<Self> {
        let mismatch = || {
            OperationError::engine_invariant_failed(
                request_id,
                None,
                "localized lookup subtree entries disagree with the retained payload",
            )
        };
        if before
            .iter()
            .any(|(key, value)| self.get(key) != Some(value))
            || after
                .iter()
                .any(|(key, _)| before.get(key).is_none() && self.get(key).is_some())
        {
            return Err(mismatch());
        }
        let len = self
            .len
            .checked_sub(before.len())
            .and_then(|len| len.checked_add(after.len()))
            .ok_or_else(mismatch)?;
        let additional = before.len().checked_add(after.len()).ok_or_else(mismatch)?;
        let mut overrides = self.override_entries(request_id, additional)?;
        for (key, _) in before.iter().filter(|(key, _)| after.get(key).is_none()) {
            self.set_override(&mut overrides, key.clone(), None);
        }
        for (key, value) in after.iter() {
            self.set_override(&mut overrides, key.clone(), Some(*value));
        }
        probe_lookup_seed_publication(
            request_id,
            "mapPublication",
            std::mem::size_of::<HashMap<BranchID, usize>>(),
        )?;
        self.with_overrides(request_id, overrides, len, "mapGrowth")
    }

    fn promoted(&self, promotion: &MutationLookupPromotion) -> OperationResult<Self> {
        let mismatch = || {
            OperationError::engine_invariant_failed(
                promotion.request_id,
                None,
                "localized mutation lookup promotion does not match its seed",
            )
        };
        let mut overrides = self.override_entries(
            promotion.request_id,
            promotion.materialization_work_updates.len(),
        )?;
        for (target, old_work, new_work) in &promotion.materialization_work_updates {
            let current = match overrides.get(target) {
                Some(value) => *value,
                None => self.base.get(target).copied(),
            };
            if current != Some(*old_work) {
                return Err(mismatch());
            }
            self.set_override(&mut overrides, target.clone(), Some(*new_work));
        }
        self.with_overrides(
            promotion.request_id,
            overrides,
            self.len,
            "promotionMapReservation",
        )
    }
}

impl PartialEq for LookupEntries {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
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

    /// Whether installing the import lookup seed can accelerate at least one
    /// localized mutation target. Zero-target payloads still prove a valid
    /// traversal, but a later mutation would have to take the ordinary
    /// structural path regardless of whether the seed was installed.
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
    seed_materialization_work: LookupEntries,
}

/// A capability for replacing exactly one complete child window of the
/// semantic/Yrs document root. It cannot address text or descendant parents.
#[derive(Debug)]
pub(crate) struct LocalizedRootWindowCompiler {
    compiler: MutationCompiler,
    document: Document,
    expected_preview: Option<Document>,
    from_child: u32,
    to_child: u32,
    expected_content: Fragment,
}

#[derive(Debug)]
pub(crate) struct LocalizedSplitCompiler {
    compiler: MutationCompiler,
    document: Document,
    position: u32,
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
