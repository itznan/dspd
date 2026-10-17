// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Physical drive discovery and storage properties querying using Win32 IOCTL.

use anyhow::Result;
use thiserror::Error;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, FILE_ATTRIBUTE_NORMAL,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
    DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::IO::DeviceIoControl;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusType {
    NVMe,
    SATA,
    USB,
    Unknown(u32),
}

impl std::fmt::Display for BusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BusType::NVMe => write!(f, "NVMe"),
            BusType::SATA => write!(f, "SATA"),
            BusType::USB => write!(f, "USB"),
            BusType::Unknown(code) => write!(f, "Unknown ({})", code),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    SSD,
    HDD,
    Unknown,
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaType::SSD => write!(f, "SSD"),
            MediaType::HDD => write!(f, "HDD"),
            MediaType::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub letter: char,
    pub model: String,
    pub bus_type: BusType,
    pub media_type: MediaType,
    pub capacity: u64,
    pub free: u64,
}

#[derive(Debug, Error)]
pub enum DriveError {
    #[error("Drive {0}: insufficient free disk space ({1:.1} GiB free, need {2:.1} GiB)")]
    InsufficientSpace(char, f64, f64),
    #[error("Drive {0}: cannot open volume handle ({1})")]
    OpenFailed(char, windows::core::Error),
    #[error("Drive {0}: IOCTL query failed ({1})")]
    IoctlFailed(char, windows::core::Error),
}

struct VolumeHandleGuard(HANDLE);

impl Drop for VolumeHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}

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

fn query_device_property(letter: char, handle: HANDLE) -> Result<(String, BusType), DriveError> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0; 1],
    };

    let mut buf = vec![0u8; 4096];
    let mut bytes_returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            buf.len() as u32,
            Some(&mut bytes_returned),
            None,
        )
        .map_err(|e| DriveError::IoctlFailed(letter, e))?;
    }

    let desc: &STORAGE_DEVICE_DESCRIPTOR =
        unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };

    let model = if desc.ProductIdOffset != 0 && (desc.ProductIdOffset as usize) < buf.len() {
        let start = desc.ProductIdOffset as usize;
        let end = buf[start..]
            .iter()
            .position(|&b| b == 0)
            .map_or(buf.len(), |n| start + n);
        let s = String::from_utf8_lossy(&buf[start..end]).trim().to_string();
        if s.is_empty() {
            "Generic Drive".to_string()
        } else {
            s
        }
    } else {
        "Generic Storage Device".to_string()
    };

    // BusType mapping according to STORAGE_BUS_TYPE Win32 definition:
    // BusTypeNvme = 0x11 (17), BusTypeNvmeof = 0x14, BusTypeSata = 0x0B (11), BusTypeAta = 0x03, BusTypeSas = 0x0A, BusTypeUsb = 0x07
    let bus_type = match desc.BusType.0 {
        0x11 | 0x14 => BusType::NVMe,
        0x0B | 0x0A | 0x03 | 0x01 => BusType::SATA,
        0x07 => BusType::USB,
        n => BusType::Unknown(n as u32),
    };

    Ok((model, bus_type))
}

fn query_seek_penalty(letter: char, handle: HANDLE) -> Result<MediaType, DriveError> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0; 1],
    };

    let mut desc: DEVICE_SEEK_PENALTY_DESCRIPTOR = unsafe { std::mem::zeroed() };
    let mut bytes_returned = 0u32;

    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut desc as *mut _ as *mut _),
            std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut bytes_returned),
            None,
        )
        .map_err(|e| DriveError::IoctlFailed(letter, e))?;
    }

    Ok(if desc.IncursSeekPenalty.as_bool() {
        MediaType::HDD
    } else {
        MediaType::SSD
    })
}

fn get_disk_space(letter: char) -> windows::core::Result<(u64, u64)> {
    let root = format!("{letter}:\\\0");
    let root_w: Vec<u16> = root.encode_utf16().collect();
    let mut free_bytes = 0u64;
    let mut total_bytes = 0u64;

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
