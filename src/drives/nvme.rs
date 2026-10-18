// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! NVMe protocol queries and SMART / Health telemetry polling via Win32 IOCTL.

use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, SYNCHRONIZE,
};
use windows::Win32::System::Ioctl::IOCTL_STORAGE_QUERY_PROPERTY;
use windows::Win32::System::IO::DeviceIoControl;

use super::space::VolumeHandleGuard;
use super::types::DriveError;

/// Storage property query identifiers
pub const STORAGE_ADAPTER_PROTOCOL_SPECIFIC_PROPERTY: u32 = 49;
pub const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY: u32 = 50;

/// Standard query type
pub const PROPERTY_STANDARD_QUERY: u32 = 0;

/// Storage protocol types
pub const PROTOCOL_TYPE_NVME: u32 = 0x03;

/// NVMe protocol data types
pub const NVME_DATA_TYPE_LOG_PAGE: u32 = 0x02;

/// NVMe Log Page IDs
pub const NVME_LOG_PAGE_SMART: u32 = 0x02;

/// Standard NVMe SMART / Health log size in bytes
pub const NVME_SMART_LOG_SIZE: usize = 512;

/// Temperature threshold above which thermal throttling warning is triggered (°C).
pub const THERMAL_THROTTLE_TEMP_THRESHOLD_C: i32 = 70;

/// Raw Win32 STORAGE_PROTOCOL_SPECIFIC_DATA structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageProtocolSpecificData {
    pub protocol_type: u32,
    pub data_type: u32,
    pub protocol_data_request_value: u32,
    pub protocol_data_request_sub_value: u32,
    pub protocol_data_offset: u32,
    pub protocol_data_length: u32,
    pub fixed_protocol_return_data: u32,
    pub protocol_data_request_sub_value2: u32,
    pub protocol_data_request_sub_value3: u32,
    pub protocol_data_request_sub_value4: u32,
}

/// Raw Win32 STORAGE_PROTOCOL_DATA_DESCRIPTOR structure.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StorageProtocolDataDescriptor {
    pub version: u32,
    pub size: u32,
    pub protocol_specific_data: StorageProtocolSpecificData,
}

/// Parsed NVMe SMART / Health information log data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NvmeSmartData {
    pub composite_temp_kelvin: u16,
    pub composite_temp_c: i32,
    pub available_spare_pct: u8,
    pub percentage_used_pct: u8,
    pub warning_temp_time_mins: u32,
    pub critical_temp_time_mins: u32,
}

/// Complete thermal telemetry report recorded during a benchmark run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThermalTelemetry {
    pub start_temp_c: i32,
    pub peak_temp_c: i32,
    pub end_temp_c: i32,
    pub throttled: bool,
    pub available_spare_pct: u8,
    pub percentage_used_pct: u8,
    pub warning_temp_time_mins: u32,
    pub critical_temp_time_start_mins: u32,
    pub critical_temp_time_end_mins: u32,
}

/// Parse raw 512-byte NVMe SMART / Health Information Log Page payload.
pub fn parse_nvme_smart_log(payload: &[u8]) -> Result<NvmeSmartData, DriveError> {
    if payload.len() < NVME_SMART_LOG_SIZE {
        return Err(DriveError::InvalidNvmeLog(payload.len()));
    }

    // Bytes 1..=2: Composite temperature in Kelvin (little-endian)
    let kelvin = u16::from_le_bytes([payload[1], payload[2]]);
    let celsius = if kelvin > 0 {
        (kelvin as f64 - 273.15).round() as i32
    } else {
        0
    };

    // Byte 3: Available Spare normalized percentage (0..100)
    let available_spare_pct = payload[3];

    // Byte 5: Percentage Used estimate (0..100+)
    let percentage_used_pct = payload[5];

    // Bytes 192..=195: Warning Composite Temperature Time in minutes
    let warning_temp_time_mins =
        u32::from_le_bytes([payload[192], payload[193], payload[194], payload[195]]);

    // Bytes 196..=199: Critical Composite Temperature Time in minutes
    let critical_temp_time_mins =
        u32::from_le_bytes([payload[196], payload[197], payload[198], payload[199]]);

    Ok(NvmeSmartData {
        composite_temp_kelvin: kelvin,
        composite_temp_c: celsius,
        available_spare_pct,
        percentage_used_pct,
        warning_temp_time_mins,
        critical_temp_time_mins,
    })
}

