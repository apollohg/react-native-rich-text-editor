#![allow(
    clippy::result_large_err,
    reason = "SessionError is the established unboxed session error envelope"
)]

#[cfg(test)]
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::boundary::{BoundaryError, ResourceLimits};
use crate::model::Document;
use crate::position::PositionMap;
use crate::schema::{presets::default_schema, Schema};
use crate::selection::Selection;
use crate::session::{EditorSession, SessionError};
use crate::yrs_engine::ResolvedSelection;
use crate::yrs_engine::YrsEngineError;

use super::editor::{json_result, with_editor};
#[cfg(test)]
use super::types::parse_canonical_u64;
use super::types::{decimal_u64, FfiJsonResult};

#[cfg(test)]
std::thread_local! {
    pub(super) static ROOT_PROJECTION_BUILDS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
    pub(super) static ROOT_BLOCK_SERIALIZATIONS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

#[cfg(test)]
struct RenderSnapshotTestHook {
    editor_id: u64,
    entered: std::sync::mpsc::SyncSender<()>,
    resume: std::sync::mpsc::Receiver<()>,
}

#[cfg(test)]
static RENDER_SNAPSHOT_TEST_HOOK: LazyLock<Mutex<Option<RenderSnapshotTestHook>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(test)]
pub(crate) struct RenderSnapshotTestHookGuard {
    editor_id: u64,
}

#[cfg(test)]
impl Drop for RenderSnapshotTestHookGuard {
    fn drop(&mut self) {
        let mut hook = RENDER_SNAPSHOT_TEST_HOOK
            .lock()
            .expect("render snapshot test hook poisoned");
        if hook
            .as_ref()
            .is_some_and(|hook| hook.editor_id == self.editor_id)
        {
            hook.take();
        }
    }
}

#[cfg(test)]
pub(crate) fn install_render_snapshot_test_hook(
    editor_id: u64,
    entered: std::sync::mpsc::SyncSender<()>,
    resume: std::sync::mpsc::Receiver<()>,
) -> RenderSnapshotTestHookGuard {
    *RENDER_SNAPSHOT_TEST_HOOK
        .lock()
        .expect("render snapshot test hook poisoned") = Some(RenderSnapshotTestHook {
        editor_id,
        entered,
        resume,
    });
    RenderSnapshotTestHookGuard { editor_id }
}

#[cfg(test)]
fn pause_render_snapshot_for_test(editor_id: &str) {
    let editor_id = match parse_canonical_u64(editor_id) {
        Some(editor_id) => editor_id,
        None => return,
    };
    let hook = {
        let mut hook = RENDER_SNAPSHOT_TEST_HOOK
            .lock()
            .expect("render snapshot test hook poisoned");
        if hook
            .as_ref()
            .is_some_and(|hook| hook.editor_id == editor_id)
        {
            hook.take()
        } else {
            None
        }
    };
    if let Some(hook) = hook {
        hook.entered
            .send(())
            .expect("render snapshot test observer dropped");
        hook.resume
            .recv()
            .expect("render snapshot test resume sender dropped");
    }
}

