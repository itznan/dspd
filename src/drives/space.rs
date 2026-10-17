// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Disk free space and volume handle management using Win32 API.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

/// RAII Guard ensuring open volume handles are properly closed when dropped.
pub struct VolumeHandleGuard(pub HANDLE);

impl Drop for VolumeHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Retrieve free and total capacity in bytes for a given drive letter.
pub fn get_disk_space(letter: char) -> windows::core::Result<(u64, u64)> {
    let root = format!("{letter}:\\\0");
    let root_w: Vec<u16> = root.encode_utf16().collect();
    let mut free_bytes = 0u64;
    let mut total_bytes = 0u64;

    // SAFETY: GetDiskFreeSpaceExW is called with valid pointers to u64 variables.
    unsafe {
        GetDiskFreeSpaceExW(
            windows::core::PCWSTR(root_w.as_ptr()),
            Some(&mut free_bytes),
            Some(&mut total_bytes),
            None,
        )?;
    }

    Ok((free_bytes, total_bytes))
}
