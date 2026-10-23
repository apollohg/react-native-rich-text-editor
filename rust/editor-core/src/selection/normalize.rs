use crate::model::Document;
use crate::position::PositionMap;

use super::Selection;

/// Normalize a selection against the document and its `PositionMap`.
///
/// - `Text`: both anchor and head are snapped independently.
/// - `Node`: clamp to document bounds, preserving openings for schema admission.
/// - `All`: returned as-is (it resolves lazily from the document).
pub fn normalize_selection(
    selection: Selection,
    doc: &Document,
    pos_map: &PositionMap,
) -> Selection {
    match selection {
        Selection::Text { anchor, head } => {
            let norm_anchor = pos_map.normalize_cursor_pos(anchor, doc);
            let norm_head = pos_map.normalize_cursor_pos(head, doc);
            Selection::Text {
                anchor: norm_anchor,
                head: norm_head,
            }
        }
        Selection::Node { pos } => Selection::Node {
            pos: pos.min(doc.content_size()),
        },
        Selection::Cell { anchor, head } => Selection::Cell { anchor, head },
        Selection::All => Selection::All,
    }
}
