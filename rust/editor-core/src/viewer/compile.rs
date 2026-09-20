use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::boundary::serialize_json_value_stack_safe;
use crate::render::{ListContext, RenderElement, RenderMark};
use crate::schema::schema_fingerprint;

use super::types::{
    FfiViewerCompileRequest, FfiViewerCompileResult, FfiViewerElement, FfiViewerMark,
    ViewerCompiledDocument,
};

const VIEWER_SEMANTIC_KEY_VERSION: u8 = 1;

pub(crate) fn compile(request: FfiViewerCompileRequest) -> FfiViewerCompileResult {
    let result = crate::boundary::with_document_stack(|| {
        let resolved = crate::ffi_v2::editor::resolve_local_document(
            &request.config_json,
            request.source_kind.clone(),
            &request.source,
        )?;
        let is_empty = crate::editor_state::document_is_empty_after_omitting(
            &resolved.document,
            &resolved.schema,
            |node| !request.images_enabled && is_image_node(node),
        );
        let trailing_empty_text_block_count = u32::try_from(
            crate::editor_state::trailing_empty_text_block_count_after_omitting(
                &resolved.document,
                &resolved.schema,
                |node| !request.images_enabled && is_image_node(node),
            ),
        )
        .unwrap_or(u32::MAX);
        let preferred_text_block_name = resolved
            .schema
            .preferred_text_block()
            .map(|spec| spec.name.clone())
            .unwrap_or_default();
        let render_cache = crate::render::incremental::CachedRenderBlocks::build(
            &resolved.document,
            &resolved.schema,
            &resolved.resource_limits,
        )
        .map_err(|_| {
            crate::boundary::BoundaryError::new(
                "DOCUMENT_INVALID",
                "table render preparation failed",
            )
        })?;
        let mut table_records = std::collections::BTreeMap::new();
        let elements =
            crate::render::incremental::flatten_render_blocks(&render_cache.materialize())
                .into_iter()
                .filter(|element| request.images_enabled || !is_image_atom(element))
                .map(|element| {
                    viewer_element(
                        element,
                        request.mention_prefix.as_deref(),
                        request.images_enabled,
                        &mut table_records,
                    )
                })
                .collect::<Vec<_>>();

        let table_records = table_records.into_values().collect::<Vec<_>>();
        let table_attributes: HashMap<String, String> = render_cache
            .table_attributes
            .iter()
            .map(|(key, json)| (key.clone(), json.to_string()))
            .collect();
        let semantic_key = semantic_key(
            &schema_fingerprint(&resolved.schema),
            &elements,
            &table_records,
            &table_attributes,
            request.images_enabled,
            request.mention_prefix.as_deref(),
        );
        let retained_bytes = retained_bytes(
            &semantic_key,
            &elements,
            &table_records,
            &table_attributes,
            &preferred_text_block_name,
        );

        Ok::<_, crate::session::SessionError>(Arc::new(ViewerCompiledDocument {
            table_attributes,
            semantic_key,
            elements,
            table_records,
            is_empty,
            preferred_text_block_name,
            trailing_empty_text_block_count,
            retained_bytes,
        }))
    });

    match result {
        Ok(document) => FfiViewerCompileResult::ok(document),
        Err(error) => FfiViewerCompileResult::err(error.into()),
    }
}

fn is_image_atom(element: &RenderElement) -> bool {
    let (node_type, attrs) = match element {
        RenderElement::VoidInline {
            node_type, attrs, ..
        }
        | RenderElement::VoidBlock {
            node_type, attrs, ..
        }
        | RenderElement::OpaqueInlineAtom {
            node_type, attrs, ..
        }
        | RenderElement::OpaqueBlockAtom {
            node_type, attrs, ..
        } => (node_type.as_str(), attrs),
        RenderElement::Table { .. }
        | RenderElement::TextRun { .. }
        | RenderElement::BlockStart { .. }
        | RenderElement::BlockEnd => return false,
    };

    is_image_node_identity(node_type, attrs)
}

fn is_image_node(node: &crate::model::Node) -> bool {
    is_image_node_identity(node.node_type(), node.attrs())
}

fn is_image_node_identity(node_type: &str, attrs: &HashMap<String, serde_json::Value>) -> bool {
    node_type == "image"
        || matches!(node_type, "__opaque_json" | "__opaque")
            && (attrs
                .get("original_type")
                .and_then(serde_json::Value::as_str)
                == Some("image")
                || attrs.get("html_tag").and_then(serde_json::Value::as_str) == Some("img"))
}

