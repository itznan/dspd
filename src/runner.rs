// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Test execution runner, pass orchestration, and benchmark execution.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use indicatif::{ProgressBar, ProgressStyle};
use windows::Win32::Foundation::CloseHandle;

use crate::cli::Args;
use crate::drives::{DriveInfo, NvmeThermalMonitor};
use crate::engine::iocp::IocpEngine;
use crate::engine::profile::{Profile, RwMix};
use crate::engine::stats::{average_passes, PassResult};
use crate::engine::sustained::{analyze_sustained_buckets, run_sustained_write_iocp};
use crate::file::{open_benchmark_file, prepare_test_file, resolve_test_file_path, FileGuard};
use crate::parse_sustained_size;
use crate::report::csv::{write_csv, write_sustained_csv};
use crate::report::json::{to_json_string, write_json, write_sustained_json};
use crate::report::ReportRow;
use crate::signal::{is_cancelled, CANCELLED};
use crate::ui::{format_latency, format_latency_u64, print_summary_tables, print_sustained_summary};

/// Benchmark a single drive across all requested profiles.
pub fn benchmark_drive(
    drive: &DriveInfo,
    profiles: &[Profile],
    test_size: u64,
    args: &Args,
    output_dir: &Path,
) -> Result<()> {
    if is_cancelled() {
        return Ok(());
    }

    let test_size_gib = test_size as f64 / (1024.0 * 1024.0 * 1024.0);
    let total_gb = drive.capacity as f64 / 1_000_000_000.0;
    let free_gb = drive.free as f64 / 1_000_000_000.0;

    println!("────────────────────────────────────────────────────────────────────────");
    println!(
        "Drive {}: {} [{}]",
        drive.letter, drive.model, drive.media_type
    );
    println!(
        "Bus Type: {} | Capacity: {:.1} GB | Free Space: {:.1} GB",
        drive.bus_type, total_gb, free_gb
    );
    println!("────────────────────────────────────────────────────────────────────────");

    // Verify free space requirement (test size + 10%)
    let min_required = (test_size as f64 * 1.1) as u64;
    if drive.free < min_required {
        eprintln!(
            "  [WARN] Drive {}: has insufficient free space ({:.2} GiB free, need {:.2} GiB). Skipping drive.\n",
            drive.letter,
            drive.free as f64 / (1024.0 * 1024.0 * 1024.0),
            min_required as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        return Ok(());
    }

    let test_file_path = resolve_test_file_path(drive.letter);

    println!(
        "  Test File: {} ({:.2} GiB)",
        test_file_path.display(),
        test_size_gib
    );
    println!(
        "  Passes   : {} pass(es) x {}s (warmup: {}s)\n",
        args.passes, args.duration, args.warmup
    );

    let _file_guard = FileGuard {
        path: test_file_path.clone(),
    };

    // Start background NVMe thermal telemetry polling if supported
    let thermal_monitor = NvmeThermalMonitor::start(drive.letter);

    // Prepare test file with pseudo-random content
    prepare_test_file(&test_file_path, test_size)?;

    let mut rows: Vec<ReportRow> = Vec::new();

    let parsed_mix = args
        .rw_mix
        .as_deref()
        .map(RwMix::parse)
        .transpose()?;

    for profile in profiles {
        if is_cancelled() {
            break;
        }

        let effective_mix = parsed_mix.or(profile.default_rw_mix);

        if let Some(mix) = effective_mix {
            // Mixed Read/Write workload
            let mixed_avg =
                run_test_passes(&test_file_path, test_size, profile, false, Some(mix), args)?;
            println!(
                "  [{}] Read ({:>2}%) : {:>8.2} MB/s | {:>8.0} IOPS | p50: {:>7} | p99: {:>7} | p99.9: {:>7}",
                profile.name,
                mix.read_pct,
                mixed_avg.read_mb_s(),
                mixed_avg.read_iops(),
                format_latency_u64(mixed_avg.read_p50_us()),
                format_latency_u64(mixed_avg.read_p99_us()),
                format_latency_u64(mixed_avg.read_p999_us())
            );
            println!(
                "  [{}] Write({:>2}%) : {:>8.2} MB/s | {:>8.0} IOPS | p50: {:>7} | p99: {:>7} | p99.9: {:>7}",
                profile.name,
                mix.write_pct,
                mixed_avg.write_mb_s(),
                mixed_avg.write_iops(),
                format_latency_u64(mixed_avg.write_p50_us()),
                format_latency_u64(mixed_avg.write_p99_us()),
                format_latency_u64(mixed_avg.write_p999_us())
            );
            if mixed_avg.flush_count > 0 {
                println!(
                    "  [{}] PLP Flush  : {} flushes | Avg: {}",
                    profile.name,
                    mixed_avg.flush_count,
                    format_latency(mixed_avg.avg_flush_us())
                );
            }
            println!();

            rows.push(ReportRow {
                profile: profile.name.to_string(),
                direction: "Read".to_string(),
                mb_s: mixed_avg.read_mb_s(),
                iops: mixed_avg.read_iops(),
                p50_lat_us: mixed_avg.read_p50_us(),
                avg_lat_us: mixed_avg.read_avg_us(),
                p99_lat_us: mixed_avg.read_p99_us(),
                p999_lat_us: mixed_avg.read_p999_us(),
            });

            rows.push(ReportRow {
                profile: profile.name.to_string(),
                direction: "Write".to_string(),
                mb_s: mixed_avg.write_mb_s(),
                iops: mixed_avg.write_iops(),
                p50_lat_us: mixed_avg.write_p50_us(),
                avg_lat_us: mixed_avg.write_avg_us(),
                p99_lat_us: mixed_avg.write_p99_us(),
                p999_lat_us: mixed_avg.write_p999_us(),
            });

            if let Some(ref monitor) = thermal_monitor {
                monitor.sample();
            }
        } else if profile.is_read_only {
            // Read-only profile (e.g. directstorage or llm-stream)
            let read_avg =
                run_test_passes(&test_file_path, test_size, profile, false, None, args)?;
            println!(
                "  [{}] Read : {:>8.2} MB/s | {:>8.0} IOPS | p50: {:>7} | p99: {:>7} | p99.9: {:>7}",
                profile.name,
                read_avg.throughput_mb_s(),
                read_avg.iops(profile.block_size),
                format_latency_u64(read_avg.p50_latency_us()),
                format_latency_u64(read_avg.p99_latency_us()),
                format_latency_u64(read_avg.p999_latency_us())
            );
            if profile.is_llm_stream {
                let mb_s = read_avg.throughput_mb_s();
                let est_load_secs = if mb_s > 0.0 {
                    (16.0 * 1024.0 * 1024.0 * 1024.0) / (mb_s * 1_000_000.0)
                } else {
                    0.0
                };
                println!(
                    "  [{}] 16 GiB Model Load Time: ~{:.2}s",
                    profile.name, est_load_secs
                );
            }
            println!();

            rows.push(ReportRow {
                profile: profile.name.to_string(),
                direction: "Read".to_string(),
                mb_s: read_avg.throughput_mb_s(),
                iops: read_avg.iops(profile.block_size),
                p50_lat_us: read_avg.p50_latency_us(),
                avg_lat_us: read_avg.avg_latency_us(),
                p99_lat_us: read_avg.p99_latency_us(),
                p999_lat_us: read_avg.p999_latency_us(),
            });

            if let Some(ref monitor) = thermal_monitor {
                monitor.sample();
            }
        } else {
            // Direction 1: READ
            let read_avg =
                run_test_passes(&test_file_path, test_size, profile, false, None, args)?;
            println!(
                "  [{}] Read : {:>8.2} MB/s | {:>8.0} IOPS | p50: {:>7} | p99: {:>7} | p99.9: {:>7}",
                profile.name,
                read_avg.throughput_mb_s(),
                read_avg.iops(profile.block_size),
                format_latency_u64(read_avg.p50_latency_us()),
                format_latency_u64(read_avg.p99_latency_us()),
                format_latency_u64(read_avg.p999_latency_us())
            );
            rows.push(ReportRow {
                profile: profile.name.to_string(),
                direction: "Read".to_string(),
                mb_s: read_avg.throughput_mb_s(),
                iops: read_avg.iops(profile.block_size),
                p50_lat_us: read_avg.p50_latency_us(),
                avg_lat_us: read_avg.avg_latency_us(),
                p99_lat_us: read_avg.p99_latency_us(),
                p999_lat_us: read_avg.p999_latency_us(),
            });

            // Sample telemetry between benchmark stages
            if let Some(ref monitor) = thermal_monitor {
                monitor.sample();
            }

            // Direction 2: WRITE
            if is_cancelled() {
                break;
            }
            let write_avg =
                run_test_passes(&test_file_path, test_size, profile, true, None, args)?;
            println!(
                "  [{}] Write: {:>8.2} MB/s | {:>8.0} IOPS | p50: {:>7} | p99: {:>7} | p99.9: {:>7}",
                profile.name,
                write_avg.throughput_mb_s(),
                write_avg.iops(profile.block_size),
                format_latency_u64(write_avg.p50_latency_us()),
                format_latency_u64(write_avg.p99_latency_us()),
                format_latency_u64(write_avg.p999_latency_us())
            );
            rows.push(ReportRow {
                profile: profile.name.to_string(),
                direction: "Write".to_string(),
                mb_s: write_avg.throughput_mb_s(),
                iops: write_avg.iops(profile.block_size),
                p50_lat_us: write_avg.p50_latency_us(),
                avg_lat_us: write_avg.avg_latency_us(),
                p99_lat_us: write_avg.p99_latency_us(),
                p999_lat_us: write_avg.p999_latency_us(),
            });
            println!();

            // Sample telemetry between benchmark stages
            if let Some(ref monitor) = thermal_monitor {
                monitor.sample();
            }
        }
    }

    // Stop background thermal monitoring and aggregate results
    let thermal_report = thermal_monitor.map(|m| m.stop());

    // Print summary performance tables in console
    print_summary_tables(&rows, drive, thermal_report.as_ref());

    // Export CSV and JSON reports
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let csv_path = output_dir.join(format!("dspd_report_{}_{}.csv", drive.letter, timestamp));
    write_csv(&csv_path, &rows)?;
    println!("  CSV Report : {}", csv_path.display());

    let json_path = output_dir.join(format!("dspd_report_{}_{}.json", drive.letter, timestamp));
    let json_report = write_json(
        &json_path,
        &rows,
        drive,
        test_size,
        args.passes,
        args.duration,
        args.warmup,
        thermal_report.clone(),
    )?;
    println!("  JSON Report: {}", json_path.display());

    if args.json {
        println!("\n─── JSON Output ───");
        println!("{}", to_json_string(&json_report)?);
    }
    println!();

    Ok(())
}

/// Execute all benchmark passes for a specific profile and transfer direction.
pub fn run_test_passes(
    test_file_path: &Path,
    test_size: u64,
    profile: &Profile,
    is_write: bool,
    rw_mix: Option<RwMix>,
    args: &Args,
) -> Result<PassResult> {
    let mut pass_metrics = Vec::with_capacity(args.passes as usize);

    for _ in 1..=args.passes {
        if is_cancelled() {
            break;
        }

        let pb = ProgressBar::new(args.duration + args.warmup);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{prefix} [{bar:20.green/black}] {pos}/{len}s")?
                .progress_chars("=>-"),
        );
        let prefix = if let Some(mix) = rw_mix {
            format!("    {} Mixed {}:{}", profile.name, mix.read_pct, mix.write_pct)
        } else if is_write {
            format!("    {} Write", profile.name)
        } else {
            format!("    {} Read", profile.name)
        };
        pb.set_prefix(prefix);

        let file_handle = open_benchmark_file(test_file_path)?;
        let mut engine = IocpEngine::new(file_handle, test_size, profile)?;

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_thread = cancel_flag.clone();
        let pb_clone = pb.clone();
        let total_secs = args.duration + args.warmup;

        let ticker = std::thread::spawn(move || {
            for _ in 0..total_secs {
                if cancel_thread.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
                pb_clone.inc(1);
            }
        });

        let pass_res = engine.run_pass(
            args.duration + args.warmup,
            args.warmup,
            is_write,
            rw_mix,
            &CANCELLED,
        )?;

        cancel_flag.store(true, Ordering::Relaxed);
        let _ = ticker.join();
        pb.finish_and_clear();

        // SAFETY: Close test file handle after each pass to reset file pointers.
        unsafe {
            let _ = CloseHandle(file_handle);
        }

        pass_metrics.push(pass_res);
    }

    average_passes(&pass_metrics)
}

