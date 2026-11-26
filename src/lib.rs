// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

pub mod cli;
pub mod drives;
pub mod engine;
pub mod file;
pub mod report;
pub mod runner;
pub mod signal;
pub mod ui;

use anyhow::{Context, Result};

fn align_sector_bytes(bytes: f64) -> Result<u64> {
    if !bytes.is_finite() || bytes <= 0.0 {
        anyhow::bail!("Size must be a positive finite number");
    }
    if bytes > (u64::MAX - 4095) as f64 {
        anyhow::bail!("Size exceeds maximum supported value");
    }
    let aligned = ((bytes as u64) + 4095) & !4095;
    if aligned == 0 {
        anyhow::bail!("Size must be greater than zero");
    }
    Ok(aligned)
}

/// Parse human-readable size strings (e.g., 1GiB, 512MiB, 4GiB, 1GB).
pub fn parse_size(input: &str) -> Result<u64> {
    let s = input.trim();
    if s.is_empty() {
        anyhow::bail!("Size argument cannot be empty");
    }

    let (num_part, unit_part) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(s.len()),
    );

    let val: f64 = num_part
        .parse()
        .with_context(|| format!("Invalid numeric value in size: '{}'", num_part))?;

    let unit = unit_part.trim().to_lowercase();
    let bytes = match unit.as_str() {
        "gib" | "g" => val * 1024.0 * 1024.0 * 1024.0,
        "gb" => val * 1_000_000_000.0,
        "mib" | "m" => val * 1024.0 * 1024.0,
        "mb" => val * 1_000_000.0,
        "kib" | "k" => val * 1024.0,
        "kb" => val * 1000.0,
        "b" | "" => val,
        other => anyhow::bail!("Unknown size unit '{}'. Use GiB, MiB, KiB, or bytes", other),
    };

    align_sector_bytes(bytes)
}

/// Parse sustained size argument, supporting both pure GiB numbers (e.g. "50")
/// and human-readable size strings (e.g. "50GiB", "64GB").
pub fn parse_sustained_size(input: &str) -> Result<u64> {
    let s = input.trim();
    if s.is_empty() {
        anyhow::bail!("Sustained size argument cannot be empty");
    }

    if s.chars().all(|c| c.is_ascii_digit() || c == '.') {
        let gib: f64 = s
            .parse()
            .with_context(|| format!("Invalid numeric value in sustained size: '{}'", s))?;
        align_sector_bytes(gib * 1024.0 * 1024.0 * 1024.0)
    } else {
        parse_size(s)
    }
}