fn viewer_element(
    element: RenderElement,
    mention_prefix: Option<&str>,
    images_enabled: bool,
    table_records: &mut std::collections::BTreeMap<u32, super::types::FfiViewerTable>,
) -> FfiViewerElement {
    match element {
        RenderElement::Table { table } => {
            let table_id = format!("t{}", table.table_pos);
            let record = super::types::FfiViewerTable {
                table_pos: table.table_pos,
                source_end: table.source_end,
                rows: table.rows,
                columns: table.columns,
                column_widths: table.column_widths.clone(),
                direction: table.direction.clone(),
                irregular: table.irregular,
                read_only_descendants: table.read_only_descendants,
                attrs_key: table.attrs_key.clone(),
                source_rows: table.source_rows.clone(),
                cells: table
                    .cells
                    .iter()
                    .map(|cell| super::types::FfiViewerTableCell {
                        source_pos: cell.source_pos,
                        source_end: cell.source_end,
                        row: cell.row,
                        column: cell.column,
                        rowspan: cell.rowspan,
                        colspan: cell.colspan,
                        header: cell.header,
                        attrs_key: cell.attrs_key.clone(),
                        content_key: cell.content_key.clone(),
                        elements: cell
                            .elements
                            .iter()
                            .filter(|element| images_enabled || !is_image_atom(element))
                            .cloned()
                            .map(|element| {
                                viewer_element(
                                    element,
                                    mention_prefix,
                                    images_enabled,
                                    table_records,
                                )
                            })
                            .collect(),
                    })
                    .collect(),
                synthetic_regions: table.synthetic_regions.clone(),
                failure: table.failure,
                compatibility_diagnostic: table.compatibility_diagnostic,
            };
            table_records.insert(record.table_pos, record);
            FfiViewerElement::Table { table_id }
        }
        RenderElement::TextRun { text, marks } => FfiViewerElement::TextRun {
            text,
            marks: marks.into_iter().map(viewer_mark).collect(),
        },
        RenderElement::VoidInline {
            node_type,
            doc_pos,
            attrs,
        } => FfiViewerElement::InlineAtom {
            label: prefixed_mention_label(
                &node_type,
                crate::render::inline_atom_label(&node_type, &attrs),
                mention_prefix,
            ),
            node_type,
            doc_pos,
            attrs_json: canonical_attrs_json(&attrs),
        },
        RenderElement::VoidBlock {
            node_type,
            doc_pos,
            attrs,
        } => FfiViewerElement::BlockAtom {
            label: prefixed_mention_label(
                &node_type,
                crate::render::inline_atom_label(&node_type, &attrs),
                mention_prefix,
            ),
            node_type,
            doc_pos,
            attrs_json: canonical_attrs_json(&attrs),
        },
        RenderElement::OpaqueInlineAtom {
            node_type,
            doc_pos,
            label,
            attrs,
            mention_theme: _,
        } => FfiViewerElement::InlineAtom {
            label: prefixed_mention_label(&node_type, label, mention_prefix),
            node_type,
            doc_pos,
            attrs_json: canonical_attrs_json(&attrs),
        },
        RenderElement::OpaqueBlockAtom {
            node_type,
            doc_pos,
            label,
            attrs,
        } => FfiViewerElement::BlockAtom {
            label: prefixed_mention_label(&node_type, label, mention_prefix),
            node_type,
            doc_pos,
            attrs_json: canonical_attrs_json(&attrs),
        },
        RenderElement::BlockStart {
            node_type,
            language,
            depth,
            list_context,
        } => FfiViewerElement::BlockStart {
            node_type,
            language,
            depth,
            list_context_json: list_context.as_ref().map(canonical_list_context_json),
        },
        RenderElement::BlockEnd => FfiViewerElement::BlockEnd,
    }
}

fn viewer_mark(mark: RenderMark) -> FfiViewerMark {
    FfiViewerMark {
        mark_type: mark.mark_type.clone(),
        attrs_json: canonical_attrs_json(&mark.attrs),
    }
}

