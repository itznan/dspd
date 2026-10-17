// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

pub mod csv;
pub mod html;

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportRow {
    pub profile: String,
    pub direction: String,
    pub mb_s: f64,
    pub iops: f64,
    pub avg_lat_us: f64,
    pub p99_lat_us: u64,
}

/// Retrieve the current user's Desktop folder path using Win32 Shell API with environment fallback.
pub fn get_desktop_dir() -> std::path::PathBuf {
    unsafe {
        if let Ok(path_ptr) = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None) {
            if !path_ptr.is_null() {
                let path_str = path_ptr.to_string().unwrap_or_default();
                CoTaskMemFree(Some(path_ptr.as_ptr() as *mut _));
                if !path_str.is_empty() {
                    return std::path::PathBuf::from(path_str);
                }
            }
        }
    }

    std::env::var_os("USERPROFILE")
        .map(|p| std::path::PathBuf::from(p).join("Desktop"))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
