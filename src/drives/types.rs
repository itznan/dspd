// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Type definitions and error structures for physical drive discovery.

use thiserror::Error;

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
    #[error("Drive {0}: NVMe SMART query unsupported ({1})")]
    NvmeUnsupported(char, String),
    #[error("Invalid NVMe SMART log length: {0} bytes (expected at least 512)")]
    InvalidNvmeLog(usize),
}