fn prefixed_mention_label(node_type: &str, label: String, mention_prefix: Option<&str>) -> String {
    let Some(prefix) = mention_prefix.filter(|prefix| !prefix.is_empty()) else {
        return label;
    };
    if node_type == "mention" && !label.starts_with(prefix) {
        format!("{prefix}{label}")
    } else {
        label
    }
}

fn canonical_attrs_json(attrs: &HashMap<String, serde_json::Value>) -> String {
    let value = serde_json::Value::Object(
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
    String::from_utf8(serialize_json_value_stack_safe(&value, 0))
        .expect("canonical JSON serialization is UTF-8")
}

fn canonical_list_context_json(context: &ListContext) -> String {
    let value = serde_json::json!({
        "ordered": context.ordered,
        "index": context.index,
        "total": context.total,
        "start": context.start,
        "isFirst": context.is_first,
        "isLast": context.is_last,
        "kind": context.kind,
        "checked": context.checked,
    });
    String::from_utf8(serialize_json_value_stack_safe(&value, 0))
        .expect("canonical JSON serialization is UTF-8")
}

fn semantic_key(
    schema_fingerprint: &str,
    elements: &[FfiViewerElement],
    table_records: &[super::types::FfiViewerTable],
    table_attributes: &HashMap<String, String>,
    images_enabled: bool,
    mention_prefix: Option<&str>,
) -> String {
    let mut digest = Sha256::new();
    digest.update([VIEWER_SEMANTIC_KEY_VERSION]);
    hash_string(&mut digest, schema_fingerprint);
    digest.update([u8::from(images_enabled)]);
    hash_optional_string(&mut digest, mention_prefix);
    hash_u32(
        &mut digest,
        u32::try_from(elements.len()).unwrap_or(u32::MAX),
    );
    for element in elements {
        hash_element(&mut digest, element);
    }
    hash_u32(
        &mut digest,
        u32::try_from(table_records.len()).unwrap_or(u32::MAX),
    );
    for table in table_records {
        hash_table(&mut digest, table);
    }
    let mut attributes = table_attributes.iter().collect::<Vec<_>>();
    attributes.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    hash_u32(
        &mut digest,
        u32::try_from(attributes.len()).unwrap_or(u32::MAX),
    );
    for (key, value) in attributes {
        hash_string(&mut digest, key);
        hash_string(&mut digest, value);
    }
    format!("{:x}", digest.finalize())
}

fn hash_element(digest: &mut Sha256, element: &FfiViewerElement) {
    match element {
        FfiViewerElement::Table { table_id } => {
            digest.update([5]);
            hash_string(digest, table_id);
        }
        FfiViewerElement::TextRun { text, marks } => {
            digest.update([0]);
            hash_string(digest, text);
            hash_u32(digest, u32::try_from(marks.len()).unwrap_or(u32::MAX));
            for mark in marks {
                hash_string(digest, &mark.mark_type);
                hash_string(digest, &mark.attrs_json);
            }
        }
        FfiViewerElement::InlineAtom {
            node_type,
            doc_pos,
            attrs_json,
            label,
        } => {
            digest.update([1]);
            hash_string(digest, node_type);
            hash_u32(digest, *doc_pos);
            hash_string(digest, attrs_json);
            hash_string(digest, label);
        }
        FfiViewerElement::BlockAtom {
            node_type,
            doc_pos,
            attrs_json,
            label,
        } => {
            digest.update([2]);
            hash_string(digest, node_type);
            hash_u32(digest, *doc_pos);
            hash_string(digest, attrs_json);
            hash_string(digest, label);
        }
        FfiViewerElement::BlockStart {
            node_type,
            language,
            depth,
            list_context_json,
        } => {
            digest.update([3]);
            hash_string(digest, node_type);
            digest.update(depth.to_be_bytes());
            hash_optional_string(digest, list_context_json.as_deref());
            hash_optional_string(digest, language.as_deref());
        }
        FfiViewerElement::BlockEnd => digest.update([4]),
    }
}

fn hash_table(digest: &mut Sha256, table: &super::types::FfiViewerTable) {
    hash_u32(digest, table.table_pos);
    hash_u32(digest, table.source_end);
    hash_u32(digest, table.rows);
    hash_u32(digest, table.columns);
    hash_u32(
        digest,
        u32::try_from(table.column_widths.len()).unwrap_or(u32::MAX),
    );
    for width in &table.column_widths {
        match width {
            Some(width) => {
                digest.update([1]);
                hash_u32(digest, *width);
            }
            None => digest.update([0]),
        }
    }
    hash_optional_string(digest, table.direction.as_deref());
    digest.update([
        u8::from(table.irregular),
        u8::from(table.read_only_descendants),
    ]);
    hash_string(digest, &table.attrs_key);
    hash_u32(
        digest,
        u32::try_from(table.source_rows.len()).unwrap_or(u32::MAX),
    );
    for row in &table.source_rows {
        hash_u32(digest, row.source_pos);
        hash_u32(digest, row.source_end);
        hash_string(digest, &row.attrs_key);
    }
    hash_u32(digest, u32::try_from(table.cells.len()).unwrap_or(u32::MAX));
    for cell in &table.cells {
        hash_u32(digest, cell.source_pos);
        hash_u32(digest, cell.source_end);
        hash_u32(digest, cell.row);
        hash_u32(digest, cell.column);
        hash_u32(digest, cell.rowspan);
        hash_u32(digest, cell.colspan);
        digest.update([u8::from(cell.header)]);
        hash_string(digest, &cell.attrs_key);
        hash_string(digest, &cell.content_key);
        hash_u32(
            digest,
            u32::try_from(cell.elements.len()).unwrap_or(u32::MAX),
        );
        for element in &cell.elements {
            hash_element(digest, element);
        }
    }
    hash_u32(
        digest,
        u32::try_from(table.synthetic_regions.len()).unwrap_or(u32::MAX),
    );
    for region in &table.synthetic_regions {
        hash_u32(digest, region.row);
        hash_u32(digest, region.column);
        hash_u32(digest, region.rowspan);
        hash_u32(digest, region.colspan);
        digest.update([u8::from(region.header)]);
        hash_string(digest, &region.attrs_key);
    }
    hash_optional_table_failure(digest, table.failure);
    hash_optional_table_diagnostic(digest, table.compatibility_diagnostic);
}

fn hash_optional_table_failure(
    digest: &mut Sha256,
    value: Option<crate::tables::render::TableRenderFailure>,
) {
    let value = value.map(|value| match value {
        crate::tables::render::TableRenderFailure::GridLimit => 0,
        crate::tables::render::TableRenderFailure::WorkLimit => 1,
        crate::tables::render::TableRenderFailure::Allocation => 2,
        crate::tables::render::TableRenderFailure::InvalidStructure => 3,
        crate::tables::render::TableRenderFailure::InvalidAttributes => 4,
    });
    match value {
        Some(value) => digest.update([1, value]),
        None => digest.update([0]),
    }
}

fn hash_optional_table_diagnostic(
    digest: &mut Sha256,
    value: Option<crate::tables::render::TableCompatibilityDiagnostic>,
) {
    let value = value.map(|value| match value {
        crate::tables::render::TableCompatibilityDiagnostic::VirtualGridLimit => 0,
        crate::tables::render::TableCompatibilityDiagnostic::EmptyReferenceSurface => 1,
        crate::tables::render::TableCompatibilityDiagnostic::UnsupportedRowRole => 2,
        crate::tables::render::TableCompatibilityDiagnostic::UnsupportedCellRole => 3,
        crate::tables::render::TableCompatibilityDiagnostic::AmbiguousSourceMap => 4,
        crate::tables::render::TableCompatibilityDiagnostic::UnsupportedGapDefault => 5,
        crate::tables::render::TableCompatibilityDiagnostic::OverlappingReferenceCells => 6,
        crate::tables::render::TableCompatibilityDiagnostic::UnmappedReferenceCell => 7,
        crate::tables::render::TableCompatibilityDiagnostic::NonrectangularReferenceCell => 8,
        crate::tables::render::TableCompatibilityDiagnostic::ZeroSpanAfterReferencePass => 9,
    });
    match value {
        Some(value) => digest.update([1, value]),
        None => digest.update([0]),
    }
}

fn hash_optional_string(digest: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            digest.update([1]);
            hash_string(digest, value);
        }
        None => digest.update([0]),
    }
}

