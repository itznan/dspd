// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Benchmark profile definitions, mixed workload ratio (RwMix), PRNG, and domain presets.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Fast, deterministic 64-bit XorShift* pseudo-random number generator for IOCP dispatch.
#[derive(Debug, Clone)]
pub struct XorShift64Star {
    state: u64,
}

impl XorShift64Star {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x853C49E6748FEA9B
            } else {
                seed
            },
        }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// Determines if next operation should be Read based on read percentage (0..=100).
    #[inline]
    pub fn decide_read(&mut self, read_pct: u32) -> bool {
        if read_pct >= 100 {
            true
        } else if read_pct == 0 {
            false
        } else {
            (self.next_u64() % 100) < (read_pct as u64)
        }
    }
}

/// Mixed Read/Write workload ratio (e.g. 70:30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RwMix {
    pub read_pct: u32,
    pub write_pct: u32,
}

impl RwMix {
    pub fn new(read_pct: u32, write_pct: u32) -> Result<Self> {
        if read_pct + write_pct != 100 {
            anyhow::bail!(
                "Read and write percentages in --rw-mix must sum to 100 (got {}:{})",
                read_pct,
                write_pct
            );
        }
        Ok(Self {
            read_pct,
            write_pct,
        })
    }

    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split([':', '/']).collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "Invalid --rw-mix format '{}'. Expected READ:WRITE (e.g. 70:30)",
                s
            );
        }
        let read_pct: u32 = parts[0]
            .trim()
            .parse()
            .with_context(|| format!("Invalid read percentage '{}'", parts[0]))?;
        let write_pct: u32 = parts[1]
            .trim()
            .parse()
            .with_context(|| format!("Invalid write percentage '{}'", parts[1]))?;

        Self::new(read_pct, write_pct)
    }

    pub fn is_mixed(&self) -> bool {
        self.read_pct > 0 && self.write_pct > 0
    }
}

/// Benchmark profile definition for disk performance testing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    pub block_size: u64,
    pub block_size_alt: Option<u64>,
    pub queue_depth: usize,
    pub is_random: bool,
    pub default_rw_mix: Option<RwMix>,
    pub flush_interval_ios: Option<u32>,
    pub is_llm_stream: bool,
    pub is_read_only: bool,
}

pub const SEQ1M_Q8T1: Profile = Profile {
    name: "SEQ1M Q8T1",
    block_size: 1024 * 1024,
    block_size_alt: None,
    queue_depth: 8,
    is_random: false,
    default_rw_mix: None,
    flush_interval_ios: None,
    is_llm_stream: false,
    is_read_only: false,
};

pub const SEQ1M_Q1T1: Profile = Profile {
    name: "SEQ1M Q1T1",
    block_size: 1024 * 1024,
    block_size_alt: None,
    queue_depth: 1,
    is_random: false,
    default_rw_mix: None,
    flush_interval_ios: None,
    is_llm_stream: false,
    is_read_only: false,
};

pub const RND4K_Q32T1: Profile = Profile {
    name: "RND4K Q32T1",
    block_size: 4096,
    block_size_alt: None,
    queue_depth: 32,
    is_random: true,
    default_rw_mix: None,
    flush_interval_ios: None,
    is_llm_stream: false,
    is_read_only: false,
};

pub const RND4K_Q1T1: Profile = Profile {
    name: "RND4K Q1T1",
    block_size: 4096,
    block_size_alt: None,
    queue_depth: 1,
    is_random: true,
    default_rw_mix: None,
    flush_interval_ios: None,
    is_llm_stream: false,
    is_read_only: false,
};

/// Preset: directstorage (32 KiB and 64 KiB random reads at QD 64 simulating GPU asset streaming).
pub const DIRECTSTORAGE: Profile = Profile {
    name: "directstorage",
    block_size: 64 * 1024,
    block_size_alt: Some(32 * 1024),
    queue_depth: 64,
    is_random: true,
    default_rw_mix: None,
    flush_interval_ios: None,
    is_llm_stream: false,
    is_read_only: true,
};

/// Preset: llm-stream (2 MiB sequential unbuffered reads at QD 8 for LLM weights streaming).
pub const LLM_STREAM: Profile = Profile {
    name: "llm-stream",
    block_size: 2 * 1024 * 1024,
    block_size_alt: None,
    queue_depth: 8,
    is_random: false,
    default_rw_mix: None,
    flush_interval_ios: None,
    is_llm_stream: true,
    is_read_only: true,
};

/// Preset: oltp-db (8 KiB random 70:30 R/W mix at QD 32 with periodic FlushFileBuffers).
pub const OLTP_DB: Profile = Profile {
    name: "oltp-db",
    block_size: 8 * 1024,
    block_size_alt: None,
    queue_depth: 32,
    is_random: true,
    default_rw_mix: Some(RwMix {
        read_pct: 70,
        write_pct: 30,
    }),
    flush_interval_ios: Some(64),
    is_llm_stream: false,
    is_read_only: false,
};

pub const ALL_PROFILES: &[Profile] = &[SEQ1M_Q8T1, SEQ1M_Q1T1, RND4K_Q32T1, RND4K_Q1T1];

pub const ALL_AVAILABLE_PROFILES: &[Profile] = &[
    SEQ1M_Q8T1,
    SEQ1M_Q1T1,
    RND4K_Q32T1,
    RND4K_Q1T1,
    DIRECTSTORAGE,
    LLM_STREAM,
    OLTP_DB,
];

impl Profile {
    /// Returns the formatted configuration parameters for this profile.
    pub fn spec(&self, duration_secs: u64, warmup_secs: u64, is_write: bool) -> String {
        let block_str = if let Some(alt) = self.block_size_alt {
            format!("{}K/{}K", alt / 1024, self.block_size / 1024)
        } else if self.block_size >= 1024 * 1024 {
            format!("{} MiB", self.block_size / (1024 * 1024))
        } else {
            format!("{} KiB", self.block_size / 1024)
        };
        let pattern = if self.is_random {
            "Random"
        } else {
            "Sequential"
        };
        let direction = if let Some(mix) = self.default_rw_mix {
            format!("Mixed {}:{}", mix.read_pct, mix.write_pct)
        } else if is_write {
            "Write".to_string()
        } else {
            "Read".to_string()
        };

        format!(
            "[{:<13} {:<11}] Block: {:>7} | QD: {:>2} | Pattern: {:<10} | Duration: {}s (warmup: {}s)",
            self.name, direction, block_str, self.queue_depth, pattern, duration_secs, warmup_secs
        )
    }
}
