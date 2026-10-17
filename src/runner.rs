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
use crate::drives::DriveInfo;
use crate::engine::iocp::IocpEngine;
use crate::engine::profile::Profile;
use crate::engine::stats::{average_passes, PassResult};
use crate::file::{open_benchmark_file, prepare_test_file, resolve_test_file_path, FileGuard};
use crate::report::csv::write_csv;
use crate::report::json::{to_json_string, write_json};
use crate::report::ReportRow;
use crate::signal::{is_cancelled, CANCELLED};
use crate::ui::{format_latency, format_latency_u64, print_summary_tables};

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

    // Prepare test file with pseudo-random content
    prepare_test_file(&test_file_path, test_size)?;

    let mut rows: Vec<ReportRow> = Vec::new();

    for profile in profiles {
        if is_cancelled() {
            break;
        }

        // Direction 1: READ
        let read_avg = run_test_passes(&test_file_path, test_size, profile, false, args)?;
        println!(
            "  [{}] Read : {:>8.2} MB/s | {:>8.0} IOPS | Avg: {:>7} | p99: {:>7}",
            profile.name,
            read_avg.throughput_mb_s(),
            read_avg.iops(profile.block_size),
            format_latency(read_avg.avg_latency_us()),
            format_latency_u64(read_avg.p99_latency_us())
        );
        rows.push(ReportRow {
            profile: profile.name.to_string(),
            direction: "Read".to_string(),
            mb_s: read_avg.throughput_mb_s(),
            iops: read_avg.iops(profile.block_size),
            avg_lat_us: read_avg.avg_latency_us(),
            p99_lat_us: read_avg.p99_latency_us(),
        });

        // Direction 2: WRITE
        if is_cancelled() {
            break;
        }
        let write_avg = run_test_passes(&test_file_path, test_size, profile, true, args)?;
        println!(
            "  [{}] Write: {:>8.2} MB/s | {:>8.0} IOPS | Avg: {:>7} | p99: {:>7}",
            profile.name,
            write_avg.throughput_mb_s(),
            write_avg.iops(profile.block_size),
            format_latency(write_avg.avg_latency_us()),
            format_latency_u64(write_avg.p99_latency_us())
        );
        rows.push(ReportRow {
            profile: profile.name.to_string(),
            direction: "Write".to_string(),
            mb_s: write_avg.throughput_mb_s(),
            iops: write_avg.iops(profile.block_size),
            avg_lat_us: write_avg.avg_latency_us(),
            p99_lat_us: write_avg.p99_latency_us(),
        });
        println!();
    }

    // Print summary performance tables in console
    print_summary_tables(&rows, drive);

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
        pb.set_prefix(format!(
            "    {} {}",
            profile.name,
            if is_write { "Write" } else { "Read" }
        ));

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
