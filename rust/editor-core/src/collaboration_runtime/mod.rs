//! Collaboration runtime host.

pub mod awareness;
pub mod outbox;
pub mod protocol;
pub mod state;

pub(crate) use outbox::CollaborationOutbox;

use crate::session::CollaborationLimits;

pub(crate) struct CollaborationRuntime {
    outbox: CollaborationOutbox,
    remote_dependency_work: u64,
    awareness: awareness::AwarenessRuntimeState,
}

impl CollaborationRuntime {
    pub(crate) fn new(limits: &CollaborationLimits) -> Self {
        Self {
            outbox: CollaborationOutbox::from_limits(limits),
            remote_dependency_work: 0,
            awareness: awareness::AwarenessRuntimeState::new(),
        }
    }

    pub(crate) fn outbox(&self) -> &CollaborationOutbox {
        &self.outbox
    }

    pub(crate) fn outbox_mut(&mut self) -> &mut CollaborationOutbox {
        &mut self.outbox
    }

    /// Accumulated dependency-quarantine work in encoded-byte units.
    pub(crate) fn remote_dependency_work(&self) -> u64 {
        self.remote_dependency_work
    }

    pub(crate) fn reset_for_restore(&mut self) {
        self.outbox.release_lease();
        self.outbox.clear_protocol_replies();
        self.remote_dependency_work = 0;
        self.awareness.reset_for_restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_hosts_an_outbox_sized_from_the_session_limits() {
        let limits = CollaborationLimits::default();
        let mut runtime = CollaborationRuntime::new(&limits);
        assert!(!runtime.outbox().has_pending_document_updates());
        assert_eq!(runtime.remote_dependency_work(), 0);
        let reservation = runtime.outbox_mut().reserve_document_update(1, 4).unwrap();
        runtime.outbox_mut().install(reservation, vec![0; 4]);
        assert_eq!(runtime.outbox().pending_document_update_count(), 1);
    }

    #[test]
    fn reset_for_restore_drops_prior_store_residue_and_keeps_document_entries() {
        let limits = CollaborationLimits::default();
        let mut runtime = CollaborationRuntime::new(&limits);
        runtime.remote_dependency_work = 512;
        let replies = runtime.outbox_mut().reserve_protocol_replies(1, 6).unwrap();
        runtime
            .outbox_mut()
            .install_protocol_replies(replies, 7, vec![vec![1; 6]]);
        let document = runtime.outbox_mut().reserve_document_update(2, 4).unwrap();
        runtime.outbox_mut().install(document, vec![2; 4]);

        runtime.reset_for_restore();

        assert_eq!(runtime.remote_dependency_work(), 0);
        assert_eq!(runtime.outbox().pending_protocol_reply_count(), 0);
        assert_eq!(runtime.outbox().pending_protocol_reply_bytes(), 0);
        // The session gate makes pending document updates impossible at
        // restore time; the reset never touches them defensively either.
        assert_eq!(runtime.outbox().pending_document_update_count(), 1);
        assert_eq!(runtime.outbox().pending_document_update_bytes(), 4);
    }
}
