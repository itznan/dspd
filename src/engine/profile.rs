// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

/// Benchmark profile definition for disk performance testing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    pub block_size: u64,
    pub queue_depth: usize,
    pub is_random: bool,
}

pub const SEQ1M_Q8T1: Profile = Profile {
    name: "SEQ1M Q8T1",
    block_size: 1024 * 1024,
    queue_depth: 8,
    is_random: false,
};

pub const SEQ1M_Q1T1: Profile = Profile {
    name: "SEQ1M Q1T1",
    block_size: 1024 * 1024,
    queue_depth: 1,
    is_random: false,
};

pub const RND4K_Q32T1: Profile = Profile {
    name: "RND4K Q32T1",
    block_size: 4096,
    queue_depth: 32,
    is_random: true,
};

pub const RND4K_Q1T1: Profile = Profile {
    name: "RND4K Q1T1",
    block_size: 4096,
    queue_depth: 1,
    is_random: true,
};

pub const ALL_PROFILES: &[Profile] = &[SEQ1M_Q8T1, SEQ1M_Q1T1, RND4K_Q32T1, RND4K_Q1T1];

impl Profile {
    /// Returns the formatted configuration parameters for this profile.
    pub fn spec(&self, duration_secs: u64, warmup_secs: u64, is_write: bool) -> String {
        let block_str = if self.block_size >= 1024 * 1024 {
            format!("{} MiB", self.block_size / (1024 * 1024))
        } else {
            format!("{} KiB", self.block_size / 1024)
        };
        let pattern = if self.is_random {
            "Random"
        } else {
            "Sequential"
        };
        let direction = if is_write { "Write" } else { "Read" };

        format!(
            "[{:<11} {:<5}] Block Size: {:>6} | Queue Depth: {:>2} | Pattern: {:<10} | Duration: {}s (warmup: {}s)",
            self.name, direction, block_str, self.queue_depth, pattern, duration_secs, warmup_secs
        )
    }
}
