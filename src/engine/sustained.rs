// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Sustained sequential write profiling engine, time-series telemetry bucketing,
//! automatic SLC cache exhaustion cliff detection, and zero-dependency Unicode sparklines.

use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use hdrhistogram::Histogram;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::WriteFile;
use windows::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED, OVERLAPPED_ENTRY,
};

use super::buffers::AlignedBuffer;
use super::qpc::{qpc_elapsed_us, qpc_now};

/// Block size for sustained sequential write profiling: 1 MiB.
pub const SUSTAINED_BLOCK_SIZE: u64 = 1024 * 1024;

/// Queue depth for sustained sequential write profiling: 8.
pub const SUSTAINED_QUEUE_DEPTH: usize = 8;

/// Unicode block characters for throughput sparklines (U+2581 through U+2588).
pub const SPARK_BLOCKS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// 1-second telemetry bucket recording throughput, IOPS, and latency.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SecondBucket {
    /// 1-based second index of the sustained run.
    pub second: u32,
    /// Bytes transferred during this second.
    pub bytes: u64,
    /// Number of I/O operations completed during this second.
    pub io_count: u32,
    /// Throughput in MB/s (1 MB = 1,000,000 bytes).
    pub mb_s: f64,
    /// 99th percentile latency in microseconds for I/Os completed in this second.
    pub p99_latency_us: u64,
    /// Cumulative GiB written up to the end of this second.
    pub cumulative_gib: f64,
}

/// Analysis metrics derived from sustained write time-series buckets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SustainedAnalysis {
    /// Average MB/s during the initial high-speed pseudo-SLC cache plateau.
    pub burst_speed_mb_s: f64,
    /// Cumulative GiB written when throughput drops by >= 35% for >= 3 consecutive seconds.
    pub slc_cliff_gib: Option<f64>,
    /// Percentage drop in throughput from burst to sustained steady state.
    pub cliff_drop_percent: Option<f64>,
    /// Average MB/s after the SLC cache cliff stabilizes (direct-to-TLC/QLC speed).
    pub sustained_speed_mb_s: Option<f64>,
    /// Coefficient of variation (std_dev / mean * 100%) measuring throughput stability.
    pub cv_percent: f64,
    /// Maximum 1-second throughput observed during the run.
    pub peak_speed_mb_s: f64,
    /// Minimum 1-second throughput observed during the run.
    pub min_speed_mb_s: f64,
    /// Total cumulative GiB written.
    pub total_gib_written: f64,
    /// Total duration of the test in seconds.
    pub duration_secs: u32,
    /// Unicode block sparkline visualization of throughput over time.
    pub sparkline: String,
}

/// Tracking metadata for in-flight requests in the pre-allocated pool.
struct InFlightSlot {
    issued_qpc: i64,
    active: bool,
}

/// Render a slice of float values into a zero-dependency Unicode block sparkline.
pub fn render_sparkline(values: &[f64]) -> String {
    if values.is_empty() {
        return String::new();
    }

    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    if (max - min).abs() < 1e-6 {
        return "█".repeat(values.len());
    }

    values
        .iter()
        .map(|&v| {
            let normalized = ((v - min) / (max - min)).clamp(0.0, 1.0);
            let idx = (normalized * 7.0).round() as usize;
            SPARK_BLOCKS[idx.min(7)]
        })
        .collect()
}

/// Render a sparkline downsampled to a maximum console character width.
pub fn render_sparkline_resampled(values: &[f64], max_width: usize) -> String {
    if values.is_empty() {
        return String::new();
    }
    if values.len() <= max_width {
        return render_sparkline(values);
    }

    let mut resampled = Vec::with_capacity(max_width);
    for i in 0..max_width {
        let start = (i * values.len()) / max_width;
        let end = ((i + 1) * values.len()) / max_width;
        let chunk = &values[start..end.max(start + 1)];
        let avg = chunk.iter().sum::<f64>() / chunk.len() as f64;
        resampled.push(avg);
    }
    render_sparkline(&resampled)
}

