//! Test-only observability for document-wide work in the Yrs editing path.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FullPassCounts {
    pub import_model_parses: usize,
    pub validated_evidence_constructions: usize,
    pub validation_certificate_constructions: usize,
    pub planner_simulations: usize,
    pub document_validations: usize,
    pub canonical_mark_tree_scans: usize,
    pub canonical_mark_validation_attempts: usize,
    pub canonical_mark_validation_completions: usize,
    pub canonical_mark_nodes_visited: usize,
    pub canonical_identity_predicate_nodes_visited: usize,
    pub canonical_projections: usize,
    pub canonical_serializations: usize,
    pub canonical_hashes: usize,
    pub affected_top_level_scans: usize,
    pub position_map_clones: usize,
    pub position_map_compactions: usize,
    pub rendered_text_derivations: usize,
    pub raw_document_text_scans: usize,
    pub document_node_count_scans: usize,
    pub render_limit_tree_scans: usize,
    pub render_identity_scans: usize,
    pub render_top_level_start_scans: usize,
    pub active_applicability_passes: usize,
    pub ordinary_step_applications: usize,
    pub table_projection_derivations: usize,
    pub table_command_availability_plans: usize,
    pub yrs_tree_walks: usize,
    pub whole_state_encodings: usize,
    pub mutation_guard_snapshot_requests: usize,
    pub compilation_snapshot_scans: usize,
    pub compilation_snapshot_reuses: usize,
    pub cell_content_keys: usize,
    pub attribute_serializations: usize,
    pub epoch_block_rebuilds: usize,
    pub cell_content_generations: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreparedAdmissionCounts {
    pub staged_seed_preparations: usize,
    pub installed_base_seed_publications: usize,
    pub staged_identity_materializations: usize,
    pub deferred_capsules_created: usize,
    pub deferred_capsules_finalized: usize,
    pub eager_fallbacks: usize,
}

macro_rules! recorder {
    ($name:ident, $field:ident) => {
        #[inline]
        pub(crate) fn $name() {
            FULL_PASS_COUNTS.with(|counts| {
                let mut next = counts.get();
                next.$field = next.$field.saturating_add(1);
                counts.set(next);
            });
        }
    };
}

std::thread_local! {
    pub(crate) static HISTORY_OWNED_SLOT_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static HISTORY_STACK_METADATA_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static HISTORY_REPLAY_METADATA_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static PREFLIGHT_CHILDREN_ENUMERATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FULL_PASS_COUNTS: std::cell::Cell<FullPassCounts> = const {
        std::cell::Cell::new(FullPassCounts {
            import_model_parses: 0,
            validated_evidence_constructions: 0,
            validation_certificate_constructions: 0,
            planner_simulations: 0,
            document_validations: 0,
            canonical_mark_tree_scans: 0,
            canonical_mark_validation_attempts: 0,
            canonical_mark_validation_completions: 0,
            canonical_mark_nodes_visited: 0,
            canonical_identity_predicate_nodes_visited: 0,
            canonical_projections: 0,
            canonical_serializations: 0,
            canonical_hashes: 0,
            affected_top_level_scans: 0,
            position_map_clones: 0,
            position_map_compactions: 0,
            rendered_text_derivations: 0,
            raw_document_text_scans: 0,
            document_node_count_scans: 0,
            render_limit_tree_scans: 0,
            render_identity_scans: 0,
            render_top_level_start_scans: 0,
            active_applicability_passes: 0,
            ordinary_step_applications: 0,
            table_projection_derivations: 0,
            table_command_availability_plans: 0,
            yrs_tree_walks: 0,
            whole_state_encodings: 0,
            mutation_guard_snapshot_requests: 0,
            compilation_snapshot_scans: 0,
            compilation_snapshot_reuses: 0,
            cell_content_keys: 0,
            attribute_serializations: 0,
            epoch_block_rebuilds: 0,
            cell_content_generations: 0,
        })
    };
    static PREPARED_ADMISSION_COUNTS: std::cell::Cell<PreparedAdmissionCounts> = const {
        std::cell::Cell::new(PreparedAdmissionCounts {
            staged_seed_preparations: 0,
            installed_base_seed_publications: 0,
            staged_identity_materializations: 0,
            deferred_capsules_created: 0,
            deferred_capsules_finalized: 0,
            eager_fallbacks: 0,
        })
    };
}

