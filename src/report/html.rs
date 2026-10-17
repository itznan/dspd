// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Self-contained HTML benchmark report generator.

use super::styles::REPORT_CSS;
use super::ReportRow;
use crate::drives::DriveInfo;
use anyhow::{Context, Result};
use std::fmt::Write as FmtWrite;
use std::path::Path;

const PROFILES: [&str; 4] = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];

/// Format latency in microseconds to a readable string (ms or µs).
fn format_lat(us: f64) -> String {
    if us >= 1000.0 {
        format!("{:.2} ms", us / 1000.0)
    } else {
        format!("{:.1} µs", us)
    }
}

/// Format u64 latency in microseconds to a readable string (ms or µs).
fn format_lat_u64(us: u64) -> String {
    if us >= 1000 {
        format!("{:.2} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}

/// Generate a self-contained, dark-themed HTML benchmark report matching the CrystalDiskMark layout.
pub fn write_html(
    path: &Path,
    rows: &[ReportRow],
    drive: &DriveInfo,
    test_size_bytes: u64,
) -> Result<()> {
    let mut html = String::with_capacity(8192);

    let test_size_gib = test_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let total_gb = drive.capacity as f64 / 1_000_000_000.0;
    let free_gb = drive.free as f64 / 1_000_000_000.0;

    writeln!(
        html,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>dspd Benchmark Report - Drive {drive_letter}:</title>
  <style>{REPORT_CSS}</style>
</head>
<body>
  <div class="container">
    <div class="header">
      <h1>dspd Disk Benchmark Report</h1>
      <p style="color: var(--text-muted);">Fast, safe, single-binary disk benchmarking for Windows</p>
      
      <div class="meta-grid">
        <div class="meta-item">
          <div class="meta-label">Target Drive</div>
          <div class="meta-value">{drive_letter}: ({media_type})</div>
        </div>
        <div class="meta-item">
          <div class="meta-label">Model / Bus</div>
          <div class="meta-value">{model} [{bus_type}]</div>
        </div>
        <div class="meta-item">
          <div class="meta-label">Capacity / Free</div>
          <div class="meta-value">{free_gb:.1} GB / {total_gb:.1} GB</div>
        </div>
        <div class="meta-item">
          <div class="meta-label">Test File Size</div>
          <div class="meta-value">{test_size_gib:.2} GiB</div>
        </div>
      </div>
    </div>

    <h2>CrystalDiskMark Layout (Throughput)</h2>
    <table>
      <thead>
        <tr>
          <th>Profile</th>
          <th>Read (MB/s)</th>
          <th>Write (MB/s)</th>
        </tr>
      </thead>
      <tbody>"#,
        drive_letter = drive.letter,
        media_type = drive.media_type,
        model = drive.model,
        bus_type = drive.bus_type,
        free_gb = free_gb,
        total_gb = total_gb,
        test_size_gib = test_size_gib,
    )?;

    for prof in PROFILES {
        let read = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Read")
            .map(|r| format!("{:.2}", r.mb_s))
            .unwrap_or_else(|| "—".to_string());
        let write = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Write")
            .map(|r| format!("{:.2}", r.mb_s))
            .unwrap_or_else(|| "—".to_string());

        writeln!(
            html,
            r#"        <tr>
          <td>{}</td>
          <td class="read-val">{}</td>
          <td class="write-val">{}</td>
        </tr>"#,
            prof, read, write
        )?;
    }

    writeln!(
        html,
        r#"      </tbody>
    </table>

    <h2>Detailed Performance Metrics (IOPS & Latency)</h2>
    <table>
      <thead>
        <tr>
          <th>Profile</th>
          <th>Read IOPS</th>
          <th>Write IOPS</th>
          <th>Read Avg Lat</th>
          <th>Read p99 Lat</th>
          <th>Write Avg Lat</th>
          <th>Write p99 Lat</th>
        </tr>
      </thead>
      <tbody>"#
    )?;

    for prof in PROFILES {
        let r = rows
            .iter()
            .find(|row| row.profile == prof && row.direction == "Read");
        let w = rows
            .iter()
            .find(|row| row.profile == prof && row.direction == "Write");

        let r_iops = r
            .map(|x| format!("{:.0}", x.iops))
            .unwrap_or_else(|| "—".to_string());
        let w_iops = w
            .map(|x| format!("{:.0}", x.iops))
            .unwrap_or_else(|| "—".to_string());
        let r_avg = r
            .map(|x| format_lat(x.avg_lat_us))
            .unwrap_or_else(|| "—".to_string());
        let r_p99 = r
            .map(|x| format_lat_u64(x.p99_lat_us))
            .unwrap_or_else(|| "—".to_string());
        let w_avg = w
            .map(|x| format_lat(x.avg_lat_us))
            .unwrap_or_else(|| "—".to_string());
        let w_p99 = w
            .map(|x| format_lat_u64(x.p99_lat_us))
            .unwrap_or_else(|| "—".to_string());

        writeln!(
            html,
            r#"        <tr>
          <td>{prof}</td>
          <td>{r_iops}</td>
          <td>{w_iops}</td>
          <td>{r_avg}</td>
          <td>{r_p99}</td>
          <td>{w_avg}</td>
          <td>{w_p99}</td>
        </tr>"#
        )?;
    }

    writeln!(
        html,
        r#"      </tbody>
    </table>

    <div class="footer">
      Generated by <b>dspd</b> by itznan, <a href="https://github.com/itznan/dspd" target="_blank" rel="noopener">github.com/itznan/dspd</a>
    </div>
  </div>
</body>
</html>"#
    )?;

    std::fs::write(path, html)
        .with_context(|| format!("Failed to write HTML report to '{}'", path.display()))?;
    Ok(())
}