/// Analyze time-series buckets to detect SLC cache exhaustion and steady-state speeds.
pub fn analyze_sustained_buckets(buckets: &[SecondBucket]) -> SustainedAnalysis {
    if buckets.is_empty() {
        return SustainedAnalysis {
            burst_speed_mb_s: 0.0,
            slc_cliff_gib: None,
            cliff_drop_percent: None,
            sustained_speed_mb_s: None,
            cv_percent: 0.0,
            peak_speed_mb_s: 0.0,
            min_speed_mb_s: 0.0,
            total_gib_written: 0.0,
            duration_secs: 0,
            sparkline: String::new(),
        };
    }

    let throughputs: Vec<f64> = buckets.iter().map(|b| b.mb_s).collect();
    let peak_speed = throughputs.iter().copied().fold(0.0, f64::max);
    let min_speed = throughputs.iter().copied().fold(f64::INFINITY, f64::min);
    let total_gib = buckets.last().map(|b| b.cumulative_gib).unwrap_or(0.0);
    let duration = buckets.len() as u32;

    // Throughput stability: Coefficient of variation (std_dev / mean * 100%)
    let mean_speed = throughputs.iter().sum::<f64>() / throughputs.len() as f64;
    let variance = throughputs
        .iter()
        .map(|&s| (s - mean_speed).powi(2))
        .sum::<f64>()
        / throughputs.len() as f64;
    let std_dev = variance.sqrt();
    let cv_percent = if mean_speed > 0.0 {
        (std_dev / mean_speed) * 100.0
    } else {
        0.0
    };

    // Automatic SLC Cliff Detection:
    // Drop by >= 35% from the candidate burst speed plateau for at least 3 consecutive seconds.
    let mut cliff_idx = None;
    let mut burst_speed = mean_speed;

    for i in 1..buckets.len() {
        // Average MB/s during the initial plateau leading up to index i
        let cand_burst = throughputs[0..i].iter().sum::<f64>() / i as f64;
        let drop_threshold = cand_burst * (1.0 - 0.35);

        // Check for 3 consecutive seconds at or below the drop threshold
        if i + 2 < buckets.len()
            && throughputs[i] <= drop_threshold
            && throughputs[i + 1] <= drop_threshold
            && throughputs[i + 2] <= drop_threshold
        {
            cliff_idx = Some(i);
            burst_speed = cand_burst;
            break;
        }
    }

    let (slc_cliff_gib, cliff_drop_percent, sustained_speed_mb_s) = if let Some(idx) = cliff_idx {
        // Cumulative GiB written up to the moment the drop began
        let cliff_gib = buckets[idx.saturating_sub(1)].cumulative_gib;
        let sustained_slice = &throughputs[idx..];
        let sustained_avg = sustained_slice.iter().sum::<f64>() / sustained_slice.len() as f64;
        let drop_pct = if burst_speed > 0.0 {
            ((burst_speed - sustained_avg) / burst_speed) * 100.0
        } else {
            0.0
        };
        (Some(cliff_gib), Some(drop_pct), Some(sustained_avg))
    } else {
        (None, None, None)
    };

    let sparkline = render_sparkline_resampled(&throughputs, 60);

    SustainedAnalysis {
        burst_speed_mb_s: burst_speed,
        slc_cliff_gib,
        cliff_drop_percent,
        sustained_speed_mb_s,
        cv_percent,
        peak_speed_mb_s: peak_speed,
        min_speed_mb_s: if min_speed.is_infinite() {
            0.0
        } else {
            min_speed
        },
        total_gib_written: total_gib,
        duration_secs: duration,
        sparkline,
    }
}

