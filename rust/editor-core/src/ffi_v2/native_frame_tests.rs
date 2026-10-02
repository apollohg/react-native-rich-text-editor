use super::native_frame::build_native_frame;
use super::types::FfiTableFrameKind;
use crate::test_support::large_table_fixture::{session_with_document, two_table_document};
use crate::yrs_engine::TypedCommand;

const OWNER: u64 = 15_001;
const REQUEST: u64 = 15_002;
const EDITOR: &str = "15000";

fn frame(
    session: &mut crate::session::EditorSession,
    owner: Option<u64>,
) -> super::types::FfiNativeRenderFrame {
    build_native_frame(session, EDITOR, owner, None).unwrap()
}

#[test]
fn native_frame_full_delta_and_pin_cursor_lifetimes() {
    let mut session = session_with_document(&two_table_document());
    let full = frame(&mut session, None);
    assert_eq!(full.tables.kind, FfiTableFrameKind::Full);
    assert_eq!(full.tables.tables.len(), 2);
    let json: serde_json::Value = serde_json::from_str(&full.snapshot_json).unwrap();
    for absent in [
        "tableRecords",
        "tableAttributes",
        "tableInputMappings",
        "positionEpoch",
    ] {
        assert!(json.get(absent).is_none(), "{absent}");
    }
    let first = frame(&mut session, Some(OWNER));
    assert_eq!(first.tables.kind, FfiTableFrameKind::Full);
    let base = session.engine.revision();
    session
        .engine
        .apply_command(REQUEST, TypedCommand::InsertText { text: "x".into() })
        .unwrap();
    session
        .pin_position_epoch(OWNER, session.engine.revision())
        .unwrap();
    assert_eq!(
        session
            .native_render_cursor(OWNER)
            .unwrap()
            .document_revision,
        base,
        "a bare pin must not consume a frame transition"
    );
    super::native_frame::CELL_START_BUILDS.with(|count| count.set(0));
    let next = frame(&mut session, Some(OWNER));
    assert_eq!(
        super::native_frame::CELL_START_BUILDS.with(|count| count.get()),
        0,
        "a prose delta must not build positions for unchanged flat tables"
    );
    assert_eq!(next.tables.kind, FfiTableFrameKind::Delta);
    assert_eq!(next.tables.base_document_revision, Some(base.to_string()));
    assert!(
        next.tables.cell_updates.is_empty(),
        "prose edits do not replace cells"
    );
    let same = frame(&mut session, Some(OWNER));
    assert!(same.tables.tables.is_empty());
    assert!(same.tables.attributes.is_empty());
    assert!(same.tables.cell_updates.is_empty());
    assert!(same.tables.removed_table_keys.is_empty());
}

#[test]
fn native_frame_cursor_seeding_requires_the_current_revision() {
    let mut session = session_with_document(&two_table_document());
    let revision = session.engine.revision();
    assert_eq!(
        session
            .seed_native_render_cursor(OWNER, revision - 1)
            .unwrap_err()
            .code,
        "REVISION_MISMATCH"
    );
    assert!(session.native_render_cursor(OWNER).is_none());
    session.seed_native_render_cursor(OWNER, revision).unwrap();
    assert_eq!(
        frame(&mut session, Some(OWNER)).tables.kind,
        FfiTableFrameKind::Delta
    );
}

fn native_edit(
    session: &mut crate::session::EditorSession,
    request: u64,
    block: usize,
    text: &str,
) {
    let map = session.engine.position_map().unwrap();
    let scalar = map.effective_scalar_start(block) + map.block(block).unwrap().scalar_prefix_len;
    let epoch = session
        .pin_position_epoch(OWNER, session.engine.revision())
        .unwrap();
    crate::native_transaction_bridge::NativeTransactionBridge::new(session).submit_native_intent(&serde_json::json!({
        "version":1,"requestId":request.to_string(),"ownerId":OWNER.to_string(),"positionEpoch":epoch.to_string(),
        "intent":{"type":"insertText","anchor":scalar,"head":scalar,"text":text}
    }).to_string()).unwrap();
}

