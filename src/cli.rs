// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Command line argument parsing and definitions.

use clap::Parser;
use std::path::PathBuf;

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
