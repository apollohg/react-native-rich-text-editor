use super::*;

const MAX_AUDIT_BYTES: usize = 16 * 1024 * 1024;
const MAX_AUDIT_ITEMS: usize = 65_536;

fn charge_ids(set: &IdSet, units: &mut u64, ranges: &mut usize) -> Option<()> {
    for (_, entries) in set.iter() {
        for range in entries {
            *ranges = ranges.checked_add(1)?;
            *units = units.checked_add(u64::from(range.end.checked_sub(range.start)?))?;
            if *ranges > MAX_AUDIT_ITEMS || *units > MAX_AUDIT_ITEMS as u64 {
                return None;
            }
        }
    }
    Some(())
}

#[derive(Debug, PartialEq, Eq)]
struct FrozenMetadata {
    wrapper_identity: Option<usize>,
    before: Option<(usize, Option<HistorySnapshot>)>,
    after: Option<(usize, Option<HistorySnapshot>)>,
}

impl FrozenMetadata {
    fn retained_bytes(slots: &HistoryMetadataSlots) -> Option<usize> {
        [&slots.before, &slots.after]
            .into_iter()
            .flatten()
            .try_fold(0usize, |total, slot| {
                total.checked_add(slot.get().map_or(0, |snapshot| snapshot.metadata_bytes))
            })
    }

    fn slots(slots: HistoryMetadataSlots) -> Self {
        Self {
            wrapper_identity: None,
            before: slots
                .before
                .map(|slot| (slot.identity(), slot.get().cloned())),
            after: slots
                .after
                .map(|slot| (slot.identity(), slot.get().cloned())),
        }
    }