fn hash_string(digest: &mut Sha256, value: &str) {
    hash_u64(digest, u64::try_from(value.len()).unwrap_or(u64::MAX));
    digest.update(value.as_bytes());
}

fn hash_u32(digest: &mut Sha256, value: u32) {
    digest.update(value.to_be_bytes());
}

fn hash_u64(digest: &mut Sha256, value: u64) {
    digest.update(value.to_be_bytes());
}

fn retained_bytes(
    semantic_key: &str,
    elements: &[FfiViewerElement],
    table_records: &[super::types::FfiViewerTable],
    table_attributes: &HashMap<String, String>,
    preferred_text_block_name: &str,
) -> usize {
    let element_bytes = elements.iter().map(element_retained_bytes).sum::<usize>();
    std::mem::size_of::<ViewerCompiledDocument>()
        .saturating_add(semantic_key.len())
        .saturating_add(preferred_text_block_name.len())
        .saturating_add(
            elements
                .len()
                .saturating_mul(std::mem::size_of::<FfiViewerElement>()),
        )
        .saturating_add(element_bytes)
        .saturating_add(
            table_records
                .len()
                .saturating_mul(std::mem::size_of::<super::types::FfiViewerTable>()),
        )
        .saturating_add(
            table_records
                .iter()
                .map(table_retained_bytes)
                .sum::<usize>(),
        )
        .saturating_add(
            table_attributes
                .capacity()
                .saturating_mul(std::mem::size_of::<(String, String)>()),
        )
        .saturating_add(
            table_attributes
                .iter()
                .map(|(key, value)| key.capacity().saturating_add(value.capacity()))
                .sum::<usize>(),
        )
}

