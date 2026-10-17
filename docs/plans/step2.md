# Step 2 — Minimal I/O Engine (RND4K Q32T1 Read)

> **Goal:** Implement a working IOCP-based I/O engine that can run **one profile, one direction** (RND4K Q32T1 read). After this step, `cargo run -- --drives C --profile "RND4K Q32T1" --passes 1 --duration 5 --yes` must print real throughput numbers.

---

## New / modified files

| File | Status |
|---|---|
| `src/engine/mod.rs` | NEW |
| `src/engine/buffers.rs` | NEW — `AlignedBuffer` via `VirtualAlloc` |
| `src/engine/iocp.rs` | NEW — IOCP worker loop |
| `src/engine/profile.rs` | NEW — `Profile` type + 4 profile constants |
| `src/engine/stats.rs` | NEW — `PassResult`, throughput math, histogram helpers |
| `src/main.rs` | MODIFY — wire engine for the single profile |

---

## `src/engine/mod.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

pub mod buffers;
pub mod iocp;
pub mod profile;
pub mod stats;

pub use stats::PassResult;
```

---

## `src/engine/buffers.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use anyhow::{Context, Result};
use rand_core::{RngCore, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use windows::Win32::System::Memory::{VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};

/// A sector-aligned heap buffer allocated with `VirtualAlloc`.
///
/// # Safety invariants
/// - `ptr` is non-null, page-aligned (≥ 4096 bytes, actually 65536 by VirtualAlloc granularity).
/// - `size` is a multiple of 4096 (the logical sector size for FILE_FLAG_NO_BUFFERING).
/// - Exclusive access — never share across threads without external synchronisation.
pub struct AlignedBuffer {
    ptr:  *mut u8,
    pub size: usize,
}

// SAFETY: AlignedBuffer owns its allocation exclusively; no shared mutable state.
unsafe impl Send for AlignedBuffer {}
unsafe impl Sync for AlignedBuffer {}

impl AlignedBuffer {
    /// Allocate `size` bytes via VirtualAlloc and fill with random data.
    /// `size` must be a multiple of 4096.
    pub fn new_random(size: usize) -> Result<Self> {
        assert!(size % 4096 == 0, "buffer size must be sector-aligned");

        // SAFETY: VirtualAlloc with MEM_RESERVE | MEM_COMMIT returns a page-aligned pointer
        // or null. We check for null immediately.
        let ptr = unsafe {
            VirtualAlloc(None, size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE)
        };
        if ptr.is_null() {
            anyhow::bail!("VirtualAlloc failed: {}", std::io::Error::last_os_error());
        }

        // Fill with random data so we never benchmark compressible/zero patterns.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        // SAFETY: ptr is valid for `size` bytes; u8 alignment is always satisfied.
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, size) };
        rng.fill_bytes(slice);

        Ok(Self { ptr: ptr as *mut u8, size })
    }

    /// Raw pointer to the buffer start.
    pub fn as_ptr(&self) -> *mut u8 { self.ptr }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        // SAFETY: ptr was allocated by VirtualAlloc with MEM_RESERVE | MEM_COMMIT;
        // size = 0 with MEM_RELEASE releases the entire region.
        unsafe {
            let _ = VirtualFree(self.ptr as _, 0, MEM_RELEASE);
        }
    }
}
```

---

## `src/engine/profile.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

/// One benchmark profile (mirrors a CrystalDiskMark 9 test).
#[derive(Debug, Clone)]
pub struct Profile {
    pub name:        &'static str,
    pub block_size:  u64,   // bytes per I/O
    pub queue_depth: usize, // concurrent in-flight requests
    pub is_random:   bool,  // false = sequential
}

pub const SEQ1M_Q8T1:  Profile = Profile { name: "SEQ1M Q8T1",  block_size: 1 << 20, queue_depth: 8,  is_random: false };
pub const SEQ1M_Q1T1:  Profile = Profile { name: "SEQ1M Q1T1",  block_size: 1 << 20, queue_depth: 1,  is_random: false };
pub const RND4K_Q32T1: Profile = Profile { name: "RND4K Q32T1", block_size: 4096,    queue_depth: 32, is_random: true  };
pub const RND4K_Q1T1:  Profile = Profile { name: "RND4K Q1T1",  block_size: 4096,    queue_depth: 1,  is_random: true  };

pub const ALL_PROFILES: &[Profile] = &[SEQ1M_Q8T1, SEQ1M_Q1T1, RND4K_Q32T1, RND4K_Q1T1];

