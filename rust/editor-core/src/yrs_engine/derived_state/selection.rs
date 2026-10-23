use super::observability::{
    record_preview_position_map_derivation, record_preview_rendered_text_derivation,
};
#[cfg(test)]
use super::observability::{
    OPERATION_RESULT_RELATIVE_TRAVERSALS, RELATIVE_SELECTION_RESOLUTION_TRAVERSALS,
};
use super::DerivedStateCache;
use crate::model::{Document, Mark};
use crate::position::PositionMap;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::admission::TableProjectionIndex;
use crate::tables::selection::{cell_pair_is_usable, snap_cell_selection, CellSelectionOrigin};
use crate::yrs_engine::compiler::node_selection_position;
use crate::yrs_engine::position::{
    cursor_sticky_index_from_doc_pos, doc_pos_to_relative_point, doc_pos_to_sticky_index,
    relative_point_to_doc_pos,
};
use crate::yrs_engine::{
    scalar_offset_to_utf16, Affinity, OperationError, OperationResult, RelativePoint,
    RelativeSelection, ResolvedPoint, ResolvedSelection, TypedOperation,
};
use yrs::types::xml::XmlFragmentRef;
use yrs::{Assoc, ReadTxn};

/// A fully materialized selection state whose three representations were
/// proven against the same prewrite view. Keeping the fields private prevents
/// later compiler stages from mixing representations from different views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinalizedSelectionState {
    pub(super) relative: RelativeSelection,
    pub(super) resolved: ResolvedSelection,
    pub(super) legacy: Selection,
}

impl FinalizedSelectionState {
    pub(crate) fn new(
        relative: RelativeSelection,
        resolved: ResolvedSelection,
        legacy: Selection,
    ) -> Option<Self> {
        (resolved_to_legacy(&resolved) == legacy).then_some(Self {
            relative,
            resolved,
            legacy,
        })
    }

    pub(crate) fn relative(&self) -> &RelativeSelection {
        &self.relative
    }

    pub(super) fn into_parts(self) -> (RelativeSelection, ResolvedSelection, Selection) {
        (self.relative, self.resolved, self.legacy)
    }

    #[cfg(test)]
    pub(crate) fn tampered_for_test(&self) -> Vec<Self> {
        let mut relative = self.clone();
        relative.relative = RelativeSelection::All;
        let mut resolved = self.clone();
        resolved.resolved = ResolvedSelection::All;
        let mut legacy = self.clone();
        legacy.legacy = Selection::all();
        vec![relative, resolved, legacy]
    }
}

impl DerivedStateCache {
    pub fn update_selection_state(
        &mut self,
        relative_selection: RelativeSelection,
        resolved_selection: ResolvedSelection,
        stored_marks: Option<Vec<Mark>>,
        state_revision: u64,
    ) {
        self.legacy_selection = resolved_to_legacy(&resolved_selection);
        self.relative_selection = relative_selection;
        self.resolved_selection = resolved_selection;
        self.stored_marks = stored_marks;
        self.reseal_state_revision(state_revision);
        self.clear_active_state_certificate();
        self.render_active_state.take();
    }

    pub fn resolve_relative_selection<T: ReadTxn>(
        &self,
        relative_selection: &RelativeSelection,
        txn: &T,
        fragment: &XmlFragmentRef,
        schema: &Schema,
    ) -> Option<ResolvedSelection> {
        resolve_selection(
            txn,
            fragment,
            relative_selection,
            schema,
            &self.document,
            &self.position_map,
            &self.rendered_text,
            &self.table_projection_index,
            self.block_branch_index.as_deref(),
        )
    }

    pub fn legacy_selection(&self) -> Selection {
        self.legacy_selection.clone()
    }
}

pub(crate) fn stored_marks_after_selection_change(
    current: Option<&[Mark]>,
    before: &ResolvedSelection,
    after: &ResolvedSelection,
    _document: &Document,
    schema: &Schema,
) -> Option<Vec<Mark>> {
    let current = current?;
    if before != after || !is_collapsed_text(after) {
        return None;
    }
    Some(canonical_marks(current, schema))
}

