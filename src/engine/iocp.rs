// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! High-performance Win32 I/O Completion Port (IOCP) storage engine.
//!
//! # Zero-Allocation OVERLAPPED Lifetime & Pinning Contract
//! Each queue slot owns a pre-allocated `OVERLAPPED` structure pinned on the heap via `Box::into_raw`.
//! Throughout the benchmark run, the pre-allocated buffers and OVERLAPPED structures are reused across
//! completed operations without any heap allocations or deallocations in the hot IOCP pump loop.
//!
//! When the kernel signals completion via `GetQueuedCompletionStatusEx`, the returned `lpOverlapped`
//! pointer is matched against our pre-allocated slot list, metrics are recorded, and the slot is immediately
//! reissued. When the engine is dropped, all pinned `OVERLAPPED` structures are reclaimed via `Box::from_raw`.

use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, ReadFile, WriteFile};
use windows::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED, OVERLAPPED_ENTRY,
};

use super::buffers::AlignedBuffer;
use super::profile::{Profile, RwMix, XorShift64Star};
use super::qpc::{qpc_elapsed_us, qpc_now};
use super::stats::PassResult;

/// Operation type for an I/O request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOp {
    Read,
    Write,
}

/// Pre-allocated tracking record for an active in-flight asynchronous I/O request slot.
struct SlotContext {
    raw_ovl: *mut OVERLAPPED,
    issued_qpc: i64,
    op: IoOp,
    in_flight: bool,
}

unsafe impl Send for SlotContext {}
unsafe impl Sync for SlotContext {}

impl Drop for SlotContext {
    fn drop(&mut self) {
        if !self.raw_ovl.is_null() {
            // SAFETY: Reclaim the pinned heap memory allocated in IocpEngine::new.
            unsafe {
                let _ = Box::from_raw(self.raw_ovl);
            }
            self.raw_ovl = std::ptr::null_mut();
        }
    }
}

pub struct IocpEngine {
    iocp: HANDLE,
    file: HANDLE,
    file_size: u64,
    profile: Profile,
    buffers: Vec<AlignedBuffer>,
    slots: Vec<SlotContext>,
}

impl IocpEngine {
    /// Initialize a new IOCP engine for the given file handle and profile.
    pub fn new(file: HANDLE, file_size: u64, profile: &Profile) -> Result<Self> {
        let qd = profile.queue_depth;

        // SAFETY: Calling CreateIoCompletionPort with INVALID_HANDLE_VALUE and None creates a new standalone IOCP.
        let iocp = unsafe {
            CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1)
                .context("Failed to create standalone I/O Completion Port")?
        };

        // SAFETY: Associate the open file handle with the newly created completion port.
        unsafe {
            CreateIoCompletionPort(file, iocp, 1, 0)
                .context("Failed to associate file handle with I/O Completion Port")?;
        }

        // Allocate dedicated, sector-aligned buffer memory for each queue slot upfront.
        // For profiles with dual block sizes (e.g. directstorage: 32K / 64K), allocate the maximum size.
        let max_bs = profile.block_size.max(profile.block_size_alt.unwrap_or(0)) as usize;
        let mut buffers = Vec::with_capacity(qd);
        for _ in 0..qd {
            buffers.push(AlignedBuffer::new_random(max_bs)?);
        }

        // Pre-allocate OVERLAPPED structures pinned on heap for each queue slot.
        let mut slots = Vec::with_capacity(qd);
        for _ in 0..qd {
            let ovl = Box::new(unsafe { mem::zeroed::<OVERLAPPED>() });
            let raw_ovl = Box::into_raw(ovl);
            slots.push(SlotContext {
                raw_ovl,
                issued_qpc: 0,
                op: IoOp::Read,
                in_flight: false,
            });
        }