/// Execute a continuous unbuffered sequential write pass with pre-allocated 1-second bucketing.
/// Guarantees zero heap allocations inside the IOCP completion loop.
pub fn run_sustained_write_iocp<F>(
    file_handle: HANDLE,
    target_bytes: u64,
    max_duration_secs: u64,
    cancelled: &AtomicBool,
    mut on_progress: F,
) -> Result<Vec<SecondBucket>>
where
    F: FnMut(u64, f64),
{
    let qd = SUSTAINED_QUEUE_DEPTH;
    let bs = SUSTAINED_BLOCK_SIZE;
    let num_blocks = target_bytes / bs;

    if num_blocks == 0 {
        anyhow::bail!("Target write size is smaller than 1 MiB block size");
    }

    // 1. Create dedicated standalone IOCP
    let iocp = unsafe {
        CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1)
            .context("Failed to create standalone I/O Completion Port")?
    };

    // Associate file handle with completion port
    unsafe {
        CreateIoCompletionPort(file_handle, iocp, 1, 0)
            .context("Failed to associate file handle with IOCP")?;
    }

    // 2. Pre-allocate sector-aligned entropy buffers (1 per queue slot)
    let mut buffers = Vec::with_capacity(qd);
    for _ in 0..qd {
        buffers.push(AlignedBuffer::new_random(bs as usize)?);
    }

    // 3. Pre-allocate OVERLAPPED structures for zero-allocation I/O re-use
    let mut ovl_pool: Vec<Box<OVERLAPPED>> = (0..qd)
        .map(|_| Box::new(unsafe { mem::zeroed() }))
        .collect();

    let mut in_flight_slots: Vec<InFlightSlot> = (0..qd)
        .map(|_| InFlightSlot {
            issued_qpc: 0,
            active: false,
        })
        .collect();

    // 4. Pre-allocate time-series telemetry buckets
    let max_buckets = (max_duration_secs as usize + 64).max(128);
    let mut buckets: Vec<SecondBucket> = Vec::with_capacity(max_buckets);

    // 5. Pre-allocate 1-second latency histogram (cleared in-place each second with 0 allocations)
    let mut active_hist = Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3)?;

    let run_start = Instant::now();
    let max_duration = std::time::Duration::from_secs(max_duration_secs);

    let mut current_sec: usize = 0;
    let mut cur_sec_bytes: u64 = 0;
    let mut cur_sec_io: u32 = 0;
    let mut cumulative_bytes: u64 = 0;
    let mut seq_block_idx: u64 = 0;
    let mut in_flight_count: usize = 0;

    // Helper closure to dispatch slot I/O
    let issue_slot = |_slot: usize,
                      block_idx: u64,
                      ovl: &mut Box<OVERLAPPED>,
                      buf: &AlignedBuffer,
                      slot_meta: &mut InFlightSlot|
     -> Result<()> {
        let offset = (block_idx % num_blocks) * bs;
        unsafe {
            *ovl.as_mut() = mem::zeroed();
        }
        ovl.Anonymous.Anonymous.Offset = offset as u32;
        ovl.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;

        let issued_qpc = qpc_now();
        slot_meta.issued_qpc = issued_qpc;
        slot_meta.active = true;

        let raw_ovl = ovl.as_mut() as *mut OVERLAPPED;
        let buf_ptr = buf.as_ptr();

        let res = unsafe {
            WriteFile(
                file_handle,
                Some(std::slice::from_raw_parts(buf_ptr, bs as usize)),
                None,
                Some(raw_ovl),
            )
        };

        match res {
            Ok(_) => {}
            Err(ref e) if e.code() == windows::Win32::Foundation::ERROR_IO_PENDING.to_hresult() => {
            }
            Err(e) => {
                slot_meta.active = false;
                return Err(e).context("WriteFile submission failed in sustained write engine");
            }
        }

        Ok(())
    };

    // 6. Prime the queue with initial batch
    for slot in 0..qd {
        if seq_block_idx >= num_blocks {
            break;
        }
        issue_slot(
            slot,
            seq_block_idx,
            &mut ovl_pool[slot],
            &buffers[slot],
            &mut in_flight_slots[slot],
        )?;
        seq_block_idx += 1;
        in_flight_count += 1;
    }

    const MAX_ENTRIES: usize = 32;
    let mut entries: [OVERLAPPED_ENTRY; MAX_ENTRIES] = unsafe { mem::zeroed() };

    // 7. Zero-allocation completion pump
    while in_flight_count > 0 {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }

        let elapsed = run_start.elapsed();
        if elapsed >= max_duration {
            break;
        }

        let timeout_ms = {
            let remaining = max_duration.saturating_sub(elapsed);
            (remaining.as_millis() as u32).clamp(1, 500)
        };

        let mut n_removed: u32 = 0;
        let ok = unsafe {
            GetQueuedCompletionStatusEx(iocp, &mut entries, &mut n_removed, timeout_ms, false)
        };

        if ok.is_err() {
            // Timeout or empty poll; continue checking deadlines
            continue;
        }

        let now_elapsed = run_start.elapsed();
        let now_qpc = qpc_now();
        let sec = now_elapsed.as_secs() as usize;

        // Check if 1-second boundary has rolled over
        if sec != current_sec {
            let p99 = active_hist.value_at_quantile(0.99);
            let mb_s = (cur_sec_bytes as f64) / 1_000_000.0;
            let cum_gib = (cumulative_bytes as f64) / (1024.0 * 1024.0 * 1024.0);

            buckets.push(SecondBucket {
                second: (current_sec + 1) as u32,
                bytes: cur_sec_bytes,
                io_count: cur_sec_io,
                mb_s,
                p99_latency_us: p99,
                cumulative_gib: cum_gib,
            });

            on_progress(cumulative_bytes, mb_s);

            active_hist.clear();
            cur_sec_bytes = 0;
            cur_sec_io = 0;

            // Handle any skipped empty seconds
            for empty_sec in (current_sec + 1)..sec {
                buckets.push(SecondBucket {
                    second: (empty_sec + 1) as u32,
                    bytes: 0,
                    io_count: 0,
                    mb_s: 0.0,
                    p99_latency_us: 0,
                    cumulative_gib: cum_gib,
                });
            }

            current_sec = sec;
        }

        for entry in entries.iter().take(n_removed as usize) {
            let bytes = entry.dwNumberOfBytesTransferred as u64;

            // Locate slot by matching OVERLAPPED pointer
            let slot = match ovl_pool
                .iter()
                .position(|b| std::ptr::eq(&**b, entry.lpOverlapped))
            {
                Some(s) => s,
                None => continue,
            };

            in_flight_slots[slot].active = false;
            in_flight_count -= 1;

            let latency_us = qpc_elapsed_us(in_flight_slots[slot].issued_qpc, now_qpc);
            active_hist.saturating_record(latency_us);
            cur_sec_bytes += bytes;
            cur_sec_io += 1;
            cumulative_bytes += bytes;

            // Re-issue write if more data remains to be written and time has not expired
            if cumulative_bytes < target_bytes
                && now_elapsed < max_duration
                && !cancelled.load(Ordering::Relaxed)
            {
                issue_slot(
                    slot,
                    seq_block_idx,
                    &mut ovl_pool[slot],
                    &buffers[slot],
                    &mut in_flight_slots[slot],
                )?;
                seq_block_idx += 1;
                in_flight_count += 1;
            }
        }
    }

    // 8. Drain any remaining in-flight completions
    while in_flight_count > 0 {
        let mut n: u32 = 0;
        let _ = unsafe { GetQueuedCompletionStatusEx(iocp, &mut entries, &mut n, 200, false) };
        for entry in entries.iter().take(n as usize) {
            if let Some(slot) = ovl_pool
                .iter()
                .position(|b| std::ptr::eq(&**b, entry.lpOverlapped))
            {
                if in_flight_slots[slot].active {
                    in_flight_slots[slot].active = false;
                    in_flight_count -= 1;
                }
            }
        }
        if n == 0 {
            break;
        }
    }

    // 9. Finalize final active bucket
    if cur_sec_bytes > 0 || buckets.is_empty() {
        let p99 = active_hist.value_at_quantile(0.99);
        let mb_s = (cur_sec_bytes as f64) / 1_000_000.0;
        let cum_gib = (cumulative_bytes as f64) / (1024.0 * 1024.0 * 1024.0);
        buckets.push(SecondBucket {
            second: (current_sec + 1) as u32,
            bytes: cur_sec_bytes,
            io_count: cur_sec_io,
            mb_s,
            p99_latency_us: p99,
            cumulative_gib: cum_gib,
        });
        on_progress(cumulative_bytes, mb_s);
    }

    // Clean up IOCP handle
    unsafe {
        let _ = CloseHandle(iocp);
    }

    Ok(buckets)
}
