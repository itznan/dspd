// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

pub mod drives;
pub mod engine;
pub mod report;

use anyhow::{Context, Result};

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

    let int_bytes = bytes as u64;
    // Align up to 4096 sector boundary
    let aligned = (int_bytes + 4095) & !4095;
    Ok(aligned)
}