        Ok(Self {
            iocp,
            file,
            file_size,
            profile: profile.clone(),
            buffers,
            slots,
        })
    }

    /// Run one timed benchmark pass.
    ///
    /// - `duration_secs`: Total runtime of the pass (warmup + active measurement).
    /// - `warmup_secs`: Duration at the start of the pass whose I/Os are discarded.
    /// - `is_write`: `true` for `WriteFile`, `false` for `ReadFile` (used when `rw_mix` is None).
    /// - `rw_mix`: Optional Read:Write ratio (e.g. 70:30) for mixed workloads.
    /// - `cancelled`: Atomic flag set by Ctrl+C signal handler.
    pub fn run_pass(
        &mut self,
        duration_secs: u64,
        warmup_secs: u64,
        is_write: bool,
        rw_mix: Option<RwMix>,
        cancelled: &AtomicBool,
    ) -> Result<PassResult> {
        let mut result = PassResult::new()?;
        let qd = self.profile.queue_depth;
        let bs = self.profile.block_size;
        let min_bs = if let Some(alt) = self.profile.block_size_alt {
            bs.min(alt)
        } else {
            bs
        };

        if self.file_size < min_bs {
            anyhow::bail!(
                "Test file size is smaller than profile minimum block size ({} bytes)",
                min_bs
            );
        }

        let seed = (qpc_now() as u64) ^ 0x517CC1B727220A95;
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        let mut prng = XorShift64Star::new(seed ^ 0x9E3779B97F4A7C15);
        let mut seq_idx: u64 = 0;
        let mut write_counter: u32 = 0;

        let active_mix = rw_mix.or(self.profile.default_rw_mix);

        let pass_start = Instant::now();
        let warmup_end = std::time::Duration::from_secs(warmup_secs);
        let pass_end = std::time::Duration::from_secs(duration_secs);

        let mut in_flight_count = 0usize;
        let mut measuring = false;
        let mut measure_start = Instant::now();

        // 1. Prime the queue: dispatch the initial batch of N concurrent requests.
        for slot in 0..qd {
            self.issue_slot(
                slot,
                &mut rng,
                &mut prng,
                &mut seq_idx,
                is_write,
                active_mix,
            )?;
            in_flight_count += 1;
        }

        const MAX_ENTRIES: usize = 32;
        let mut entries: [OVERLAPPED_ENTRY; MAX_ENTRIES] = unsafe { mem::zeroed() };

        // 2. Completion pump: process completed I/Os and reissue immediately to maintain exact queue depth.
        loop {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }

            let elapsed = pass_start.elapsed();
            if elapsed >= pass_end {
                break;
            }

            // Transition from warmup to active measurement window.
            if !measuring && elapsed >= warmup_end {
                measuring = true;
                measure_start = Instant::now();
            }

            let timeout_ms = {
                let remaining = pass_end.saturating_sub(elapsed);
                (remaining.as_millis() as u32).clamp(1, 500)
            };

            let mut n_removed: u32 = 0;
            // SAFETY: entries is a valid slice; iocp is a valid completion port.
            let ok = unsafe {
                GetQueuedCompletionStatusEx(
                    self.iocp,
                    &mut entries,
                    &mut n_removed,
                    timeout_ms,
                    false,
                )
            };

            if ok.is_err() {
                // Timeout elapsed or port polled with no immediate completions; re-check time limits.
                continue;
            }

            let now_elapsed = pass_start.elapsed();
            let now_qpc = qpc_now();

            for entry in entries.iter().take(n_removed as usize) {
                let bytes = entry.dwNumberOfBytesTransferred as u64;

                // Match completion packet to its corresponding pre-allocated slot.
                let slot_idx = self
                    .slots
                    .iter()
                    .position(|s| s.raw_ovl == entry.lpOverlapped)
                    .expect("Received IOCP completion for untracked OVERLAPPED pointer");

                self.slots[slot_idx].in_flight = false;
                in_flight_count -= 1;

                // Record throughput and latency only if inside the measured window (post-warmup).
                if measuring && now_elapsed < pass_end {
                    let latency_us = qpc_elapsed_us(self.slots[slot_idx].issued_qpc, now_qpc);
                    match self.slots[slot_idx].op {
                        IoOp::Read => {
                            result.record_read(bytes, latency_us);
                        }
                        IoOp::Write => {
                            result.record_write(bytes, latency_us);
                            // Periodic cache flush check for OLTP database profile
                            if let Some(flush_interval) = self.profile.flush_interval_ios {
                                write_counter += 1;
                                if write_counter >= flush_interval {
                                    write_counter = 0;
                                    let flush_start = qpc_now();
                                    // SAFETY: Flush file buffers to drive non-volatile storage
                                    let _ = unsafe { FlushFileBuffers(self.file) };
                                    let flush_us = qpc_elapsed_us(flush_start, qpc_now());
                                    result.record_flush(flush_us);
                                }
                            }
                        }
                    }
                }

                // Re-issue another I/O into the vacated slot if pass duration has not elapsed.
                if now_elapsed < pass_end && !cancelled.load(Ordering::Relaxed) {
                    self.issue_slot(
                        slot_idx,
                        &mut rng,
                        &mut prng,
                        &mut seq_idx,
                        is_write,
                        active_mix,
                    )?;
                    in_flight_count += 1;
                }
            }
        }

        // 3. Drain remaining in-flight requests so no dangling asynchronous operations touch buffers.
        while in_flight_count > 0 {
            let mut n: u32 = 0;
            // SAFETY: Drain poll with short 200ms timeout.
            let _ =
                unsafe { GetQueuedCompletionStatusEx(self.iocp, &mut entries, &mut n, 200, false) };
            for entry in entries.iter().take(n as usize) {
                let ovl_ptr = entry.lpOverlapped;
                if let Some(idx) = self.slots.iter().position(|s| s.raw_ovl == ovl_ptr) {
                    if self.slots[idx].in_flight {
                        self.slots[idx].in_flight = false;
                        in_flight_count -= 1;
                    }
                }
            }
            if n == 0 {
                break;
            }
        }

        result.elapsed_ns = measure_start.elapsed().as_nanos() as u64;
        Ok(result)
    }

    /// Issue a single asynchronous `ReadFile` or `WriteFile` request into the specified slot.
    fn issue_slot(
        &mut self,
        slot: usize,
        rng: &mut Xoshiro256PlusPlus,
        prng: &mut XorShift64Star,
        seq_idx: &mut u64,
        is_write: bool,
        active_mix: Option<RwMix>,
    ) -> Result<()> {
        let op = if let Some(mix) = active_mix {
            if prng.decide_read(mix.read_pct) {
                IoOp::Read
            } else {
                IoOp::Write
            }
        } else if is_write {
            IoOp::Write
        } else {
            IoOp::Read
        };

        // Determine block size (for directstorage, 50% 32 KiB and 50% 64 KiB)
        let cur_bs = if let Some(alt) = self.profile.block_size_alt {
            if prng.next_u64() & 1 == 0 {
                alt
            } else {
                self.profile.block_size
            }
        } else {
            self.profile.block_size
        };

        let num_blocks = self.file_size / cur_bs;
        let offset = if self.profile.is_random {
            (rng.next_u64() % num_blocks) * cur_bs
        } else {
            let idx = *seq_idx % num_blocks;
            *seq_idx += 1;
            idx * cur_bs
        };

        let slot_ctx = &mut self.slots[slot];
        slot_ctx.op = op;
        slot_ctx.issued_qpc = qpc_now();

        // SAFETY: Reset the pre-allocated OVERLAPPED struct for reuse.
        unsafe {
            let ovl = &mut *slot_ctx.raw_ovl;
            ovl.Internal = 0;
            ovl.InternalHigh = 0;
            ovl.Anonymous.Anonymous.Offset = (offset & 0xFFFF_FFFF) as u32;
            ovl.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
            ovl.hEvent = windows::Win32::Foundation::HANDLE::default();
        }

        let buf_ptr = self.buffers[slot].as_ptr();
        let raw_ovl = slot_ctx.raw_ovl;

        let result = match op {
            IoOp::Write => {
                // SAFETY: buf_ptr is valid for cur_bs bytes; raw_ovl is pinned; file opened with FILE_FLAG_OVERLAPPED.
                unsafe {
                    WriteFile(
                        self.file,
                        Some(std::slice::from_raw_parts(buf_ptr, cur_bs as usize)),
                        None,
                        Some(raw_ovl),
                    )
                }
            }
            IoOp::Read => {
                // SAFETY: Same safety preconditions as WriteFile.
                unsafe {
                    ReadFile(
                        self.file,
                        Some(std::slice::from_raw_parts_mut(buf_ptr, cur_bs as usize)),
                        None,
                        Some(raw_ovl),
                    )
                }
            }
        };

        match result {
            Ok(_) => {
                self.slots[slot].in_flight = true;
            }
            Err(ref e) if e.code() == windows::Win32::Foundation::ERROR_IO_PENDING.to_hresult() => {
                // Expected status for asynchronous I/O completion.
                self.slots[slot].in_flight = true;
            }
            Err(e) => {
                self.slots[slot].in_flight = false;
                return Err(e).context("ReadFile/WriteFile submission failed");
            }
        }

        Ok(())
    }
}

impl Drop for IocpEngine {
    fn drop(&mut self) {
        if !self.iocp.is_invalid() {
            // SAFETY: Close the completion port handle.
            unsafe {
                let _ = CloseHandle(self.iocp);
            }
        }
    }
}
