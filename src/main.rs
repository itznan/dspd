// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use std::fs::OpenOptions;
use std::io::{stdin, stdout, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::IntoRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_FLAG_WRITE_THROUGH,
};
use windows::Win32::System::Console::SetConsoleCtrlHandler;

use dspd::drives::{self, DriveInfo};
use dspd::engine::buffers::AlignedBuffer;
use dspd::engine::iocp::IocpEngine;
use dspd::engine::profile::{Profile, ALL_PROFILES};
use dspd::engine::stats::average_passes;
use dspd::parse_size;
use dspd::report::csv::write_csv;
use dspd::report::html::write_html;
use dspd::report::{get_desktop_dir, ReportRow};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Parser)]
#[command(
    name    = "dspd",
    about   = "Fast, safe, single-binary disk benchmarking for Windows, written in Rust.",
    version = VERSION,
)]
pub struct Args {
    /// Drive letters to benchmark, comma-separated (e.g. C,D)
    #[arg(long, value_delimiter = ',')]
    pub drives: Vec<char>,

    /// Test file size (e.g. 1GiB, 512MiB, 4GiB)
    #[arg(long, default_value = "1GiB")]
    pub size: String,

    /// Number of passes per test
    #[arg(long, default_value_t = 3)]
    pub passes: u32,

    /// Duration of each pass in seconds
    #[arg(long, default_value_t = 5)]
    pub duration: u64,

    /// Warmup duration in seconds (discarded from metrics)
    #[arg(long, default_value_t = 1)]
    pub warmup: u64,

    /// Include removable drives (e.g. USB drives)
    #[arg(long)]
    pub include_removable: bool,

    /// Run a single named profile only (e.g. "RND4K Q32T1")
    #[arg(long)]
    pub profile: Option<String>,

    /// Output directory for reports (default: Desktop)
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Skip HTML report generation
    #[arg(long)]
    pub no_html: bool,

    /// Skip SSD wear confirmation prompt
    #[arg(long)]
    pub yes: bool,

    /// Print equivalent DiskSpd command lines for each profile
    #[arg(long)]
    pub validate: bool,
}

static CANCELLED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn ctrl_handler(_: u32) -> windows::Win32::Foundation::BOOL {
    CANCELLED.store(true, Ordering::SeqCst);
    eprintln!("\n[!] Ctrl+C detected. Cleaning up benchmark files safely...");
    windows::Win32::Foundation::TRUE
}

/// RAII Guard ensuring benchmark files are cleanly deleted on exit or panic.
pub struct FileGuard {
    pub path: PathBuf,
}

impl Drop for FileGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Prepare the test file by pre-allocating and writing pseudo-random blocks sequentially.
fn prepare_test_file(path: &Path, size: u64, cancelled: &AtomicBool) -> Result<()> {
    const CHUNK_SIZE: usize = 1024 * 1024; // 1 MiB write chunks
    let num_chunks = (size as usize).div_ceil(CHUNK_SIZE);

    let pb = ProgressBar::new(num_chunks as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{prefix} [{elapsed_precise}] [{bar:30.cyan/blue}] {pos}/{len} MiB")?
            .progress_chars("#>-"),
    );
    pb.set_prefix("  Preparing test file");

    let chunk_buffer = AlignedBuffer::new_random(CHUNK_SIZE)?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(FILE_FLAG_NO_BUFFERING.0 | FILE_FLAG_WRITE_THROUGH.0)
        .open(path)
        .with_context(|| {
            format!(
                "Failed to create benchmark test file at '{}'",
                path.display()
            )
        })?;

    let raw_handle = HANDLE(file.into_raw_handle() as _);

    for _ in 0..num_chunks {
        if cancelled.load(Ordering::Relaxed) {
            unsafe {
                let _ = CloseHandle(raw_handle);
            }
            anyhow::bail!("Benchmark cancelled by user");
        }

        // SAFETY: Direct synchronous sector-aligned write
        unsafe {
            let slice = std::slice::from_raw_parts(chunk_buffer.as_ptr(), CHUNK_SIZE);
            windows::Win32::Storage::FileSystem::WriteFile(raw_handle, Some(slice), None, None)?;
        }
        pb.inc(1);
    }

    unsafe {
        let _ = CloseHandle(raw_handle);
    }
    pb.finish_and_clear();
    Ok(())
}

fn open_benchmark_file(path: &Path) -> Result<HANDLE> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_NO_BUFFERING.0 | FILE_FLAG_WRITE_THROUGH.0 | FILE_FLAG_OVERLAPPED.0)
        .open(path)
        .with_context(|| format!("Failed to open benchmark test file at '{}'", path.display()))?;

    Ok(HANDLE(file.into_raw_handle() as _))
}

fn format_latency(us: f64) -> String {
    if us >= 1000.0 {
        format!("{:.2} ms", us / 1000.0)
    } else {
        format!("{:.1} µs", us)
    }
}

