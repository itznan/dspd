// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use anyhow::Result;
use hdrhistogram::Histogram;

/// Benchmark metrics collected from one measurement pass.
#[derive(Debug, Clone)]
pub struct PassResult {
    /// Wall-clock nanoseconds of the active measurement window.
    pub elapsed_ns: u64,
    /// Total bytes successfully transferred in the measurement window.
    pub bytes_xfer: u64,
    /// Latency histogram in microseconds (trackable up to 3.6 million ms / 1 hour).
    pub hist: Histogram<u64>,
}

impl PassResult {
    /// Creates a new, empty `PassResult` tracking values up to 3,600,000,000 µs (1 hr) with 3 sigfigs.
    pub fn new() -> Result<Self> {
        Ok(Self {
            elapsed_ns: 0,
            bytes_xfer: 0,
            hist: Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3)?,
        })
    }

    /// Throughput in MB/s (1 MB = 1,000,000 bytes, following CrystalDiskMark convention).
    pub fn throughput_mb_s(&self) -> f64 {
        if self.elapsed_ns == 0 {
            return 0.0;
        }
        (self.bytes_xfer as f64 / self.elapsed_ns as f64) * 1_000.0
    }

    /// IOPS (I/O operations per second).
    pub fn iops(&self, block_size: u64) -> f64 {
        if self.elapsed_ns == 0 || block_size == 0 {
            return 0.0;
        }
        let ios = self.bytes_xfer as f64 / block_size as f64;
        let secs = self.elapsed_ns as f64 / 1_000_000_000.0;
        ios / secs
    }

    /// Average I/O latency in microseconds.
    pub fn avg_latency_us(&self) -> f64 {
        self.hist.mean()
    }

    /// 99th percentile (p99) latency in microseconds.
    pub fn p99_latency_us(&self) -> u64 {
        self.hist.value_at_quantile(0.99)
    }
}

/// Compute average metrics across multiple passes.
pub fn average_passes(results: &[PassResult]) -> Result<PassResult> {
    let mut averaged = PassResult::new()?;
    let n = results.len() as u64;
    if n == 0 {
        return Ok(averaged);
    }

    for r in results {
        averaged.elapsed_ns += r.elapsed_ns / n;
        averaged.bytes_xfer += r.bytes_xfer / n;
        averaged.hist.add(&r.hist)?;
    }

    Ok(averaged)
}