/// Execute IOCTL_STORAGE_QUERY_PROPERTY with a specific property ID (adapter or device).
fn issue_nvme_smart_query(
    handle: HANDLE,
    property_id: u32,
) -> Result<NvmeSmartData, windows::core::Error> {
    let header_size = 8usize; // sizeof(PropertyId) + sizeof(QueryType)
    let protocol_data_size = mem::size_of::<StorageProtocolSpecificData>();
    let total_query_size = header_size + protocol_data_size;
    let payload_size = NVME_SMART_LOG_SIZE;
    let buffer_size = total_query_size + payload_size;

    let mut buffer = vec![0u8; buffer_size];

    unsafe {
        let p_prop = buffer.as_mut_ptr() as *mut u32;
        *p_prop = property_id;
        let p_query_type = buffer.as_mut_ptr().add(4) as *mut u32;
        *p_query_type = PROPERTY_STANDARD_QUERY;

        let p_protocol = buffer.as_mut_ptr().add(header_size) as *mut StorageProtocolSpecificData;
        (*p_protocol).protocol_type = PROTOCOL_TYPE_NVME;
        (*p_protocol).data_type = NVME_DATA_TYPE_LOG_PAGE;
        (*p_protocol).protocol_data_request_value = NVME_LOG_PAGE_SMART;
        (*p_protocol).protocol_data_request_sub_value = 0;
        (*p_protocol).protocol_data_offset = protocol_data_size as u32;
        (*p_protocol).protocol_data_length = payload_size as u32;
    }

    let mut bytes_returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(buffer.as_ptr() as *const _),
            buffer.len() as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            buffer.len() as u32,
            Some(&mut bytes_returned),
            None,
        )?;
    }

    let desc = unsafe { &*(buffer.as_ptr() as *const StorageProtocolDataDescriptor) };
    let offset = desc.protocol_specific_data.protocol_data_offset as usize;
    let payload_start = if offset >= mem::size_of::<StorageProtocolDataDescriptor>() {
        offset
    } else {
        8 + offset
    };

    if payload_start + NVME_SMART_LOG_SIZE <= buffer.len() {
        parse_nvme_smart_log(&buffer[payload_start..payload_start + NVME_SMART_LOG_SIZE])
            .map_err(|_| windows::core::Error::from_win32())
    } else {
        Err(windows::core::Error::from_win32())
    }
}

/// Query NVMe SMART / Health information using StorageAdapterProtocolSpecificProperty with
/// StorageDeviceProtocolSpecificProperty fallback.
pub fn query_nvme_smart(handle: HANDLE) -> Result<NvmeSmartData, windows::core::Error> {
    match issue_nvme_smart_query(handle, STORAGE_ADAPTER_PROTOCOL_SPECIFIC_PROPERTY) {
        Ok(data) => Ok(data),
        Err(adapter_err) => {
            // Fallback to StorageDeviceProtocolSpecificProperty
            issue_nvme_smart_query(handle, STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY)
                .map_err(|_| adapter_err)
        }
    }
}

