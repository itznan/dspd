// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Console user interface, interactive prompts, and summary performance tables.

use anyhow::Result;
use std::io::{stdin, stdout, Write};

use crate::drives::{DriveInfo, ThermalTelemetry};
use crate::engine::profile::ALL_PROFILES;
use crate::engine::sustained::SustainedAnalysis;
use crate::report::ReportRow;

const SUMMARY_PROFILES: [&str; 4] = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];

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

/// Print profile specifications and configurations for validation purposes.
pub fn print_validation_commands(duration: u64, warmup: u64) {
    println!("dspd benchmark profile configurations:\n");
    for p in ALL_PROFILES {
        println!("  {}", p.spec(duration, warmup, false));
        println!("  {}", p.spec(duration, warmup, true));
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

/// Print formatted console performance summary tables.
pub fn print_summary_tables(
    rows: &[ReportRow],
    drive: &DriveInfo,
    thermal: Option<&ThermalTelemetry>,
) {
    println!("════════════════════════════════════════════════════════════════════════");
    println!(
        "  dspd Benchmark Summary — Drive {}: ({})",
        drive.letter, drive.model
    );
    println!("────────────────────────────────────────────────────────────────────────");
    println!("  Profile        │ Read (MB/s) │ Write (MB/s) │ Read IOPS │ Write IOPS");
    println!("────────────────────────────────────────────────────────────────────────");

    for prof in SUMMARY_PROFILES {
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
    for prof in SUMMARY_PROFILES {
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

    if let Some(t) = thermal {
        println!("────────────────────────────────────────────────────────────────────────");
        println!(
            "  Thermal        │ Temp: {}°C -> {}°C peak (end: {}°C)",
            t.start_temp_c, t.peak_temp_c, t.end_temp_c
        );
        println!(
            "  SMART Health   │ Spare: {}% │ Life Used: {}% │ Crit Temp Time: {} min",
            t.available_spare_pct, t.percentage_used_pct, t.critical_temp_time_end_mins
        );
        if t.throttled {
            println!("────────────────────────────────────────────────────────────────────────");
            println!(
                "  [!] WARNING: Thermal throttling likely impacted the results!"
            );
            if t.peak_temp_c >= 70 {
                println!(
                    "      - Drive temperature reached {}°C (crossed 70°C threshold).",
                    t.peak_temp_c
                );
            }
            if t.critical_temp_time_end_mins > t.critical_temp_time_start_mins {
                println!(
                    "      - NVMe Critical Composite Temp Time increased by {} min ({} -> {} min).",
                    t.critical_temp_time_end_mins - t.critical_temp_time_start_mins,
                    t.critical_temp_time_start_mins,
                    t.critical_temp_time_end_mins
                );
            }
        }
    }
    println!("════════════════════════════════════════════════════════════════════════\n");
}

/// Print formatted sustained write profile summary including throughput curve sparkline,
/// SLC cliff point, sustained speed, and thermal trajectory.
pub fn print_sustained_summary(
    drive: &DriveInfo,
    analysis: &SustainedAnalysis,
    thermal: Option<&ThermalTelemetry>,
) {
    println!("════════════════════════════════════════════════════════════════════════");
    println!(
        "  dspd Sustained Write Profile — Drive {}: ({})",
        drive.letter, drive.model
    );
    println!("────────────────────────────────────────────────────────────────────────");
    println!(
        "  Throughput Curve ({:.1} GiB written over {}s):",
        analysis.total_gib_written, analysis.duration_secs
    );
    println!("  {}", analysis.sparkline);
    println!("────────────────────────────────────────────────────────────────────────");
    println!(
        "  Burst Write Speed     : {:>8.1} MB/s (initial pseudo-SLC plateau)",
        analysis.burst_speed_mb_s
    );
    if let (Some(cliff_gib), Some(drop_pct)) = (analysis.slc_cliff_gib, analysis.cliff_drop_percent) {
        println!(
            "  SLC Cache Cliff Point : {:>8.1} GiB (drop of {:.1}% after {:.1} GiB written)",
            cliff_gib, drop_pct, cliff_gib
        );
    } else {
        println!(
            "  SLC Cache Cliff Point : Not reached (>= {:.1} GiB without >= 35% drop)",
            analysis.total_gib_written
        );
    }
    if let Some(sustained_speed) = analysis.sustained_speed_mb_s {
        println!(
            "  Sustained Write Speed : {:>8.1} MB/s (steady-state direct-to-TLC/QLC)",
            sustained_speed
        );
    } else {
        println!("  Sustained Write Speed :      N/A (drive remained in SLC cache)");
    }
    println!(
        "  Throughput Stability  : CV {:>5.1}% (peak: {:.1} MB/s, min: {:.1} MB/s)",
        analysis.cv_percent, analysis.peak_speed_mb_s, analysis.min_speed_mb_s
    );

    if let Some(t) = thermal {
        println!("────────────────────────────────────────────────────────────────────────");
        println!(
            "  Thermal Trajectory    : Temp: {}°C -> {}°C peak (end: {}°C)",
            t.start_temp_c, t.peak_temp_c, t.end_temp_c
        );
        println!(
            "  SMART Health          : Spare: {}% │ Life Used: {}% │ Crit Temp Time: {} min",
            t.available_spare_pct, t.percentage_used_pct, t.critical_temp_time_end_mins
        );
        if t.throttled {
            println!("────────────────────────────────────────────────────────────────────────");
            println!(
                "  [!] WARNING: Thermal throttling likely impacted the results!"
            );
            if t.peak_temp_c >= 70 {
                println!(
                    "      - Drive temperature reached {}°C (crossed 70°C threshold).",
                    t.peak_temp_c
                );
            }
            if t.critical_temp_time_end_mins > t.critical_temp_time_start_mins {
                println!(
                    "      - NVMe Critical Composite Temp Time increased by {} min ({} -> {} min).",
                    t.critical_temp_time_end_mins - t.critical_temp_time_start_mins,
                    t.critical_temp_time_start_mins,
                    t.critical_temp_time_end_mins
                );
            }
        }
    }
    println!("════════════════════════════════════════════════════════════════════════\n");
}
