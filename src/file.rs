// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Benchmark file lifecycle management, RAII cleanup guards, and pre-allocation.

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::IntoRawHandle;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    SetEndOfFile, SetFilePointerEx, FILE_BEGIN, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED,
    FILE_FLAG_WRITE_THROUGH,
};

use crate::drives::space::VolumeHandleGuard;
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

/// Determine the benchmark file path on the specified drive.
/// Only falls back to temp dir if temp dir resides on the exact same drive letter.
pub fn resolve_test_file_path(letter: char) -> Result<PathBuf> {
    let upper = letter.to_ascii_uppercase();
    let filename = format!("dspd_test_{upper}_{}.dat", std::process::id());
    let candidate_path = PathBuf::from(format!(r"{upper}:\{filename}"));

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&candidate_path)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&candidate_path);
            Ok(candidate_path)
        }
        Err(root_err) => {
            let temp_dir = std::env::temp_dir();
            let temp_str = temp_dir.to_string_lossy();
            let same_drive = temp_str.as_bytes().get(1) == Some(&b':')
                && temp_str
                    .chars()
                    .next()
                    .is_some_and(|c| c.eq_ignore_ascii_case(&upper));

            if same_drive {
                let temp_candidate = temp_dir.join(&filename);
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temp_candidate)
                    .with_context(|| {
                        format!(
                            "Drive {upper}: is not writable at root ({root_err}) or temp dir '{}'",
                            temp_dir.display()
                        )
                    })?;
                let _ = std::fs::remove_file(&temp_candidate);
                Ok(temp_candidate)
            } else {
                anyhow::bail!(
                    "Cannot create benchmark test file on drive {upper}: ({root_err}). Run as Administrator or verify write permissions."
                );
            }
        }
    }
}

/// Pre-allocate a sustained test file to `size` bytes without pre-writing flash cells.
pub fn preallocate_test_file(path: &Path, size: u64) -> Result<()> {
    let file_size = i64::try_from(size).context("Test file size exceeds i64 range")?;
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

    let guard = VolumeHandleGuard(HANDLE(file.into_raw_handle() as _));
    unsafe {
        SetFilePointerEx(guard.0, file_size, None, FILE_BEGIN)?;
        SetEndOfFile(guard.0)?;
    }
    Ok(())
}

/// Prepare the test file by pre-allocating and writing pseudo-random blocks sequentially.
pub fn prepare_test_file(path: &Path, size: u64) -> Result<()> {
    const CHUNK_SIZE: usize = 1024 * 1024; // 1 MiB write chunks
    let num_chunks = size.div_ceil(CHUNK_SIZE as u64);

    let pb = ProgressBar::new(num_chunks);
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
            pb.finish_and_clear();
            format!(
                "Failed to create benchmark test file at '{}'",
                path.display()
            )
        })?;

    let _guard = VolumeHandleGuard(HANDLE(file.into_raw_handle() as _));
    let mut remaining = size;

    while remaining > 0 {
        if is_cancelled() {
            pb.finish_and_clear();
            anyhow::bail!("Benchmark cancelled by user");
        }

        let write_len = remaining.min(CHUNK_SIZE as u64).div_ceil(4096) * 4096;
        // SAFETY: Direct synchronous sector-aligned write within CHUNK_SIZE buffer.
        let res = unsafe {
            let slice = std::slice::from_raw_parts(chunk_buffer.as_ptr(), write_len as usize);
            windows::Win32::Storage::FileSystem::WriteFile(_guard.0, Some(slice), None, None)
        };
        if let Err(e) = res {
            pb.finish_and_clear();
            return Err(e.into());
        }
        remaining = remaining.saturating_sub(CHUNK_SIZE as u64);
        pb.inc(1);
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
