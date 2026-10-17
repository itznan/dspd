// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Benchmark file lifecycle management, RAII cleanup guards, and pre-allocation.

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::IntoRawHandle;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_FLAG_WRITE_THROUGH,
};

use crate::engine::buffers::AlignedBuffer;
use crate::signal::is_cancelled;

/// RAII Guard ensuring benchmark files are cleanly deleted on exit, panic, or cancellation.
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

/// Determine the benchmark file path on the specified drive, with fallback to temp directory.
pub fn resolve_test_file_path(letter: char) -> PathBuf {
    let candidate_path = PathBuf::from(format!(r"{letter}:\dspd_test.dat"));
    match OpenOptions::new()
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
            temp_dir.join(format!("dspd_test_{letter}.dat"))
        }
    }
}

/// Prepare the test file by pre-allocating and writing pseudo-random blocks sequentially.
pub fn prepare_test_file(path: &Path, size: u64) -> Result<()> {
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
        if is_cancelled() {
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

/// Open an existing benchmark test file with direct, unbuffered, asynchronous flags.
pub fn open_benchmark_file(path: &Path) -> Result<HANDLE> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_NO_BUFFERING.0 | FILE_FLAG_WRITE_THROUGH.0 | FILE_FLAG_OVERLAPPED.0)
        .open(path)
        .with_context(|| format!("Failed to open benchmark test file at '{}'", path.display()))?;

    Ok(HANDLE(file.into_raw_handle() as _))
}