pub(crate) fn apply_stored_mark_operation(
    marks: &mut Vec<Mark>,
    operation: &TypedOperation,
    schema: &Schema,
) -> OperationResult<bool> {
    match operation {
        TypedOperation::AddMark { mark, .. } => {
            if let Some(existing) = marks
                .iter()
                .find(|candidate| candidate.mark_type() == mark.mark_type())
            {
                if existing != mark {
                    return Err(OperationError::operation_invalid(
                        0,
                        0,
                        "mark",
                        "AddMark conflicts with an existing same-type mark; use ReplaceMark",
                    ));
                }
                return Ok(false);
            }
            insert_mark_ranked(marks, mark.clone(), schema);
            Ok(true)
        }
        TypedOperation::RemoveMark { mark_type, .. } => {
            let previous_len = marks.len();
            marks.retain(|candidate| candidate.mark_type() != mark_type);
            Ok(marks.len() != previous_len)
        }
        TypedOperation::ReplaceMark { mark, .. } => {
            if marks
                .iter()
                .find(|candidate| candidate.mark_type() == mark.mark_type())
                == Some(mark)
            {
                return Ok(false);
            }
            marks.retain(|candidate| candidate.mark_type() != mark.mark_type());
            insert_mark_ranked(marks, mark.clone(), schema);
            Ok(true)
        }
        _ => Err(OperationError::engine_invariant_failed(
            0,
            None,
            "stored mark transition received a non-mark operation",
        )),
    }
}

pub(crate) fn selection_table_index(
    document: &Document,
    selection: &Selection,
    schema: &Schema,
    resource_limits: &crate::boundary::ResourceLimits,
) -> TableProjectionIndex {
    match selection {
        Selection::Cell { .. } => {
            TableProjectionIndex::derive_or_fallback(document, schema, resource_limits)
        }
        Selection::Text { .. } | Selection::Node { .. } | Selection::All => {
            TableProjectionIndex::empty()
        }
    }
}

pub(crate) fn resolved_from_legacy(
    document: &Document,
    selection: &Selection,
    schema: &Schema,
    table_index: &TableProjectionIndex,
) -> Option<ResolvedSelection> {
    record_preview_position_map_derivation();
    let position_map = PositionMap::build(document, schema);
    record_preview_rendered_text_derivation();
    let rendered = crate::render::rendered_text(document, schema);
    resolved_from_legacy_with_view(
        document,
        selection,
        schema,
        &position_map,
        &rendered,
        table_index,
    )
}

pub(crate) fn resolved_from_legacy_with_view(
    document: &Document,
    selection: &Selection,
    schema: &Schema,
    position_map: &PositionMap,
    rendered: &str,
    table_index: &TableProjectionIndex,
) -> Option<ResolvedSelection> {
    let point = |document_position| {
        let scalar = crate::alignment::selection_scalar_range(
            document,
            schema,
            position_map,
            document_position,
        )
        .map_or_else(
            || position_map.doc_to_scalar(document_position, document),
            |(start, _)| start,
        );
        Some(ResolvedPoint {
            document: document_position,
            scalar,
            utf16: scalar_offset_to_utf16(rendered, scalar)?,
        })
    };
    match selection {
        Selection::Text { anchor, head } => Some(ResolvedSelection::Text {
            anchor: point(*anchor)?,
            head: point(*head)?,
        }),
        Selection::Node { pos } => Some(ResolvedSelection::Node {
            at: point(node_selection_position(document, schema, *pos)?)?,
        }),
        Selection::Cell { anchor, head } => {
            if !cell_pair_is_usable(table_index, *anchor, *head, CellSelectionOrigin::Preserved) {
                return None;
            }
            Some(ResolvedSelection::Cell {
                anchor: point(*anchor)?,
                head: point(*head)?,
            })
        }
        Selection::All => Some(ResolvedSelection::All),
    }
}

pub(super) fn is_collapsed_text(selection: &ResolvedSelection) -> bool {
    matches!(selection, ResolvedSelection::Text { anchor, head } if anchor.document == head.document)
}

pub(crate) fn canonical_marks(marks: &[Mark], schema: &Schema) -> Vec<Mark> {
    let mut marks = marks.to_vec();
    marks.sort_by(|left, right| {
        schema
            .mark_rank(left.mark_type())
            .unwrap_or(usize::MAX)
            .cmp(&schema.mark_rank(right.mark_type()).unwrap_or(usize::MAX))
            .then_with(|| left.mark_type().cmp(right.mark_type()))
    });
    marks
}