#[cfg(not(test))]
fn pause_render_snapshot_for_test(_: &str) {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AtomicRenderSnapshot<'a> {
    render_blocks: Option<&'a [Box<RawValue>]>,
    render_patch: Option<RootRenderPatch<'a>>,
    #[serde(flatten)]
    state: RenderStateFields,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AtomicRenderState {
    void_blocks: Vec<Vec<Value>>,
    #[serde(flatten)]
    state: RenderStateFields,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderStateFields {
    selection: Value,
    active_state: Value,
    history_state: Value,
    document_version: String,
    state_revision: String,
    scalar_length: u32,
    /// Authoritative empty state for host affordances such as the placeholder.
    /// Hosts must consume this rather than inspecting rendered characters,
    /// which cannot see structure that renders no text (an empty list item).
    document_is_empty: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RootRenderPatch<'a> {
    base_document_version: String,
    start_index: usize,
    delete_count: usize,
    render_blocks: &'a [Box<RawValue>],
}

#[derive(Clone, Debug)]
pub(crate) struct RootRenderBlock {
    // Keep source allocation identity without extending guarded payload lifetimes.
    elements: std::sync::Weak<Vec<crate::render::RenderElement>>,
    digest: [u8; 32],
}

impl PartialEq for RootRenderBlock {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest
    }
}

pub(crate) struct RootProjection {
    blocks: Vec<RootRenderBlock>,
    published: Vec<Option<Box<RawValue>>>,
}

impl RootProjection {
    pub(crate) fn into_retained(self) -> Vec<RootRenderBlock> {
        self.blocks
    }
}

/// Resolve the schema for a v2 create envelope exactly the way the session
/// construction does (default preset when the envelope carries no schema).
/// Resolved BEFORE the session is created so an invalid schema fails with
/// the identical structured error and no partial registration exists.
pub(crate) fn resolve_create_schema(schema: &Option<Value>) -> Result<Schema, SessionError> {
    match schema {
        None => Ok(default_schema()),
        Some(value) => Schema::from_json_with_limits(value, &ResourceLimits::default())
            .map_err(SessionError::from),
    }
}

fn engine_not_ready() -> SessionError {
    SessionError::from(YrsEngineError::new(
        "ENGINE_NOT_READY",
        "the document engine is not ready",
    ))
}

fn config_invalid(message: impl Into<String>) -> SessionError {
    SessionError::from(BoundaryError::new("CONFIG_INVALID", message))
}

/// The probe's selection mapping: lenient scalar->doc, collapsed selections
/// become cursors, then cursor normalization (legacy `setSelectionScalar`).
fn map_scalar_selection(
    document: &Document,
    position_map: &PositionMap,
    scalar_anchor: u32,
    scalar_head: u32,
) -> Selection {
    let doc_anchor = position_map.scalar_to_doc(scalar_anchor, document);
    let doc_head = position_map.scalar_to_doc(scalar_head, document);
    if doc_anchor == doc_head {
        Selection::cursor(doc_anchor)
    } else {
        Selection::text(doc_anchor, doc_head)
    }
    .normalized(document, position_map)
}

/// The legacy update-JSON selection shape: doc positions plus the scalar
/// round-trip (`anchorScalar`/`headScalar`).
fn selection_json(document: &Document, position_map: &PositionMap, selection: &Selection) -> Value {
    let scalar = match selection {
        Selection::Text { anchor, head } => Selection::text(
            position_map.doc_to_scalar(*anchor, document),
            position_map.doc_to_scalar(*head, document),
        ),
        Selection::Node { pos } => Selection::node(position_map.doc_to_scalar(*pos, document)),
        Selection::Cell { anchor, head } => Selection::cell(*anchor, *head),
        Selection::All => Selection::All,
    };
    selection_to_json(selection, Some(&scalar))
}

fn resolved_selection_to_legacy(selection: &ResolvedSelection) -> Selection {
    match selection {
        ResolvedSelection::Text { anchor, head } => Selection::text(anchor.document, head.document),
        ResolvedSelection::Node { at } => Selection::node(at.document),
        ResolvedSelection::Cell { anchor, head } => Selection::cell(anchor.document, head.document),
        ResolvedSelection::All => Selection::all(),
    }
}

fn resolved_selection_json(selection: &ResolvedSelection) -> Value {
    match selection {
        ResolvedSelection::Text { anchor, head } => serde_json::json!({
            "type": "text",
            "anchor": anchor.document,
            "head": head.document,
            "anchorScalar": anchor.scalar,
            "headScalar": head.scalar,
        }),
        ResolvedSelection::Node { at } => serde_json::json!({
            "type": "node",
            "pos": at.document,
            "posScalar": at.scalar,
        }),
        ResolvedSelection::Cell { anchor, head } => serde_json::json!({
            "type": "cell",
            "anchorCell": anchor.document,
            "headCell": head.document,
        }),
        ResolvedSelection::All => serde_json::json!({ "type": "all" }),
    }
}

#[uniffi::export]
pub fn editor_v2_render_state(
    editor_id: String,
    mirror_scalar_anchor: Option<u32>,
    mirror_scalar_head: Option<u32>,
) -> FfiJsonResult {
    json_result(with_editor(&editor_id, |session| {
        let engine = &session.engine;
        let state = render_state_fields(
            engine,
            render_mirror(mirror_scalar_anchor, mirror_scalar_head)?,
        )?;
        let snapshot = AtomicRenderState {
            void_blocks: void_block_projection(engine)?,
            state,
        };
        Ok(serde_json::to_string(&snapshot).expect("atomic render state serializes"))
    }))
}

#[uniffi::export]
pub fn editor_v2_render_update(
    editor_id: String,
    mirror_scalar_anchor: Option<u32>,
    mirror_scalar_head: Option<u32>,
) -> FfiJsonResult {
    json_result(with_editor(&editor_id, |session| {
        root_snapshot(
            session,
            &editor_id,
            None,
            render_mirror(mirror_scalar_anchor, mirror_scalar_head)?,
        )
        .map(|(json, _)| json)
    }))
}

fn render_mirror(
    anchor: Option<u32>,
    head: Option<u32>,
) -> Result<Option<(u32, u32)>, SessionError> {
    match (anchor, head) {
        (None, None) => Ok(None),
        (Some(anchor), Some(head)) => Ok(Some((anchor, head))),
        _ => Err(config_invalid(
            "render mirror requires both scalar anchor and head",
        )),
    }
}

pub(crate) fn root_snapshot(
    session: &mut EditorSession,
    editor_id: &str,
    owner_id: Option<u64>,
    mirror: Option<(u32, u32)>,
) -> Result<(String, Arc<Vec<RootRenderBlock>>), SessionError> {
    let previous_native_render =
        owner_id.and_then(|owner_id| session.native_render_cursor(owner_id));
    let engine = &session.engine;
    pause_render_snapshot_for_test(&editor_id);
    let state = render_state_fields(engine, mirror)?;

    let document_version = engine.revision();
    let previous_projection = previous_native_render
        .as_ref()
        .filter(|previous| previous.schema_fingerprint == engine.schema_fingerprint());
    let (current_projection, mut published) = match previous_projection
        .filter(|previous| previous.document_revision == document_version)
    {
        Some(previous) => (previous.root_projection.clone(), Vec::new()),
        None => {
            let projection = root_projection(
                engine,
                previous_projection.map(|previous| previous.root_projection.as_slice()),
            )?;
            (Arc::new(projection.blocks), projection.published)
        }
    };
    let current = current_projection.as_slice();
    let (emitted, patch) = match previous_projection {
        Some(previous) => {
            let old = &previous.root_projection;
            let prefix = if Arc::ptr_eq(old, &current_projection) {
                current.len()
            } else {
                old.iter().zip(current).take_while(|(a, b)| a == b).count()
            };
            let suffix = old[prefix..]
                .iter()
                .rev()
                .zip(current[prefix..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            (
                prefix..current.len() - suffix,
                Some((previous.document_revision, old.len() - prefix - suffix)),
            )
        }
        None => (0..current.len(), None),
    };
    let start_index = emitted.start;
    let render_blocks = publish_root_blocks(engine, &mut published, emitted)?;
    let snapshot = AtomicRenderSnapshot {
        render_blocks: patch.is_none().then_some(render_blocks.as_slice()),
        render_patch: patch.map(|(base_revision, delete_count)| RootRenderPatch {
            base_document_version: base_revision.to_string(),
            start_index,
            delete_count,
            render_blocks: &render_blocks,
        }),
        state,
    };
    let json = serde_json::to_string(&snapshot).expect("atomic render snapshot serializes");
    Ok((json, current_projection))
}

fn publish_root_blocks(
    engine: &crate::yrs_engine::YrsDocumentEngine,
    published: &mut [Option<Box<RawValue>>],
    emitted: std::ops::Range<usize>,
) -> Result<Vec<Box<RawValue>>, SessionError> {
    let cache = engine.cached_render_blocks().ok_or_else(engine_not_ready)?;
    let mut source_ids = None;
    cache
        .root_blocks()
        .enumerate()
        .skip(emitted.start)
        .take(emitted.len())
        .map(
            |(index, block)| match published.get_mut(index).and_then(Option::take) {
                Some(json) => Ok(json),
                None => {
                    let source_ids = match &source_ids {
                        Some(source_ids) => source_ids,
                        None => source_ids.insert(RenderSourceIds::for_engine(engine)?),
                    };
                    serialize_root_block(block, source_ids)
                }
            },
        )
        .collect()
}

fn serialize_root_block(
    block: &[crate::render::RenderElement],
    source_ids: &RenderSourceIds<'_>,
) -> Result<Box<RawValue>, SessionError> {
    #[cfg(test)]
    ROOT_BLOCK_SERIALIZATIONS.set(ROOT_BLOCK_SERIALIZATIONS.get().saturating_add(1));
    let values = block
        .iter()
        .map(|element| serialize_render_element(element, source_ids))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::value::to_raw_value(&values).expect("root render block serializes"))
}

fn render_state_fields(
    engine: &crate::yrs_engine::YrsDocumentEngine,
    mirror: Option<(u32, u32)>,
) -> Result<RenderStateFields, SessionError> {
    let document = engine.document().ok_or_else(engine_not_ready)?;
    let position_map = engine.position_map().ok_or_else(engine_not_ready)?;
    let schema = engine.schema();
    let (selection, selection_value, stored_marks) = match mirror {
        Some((anchor, head)) => {
            let selection = map_scalar_selection(document, position_map, anchor, head);
            let value = selection_json(document, position_map, &selection);
            (selection, value, None)
        }
        None => {
            let resolved = engine.resolved_selection().ok_or_else(engine_not_ready)?;
            (
                resolved_selection_to_legacy(resolved),
                resolved_selection_json(resolved),
                engine.stored_marks(),
            )
        }
    };
    let active_state = if mirror.is_none()
        || (engine.stored_marks().is_none()
            && engine
                .resolved_selection()
                .is_some_and(|resolved| resolved_selection_to_legacy(resolved) == selection))
    {
        engine.active_state().ok_or_else(engine_not_ready)?
    } else {
        let commands = crate::editor_state::command_applicability(
            document,
            schema,
            &selection,
            engine.resource_limits(),
        );
        crate::editor_state::active_state(
            document,
            schema,
            &selection,
            stored_marks,
            commands,
            engine.resource_limits(),
        )
    };
    let scalar_length = position_map.doc_to_scalar(u32::MAX, document);
    let document_is_empty = crate::editor_state::document_is_empty(document, schema);

    Ok(RenderStateFields {
        selection: selection_value,
        active_state: serialize_active_state(&active_state),
        history_state: serde_json::json!({
            "canUndo": engine.can_undo(),
            "canRedo": engine.can_redo(),
        }),
        document_version: decimal_u64(engine.revision())
            .as_str()
            .expect("decimal u64 serializer returns a string")
            .to_owned(),
        state_revision: decimal_u64(engine.state_revision())
            .as_str()
            .expect("decimal u64 serializer returns a string")
            .to_owned(),
        scalar_length,
        document_is_empty,
    })
}

pub(crate) fn native_selection_state(
    engine: &crate::yrs_engine::YrsDocumentEngine,
) -> Result<Value, SessionError> {
    Ok(serde_json::to_value(render_state_fields(engine, None)?)
        .expect("native selection state serializes"))
}

fn void_block_projection(
    engine: &crate::yrs_engine::YrsDocumentEngine,
) -> Result<Vec<Vec<Value>>, SessionError> {
    let source_ids = RenderSourceIds::for_engine(engine)?;
    let cache = engine.cached_render_blocks().ok_or_else(engine_not_ready)?;
    cache
        .root_blocks()
        .map(|block| {
            block
                .iter()
                .filter(|element| matches!(element, crate::render::RenderElement::VoidBlock { .. }))
                .map(|element| serialize_render_element(element, &source_ids))
                .collect()
        })
        .collect()
}

#[uniffi::export]
pub fn editor_v2_resolve_scalar_selection(
    editor_id: String,
    scalar_anchor: u32,
    scalar_head: u32,
) -> FfiJsonResult {
    json_result(with_editor(&editor_id, |session| {
        let engine = &session.engine;
        let document = engine.document().ok_or_else(engine_not_ready)?;
        let position_map = engine.position_map().ok_or_else(engine_not_ready)?;
        let selection = map_scalar_selection(document, position_map, scalar_anchor, scalar_head);
        Ok(selection_json(document, position_map, &selection).to_string())
    }))
}

#[uniffi::export]
pub fn editor_v2_doc_to_scalar(editor_id: String, doc_pos: u32) -> FfiJsonResult {
    json_result(with_editor(&editor_id, |session| {
        let engine = &session.engine;
        let document = engine.document().ok_or_else(engine_not_ready)?;
        let position_map = engine.position_map().ok_or_else(engine_not_ready)?;
        Ok(
            serde_json::json!({ "scalar": position_map.doc_to_scalar(doc_pos, document) })
                .to_string(),
        )
    }))
}

#[uniffi::export]
pub fn editor_v2_scalar_to_doc(editor_id: String, scalar: u32) -> FfiJsonResult {
    json_result(with_editor(&editor_id, |session| {
        let engine = &session.engine;
        let document = engine.document().ok_or_else(engine_not_ready)?;
        let position_map = engine.position_map().ok_or_else(engine_not_ready)?;
        Ok(serde_json::json!({ "doc": position_map.scalar_to_doc(scalar, document) }).to_string())
    }))
}

pub(crate) fn table_keys(
    engine: &crate::yrs_engine::YrsDocumentEngine,
) -> Result<std::collections::BTreeMap<u32, String>, SessionError> {
    let source_ids = RenderSourceIds::for_engine(engine)?;
    let cache = engine.cached_render_blocks().ok_or_else(engine_not_ready)?;
    let mut tables = Vec::new();
    cache.visit_table_records(&mut tables);
    tables
        .into_iter()
        .map(|(position, _)| {
            source_ids
                .table_key(position)
                .map(|key| (position, key.to_owned()))
                .ok_or_else(|| {
                    SessionError::from(YrsEngineError::new(
                        "ENGINE_INVARIANT_FAILED",
                        "table source identity is missing",
                    ))
                })
        })
        .collect()
}

pub(crate) fn root_projection(
    engine: &crate::yrs_engine::YrsDocumentEngine,
    previous: Option<&[RootRenderBlock]>,
) -> Result<RootProjection, SessionError> {
    #[cfg(test)]
    ROOT_PROJECTION_BUILDS.set(ROOT_PROJECTION_BUILDS.get().saturating_add(1));
    let source_ids = RenderSourceIds::for_engine(engine)?;
    let cache = engine.cached_render_blocks().ok_or_else(engine_not_ready)?;
    let previous: std::collections::HashMap<_, _> = previous
        .into_iter()
        .flatten()
        .map(|block| (block.elements.as_ptr(), block))
        .collect();
    let mut projection = RootProjection {
        blocks: Vec::with_capacity(cache.root_blocks().len()),
        published: Vec::with_capacity(cache.root_blocks().len()),
    };
    for block in cache.root_blocks() {
        // Tables and block atoms also depend on live CRDT source identities.
        let source_dependent = block.iter().any(|element| {
            matches!(
                element,
                crate::render::RenderElement::Table { .. }
                    | crate::render::RenderElement::VoidBlock { .. }
            )
        });
        if let Some(previous) = previous
            .get(&Arc::as_ptr(block))
            .filter(|_| !source_dependent)
        {
            projection.blocks.push((*previous).clone());
            projection.published.push(None);
            continue;
        }
        let json = serialize_root_block(block, &source_ids)?;
        projection.blocks.push(RootRenderBlock {
            elements: Arc::downgrade(block),
            digest: Sha256::digest(json.get()).into(),
        });
        projection.published.push(Some(json));
    }
    Ok(projection)
}

struct RenderSourceIds<'a> {
    index: &'a crate::yrs_engine::BlockBranchIndex,
    document: &'a crate::model::Document,
}

impl<'a> RenderSourceIds<'a> {
    fn for_engine(engine: &'a crate::yrs_engine::YrsDocumentEngine) -> Result<Self, SessionError> {
        Ok(Self {
            document: engine.document().ok_or_else(engine_not_ready)?,
            index: engine.block_branch_index().ok_or_else(engine_not_ready)?,
        })
    }

    fn table_key(&self, position: u32) -> Option<&str> {
        self.index
            .table_key(&crate::tables::commands::node_path_starting_at(
                self.document,
                position,
            )?)
    }

    fn atom_id(&self, position: u32) -> Option<&str> {
        self.index
            .atom_id(&crate::tables::commands::node_path_starting_at(
                self.document,
                position,
            )?)
    }
}

fn serialize_render_element(
    element: &crate::render::RenderElement,
    source_ids: &RenderSourceIds<'_>,
) -> Result<Value, SessionError> {
    Ok(match element {
        crate::render::RenderElement::Table { doc_offset, .. } => source_ids
            .table_key(*doc_offset)
            .map(|key| serde_json::json!({"type":"table", "tableId":key}))
            .ok_or_else(|| {
                SessionError::from(YrsEngineError::new(
                    "ENGINE_INVARIANT_FAILED",
                    "table source identity is missing",
                ))
            })?,
        crate::render::RenderElement::TextRun { text, marks } => {
            serde_json::json!({
                "type": "textRun",
                "text": text,
                "marks": marks.iter().map(serialize_render_mark).collect::<Vec<_>>(),
            })
        }
        crate::render::RenderElement::VoidInline {
            node_type,
            doc_pos,
            attrs,
        } => {
            let mut obj = serde_json::json!({
                "type": "voidInline",
                "nodeType": node_type,
                "docPos": *doc_pos,
            });
            if !attrs.is_empty() {
                obj["attrs"] = serde_json::Value::Object(
                    attrs
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.clone(),
                                crate::boundary::clone_json_value_stack_safe(value),
                            )
                        })
                        .collect(),
                );
            }
            obj
        }
        crate::render::RenderElement::VoidBlock {
            node_type,
            doc_pos,
            attrs,
        } => {
            let mut obj = serde_json::json!({
                "type": "voidBlock",
                "nodeType": node_type,
                "docPos": *doc_pos,
            });
            if !attrs.is_empty() {
                obj["attrs"] = serde_json::Value::Object(
                    attrs
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.clone(),
                                crate::boundary::clone_json_value_stack_safe(value),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(atom_id) = source_ids.atom_id(*doc_pos) {
                obj["atomId"] = Value::String(atom_id.to_owned());
            }
            obj
        }
        crate::render::RenderElement::OpaqueInlineAtom {
            node_type,
            label,
            doc_pos,
            attrs,
            mention_theme,
        } => {
            let mut obj = serde_json::json!({
                "type": "opaqueInlineAtom",
                "nodeType": node_type,
                "label": label,
                "docPos": *doc_pos,
            });
            if !attrs.is_empty() {
                obj["attrs"] = serde_json::Value::Object(
                    attrs
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.clone(),
                                crate::boundary::clone_json_value_stack_safe(value),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(mention_theme) = mention_theme {
                obj["mentionTheme"] = serde_json::Value::Object(
                    mention_theme
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.clone(),
                                crate::boundary::clone_json_value_stack_safe(value),
                            )
                        })
                        .collect(),
                );
            }
            obj
        }
        crate::render::RenderElement::OpaqueBlockAtom {
            node_type,
            label,
            doc_pos,
            attrs,
        } => {
            let mut obj = serde_json::json!({
                "type": "opaqueBlockAtom",
                "nodeType": node_type,
                "label": label,
                "docPos": *doc_pos,
            });
            if !attrs.is_empty() {
                obj["attrs"] = serde_json::Value::Object(
                    attrs
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.clone(),
                                crate::boundary::clone_json_value_stack_safe(value),
                            )
                        })
                        .collect(),
                );
            }
            obj
        }
        crate::render::RenderElement::BlockStart {
            node_type,
            language,
            depth,
            list_context,
        } => {
            let mut obj = serde_json::json!({
                "type": "blockStart",
                "nodeType": node_type,
                "depth": depth,
            });
            if let Some(language) = language {
                obj["language"] = serde_json::Value::String(language.clone());
            }
            if let Some(ctx) = list_context {
                obj["listContext"] = serde_json::json!({
                    "ordered": ctx.ordered,
                    "index": ctx.index,
                    "total": ctx.total,
                    "start": ctx.start,
                    "isFirst": ctx.is_first,
                    "isLast": ctx.is_last,
                });
                if let Some(kind) = &ctx.kind {
                    obj["listContext"]["kind"] = serde_json::Value::String(kind.clone());
                }
                if let Some(checked) = ctx.checked {
                    obj["listContext"]["checked"] = serde_json::Value::Bool(checked);
                }
            }
            obj
        }
        crate::render::RenderElement::BlockEnd => {
            serde_json::json!({"type": "blockEnd"})
        }
    })
}

