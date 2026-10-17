// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use anyhow::Result;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use windows::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};

/// A sector-aligned heap buffer allocated directly with Win32 `VirtualAlloc`.
///
/// # Safety invariants
/// - `ptr` is non-null, page-aligned (VirtualAlloc guarantees 64 KiB alignment),
///   which satisfies the 4096-byte alignment requirement for `FILE_FLAG_NO_BUFFERING`.
/// - `size` is a multiple of 4096 bytes.
/// - Exclusive access: each buffer slot is owned exclusively by its in-flight request.
pub struct AlignedBuffer {
    ptr: *mut u8,
    _size: usize,
}

// SAFETY: AlignedBuffer owns its allocation exclusively; no shared aliasing occurs.
unsafe impl Send for AlignedBuffer {}
unsafe impl Sync for AlignedBuffer {}

impl AlignedBuffer {
    /// Allocate `size` bytes via `VirtualAlloc` and initialize with non-zero random data.
    /// `size` must be a multiple of 4096.
    pub fn new_random(size: usize) -> Result<Self> {
        assert!(
            size.is_multiple_of(4096),
            "Buffer size must be sector-aligned (multiple of 4096)"
        );

        // SAFETY: VirtualAlloc with MEM_RESERVE | MEM_COMMIT returns a page-aligned pointer or null.
        let ptr = unsafe { VirtualAlloc(None, size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) };
        if ptr.is_null() {
            anyhow::bail!("VirtualAlloc failed: {}", std::io::Error::last_os_error());
        }

        // Fill buffer with pseudo-random data so SSD controllers cannot deduplicate or compress.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);

        // SAFETY: ptr is valid for `size` bytes, and u8 has alignment 1.
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, size) };
        rng.fill_bytes(slice);

        Ok(Self {
            ptr: ptr as *mut u8,
            _size: size,
        })
    }

    /// Returns the raw pointer to the start of the buffer.
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        // SAFETY: ptr was allocated by VirtualAlloc with MEM_RESERVE | MEM_COMMIT.
        // size = 0 with MEM_RELEASE releases the entire allocation.
        unsafe {
            let _ = VirtualFree(self.ptr as _, 0, MEM_RELEASE);
        }
    }
}
