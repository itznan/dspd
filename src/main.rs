// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Main entry point for dspd: Fast, safe disk benchmarking CLI for Windows.

use anyhow::{Context, Result};
use clap::Parser;

use dspd::cli::{Args, VERSION};
use dspd::drives;
use dspd::engine::profile::{Profile, ALL_PROFILES};
use dspd::parse_size;
use dspd::report::get_desktop_dir;
use dspd::runner::benchmark_drive;
use dspd::signal::{is_cancelled, register_ctrl_handler};
use dspd::ui::{print_validation_commands, prompt_ssd_wear};

fn main() -> Result<()> {
    println!("dspd v{} by itznan (github.com/itznan/dspd)\n", VERSION);

    let args = Args::parse();
    register_ctrl_handler();

    if args.validate {
        print_validation_commands(args.duration, args.warmup);
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
            .with_context(|| {
                format!(
                    "Unknown profile '{}'. Valid profiles: SEQ1M Q8T1, SEQ1M Q1T1, RND4K Q32T1, RND4K Q1T1",
                    prof_name
                )
            })?]
    } else {
        ALL_PROFILES.to_vec()
    };

    // SSD Wear Confirmation Prompt before performing write benchmarks
    if !args.yes && !prompt_ssd_wear(test_size_gib, profiles_to_run.len(), args.passes)? {
        println!("Benchmark aborted by user.");
        return Ok(());
    }
    println!();

    let output_directory = args.output_dir.clone().unwrap_or_else(get_desktop_dir);
    std::fs::create_dir_all(&output_directory).with_context(|| {
        format!(
            "Failed to create output directory '{}'",
            output_directory.display()
        )
    })?;

    // Benchmark each drive
    for drive in &available_drives {
        if is_cancelled() {
            break;
        }
        benchmark_drive(drive, &profiles_to_run, test_size, &args, &output_directory)?;
    }

    Ok(())
}
