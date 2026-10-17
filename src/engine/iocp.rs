// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! High-performance Win32 I/O Completion Port (IOCP) storage engine.
//!
//! # OVERLAPPED Lifetime & Pinning Contract
//! Each in-flight I/O request owns an `OVERLAPPED` structure pinned on the heap via `Box<OVERLAPPED>`.
//! Immediately prior to calling `ReadFile` or `WriteFile`, `Box::into_raw` leaks the pointer, transferring
//! temporary memory ownership to the Windows kernel.
//!
//! When the kernel signals completion via `GetQueuedCompletionStatusEx`, the returned `lpOverlapped`
//! pointer is matched against our in-flight tracking list, and ownership is safely reclaimed via
//! `Box::from_raw`. If `ReadFile` or `WriteFile` fails immediately (and does not return `ERROR_IO_PENDING`),
//! ownership is reclaimed immediately to avoid leaking memory.

use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED, OVERLAPPED_ENTRY,
};

use super::buffers::AlignedBuffer;
use super::profile::Profile;
use super::qpc::{qpc_elapsed_us, qpc_now};
use super::stats::PassResult;

/// Tracking record for an active in-flight asynchronous I/O request.
struct InFlight {
    /// Raw pointer to heap-allocated OVERLAPPED struct currently in possession of the OS kernel.
    ovl: *mut OVERLAPPED,
    /// Index of the sector-aligned buffer slot dedicated to this in-flight request.
    buf_slot: usize,
    /// QPC timestamp recorded at the exact moment the I/O was submitted to the kernel.
    issued_qpc: i64,
}

pub struct IocpEngine {
    iocp: HANDLE,
    file: HANDLE,
    file_size: u64,
    profile: Profile,
    buffers: Vec<AlignedBuffer>,
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
        let bs = profile.block_size as usize;
        let mut buffers = Vec::with_capacity(qd);
        for _ in 0..qd {
            buffers.push(AlignedBuffer::new_random(bs)?);
        }

