// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Benchmark pass result metrics, multi-pass averaging, and percentile calculations.

use anyhow::Result;
use hdrhistogram::Histogram;

/// Benchmark metrics collected from one measurement pass (supports pure and mixed workloads).
#[derive(Debug, Clone)]
pub struct PassResult {
    /// Wall-clock nanoseconds of the active measurement window.
    pub elapsed_ns: u64,
    /// Total bytes successfully transferred (read + written).
    pub bytes_xfer: u64,
    /// Total bytes read.
    pub bytes_read: u64,
    /// Total bytes written.
    pub bytes_written: u64,
    /// Read I/O operations count.
    pub read_ios: u64,
    /// Write I/O operations count.
    pub write_ios: u64,
    /// Latency histogram for Read operations in microseconds.
    pub read_hist: Histogram<u64>,
    /// Latency histogram for Write operations in microseconds.
    pub write_hist: Histogram<u64>,
    /// Combined latency histogram in microseconds.
    pub hist: Histogram<u64>,
    /// Periodic FlushFileBuffers count.
    pub flush_count: u64,
    /// Total duration of all FlushFileBuffers calls in microseconds.
    pub flush_total_us: u64,
    /// CPU efficiency telemetry for the pass.
    pub cpu: Option<super::cpu::CpuMetrics>,
}

impl PassResult {
    /// Creates a new, empty `PassResult` tracking values up to 3,600,000,000 µs (1 hr) with 3 sigfigs.
    pub fn new() -> Result<Self> {
        Ok(Self {
            elapsed_ns: 0,
            bytes_xfer: 0,
            bytes_read: 0,
            bytes_written: 0,
            read_ios: 0,
            write_ios: 0,
            read_hist: Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3)?,
            write_hist: Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3)?,
            hist: Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3)?,
            flush_count: 0,
            flush_total_us: 0,
            cpu: None,
        })
    }

    /// Record a completed read operation.
    pub fn record_read(&mut self, bytes: u64, latency_us: u64) {
        self.bytes_read += bytes;
        self.bytes_xfer += bytes;
        self.read_ios += 1;
        self.read_hist.saturating_record(latency_us);
        self.hist.saturating_record(latency_us);
    }

    /// Record a completed write operation.
    pub fn record_write(&mut self, bytes: u64, latency_us: u64) {
        self.bytes_written += bytes;
        self.bytes_xfer += bytes;
        self.write_ios += 1;
        self.write_hist.saturating_record(latency_us);
        self.hist.saturating_record(latency_us);
    }

    /// Record a completed flush operation.
    pub fn record_flush(&mut self, latency_us: u64) {
        self.flush_count += 1;
        self.flush_total_us += latency_us;
    }

    /// Total throughput in MB/s (1 MB = 1,000,000 bytes).
    pub fn throughput_mb_s(&self) -> f64 {
        if self.elapsed_ns == 0 {
            return 0.0;
        }
        (self.bytes_xfer as f64 / self.elapsed_ns as f64) * 1_000.0
    }

    /// Read throughput in MB/s.
    pub fn read_mb_s(&self) -> f64 {
        if self.elapsed_ns == 0 {
            return 0.0;
        }
        (self.bytes_read as f64 / self.elapsed_ns as f64) * 1_000.0
    }

    /// Write throughput in MB/s.
    pub fn write_mb_s(&self) -> f64 {
        if self.elapsed_ns == 0 {
            return 0.0;
        }
        (self.bytes_written as f64 / self.elapsed_ns as f64) * 1_000.0
    }

    /// Total IOPS.
    pub fn iops(&self, _block_size: u64) -> f64 {
        if self.elapsed_ns == 0 {
            return 0.0;
        }
        let total_ios = (self.read_ios + self.write_ios) as f64;
        let secs = self.elapsed_ns as f64 / 1_000_000_000.0;
        total_ios / secs
    }

    /// Read IOPS.
    pub fn read_iops(&self) -> f64 {
        if self.elapsed_ns == 0 {
            return 0.0;
        }
        let secs = self.elapsed_ns as f64 / 1_000_000_000.0;
        self.read_ios as f64 / secs
    }

    /// Write IOPS.
    pub fn write_iops(&self) -> f64 {
        if self.elapsed_ns == 0 {
            return 0.0;
        }
        let secs = self.elapsed_ns as f64 / 1_000_000_000.0;
        self.write_ios as f64 / secs
    }

    /// Read latency percentiles.
    pub fn read_p50_us(&self) -> u64 {
        self.read_hist.value_at_quantile(0.50)
    }

    pub fn read_p99_us(&self) -> u64 {
        self.read_hist.value_at_quantile(0.99)
    }

    pub fn read_p999_us(&self) -> u64 {
        self.read_hist.value_at_quantile(0.999)
    }

    pub fn read_avg_us(&self) -> f64 {
        self.read_hist.mean()
    }

    /// Write latency percentiles.
    pub fn write_p50_us(&self) -> u64 {
        self.write_hist.value_at_quantile(0.50)
    }

    pub fn write_p99_us(&self) -> u64 {
        self.write_hist.value_at_quantile(0.99)
    }

    pub fn write_p999_us(&self) -> u64 {
        self.write_hist.value_at_quantile(0.999)
    }

    pub fn write_avg_us(&self) -> f64 {
        self.write_hist.mean()
    }

    /// Average flush latency in microseconds.
    pub fn avg_flush_us(&self) -> f64 {
        if self.flush_count > 0 {
            self.flush_total_us as f64 / self.flush_count as f64
        } else {
            0.0
        }
    }

    /// Combined average I/O latency in microseconds.
    pub fn avg_latency_us(&self) -> f64 {
        self.hist.mean()
    }

    /// Combined 50th percentile (p50) latency in microseconds.
    pub fn p50_latency_us(&self) -> u64 {
        self.hist.value_at_quantile(0.50)
    }

    /// Combined 99th percentile (p99) latency in microseconds.
    pub fn p99_latency_us(&self) -> u64 {
        self.hist.value_at_quantile(0.99)
    }

    /// Combined 99.9th percentile (p99.9) latency in microseconds.
    pub fn p999_latency_us(&self) -> u64 {
        self.hist.value_at_quantile(0.999)
    }
}

