// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

/// Benchmark profile definition matching CrystalDiskMark 9 default test profiles.
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
    /// Returns the equivalent DiskSpd command line for this profile.
    pub fn diskspd_cmd(
        &self,
        duration_secs: u64,
        warmup_secs: u64,
        is_write: bool,
        file_path: &str,
    ) -> String {
        let w = if is_write { 100 } else { 0 };
        let r = if self.is_random { "-r" } else { "-si" };
        let b = if self.block_size >= 1024 * 1024 {
            format!("{}M", self.block_size / (1024 * 1024))
        } else {
            format!("{}K", self.block_size / 1024)
        };

        format!(
            "diskspd.exe -b{} -d{} -W{} -t1 -o{} {} -Sh -w{} -L {}",
            b, duration_secs, warmup_secs, self.queue_depth, r, w, file_path
        )
    }
}
