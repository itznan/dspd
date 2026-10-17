// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

#[test]
fn test_random_offsets_are_aligned() {
    let block_size = 4096u64;
    let file_size = 1024 * 1024 * 1024u64; // 1 GiB
    let num_blocks = file_size / block_size;

    let mut rng = Xoshiro256PlusPlus::seed_from_u64(42);

    for _ in 0..100_000 {
        let block_idx = rng.next_u64() % num_blocks;
        let offset = block_idx * block_size;

        assert_eq!(offset % block_size, 0, "Offset must be block aligned");
        assert!(
            offset + block_size <= file_size,
            "Offset plus block size must stay within file boundary"
        );
    }
}

#[test]
fn test_sequential_offsets_are_aligned() {
    let block_size = 1024 * 1024u64; // 1 MiB
    let file_size = 1024 * 1024 * 1024u64;
    let num_blocks = file_size / block_size;

    for i in 0..num_blocks * 2 {
        let offset = (i % num_blocks) * block_size;
        assert_eq!(offset % block_size, 0);
        assert!(offset + block_size <= file_size);
    }
}