recorder!(
    record_mutation_guard_snapshot_request,
    mutation_guard_snapshot_requests
);
recorder!(record_compilation_snapshot_scan, compilation_snapshot_scans);
recorder!(
    record_compilation_snapshot_reuse,
    compilation_snapshot_reuses
);
recorder!(record_planner_simulation, planner_simulations);
recorder!(record_import_model_parse, import_model_parses);
recorder!(
    record_validated_evidence_construction,
    validated_evidence_constructions
);
recorder!(
    record_validation_certificate_construction,
    validation_certificate_constructions
);
recorder!(record_document_validation, document_validations);
pub(crate) fn record_canonical_mark_validation_attempt() {
    FULL_PASS_COUNTS.with(|counts| {
        let mut next = counts.get();
        next.canonical_mark_tree_scans = next.canonical_mark_tree_scans.saturating_add(1);
        next.canonical_mark_validation_attempts =
            next.canonical_mark_validation_attempts.saturating_add(1);
        counts.set(next);
    });
}
recorder!(
    record_canonical_mark_validation_completion,
    canonical_mark_validation_completions
);
recorder!(
    record_canonical_mark_node_visited,
    canonical_mark_nodes_visited
);
recorder!(
    record_canonical_identity_predicate_node_visited,
    canonical_identity_predicate_nodes_visited
);
recorder!(record_canonical_projection, canonical_projections);
std::thread_local! {
    static REPLACEMENT_JSON_PROJECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn record_replacement_json_projection() {
    REPLACEMENT_JSON_PROJECTIONS.set(REPLACEMENT_JSON_PROJECTIONS.get().saturating_add(1));
}

pub(crate) fn take_replacement_json_projections_for_test() -> usize {
    REPLACEMENT_JSON_PROJECTIONS.replace(0)
}
recorder!(record_canonical_serialization, canonical_serializations);
recorder!(record_canonical_hash, canonical_hashes);
recorder!(record_affected_top_level_scan, affected_top_level_scans);
recorder!(record_position_map_clone, position_map_clones);
std::thread_local! {
    static STEP_MAP_PREFIX_RANGES_COPIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn record_step_map_prefix_copy(ranges: usize) {
    STEP_MAP_PREFIX_RANGES_COPIED.with(|count| count.set(count.get().saturating_add(ranges)));
}

pub(crate) fn take_step_map_prefix_ranges_copied() -> usize {
    STEP_MAP_PREFIX_RANGES_COPIED.with(|count| count.replace(0))
}
recorder!(record_position_map_compaction, position_map_compactions);
recorder!(record_rendered_text_derivation, rendered_text_derivations);
recorder!(record_raw_document_text_scan, raw_document_text_scans);
recorder!(record_document_node_count_scan, document_node_count_scans);
recorder!(record_render_limit_tree_scan, render_limit_tree_scans);
recorder!(
    record_render_top_level_start_scan,
    render_top_level_start_scans
);
recorder!(
    record_active_applicability_pass,
    active_applicability_passes
);
recorder!(record_ordinary_step_application, ordinary_step_applications);
recorder!(
    record_table_projection_derivation,
    table_projection_derivations
);
recorder!(
    record_table_command_availability_plan,
    table_command_availability_plans
);
recorder!(record_yrs_tree_walk, yrs_tree_walks);
recorder!(record_whole_state_encoding, whole_state_encodings);
recorder!(record_cell_content_key, cell_content_keys);
recorder!(record_attribute_serialization, attribute_serializations);
recorder!(record_epoch_block_rebuild, epoch_block_rebuilds);
recorder!(record_cell_content_generation, cell_content_generations);

pub(crate) fn reset_full_pass_counts_for_test() {
    FULL_PASS_COUNTS.set(FullPassCounts::default());
}

pub(crate) fn take_full_pass_counts_for_test() -> FullPassCounts {
    FULL_PASS_COUNTS.replace(FullPassCounts::default())
}

pub(crate) fn reset_prepared_admission_counts_for_test() {
    PREPARED_ADMISSION_COUNTS.set(PreparedAdmissionCounts::default());
}

pub(crate) fn take_prepared_admission_counts_for_test() -> PreparedAdmissionCounts {
    PREPARED_ADMISSION_COUNTS.replace(PreparedAdmissionCounts::default())
}

pub(crate) fn record_staged_seed_preparation() {
    PREPARED_ADMISSION_COUNTS.with(|counts| {
        let mut next = counts.get();
        next.staged_seed_preparations = next.staged_seed_preparations.saturating_add(1);
        counts.set(next);
    });
}

pub(crate) fn record_installed_base_seed_publication() {
    PREPARED_ADMISSION_COUNTS.with(|counts| {
        let mut next = counts.get();
        next.installed_base_seed_publications =
            next.installed_base_seed_publications.saturating_add(1);
        counts.set(next);
    });
}

pub(crate) fn record_staged_identity_materialization() {
    PREPARED_ADMISSION_COUNTS.with(|counts| {
        let mut next = counts.get();
        next.staged_identity_materializations =
            next.staged_identity_materializations.saturating_add(1);
        counts.set(next);
    });
}

pub(crate) fn record_deferred_capsule_created() {
    PREPARED_ADMISSION_COUNTS.with(|counts| {
        let mut next = counts.get();
        next.deferred_capsules_created = next.deferred_capsules_created.saturating_add(1);
        counts.set(next);
    });
}

pub(crate) fn record_deferred_capsule_finalized() {
    PREPARED_ADMISSION_COUNTS.with(|counts| {
        let mut next = counts.get();
        next.deferred_capsules_finalized = next.deferred_capsules_finalized.saturating_add(1);
        counts.set(next);
    });
}
