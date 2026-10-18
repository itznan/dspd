// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Low-level Win32 IOCTL queries for storage device properties and seek penalty.

use std::mem;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
    DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::IO::DeviceIoControl;

use super::types::{BusType, DriveError, MediaType};

/// Query model string and bus interface type (NVMe, SATA, USB) for an open volume handle.
pub fn query_device_property(
    letter: char,
    handle: HANDLE,
) -> Result<(String, BusType), DriveError> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0; 1],
    };

    let mut buf = vec![0u8; 4096];
    let mut bytes_returned = 0u32;

    // SAFETY: Issue IOCTL_STORAGE_QUERY_PROPERTY to read STORAGE_DEVICE_DESCRIPTOR.
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
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

/// Query seek penalty property to determine if the drive is solid-state (SSD) or rotational (HDD).
pub fn query_seek_penalty(letter: char, handle: HANDLE) -> Result<MediaType, DriveError> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0; 1],
    };

    let mut desc: DEVICE_SEEK_PENALTY_DESCRIPTOR = unsafe { mem::zeroed() };
    let mut bytes_returned = 0u32;

    // SAFETY: Issue IOCTL_STORAGE_QUERY_PROPERTY for StorageDeviceSeekPenaltyProperty.
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut desc as *mut _ as *mut _),
            mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut bytes_returned),
            None,
        )
        .map_err(|e| DriveError::IoctlFailed(letter, e))?;
    }

    Ok(if desc.IncursSeekPenalty {
        MediaType::HDD
    } else {
        MediaType::SSD
    })
}
