use std::borrow::Cow;
use std::fmt;
use std::mem::{align_of, size_of};
use std::ops::Range;
use std::sync::Arc;

use smallvec::SmallVec;

use super::{BlockMapping, BLOCK_PATH_INLINE_CAPACITY};
use crate::model::arc_allocation_retained_bytes;

const BLOCKS_PER_PAGE: usize = 128;

type Directory = Vec<Page>;

pub(super) enum Blocks {
    Dense(Vec<BlockMapping>),
    Paged {
        directory: Arc<Directory>,
        capacity: usize,
    },
}

// The existing history charge includes the PositionMap header itself.
const _: () = assert!(size_of::<Blocks>() == size_of::<Vec<BlockMapping>>());
const _: () = assert!(align_of::<Blocks>() == align_of::<Vec<BlockMapping>>());

#[derive(Clone)]
pub(super) struct Page {
    blocks: Arc<[PackedBlock]>,
    doc_offset: u32,
    scalar_offset: u32,
}

#[derive(Clone, Copy)]
struct PackedBlock {
    doc_start: u32,
    doc_end: u32,
    scalar_start: u32,
    scalar_len: u32,
    scalar_prefix_len: u32,
    rendered_break_after: u32,
    node_path: [u32; BLOCK_PATH_INLINE_CAPACITY],
    path_len: u8,
    is_void_block: bool,
}

impl PackedBlock {
    fn pack(block: &BlockMapping) -> Self {
        debug_assert!(!block.node_path.spilled());
        let mut node_path = [0; BLOCK_PATH_INLINE_CAPACITY];
        node_path[..block.node_path.len()].copy_from_slice(&block.node_path);
        Self {
            doc_start: block.doc_start,
            doc_end: block.doc_end,
            scalar_start: block.scalar_start,
            scalar_len: block.scalar_len,
            scalar_prefix_len: block.scalar_prefix_len,
            rendered_break_after: block.rendered_break_after,
            node_path,
            path_len: block.node_path.len() as u8,
            is_void_block: block.is_void_block,
        }
    }

    fn unpack(&self, doc_offset: u32, scalar_offset: u32) -> BlockMapping {
        BlockMapping {
            doc_start: self.doc_start.wrapping_add(doc_offset),
            doc_end: self.doc_end.wrapping_add(doc_offset),
            scalar_start: self.scalar_start.wrapping_add(scalar_offset),
            scalar_len: self.scalar_len,
            scalar_prefix_len: self.scalar_prefix_len,
            rendered_break_after: self.rendered_break_after,
            node_path: SmallVec::from_slice(self.path()),
            is_void_block: self.is_void_block,
        }
    }

    fn path(&self) -> &[u32] {
        &self.node_path[..usize::from(self.path_len)]
    }

    fn shift(&mut self, doc: u32, scalar: u32) {
        self.doc_start = self.doc_start.wrapping_add(doc);
        self.doc_end = self.doc_end.wrapping_add(doc);
        self.scalar_start = self.scalar_start.wrapping_add(scalar);
    }
}

impl Clone for Blocks {
    fn clone(&self) -> Self {
        match self {
            Self::Dense(blocks) => Self::Dense(blocks.clone()),
            Self::Paged { directory, .. } => Self::Paged {
                directory: Arc::clone(directory),
                capacity: self.len(),
            },
        }
    }
}

impl fmt::Debug for Blocks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl Blocks {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Dense(blocks) => blocks.len(),
            Self::Paged { directory, .. } => {
                (directory.len() - 1) * BLOCKS_PER_PAGE + directory.last().unwrap().blocks.len()
            }
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(super) fn capacity(&self) -> usize {
        match self {
            Self::Dense(blocks) => blocks.capacity(),
            Self::Paged { capacity, .. } => *capacity,
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<Cow<'_, BlockMapping>> {
        match self {
            Self::Dense(blocks) => blocks.get(index).map(Cow::Borrowed),
            Self::Paged { directory, .. } => {
                let page = directory.get(index / BLOCKS_PER_PAGE)?;
                let block = page.blocks.get(index % BLOCKS_PER_PAGE)?;
                Some(Cow::Owned(
                    block.unpack(page.doc_offset, page.scalar_offset),
                ))
            }
        }
    }

    pub(super) fn offsets(&self, index: usize) -> Option<(u32, u32, u32)> {
        match self {
            Self::Dense(blocks) => blocks
                .get(index)
                .map(|block| (block.doc_start, block.doc_end, block.scalar_start)),
            Self::Paged { directory, .. } => {
                let page = directory.get(index / BLOCKS_PER_PAGE)?;
                let block = page.blocks.get(index % BLOCKS_PER_PAGE)?;
                Some((
                    block.doc_start.wrapping_add(page.doc_offset),
                    block.doc_end.wrapping_add(page.doc_offset),
                    block.scalar_start.wrapping_add(page.scalar_offset),
                ))
            }
        }
    }

    pub(super) fn path(&self, index: usize) -> Option<&[u32]> {
        match self {
            Self::Dense(blocks) => blocks.get(index).map(|block| block.node_path.as_slice()),
            Self::Paged { directory, .. } => directory
                .get(index / BLOCKS_PER_PAGE)?
                .blocks
                .get(index % BLOCKS_PER_PAGE)
                .map(PackedBlock::path),
        }
    }

    pub(super) fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = Cow<'_, BlockMapping>> + DoubleEndedIterator {
        (0..self.len()).map(|index| self.get(index).unwrap())
    }