impl Profile {
    /// Equivalent DiskSpd command for `--validate` output.
    pub fn diskspd_cmd(&self, file: &str, is_write: bool) -> String {
        let w = if is_write { 100 } else { 0 };
        let r = if self.is_random { "-r" } else { "-si" };
        format!(
            "diskspd.exe -b{} -d5 -W1 -t1 -o{} {} -Sh -w{} -L {}",
            if self.block_size >= 1 << 20 { "1M".to_string() } else { format!("{}K", self.block_size / 1024) },
            self.queue_depth, r, w, file
        )
    }
}
```

---

## `src/engine/stats.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use hdrhistogram::Histogram;

/// Result from one benchmark pass (warmup already excluded by the engine).
#[derive(Debug)]
pub struct PassResult {
    /// Wall-clock nanoseconds from first post-warmup I/O to last completion.
    pub elapsed_ns: u64,
    /// Total bytes successfully transferred in the measurement window.
    pub bytes_xfer:  u64,
    /// I/O latency histogram (values in **microseconds**).
    pub hist: Histogram<u64>,
}

impl PassResult {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            elapsed_ns: 0,
            bytes_xfer:  0,
            hist: Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3)?,
        })
    }

    /// Throughput in MB/s (1 MB = 1,000,000 bytes, matching CrystalDiskMark convention).
    pub fn throughput_mb_s(&self) -> f64 {
        if self.elapsed_ns == 0 { return 0.0; }
        self.bytes_xfer as f64 / self.elapsed_ns as f64 * 1_000.0
    }

    /// IOPS.
    pub fn iops(&self, block_size: u64) -> f64 {
        if self.elapsed_ns == 0 || block_size == 0 { return 0.0; }
        (self.bytes_xfer as f64 / block_size as f64)
            / (self.elapsed_ns as f64 / 1_000_000_000.0)
    }

    pub fn avg_latency_us(&self) -> f64 { self.hist.mean() }
    pub fn p99_latency_us(&self) -> u64 { self.hist.value_at_quantile(0.99) }
}