fn format_latency_u64(us: u64) -> String {
    if us >= 1000 {
        format!("{:.2} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}

fn main() -> Result<()> {
    println!("dspd v{} by itznan (github.com/itznan/dspd)\n", VERSION);

    let args = Args::parse();

    // Register Win32 console signal handler
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), windows::Win32::Foundation::TRUE);
    }

    if args.validate {
        println!("Equivalent DiskSpd command lines for validation:\n");
        for p in ALL_PROFILES {
            println!(
                "  {}",
                p.diskspd_cmd(args.duration, args.warmup, false, "<file>")
            );
            println!(
                "  {}",
                p.diskspd_cmd(args.duration, args.warmup, true, "<file>")
            );
        }
        return Ok(());
    }

    let test_size = parse_size(&args.size)?;
    let test_size_gib = test_size as f64 / (1024.0 * 1024.0 * 1024.0);

    // Discover eligible drives
    let mut available_drives = drives::discover(args.include_removable);
    if available_drives.is_empty() {
        anyhow::bail!("No supported drives found for benchmarking.");
    }

    // Filter to requested drives if specified
    if !args.drives.is_empty() {
        available_drives.retain(|d| {
            args.drives
                .iter()
                .any(|&req| req.eq_ignore_ascii_case(&d.letter))
        });
        if available_drives.is_empty() {
            anyhow::bail!("None of the specified drive letters were found.");
        }
    }

    // Select profiles
    let profiles_to_run: Vec<Profile> = if let Some(ref prof_name) = args.profile {
        vec![ALL_PROFILES
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(prof_name))
            .cloned()
            .with_context(|| format!("Unknown profile '{}'. Valid profiles: SEQ1M Q8T1, SEQ1M Q1T1, RND4K Q32T1, RND4K Q1T1", prof_name))?]
    } else {
        ALL_PROFILES.to_vec()
    };

    // SSD Wear Confirmation Prompt before performing write benchmarks
    if !args.yes {
        println!(
            "  [!] NOTICE: Benchmarking drives writes unbuffered data streams to flash cells."
        );
        println!(
            "      Estimated data write per drive: ~{:.2} GiB ({} pass(es)).",
            test_size_gib * (profiles_to_run.len() as f64 * args.passes as f64 + 1.0),
            args.passes
        );
        print!("  Do you want to continue? [y/N]: ");
        stdout().flush()?;

        let mut input = String::new();
        stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            println!("Benchmark aborted by user.");
            return Ok(());
        }
        println!();
    }

    let output_directory = args.output_dir.clone().unwrap_or_else(get_desktop_dir);

    std::fs::create_dir_all(&output_directory).with_context(|| {
        format!(
            "Failed to create output directory '{}'",
            output_directory.display()
        )
    })?;

    // Benchmark each drive
    for drive in &available_drives {
        if CANCELLED.load(Ordering::Relaxed) {
            break;
        }

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
            continue;
        }

        let candidate_path = PathBuf::from(format!(r"{}:\dspd_test.dat", drive.letter));
        let test_file_path = match OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&candidate_path)
        {
            Ok(_) => {
                let _ = std::fs::remove_file(&candidate_path);
                candidate_path
            }
            Err(_) => {
                let temp_dir = std::env::temp_dir();
                temp_dir.join(format!("dspd_test_{}.dat", drive.letter))
            }
        };

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
        prepare_test_file(&test_file_path, test_size, &CANCELLED)?;

        let mut rows: Vec<ReportRow> = Vec::new();

        for profile in &profiles_to_run {
            if CANCELLED.load(Ordering::Relaxed) {
                break;
            }

            // Direction 1: READ
            let read_avg = run_test_passes(&test_file_path, test_size, profile, false, &args)?;
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
            if CANCELLED.load(Ordering::Relaxed) {
                break;
            }
            let write_avg = run_test_passes(&test_file_path, test_size, profile, true, &args)?;
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

        // Print CrystalDiskMark layout table in console
        print_summary_tables(&rows, drive);

        // Export CSV and HTML reports
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let csv_path =
            output_directory.join(format!("dspd_report_{}_{}.csv", drive.letter, timestamp));
        write_csv(&csv_path, &rows)?;
        println!("  CSV Report : {}", csv_path.display());

        if !args.no_html {
            let html_path =
                output_directory.join(format!("dspd_report_{}_{}.html", drive.letter, timestamp));
            write_html(&html_path, &rows, drive, test_size)?;
            println!("  HTML Report: {}", html_path.display());
        }
        println!();
    }

    Ok(())
}

fn run_test_passes(
    test_file_path: &Path,
    test_size: u64,
    profile: &Profile,
    is_write: bool,
    args: &Args,
) -> Result<dspd::engine::stats::PassResult> {
    let mut pass_metrics = Vec::with_capacity(args.passes as usize);

    for _ in 1..=args.passes {
        if CANCELLED.load(Ordering::Relaxed) {
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

        unsafe {
            let _ = CloseHandle(file_handle);
        }

        pass_metrics.push(pass_res);
    }

    average_passes(&pass_metrics)
}

fn print_summary_tables(rows: &[ReportRow], drive: &DriveInfo) {
    println!("════════════════════════════════════════════════════════════════════════");
    println!(
        "  CrystalDiskMark 9 Summary — Drive {}: ({})",
        drive.letter, drive.model
    );
    println!("────────────────────────────────────────────────────────────────────────");
    println!("  Profile        │ Read (MB/s) │ Write (MB/s) │ Read IOPS │ Write IOPS");
    println!("────────────────────────────────────────────────────────────────────────");

    let profiles = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];

    for prof in profiles {
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
    for prof in profiles {
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