pub(super) fn insert_mark_ranked(marks: &mut Vec<Mark>, mark: Mark, schema: &Schema) {
    let rank = schema.mark_rank(mark.mark_type()).unwrap_or(usize::MAX);
    let index = marks
        .iter()
        .position(|candidate| {
            schema
                .mark_rank(candidate.mark_type())
                .unwrap_or(usize::MAX)
                > rank
        })
        .unwrap_or(marks.len());
    marks.insert(index, mark);
}

pub(crate) fn marks_at_position(document: &Document, schema: &Schema, position: u32) -> Vec<Mark> {
    crate::editor_state::marks_at_position(document, schema, position)
}

pub(super) fn preserve_with_mapped_fallback<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    current: &RelativeSelection,
    mapped: &Selection,
    schema: &Schema,
    strict_affinity: bool,
    table_index: &TableProjectionIndex,
) -> RelativeSelection {
    let point = |current: &RelativePoint, mapped_position| {
        if relative_point_to_doc_pos(txn, fragment, current, schema).is_some() {
            return current.clone();
        }
        if let Some(point) =
            doc_pos_to_relative_point(txn, fragment, mapped_position, current.affinity, schema)
        {
            return point;
        }
        assert!(
            !strict_affinity,
            "prevalidated explicit selection affinity must remain exactly representable"
        );
        let sticky = cursor_sticky_index_from_doc_pos(txn, fragment, mapped_position, true, schema)
            .expect("compiler-normalized mapped fallback has a Yrs association");
        RelativePoint {
            affinity: affinity_from_assoc(sticky.assoc),
            sticky,
        }
    };
    match (current, mapped) {
        (
            RelativeSelection::Text { anchor, head },
            Selection::Text {
                anchor: mapped_anchor,
                head: mapped_head,
            },
        ) => RelativeSelection::Text {
            anchor: point(anchor, *mapped_anchor),
            head: point(head, *mapped_head),
        },
        (RelativeSelection::Node { point: current }, Selection::Node { pos }) => {
            RelativeSelection::Node {
                point: point(current, *pos),
            }
        }
        (
            RelativeSelection::Cell { .. },
            Selection::Cell {
                anchor: mapped_anchor,
                head: mapped_head,
            },
        ) => {
            let snapped = snap_cell_selection(table_index, *mapped_anchor, *mapped_head)
                .unwrap_or_else(|| Selection::cursor(*mapped_anchor));
            operation_result_to_relative(txn, fragment, &snapped, schema, None)
        }
        (RelativeSelection::All, Selection::All) => RelativeSelection::All,
        _ => operation_result_to_relative(txn, fragment, mapped, schema, None),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_selection<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    relative_selection: &RelativeSelection,
    schema: &Schema,
    document: &Document,
    position_map: &PositionMap,
    rendered_text: &str,
    table_index: &TableProjectionIndex,
    block_index: Option<&crate::yrs_engine::block_branch_index::BlockBranchIndex>,
) -> Option<ResolvedSelection> {
    #[cfg(test)]
    RELATIVE_SELECTION_RESOLUTION_TRAVERSALS.set(
        RELATIVE_SELECTION_RESOLUTION_TRAVERSALS
            .get()
            .saturating_add(1),
    );
    let selection = if block_index.is_some() {
        crate::yrs_engine::position::relative_selection_with_resolver(
            relative_selection,
            document,
            position_map,
            |point| {
                block_index
                    .and_then(|index| {
                        index.doc_pos_of_offset(
                            txn,
                            &point.sticky.get_offset(txn)?,
                            position_map,
                            document,
                        )
                    })
                    .or_else(|| relative_point_to_doc_pos(txn, fragment, point, schema))
            },
        )
    } else {
        crate::yrs_engine::position::relative_selection_to_selection(
            txn,
            fragment,
            relative_selection,
            schema,
            document,
            position_map,
        )
    }?;
    let point = |document_position| {
        let scalar = crate::alignment::selection_scalar_range(
            document,
            schema,
            position_map,
            document_position,
        )
        .map_or_else(
            || position_map.doc_to_scalar(document_position, document),
            |(start, _)| start,
        );
        Some(ResolvedPoint {
            document: document_position,
            scalar,
            utf16: scalar_offset_to_utf16(rendered_text, scalar)?,
        })
    };
    match selection {
        Selection::Text { anchor, head } => Some(ResolvedSelection::Text {
            anchor: point(anchor)?,
            head: point(head)?,
        }),
        Selection::Node { pos } => Some(ResolvedSelection::Node {
            at: point(node_selection_position(document, schema, pos)?)?,
        }),
        Selection::Cell { anchor, head } => {
            if !cell_pair_is_usable(table_index, anchor, head, CellSelectionOrigin::Preserved) {
                return None;
            }
            Some(ResolvedSelection::Cell {
                anchor: point(anchor)?,
                head: point(head)?,
            })
        }
        Selection::All => Some(ResolvedSelection::All),
    }
}

pub(crate) fn operation_result_to_relative<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    selection: &Selection,
    schema: &Schema,
    indexed_view: Option<(
        &crate::yrs_engine::block_branch_index::BlockBranchIndex,
        &PositionMap,
        &Document,
    )>,
) -> RelativeSelection {
    #[cfg(test)]
    OPERATION_RESULT_RELATIVE_TRAVERSALS
        .set(OPERATION_RESULT_RELATIVE_TRAVERSALS.get().saturating_add(1));
    let sticky_at = |position, assoc| {
        indexed_view
            .and_then(|(index, map, document)| {
                index.sticky_at_doc_pos(txn, position, assoc, map, document)
            })
            .or_else(|| doc_pos_to_sticky_index(txn, fragment, position, assoc, schema))
    };
    let before = |position| {
        sticky_at(position, Assoc::Before).expect(
            "compiler-normalized operation-result position has an exact Before Yrs association",
        )
    };
    match selection {
        Selection::Text { anchor, head } if anchor == head => {
            let sticky = indexed_view
                .and_then(|(index, map, document)| {
                    index
                        .sticky_at_doc_pos(txn, *anchor, Assoc::After, map, document)
                        .or_else(|| {
                            index.sticky_at_doc_pos(txn, *anchor, Assoc::Before, map, document)
                        })
                })
                .or_else(|| cursor_sticky_index_from_doc_pos(txn, fragment, *anchor, true, schema))
                .expect("compiler-normalized cursor has a Yrs association");
            let point = RelativePoint {
                affinity: affinity_from_assoc(sticky.assoc),
                sticky,
            };
            RelativeSelection::Text {
                anchor: point.clone(),
                head: point,
            }
        }
        Selection::Text { anchor, head } => RelativeSelection::Text {
            anchor: RelativePoint {
                sticky: before(*anchor),
                affinity: Affinity::Before,
            },
            head: RelativePoint {
                sticky: before(*head),
                affinity: Affinity::Before,
            },
        },
        Selection::Node { pos } => RelativeSelection::Node {
            point: RelativePoint {
                sticky: before(*pos),
                affinity: Affinity::Before,
            },
        },
        Selection::Cell { anchor, head } => RelativeSelection::Cell {
            anchor: RelativePoint {
                sticky: before(*anchor),
                affinity: Affinity::Before,
            },
            head: RelativePoint {
                sticky: before(*head),
                affinity: Affinity::Before,
            },
        },
        Selection::All => RelativeSelection::All,
    }
}