/// Open a non-elevated handle to the drive volume for telemetry polling using
/// `FILE_READ_ATTRIBUTES | SYNCHRONIZE` (with 0 access mask fallback).
pub fn open_telemetry_handle(letter: char) -> Result<HANDLE, windows::core::Error> {
    let path = format!(r"\\.\{letter}:");
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    let access = FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
    let res = unsafe {
        CreateFileW(
            windows::core::PCWSTR(path_w.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    };

    match res {
        Ok(h) if !h.is_invalid() => Ok(h),
        _ => unsafe {
            // Fallback to 0 access mask
            CreateFileW(
                windows::core::PCWSTR(path_w.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        },
    }
}

/// Internal state tracked by the background thermal telemetry polling thread.
#[derive(Debug, Clone)]
struct MonitorState {
    start_temp_c: i32,
    peak_temp_c: i32,
    end_temp_c: i32,
    available_spare_pct: u8,
    percentage_used_pct: u8,
    warning_temp_time_mins: u32,
    critical_temp_time_start_mins: u32,
    critical_temp_time_end_mins: u32,
}

/// Lightweight background thermal and SMART telemetry monitor.
/// Runs alongside the benchmark on a separate volume handle without impacting the IOCP hot path.
pub struct NvmeThermalMonitor {
    letter: char,
    stop_signal: Arc<AtomicBool>,
    state: Arc<Mutex<MonitorState>>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl NvmeThermalMonitor {
    /// Start non-elevated background thermal polling on a drive if it supports NVMe SMART queries.
    /// Returns None gracefully for SATA, USB, virtual disks, or when unsupported.
    pub fn start(letter: char) -> Option<Self> {
        let handle = open_telemetry_handle(letter).ok()?;
        let initial_data = match query_nvme_smart(handle) {
            Ok(data) => data,
            Err(_) => {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return None;
            }
        };
        unsafe {
            let _ = CloseHandle(handle);
        }

        let state = Arc::new(Mutex::new(MonitorState {
            start_temp_c: initial_data.composite_temp_c,
            peak_temp_c: initial_data.composite_temp_c,
            end_temp_c: initial_data.composite_temp_c,
            available_spare_pct: initial_data.available_spare_pct,
            percentage_used_pct: initial_data.percentage_used_pct,
            warning_temp_time_mins: initial_data.warning_temp_time_mins,
            critical_temp_time_start_mins: initial_data.critical_temp_time_mins,
            critical_temp_time_end_mins: initial_data.critical_temp_time_mins,
        }));

        let stop_signal = Arc::new(AtomicBool::new(false));
        let thread_stop = stop_signal.clone();
        let thread_state = state.clone();

        let thread_handle = std::thread::spawn(move || {
            let handle = match open_telemetry_handle(letter) {
                Ok(h) => h,
                Err(_) => return,
            };
            let _guard = VolumeHandleGuard(handle);

            while !thread_stop.load(Ordering::Relaxed) {
                // Poll every 500ms (10 x 50ms intervals for quick shutdown)
                for _ in 0..10 {
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }

                if let Ok(data) = query_nvme_smart(handle) {
                    if let Ok(mut s) = thread_state.lock() {
                        s.end_temp_c = data.composite_temp_c;
                        if data.composite_temp_c > s.peak_temp_c {
                            s.peak_temp_c = data.composite_temp_c;
                        }
                        s.critical_temp_time_end_mins = data.critical_temp_time_mins;
                        s.available_spare_pct = data.available_spare_pct;
                        s.percentage_used_pct = data.percentage_used_pct;
                        s.warning_temp_time_mins = data.warning_temp_time_mins;
                    }
                }
            }
        });

        Some(Self {
            letter,
            stop_signal,
            state,
            thread_handle: Some(thread_handle),
        })
    }

    /// Sample telemetry on demand (e.g. between benchmark stages).
    pub fn sample(&self) {
        if let Ok(handle) = open_telemetry_handle(self.letter) {
            let _guard = VolumeHandleGuard(handle);
            if let Ok(data) = query_nvme_smart(handle) {
                if let Ok(mut s) = self.state.lock() {
                    s.end_temp_c = data.composite_temp_c;
                    if data.composite_temp_c > s.peak_temp_c {
                        s.peak_temp_c = data.composite_temp_c;
                    }
                    s.critical_temp_time_end_mins = data.critical_temp_time_mins;
                    s.available_spare_pct = data.available_spare_pct;
                    s.percentage_used_pct = data.percentage_used_pct;
                    s.warning_temp_time_mins = data.warning_temp_time_mins;
                }
            }
        }
    }

    /// Stop background polling and retrieve the aggregated thermal telemetry.
    pub fn stop(mut self) -> ThermalTelemetry {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(th) = self.thread_handle.take() {
            let _ = th.join();
        }

        // Take a final sample
        self.sample();

        let s = self.state.lock().unwrap();
        let throttled = s.peak_temp_c >= THERMAL_THROTTLE_TEMP_THRESHOLD_C
            || s.critical_temp_time_end_mins > s.critical_temp_time_start_mins;

        ThermalTelemetry {
            start_temp_c: s.start_temp_c,
            peak_temp_c: s.peak_temp_c,
            end_temp_c: s.end_temp_c,
            throttled,
            available_spare_pct: s.available_spare_pct,
            percentage_used_pct: s.percentage_used_pct,
            warning_temp_time_mins: s.warning_temp_time_mins,
            critical_temp_time_start_mins: s.critical_temp_time_start_mins,
            critical_temp_time_end_mins: s.critical_temp_time_end_mins,
        }
    }
}

impl Drop for NvmeThermalMonitor {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(th) = self.thread_handle.take() {
            let _ = th.join();
        }
    }
}