fn table_retained_bytes(table: &super::types::FfiViewerTable) -> usize {
    table
        .column_widths
        .capacity()
        .saturating_mul(std::mem::size_of::<Option<u32>>())
        .saturating_add(table.direction.as_ref().map_or(0, String::capacity))
        .saturating_add(table.attrs_key.capacity())
        .saturating_add(
            table
                .source_rows
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::tables::render::TableRenderRow>()),
        )
        .saturating_add(
            table
                .source_rows
                .iter()
                .map(|row| row.attrs_key.capacity())
                .sum::<usize>(),
        )
        .saturating_add(
            table
                .cells
                .capacity()
                .saturating_mul(std::mem::size_of::<super::types::FfiViewerTableCell>()),
        )
        .saturating_add(
            table
                .cells
                .iter()
                .map(|cell| {
                    cell.attrs_key
                        .capacity()
                        .saturating_add(cell.content_key.capacity())
                        .saturating_add(
                            cell.elements
                                .capacity()
                                .saturating_mul(std::mem::size_of::<FfiViewerElement>()),
                        )
                        .saturating_add(
                            cell.elements
                                .iter()
                                .map(element_retained_bytes)
                                .sum::<usize>(),
                        )
                })
                .sum::<usize>(),
        )
        .saturating_add(
            table
                .synthetic_regions
                .capacity()
                .saturating_mul(std::mem::size_of::<
                    crate::tables::render::TableRenderSyntheticRegion,
                >()),
        )
        .saturating_add(
            table
                .synthetic_regions
                .iter()
                .map(|region| region.attrs_key.capacity())
                .sum::<usize>(),
        )
}

fn element_retained_bytes(element: &FfiViewerElement) -> usize {
    match element {
        FfiViewerElement::Table { table_id } => table_id.capacity(),
        FfiViewerElement::TextRun { text, marks } => text
            .capacity()
            .saturating_add(
                marks
                    .capacity()
                    .saturating_mul(std::mem::size_of::<FfiViewerMark>()),
            )
            .saturating_add(
                marks
                    .iter()
                    .map(|mark| {
                        mark.mark_type
                            .capacity()
                            .saturating_add(mark.attrs_json.capacity())
                    })
                    .sum(),
            ),
        FfiViewerElement::InlineAtom {
            node_type,
            attrs_json,
            label,
            ..
        }
        | FfiViewerElement::BlockAtom {
            node_type,
            attrs_json,
            label,
            ..
        } => node_type
            .capacity()
            .saturating_add(attrs_json.capacity())
            .saturating_add(label.capacity()),
        FfiViewerElement::BlockStart {
            node_type,
            language,
            list_context_json,
            ..
        } => node_type
            .capacity()
            .saturating_add(list_context_json.as_ref().map_or(0, String::capacity))
            .saturating_add(language.as_ref().map_or(0, String::capacity)),
        FfiViewerElement::BlockEnd => 0,
    }
}