pub(crate) fn command_initial_selection_to_relative<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    input: &crate::yrs_engine::SelectionInput,
    resolved: &ResolvedSelection,
    state: &DerivedStateCache,
    schema: &Schema,
) -> Option<RelativeSelection> {
    let requests = match (input, resolved) {
        (
            crate::yrs_engine::SelectionInput::Text { anchor, head },
            ResolvedSelection::Text { anchor: a, head: h },
        ) => vec![(a.document, anchor.affinity), (h.document, head.affinity)],
        (crate::yrs_engine::SelectionInput::Node { at }, ResolvedSelection::Node { at: p }) => {
            vec![(p.document, at.affinity())]
        }
        (
            crate::yrs_engine::SelectionInput::Cell { anchor, head },
            ResolvedSelection::Cell { anchor: a, head: h },
        ) => vec![
            (a.document, anchor.affinity()),
            (h.document, head.affinity()),
        ],
        (crate::yrs_engine::SelectionInput::All, ResolvedSelection::All) => {
            return Some(RelativeSelection::All);
        }
        _ => return None,
    };
    relative_selection_at_document_points(
        txn,
        fragment,
        resolved,
        &requests,
        schema,
        state
            .block_branch_index
            .as_deref()
            .map(|index| (index, &state.position_map, &state.document)),
    )
}

