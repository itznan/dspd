// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Main entry point for dspd: Fast, safe disk benchmarking CLI for Windows.

use anyhow::{Context, Result};
use clap::Parser;

use dspd::cli::{Args, VERSION};
use dspd::drives;
use dspd::engine::profile::{Profile, ALL_AVAILABLE_PROFILES, ALL_PROFILES};
use dspd::report::get_desktop_dir;
use dspd::runner::{benchmark_drive, benchmark_drive_sustained};
use dspd::signal::{is_cancelled, register_ctrl_handler};
use dspd::ui::{print_validation_commands, prompt_ssd_wear};
use dspd::{parse_size, parse_sustained_size};

fn main() -> Result<()> {
    println!("dspd v{} by itznan (github.com/itznan/dspd)\n", VERSION);

    let args = Args::parse();
    register_ctrl_handler();

    if args.validate {
        print_validation_commands(args.duration, args.warmup);
        return Ok(());
    }

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

    let output_directory = args.output_dir.clone().unwrap_or_else(get_desktop_dir);
    std::fs::create_dir_all(&output_directory).with_context(|| {
        format!(
            "Failed to create output directory '{}'",
            output_directory.display()
        )
    })?;

    // Sustained write profiling mode (--sustained <SIZE_GIB> or --sustained-duration <SECS>)
    if args.sustained.is_some() || args.sustained_duration.is_some() {
        let sustained_size = if let Some(ref s) = args.sustained {
            parse_sustained_size(s)?
        } else {
            parse_size(&args.size)?
        };
        let sustained_gib = sustained_size as f64 / (1024.0 * 1024.0 * 1024.0);

        if !args.yes && !prompt_ssd_wear(sustained_gib, 1, 1)? {
            println!("Benchmark aborted by user.");
            return Ok(());
        }
        println!();

        for drive in &available_drives {
            if is_cancelled() {
                break;
            }
            benchmark_drive_sustained(drive, &args, &output_directory)?;
        }

        return Ok(());
    }

    let test_size = parse_size(&args.size)?;
    let test_size_gib = test_size as f64 / (1024.0 * 1024.0 * 1024.0);

    // Select profiles
    let profiles_to_run: Vec<Profile> = if let Some(ref prof_name) = args.profile {
        vec![ALL_AVAILABLE_PROFILES
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(prof_name))
            .cloned()
            .with_context(|| {
                let valid_names: Vec<&str> =
                    ALL_AVAILABLE_PROFILES.iter().map(|p| p.name).collect();
                format!(
                    "Unknown profile '{}'. Valid profiles: {}",
                    prof_name,
                    valid_names.join(", ")
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

    // Benchmark each drive
    for drive in &available_drives {
        if is_cancelled() {
            break;
        }
        benchmark_drive(drive, &profiles_to_run, test_size, &args, &output_directory)?;
    }

    Ok(())
}