/// Execute sustained sequential write profiling pass on a drive.
pub fn benchmark_drive_sustained(
    drive: &DriveInfo,
    args: &Args,
    output_dir: &Path,
) -> Result<()> {
    if is_cancelled() {
        return Ok(());
    }

    // Determine target size
    let target_size = if let Some(ref s) = args.sustained {
        parse_sustained_size(s)?
    } else if args.sustained_duration.is_some() {
        crate::parse_size(&args.size)?
    } else {
        // Default 64 GiB if unspecified
        64 * 1024 * 1024 * 1024
    };

    let test_size_gib = target_size as f64 / (1024.0 * 1024.0 * 1024.0);

    // Max duration: user specified or automatic based on minimum 50 MB/s speed (clamped 180s..7200s)
    let max_duration_secs = args
        .sustained_duration
        .unwrap_or_else(|| (target_size / (50 * 1024 * 1024)).clamp(180, 7200));

    println!("────────────────────────────────────────────────────────────────────────");
    println!(
        "Drive {}: {} [{}] — Sustained Write Profiler",
        drive.letter, drive.model, drive.media_type
    );
    println!(
        "Bus Type: {} | Target Size: {:.1} GiB | Max Duration: {}s",
        drive.bus_type, test_size_gib, max_duration_secs
    );
    println!("────────────────────────────────────────────────────────────────────────");

    // Verify free space requirement (target size + 10%)
    let min_required = (target_size as f64 * 1.1) as u64;
    if drive.free < min_required {
        eprintln!(
            "  [WARN] Drive {}: has insufficient free space ({:.2} GiB free, need {:.2} GiB). Skipping drive.\n",
            drive.letter,
            drive.free as f64 / (1024.0 * 1024.0 * 1024.0),
            min_required as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        return Ok(());
    }

    let test_file_path = resolve_test_file_path(drive.letter);
    let _file_guard = FileGuard {
        path: test_file_path.clone(),
    };

    // Start background NVMe thermal telemetry polling if supported
    let thermal_monitor = NvmeThermalMonitor::start(drive.letter);

    println!(
        "  Allocating test file: {} ({:.1} GiB)...",
        test_file_path.display(),
        test_size_gib
    );
    prepare_test_file(&test_file_path, target_size)?;

    let file_handle = open_benchmark_file(&test_file_path)?;

    let pb = ProgressBar::new(target_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  Sustained Write [{bar:25.green/black}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})")?
            .progress_chars("=>-"),
    );

    let pb_clone = pb.clone();
    let buckets = run_sustained_write_iocp(
        file_handle,
        target_size,
        max_duration_secs,
        &CANCELLED,
        move |cum_bytes, _mb_s| {
            pb_clone.set_position(cum_bytes);
        },
    )?;

    pb.finish_and_clear();

    // Close test file handle
    unsafe {
        let _ = CloseHandle(file_handle);
    }

    // Stop background thermal monitor and aggregate results
    let thermal_report = thermal_monitor.map(|m| m.stop());

    // Analyze time-series telemetry buckets
    let analysis = analyze_sustained_buckets(&buckets);

    // Print summary and Unicode sparkline to console
    print_sustained_summary(drive, &analysis, thermal_report.as_ref());

    // Export reports
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let csv_path = output_dir.join(format!(
        "dspd_sustained_{}_{}.csv",
        drive.letter, timestamp
    ));
    write_sustained_csv(&csv_path, &buckets)?;
    println!("  CSV Time-Series : {}", csv_path.display());

    let json_path = output_dir.join(format!(
        "dspd_sustained_{}_{}.json",
        drive.letter, timestamp
    ));
    let json_report = write_sustained_json(
        &json_path,
        drive,
        target_size,
        max_duration_secs,
        &analysis,
        &buckets,
        thermal_report.clone(),
    )?;
    println!("  JSON Report     : {}", json_path.display());

    if args.json {
        println!("\n─── JSON Output ───");
        println!("{}", to_json_string(&json_report)?);
    }
    println!();

    Ok(())
}