#[test]
fn native_frame_keystroke_changes_one_cell_and_root_extents() {
    let mut session = session_with_document(&two_table_document());
    let first = frame(&mut session, Some(OWNER));
    native_edit(&mut session, REQUEST, 1, "🦀");
    super::native_frame::CELL_START_BUILDS.with(|count| count.set(0));
    let delta = frame(&mut session, Some(OWNER));
    assert_eq!(
        super::native_frame::CELL_START_BUILDS.with(|count| count.get()),
        0,
        "a single-cell delta needs only its two boundaries, not a whole-table offset array"
    );
    assert!(
        delta.tables.tables.is_empty(),
        "text edits preserve table layout"
    );
    assert_eq!(delta.tables.cell_updates.len(), 1);
    assert_eq!(
        delta.tables.cell_updates[0].table_key,
        first.tables.tables[0].table_key
    );
    assert_eq!(delta.tables.cell_updates[0].cell_index, 0);
    assert_eq!(
        delta.tables.extents[0].doc_size,
        first.tables.extents[0].doc_size + 1
    );
    assert_eq!(
        delta.tables.extents[1].doc_size,
        first.tables.extents[1].doc_size
    );
    assert_eq!(
        delta.tables.extents[1].doc_start,
        first.tables.extents[1].doc_start + 1,
        "later table moves but its size is unchanged"
    );
    let json: serde_json::Value = serde_json::from_str(&delta.snapshot_json).unwrap();
    assert_eq!(json["renderPatch"]["deleteCount"], 0);
    assert_eq!(json["renderPatch"]["renderBlocks"], serde_json::json!([]));
}