fn relative_selection_at_document_points<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    resolved: &ResolvedSelection,
    requests: &[(u32, Affinity)],
    schema: &Schema,
    indexed_view: Option<(
        &crate::yrs_engine::block_branch_index::BlockBranchIndex,
        &PositionMap,
        &Document,
    )>,
) -> Option<RelativeSelection> {
    let indexed_point = |position, affinity: Affinity| {
        let index = indexed_view?.0;
        let assoc = affinity.into();
        let opposite = match assoc {
            Assoc::Before => Assoc::After,
            Assoc::After => Assoc::Before,
        };
        let sticky = index
            .sticky_at_doc_pos(txn, position, assoc, indexed_view?.1, indexed_view?.2)
            .or_else(|| {
                index.sticky_at_doc_pos(txn, position, opposite, indexed_view?.1, indexed_view?.2)
            })?;
        (index.doc_pos_of_offset(
            txn,
            &sticky.get_offset(txn)?,
            indexed_view?.1,
            indexed_view?.2,
        ) == Some(position))
        .then_some(RelativePoint { sticky, affinity })
    };
    let mut points = requests
        .iter()
        .map(|&(position, affinity)| indexed_point(position, affinity))
        .collect::<Vec<_>>();
    if points.iter().any(Option::is_none) {
        let positions = requests.iter().map(|request| request.0).collect();
        let chunks = crate::yrs_engine::position::boundary_chunks_at_doc_positions(
            txn,
            fragment,
            &[positions],
            schema,
        )?;
        for (index, (_, affinity)) in requests.iter().enumerate() {
            if points[index].is_none() {
                let boundary = chunks.first()?.anchors.get(index)?;
                points[index] = Some(RelativePoint {
                    sticky: match affinity {
                        Affinity::Before => boundary.before.clone(),
                        Affinity::After => boundary.after.clone(),
                    },
                    affinity: *affinity,
                });
            }
        }
    }
    let mut points = points.into_iter().collect::<Option<Vec<_>>>()?.into_iter();
    match resolved {
        ResolvedSelection::Text { .. } => Some(RelativeSelection::Text {
            anchor: points.next()?,
            head: points.next()?,
        }),
        ResolvedSelection::Node { .. } => Some(RelativeSelection::Node {
            point: points.next()?,
        }),
        ResolvedSelection::Cell { .. } => Some(RelativeSelection::Cell {
            anchor: points.next()?,
            head: points.next()?,
        }),
        ResolvedSelection::All => Some(RelativeSelection::All),
    }
}

pub(crate) fn history_selection_to_relative<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    expected_relative: &RelativeSelection,
    expected_resolved: &ResolvedSelection,
    schema: &Schema,
) -> Option<RelativeSelection> {
    #[cfg(test)]
    OPERATION_RESULT_RELATIVE_TRAVERSALS
        .set(OPERATION_RESULT_RELATIVE_TRAVERSALS.get().saturating_add(1));
    let requests = match (expected_relative, expected_resolved) {
        (
            RelativeSelection::Text { anchor: a, head: h },
            ResolvedSelection::Text { anchor, head },
        ) => vec![(anchor.document, a.affinity), (head.document, h.affinity)],
        (RelativeSelection::Node { point }, ResolvedSelection::Node { at }) => {
            vec![(at.document, point.affinity)]
        }
        (
            RelativeSelection::Cell { anchor: a, head: h },
            ResolvedSelection::Cell { anchor, head },
        ) => vec![(anchor.document, a.affinity), (head.document, h.affinity)],
        (RelativeSelection::All, ResolvedSelection::All) => return Some(RelativeSelection::All),
        _ => return None,
    };
    relative_selection_at_document_points(txn, fragment, expected_resolved, &requests, schema, None)
}

pub(crate) fn exact_point_is_representable<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    position: u32,
    point: &RelativePoint,
    schema: &Schema,
) -> bool {
    doc_pos_to_relative_point(txn, fragment, position, point.affinity, schema).is_some()
}

pub(crate) fn resolved_to_legacy(selection: &ResolvedSelection) -> Selection {
    match selection {
        ResolvedSelection::Text { anchor, head } => Selection::text(anchor.document, head.document),
        ResolvedSelection::Node { at } => Selection::node(at.document),
        ResolvedSelection::Cell { anchor, head } => Selection::cell(anchor.document, head.document),
        ResolvedSelection::All => Selection::all(),
    }
}

pub(super) fn affinity_from_assoc(assoc: Assoc) -> Affinity {
    match assoc {
        Assoc::Before => Affinity::Before,
        Assoc::After => Affinity::After,
    }
}