    fn capture(metadata: &HistoryMetadata) -> Self {
        let mut frozen = Self::slots(metadata.slots());
        frozen.wrapper_identity = Some(Arc::as_ptr(&metadata.0) as usize);
        frozen
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FrozenEvent {
    Recorded {
        update: Vec<u8>,
        origin: TransactionOrigin,
        policy: HistoryPolicy,
        class: HistoryClass,
        undo_units_bound: u64,
        capture_millis: u64,
        metadata: FrozenMetadata,
    },
    Excluded {
        update: Vec<u8>,
        origin: TransactionOrigin,
        work_units: u64,
    },
    Action(HistoryAction),
    Boundary,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AvailabilityHistoryAudit {
    manager: yrs::undo::HistoryAudit<FrozenMetadata>,
    clock_identity: usize,
    clock_latched: Option<u64>,
    capture: (usize, Option<FrozenMetadata>),
    pop: (usize, Option<FrozenMetadata>),
    popped: (usize, Option<(EventKind, FrozenMetadata)>),
    last_capture_millis: Option<u64>,
    last_class: Option<HistoryClass>,
    last_origin: Option<TransactionOrigin>,
    force_next_boundary: bool,
    baseline: Vec<u8>,
    events: Vec<FrozenEvent>,
    replay_bytes: usize,
    replay_work_units: u64,
    replay_metadata_bytes: usize,
    max_encoded_state_bytes: usize,
    rebase_before_next_event: bool,
    recording_replay_events: bool,
    redone_chains: Vec<(IdSet, IdSet)>,
}

impl AvailabilityHistoryAudit {
    pub(crate) fn counts(&self) -> [usize; 3] {
        [
            self.manager.undo.len(),
            self.manager.redo.len(),
            self.events.len(),
        ]
    }
}

impl YrsHistory {
    pub(crate) fn availability_audit(&self) -> Option<AvailabilityHistoryAudit> {
        // The peer observes settled command boundaries, never an in-flight capture.
        if self.pending_replay_event.is_some() {
            return None;
        }
        let items = self
            .manager
            .undo_stack()
            .len()
            .checked_add(self.manager.redo_stack().len())?
            .checked_add(self.replay_events.len())?;
        let bytes = self
            .epoch_baseline
            .len()
            .checked_add(self.replay_bytes)?
            .checked_add(self.replay_metadata_bytes)?;
        if items > MAX_AUDIT_ITEMS || bytes > MAX_AUDIT_BYTES {
            return None;
        }
        let mut units = 0;
        let mut ranges = 0;
        for item in self
            .manager
            .undo_stack()
            .iter()
            .chain(self.manager.redo_stack())
        {
            charge_ids(item.insertions(), &mut units, &mut ranges)?;
            charge_ids(item.deletions(), &mut units, &mut ranges)?;
        }
        for chain in &self.redone_chains {
            charge_ids(&chain.originals, &mut units, &mut ranges)?;
            charge_ids(&chain.copies, &mut units, &mut ranges)?;
        }
        let mut metadata_bytes = 0usize;
        for item in self
            .manager
            .undo_stack()
            .iter()
            .chain(self.manager.redo_stack())
        {
            metadata_bytes = metadata_bytes
                .checked_add(FrozenMetadata::retained_bytes(&item.meta().slots())?)?;
        }
        for event in &self.replay_events {
            if let ReplayEvent::Recorded { metadata, .. } = event {
                metadata_bytes = metadata_bytes
                    .checked_add(FrozenMetadata::retained_bytes(&metadata.slots())?)?;
            }
        }
        for slot in [&self.pending_capture, &self.pending_pop] {
            if let Some(metadata) = slot.lock().ok()?.as_ref() {
                metadata_bytes = metadata_bytes
                    .checked_add(FrozenMetadata::retained_bytes(&metadata.slots())?)?;
            }
        }
        if let Some((_, slots)) = self.popped.lock().ok()?.as_ref() {
            metadata_bytes = metadata_bytes.checked_add(FrozenMetadata::retained_bytes(slots)?)?;
        }
        if units > MAX_AUDIT_ITEMS as u64 || metadata_bytes > MAX_AUDIT_BYTES {
            return None;
        }
        let freeze_pending = |slot: &Arc<Mutex<Option<HistoryMetadata>>>| {
            Some((
                Arc::as_ptr(slot) as usize,
                slot.lock().ok()?.as_ref().map(FrozenMetadata::capture),
            ))
        };
        let events = self
            .replay_events
            .iter()
            .map(|event| match event {
                ReplayEvent::Recorded {
                    update,
                    origin,
                    policy,
                    class,
                    undo_units_bound,
                    capture_millis,
                    metadata,
                } => FrozenEvent::Recorded {
                    update: update.clone(),
                    origin: *origin,
                    policy: *policy,
                    class: *class,
                    undo_units_bound: *undo_units_bound,
                    capture_millis: *capture_millis,
                    metadata: FrozenMetadata::capture(metadata),
                },
                ReplayEvent::Excluded {
                    update,
                    origin,
                    work_units,
                } => FrozenEvent::Excluded {
                    update: update.clone(),
                    origin: *origin,
                    work_units: *work_units,
                },
                ReplayEvent::Action(action) => FrozenEvent::Action(*action),
                ReplayEvent::Boundary => FrozenEvent::Boundary,
            })
            .collect();
        Some(AvailabilityHistoryAudit {
            manager: self.manager.history_audit(FrozenMetadata::capture),
            clock_identity: Arc::as_ptr(&self.clock) as usize,
            clock_latched: *self.clock.latched.lock().ok()?,
            capture: freeze_pending(&self.pending_capture)?,
            pop: freeze_pending(&self.pending_pop)?,
            popped: (
                Arc::as_ptr(&self.popped) as usize,
                self.popped
                    .lock()
                    .ok()?
                    .as_ref()
                    .map(|(kind, slots)| (*kind, FrozenMetadata::slots(slots.clone()))),
            ),
            last_capture_millis: self.last_capture_millis,
            last_class: self.last_class,
            last_origin: self.last_origin,
            force_next_boundary: self.force_next_boundary,
            baseline: self.epoch_baseline.clone(),
            events,
            replay_bytes: self.replay_bytes,
            replay_work_units: self.replay_work_units,
            replay_metadata_bytes: self.replay_metadata_bytes,
            max_encoded_state_bytes: self.max_encoded_state_bytes,
            rebase_before_next_event: self.rebase_before_next_event,
            recording_replay_events: self.recording_replay_events,
            redone_chains: self
                .redone_chains
                .iter()
                .map(|chain| (chain.originals.clone(), chain.copies.clone()))
                .collect(),
        })
    }
}