/// Compute average metrics across multiple passes.
pub fn average_passes(results: &[PassResult]) -> Result<PassResult> {
    let mut averaged = PassResult::new()?;
    let n = results.len() as u128;
    if n == 0 {
        return Ok(averaged);
    }

    let mut sum_elapsed_ns = 0u128;
    let mut sum_bytes_xfer = 0u128;
    let mut sum_bytes_read = 0u128;
    let mut sum_bytes_written = 0u128;
    let mut sum_read_ios = 0u128;
    let mut sum_write_ios = 0u128;
    let mut sum_flush_count = 0u128;
    let mut sum_flush_total_us = 0u128;

    let mut total_cpu_util = 0.0;
    let mut total_cycles_per_io = 0u128;
    let mut total_iops_per_core = 0.0;
    let mut cpu_count = 0u64;

    for r in results {
        sum_elapsed_ns += r.elapsed_ns as u128;
        sum_bytes_xfer += r.bytes_xfer as u128;
        sum_bytes_read += r.bytes_read as u128;
        sum_bytes_written += r.bytes_written as u128;
        sum_read_ios += r.read_ios as u128;
        sum_write_ios += r.write_ios as u128;
        sum_flush_count += r.flush_count as u128;
        sum_flush_total_us += r.flush_total_us as u128;
        averaged.read_hist.add(&r.read_hist)?;
        averaged.write_hist.add(&r.write_hist)?;
        averaged.hist.add(&r.hist)?;

        if let Some(c) = r.cpu {
            total_cpu_util += c.cpu_util_pct;
            total_cycles_per_io += c.cycles_per_io as u128;
            total_iops_per_core += c.iops_per_core;
            cpu_count += 1;
        }
    }

    averaged.elapsed_ns = (sum_elapsed_ns / n) as u64;
    averaged.bytes_xfer = (sum_bytes_xfer / n) as u64;
    averaged.bytes_read = (sum_bytes_read / n) as u64;
    averaged.bytes_written = (sum_bytes_written / n) as u64;
    averaged.read_ios = (sum_read_ios / n) as u64;
    averaged.write_ios = (sum_write_ios / n) as u64;
    averaged.flush_count = if sum_flush_count > 0 {
        ((sum_flush_count + n / 2) / n).max(1) as u64
    } else {
        0
    };
    averaged.flush_total_us = (sum_flush_total_us / n) as u64;

    if cpu_count > 0 {
        averaged.cpu = Some(super::cpu::CpuMetrics {
            cpu_util_pct: total_cpu_util / (cpu_count as f64),
            cycles_per_io: (total_cycles_per_io / cpu_count as u128) as u64,
            iops_per_core: total_iops_per_core / (cpu_count as f64),
        });
    }

    Ok(averaged)
}