#[test]
fn native_frame_structure_and_attribute_changes_are_exact() {
    use crate::tables::commands::{TableCommand, TableEdge};
    let mut session = session_with_document(&two_table_document());
    let first = frame(&mut session, Some(OWNER));
    native_edit(&mut session, REQUEST, 1, "x");
    frame(&mut session, Some(OWNER));
    session
        .engine
        .apply_command(
            REQUEST + 1,
            TypedCommand::Table(TableCommand::AddTableRow {
                side: TableEdge::After,
            }),
        )
        .unwrap()
        .unwrap();
    let delta = frame(&mut session, Some(OWNER));
    assert_eq!(delta.tables.tables.len(), 1);
    assert_eq!(
        delta.tables.tables[0].table_key,
        first.tables.tables[0].table_key
    );
    assert!(delta.tables.cell_updates.is_empty());
    let previous_pool: std::collections::BTreeSet<_> = first
        .tables
        .attributes
        .iter()
        .map(|entry| entry.key.clone())
        .collect();
    let full = frame(&mut session, None);
    let current_pool: std::collections::BTreeSet<_> = full
        .tables
        .attributes
        .iter()
        .map(|entry| entry.key.clone())
        .collect();
    assert_eq!(
        delta
            .tables
            .attributes
            .iter()
            .map(|entry| entry.key.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        current_pool.difference(&previous_pool).cloned().collect()
    );
    assert_eq!(
        delta
            .tables
            .removed_attribute_keys
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        previous_pool.difference(&current_pool).cloned().collect()
    );
    session
        .engine
        .apply_command(
            REQUEST + 2,
            TypedCommand::Table(TableCommand::DeleteTable {
                table_pos: Some(delta.tables.extents[0].doc_start),
            }),
        )
        .unwrap()
        .unwrap();
    let deleted = frame(&mut session, Some(OWNER));
    assert_eq!(
        deleted.tables.removed_table_keys,
        vec![first.tables.tables[0].table_key.clone()]
    );
    assert_eq!(deleted.tables.extents.len(), 1);
}

#[test]
fn native_frame_nested_hosts_and_schema_reset() {
    let mut session = session_with_document(
        &crate::test_support::large_table_fixture::multi_paragraph_cell_document(),
    );
    let full = frame(&mut session, Some(OWNER));
    let nested = full
        .tables
        .tables
        .iter()
        .find(|table| table.host.is_some())
        .unwrap();
    let host = nested.host.as_ref().unwrap();
    let parent = full
        .tables
        .tables
        .iter()
        .find(|table| table.table_key == host.table_key)
        .unwrap();
    assert!(parent.cells[host.cell_index as usize]
        .nested_tables
        .iter()
        .any(|entry| entry.table_key == nested.table_key));
    session.engine =
        crate::yrs_engine::YrsDocumentEngine::new(crate::yrs_engine::YrsEngineConfig {
            schema: crate::schema::presets::default_schema(),
            fragment_name: "prosemirror".into(),
            initialization_mode: crate::yrs_engine::InitializationMode::LocalEmpty,
            resource_limits: crate::boundary::ResourceLimits::default(),
            editing_limits: crate::yrs_engine::EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .unwrap();
    assert_eq!(
        frame(&mut session, Some(OWNER)).tables.kind,
        FfiTableFrameKind::Full
    );
}

#[test]
fn the_frame_mirror_matches_position_map_input_blocks() {
    for source in [
        two_table_document(),
        crate::test_support::large_table_fixture::multi_paragraph_cell_document(),
        crate::test_support::large_table_fixture::plain_table_document(3, 3),
    ] {
        let mut session = session_with_document(&source);
        let mut mirror = crate::test_support::table_frame_mirror::TableFrameMirror::default();
        mirror
            .apply(&frame(&mut session, None))
            .expect("full frame");
        mirror.assert_matches_position_map(&session.engine, "initial frame");
    }
}

fn assert_mirror(
    session: &mut crate::session::EditorSession,
    mirror: &mut crate::test_support::table_frame_mirror::TableFrameMirror,
    label: &str,
) {
    let delta = frame(session, Some(OWNER));
    mirror
        .apply(&delta)
        .unwrap_or_else(|error| panic!("{label}: delta adoption {error:?}"));
    let full = frame(session, None);
    let mut fresh = crate::test_support::table_frame_mirror::TableFrameMirror::default();
    fresh
        .apply(&full)
        .unwrap_or_else(|error| panic!("{label}: full adoption {error:?}"));
    assert_eq!(mirror.revision, fresh.revision, "{label}: revision");
    assert_eq!(mirror.attributes, fresh.attributes, "{label}: attributes");
    assert_eq!(mirror.extents, fresh.extents, "{label}: extents");
    assert_eq!(
        mirror.root_blocks, fresh.root_blocks,
        "{label}: root blocks"
    );
    assert_eq!(
        mirror.tables.keys().collect::<Vec<_>>(),
        fresh.tables.keys().collect::<Vec<_>>(),
        "{label}: table keys"
    );
    for (key, table) in &mirror.tables {
        let expected = &fresh.tables[key];
        assert_eq!(
            table.cells.len(),
            expected.cells.len(),
            "{label} {key}: cell count"
        );
        for (index, (actual, expected)) in table.cells.iter().zip(&expected.cells).enumerate() {
            assert_eq!(actual, expected, "{label} {key} cell {index}");
        }
        let mut actual = table.clone();
        actual.cells.clear();
        let mut expected = expected.clone();
        expected.cells.clear();
        assert_eq!(actual, expected, "{label} {key}: table structure");
    }
    mirror.assert_matches_position_map(&session.engine, label);
}

#[test]
fn frames_replayed_by_the_mirror_equal_full_frames_and_position_maps() {
    use crate::tables::commands::TableCommand;
    const FRAME_SEEDED_STEPS: usize = 500;
    const CYCLE: usize = 8;
    const REMOTE_REQUEST: u64 = 50_000;
    for (label, source) in [
        ("two", two_table_document()),
        (
            "nested",
            crate::test_support::large_table_fixture::multi_paragraph_cell_document(),
        ),
    ] {
        let _clients = crate::test_support::deterministic_clients::DeterministicClients::new();
        let mut session = session_with_document(&source);
        let mut mirror = crate::test_support::table_frame_mirror::TableFrameMirror::default();
        assert_mirror(&mut session, &mut mirror, label);
        for step in 0..FRAME_SEEDED_STEPS {
            let request = REQUEST + step as u64;
            let block = if label == "two" {
                1 + step % 9
            } else {
                step % 8
            };
            match step % CYCLE {
                0 | 1 | 2 | 7 => {
                    let map = session.engine.position_map().unwrap();
                    let start = map.effective_scalar_start(block)
                        + map.block(block).unwrap().scalar_prefix_len;
                    let length = map.block(block).unwrap().scalar_len;
                    let text = match step % CYCLE {
                        1 => "🦀",
                        2 => "",
                        _ => "x",
                    };
                    let end = start + u32::from(matches!(step % CYCLE, 1 | 2) && length > 0);
                    let epoch = session
                        .pin_position_epoch(OWNER, session.engine.revision())
                        .unwrap();
                    crate::native_transaction_bridge::NativeTransactionBridge::new(&mut session).submit_native_intent(&serde_json::json!({"version":1,"requestId":request.to_string(),"ownerId":OWNER.to_string(),"positionEpoch":epoch.to_string(),"intent":{"type":"insertText","anchor":start,"head":end,"text":text}}).to_string()).unwrap();
                }
                3 => {
                    session
                        .engine
                        .apply_command(
                            request,
                            TypedCommand::Table(TableCommand::ToggleTableHeader {
                                target: crate::tables::commands::TableHeaderTarget::Cell,
                            }),
                        )
                        .unwrap();
                }
                4 => {
                    session.engine.undo(request).unwrap();
                }
                5 => {
                    session.engine.redo(request).unwrap();
                }
                6 => {
                    let mut peer = crate::yrs_engine::YrsDocumentEngine::new(
                        crate::yrs_engine::YrsEngineConfig {
                            schema: session.engine.schema().clone(),
                            fragment_name: "prosemirror".into(),
                            initialization_mode: crate::yrs_engine::InitializationMode::AwaitRemote,
                            resource_limits: crate::boundary::ResourceLimits::default(),
                            editing_limits: crate::yrs_engine::EditingLimits::default(),
                            max_length: None,
                            scope: None,
                        },
                    )
                    .unwrap();
                    peer.apply_remote_update_v1(
                        REMOTE_REQUEST,
                        &session.engine.encoded_state().unwrap(),
                    )
                    .unwrap();
                    peer.apply_command(
                        REMOTE_REQUEST + 1,
                        TypedCommand::InsertText { text: "r".into() },
                    )
                    .unwrap();
                    session
                        .engine
                        .apply_remote_update_v1(request, &peer.encoded_state().unwrap())
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert_mirror(&mut session, &mut mirror, &format!("{label} step {step}"));
        }
    }
}

proptest::proptest! {
    #[test]
    fn equal_diff_keys_imply_equal_strides_and_mappings(text in "[a-z🦀]{0,24}") {
        let mut source = two_table_document();
        source["content"][1]["content"][0]["content"][1]["content"][0] = if text.is_empty() { serde_json::json!({"type":"paragraph"}) } else { serde_json::json!({"type":"paragraph","content":[{"type":"text","text":text}]}) };
        let mut session = session_with_document(&source);
        let before = frame(&mut session,None);
        native_edit(&mut session,REQUEST,1,"prefix");
        let after = frame(&mut session,None);
        for (old,new) in before.tables.tables.iter().zip(&after.tables.tables) {
            for (a,b) in old.cells.iter().zip(&new.cells) {
                if (a.content_key.as_str(),a.attrs_key.as_str(),a.doc_size) == (b.content_key.as_str(),b.attrs_key.as_str(),b.doc_size) {
                    proptest::prop_assert_eq!(a.scalar_stride,b.scalar_stride);
                    proptest::prop_assert_eq!(&a.input_blocks,&b.input_blocks);
                    proptest::prop_assert_eq!(&a.nested_tables,&b.nested_tables);
                    proptest::prop_assert_eq!(&a.void_element_indices,&b.void_element_indices);
                }
            }
        }
    }
}

#[test]
fn native_frame_failed_table_preserves_its_real_extent() {
    for empty in [false, true] {
        let mut source = crate::test_support::large_table_fixture::plain_table_document(1, 1);
        if empty {
            source["content"][0]["content"][0]["content"][0]["content"][0] =
                serde_json::json!({"type":"paragraph"});
        }
        let mut session = session_with_document(&source);
        let limits = crate::boundary::ResourceLimits {
            max_table_grid_slots: 0,
            ..crate::boundary::ResourceLimits::default()
        };
        let cache = crate::render::incremental::CachedRenderBlocks::build(
            session.engine.document().unwrap(),
            session.engine.schema(),
            &limits,
        )
        .unwrap();
        let mut records = Vec::new();
        cache.visit_table_records(&mut records);
        let keys = super::render::table_keys(&session.engine).unwrap();
        let contexts = super::native_frame::contexts(&records, &keys).unwrap();
        let table = super::native_frame::table_record(&session, &contexts[0], &keys).unwrap();
        assert!(table.failure.is_some());
        assert!(table.cells.is_empty());
        assert_eq!(table.doc_size, records[0].1.structure.doc_size);
        let extent = super::native_frame_mapping::scalar_range(
            session.engine.position_map().unwrap(),
            0,
            table.doc_size,
        );
        let mut full = frame(&mut session, None);
        full.tables.tables[0] = table;
        assert_eq!(
            (
                full.tables.extents[0].scalar_start,
                full.tables.extents[0].scalar_end
            ),
            extent
        );
        if empty {
            full.tables.extents[0].scalar_end = full.tables.extents[0].scalar_start;
        }
        crate::test_support::table_frame_mirror::TableFrameMirror::default()
            .apply(&full)
            .unwrap();
    }
}

#[test]
fn native_frame_failed_source_edits_refresh_root_and_nested_table_bounds() {
    use crate::session::{
        CollaborationLimits, DocumentState, EditorSession, EditorSessionConfig, SessionPolicy,
    };
    use crate::yrs_engine::{
        EditingLimits, InitializationMode, ReplacementHistory, YrsDocumentEngine, YrsEngineConfig,
    };
    use serde_json::json;

    for nested in [false, true] {
        let failed = json!({"type":"table","content":[
            {"type":"paragraph","content":[{"type":"text","text":"source"}]}
        ]});
        let source = if nested {
            json!({"type":"doc","content":[{"type":"table","content":[
                {"type":"table_row","content":[{"type":"table_cell","content":[failed]}]}
            ]}]})
        } else {
            json!({"type":"doc","content":[failed]})
        };
        let engine = YrsDocumentEngine::new(YrsEngineConfig {
            schema: crate::tables::interchange_tests::schema_admitting_a_stray_table_child(),
            fragment_name: "prosemirror".into(),
            initialization_mode: InitializationMode::LocalEmpty,
            resource_limits: crate::boundary::ResourceLimits::default(),
            editing_limits: EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .unwrap();
        let mut session = EditorSession::new(
            engine,
            SessionPolicy::from_config(&EditorSessionConfig::local_for_test()),
            DocumentState::LocalReady,
            CollaborationLimits::default(),
        )
        .unwrap();
        session
            .replace_document_json(
                REQUEST,
                &source.to_string(),
                ReplacementHistory::ResetAndClear,
            )
            .unwrap();
        let full = frame(&mut session, Some(OWNER));
        let failed_key = full
            .tables
            .tables
            .iter()
            .find(|table| table.failure.is_some())
            .unwrap()
            .table_key
            .clone();
        let mut mirror = crate::test_support::table_frame_mirror::TableFrameMirror::default();
        mirror.apply(&full).unwrap();
        native_edit(&mut session, REQUEST + 1, 0, "🦀 edited ");
        let delta = frame(&mut session, Some(OWNER));
        assert!(
            delta
                .tables
                .tables
                .iter()
                .any(|table| table.table_key == failed_key),
            "nested={nested}: changed failed source must replace the empty-cell record"
        );
        mirror.apply(&delta).unwrap();
        assert_mirror(
            &mut session,
            &mut mirror,
            &format!("nested={nested} failed source edit"),
        );
    }
}

#[test]
fn native_frame_mirror_rejects_invalid_deltas_atomically() {
    use crate::test_support::table_frame_mirror::TableFrameMirror;
    let mut session = session_with_document(&two_table_document());
    let full = frame(&mut session, Some(OWNER));
    let mut mirror = TableFrameMirror::default();
    mirror.apply(&full).unwrap();
    native_edit(&mut session, REQUEST, 1, "x");
    let delta = frame(&mut session, Some(OWNER));
    let mut mutations = Vec::new();
    let mut bad = delta.clone();
    bad.tables.base_document_revision = None;
    mutations.push(bad);
    let mut bad = delta.clone();
    bad.tables.cell_updates[0].cell.header = !bad.tables.cell_updates[0].cell.header;
    mutations.push(bad);
    let mut bad = delta.clone();
    bad.tables.cell_updates[0].cell.attrs_key = "missing".into();
    mutations.push(bad);
    let mut bad = delta.clone();
    bad.tables.cell_updates[0].cell.scalar_stride = 0;
    mutations.push(bad);
    let mut bad = delta.clone();
    bad.tables.extents.clear();
    mutations.push(bad);
    let mut bad = delta.clone();
    bad.tables.extents[0].doc_size += 1;
    mutations.push(bad);
    let mut bad = delta.clone();
    bad.snapshot_json = "{}".into();
    mutations.push(bad);
    let mut bad = delta.clone();
    bad.tables.cell_updates[0].cell_index = u32::MAX;
    mutations.push(bad);
    for (index, bad) in mutations.iter().enumerate() {
        let before = mirror.clone();
        assert!(mirror.apply(bad).is_err(), "malformation {index}");
        assert_eq!(
            mirror, before,
            "malformation {index} must not partially apply"
        );
    }
    mirror.apply(&delta).unwrap();
}

#[test]
fn native_frame_attribute_pool_additions_and_removals_follow_attributes() {
    let mut source = two_table_document();
    let mut schema =
        crate::tables::tests::tabled_schema_json(crate::tables::tests::PROSEMIRROR_TABLE_NAMES);
    for node in schema["nodes"].as_array_mut().unwrap() {
        if node["name"] == "table" {
            node["attrs"] = serde_json::json!({"tone":{"default":null}});
        }
    }
    let mut session = session_with_document(&source);
    session.engine =
        crate::yrs_engine::YrsDocumentEngine::new(crate::yrs_engine::YrsEngineConfig {
            schema: crate::schema::Schema::from_json(&schema).unwrap(),
            fragment_name: "prosemirror".into(),
            initialization_mode: crate::yrs_engine::InitializationMode::LocalEmpty,
            resource_limits: crate::boundary::ResourceLimits::default(),
            editing_limits: crate::yrs_engine::EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .unwrap();
    session
        .engine
        .import_json(
            &source.to_string(),
            crate::yrs_engine::TransactionOrigin::DocumentImport,
        )
        .unwrap();
    let before = frame(&mut session, Some(OWNER));
    source["content"][1]["attrs"] = serde_json::json!({"tone":"red"});
    session
        .replace_document_json(
            REQUEST,
            &source.to_string(),
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    let delta = frame(&mut session, Some(OWNER));
    assert!(
        !delta.tables.attributes.is_empty(),
        "a new authored attribute value enters the pool"
    );
    let mut mirror = crate::test_support::table_frame_mirror::TableFrameMirror::default();
    mirror.apply(&before).unwrap();
    mirror.apply(&delta).unwrap();
    let full = frame(&mut session, None);
    assert_eq!(
        mirror.attributes,
        full.tables
            .attributes
            .iter()
            .map(|entry| (entry.key.clone(), entry.json.clone()))
            .collect()
    );
    session.engine.undo(REQUEST + 1).unwrap();
    let undo = frame(&mut session, Some(OWNER));
    assert_eq!(
        undo.tables.removed_attribute_keys,
        delta
            .tables
            .attributes
            .iter()
            .map(|entry| entry.key.clone())
            .collect::<Vec<_>>()
    );
    mirror.apply(&undo).unwrap();
    assert_eq!(
        mirror.attributes,
        before
            .tables
            .attributes
            .iter()
            .map(|entry| (entry.key.clone(), entry.json.clone()))
            .collect()
    );
}

#[test]
fn native_frame_cursor_seeding_is_bounded_and_release_reclaims_capacity() {
    let mut session = session_with_document(&two_table_document());
    let limit = crate::position_epoch::PositionEpochLimits::default().max_owners as u64;
    let revision = session.engine.revision();
    for owner in 0..limit {
        session.seed_native_render_cursor(owner, revision).unwrap();
    }
    let error = session
        .seed_native_render_cursor(limit, revision)
        .unwrap_err();
    assert_eq!(error.code, "OPERATION_RESOURCE_EXHAUSTED");
    assert!(session.native_render_cursor(limit).is_none());
    session.release_position_epoch_owner(0);
    session.seed_native_render_cursor(limit, revision).unwrap();
}

#[test]
fn native_frame_updates_a_nested_tables_host_after_inserting_an_outer_row() {
    use crate::tables::commands::{TableCommand, TableEdge};
    let mut session = session_with_document(
        &crate::test_support::large_table_fixture::multi_paragraph_cell_document(),
    );
    let first = frame(&mut session, Some(OWNER));
    let nested = first
        .tables
        .tables
        .iter()
        .find(|table| table.host.is_some())
        .unwrap();
    let old_host = nested.host.as_ref().unwrap();
    let nested_key = nested.table_key.clone();
    native_edit(&mut session, REQUEST, 0, "x");
    let edited = frame(&mut session, Some(OWNER));
    session
        .engine
        .apply_command(
            REQUEST + 1,
            TypedCommand::Table(TableCommand::AddTableRow {
                side: TableEdge::Before,
            }),
        )
        .unwrap()
        .unwrap();
    let delta = frame(&mut session, Some(OWNER));
    let changed = delta
        .tables
        .tables
        .iter()
        .find(|table| table.table_key == nested_key)
        .expect("the nested table carries its new host index");
    assert_eq!(
        changed.host.as_ref().unwrap().cell_index,
        old_host.cell_index + first.tables.tables[0].columns
    );
    let mut mirror = crate::test_support::table_frame_mirror::TableFrameMirror::default();
    mirror.apply(&first).unwrap();
    mirror.apply(&edited).unwrap();
    mirror.apply(&delta).unwrap();
    assert_mirror(&mut session, &mut mirror, "nested host shifted");
}

#[test]
fn native_frame_exports_return_typed_errors_and_round_trip_frames() {
    use super::native_frame::{editor_v2_render_native_frame, editor_v2_seed_native_render_cursor};
    let unknown = editor_v2_render_native_frame(u64::MAX.to_string(), None, None, None);
    assert!(unknown.frame.is_none());
    assert_eq!(unknown.error.unwrap().code, "ENGINE_DESTROYED");
    let malformed =
        editor_v2_render_native_frame(u64::MAX.to_string(), Some("01".into()), None, None);
    assert!(malformed.frame.is_none());
    assert_eq!(malformed.error.unwrap().code, "CONFIG_INVALID");
    let editor = crate::test_support::large_table_fixture::ffi_empty_editor();
    let first = editor_v2_render_native_frame(editor.clone(), Some(OWNER.to_string()), None, None);
    assert!(first.error.is_none());
    let first = first.frame.unwrap();
    assert_eq!(first.tables.kind, FfiTableFrameKind::Full);
    let snapshot: serde_json::Value = serde_json::from_str(&first.snapshot_json).unwrap();
    let revision = snapshot["documentVersion"].as_str().unwrap();
    let seeded = editor_v2_seed_native_render_cursor(
        editor.clone(),
        (OWNER + 1).to_string(),
        revision.into(),
    );
    assert!(seeded.error.is_none());
    let delta =
        editor_v2_render_native_frame(editor.clone(), Some((OWNER + 1).to_string()), None, None)
            .frame
            .unwrap();
    assert_eq!(delta.tables.kind, FfiTableFrameKind::Delta);
    assert!(delta.tables.tables.is_empty());
    assert_eq!(
        editor_v2_render_native_frame(editor.clone(), None, Some(0), None)
            .error
            .unwrap()
            .code,
        "CONFIG_INVALID"
    );
    assert_eq!(
        editor_v2_seed_native_render_cursor(editor.clone(), OWNER.to_string(), "01".into())
            .error
            .unwrap()
            .code,
        "CONFIG_INVALID"
    );
    assert_eq!(
        editor_v2_seed_native_render_cursor(
            editor.clone(),
            OWNER.to_string(),
            u64::MAX.to_string()
        )
        .error
        .unwrap()
        .code,
        "REVISION_MISMATCH"
    );
    super::editor::editor_v2_destroy(editor);
}

#[test]
fn native_frame_preserves_void_atom_render_kinds() {
    let mut document = crate::test_support::large_table_fixture::plain_table_document(1, 1);
    document["content"][0]["content"][0]["content"][0]["content"] = serde_json::json!([
        {"type":"paragraph","content":[
            {"type":"text","text":"x"}, {"type":"hard_break"},
            {"type":"mention","attrs":{"id":"atom","label":"Ada"}}
        ]},
        {"type":"horizontal_rule"}
    ]);
    let mut session = session_with_document(&document);
    let full = frame(&mut session, Some(OWNER));
    let cell = &full.tables.tables[0].cells[0];
    assert_eq!(cell.void_element_indices, vec![2, 5]);
    assert!(matches!(
        &cell.elements[3],
        crate::viewer::FfiViewerElement::InlineAtom { .. }
    ));
}

#[test]
fn native_table_typing_reuses_ancestors_observed_under_the_compilation_lock() {
    use crate::test_support::large_table_fixture::plain_table_document;
    use crate::yrs_engine::observability::PREFLIGHT_CHILDREN_ENUMERATED;
    const DEEP_WRAPPERS: usize = 128;
    const SINGLE_CHILD_ANCESTORS: usize = 3;
    const LIVE_TEXTBLOCK_READS: usize = 2;
    for (rows, columns, depth) in [(1000, 20, 0), (1, 1, DEEP_WRAPPERS)] {
        let mut source = plain_table_document(rows, columns);
        let mut block = source["content"][0].take();
        for _ in 0..depth {
            block = serde_json::json!({"type":"blockquote","content":[block]});
        }
        source["content"][0] = block;
        let mut session = session_with_document(&source);
        let mut mirror = crate::test_support::table_frame_mirror::TableFrameMirror::default();
        mirror.apply(&frame(&mut session, Some(OWNER))).unwrap();
        native_edit(&mut session, REQUEST, 0, "warm");
        mirror.apply(&frame(&mut session, Some(OWNER))).unwrap();
        PREFLIGHT_CHILDREN_ENUMERATED.set(0);
        let before = session.engine.revision();
        native_edit(&mut session, REQUEST + 1, 0, "🙂");
        let scanned = PREFLIGHT_CHILDREN_ENUMERATED.get();
        assert_eq!(session.engine.revision(), before + 1);
        let delta = frame(&mut session, Some(OWNER));
        assert_eq!(delta.tables.cell_updates.len(), 1);
        mirror.apply(&delta).unwrap();
        assert_mirror(&mut session, &mut mirror, "scoped ancestor reuse");
        assert_eq!(
            scanned, rows + columns + depth + SINGLE_CHILD_ANCESTORS + LIVE_TEXTBLOCK_READS,
            "candidate-store traversal remains complete; live checks reuse only ancestors observed under their held read lock"
        );
    }
}

#[test]
fn native_frame_single_cell_offsets_stop_at_the_next_boundary_and_bulk_offsets_stay_linear() {
    use super::native_frame::CELL_START_BUILDS;
    use crate::tables::render::CELL_START_VALUE_VISITS;
    const ROWS: usize = 1000;
    const COLUMNS: usize = 20;
    const CELLS: usize = ROWS * COLUMNS;
    const CURRENT_AND_NEXT: usize = 2;
    let source = crate::test_support::large_table_fixture::plain_table_document(ROWS, COLUMNS);
    let mut session = session_with_document(&source);
    let mut mirror = crate::test_support::table_frame_mirror::TableFrameMirror::default();
    CELL_START_BUILDS.set(0);
    CELL_START_VALUE_VISITS.set(0);
    let full = frame(&mut session, Some(OWNER));
    assert_eq!(
        CELL_START_BUILDS.get(),
        1,
        "full frames share one offset array"
    );
    assert_eq!(
        CELL_START_VALUE_VISITS.get(),
        CELLS,
        "full frames scan the cells once"
    );
    mirror.apply(&full).unwrap();
    let edited_cells = [0, COLUMNS - 1, COLUMNS, CELLS / 2, CELLS - 1];
    for (step, index) in edited_cells.into_iter().enumerate() {
        native_edit(&mut session, REQUEST + step as u64, index, "🦀");
        CELL_START_BUILDS.set(0);
        CELL_START_VALUE_VISITS.set(0);
        let delta = frame(&mut session, Some(OWNER));
        assert_eq!(
            CELL_START_BUILDS.get(),
            0,
            "cell {index}: no whole-table offsets"
        );
        assert_eq!(
            CELL_START_VALUE_VISITS.get(),
            (index + CURRENT_AND_NEXT).min(CELLS),
            "cell {index}: stop after the following start, including row gaps"
        );
        assert_eq!(delta.tables.cell_updates.len(), 1);
        assert_eq!(delta.tables.cell_updates[0].cell_index, index as u32);
        mirror.apply(&delta).unwrap();
        assert_mirror(&mut session, &mut mirror, &format!("single cell {index}"));
    }
    let batch_request = REQUEST + edited_cells.len() as u64;
    native_edit(&mut session, batch_request, 0, "first");
    native_edit(&mut session, batch_request + 1, CELLS - 1, "last");
    CELL_START_BUILDS.set(0);
    CELL_START_VALUE_VISITS.set(0);
    let delta = frame(&mut session, Some(OWNER));
    assert_eq!(delta.tables.cell_updates.len(), CURRENT_AND_NEXT);
    assert_eq!(
        CELL_START_BUILDS.get(),
        1,
        "bulk updates reuse a shared offset array"
    );
    assert_eq!(
        CELL_START_VALUE_VISITS.get(),
        CELLS,
        "bulk updates scan the cells once"
    );
    mirror.apply(&delta).unwrap();
    assert_mirror(&mut session, &mut mirror, "bulk update");
}
