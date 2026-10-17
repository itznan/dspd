// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! High-resolution Windows QueryPerformanceCounter (QPC) timing utilities.

use std::sync::OnceLock;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

/// Retrieve the current QueryPerformanceCounter tick count.
#[inline]
pub fn qpc_now() -> i64 {
    let mut t = 0i64;
    // SAFETY: QueryPerformanceCounter succeeds on all modern Windows installations.
    unsafe {
        let _ = QueryPerformanceCounter(&mut t);
    }
    t
}

/// Calculate elapsed microseconds between two QPC tick counts using 128-bit arithmetic to prevent overflow.
#[inline]
pub fn qpc_elapsed_us(start: i64, end: i64) -> u64 {
    static FREQ: OnceLock<i64> = OnceLock::new();
    let freq = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut f);
        }
        f
    });
    if freq <= 0 {
        return 0;
    }
    let diff = (end - start).max(0) as u128;
    ((diff * 1_000_000) / freq as u128) as u64
}
