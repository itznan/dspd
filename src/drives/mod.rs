// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Physical drive discovery and storage properties querying using Win32 API and IOCTL.

pub mod query;
pub mod space;
pub mod types;

pub use space::get_disk_space;
pub use types::{BusType, DriveError, DriveInfo, MediaType};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, GetLogicalDrives, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};

use query::{query_device_property, query_seek_penalty};
use space::VolumeHandleGuard;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

/// Discover eligible local drives (fixed by default, plus removable if requested).
pub fn discover(include_removable: bool) -> Vec<DriveInfo> {
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();

    for bit in 0..26u32 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let letter = char::from(b'A' + bit as u8);
        let root = format!("{letter}:\\\0");
        let root_w: Vec<u16> = root.encode_utf16().collect();

        // SAFETY: GetDriveTypeW is called with a null-terminated UTF-16 path.
        let drive_type = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_w.as_ptr())) };
        let is_fixed = drive_type == DRIVE_FIXED;
        let is_removable = drive_type == DRIVE_REMOVABLE;

        if !is_fixed && !(include_removable && is_removable) {
            continue;
        }

        match query_drive(letter) {
            Ok(info) => drives.push(info),
            Err(e) => {
                // If opening physical device handle fails (e.g. non-admin or restricted), fallback to capacity only
                let (free, total) = get_disk_space(letter).unwrap_or((0, 0));
                if total > 0 {
                    drives.push(DriveInfo {
                        letter,
                        model: "Generic Storage Device".to_string(),
                        bus_type: if is_removable {
                            BusType::USB
                        } else {
                            BusType::SATA
                        },
                        media_type: MediaType::Unknown,
                        capacity: total,
                        free,
                    });
                } else {
                    eprintln!("  [WARN] Skipping drive {letter}:: {e}");
                }
            }
        }
    }
    drives
}

/// Query storage geometry and device property descriptors for a specific drive letter.
pub fn query_drive(letter: char) -> Result<DriveInfo, DriveError> {
    let path = format!(r"\\.\{letter}:");
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: Open handle to the drive volume for metadata queries.
    let handle: HANDLE = unsafe {
        CreateFileW(
            windows::core::PCWSTR(path_w.as_ptr()),
            0, // No read/write access required for property queries
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|e| DriveError::OpenFailed(letter, e))?
    };
    let _guard = VolumeHandleGuard(handle);

    // Query Device Property (Model + Bus Type)
    let (model, bus_type) = query_device_property(letter, handle)?;

    // Query Seek Penalty (SSD vs HDD)
    let media_type = query_seek_penalty(letter, handle).unwrap_or(MediaType::Unknown);

    // Query Total and Free Capacity
    let (free, total) = get_disk_space(letter).map_err(|e| DriveError::IoctlFailed(letter, e))?;

    Ok(DriveInfo {
        letter,
        model,
        bus_type,
        media_type,
        capacity: total,
        free,
    })
}
