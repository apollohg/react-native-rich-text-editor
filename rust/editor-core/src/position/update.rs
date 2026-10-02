use crate::model::Document;
use crate::schema::Schema;
use crate::transform::StepMap;

use super::build::{build_position_map, rebuild_existing_block_mapping};
use super::{BlockMapping, PositionMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    Rebuild,
    MarksOnly,
    InlineTextOnly,
}

struct IncrementalUpdate {
    block_index: usize,
    block: BlockMapping,
    doc_delta: i32,
    scalar_delta: i32,
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
                if let Some(update) =
                    self.prepare_incremental_update(range, old_doc, new_doc, schema)
                {
                    self.blocks[update.block_index] = update.block;
                    if update.block_index + 1 < self.blocks.len() {
                        self.prefix_deltas.insert(
                            update.block_index + 1,
                            update.doc_delta,
                            update.scalar_delta,
                        );
                    }
                    return;
                }
            }
        }

        *self = build_position_map(new_doc, schema);
    }

    pub(crate) fn clone_updated_and_compacted(
        &self,
        step_map: &StepMap,
        old_doc: &Document,
        new_doc: &Document,
        mode: UpdateMode,
        schema: &Schema,
    ) -> Self {
        if self.prefix_deltas.is_empty() && mode == UpdateMode::InlineTextOnly {
            if let Some(update) = step_map
                .single_range()
                .and_then(|range| self.prepare_incremental_update(range, old_doc, new_doc, schema))
            {
                let mut blocks = Vec::with_capacity(self.blocks.len());
                blocks.extend(self.blocks[..update.block_index].iter().cloned());
                blocks.push(update.block);
                blocks.extend(self.blocks[update.block_index + 1..].iter().map(|block| {
                    let mut block = block.clone();
                    block.doc_start = (block.doc_start as i64 + update.doc_delta as i64) as u32;
                    block.doc_end = (block.doc_end as i64 + update.doc_delta as i64) as u32;
                    block.scalar_start =
                        (block.scalar_start as i64 + update.scalar_delta as i64) as u32;
                    block
                }));
                // Preserve the allocation retained by the ordinary update and compaction.
                let mut prefix_deltas = self.prefix_deltas.clone();
                if update.block_index + 1 < self.blocks.len() {
                    prefix_deltas.insert(
                        update.block_index + 1,
                        update.doc_delta,
                        update.scalar_delta,
                    );
                }
                prefix_deltas.clear();
                return Self {
                    blocks,
                    prefix_deltas,
                    hard_break_node_types: self.hard_break_node_types.clone(),
                };
            }
        }
        let mut result = self.clone();
        result.update(step_map, old_doc, new_doc, mode, schema);
        result.compact();
        result
    }

    fn prepare_incremental_update(
        &self,
        (pos, deleted, inserted): (u32, u32, u32),
        old_doc: &Document,
        new_doc: &Document,
        schema: &Schema,
    ) -> Option<IncrementalUpdate> {
        let block_idx = match self.find_block_for_doc_pos(pos) {
            Some(idx) => idx,
            None => return None,
        };

        let old_doc_end = self.effective_doc_end(block_idx);
        let old_scalar_len = self.blocks[block_idx].scalar_len;

        let edit_end = pos + deleted;
        if edit_end > old_doc_end {
            return None;
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
                return None;
            }
        }
        if block_idx + 1 < self.blocks.len() {
            let next = &self.blocks[block_idx + 1];
            let old_next = old_doc.node_at(&next.node_path);
            let new_next = new_doc.node_at(&next.node_path);
            if old_next != new_next {
                return None;
            }
        }

        let new_node = match new_doc.node_at(&old_block.node_path) {
            Some(node) => node,
            None => return None,
        };
        let rebuilt_block = match rebuild_existing_block_mapping(new_node, &old_block, schema) {
            Some(block) => block,
            None => return None,
        };
        let rebuilt_doc_delta = rebuilt_block.doc_end as i32 - old_block.doc_end as i32;
        if rebuilt_doc_delta != doc_delta {
            return None;
        }

        let scalar_delta = rebuilt_block.scalar_len as i32 - old_scalar_len as i32;

        Some(IncrementalUpdate {
            block_index: block_idx,
            block: rebuilt_block,
            doc_delta,
            scalar_delta,
        })
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

#[cfg(test)]
mod tests;
