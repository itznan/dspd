// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Test execution runner, pass orchestration, and benchmark execution.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::Args;
use crate::drives::space::VolumeHandleGuard;
use crate::drives::{DriveInfo, NvmeThermalMonitor};
use crate::engine::cpu::{apply_thread_affinity, parse_affinity_mask, CpuMetrics};
use crate::engine::iocp::IocpEngine;
use crate::engine::ioring::IoRingEngine;
use crate::engine::profile::{Profile, RwMix};
use crate::engine::stats::{average_passes, PassResult};
use crate::engine::sustained::{analyze_sustained_buckets, run_sustained_write_iocp};
use crate::file::{
    open_benchmark_file, preallocate_test_file, prepare_test_file, resolve_test_file_path,
    FileGuard,
};
use crate::report::csv::{write_csv, write_sustained_csv};
use crate::report::json::{to_json_string, write_json, write_sustained_json};
use crate::report::ReportRow;
use crate::signal::{is_cancelled, CANCELLED};
use crate::ui::{
    format_latency, format_latency_u64, print_summary_tables, print_sustained_summary,
};

fn check_free_space(drive: &DriveInfo, required_bytes: u64) -> bool {
    let min_required = (required_bytes as f64 * 1.1) as u64;
    if drive.free < min_required {
        eprintln!(
            "  [WARN] Drive {}: has insufficient free space ({:.2} GiB free, need {:.2} GiB). Skipping drive.\n",
            drive.letter,
            drive.free as f64 / (1024.0 * 1024.0 * 1024.0),
            min_required as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        return false;
    }
    true
}

fn setup_affinity(affinity: Option<&str>) {
    if let Some(aff) = affinity {
        match parse_affinity_mask(aff) {
            Ok(mask) => {
                if let Err(e) = apply_thread_affinity(mask) {
                    eprintln!("  [WARN] Failed to apply thread affinity: {}", e);
                } else {
                    println!("  Thread Affinity: Mask 0x{:X} (Priority: HIGHEST)", mask);
                }
            }
            Err(e) => {
                eprintln!("  [WARN] Invalid affinity specification '{}': {}", aff, e);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_and_print_stage(
    rows: &mut Vec<ReportRow>,
    profile_name: &str,
    label: &str,
    direction: &str,
    mb_s: f64,
    iops: f64,
    avg_lat_us: f64,
    p50_lat_us: u64,
    p99_lat_us: u64,
    p999_lat_us: u64,
    cpu: Option<&CpuMetrics>,
    cpu_label: Option<&str>,
) {
    println!(
        "  [{}] {:<11}: {:>8.2} MB/s | {:>8.0} IOPS | p50: {:>7} | p99: {:>7} | p99.9: {:>7}",
        profile_name,
        label,
        mb_s,
        iops,
        format_latency_u64(p50_lat_us),
        format_latency_u64(p99_lat_us),
        format_latency_u64(p999_lat_us)
    );
    if let (Some(cpu_metrics), Some(c_label)) = (cpu, cpu_label) {
        println!(
            "  [{}] {}: {:>5.1}% CPU Util | {:>6} Cycles/IO | {:>8.0} IOPS/Core",
            profile_name,
            c_label,
            cpu_metrics.cpu_util_pct,
            cpu_metrics.cycles_per_io,
            cpu_metrics.iops_per_core
        );
    }
    rows.push(ReportRow {
        profile: profile_name.to_string(),
        direction: direction.to_string(),
        mb_s,
        iops,
        p50_lat_us,
        avg_lat_us,
        p99_lat_us,
        p999_lat_us,
        cpu_util_pct: cpu.map(|c| c.cpu_util_pct),
        cycles_per_io: cpu.map(|c| c.cycles_per_io),
        iops_per_core: cpu.map(|c| c.iops_per_core),
    });
}

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

    if !check_free_space(drive, test_size) {
        return Ok(());
    }

    let test_file_path = resolve_test_file_path(drive.letter)?;

    println!(
        "  Test File: {} ({:.2} GiB)",
        test_file_path.display(),
        test_size_gib
    );
    println!(
        "  Passes   : {} pass(es) x {}s (warmup: {}s)",
        args.passes, args.duration, args.warmup
    );

    setup_affinity(args.affinity.as_deref());

    // Report and validate storage engine
    let requested_ioring = args.engine.eq_ignore_ascii_case("ioring");
    if requested_ioring {
        if IoRingEngine::is_available() {
            println!("  Engine   : Windows 11 IoRing (pre-registered handles & buffers)");
            if !IoRingEngine::supports_writes() {
                println!("  [WARN] IoRing write support unavailable on this OS build; write stages will use IOCP.");
            }
            println!();
        } else {
            println!("  [WARN] Windows 11 IoRing is not available on this OS build. Falling back to IOCP.");
            println!("  Engine   : IOCP (I/O Completion Ports)\n");
        }
    } else {
        println!("  Engine   : IOCP (I/O Completion Ports)\n");
    }

    let _file_guard = FileGuard {
        path: test_file_path.clone(),
    };

    // Start background NVMe thermal telemetry polling if supported
    let thermal_monitor = NvmeThermalMonitor::start(drive.letter);

    // Prepare test file with pseudo-random content
    prepare_test_file(&test_file_path, test_size)?;

    let mut rows: Vec<ReportRow> = Vec::new();
    let parsed_mix = args.rw_mix.as_deref().map(RwMix::parse).transpose()?;

    for profile in profiles {
        if is_cancelled() {
            break;
        }

        let effective_mix = parsed_mix.or(profile.default_rw_mix);

        if let Some(mix) = effective_mix.filter(|m| m.is_mixed()) {
            let mixed_avg =
                run_test_passes(&test_file_path, test_size, profile, false, Some(mix), args)?;
            if is_cancelled() {
                break;
            }
            push_and_print_stage(
                &mut rows,
                profile.name,
                &format!("Read ({:>2}%)", mix.read_pct),
                "Read",
                mixed_avg.read_mb_s(),
                mixed_avg.read_iops(),
                mixed_avg.read_avg_us(),
                mixed_avg.read_p50_us(),
                mixed_avg.read_p99_us(),
                mixed_avg.read_p999_us(),
                None,
                None,
            );
            push_and_print_stage(
                &mut rows,
                profile.name,
                &format!("Write({:>2}%)", mix.write_pct),
                "Write",
                mixed_avg.write_mb_s(),
                mixed_avg.write_iops(),
                mixed_avg.write_avg_us(),
                mixed_avg.write_p50_us(),
                mixed_avg.write_p99_us(),
                mixed_avg.write_p999_us(),
                mixed_avg.cpu.as_ref(),
                Some("CPU Efficiency"),
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
        } else if profile.is_read_only || effective_mix.is_some() {
            let is_write = effective_mix.is_some_and(|m| m.write_pct == 100);
            let dir = if is_write { "Write" } else { "Read" };
            let avg = run_test_passes(&test_file_path, test_size, profile, is_write, None, args)?;
            if is_cancelled() {
                break;
            }
            push_and_print_stage(
                &mut rows,
                profile.name,
                dir,
                dir,
                avg.throughput_mb_s(),
                avg.iops(profile.block_size),
                avg.avg_latency_us(),
                avg.p50_latency_us(),
                avg.p99_latency_us(),
                avg.p999_latency_us(),
                avg.cpu.as_ref(),
                Some("CPU Efficiency"),
            );
            if profile.is_llm_stream && !is_write {
                let mb_s = avg.throughput_mb_s();
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
        } else {
            // Direction 1: READ
            let read_avg = run_test_passes(&test_file_path, test_size, profile, false, None, args)?;
            if is_cancelled() {
                break;
            }
            push_and_print_stage(
                &mut rows,
                profile.name,
                "Read",
                "Read",
                read_avg.throughput_mb_s(),
                read_avg.iops(profile.block_size),
                read_avg.avg_latency_us(),
                read_avg.p50_latency_us(),
                read_avg.p99_latency_us(),
                read_avg.p999_latency_us(),
                read_avg.cpu.as_ref(),
                Some("CPU (Read) "),
            );

            if let Some(ref monitor) = thermal_monitor {
                monitor.sample();
            }

            // Direction 2: WRITE
            let write_avg = run_test_passes(&test_file_path, test_size, profile, true, None, args)?;
            if is_cancelled() {
                break;
            }
            push_and_print_stage(
                &mut rows,
                profile.name,
                "Write",
                "Write",
                write_avg.throughput_mb_s(),
                write_avg.iops(profile.block_size),
                write_avg.avg_latency_us(),
                write_avg.p50_latency_us(),
                write_avg.p99_latency_us(),
                write_avg.p999_latency_us(),
                write_avg.cpu.as_ref(),
                Some("CPU (Write)"),
            );
            println!();
        }

        if let Some(ref monitor) = thermal_monitor {
            monitor.sample();
        }
    }

    if is_cancelled() {
        println!("  Benchmark cancelled; partial results discarded.\n");
        return Ok(());
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

struct TickerGuard {
    tx: Option<mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
    pb: ProgressBar,
}

impl Drop for TickerGuard {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.pb.finish_and_clear();
    }
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
    let needs_write = is_write
        || rw_mix
            .or(profile.default_rw_mix)
            .is_some_and(|m| m.write_pct > 0);

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
            format!(
                "    {} Mixed {}:{}",
                profile.name, mix.read_pct, mix.write_pct
            )
        } else if is_write {
            format!("    {} Write", profile.name)
        } else {
            format!("    {} Read", profile.name)
        };
        pb.set_prefix(prefix);

        let file_guard = VolumeHandleGuard(open_benchmark_file(test_file_path)?);

        let (tx, rx) = mpsc::channel::<()>();
        let pb_clone = pb.clone();
        let total_secs = args.duration + args.warmup;

        let handle = std::thread::spawn(move || {
            for _ in 0..total_secs {
                if rx.recv_timeout(Duration::from_secs(1)).is_ok() || is_cancelled() {
                    break;
                }
                pb_clone.inc(1);
            }
        });
        let _ticker_guard = TickerGuard {
            tx: Some(tx),
            handle: Some(handle),
            pb,
        };

        let use_ioring = args.engine.eq_ignore_ascii_case("ioring")
            && IoRingEngine::is_available()
            && (!needs_write || IoRingEngine::supports_writes());
        let pass_res = if use_ioring {
            let mut engine = IoRingEngine::new(file_guard.0, test_size, profile)?;
            engine.run_pass(
                args.duration + args.warmup,
                args.warmup,
                is_write,
                rw_mix,
                &CANCELLED,
            )?
        } else {
            let mut engine = IocpEngine::new(file_guard.0, test_size, profile)?;
            engine.run_pass(
                args.duration + args.warmup,
                args.warmup,
                is_write,
                rw_mix,
                &CANCELLED,
            )?
        };

        if is_cancelled() {
            break;
        }

        pass_metrics.push(pass_res);
    }

    average_passes(&pass_metrics)
}

/// Execute sustained sequential write profiling pass on a drive.
pub fn benchmark_drive_sustained(
    drive: &DriveInfo,
    target_size: u64,
    args: &Args,
    output_dir: &Path,
) -> Result<()> {
    if is_cancelled() {
        return Ok(());
    }

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

    setup_affinity(args.affinity.as_deref());
    println!("────────────────────────────────────────────────────────────────────────");

    if !check_free_space(drive, target_size) {
        return Ok(());
    }

    let test_file_path = resolve_test_file_path(drive.letter)?;
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
    preallocate_test_file(&test_file_path, target_size)?;

    let file_guard = VolumeHandleGuard(open_benchmark_file(&test_file_path)?);

    let pb = ProgressBar::new(target_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  Sustained Write [{bar:25.green/black}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})")?
            .progress_chars("=>-"),
    );

    let pb_clone = pb.clone();
    let buckets_res = run_sustained_write_iocp(
        file_guard.0,
        target_size,
        max_duration_secs,
        &CANCELLED,
        move |cum_bytes, _mb_s| {
            pb_clone.set_position(cum_bytes);
        },
    );
    pb.finish_and_clear();
    let buckets = buckets_res?;
    drop(file_guard);

    if is_cancelled() {
        println!("  Sustained write benchmark cancelled; partial results discarded.\n");
        return Ok(());
    }

    // Stop background thermal monitor and aggregate results
    let thermal_report = thermal_monitor.map(|m| m.stop());

    // Analyze time-series telemetry buckets
    let analysis = analyze_sustained_buckets(&buckets);

    // Print summary and Unicode sparkline to console
    print_sustained_summary(drive, &analysis, thermal_report.as_ref());

    // Export reports
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let csv_path = output_dir.join(format!("dspd_sustained_{}_{}.csv", drive.letter, timestamp));
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
