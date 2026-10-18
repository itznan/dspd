// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Structured JSON benchmark report generator.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};

use super::ReportRow;
use crate::cli::VERSION;
use crate::drives::{DriveInfo, ThermalTelemetry};
use crate::engine::sustained::{SecondBucket, SustainedAnalysis};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonReport {
    pub tool: String,
    pub version: String,
    pub timestamp: String,
    pub drive: DriveMeta,
    pub configuration: TestConfig,
    pub results: Vec<ReportRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal: Option<ThermalTelemetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sustained: Option<SustainedReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SustainedReport {
    pub burst_speed_mb_s: f64,
    pub slc_cliff_gib: Option<f64>,
    pub cliff_drop_percent: Option<f64>,
    pub sustained_speed_mb_s: Option<f64>,
    pub cv_percent: f64,
    pub peak_speed_mb_s: f64,
    pub min_speed_mb_s: f64,
    pub total_gib_written: f64,
    pub duration_secs: u32,
    pub sparkline: String,
    pub buckets: Vec<SecondBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriveMeta {
    pub letter: String,
    pub model: String,
    pub bus_type: String,
    pub media_type: String,
    pub capacity_bytes: u64,
    pub capacity_gb: f64,
    pub free_bytes: u64,
    pub free_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TestConfig {
    pub test_size_bytes: u64,
    pub test_size_gib: f64,
    pub passes: u32,
    pub duration_secs: u64,
    pub warmup_secs: u64,
}

/// Construct a complete `JsonReport` structure from benchmark data.
#[allow(clippy::too_many_arguments)]
pub fn build_json_report(
    rows: &[ReportRow],
    drive: &DriveInfo,
    test_size_bytes: u64,
    passes: u32,
    duration: u64,
    warmup: u64,
    thermal: Option<ThermalTelemetry>,
) -> JsonReport {
    JsonReport {
        tool: "dspd".to_string(),
        version: VERSION.to_string(),
        timestamp: Local::now().to_rfc3339(),
        drive: DriveMeta {
            letter: drive.letter.to_string(),
            model: drive.model.clone(),
            bus_type: drive.bus_type.to_string(),
            media_type: drive.media_type.to_string(),
            capacity_bytes: drive.capacity,
            capacity_gb: drive.capacity as f64 / 1_000_000_000.0,
            free_bytes: drive.free,
            free_gb: drive.free as f64 / 1_000_000_000.0,
        },
        configuration: TestConfig {
            test_size_bytes,
            test_size_gib: test_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            passes,
            duration_secs: duration,
            warmup_secs: warmup,
        },
        results: rows.to_vec(),
        thermal,
        sustained: None,
    }
}

/// Serialize a `JsonReport` into a pretty-printed JSON string.
pub fn to_json_string(report: &JsonReport) -> Result<String> {
    serde_json::to_string_pretty(report)
        .context("Failed to serialize benchmark report to JSON format")
}

/// Serialize and write benchmark report to a JSON file.
#[allow(clippy::too_many_arguments)]
pub fn write_json(
    path: &Path,
    rows: &[ReportRow],
    drive: &DriveInfo,
    test_size_bytes: u64,
    passes: u32,
    duration: u64,
    warmup: u64,
    thermal: Option<ThermalTelemetry>,
) -> Result<JsonReport> {
    let report =
        build_json_report(rows, drive, test_size_bytes, passes, duration, warmup, thermal);
    let json_string = to_json_string(&report)?;

    std::fs::write(path, json_string)
        .with_context(|| format!("Failed to write JSON report to '{}'", path.display()))?;

    Ok(report)
}

/// Construct a `JsonReport` structure for sustained write profiling.
#[allow(clippy::too_many_arguments)]
pub fn build_sustained_json_report(
    drive: &DriveInfo,
    test_size_bytes: u64,
    max_duration_secs: u64,
    analysis: &SustainedAnalysis,
    buckets: &[SecondBucket],
    thermal: Option<ThermalTelemetry>,
) -> JsonReport {
    JsonReport {
        tool: "dspd".to_string(),
        version: VERSION.to_string(),
        timestamp: Local::now().to_rfc3339(),
        drive: DriveMeta {
            letter: drive.letter.to_string(),
            model: drive.model.clone(),
            bus_type: drive.bus_type.to_string(),
            media_type: drive.media_type.to_string(),
            capacity_bytes: drive.capacity,
            capacity_gb: drive.capacity as f64 / 1_000_000_000.0,
            free_bytes: drive.free,
            free_gb: drive.free as f64 / 1_000_000_000.0,
        },
        configuration: TestConfig {
            test_size_bytes,
            test_size_gib: test_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            passes: 1,
            duration_secs: max_duration_secs,
            warmup_secs: 0,
        },
        results: Vec::new(),
        thermal,
        sustained: Some(SustainedReport {
            burst_speed_mb_s: analysis.burst_speed_mb_s,
            slc_cliff_gib: analysis.slc_cliff_gib,
            cliff_drop_percent: analysis.cliff_drop_percent,
            sustained_speed_mb_s: analysis.sustained_speed_mb_s,
            cv_percent: analysis.cv_percent,
            peak_speed_mb_s: analysis.peak_speed_mb_s,
            min_speed_mb_s: analysis.min_speed_mb_s,
            total_gib_written: analysis.total_gib_written,
            duration_secs: analysis.duration_secs,
            sparkline: analysis.sparkline.clone(),
            buckets: buckets.to_vec(),
        }),
    }
}

/// Serialize and write sustained benchmark report to a JSON file.
#[allow(clippy::too_many_arguments)]
pub fn write_sustained_json(
    path: &Path,
    drive: &DriveInfo,
    test_size_bytes: u64,
    max_duration_secs: u64,
    analysis: &SustainedAnalysis,
    buckets: &[SecondBucket],
    thermal: Option<ThermalTelemetry>,
) -> Result<JsonReport> {
    let report = build_sustained_json_report(
        drive,
        test_size_bytes,
        max_duration_secs,
        analysis,
        buckets,
        thermal,
    );
    let json_string = to_json_string(&report)?;

    std::fs::write(path, json_string).with_context(|| {
        format!(
            "Failed to write sustained JSON report to '{}'",
            path.display()
        )
    })?;

    Ok(report)
}
