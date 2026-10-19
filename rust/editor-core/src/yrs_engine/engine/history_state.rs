use crate::boundary::ResourceLimits;
use crate::yrs_engine;
use crate::yrs_engine::canonical::CanonicalArtifact;
use crate::yrs_engine::derived_state::DerivedStateCache;
use crate::yrs_engine::{DocumentScope, YrsEngineError};

#[allow(clippy::too_many_arguments)]
pub(super) fn history_local_state(
    state: &DerivedStateCache,
    fragment_name: &str,
    scope: Option<&DocumentScope>,
    resource_limits: &ResourceLimits,
    editing_limits: &yrs_engine::EditingLimits,
    max_length: Option<u32>,
    initial_selection: Option<(&yrs_engine::ResolvedSelection, &crate::schema::Schema)>,
    document_snapshot_retained_bytes: Option<
        yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes,
    >,
) -> yrs_engine::history::HistoryLocalState {
    let stored_marks = match initial_selection {
        Some((selection, schema)) => {
            yrs_engine::derived_state::stored_marks_after_selection_change(
                state.stored_marks.as_deref(),
                &state.resolved_selection,
                selection,
                &state.document,
                schema,
            )
        }
        None => state.stored_marks.clone(),
    };
    let document_snapshot = document_snapshot_retained_bytes.map(|retained_bytes| {
        state.capture_history_document_snapshot(
            resource_limits,
            editing_limits,
            max_length,
            fragment_name,
            scope,
            retained_bytes,
        )
    });
    let metadata_bytes = history_metadata_bytes(stored_marks.as_deref(), fragment_name)
        .saturating_add(
            document_snapshot
                .as_deref()
                .map(yrs_engine::derived_state::HistoryDocumentSnapshot::retained_bytes)
                .unwrap_or(0),
        );
    yrs_engine::history::HistoryLocalState {
        relative_selection: state.relative_selection.clone(),
        resolved_selection: state.resolved_selection.clone(),
        stored_marks,
        text_length: state.canonical_artifact.text_scalar_len(),
        canonical_fingerprint: yrs_engine::history::HistoryCanonicalIdentity::from_artifact(
            &state.canonical_artifact,
            document_snapshot.is_some(),
        ),
        derived_output_bytes: state.canonical_artifact.serialized_len(),
        metadata_bytes,
        document_snapshot,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct HistoryDocumentSnapshotRetainedPair {
    pub(super) before: yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes,
    pub(super) after: yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes,
}

pub(super) struct HistorySnapshotAdmission<'a> {
    pub(super) resource_limits: &'a ResourceLimits,
    pub(super) editing_limits: &'a yrs_engine::EditingLimits,
    pub(super) max_length: Option<u32>,
    pub(super) schema_fingerprint: &'a str,
    pub(super) fragment_name: &'a str,
    pub(super) scope: Option<&'a DocumentScope>,
}

pub(super) struct HistorySnapshotsFit {
    pub(super) retained: Option<HistoryDocumentSnapshotRetainedPair>,
    pub(super) after_standalone: Option<usize>,
}

pub(super) fn history_document_snapshots_fit(
    before: &DerivedStateCache,
    after: &yrs_engine::derived_state::HistorySnapshotParts<'_>,
    after_stored_marks: Option<&[crate::model::Mark]>,
    group_before: Option<&yrs_engine::derived_state::HistoryDocumentSnapshot>,
    admission: &HistorySnapshotAdmission<'_>,
) -> HistorySnapshotsFit {
    let unavailable = HistorySnapshotsFit {
        retained: None,
        after_standalone: None,
    };
    let HistorySnapshotAdmission {
        resource_limits,
        editing_limits,
        max_length,
        schema_fingerprint,
        fragment_name,
        scope,
    } = *admission;
    let Some(stored_marks_bytes) =
        history_metadata_bytes(before.stored_marks.as_deref(), fragment_name)
            .checked_add(history_metadata_bytes(after_stored_marks, fragment_name))
    else {
        return unavailable;
    };
    let before_parts = before.history_snapshot_parts();
    let captured = before.captured_history_document_snapshot(
        resource_limits,
        editing_limits,
        max_length,
        fragment_name,
        scope,
    );
    let before_standalone = match captured {
        Some(captured) => Some(captured.standalone_retained_bytes()),
        None => before.history_snapshot_standalone_bytes(|| {
            let document_charge = before
                .canonical_artifact
                .history_snapshot_retained_charge()?
                .canonical_retained_bytes;
            if stored_marks_bytes.checked_add(document_charge)?
                > editing_limits.max_derived_output_bytes
            {
                return None;
            }
            yrs_engine::derived_state::history_document_snapshot_standalone_retained_bytes(
                &before_parts,
                schema_fingerprint,
                fragment_name,
                scope,
            )
        }),
    };
    let Some(before_standalone) = before_standalone else {
        return unavailable;
    };
    let before_charge = captured.map_or(before_standalone, |captured| captured.retained_bytes());
    let beyond = |parts, base| {
        yrs_engine::derived_state::history_document_snapshot_retained_bytes_beyond(
            parts,
            base,
            schema_fingerprint,
            fragment_name,
            scope,
        )
    };
    let Some(after_beyond_before) = beyond(after, &before_parts) else {
        return unavailable;
    };
    let after_standalone = beyond(&before_parts, after).and_then(|before_beyond_after| {
        before_standalone
            .checked_add(after_beyond_before)?
            .checked_sub(before_beyond_after)
    });
    let after_charge = match group_before {
        Some(group_before) => beyond(after, &group_before.parts())
            .map(|group_beyond| after_beyond_before.max(group_beyond)),
        None => Some(after_beyond_before),
    };
    let retained = after_standalone
        .zip(after_charge)
        .filter(|(_, after_charge)| {
            stored_marks_bytes
                .checked_add(before_standalone)
                .and_then(|bytes| bytes.checked_add(*after_charge))
                .is_some_and(|total| total <= editing_limits.max_derived_output_bytes)
        })
        .map(|(after_standalone, after_charge)| HistoryDocumentSnapshotRetainedPair {
            before:
                yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes::beyond_retained_base(
                    before_charge,
                    before_standalone,
                ),
            after:
                yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes::beyond_retained_base(
                    after_charge,
                    after_standalone,
                ),
        });
    HistorySnapshotsFit {
        retained,
        after_standalone,
    }
}

pub(super) fn history_snapshot_template(
    canonical_artifact: &CanonicalArtifact,
    stored_marks: Option<&[crate::model::Mark]>,
    fragment_name: &str,
    document_snapshot_retained_bytes: Option<
        yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes,
    >,
) -> yrs_engine::history::HistorySnapshotTemplate {
    history_snapshot_template_from_identity(
        canonical_artifact.text_scalar_len(),
        yrs_engine::history::HistoryCanonicalIdentity::from_artifact(
            canonical_artifact,
            document_snapshot_retained_bytes.is_some(),
        ),
        canonical_artifact.serialized_len(),
        stored_marks,
        fragment_name,
        document_snapshot_retained_bytes,
    )
}

pub(super) fn history_snapshot_template_from_identity(
    text_length: u64,
    canonical_fingerprint: yrs_engine::history::HistoryCanonicalIdentity,
    derived_output_bytes: usize,
    stored_marks: Option<&[crate::model::Mark]>,
    fragment_name: &str,
    document_snapshot_retained_bytes: Option<
        yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes,
    >,
) -> yrs_engine::history::HistorySnapshotTemplate {
    let retained_bytes = document_snapshot_retained_bytes
        .map(yrs_engine::derived_state::HistoryDocumentSnapshotRetainedBytes::get)
        .unwrap_or(0);
    yrs_engine::history::HistorySnapshotTemplate {
        stored_marks: stored_marks.map(<[crate::model::Mark]>::to_vec),
        text_length,
        canonical_fingerprint,
        derived_output_bytes,
        metadata_bytes: history_metadata_bytes(stored_marks, fragment_name)
            .saturating_add(retained_bytes),
        document_snapshot_retained_bytes,
    }
}

pub(super) fn history_metadata_bytes(
    stored_marks: Option<&[crate::model::Mark]>,
    fragment_name: &str,
) -> usize {
    // Fixed selection metadata covers the bounded relative/resolved selection
    // representation. Stored marks are cloned deeply, so charge their source
    // container and recursive capacities rather than serialized logical size.
    const FIXED_SELECTION_BYTES: usize = 512;
    const EMPTY_MARKS_SEQUENCE_BYTES: usize = 2;
    const PROJECTION_FINGERPRINT_BYTES: usize = std::mem::size_of::<[u8; 32]>();
    let stored_marks = stored_marks.unwrap_or_default();
    let mark_bytes = stored_marks
        .len()
        .checked_mul(std::mem::size_of::<crate::model::Mark>())
        .and_then(|slots| {
            stored_marks.iter().try_fold(slots, |total, mark| {
                total.checked_add(mark.history_snapshot_clone_retained_bytes()?)
            })
        })
        .unwrap_or(usize::MAX);
    FIXED_SELECTION_BYTES
        .checked_add(PROJECTION_FINGERPRINT_BYTES)
        .and_then(|bytes| bytes.checked_add(fragment_name.len()))
        .and_then(|bytes| bytes.checked_add(EMPTY_MARKS_SEQUENCE_BYTES))
        .and_then(|bytes| bytes.checked_add(mark_bytes))
        .unwrap_or(usize::MAX)
}

pub(super) fn history_operation_error(
    request_id: u64,
    error: YrsEngineError,
) -> yrs_engine::OperationError {
    if let (Some(limit), Some(actual)) = (error.limit, error.actual) {
        let field = if error.code == "INPUT_LIMIT_EXCEEDED" {
            "maxEncodedStateBytes"
        } else {
            "document"
        };
        yrs_engine::OperationError::document_limit_exceeded(
            request_id,
            None,
            field,
            u64::try_from(limit).unwrap_or(u64::MAX),
            u64::try_from(actual).unwrap_or(u64::MAX),
        )
    } else {
        yrs_engine::OperationError::engine_invariant_failed(request_id, None, error.message)
    }
}

impl super::YrsDocumentEngine {
    pub(super) fn history_snapshot_admission(&self) -> HistorySnapshotAdmission<'_> {
        HistorySnapshotAdmission {
            resource_limits: &self.resource_limits,
            editing_limits: &self.editing_limits,
            max_length: self.max_length,
            schema_fingerprint: &self.schema_fingerprint,
            fragment_name: &self.fragment_name,
            scope: self.scope.as_ref(),
        }
    }
}