        Ok(Self {
            iocp,
            file,
            file_size,
            profile: profile.clone(),
            buffers,
        })
    }

    /// Run one timed benchmark pass.
    ///
    /// - `duration_secs`: Total runtime of the pass (warmup + active measurement).
    /// - `warmup_secs`: Duration at the start of the pass whose I/Os are discarded.
    /// - `is_write`: `true` for `WriteFile`, `false` for `ReadFile`.
    /// - `cancelled`: Atomic flag set by Ctrl+C signal handler.
    pub fn run_pass(
        &mut self,
        duration_secs: u64,
        warmup_secs: u64,
        is_write: bool,
        cancelled: &AtomicBool,
    ) -> Result<PassResult> {
        let mut result = PassResult::new()?;
        let qd = self.profile.queue_depth;
        let bs = self.profile.block_size;
        let num_blocks = self.file_size / bs;

        if num_blocks == 0 {
            anyhow::bail!(
                "Test file size is smaller than profile block size ({} bytes)",
                bs
            );
        }

        let seed = (qpc_now() as u64) ^ 0x517CC1B727220A95;
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        let mut seq_idx: u64 = 0;

        let pass_start = Instant::now();
        let warmup_end = std::time::Duration::from_secs(warmup_secs);
        let pass_end = std::time::Duration::from_secs(duration_secs);

        let mut in_flight: Vec<InFlight> = Vec::with_capacity(qd);
        let mut in_flight_count = 0usize;
        let mut measuring = false;
        let mut measure_start = Instant::now();

        // 1. Prime the queue: dispatch the initial batch of N concurrent requests.
        for slot in 0..qd {
            self.issue_one(
                slot,
                &mut rng,
                &mut seq_idx,
                num_blocks,
                is_write,
                &mut in_flight,
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

                // Match completion packet to its corresponding in-flight record.
                let inf_idx = in_flight
                    .iter()
                    .position(|f| f.ovl == entry.lpOverlapped)
                    .expect("Received IOCP completion for untracked OVERLAPPED pointer");
                let inf = in_flight.swap_remove(inf_idx);

                // SAFETY: Kernel has finished with the OVERLAPPED memory; reclaim ownership to free it.
                let _ovl_box: Box<OVERLAPPED> = unsafe { Box::from_raw(inf.ovl) };
                in_flight_count -= 1;

                // Record throughput and latency only if inside the measured window (post-warmup).
                if measuring && now_elapsed < pass_end {
                    let latency_us = qpc_elapsed_us(inf.issued_qpc, now_qpc);
                    result.hist.saturating_record(latency_us);
                    result.bytes_xfer += bytes;
                }

                // Re-issue another I/O into the vacated slot if pass duration has not elapsed.
                if now_elapsed < pass_end && !cancelled.load(Ordering::Relaxed) {
                    self.issue_one(
                        inf.buf_slot,
                        &mut rng,
                        &mut seq_idx,
                        num_blocks,
                        is_write,
                        &mut in_flight,
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
                if let Some(idx) = in_flight.iter().position(|f| f.ovl == ovl_ptr) {
                    let inf = in_flight.swap_remove(idx);
                    // SAFETY: Reclaim memory.
                    let _: Box<OVERLAPPED> = unsafe { Box::from_raw(inf.ovl) };
                    in_flight_count -= 1;
                }
            }
            if n == 0 {
                break;
            }
        }

        result.elapsed_ns = measure_start.elapsed().as_nanos() as u64;
        Ok(result)
    }

    /// Issue a single asynchronous `ReadFile` or `WriteFile` request.
    fn issue_one(
        &self,
        slot: usize,
        rng: &mut Xoshiro256PlusPlus,
        seq_idx: &mut u64,
        num_blocks: u64,
        is_write: bool,
        in_flight: &mut Vec<InFlight>,
    ) -> Result<()> {
        let offset = if self.profile.is_random {
            (rng.next_u64() % num_blocks) * self.profile.block_size
        } else {
            let idx = *seq_idx % num_blocks;
            *seq_idx += 1;
            idx * self.profile.block_size
        };

        // SAFETY: Allocate and zero-initialize a new OVERLAPPED struct for this I/O.
        let mut ovl: Box<OVERLAPPED> = Box::new(unsafe { mem::zeroed() });
        ovl.Anonymous.Anonymous.Offset = offset as u32;
        ovl.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;

        let issued_qpc = qpc_now();
        let buf_ptr = self.buffers[slot].as_ptr();
        let buf_size = self.profile.block_size as u32;

        // Pinned heap pointer handed to kernel.
        let raw_ovl = Box::into_raw(ovl);

        let result = if is_write {
            // SAFETY: buf_ptr is valid for buf_size bytes; raw_ovl is pinned; file opened with FILE_FLAG_OVERLAPPED.
            unsafe {
                WriteFile(
                    self.file,
                    Some(std::slice::from_raw_parts(buf_ptr, buf_size as usize)),
                    None,
                    Some(raw_ovl),
                )
            }
        } else {
            // SAFETY: Same safety preconditions as WriteFile.
            unsafe {
                ReadFile(
                    self.file,
                    Some(std::slice::from_raw_parts_mut(buf_ptr, buf_size as usize)),
                    None,
                    Some(raw_ovl),
                )
            }
        };

        match result {
            Ok(_) => {}
            Err(ref e) if e.code() == windows::Win32::Foundation::ERROR_IO_PENDING.to_hresult() => {
                // Expected status for asynchronous I/O completion.
            }
            Err(e) => {
                // SAFETY: Kernel rejected the I/O; reclaim ownership immediately to prevent leak.
                let _ = unsafe { Box::from_raw(raw_ovl) };
                return Err(e).context("ReadFile/WriteFile submission failed");
            }
        }

        in_flight.push(InFlight {
            ovl: raw_ovl,
            buf_slot: slot,
            issued_qpc,
        });

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
