// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Console user interface, interactive prompts, and CrystalDiskMark layout tables.

use anyhow::Result;
use std::io::{stdin, stdout, Write};

use crate::drives::DriveInfo;
use crate::engine::profile::ALL_PROFILES;
use crate::report::ReportRow;

const CDM_PROFILES: [&str; 4] = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];

/// Format floating-point microsecond latency to readable units.
pub fn format_latency(us: f64) -> String {
    if us >= 1000.0 {
        format!("{:.2} ms", us / 1000.0)
    } else {
        format!("{:.1} µs", us)
    }
}

/// Format integer microsecond latency to readable units.
pub fn format_latency_u64(us: u64) -> String {
    if us >= 1000 {
        format!("{:.2} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}

/// Print equivalent DiskSpd command lines for validation purposes.
pub fn print_validation_commands(duration: u64, warmup: u64) {
    println!("Equivalent DiskSpd command lines for validation:\n");
    for p in ALL_PROFILES {
        println!("  {}", p.diskspd_cmd(duration, warmup, false, "<file>"));
        println!("  {}", p.diskspd_cmd(duration, warmup, true, "<file>"));
    }
}

/// Display SSD-wear warning and confirm user continuation.
pub fn prompt_ssd_wear(test_size_gib: f64, num_profiles: usize, passes: u32) -> Result<bool> {
    println!("  [!] NOTICE: Benchmarking drives writes unbuffered data streams to flash cells.");
    println!(
        "      Estimated data write per drive: ~{:.2} GiB ({} pass(es)).",
        test_size_gib * (num_profiles as f64 * passes as f64 + 1.0),
        passes
    );
    print!("  Do you want to continue? [y/N]: ");
    stdout().flush()?;

    let mut input = String::new();
    stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();
    Ok(trimmed == "y" || trimmed == "yes")
}

/// Print formatted CrystalDiskMark 9 console summary tables.
pub fn print_summary_tables(rows: &[ReportRow], drive: &DriveInfo) {
    println!("════════════════════════════════════════════════════════════════════════");
    println!(
        "  CrystalDiskMark 9 Summary — Drive {}: ({})",
        drive.letter, drive.model
    );
    println!("────────────────────────────────────────────────────────────────────────");
    println!("  Profile        │ Read (MB/s) │ Write (MB/s) │ Read IOPS │ Write IOPS");
    println!("────────────────────────────────────────────────────────────────────────");

    for prof in CDM_PROFILES {
        let read = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Read");
        let write = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Write");

        let r_mb = read
            .map(|r| format!("{:>10.2}", r.mb_s))
            .unwrap_or_else(|| "         —".to_string());
        let w_mb = write
            .map(|r| format!("{:>10.2}", r.mb_s))
            .unwrap_or_else(|| "         —".to_string());
        let r_iops = read
            .map(|r| format!("{:>10.0}", r.iops))
            .unwrap_or_else(|| "         —".to_string());
        let w_iops = write
            .map(|r| format!("{:>10.0}", r.iops))
            .unwrap_or_else(|| "         —".to_string());

        println!(
            "  {:<14} │ {} │ {} │ {} │ {}",
            prof, r_mb, w_mb, r_iops, w_iops
        );
    }
    println!("────────────────────────────────────────────────────────────────────────");

    println!("  Profile        │ Read Avg Lat │ Read p99 Lat │ Write Avg Lat │ Write p99 Lat");
    println!("────────────────────────────────────────────────────────────────────────");
    for prof in CDM_PROFILES {
        let read = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Read");
        let write = rows
            .iter()
            .find(|r| r.profile == prof && r.direction == "Write");

        let r_avg = read
            .map(|r| format!("{:>11}", format_latency(r.avg_lat_us)))
            .unwrap_or_else(|| "          —".to_string());
        let r_p99 = read
            .map(|r| format!("{:>11}", format_latency_u64(r.p99_lat_us)))
            .unwrap_or_else(|| "          —".to_string());
        let w_avg = write
            .map(|r| format!("{:>12}", format_latency(r.avg_lat_us)))
            .unwrap_or_else(|| "           —".to_string());
        let w_p99 = write
            .map(|r| format!("{:>12}", format_latency_u64(r.p99_lat_us)))
            .unwrap_or_else(|| "           —".to_string());

        println!(
            "  {:<14} │ {} │ {} │ {} │ {}",
            prof, r_avg, r_p99, w_avg, w_p99
        );
    }
    println!("════════════════════════════════════════════════════════════════════════\n");
}
