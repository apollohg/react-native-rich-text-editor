#[test]
fn android_compression_matches_software_for_unaligned_blocks_and_arbitrary_states() {
    const BLOCK_BYTES: usize = 64;
    const MAX_BLOCKS: usize = 128;
    const ALIGNMENTS: usize = 16;
    const SEEDS: [u32; 4] = [0, 1, u32::MAX, 0x1357_9bdf];
    let mut bytes = [0u8; MAX_BLOCKS * BLOCK_BYTES + ALIGNMENTS];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (index.wrapping_mul(73) ^ (index >> 7)) as u8;
    }
    for alignment in 0..ALIGNMENTS {
        for count in [0, 1, 2, 3, 64, MAX_BLOCKS] {
            let input = &bytes[alignment..alignment + count * BLOCK_BYTES];
            let blocks = input.as_chunks::<BLOCK_BYTES>().0;
            for seed in SEEDS {
                let mut actual = core::array::from_fn(|index| {
                    seed.wrapping_mul(index as u32 + 1).rotate_left(index as u32)
                });
                let mut expected = actual;
                super::aarch64::compress(&mut actual, blocks);
                super::soft::compress(&mut expected, blocks);
                assert_eq!(actual, expected, "alignment={alignment} count={count} seed={seed}");
            }
        }
    }
}
