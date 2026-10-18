// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! High-performance Windows 11 `IoRing` storage engine using dynamic runtime symbol resolution.
//!
//! # Windows 10 Backward Compatibility
//! `kernelbase.dll` symbols are resolved dynamically via `GetModuleHandleW` and `GetProcAddress`.
//! If running on Windows 10 or an OS build where `IoRing` is unavailable, `IoRingEngine::is_available()`
//! returns `false`, allowing the benchmark runner to gracefully fall back to `IocpEngine`.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use windows::core::{s, w, HRESULT};
use windows::Win32::Foundation::{HANDLE, S_OK};
use windows::Win32::Storage::FileSystem::FlushFileBuffers;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};

use super::buffers::AlignedBuffer;
use super::cpu::{CpuMetrics, CpuSnapshot};
use super::iocp::IoOp;
use super::profile::{Profile, RwMix, XorShift64Star};
use super::qpc::{qpc_elapsed_us, qpc_now};
use super::stats::PassResult;

// ── Raw Win32 IoRing Structs (ioringapi.h & ntioring_x.h) ────────────────────

pub const IORING_VERSION_1: u32 = 1;
pub const IORING_VERSION_2: u32 = 2;
pub const IORING_VERSION_3: u32 = 300;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IORING_CREATE_FLAGS {
    pub Required: u32,
    pub Advisory: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IORING_CAPABILITIES {
    pub MaxVersion: u32,
    pub MaxSubmissionQueueSize: u32,
    pub MaxCompletionQueueSize: u32,
    pub FeatureFlags: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct IORING_HANDLE_REF {
    pub Kind: u32,
    _pad: u32,
    pub Handle: usize,
}

impl IORING_HANDLE_REF {
    #[inline]
    pub fn from_handle(h: HANDLE) -> Self {
        Self {
            Kind: 0, // IORING_REF_RAW
            _pad: 0,
            Handle: h.0 as usize,
        }
    }

    #[inline]
    pub fn from_index(index: u32) -> Self {
        Self {
            Kind: 1, // IORING_REF_REGISTERED
            _pad: 0,
            Handle: index as usize,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct IORING_BUFFER_REF {
    pub Kind: u32,
    _pad: u32,
    pub Buffer: usize,
}

impl IORING_BUFFER_REF {
    #[inline]
    pub fn from_pointer(ptr: *mut u8) -> Self {
        Self {
            Kind: 0, // IORING_REF_RAW
            _pad: 0,
            Buffer: ptr as usize,
        }
    }

    #[inline]
    pub fn from_index_and_offset(index: u32, offset: u32) -> Self {
        Self {
            Kind: 1, // IORING_REF_REGISTERED
            _pad: 0,
            Buffer: (index as usize) | ((offset as usize) << 32),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct IORING_BUFFER_INFO {
    pub Address: *mut c_void,
    pub Length: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct IORING_CQE {
    pub UserData: usize,
    pub ResultCode: HRESULT,
    _pad: u32,
    pub Information: usize,
}

// ── Dynamic Win32 FFI Function Signatures ────────────────────────────────────

#[allow(non_snake_case)]
type PfnQueryIoRingCapabilities =
    unsafe extern "system" fn(capabilities: *mut IORING_CAPABILITIES) -> HRESULT;

#[allow(non_snake_case)]
type PfnCreateIoRing = unsafe extern "system" fn(
    ioringVersion: u32,
    flags: IORING_CREATE_FLAGS,
    submissionQueueSize: u32,
    completionQueueSize: u32,
    hIoRing: *mut *mut c_void,
) -> HRESULT;

#[allow(non_snake_case)]
type PfnCloseIoRing = unsafe extern "system" fn(hIoRing: *mut c_void) -> HRESULT;

#[allow(non_snake_case)]
type PfnSubmitIoRing = unsafe extern "system" fn(
    hIoRing: *mut c_void,
    waitOperations: u32,
    milliseconds: u32,
    submittedEntries: *mut u32,
) -> HRESULT;

#[allow(non_snake_case)]
type PfnPopIoRingCompletion =
    unsafe extern "system" fn(hIoRing: *mut c_void, cqe: *mut IORING_CQE) -> HRESULT;

#[allow(non_snake_case)]
type PfnBuildIoRingReadFile = unsafe extern "system" fn(
    hIoRing: *mut c_void,
    fileRef: IORING_HANDLE_REF,
    dataRef: IORING_BUFFER_REF,
    numberOfBytesToRead: u32,
    fileOffset: u64,
    userData: usize,
    sqeFlags: u32,
) -> HRESULT;

#[allow(non_snake_case)]
type PfnBuildIoRingWriteFile = unsafe extern "system" fn(
    hIoRing: *mut c_void,
    fileRef: IORING_HANDLE_REF,
    dataRef: IORING_BUFFER_REF,
    numberOfBytesToWrite: u32,
    fileOffset: u64,
    writeFlags: u32,
    userData: usize,
    sqeFlags: u32,
) -> HRESULT;

#[allow(non_snake_case)]
type PfnBuildIoRingRegisterFileHandles = unsafe extern "system" fn(
    hIoRing: *mut c_void,
    count: u32,
    handles: *const HANDLE,
    userData: usize,
) -> HRESULT;

#[allow(non_snake_case)]
type PfnBuildIoRingRegisterBuffers = unsafe extern "system" fn(
    hIoRing: *mut c_void,
    count: u32,
    buffers: *const IORING_BUFFER_INFO,
    userData: usize,
) -> HRESULT;

#[derive(Clone, Copy)]
pub struct IoRingApi {
    pub query_capabilities: Option<PfnQueryIoRingCapabilities>,
    pub create_io_ring: PfnCreateIoRing,
    pub close_io_ring: PfnCloseIoRing,
    pub submit_io_ring: PfnSubmitIoRing,
    pub pop_io_ring_completion: PfnPopIoRingCompletion,
    pub build_read_file: PfnBuildIoRingReadFile,
    pub build_write_file: Option<PfnBuildIoRingWriteFile>,
    pub build_register_file_handles: Option<PfnBuildIoRingRegisterFileHandles>,
    pub build_register_buffers: Option<PfnBuildIoRingRegisterBuffers>,
}

impl IoRingApi {
    /// Attempt to dynamically resolve IoRing symbols from `kernelbase.dll` or `kernel32.dll`.
    #[allow(clippy::missing_transmute_annotations)]
    pub fn load() -> Option<Self> {
        unsafe {
            let mut h_mod = GetModuleHandleW(w!("kernelbase.dll")).unwrap_or_default();
            if h_mod.is_invalid() {
                if let Ok(loaded) = LoadLibraryW(w!("kernelbase.dll")) {
                    h_mod = loaded;
                }
            }
            if h_mod.is_invalid() {
                return None;
            }

            let p_create = GetProcAddress(h_mod, s!("CreateIoRing"))?;
            let p_close = GetProcAddress(h_mod, s!("CloseIoRing"))?;
            let p_submit = GetProcAddress(h_mod, s!("SubmitIoRing"))?;
            let p_pop = GetProcAddress(h_mod, s!("PopIoRingCompletion"))?;
            let p_read = GetProcAddress(h_mod, s!("BuildIoRingReadFile"))?;

            let p_caps = GetProcAddress(h_mod, s!("QueryIoRingCapabilities"));
            let p_write = GetProcAddress(h_mod, s!("BuildIoRingWriteFile"));
            let p_reg_files = GetProcAddress(h_mod, s!("BuildIoRingRegisterFileHandles"));
            let p_reg_bufs = GetProcAddress(h_mod, s!("BuildIoRingRegisterBuffers"));

            Some(Self {
                query_capabilities: p_caps
                    .map(|p| std::mem::transmute::<_, PfnQueryIoRingCapabilities>(p)),
                create_io_ring: std::mem::transmute::<_, PfnCreateIoRing>(p_create),
                close_io_ring: std::mem::transmute::<_, PfnCloseIoRing>(p_close),
                submit_io_ring: std::mem::transmute::<_, PfnSubmitIoRing>(p_submit),
                pop_io_ring_completion: std::mem::transmute::<_, PfnPopIoRingCompletion>(p_pop),
                build_read_file: std::mem::transmute::<_, PfnBuildIoRingReadFile>(p_read),
                build_write_file: p_write
                    .map(|p| std::mem::transmute::<_, PfnBuildIoRingWriteFile>(p)),
                build_register_file_handles: p_reg_files
                    .map(|p| std::mem::transmute::<_, PfnBuildIoRingRegisterFileHandles>(p)),
                build_register_buffers: p_reg_bufs
                    .map(|p| std::mem::transmute::<_, PfnBuildIoRingRegisterBuffers>(p)),
            })
        }
    }
}

// ── In-Flight Slot Tracking ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
struct IoRingSlot {
    issued_qpc: i64,
    op: IoOp,
    in_flight: bool,
}

// ── IoRing Storage Engine ────────────────────────────────────────────────────

pub struct IoRingEngine {
    api: IoRingApi,
    ioring: *mut c_void,
    file: HANDLE,
    file_size: u64,
    profile: Profile,
    buffers: Vec<AlignedBuffer>,
    slots: Vec<IoRingSlot>,
    is_registered: bool,
}

unsafe impl Send for IoRingEngine {}
unsafe impl Sync for IoRingEngine {}

impl IoRingEngine {
    /// Check whether Windows 11 IoRing APIs are dynamically available on the current OS.
    pub fn is_available() -> bool {
        IoRingApi::load().is_some()
    }

    /// Initialize a new IoRing storage engine for the specified file handle and profile.
    pub fn new(file: HANDLE, file_size: u64, profile: &Profile) -> Result<Self> {
        let api = IoRingApi::load()
            .context("IoRing is not supported on this Windows build (requires Windows 11)")?;

        let qd = profile.queue_depth;
        let sq_size = (qd as u32).clamp(16, 2048);
        let cq_size = sq_size * 2;

        let mut version = IORING_VERSION_1;
        if let Some(query_caps) = api.query_capabilities {
            let mut caps = IORING_CAPABILITIES::default();
            if unsafe { (query_caps)(&mut caps) }.is_ok() && caps.MaxVersion >= IORING_VERSION_3 {
                version = IORING_VERSION_3;
            }
        }

        let flags = IORING_CREATE_FLAGS::default();
        let mut ioring: *mut c_void = std::ptr::null_mut();

        // SAFETY: Calling CreateIoRing with requested queue sizes.
        let hr = unsafe { (api.create_io_ring)(version, flags, sq_size, cq_size, &mut ioring) };
        if hr.is_err() {
            // Fall back to Version 1 if Version 3 failed
            let hr_v1 = unsafe {
                (api.create_io_ring)(IORING_VERSION_1, flags, sq_size, cq_size, &mut ioring)
            };
            if hr_v1.is_err() {
                anyhow::bail!("CreateIoRing failed: {:?}", hr);
            }
        }

        let max_bs = profile.block_size.max(profile.block_size_alt.unwrap_or(0)) as usize;
        let mut buffers = Vec::with_capacity(qd);
        for _ in 0..qd {
            buffers.push(AlignedBuffer::new_random(max_bs)?);
        }

        let mut engine = Self {
            api,
            ioring,
            file,
            file_size,
            profile: profile.clone(),
            buffers,
            slots: vec![IoRingSlot::default(); qd],
            is_registered: false,
        };

        // Attempt pre-registration of file handles and buffers on Windows 11 Version 3+
        if version >= IORING_VERSION_3 {
            engine.try_register_handles_and_buffers(max_bs);
            if !engine.is_registered {
                // Recreate clean ring without registration so submission queue is empty
                unsafe {
                    let _ = (api.close_io_ring)(engine.ioring);
                    engine.ioring = std::ptr::null_mut();
                    let _ =
                        (api.create_io_ring)(version, flags, sq_size, cq_size, &mut engine.ioring);
                }
            }
        }

        Ok(engine)
    }

    /// Try to pre-register the file handle and buffer pool with the IoRing.
    fn try_register_handles_and_buffers(&mut self, max_bs: usize) {
        let reg_files = match self.api.build_register_file_handles {
            Some(f) => f,
            None => return,
        };
        let reg_bufs = match self.api.build_register_buffers {
            Some(f) => f,
            None => return,
        };

        let handles = [self.file];
        let buffer_infos: Vec<IORING_BUFFER_INFO> = self
            .buffers
            .iter()
            .map(|b| IORING_BUFFER_INFO {
                Address: b.as_ptr() as *mut c_void,
                Length: max_bs as u32,
                _pad: 0,
            })
            .collect();

        unsafe {
            let hr_files = (reg_files)(self.ioring, 1, handles.as_ptr(), 0x1001);
            if hr_files.is_err() {
                return;
            }
            let hr_bufs = (reg_bufs)(
                self.ioring,
                buffer_infos.len() as u32,
                buffer_infos.as_ptr(),
                0x1002,
            );
            if hr_bufs.is_err() {
                return;
            }

            let mut submitted = 0u32;
            let hr_sub = (self.api.submit_io_ring)(self.ioring, 2, 2000, &mut submitted);
            if hr_sub.is_err() || submitted < 2 {
                return;
            }

            let mut cqe1 = IORING_CQE::default();
            let mut cqe2 = IORING_CQE::default();
            let hr_pop1 = (self.api.pop_io_ring_completion)(self.ioring, &mut cqe1);
            let hr_pop2 = (self.api.pop_io_ring_completion)(self.ioring, &mut cqe2);

            if hr_pop1 == S_OK
                && hr_pop2 == S_OK
                && cqe1.ResultCode.is_ok()
                && cqe2.ResultCode.is_ok()
            {
                self.is_registered = true;
            }
        }
    }

    /// Run one timed benchmark pass using the IoRing backend.
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

        let seed = (qpc_now() as u64) ^ 0x3A5C9B1E4D2F7A8B;
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
        let mut cpu_before = CpuSnapshot::default();

        // 1. Prime the submission queue
        for slot in 0..qd {
            self.build_io_slot(
                slot,
                &mut rng,
                &mut prng,
                &mut seq_idx,
                is_write,
                active_mix,
            )?;
            in_flight_count += 1;
        }

        // Submit initial batch (all pending entries without waiting)
        let mut submitted = 0u32;
        unsafe {
            let _ = (self.api.submit_io_ring)(self.ioring, 0, 0, &mut submitted);
        }

        // 2. Harvesting and submission pump
        let mut cqe = IORING_CQE::default();

        loop {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }

            let elapsed = pass_start.elapsed();
            if elapsed >= pass_end {
                break;
            }

            // Transition from warmup to active measurement window
            if !measuring && elapsed >= warmup_end {
                measuring = true;
                measure_start = Instant::now();
                cpu_before = CpuSnapshot::take();
            }

            let now_elapsed = pass_start.elapsed();
            let now_qpc = qpc_now();

            // Reap completed entries (PopIoRingCompletion returns S_OK if an entry was popped, S_FALSE if empty)
            let hr = unsafe { (self.api.pop_io_ring_completion)(self.ioring, &mut cqe) };

            if hr == S_OK && cqe.ResultCode.is_ok() {
                let slot = cqe.UserData;
                if slot < qd && self.slots[slot].in_flight {
                    self.slots[slot].in_flight = false;
                    in_flight_count -= 1;

                    let bytes = cqe.Information as u64;
                    let latency_us = qpc_elapsed_us(self.slots[slot].issued_qpc, now_qpc);

                    if measuring && now_elapsed < pass_end {
                        match self.slots[slot].op {
                            IoOp::Read => result.record_read(bytes, latency_us),
                            IoOp::Write => {
                                result.record_write(bytes, latency_us);
                                if let Some(flush_interval) = self.profile.flush_interval_ios {
                                    write_counter += 1;
                                    if write_counter >= flush_interval {
                                        write_counter = 0;
                                        let flush_start = qpc_now();
                                        let _ = unsafe { FlushFileBuffers(self.file) };
                                        let flush_us = qpc_elapsed_us(flush_start, qpc_now());
                                        result.record_flush(flush_us);
                                    }
                                }
                            }
                        }
                    }

                    // Reissue slot if pass has not completed
                    if now_elapsed < pass_end && !cancelled.load(Ordering::Relaxed) {
                        self.build_io_slot(
                            slot,
                            &mut rng,
                            &mut prng,
                            &mut seq_idx,
                            is_write,
                            active_mix,
                        )?;
                        in_flight_count += 1;

                        let mut sub = 0u32;
                        unsafe {
                            let _ = (self.api.submit_io_ring)(self.ioring, 0, 0, &mut sub);
                        }
                    }
                }
            } else {
                // Completion queue was empty (or transient error); wait up to 10ms for at least 1 completion
                let remaining = pass_end.saturating_sub(elapsed);
                let timeout_ms = (remaining.as_millis() as u32).clamp(1, 10);
                let mut sub = 0u32;
                unsafe {
                    let _ = (self.api.submit_io_ring)(self.ioring, 1, timeout_ms, &mut sub);
                }
            }
        }

        // 3. Drain in-flight completions
        while in_flight_count > 0 {
            let mut sub = 0u32;
            unsafe {
                let _ = (self.api.submit_io_ring)(self.ioring, 1, 100, &mut sub);
            }
            let hr = unsafe { (self.api.pop_io_ring_completion)(self.ioring, &mut cqe) };
            if hr == S_OK {
                let slot = cqe.UserData;
                if slot < qd && self.slots[slot].in_flight {
                    self.slots[slot].in_flight = false;
                    in_flight_count -= 1;
                }
            } else {
                break;
            }
        }

        if measuring {
            let cpu_after = CpuSnapshot::take();
            result.elapsed_ns = measure_start.elapsed().as_nanos() as u64;
            let total_ios = result.read_ios + result.write_ios;
            result.cpu = Some(CpuMetrics::compute(
                &cpu_before,
                &cpu_after,
                total_ios,
                result.elapsed_ns,
            ));
        }

        Ok(result)
    }

    /// Build a single asynchronous Read or Write SQE into the IoRing submission queue.
    fn build_io_slot(
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

        self.slots[slot].op = op;
        self.slots[slot].issued_qpc = qpc_now();
        self.slots[slot].in_flight = true;

        let file_ref = if self.is_registered {
            IORING_HANDLE_REF::from_index(0)
        } else {
            IORING_HANDLE_REF::from_handle(self.file)
        };

        let data_ref = if self.is_registered {
            IORING_BUFFER_REF::from_index_and_offset(slot as u32, 0)
        } else {
            IORING_BUFFER_REF::from_pointer(self.buffers[slot].as_ptr())
        };

        let hr = match op {
            IoOp::Read => unsafe {
                (self.api.build_read_file)(
                    self.ioring,
                    file_ref,
                    data_ref,
                    cur_bs as u32,
                    offset,
                    slot,
                    0,
                )
            },
            IoOp::Write => {
                let build_write = self
                    .api
                    .build_write_file
                    .context("IoRing write operations are not supported by this Windows version")?;
                unsafe {
                    (build_write)(
                        self.ioring,
                        file_ref,
                        data_ref,
                        cur_bs as u32,
                        offset,
                        0, // writeFlags
                        slot,
                        0, // sqeFlags
                    )
                }
            }
        };

        if hr.is_err() {
            self.slots[slot].in_flight = false;
            anyhow::bail!("Failed to build IoRing SQE: {:?}", hr);
        }

        Ok(())
    }
}

impl Drop for IoRingEngine {
    fn drop(&mut self) {
        if !self.ioring.is_null() {
            unsafe {
                let _ = (self.api.close_io_ring)(self.ioring);
            }
            self.ioring = std::ptr::null_mut();
        }
    }
}
