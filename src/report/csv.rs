// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use super::ReportRow;
use anyhow::{Context, Result};
use std::path::Path;

/// Export benchmark results to a standard CSV file.
pub fn write_csv(path: &Path, rows: &[ReportRow]) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)
        .with_context(|| format!("Failed to create CSV report at '{}'", path.display()))?;

    for row in rows {
        writer.serialize(row)?;
    }

    writer.flush()?;
    Ok(())
}

/// Export sustained time-series buckets to a CSV file.
pub fn write_sustained_csv(
    path: &Path,
    buckets: &[crate::engine::sustained::SecondBucket],
) -> Result<()> {
    let mut writer = csv::Writer::from_path(path).with_context(|| {
        format!(
            "Failed to create sustained CSV report at '{}'",
            path.display()
        )
    })?;

    for bucket in buckets {
        writer.serialize(bucket)?;
    }

    writer.flush()?;
    Ok(())
}
