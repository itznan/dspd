// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Console signal handling (Ctrl+C, break) and cancellation flags.

use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::System::Console::SetConsoleCtrlHandler;

pub static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Check whether cancellation has been requested.
#[inline]
pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

/// Request cancellation of running benchmarks.
#[inline]
pub fn set_cancelled() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// Win32 Console control event callback handler.
unsafe extern "system" fn ctrl_handler(_: u32) -> windows::Win32::Foundation::BOOL {
    set_cancelled();
    eprintln!("\n[!] Ctrl+C detected. Cleaning up benchmark files safely...");
    windows::Win32::Foundation::TRUE
}

/// Register Win32 console signal handler.
pub fn register_ctrl_handler() {
    // SAFETY: SetConsoleCtrlHandler registers a valid system callback.
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), windows::Win32::Foundation::TRUE);
    }
}