/// Average a slice of PassResults into one representative result.
pub fn average_passes(results: &[PassResult]) -> anyhow::Result<PassResult> {
    let mut merged = PassResult::new()?;
    let n = results.len() as u64;
    if n == 0 { return Ok(merged); }

    for r in results {
        merged.elapsed_ns += r.elapsed_ns / n;
        merged.bytes_xfer  += r.bytes_xfer  / n;
        merged.hist.add(&r.hist)?;
    }
    Ok(merged)
}
```

---

## `src/engine/iocp.rs`

The most complex module. All `unsafe` blocks are individually documented.

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! IOCP-based asynchronous I/O engine.
//!
//! # OVERLAPPED lifetime contract
//! Each in-flight request owns a `Box<OVERLAPPED>` pinned on the heap.
//! Before issuing a `ReadFile`/`WriteFile`, we call `Box::into_raw` to yield
//! a raw pointer, which is handed to the kernel. The `Box` is reconstructed
//! only after `GetQueuedCompletionStatusEx` returns the pointer via
//! `OVERLAPPED_ENTRY.lpOverlapped`, at which point the kernel has released
//! ownership. This guarantees no double-free and no use-after-free.

use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use rand_core::{RngCore, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, ERROR_HANDLE_EOF};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED, OVERLAPPED_ENTRY,
};
use windows::Win32::System::SystemInformation::QueryPerformanceCounter;

use super::{buffers::AlignedBuffer, profile::Profile, stats::PassResult};

/// State for one in-flight I/O request.
struct InFlight {
    /// Heap-pinned OVERLAPPED. Raw pointer is owned by the kernel while in-flight.
    ovl:       *mut OVERLAPPED,
    buf_slot:  usize,
    issued_qpc: i64,  // QPC tick when the request was issued
}

pub struct IocpEngine {
    pub iocp:   HANDLE,
    pub file:   HANDLE,
    pub file_size: u64,
    pub profile: Profile,
    pub buffers: Vec<AlignedBuffer>,
}

impl IocpEngine {
    pub fn new(file: HANDLE, file_size: u64, profile: &Profile) -> Result<Self> {
        let qd = profile.queue_depth;

        // SAFETY: CreateIoCompletionPort creates a new IOCP (INVALID_HANDLE_VALUE as file).
        let iocp = unsafe {
            CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1)
                .context("CreateIoCompletionPort (new port)")?
        };

        // Associate the file handle with the IOCP.
        // SAFETY: file is a valid open handle; iocp is the newly created port.
        unsafe {
            CreateIoCompletionPort(file, Some(iocp), 1, 0)
                .context("CreateIoCompletionPort (associate file)")?;
        }

        // Allocate one buffer per queue slot.
        let bs = profile.block_size as usize;
        let mut buffers = Vec::with_capacity(qd);
        for _ in 0..qd {
            buffers.push(AlignedBuffer::new_random(bs)?);
        }

        Ok(Self { iocp, file, file_size, profile: profile.clone(), buffers })
    }

    /// Run one timed pass.
    /// `duration_secs`: total pass length (warmup + measurement).
    /// `warmup_secs`:   initial period whose I/Os are discarded.
    /// `is_write`:      true = WriteFile, false = ReadFile.
    /// `cancelled`:     shared flag from Ctrl+C handler.
    pub fn run_pass(
        &mut self,
        duration_secs: u64,
        warmup_secs:   u64,
        is_write:      bool,
        cancelled:     &AtomicBool,
    ) -> Result<PassResult> {
        let mut result = PassResult::new()?;
        let qd = self.profile.queue_depth;
        let bs = self.profile.block_size;
        let num_blocks = self.file_size / bs;

        let seed = (qpc_now() as u64) ^ 0x517CC1B727220A95;
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        let mut seq_idx: u64 = 0;

        let pass_start = Instant::now();
        let warmup_end = std::time::Duration::from_secs(warmup_secs);
        let pass_end   = std::time::Duration::from_secs(duration_secs);

        let mut in_flight: Vec<InFlight> = Vec::with_capacity(qd);
        let mut in_flight_count = 0usize;
        let mut measuring = false;
        let mut measure_start = Instant::now();

        // Issue initial batch of requests.
        for slot in 0..qd {
            self.issue_one(slot, &mut rng, &mut seq_idx, num_blocks, is_write, &mut in_flight)?;
            in_flight_count += 1;
        }

        let mut entries: [OVERLAPPED_ENTRY; 32] = unsafe { mem::zeroed() };

        loop {
            if cancelled.load(Ordering::Relaxed) { break; }

            let elapsed = pass_start.elapsed();
            if elapsed >= pass_end { break; }

            // Switch into measurement mode after warmup.
            if !measuring && elapsed >= warmup_end {
                measuring = true;
                measure_start = Instant::now();
            }

            let timeout_ms = {
                let remaining = pass_end.saturating_sub(elapsed);
                remaining.as_millis().min(500) as u32
            };

            let mut n_removed: u32 = 0;
            // SAFETY: entries is zeroed; iocp is a valid IOCP handle; n_removed is out param.
            let ok = unsafe {
                GetQueuedCompletionStatusEx(
                    self.iocp,
                    entries.as_mut_ptr(),
                    entries.len() as u32,
                    &mut n_removed,
                    timeout_ms,
                    false.into(),
                )
            };

            if !ok.as_bool() {
                // WAIT_TIMEOUT or transient poll expiration — loop again.
                continue;
            }

            let now_elapsed = pass_start.elapsed();
            let now_qpc = qpc_now();

            for i in 0..n_removed as usize {
                let entry = &entries[i];
                let bytes = entry.dwNumberOfBytesTransferred as u64;

                // Recover the in-flight record that owns this OVERLAPPED.
                // SAFETY: lpOverlapped was produced by Box::into_raw in issue_one;
                // the kernel has returned it, so we now own it again.
                let inf_idx = in_flight
                    .iter()
                    .position(|f| f.ovl == entry.lpOverlapped)
                    .expect("completion for unknown OVERLAPPED");
                let inf = in_flight.swap_remove(inf_idx);

                // Reconstruct Box to free the OVERLAPPED.
                // SAFETY: see module-level contract.
                let _ovl_box: Box<OVERLAPPED> = unsafe { Box::from_raw(inf.ovl) };
                in_flight_count -= 1;

                // Record latency if in measurement window.
                if measuring && now_elapsed < pass_end {
                    let latency_us = qpc_elapsed_us(inf.issued_qpc, now_qpc);
                    result.hist.saturating_record(latency_us);
                    result.bytes_xfer += bytes;
                }

                // Reissue if time remains.
                if now_elapsed < pass_end {
                    self.issue_one(
                        inf.buf_slot, &mut rng, &mut seq_idx, num_blocks,
                        is_write, &mut in_flight,
                    )?;
                    in_flight_count += 1;
                }
            }
        }

        // Drain remaining in-flight completions (best-effort, short timeout).
        while in_flight_count > 0 {
            let mut n: u32 = 0;
            unsafe {
                let _ = GetQueuedCompletionStatusEx(
                    self.iocp,
                    entries.as_mut_ptr(),
                    entries.len() as u32,
                    &mut n,
                    200,
                    false.into(),
                );
            }
            for i in 0..n as usize {
                let ovl_ptr = entries[i].lpOverlapped;
                if let Some(idx) = in_flight.iter().position(|f| f.ovl == ovl_ptr) {
                    let inf = in_flight.swap_remove(idx);
                    let _: Box<OVERLAPPED> = unsafe { Box::from_raw(inf.ovl) };
                    in_flight_count -= 1;
                }
            }
            if n == 0 { break; }
        }

        result.elapsed_ns = measure_start.elapsed().as_nanos() as u64;
        Ok(result)
    }

    fn issue_one(
        &self,
        slot:      usize,
        rng:       &mut Xoshiro256PlusPlus,
        seq_idx:   &mut u64,
        num_blocks: u64,
        is_write:  bool,
        in_flight: &mut Vec<InFlight>,
    ) -> Result<()> {
        let offset = if self.profile.is_random {
            (rng.next_u64() % num_blocks) * self.profile.block_size
        } else {
            let idx = *seq_idx % num_blocks;
            *seq_idx += 1;
            idx * self.profile.block_size
        };

        // Allocate and zero a new OVERLAPPED for this request.
        // SAFETY: OVERLAPPED is a C struct; zero-initialisation is valid.
        let mut ovl: Box<OVERLAPPED> = Box::new(unsafe { mem::zeroed() });
        ovl.Anonymous.Anonymous.Offset     = offset as u32;
        ovl.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;

        let issued_qpc = qpc_now();

        let buf_ptr  = self.buffers[slot].as_ptr();
        let buf_size = self.profile.block_size as u32;
        let raw_ovl  = Box::into_raw(ovl); // kernel now owns the OVERLAPPED

        let result = if is_write {
            // SAFETY: buf_ptr is valid for buf_size bytes; raw_ovl is heap-pinned;
            // file is open with FILE_FLAG_OVERLAPPED.
            unsafe {
                WriteFile(self.file, Some(std::slice::from_raw_parts(buf_ptr, buf_size as usize)), None, Some(raw_ovl))
            }
        } else {
            // SAFETY: same as above.
            unsafe {
                ReadFile(self.file, Some(std::slice::from_raw_parts_mut(buf_ptr, buf_size as usize)), None, Some(raw_ovl))
            }
        };

        // For overlapped I/O, ERROR_IO_PENDING is the success path.
        match result {
            Ok(_) | Err(ref e) if e.code() == windows::Win32::Foundation::ERROR_IO_PENDING.to_hresult() => {}
            Err(e) => {
                // SAFETY: we are taking back ownership because the I/O was not accepted.
                let _ = unsafe { Box::from_raw(raw_ovl) };
                return Err(e).context("ReadFile/WriteFile (overlapped issue)");
            }
        }

        in_flight.push(InFlight { ovl: raw_ovl, buf_slot: slot, issued_qpc });
        Ok(())
    }
}

fn qpc_now() -> i64 {
    let mut t = 0i64;
    // SAFETY: QueryPerformanceCounter always succeeds on modern Windows.
    unsafe { let _ = QueryPerformanceCounter(&mut t); }
    t
}

fn qpc_elapsed_us(start: i64, end: i64) -> u64 {
    static FREQ: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let freq = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        unsafe { let _ = windows::Win32::System::SystemInformation::QueryPerformanceFrequency(&mut f); }
        f
    });
    if freq <= 0 { return 0; }
    let diff = (end - start).max(0) as u128;
    ((diff * 1_000_000) / freq as u128) as u64
}
```