fn serialize_render_mark(mark: &crate::render::RenderMark) -> serde_json::Value {
    if mark.attrs.is_empty() {
        serde_json::Value::String(mark.mark_type.clone())
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "type".to_string(),
            serde_json::Value::String(mark.mark_type.clone()),
        );
        for (key, value) in &mark.attrs {
            obj.insert(key.clone(), value.clone());
        }
        serde_json::Value::Object(obj)
    }
}

fn selection_to_json(
    selection: &crate::selection::Selection,
    scalar_selection: Option<&crate::selection::Selection>,
) -> serde_json::Value {
    match (selection, scalar_selection) {
        (
            crate::selection::Selection::Text { anchor, head },
            Some(crate::selection::Selection::Text {
                anchor: anchor_scalar,
                head: head_scalar,
            }),
        ) => serde_json::json!({
            "type": "text",
            "anchor": anchor,
            "head": head,
            "anchorScalar": anchor_scalar,
            "headScalar": head_scalar,
        }),
        (crate::selection::Selection::Text { anchor, head }, _) => {
            serde_json::json!({"type": "text", "anchor": anchor, "head": head})
        }
        (
            crate::selection::Selection::Node { pos },
            Some(crate::selection::Selection::Node { pos: pos_scalar }),
        ) => serde_json::json!({
            "type": "node",
            "pos": pos,
            "posScalar": pos_scalar,
        }),
        (crate::selection::Selection::Node { pos }, _) => {
            serde_json::json!({"type": "node", "pos": pos})
        }
        (crate::selection::Selection::Cell { anchor, head }, _) => serde_json::json!({
            "type": "cell",
            "anchorCell": anchor,
            "headCell": head,
        }),
        (crate::selection::Selection::All, _) => serde_json::json!({"type": "all"}),
    }
}

fn serialize_active_state(active_state: &crate::editor_state::ActiveState) -> serde_json::Value {
    serde_json::json!({
        "marks": &active_state.marks,
        "markAttrs": &active_state.mark_attrs,
        "nodes": &active_state.nodes,
        "commands": &active_state.commands,
        "allowedMarks": &active_state.allowed_marks,
        "insertableNodes": &active_state.insertable_nodes,
    })
}
