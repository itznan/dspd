// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use super::ReportRow;
use crate::drives::DriveInfo;
use anyhow::{Context, Result};
use std::fmt::Write as FmtWrite;
use std::path::Path;

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
  <style>
    :root {{
      --bg: #0d1117;
      --card-bg: #161b22;
      --border: #30363d;
      --text: #c9d1d9;
      --text-muted: #8b949e;
      --read-color: #58a6ff;
      --write-color: #3fb950;
      --accent: #f0883e;
      --font-mono: 'Consolas', 'Courier New', monospace;
    }}
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      background-color: var(--bg);
      color: var(--text);
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
      padding: 2.5rem;
      line-height: 1.5;
    }}
    .container {{ max-width: 960px; margin: 0 auto; }}
    .header {{
      border-bottom: 1px solid var(--border);
      padding-bottom: 1.5rem;
      margin-bottom: 2rem;
    }}
    h1 {{ font-size: 2rem; color: #fff; margin-bottom: 0.5rem; }}
    .meta-grid {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
      gap: 1rem;
      margin-top: 1rem;
    }}
    .meta-item {{
      background: var(--card-bg);
      padding: 0.75rem 1rem;
      border-radius: 6px;
      border: 1px solid var(--border);
    }}
    .meta-label {{ font-size: 0.75rem; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; }}
    .meta-value {{ font-size: 1rem; font-weight: 600; color: #fff; margin-top: 0.25rem; }}
    h2 {{ font-size: 1.25rem; color: #fff; margin: 2rem 0 1rem; display: flex; align-items: center; gap: 0.5rem; }}
    table {{
      width: 100%;
      border-collapse: collapse;
      background: var(--card-bg);
      border-radius: 6px;
      overflow: hidden;
      border: 1px solid var(--border);
      margin-bottom: 1.5rem;
    }}
    th {{
      background: #21262d;
      color: var(--text);
      font-weight: 600;
      text-align: right;
      padding: 0.85rem 1.2rem;
      font-size: 0.85rem;
      border-bottom: 1px solid var(--border);
    }}
    th:first-child {{ text-align: left; }}
    td {{
      padding: 0.85rem 1.2rem;
      text-align: right;
      font-family: var(--font-mono);
      font-size: 0.95rem;
      border-bottom: 1px solid #21262d;
    }}
    td:first-child {{
      text-align: left;
      font-weight: 600;
      font-family: inherit;
      color: #fff;
    }}
    tr:last-child td {{ border-bottom: none; }}
    tr:hover td {{ background: rgba(255, 255, 255, 0.02); }}
    .read-val {{ color: var(--read-color); font-weight: 600; }}
    .write-val {{ color: var(--write-color); font-weight: 600; }}
    .footer {{
      margin-top: 3rem;
      padding-top: 1.5rem;
      border-top: 1px solid var(--border);
      font-size: 0.85rem;
      color: var(--text-muted);
      text-align: center;
    }}
    .footer a {{ color: var(--read-color); text-decoration: none; }}
    .footer a:hover {{ text-decoration: underline; }}
  </style>
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

    let profiles = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];

    for prof in profiles {
        let read_row = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Read");
        let write_row = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Write");

        let read_str = read_row
            .map(|r| format!("{:.2}", r.mb_s))
            .unwrap_or_else(|| "—".to_string());
        let write_str = write_row
            .map(|r| format!("{:.2}", r.mb_s))
            .unwrap_or_else(|| "—".to_string());

        writeln!(
            html,
            r#"        <tr>
          <td>{}</td>
          <td class="read-val">{}</td>
          <td class="write-val">{}</td>
        </tr>"#,
            prof, read_str, write_str
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

    for prof in profiles {
        let read_row = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Read");
        let write_row = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Write");

        let fmt_iops = |row: Option<&ReportRow>| {
            row.map(|r| format!("{:.0}", r.iops))
                .unwrap_or_else(|| "—".to_string())
        };
        let fmt_avg = |row: Option<&ReportRow>| {
            row.map(|r| {
                if r.avg_lat_us >= 1000.0 {
                    format!("{:.2} ms", r.avg_lat_us / 1000.0)
                } else {
                    format!("{:.1} µs", r.avg_lat_us)
                }
            })
            .unwrap_or_else(|| "—".to_string())
        };
        let fmt_p99 = |row: Option<&ReportRow>| {
            row.map(|r| {
                if r.p99_lat_us >= 1000 {
                    format!("{:.2} ms", r.p99_lat_us as f64 / 1000.0)
                } else {
                    format!("{} µs", r.p99_lat_us)
                }
            })
            .unwrap_or_else(|| "—".to_string())
        };

        writeln!(
            html,
            r#"        <tr>
          <td>{}</td>
          <td>{}</td>
          <td>{}</td>
          <td>{}</td>
          <td>{}</td>
          <td>{}</td>
          <td>{}</td>
        </tr>"#,
            prof,
            fmt_iops(read_row),
            fmt_iops(write_row),
            fmt_avg(read_row),
            fmt_p99(read_row),
            fmt_avg(write_row),
            fmt_p99(write_row)
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
