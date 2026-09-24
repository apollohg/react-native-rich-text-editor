impl EditorSession {
    pub(crate) fn get_json(&self) -> Result<serde_json::Value, SessionError> {
        self.engine.document_json().ok_or_else(engine_not_ready)
    }

    pub(crate) fn get_json_string(&self) -> Result<String, SessionError> {
        self.engine
            .document_json_string()
            .ok_or_else(engine_not_ready)
    }

    pub(crate) fn get_html(&self) -> Result<String, SessionError> {
        self.engine.document_html().ok_or_else(engine_not_ready)
    }

    pub(crate) fn replace_document_json(
        &mut self,
        request_id: u64,
        json: &str,
        history: crate::yrs_engine::ReplacementHistory,
    ) -> Result<crate::yrs_engine::TransactionCommit, SessionError> {
        self.admit_whole_document_replacement(request_id)?;
        let (engine, outbox) = split_engine_and_outbox(&mut self.engine, &mut self.collaboration);
        engine
            .prepare_root_replacement_json_with_outbox(request_id, json, history, outbox)
            .map_err(|error| replacement_session_error(error, request_id))
    }

    pub(crate) fn replace_document_html(
        &mut self,
        request_id: u64,
        html: &str,
        history: crate::yrs_engine::ReplacementHistory,
    ) -> Result<crate::yrs_engine::TransactionCommit, SessionError> {
        self.admit_whole_document_replacement(request_id)?;
        let options = crate::serialize::FromHtmlOptions {
            strict: false,
            allow_base64_images: self.policy.allow_base64_images,
        };
        let (engine, outbox) = split_engine_and_outbox(&mut self.engine, &mut self.collaboration);
        engine
            .prepare_root_replacement_html_with_outbox(request_id, html, &options, history, outbox)
            .map_err(|error| replacement_session_error(error, request_id))
    }

    /// Export remains available while connected.
    pub(crate) fn export_snapshot(
        &self,
        request_id: u64,
    ) -> Result<DocumentSnapshot, SessionError> {
        self.engine
            .export_snapshot()
            .map_err(|error| snapshot_session_error(error, request_id))
    }

    /// Failed restores leave the session unchanged.
    pub(crate) fn restore_snapshot(
        &mut self,
        request_id: u64,
        snapshot: &DocumentSnapshot,
    ) -> Result<crate::yrs_engine::EngineCommit, SessionError> {
        self.admit_snapshot_restore(request_id)?;
        let commit = self
            .engine
            .restore_snapshot(snapshot)
            .map_err(|error| snapshot_session_error(error, request_id))?;
        self.position_epochs.clear();
        self.native_request_ledgers.clear();
        self.native_render_cursors.clear();
        if self.document_state == DocumentState::AwaitRemote {
            self.document_state = DocumentState::RoomReady;
        }
        if self.room_bound() {
            self.collaboration.transport.settle_for_restore();
        }
        if let Some(runtime) = self.collaboration.runtime.as_mut() {
            runtime.reset_for_restore();
        }
        Ok(commit)
    }

    fn admit_snapshot_restore(&self, request_id: u64) -> Result<(), SessionError> {
        match self.transport_state() {
            TransportState::Detached | TransportState::Disconnected => {}
            TransportState::Connecting
            | TransportState::Handshaking
            | TransportState::Synchronized
            | TransportState::Incompatible => {
                let mut error = SessionError::new(
                    ErrorDomain::Snapshot,
                    "SNAPSHOT_RESTORE_CONNECTED",
                    format!(
                        "snapshot restore is rejected while the collaboration \
                         transport is {}",
                        self.transport_state().as_str()
                    ),
                );
                error.request_id = Some(request_id);
                return Err(error);
            }
            TransportState::Destroying | TransportState::Destroyed => unreachable!(
                "with_alive rejects destroying/destroyed sessions before policy evaluation"
            ),
        }
        if let Some(outbox) = self.collaboration_outbox() {
            if outbox.has_pending_document_updates() {
                let mut error = SessionError::new(
                    ErrorDomain::Snapshot,
                    "SNAPSHOT_OUTBOX_NOT_EMPTY",
                    "snapshot restore is rejected while unsent local document \
                     updates are pending in the collaboration outbox",
                );
                error.request_id = Some(request_id);
                return Err(error);
            }
        }
        Ok(())
    }
}