    pub(super) fn partition_point(
        &self,
        mut predicate: impl FnMut(&BlockMapping) -> bool,
    ) -> usize {
        let mut lo = 0;
        let mut hi = self.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if predicate(&self.get(mid).unwrap()) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    fn packed_bytes_with_scratch(length: usize) -> Option<usize> {
        let pages = length.div_ceil(BLOCKS_PER_PAGE);
        let page_bytes = length
            .checked_mul(size_of::<PackedBlock>())?
            .checked_add(pages.checked_mul(arc_allocation_retained_bytes(0)?)?)?;
        let directory_bytes = pages
            .checked_mul(size_of::<Page>())?
            .checked_add(arc_allocation_retained_bytes(size_of::<Directory>())?)?;
        let replacement_page =
            arc_allocation_retained_bytes(BLOCKS_PER_PAGE.checked_mul(size_of::<PackedBlock>())?)?;
        page_bytes
            .checked_add(directory_bytes.checked_mul(2)?)?
            .checked_add(replacement_page)
    }

    fn can_pack(blocks: &[BlockMapping]) -> bool {
        let fits = Self::packed_bytes_with_scratch(blocks.len())
            .zip(blocks.len().checked_mul(size_of::<BlockMapping>()))
            .is_some_and(|(physical, charged)| physical <= charged);
        fits && blocks.iter().all(|block| !block.node_path.spilled())
    }

    pub(super) fn clone_updated(
        &self,
        index: usize,
        block: BlockMapping,
        doc: i32,
        scalar: i32,
    ) -> Self {
        if let Self::Dense(blocks) = self {
            if !block.node_path.spilled() && Self::can_pack(blocks) {
                let directory = blocks
                    .chunks(BLOCKS_PER_PAGE)
                    .map(|chunk| Page {
                        blocks: chunk
                            .iter()
                            .map(PackedBlock::pack)
                            .collect::<Vec<_>>()
                            .into(),
                        doc_offset: 0,
                        scalar_offset: 0,
                    })
                    .collect::<Vec<_>>();
                let mut result = Self::Paged {
                    directory: Arc::new(directory),
                    capacity: blocks.len(),
                };
                result.set(index, block);
                result.shift(index + 1..blocks.len(), doc, scalar);
                return result;
            }
            let mut updated = Vec::with_capacity(blocks.len());
            updated.extend(blocks[..index].iter().cloned());
            updated.push(block);
            updated.extend(blocks[index + 1..].iter().map(|block| {
                let mut block = block.clone();
                block.doc_start = block.doc_start.wrapping_add(doc as u32);
                block.doc_end = block.doc_end.wrapping_add(doc as u32);
                block.scalar_start = block.scalar_start.wrapping_add(scalar as u32);
                block
            }));
            return Self::Dense(updated);
        }
        let mut result = self.clone();
        result.set(index, block);
        result.shift(index + 1..self.len(), doc, scalar);
        result
    }

    pub(super) fn set(&mut self, index: usize, block: BlockMapping) {
        match self {
            Self::Dense(blocks) => blocks[index] = block,
            Self::Paged { directory, .. } => {
                let page = &mut Arc::make_mut(directory)[index / BLOCKS_PER_PAGE];
                let mut packed = PackedBlock::pack(&block);
                packed.shift(
                    page.doc_offset.wrapping_neg(),
                    page.scalar_offset.wrapping_neg(),
                );
                Arc::make_mut(&mut page.blocks)[index % BLOCKS_PER_PAGE] = packed;
            }
        }
    }

    pub(super) fn shift(&mut self, range: Range<usize>, doc: i32, scalar: i32) {
        if range.is_empty() || (doc == 0 && scalar == 0) {
            return;
        }
        match self {
            Self::Dense(blocks) => {
                for block in &mut blocks[range] {
                    block.doc_start = block.doc_start.wrapping_add(doc as u32);
                    block.doc_end = block.doc_end.wrapping_add(doc as u32);
                    block.scalar_start = block.scalar_start.wrapping_add(scalar as u32);
                }
            }
            Self::Paged { directory, .. } => {
                let first_page = range.start / BLOCKS_PER_PAGE;
                let last_page = (range.end - 1) / BLOCKS_PER_PAGE;
                for (index, page) in Arc::make_mut(directory)
                    .iter_mut()
                    .enumerate()
                    .take(last_page + 1)
                    .skip(first_page)
                {
                    let start = range.start.saturating_sub(index * BLOCKS_PER_PAGE);
                    let end = (range.end - index * BLOCKS_PER_PAGE).min(page.blocks.len());
                    if start == 0 && end == page.blocks.len() {
                        page.doc_offset = page.doc_offset.wrapping_add(doc as u32);
                        page.scalar_offset = page.scalar_offset.wrapping_add(scalar as u32);
                    } else {
                        for block in &mut Arc::make_mut(&mut page.blocks)[start..end] {
                            block.shift(doc as u32, scalar as u32);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn spilled_path_bytes(&self) -> Option<usize> {
        match self {
            Self::Paged { .. } => Some(0),
            Self::Dense(blocks) => blocks.iter().try_fold(0usize, |total, block| {
                let bytes = if block.node_path.spilled() {
                    block.node_path.capacity().checked_mul(size_of::<u32>())?
                } else {
                    0
                };
                total.checked_add(bytes)
            }),
        }
    }

    #[cfg(test)]
    pub(super) fn unshared_block_count(&self, previous: &Self) -> usize {
        match (self, previous) {
            (Self::Paged { directory, .. }, Self::Paged { directory: old, .. }) => directory
                .iter()
                .zip(old.iter())
                .filter(|(page, old)| !Arc::ptr_eq(&page.blocks, &old.blocks))
                .map(|(page, _)| page.blocks.len())
                .sum(),
            _ => self.len(),
        }
    }

    #[cfg(test)]
    pub(super) fn dense_mut(&mut self) -> &mut Vec<BlockMapping> {
        match self {
            Self::Dense(blocks) => blocks,
            Self::Paged { .. } => panic!("test requires original dense storage"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(index: usize) -> BlockMapping {
        BlockMapping {
            doc_start: index as u32,
            doc_end: (index as u32).wrapping_add(1),
            scalar_start: (index as u32).wrapping_mul(2),
            scalar_len: 1,
            scalar_prefix_len: 0,
            rendered_break_after: 1,
            node_path: SmallVec::from_slice(&[u32::MAX, index as u32]),
            is_void_block: false,
        }
    }

    #[test]
    fn paged_storage_preserves_modular_offsets_and_releases_final_owners() {
        const LENGTH: usize = 4097;
        let dense = Blocks::Dense((0..LENGTH).map(block).collect());
        let mut packed = dense.clone_updated(0, block(0), 0, 0);
        let Blocks::Paged { directory, .. } = &packed else {
            panic!("large map must be paged")
        };
        let directory_weak = Arc::downgrade(directory);
        let page_weak = Arc::downgrade(&directory.last().unwrap().blocks);
        let original = packed.clone();
        let mut expected = dense;
        let cases = [
            (0..LENGTH, i32::MAX, i32::MIN),
            (127..129, i32::MAX, i32::MIN),
            (128..1024, 2, -2),
            (0..LENGTH, -1, 1),
            (255..LENGTH, i32::MIN, i32::MAX),
        ];
        for _ in 0..4 {
            for (range, doc, scalar) in &cases {
                packed.shift(range.clone(), *doc, *scalar);
                expected.shift(range.clone(), *doc, *scalar);
                assert_eq!(
                    format!("{packed:?}"),
                    format!("{expected:?}"),
                    "range={range:?}"
                );
                for index in [0, 127, 128, 255, 256, LENGTH - 1] {
                    assert_eq!(packed.path(index), expected.path(index));
                }
            }
        }
        assert_eq!(
            format!("{original:?}"),
            format!("{:?}", Blocks::Dense((0..LENGTH).map(block).collect()))
        );
        drop(original);
        assert!(directory_weak.upgrade().is_none());
        // The unchanged last page is still shared by the updated map.
        assert!(page_weak.upgrade().is_some());
        drop(packed);
        assert!(page_weak.upgrade().is_none());
    }

    #[test]
    fn first_funded_page_layout_preserves_the_length_based_boundary() {
        const SEARCH_LIMIT: usize = BLOCKS_PER_PAGE * 32;
        let first_funded = (1..=SEARCH_LIMIT)
            .find(|length| {
                Blocks::packed_bytes_with_scratch(*length).unwrap()
                    <= length * size_of::<BlockMapping>()
            })
            .expect("supported block layouts must fund paging");
        for length in [first_funded - 1, first_funded, first_funded + 1] {
            let mut blocks = (0..length).map(block).collect::<Vec<_>>();
            blocks.reserve(SEARCH_LIMIT);
            let source = Blocks::Dense(blocks);
            let clone = source.clone_updated(0, block(0), 1, -1);
            assert_eq!(
                matches!(clone, Blocks::Paged { .. }),
                length >= first_funded,
                "pointer bytes={}, first funded={first_funded}, length={length}",
                size_of::<usize>()
            );
            assert_eq!(clone.capacity(), length);
            for (index, copied) in clone.iter().enumerate() {
                let original = source.get(index).unwrap();
                let delta = u32::from(index > 0);
                assert_eq!(copied.doc_start, original.doc_start.wrapping_add(delta));
                assert_eq!(
                    copied.scalar_start,
                    original.scalar_start.wrapping_sub(delta)
                );
                assert_eq!(copied.node_path.capacity(), original.node_path.capacity());
            }
        }
        eprintln!(
            "pointer_bytes={} block_bytes={} packed_bytes={} first_funded={first_funded}",
            size_of::<usize>(),
            size_of::<BlockMapping>(),
            size_of::<PackedBlock>()
        );
    }

    #[test]
    fn packing_is_funded_by_length_and_excludes_every_spilled_path() {
        for length in [0, 1, 127, 128, 129, 511, 512, 513, 1025, 20_000] {
            let mut blocks = (0..length).map(block).collect::<Vec<_>>();
            blocks.reserve(20_000);
            let eligible = Blocks::can_pack(&blocks);
            if eligible {
                let source = Blocks::Dense(blocks.clone());
                let packed = source.clone_updated(0, block(0), 0, 0);
                let Blocks::Paged { directory, .. } = &packed else {
                    panic!("eligible map not packed")
                };
                let physical = arc_allocation_retained_bytes(size_of::<Directory>()).unwrap()
                    + directory.capacity() * size_of::<Page>()
                    + directory
                        .iter()
                        .map(|page| {
                            arc_allocation_retained_bytes(std::mem::size_of_val(
                                page.blocks.as_ref(),
                            ))
                            .unwrap()
                        })
                        .sum::<usize>();
                let scratch = arc_allocation_retained_bytes(size_of::<Directory>()).unwrap()
                    + directory.len() * size_of::<Page>()
                    + arc_allocation_retained_bytes(BLOCKS_PER_PAGE * size_of::<PackedBlock>())
                        .unwrap();
                assert_eq!(
                    physical + scratch,
                    Blocks::packed_bytes_with_scratch(length).unwrap()
                );
                assert!(
                    physical + scratch <= length * size_of::<BlockMapping>(),
                    "length={length}"
                );
                assert_eq!(packed.capacity(), length);
                assert_eq!(packed.spilled_path_bytes(), Some(0));
                blocks[length / 2]
                    .node_path
                    .reserve(BLOCK_PATH_INLINE_CAPACITY * 2);
                assert!(blocks[length / 2].node_path.spilled());
                assert!(
                    !Blocks::can_pack(&blocks),
                    "short spilled paths must retain clone semantics"
                );
                blocks[length / 2]
                    .node_path
                    .extend([0; BLOCK_PATH_INLINE_CAPACITY]);
                assert!(!Blocks::can_pack(&blocks), "deep paths must remain dense");
            } else {
                assert!(
                    Blocks::packed_bytes_with_scratch(length).unwrap()
                        > length * size_of::<BlockMapping>()
                );
            }
        }
    }
}
