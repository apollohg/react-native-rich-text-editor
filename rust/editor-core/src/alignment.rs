use crate::boundary::ResourceLimits;
use crate::model::{Document, Node};
use crate::schema::{AttrSpec, NodeRole, NodeSpec, Schema};
use crate::selection::Selection;

pub const TEXT_ALIGN_ATTR: &str = "textAlign";
static SUPPORTED_ATTR_VALUES: std::sync::LazyLock<[serde_json::Value; 4]> =
    std::sync::LazyLock::new(|| {
        [
            serde_json::Value::Null,
            serde_json::json!(TextAlign::Left),
            serde_json::json!(TextAlign::Center),
            serde_json::json!(TextAlign::Right),
        ]
    });
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}
impl TextAlign {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "left" => Some(Self::Left),
            "center" => Some(Self::Center),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}
pub(crate) fn attr_spec() -> AttrSpec {
    AttrSpec {
        default: Some(serde_json::Value::Null),
        has_default: true,
        constraints: [(
            "enum".to_owned(),
            serde_json::json!([null, "left", "center", "right"]),
        )]
        .into(),
        ..AttrSpec::default()
    }
}
pub(crate) fn eligible(spec: &NodeSpec) -> bool {
    matches!(spec.role, NodeRole::TextBlock)
        && spec.attrs.get(TEXT_ALIGN_ATTR).is_some_and(|attr| {
            SUPPORTED_ATTR_VALUES
                .iter()
                .all(|value| attr.validate_value(value).is_ok())
        })
        && (spec.name == "paragraph"
            || spec.name == "heading"
            || matches!(
                spec.html_rules
                    .as_ref()
                    .map(|rules| rules.tag.as_str())
                    .or(spec.html_tag.as_deref()),
                Some("p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
            ))
}
pub(crate) fn value(node: &Node) -> Option<TextAlign> {
    node.attrs()
        .get(TEXT_ALIGN_ATTR)
        .and_then(|v| v.as_str())
        .and_then(TextAlign::parse)
}
pub(crate) fn render_value(node: &Node, schema: &Schema) -> Option<TextAlign> {
    schema
        .node(node.node_type())
        .filter(|spec| eligible(spec))
        .and_then(|_| value(node))
}
pub(crate) fn selection_scalar_range(
    document: &Document,
    schema: &Schema,
    map: &crate::position::PositionMap,
    opening: u32,
) -> Option<(u32, u32)> {
    let content_start = opening.checked_add(1)?;
    let resolved = document.resolve(content_start).ok()?;
    let node = resolved.parent(document);
    if resolved.parent_offset != 0
        || resolved.node_path.is_empty()
        || !schema.node(node.node_type()).is_some_and(eligible)
    {
        return None;
    }
    let index = map.find_block_for_doc_pos(content_start)?;
    let block = map.block(index)?;
    if map.effective_doc_start(index) != content_start || block.node_path != resolved.node_path {
        return None;
    }
    let start = map
        .effective_scalar_start(index)
        .checked_add(block.scalar_prefix_len)?;
    Some((start, start.checked_add(block.scalar_len)?))
}
pub(crate) fn targets<'a>(
    document: &'a Document,
    schema: &Schema,
    selection: &Selection,
    limits: &ResourceLimits,
) -> Vec<(u32, &'a Node)> {
    targets_in(document, schema, selection, limits, None)
}
pub(crate) fn targets_in<'a>(
    document: &'a Document,
    schema: &Schema,
    selection: &Selection,
    limits: &ResourceLimits,
    table_index: Option<&crate::tables::admission::TableProjectionIndex>,
) -> Vec<(u32, &'a Node)> {
    if let Selection::Text { anchor, head } = selection {
        if anchor == head {
            let Ok(resolved) = document.resolve(*anchor) else {
                return Vec::new();
            };
            let node = resolved.parent(document);
            return if schema.node(node.node_type()).is_some_and(eligible) {
                vec![(anchor.saturating_sub(resolved.parent_offset + 1), node)]
            } else {
                Vec::new()
            };
        }
    }
    if !schema.all_nodes().any(eligible) {
        return Vec::new();
    }
    let cells = if let Selection::Cell { anchor, head } = selection {
        let owned_index;
        let index = match table_index {
            Some(index) => index,
            None => {
                owned_index = crate::tables::admission::TableProjectionIndex::derive_or_fallback(
                    document, schema, limits,
                );
                &owned_index
            }
        };
        let Some(rect) = crate::tables::selection::resolve_cell_rect(index, *anchor, *head) else {
            return Vec::new();
        };
        Some(
            rect.cells
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
        )
    } else {
        None
    };
    let mut stack = vec![(document.root(), 0u32, None)];
    let mut found = Vec::new();
    let mut visited = 0usize;
    while let Some((node, pos, cell)) = stack.pop() {
        visited += 1;
        if visited > limits.max_document_nodes {
            return Vec::new();
        }
        if !std::ptr::eq(node, document.root()) {
            match selection {
                Selection::Text { anchor, head }
                    if pos >= (*anchor).max(*head)
                        || pos + node.node_size() <= (*anchor).min(*head) =>
                {
                    continue
                }
                Selection::Node { pos: at } if pos > *at || pos + node.node_size() <= *at => {
                    continue
                }
                _ => {}
            }
        }
        let spec = schema.node(node.node_type());
        let inside_selected_cell =
            cell.is_some_and(|at| cells.as_ref().is_some_and(|cells| cells.contains(&at)));
        let cell = if !inside_selected_cell
            && spec.is_some_and(|s| {
                matches!(
                    s.table_role,
                    Some(crate::tables::TableRole::Cell | crate::tables::TableRole::HeaderCell)
                )
            }) {
            Some(pos)
        } else {
            cell
        };
        if spec.is_some_and(eligible) {
            let selected = match selection {
                Selection::All => true,
                Selection::Node { pos: at } => *at == pos,
                Selection::Cell { .. } => {
                    cell.is_some_and(|at| cells.as_ref().is_some_and(|cells| cells.contains(&at)))
                }
                Selection::Text { anchor, head } => {
                    let from = (*anchor).min(*head);
                    let to = (*anchor).max(*head);
                    if from == to {
                        pos < from && from < pos + node.node_size()
                    } else {
                        (from < pos + node.node_size() - 1
                            || (node.content_size() == 0 && from == pos + 1))
                            && to > pos + 1
                    }
                }
            };
            if selected {
                found.push((pos, node));
            }
        }
        if let Some(content) = node.content() {
            let mut offset = if std::ptr::eq(node, document.root()) {
                0
            } else {
                pos + 1
            };
            let mut children = Vec::with_capacity(content.child_count());
            for child in content.iter() {
                children.push((child, offset, cell));
                offset += child.node_size();
            }
            stack.extend(children.into_iter().rev());
        }
    }
    found
}
pub(crate) fn state(targets: &[(u32, &Node)]) -> Option<&'static str> {
    let (_, first) = targets.first()?;
    let first = value(first);
    if targets.iter().any(|(_, node)| value(node) != first) {
        Some("mixed")
    } else {
        first.map(TextAlign::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yrs_engine::{
        EditingLimits, InitializationMode, TransactionOrigin, TypedCommand, YrsDocumentEngine,
        YrsEngineConfig,
    };
    fn engine() -> YrsDocumentEngine {
        engine_with_mode(InitializationMode::LocalEmpty)
    }
    fn engine_with_mode(initialization_mode: InitializationMode) -> YrsDocumentEngine {
        YrsDocumentEngine::new(YrsEngineConfig {
            schema: crate::tiptap_schema(),
            fragment_name: "prosemirror".into(),
            initialization_mode,
            resource_limits: ResourceLimits::default(),
            editing_limits: EditingLimits::default(),
            max_length: None,
            scope: None,
        })
        .unwrap()
    }
    fn select(
        engine: &mut YrsDocumentEngine,
        request_id: u64,
        selection: crate::yrs_engine::SelectionInput,
    ) {
        engine
            .apply_typed_transaction(crate::yrs_engine::TypedTransaction {
                request_id,
                base_document_revision: engine.revision(),
                origin: TransactionOrigin::LocalApi,
                operations: vec![],
                selection_intent: crate::yrs_engine::SelectionIntent::Set(selection),
                history_policy: crate::yrs_engine::HistoryPolicy::Skip,
            })
            .unwrap();
    }
    #[test]
    fn alignment_caret_noop_history_and_render() {
        let mut engine = engine();
        let revision = engine.revision();
        engine
            .apply_command(
                1,
                TypedCommand::SetTextAlign {
                    align: TextAlign::Center,
                },
            )
            .unwrap();
        assert_eq!(
            engine.revision(),
            revision + 1,
            "empty paragraph alignment is document content"
        );
        assert_eq!(
            engine.active_state().unwrap().text_align.as_deref(),
            Some("center")
        );
        assert!(engine
            .document_html()
            .unwrap()
            .contains("style=\"text-align:center\""));
        let revision = engine.revision();
        engine
            .apply_command(
                2,
                TypedCommand::SetTextAlign {
                    align: TextAlign::Center,
                },
            )
            .unwrap();
        assert_eq!(
            engine.revision(),
            revision,
            "uniform alignment must not produce a revision"
        );
        engine.undo(3).unwrap();
        assert_eq!(
            engine.active_state().unwrap().text_align,
            None,
            "one undo removes the effective alignment transaction"
        );
        engine.redo(4).unwrap();
        assert_eq!(
            engine.active_state().unwrap().text_align.as_deref(),
            Some("center")
        );
        let elements =
            crate::render::generate::generate(engine.document().unwrap(), &crate::tiptap_schema());
        assert!(matches!(
            elements.unwrap().first(),
            Some(crate::render::RenderElement::BlockStart {
                text_align: Some(TextAlign::Center),
                ..
            })
        ));
        engine
            .apply_command(5, TypedCommand::UnsetTextAlign)
            .unwrap();
        assert!(!engine.document_html().unwrap().contains("text-align"));
    }
    #[test]
    fn alignment_ranges_exclude_next_block_start_and_preserve_direction() {
        let schema = crate::tiptap_schema();
        let document = crate::serialize::from_html(
            "<p>abc</p><h2>def</h2><pre>code</pre><p></p>",
            &schema,
            &Default::default(),
        )
        .unwrap();
        let limits = ResourceLimits::default();
        for selection in [Selection::text(2, 6), Selection::text(6, 2)] {
            assert_eq!(
                targets(&document, &schema, &selection, &limits)
                    .iter()
                    .map(|(p, _)| *p)
                    .collect::<Vec<_>>(),
                vec![0]
            );
        }
        assert_eq!(
            targets(&document, &schema, &Selection::all(), &limits).len(),
            3
        );
        assert_eq!(
            targets(&document, &schema, &Selection::node(5), &limits).len(),
            1
        );
        assert!(targets(&document, &schema, &Selection::node(10), &limits).is_empty());
    }
    #[test]
    fn alignment_html_json_and_heading_split_preserve_attributes() {
        let mut engine = engine();
        engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","attrs":{"textAlign":"right"},"content":[{"type":"text","text":"hello"}]}]}"#, TransactionOrigin::LocalApi).unwrap();
        engine
            .apply_command(1, TypedCommand::ToggleHeading { level: 2 })
            .unwrap();
        assert!(engine
            .document_html()
            .unwrap()
            .starts_with("<h2 style=\"text-align:right\">"));
        engine.apply_command(2, TypedCommand::SplitBlock).unwrap();
        let json = engine.document_json().unwrap();
        for node in json["content"].as_array().unwrap() {
            assert_eq!(
                node["attrs"][TEXT_ALIGN_ATTR], "right",
                "split must retain alignment: {json}"
            );
        }
        let schema = crate::tiptap_schema();
        let html="<p style=\"color:red;text-align:center\">abc</p><h3 style=\"text-align:justify\">def</h3>";
        let doc = crate::serialize::from_html(html, &schema, &Default::default()).unwrap();
        let out = crate::serialize::to_html(&doc, &schema);
        assert_eq!(out, "<p style=\"text-align:center\">abc</p><h3>def</h3>");
    }
    #[test]
    fn alignment_nested_mixed_and_schema_opt_in() {
        let schema = crate::tiptap_schema();
        let doc=crate::serialize::from_html("<blockquote><p style=\"text-align:left\">a</p></blockquote><ul><li><p>b</p></li></ul><pre>code</pre>",&schema,&Default::default()).unwrap();
        let targets = targets(&doc, &schema, &Selection::all(), &ResourceLimits::default());
        assert_eq!(targets.len(), 2, "only nested paragraphs are eligible");
        assert_eq!(
            state(&targets),
            Some("mixed"),
            "explicit left differs from inheritance"
        );
        let unsupported=Schema::from_json(&serde_json::json!({"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock","htmlTag":"p"},{"name":"text","group":"inline","role":"text"}],"marks":[]})).unwrap();
        let doc =
            crate::serialize::from_html("<p>a</p>", &unsupported, &Default::default()).unwrap();
        assert!(
            super::targets(
                &doc,
                &unsupported,
                &Selection::all(),
                &ResourceLimits::default()
            )
            .is_empty(),
            "custom schemas must opt in"
        );
    }
    #[test]
    fn alignment_merged_cell_rectangle_visits_each_paragraph_once() {
        let schema = crate::schema::presets::tiptap_table_schema();
        let doc=crate::serialize::from_html("<table><tr><td colspan=\"2\"><p>a</p><h2>b</h2></td></tr><tr><td><p>c</p></td><td><p>d</p></td></tr></table>",&schema,&Default::default()).unwrap();
        let limits = ResourceLimits::default();
        let index = crate::tables::admission::TableProjectionIndex::derive_or_fallback(
            &doc, &schema, &limits,
        );
        let table = index.table_at(0).unwrap();
        let first = table.cells.first().unwrap().source_pos;
        let last = table.cells.last().unwrap().source_pos;
        let selected = targets(&doc, &schema, &Selection::cell(first, last), &limits);
        assert_eq!(
            selected.len(),
            4,
            "merged cells and multiple eligible descendants are formatted once"
        );
        let reversed = targets(&doc, &schema, &Selection::cell(last, first), &limits);
        assert_eq!(selected, reversed);
    }
    #[test]
    fn alignment_concurrent_text_survives_local_format_undo() {
        let mut local = engine();
        local
            .apply_command(
                1,
                TypedCommand::InsertText {
                    text: "base".into(),
                },
            )
            .unwrap();
        let mut remote = engine_with_mode(InitializationMode::AwaitRemote);
        remote
            .apply_remote_update_v1(2, &local.encoded_state().unwrap())
            .unwrap();
        local
            .apply_command(
                3,
                TypedCommand::SetTextAlign {
                    align: TextAlign::Right,
                },
            )
            .unwrap();
        remote
            .apply_command(
                4,
                TypedCommand::InsertText {
                    text: "remote".into(),
                },
            )
            .unwrap();
        local
            .apply_remote_update_v1(5, &remote.encoded_state().unwrap())
            .unwrap();
        assert!(local.document_html().unwrap().contains("text-align:right"));
        assert!(local.document_html().unwrap().contains("remote"));
        local.undo(6).unwrap();
        let html = local.document_html().unwrap();
        assert!(
            html.contains("remote") && html.contains("base"),
            "format undo preserves unrelated remote text: {html}"
        );
        assert!(
            !html.contains("text-align"),
            "format undo resets inheritance: {html}"
        );
    }
    #[test]
    fn alignment_only_render_changes_equality() {
        let schema = crate::tiptap_schema();
        let before = crate::serialize::from_html("<p>a</p>", &schema, &Default::default()).unwrap();
        let after = crate::serialize::from_html(
            "<p style=\"text-align:center\">a</p>",
            &schema,
            &Default::default(),
        )
        .unwrap();
        let left = crate::render::incremental::render_blocks(&before, &schema);
        let right = crate::render::incremental::render_blocks(&after, &schema);
        assert_ne!(
            left, right,
            "alignment-only edits must invalidate rendered layout"
        );
        assert!(matches!(
            right[0][0],
            crate::render::RenderElement::BlockStart {
                text_align: Some(TextAlign::Center),
                ..
            }
        ));
    }
    #[test]
    fn alignment_runtime_values_and_document_attrs_reject_invalid_inputs() {
        for raw in [
            r#"{"type":"setTextAlign","align":"justify"}"#,
            r#"{"type":"setTextAlign","align":null}"#,
            r#"{"type":"setTextAlign","align":1}"#,
        ] {
            assert!(
                crate::native_transaction_bridge::table_command_envelope_for_test(raw).is_err(),
                "native command boundary must reject {raw}"
            );
        }
        assert!(matches!(
            crate::native_transaction_bridge::table_command_envelope_for_test(
                r#"{"type":"setTextAlign","align":"left"}"#
            )
            .unwrap(),
            TypedCommand::SetTextAlign {
                align: TextAlign::Left
            }
        ));
        let mut engine = engine();
        let before = engine.document_json();
        let invalid = serde_json::json!({"type":"doc","content":[{"type":"paragraph","attrs":{"textAlign":"justify"}}]});
        assert!(engine
            .import_json(&invalid.to_string(), TransactionOrigin::LocalApi)
            .is_err());
        assert_eq!(
            engine.document_json(),
            before,
            "invalid attrs cannot mutate document"
        );
    }
    #[test]
    fn alignment_prepared_viewer_semantic_key_includes_alignment() {
        let compile = |html: &str| {
            crate::viewer::viewer_compile(crate::viewer::FfiViewerCompileRequest {
                source_kind: crate::viewer::FfiViewerSourceKind::Html,
                source: html.into(),
                config_json: r#"{"initialization":{"type":"localEmpty"}}"#.into(),
                images_enabled: true,
                mention_prefix: None,
            })
            .value
            .unwrap()
        };
        let inherited = compile("<p>a</p>");
        let center = compile("<p style=\"text-align:center\">a</p>");
        assert_ne!(
            inherited.semantic_key(),
            center.semantic_key(),
            "prepared viewer caches must invalidate for alignment alone"
        );
        assert!(
            matches!(center.elements().first(),Some(crate::viewer::FfiViewerElement::BlockStart{text_align:Some(align),..}) if align == "center")
        );
    }
    #[test]
    fn alignment_active_state_tracks_cursor_moves_and_localized_typing() {
        use crate::yrs_engine::{
            Affinity, EditorOffsetKind, HistoryPolicy, RevisionedPosition, SelectionInput,
            SelectionIntent, TypedTransaction,
        };
        let mut engine = engine();
        engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","attrs":{"textAlign":"left"},"content":[{"type":"text","text":"a"}]},{"type":"paragraph","attrs":{"textAlign":"right"},"content":[{"type":"text","text":"b"}]}]}"#,TransactionOrigin::LocalApi).unwrap();
        for (request, offset, expected) in [(1, 0, "left"), (2, 2, "right"), (3, 0, "left")] {
            let point = RevisionedPosition {
                offset,
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::After,
            };
            engine
                .apply_typed_transaction(TypedTransaction {
                    request_id: request,
                    base_document_revision: engine.revision(),
                    origin: TransactionOrigin::LocalApi,
                    operations: vec![],
                    selection_intent: SelectionIntent::Set(SelectionInput::Text {
                        anchor: point,
                        head: point,
                    }),
                    history_policy: HistoryPolicy::Skip,
                })
                .unwrap();
            assert_eq!(
                engine.active_state().unwrap().text_align.as_deref(),
                Some(expected),
                "selection move must invalidate cached alignment"
            );
        }
        for request in 4..7 {
            engine
                .apply_command(request, TypedCommand::InsertText { text: "x".into() })
                .unwrap();
            let state = engine.active_state().unwrap();
            assert_eq!(state.text_align.as_deref(), Some("left"));
            assert_eq!(state.commands.get("unsetTextAlign"), Some(&true));
        }
    }
    #[test]
    fn alignment_select_all_formats_only_eligible_blocks_in_one_undo_group() {
        let mut engine = engine();
        engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]},{"type":"heading","attrs":{"level":3},"content":[{"type":"text","text":"b"}]},{"type":"codeBlock","content":[{"type":"text","text":"code"}]}]}"#,TransactionOrigin::LocalApi).unwrap();
        select(&mut engine, 1, crate::yrs_engine::SelectionInput::All);
        let before = engine.document_json();
        let revision = engine.revision();
        engine
            .apply_command(
                2,
                TypedCommand::SetTextAlign {
                    align: TextAlign::Center,
                },
            )
            .unwrap();
        assert_eq!(engine.revision(), revision + 1);
        let json = engine.document_json().unwrap();
        assert_eq!(json["content"][0]["attrs"][TEXT_ALIGN_ATTR], "center");
        assert_eq!(json["content"][1]["attrs"][TEXT_ALIGN_ATTR], "center");
        assert_eq!(json["content"][1]["attrs"]["level"], 3);
        assert!(json["content"][2]["attrs"].get(TEXT_ALIGN_ATTR).is_none());
        engine.undo(3).unwrap();
        assert_eq!(
            engine.document_json(),
            before,
            "all selected blocks share one formatting undo item"
        );
    }
    #[test]
    fn alignment_code_conversion_removes_alignment_and_join_keeps_surviving_attrs() {
        let mut engine = engine();
        engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","attrs":{"textAlign":"left"},"content":[{"type":"text","text":"a"}]},{"type":"paragraph","attrs":{"textAlign":"right"},"content":[{"type":"text","text":"b"}]}]}"#,TransactionOrigin::LocalApi).unwrap();
        let point = crate::yrs_engine::RevisionedPosition {
            offset: 2,
            kind: crate::yrs_engine::EditorOffsetKind::Scalar,
            affinity: crate::yrs_engine::Affinity::After,
        };
        select(
            &mut engine,
            1,
            crate::yrs_engine::SelectionInput::Text {
                anchor: point,
                head: point,
            },
        );
        engine
            .apply_command(2, TypedCommand::DeleteBackward)
            .unwrap();
        assert_eq!(
            engine.document_json().unwrap()["content"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            engine.active_state().unwrap().text_align.as_deref(),
            Some("left"),
            "join follows surviving first block alignment"
        );
        engine
            .apply_command(3, TypedCommand::ToggleCodeBlock)
            .unwrap();
        let state = engine.active_state().unwrap();
        assert_eq!(state.text_align, None);
        assert_eq!(state.commands.get("setTextAlign"), Some(&false));
        assert!(!engine.document_html().unwrap().contains("text-align"));
    }
    #[test]
    fn alignment_outer_cell_rectangle_includes_nested_cell_text() {
        let schema = crate::schema::presets::tiptap_table_schema();
        let document=crate::serialize::from_html("<table><tr><td><p>outer</p><table><tr><td><p>inner</p></td></tr></table></td></tr></table>",&schema,&Default::default()).unwrap();
        let limits = ResourceLimits::default();
        let index = crate::tables::admission::TableProjectionIndex::derive_or_fallback(
            &document, &schema, &limits,
        );
        let cell = index.table_at(0).unwrap().cells[0].source_pos;
        assert_eq!(
            targets(&document, &schema, &Selection::cell(cell, cell), &limits).len(),
            2,
            "outer selected cells include eligible text in nested tables"
        );
    }
    #[test]
    fn alignment_updates_preserve_xml_branches_and_concurrent_text_identity() {
        use yrs::updates::decoder::Decode;
        use yrs::{GetString, ReadTxn, Text, Transact, XmlFragment, XmlOut};
        let mut native = engine();
        native
            .apply_command(
                1,
                TypedCommand::InsertText {
                    text: "alpha".into(),
                },
            )
            .unwrap();
        let read_branches = |txn: &yrs::Transaction<'_>, fragment: &yrs::XmlFragmentRef| {
            let XmlOut::Element(paragraph) = fragment.get(txn, 0).unwrap() else {
                panic!("paragraph XML element");
            };
            let XmlOut::Text(text) = paragraph.get(txn, 0).unwrap() else {
                panic!("paragraph XML text");
            };
            (paragraph, text)
        };
        let read_identities = |txn: &yrs::Transaction<'_>, fragment: &yrs::XmlFragmentRef| {
            let (paragraph, text) = read_branches(txn, fragment);
            (
                <yrs::XmlElementRef as AsRef<yrs::branch::Branch>>::as_ref(&paragraph).id(),
                <yrs::XmlTextRef as AsRef<yrs::branch::Branch>>::as_ref(&text).id(),
            )
        };
        let (original_paragraph, original_text) =
            native.read_fragment_for_test(read_identities).unwrap();
        let web = yrs::Doc::new();
        web.transact_mut()
            .apply_update(yrs::Update::decode_v1(&native.encoded_state().unwrap()).unwrap())
            .unwrap();
        let (web_paragraph, web_text) = {
            let txn = web.transact();
            let fragment = txn.get_xml_fragment("prosemirror").unwrap();
            read_branches(&txn, &fragment)
        };
        native
            .apply_command(
                2,
                TypedCommand::SetTextAlign {
                    align: TextAlign::Center,
                },
            )
            .unwrap();
        let (aligned_paragraph, aligned_text) =
            native.read_fragment_for_test(read_identities).unwrap();
        assert_eq!(
            original_paragraph, aligned_paragraph,
            "alignment must update attrs on the original shared XML element"
        );
        assert_eq!(
            original_text, aligned_text,
            "alignment must retain the shared XML text branch"
        );
        web_text.insert(&mut web.transact_mut(), 0, "web ");
        let update = web
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());
        native.apply_remote_update_v1(3, &update).unwrap();
        web.transact_mut()
            .apply_update(yrs::Update::decode_v1(&native.encoded_state().unwrap()).unwrap())
            .unwrap();
        {
            let txn = web.transact();
            let fragment = txn.get_xml_fragment("prosemirror").unwrap();
            let (paragraph, text) = read_branches(&txn, &fragment);
            assert_eq!(paragraph, web_paragraph);
            assert_eq!(text, web_text);
            assert_eq!(web_text.get_string(&txn), "web alpha");
        }
        native.undo(4).unwrap();
        let (undone_paragraph, undone_text) =
            native.read_fragment_for_test(read_identities).unwrap();
        assert_eq!(
            original_paragraph, undone_paragraph,
            "format undo must preserve original element identity"
        );
        assert_eq!(
            original_text, undone_text,
            "format undo must preserve original text identity"
        );
        assert!(native.document_html().unwrap().contains("web alpha"));
        assert!(!native.document_html().unwrap().contains("text-align"));
    }
    #[test]
    fn alignment_ranges_include_initial_empty_blocks_but_exclude_final_boundary() {
        let schema = crate::tiptap_schema();
        let limits = ResourceLimits::default();
        let document =
            crate::serialize::from_html("<p></p><p>x</p>", &schema, &Default::default()).unwrap();
        for selection in [Selection::text(1, 4), Selection::text(4, 1)] {
            assert_eq!(
                targets(&document, &schema, &selection, &limits)
                    .iter()
                    .map(|(pos, _)| *pos)
                    .collect::<Vec<_>>(),
                vec![0, 2],
                "range beginning in an empty paragraph includes that paragraph: {selection:?}"
            );
        }
        let document =
            crate::serialize::from_html("<p>x</p><p></p>", &schema, &Default::default()).unwrap();
        for selection in [Selection::text(1, 4), Selection::text(4, 1)] {
            assert_eq!(
                targets(&document, &schema, &selection, &limits)
                    .iter()
                    .map(|(pos, _)| *pos)
                    .collect::<Vec<_>>(),
                vec![0],
                "range ending at an empty paragraph's start excludes it: {selection:?}"
            );
        }
    }
    #[test]
    fn alignment_node_selection_survives_attribute_commands_and_undo() {
        for node_type in ["paragraph", "heading"] {
            let mut engine = engine();
            let first = serde_json::json!({"type":node_type,"attrs":{"level":2},"content":[{"type":"text","text":"first"}]});
            let source = serde_json::json!({"type":"doc","content":[first,{"type":"paragraph","content":[{"type":"text","text":"second"}]}]});
            engine
                .import_json(&source.to_string(), TransactionOrigin::LocalApi)
                .unwrap();
            select(
                &mut engine,
                1,
                crate::yrs_engine::SelectionInput::Node {
                    at: (crate::yrs_engine::RevisionedPosition {
                        offset: 0,
                        kind: crate::yrs_engine::EditorOffsetKind::Scalar,
                        affinity: crate::yrs_engine::Affinity::Before,
                    })
                    .into(),
                },
            );
            for (request, command, expected) in [
                (
                    2,
                    TypedCommand::SetTextAlign {
                        align: TextAlign::Right,
                    },
                    Some("right"),
                ),
                (3, TypedCommand::UnsetTextAlign, None),
            ] {
                engine.apply_command(request, command).unwrap();
                assert!(
                    matches!(engine.resolved_selection(),Some(crate::yrs_engine::ResolvedSelection::Node {at}) if at.document == 0),
                    "formatting retains the selected {node_type} opening"
                );
                assert_eq!(
                    engine.active_state().unwrap().text_align.as_deref(),
                    expected
                );
                assert!(
                    engine.document_json().unwrap()["content"][1]["attrs"]
                        .get(TEXT_ALIGN_ATTR)
                        .is_none(),
                    "node selection must not affect adjacent blocks"
                );
            }
            engine.undo(4).unwrap();
            assert_eq!(
                engine.active_state().unwrap().text_align.as_deref(),
                Some("right")
            );
            assert!(
                matches!(engine.resolved_selection(),Some(crate::yrs_engine::ResolvedSelection::Node {at}) if at.document == 0)
            );
        }
    }
    #[test]
    fn alignment_document_node_openings_disambiguate_empty_nested_and_leading_atoms() {
        use crate::yrs_engine::{Affinity, NodeSelectionPoint, ResolvedSelection, SelectionInput};
        for (source, opening) in [
            (
                serde_json::json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"hardBreak"},{"type":"text","text":"a"}]}]}),
                0,
            ),
            (
                serde_json::json!({"type":"doc","content":[{"type":"paragraph"}]}),
                0,
            ),
            (
                serde_json::json!({"type":"doc","content":[{"type":"blockquote","content":[{"type":"paragraph","content":[{"type":"text","text":"nested"}]}]}]}),
                1,
            ),
        ] {
            let mut engine = engine();
            engine
                .import_json(&source.to_string(), TransactionOrigin::LocalApi)
                .unwrap();
            select(
                &mut engine,
                1,
                SelectionInput::Node {
                    at: NodeSelectionPoint::Document {
                        opening,
                        affinity: Affinity::Before,
                    },
                },
            );
            engine
                .apply_command(
                    2,
                    TypedCommand::SetTextAlign {
                        align: TextAlign::Center,
                    },
                )
                .unwrap();
            assert!(
                matches!(engine.resolved_selection(),Some(ResolvedSelection::Node {at}) if at.document == opening)
            );
            assert_eq!(
                engine.active_state().unwrap().text_align.as_deref(),
                Some("center")
            );
            engine.undo(3).unwrap();
            assert!(
                matches!(engine.resolved_selection(),Some(ResolvedSelection::Node {at}) if at.document == opening)
            );
        }
    }
    #[test]
    fn alignment_node_document_offsets_reject_interiors_and_scalar_atoms_keep_precedence() {
        use crate::yrs_engine::{
            Affinity, EditorOffsetKind, HistoryPolicy, NodeSelectionPoint, RevisionedPosition,
            SelectionInput, SelectionIntent, TypedTransaction,
        };
        let mut engine = engine();
        engine
            .apply_command(1, TypedCommand::InsertText { text: "abc".into() })
            .unwrap();
        let invalid = TypedTransaction {
            request_id: 2,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: vec![],
            selection_intent: SelectionIntent::Set(SelectionInput::Node {
                at: NodeSelectionPoint::Document {
                    opening: 1,
                    affinity: Affinity::Before,
                },
            }),
            history_policy: HistoryPolicy::Skip,
        };
        assert!(
            engine.apply_typed_transaction(invalid).is_err(),
            "document coordinates require exact node openings"
        );
        engine.import_json(r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"hardBreak"},{"type":"text","text":"a"}]}]}"#,TransactionOrigin::LocalApi).unwrap();
        select(
            &mut engine,
            3,
            SelectionInput::Node {
                at: RevisionedPosition {
                    offset: 0,
                    kind: EditorOffsetKind::Scalar,
                    affinity: Affinity::Before,
                }
                .into(),
            },
        );
        assert!(
            matches!(engine.resolved_selection(),Some(crate::yrs_engine::ResolvedSelection::Node {at}) if at.document == 1)
        );
        assert_eq!(
            engine.active_state().unwrap().commands.get("setTextAlign"),
            Some(&false),
            "legacy scalar node input still selects the leading inline atom"
        );
    }
    #[test]
    fn alignment_node_extents_exclude_following_separator_and_preserve_empty_bounds() {
        let schema = crate::tiptap_schema();
        for (html, opening, expected) in [
            ("<p>abc</p><p>x</p>", 0, 3),
            ("<p></p><p>x</p>", 0, 1),
            ("<blockquote><p>abc</p></blockquote><p>x</p>", 1, 3),
        ] {
            let document = crate::serialize::from_html(html, &schema, &Default::default()).unwrap();
            let map = crate::position::PositionMap::build(&document, &schema);
            assert_eq!(
                selection_scalar_range(&document, &schema, &map, opening).map(|(_, end)| end),
                Some(expected),
                "selected node ends before subsequent separators: {html}"
            );
        }
    }
    #[test]
    fn alignment_empty_cell_node_extent_matches_native_cell_input_bounds() {
        let schema = crate::schema::presets::tiptap_table_schema();
        let document = crate::serialize::from_html(
            "<table><tr><td><p></p></td><td><p>x</p></td></tr></table>",
            &schema,
            &Default::default(),
        )
        .unwrap();
        let map = crate::position::PositionMap::build(&document, &schema);
        let opening = 3;
        let index = map.find_block_for_doc_pos(opening + 1).unwrap();
        let block = map.block(index).unwrap();
        let cell_input_end =
            map.effective_scalar_start(index) + block.scalar_prefix_len + block.scalar_len;
        assert_eq!(
            selection_scalar_range(&document, &schema, &map, opening).map(|(_, end)| end),
            Some(cell_input_end),
            "node end matches FfiCellInputBlock scalarEnd"
        );
        assert_eq!(
            cell_input_end,
            map.effective_scalar_start(index) + block.scalar_prefix_len + 1,
            "empty cell includes its rendered placeholder"
        );
    }
}
