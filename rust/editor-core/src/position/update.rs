use crate::model::Document;
use crate::schema::Schema;
use crate::transform::StepMap;

use super::build::{build_position_map, rebuild_existing_block_mapping};
use super::PositionMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    Rebuild,
    MarksOnly,
    InlineTextOnly,
}

impl PositionMap {
    /// Incrementally update the position map after a transaction.
    ///
    /// For the simple implementation: if the edit is a single-range change
    /// that falls entirely within one block, we can update just that block
    /// and shift trailing blocks via the DeltaTree. Otherwise we fall back
    /// to a full rebuild.
    ///
    /// `step_map` is the composed mapping from the transaction.
    /// `new_doc` is the document *after* the transaction has been applied.
    /// `schema` is only consulted on the full-rebuild fallback path (list
    /// detection needs it); the incremental path reuses the existing block's
    /// `scalar_prefix_len` unchanged.
    pub fn update(
        &mut self,
        step_map: &StepMap,
        old_doc: &Document,
        new_doc: &Document,
        mode: UpdateMode,
        schema: &Schema,
    ) {
        if mode == UpdateMode::MarksOnly {
            return;
        }

        if mode == UpdateMode::InlineTextOnly {
            if let Some(range) = step_map.single_range() {
                if self.try_incremental_update(range, old_doc, new_doc, schema) {
                    return;
                }
            }
        }

        *self = build_position_map(new_doc, schema);
    }

    /// Attempt an incremental update for a single (pos, deleted, inserted) change.
    ///
    /// Returns `true` if the incremental update succeeded, `false` if we need
    /// a full rebuild.
    fn try_incremental_update(
        &mut self,
        (pos, deleted, inserted): (u32, u32, u32),
        old_doc: &Document,
        new_doc: &Document,
        schema: &Schema,
    ) -> bool {
        let block_idx = match self.find_block_for_doc_pos(pos) {
            Some(idx) => idx,
            None => return false,
        };

        let old_doc_end = self.effective_doc_end(block_idx);
        let old_scalar_len = self.blocks[block_idx].scalar_len;

        let edit_end = pos + deleted;
        if edit_end > old_doc_end {
            return false;
        }

        let doc_delta = inserted as i32 - deleted as i32;
        let old_block = self.blocks[block_idx].clone();

        // Structural edits like split/join shift adjacent block paths. Require
        // the neighboring blocks to remain identical at the same paths before
        // we trust an inline-only update.
        if block_idx > 0 {
            let previous = &self.blocks[block_idx - 1];
            let old_previous = old_doc.node_at(&previous.node_path);
            let new_previous = new_doc.node_at(&previous.node_path);
            if old_previous != new_previous {
                return false;
            }
        }
        if block_idx + 1 < self.blocks.len() {
            let next = &self.blocks[block_idx + 1];
            let old_next = old_doc.node_at(&next.node_path);
            let new_next = new_doc.node_at(&next.node_path);
            if old_next != new_next {
                return false;
            }
        }

        let new_node = match new_doc.node_at(&old_block.node_path) {
            Some(node) => node,
            None => return false,
        };
        let rebuilt_block = match rebuild_existing_block_mapping(new_node, &old_block, schema) {
            Some(block) => block,
            None => return false,
        };
        let rebuilt_doc_delta = rebuilt_block.doc_end as i32 - old_block.doc_end as i32;
        if rebuilt_doc_delta != doc_delta {
            return false;
        }

        let scalar_delta = rebuilt_block.scalar_len as i32 - old_scalar_len as i32;

        self.blocks[block_idx] = rebuilt_block;

        if block_idx + 1 < self.blocks.len() {
            self.prefix_deltas
                .insert(block_idx + 1, doc_delta, scalar_delta);
        }

        true
    }

    /// Fold all pending deltas from the `DeltaTree` into the `BlockMapping`
    /// values, then clear the tree.
    ///
    /// Call this periodically (e.g. every N transactions) to keep lookups fast.
    pub fn compact(&mut self) {
        if self.prefix_deltas.is_empty() {
            return;
        }

        for (range, dd, sd) in self.prefix_deltas.ranges(self.blocks.len()) {
            if dd != 0 || sd != 0 {
                for block in &mut self.blocks[range] {
                    block.doc_start = (block.doc_start as i64 + dd as i64) as u32;
                    block.doc_end = (block.doc_end as i64 + dd as i64) as u32;
                    block.scalar_start = (block.scalar_start as i64 + sd as i64) as u32;
                }
            }
        }

        self.prefix_deltas.clear();
    }
}

/// Extension trait so we can ask StepMap for a single-range change.
pub(crate) trait StepMapExt {
    fn single_range(&self) -> Option<(u32, u32, u32)>;
}

impl StepMapExt for StepMap {
    fn single_range(&self) -> Option<(u32, u32, u32)> {
        let ranges = self.ranges();
        if ranges.len() == 1 {
            Some(ranges[0])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod compaction_tests {
    use super::*;
    use crate::test_support::large_table_fixture::{plain_table_document, session_with_document};

    #[test]
    fn compact_preserves_effective_offsets_and_storage_across_delta_ranges() {
        const BLOCKS: usize = 8;
        let session = session_with_document(&plain_table_document(1, BLOCKS));
        let original = session.engine.position_map().unwrap();
        assert_eq!(original.block_count(), BLOCKS);
        let cases: &[&[(usize, i32, i32)]] = &[
            &[],
            &[(0, 3, 2)],
            &[(1, 3, 2), (3, -3, -2), (4, 1, 0), (5, -1, 0)],
            &[(2, 5, 7), (2, -2, -3), (6, -1, -2)],
            &[(BLOCKS - 1, -2, -1)],
            &[(0, i32::MAX, i32::MAX), (BLOCKS, 1, 1), (usize::MAX, 1, 1)],
        ];
        for block_count in [0, 1, BLOCKS] {
            for deltas in cases {
                let mut map = original.clone();
                map.blocks.truncate(block_count);
                for &(index, doc, scalar) in *deltas {
                    map.prefix_deltas.insert(index, doc, scalar);
                }
                let mut expected = map.blocks.clone();
                for (index, block) in expected.iter_mut().enumerate() {
                    let (doc, scalar) = map.prefix_deltas.accumulated_delta(index);
                    block.doc_start = (block.doc_start as i64 + doc as i64) as u32;
                    block.doc_end = (block.doc_end as i64 + doc as i64) as u32;
                    block.scalar_start = (block.scalar_start as i64 + scalar as i64) as u32;
                }
                let block_storage = map.blocks.as_ptr();
                let block_capacity = map.blocks.capacity();
                let delta_charge = map.prefix_deltas.history_snapshot_clone_retained_bytes();
                map.compact();
                assert_eq!(
                    format!("{:?}", map.blocks),
                    format!("{expected:?}"),
                    "blocks={block_count}, deltas={deltas:?}"
                );
                assert_eq!(map.blocks.as_ptr(), block_storage);
                assert_eq!(map.blocks.capacity(), block_capacity);
                assert!(map.prefix_deltas.is_empty());
                assert_eq!(
                    map.prefix_deltas.history_snapshot_clone_retained_bytes(),
                    delta_charge
                );
            }
        }
    }
}