---

## `src/main.rs` (updated)

Adds wiring to call `IocpEngine` for `RND4K Q32T1` read on the specified drive letter.

Key additions:
- Open test file with `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH | FILE_FLAG_OVERLAPPED`
- Write file sequentially before reading (so NTFS allocates all extents)
- Call `iocp::IocpEngine::run_pass()` 
- Print `PassResult` throughput table
- Drop guard deletes test file on exit

---

## Verification

```powershell
# Must run in an elevated PowerShell
cargo build 2>&1
# Expect: Compiling dspd v0.1.0 … Finished dev [optimized] target(s)

cargo test
# Expect: test result: ok. N passed; 0 failed

cargo run -- --drives C --profile "RND4K Q32T1" --passes 1 --duration 5 --yes
```

### Expected output (NVMe example)
```
dspd v0.1.0 by itznan (github.com/itznan/dspd)
[WARNING] Write tests will stress flash cells. Skipping write for this profile.
[  RND4K Q32T1  ]  READ
  Pass 1 ████████████████ 5.1s  done
  MB/s : 1842.3
  IOPS : 449,780
  Avg lat: 71 µs  p99: 380 µs
```

### Cross-check with DiskSpd
```powershell
diskspd.exe -b4K -d5 -W1 -t1 -o32 -r -Sh -w0 -L C:\dspd_test.dat
```
Expect dspd result within **±15%** of DiskSpd on the same file.
