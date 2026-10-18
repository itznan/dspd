// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! CPU cycle accounting, process telemetry, and worker thread affinity controls.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{BOOL, FILETIME, HANDLE};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, GetProcessTimes, SetThreadAffinityMask, SetThreadPriority,
    THREAD_PRIORITY_HIGHEST,
};

#[link(name = "kernel32")]
extern "system" {
    fn QueryProcessCycleTime(ProcessHandle: HANDLE, CycleTime: *mut u64) -> BOOL;
}

/// CPU telemetry snapshot taken at the start or end of the active measurement window.
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuSnapshot {
    pub user_time_100ns: u64,
    pub kernel_time_100ns: u64,
    pub cycle_time: u64,
}

impl CpuSnapshot {
    /// Take an instantaneous snapshot of process times and CPU cycle counter.
    pub fn take() -> Self {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let mut cycle_time = 0u64;

        unsafe {
            let process = GetCurrentProcess();
            let _ = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
            let _ = QueryProcessCycleTime(process, &mut cycle_time);
        }

        let user_time_100ns = ((user.dwHighDateTime as u64) << 32) | (user.dwLowDateTime as u64);
        let kernel_time_100ns =
            ((kernel.dwHighDateTime as u64) << 32) | (kernel.dwLowDateTime as u64);

        Self {
            user_time_100ns,
            kernel_time_100ns,
            cycle_time,
        }
    }
}

/// Calculated CPU efficiency telemetry for a timed benchmark pass.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct CpuMetrics {
    /// CPU utilization (%) normalized across all logical processors.
    pub cpu_util_pct: f64,
    /// Total CPU cycles consumed per completed I/O operation.
    pub cycles_per_io: u64,
    /// Efficiency score: total IOPS per 1.0 CPU core of compute consumed.
    pub iops_per_core: f64,
}

impl CpuMetrics {
    /// Compute CPU metrics over an active measurement window.
    pub fn compute(
        before: &CpuSnapshot,
        after: &CpuSnapshot,
        total_ios: u64,
        elapsed_ns: u64,
    ) -> Self {
        if elapsed_ns == 0 || total_ios == 0 {
            return Self::default();
        }

        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f64;

        // Process CPU time in nanoseconds (100ns intervals * 100)
        let delta_user_ns = after.user_time_100ns.saturating_sub(before.user_time_100ns) * 100;
        let delta_kernel_ns = after
            .kernel_time_100ns
            .saturating_sub(before.kernel_time_100ns)
            * 100;
        let total_cpu_ns = delta_user_ns + delta_kernel_ns;

        // Normalized CPU utilization (%) across all logical processors
        let total_available_ns = (elapsed_ns as f64) * num_cpus;
        let cpu_util_pct = if total_available_ns > 0.0 {
            (total_cpu_ns as f64 / total_available_ns) * 100.0
        } else {
            0.0
        };

        // CPU cycles per completed I/O
        let delta_cycles = after.cycle_time.saturating_sub(before.cycle_time);
        let cycles_per_io = delta_cycles.checked_div(total_ios).unwrap_or(0);

        // IOPS per Core:
        // Total IOPS divided by effective number of cores utilized by the process
        let elapsed_secs = elapsed_ns as f64 / 1_000_000_000.0;
        let total_iops = total_ios as f64 / elapsed_secs;
        let effective_cores = (cpu_util_pct / 100.0) * num_cpus;
        let iops_per_core = if effective_cores > 0.01 {
            total_iops / effective_cores
        } else {
            total_iops
        };

        Self {
            cpu_util_pct,
            cycles_per_io,
            iops_per_core,
        }
    }
}

/// Parse CPU affinity specification from a hex string (e.g. "0xF", "0x55")
/// or comma-separated list of core indices / ranges (e.g. "0,2,4", "0-3").
pub fn parse_affinity_mask(s: &str) -> Result<usize> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Affinity specification cannot be empty");
    }

    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
        let hex_str = &trimmed[2..];
        usize::from_str_radix(hex_str, 16)
            .map_err(|e| anyhow::anyhow!("Invalid hex affinity mask '{}': {}", trimmed, e))
    } else {
        let mut mask: usize = 0;
        for part in trimmed.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((start_str, end_str)) = part.split_once('-') {
                let start: usize = start_str
                    .trim()
                    .parse()
                    .with_context(|| format!("Invalid core index '{}' in range", start_str))?;
                let end: usize = end_str
                    .trim()
                    .parse()
                    .with_context(|| format!("Invalid core index '{}' in range", end_str))?;
                if start > end {
                    anyhow::bail!("Invalid core range '{}-{}': start > end", start, end);
                }
                for core in start..=end {
                    if core >= usize::BITS as usize {
                        anyhow::bail!(
                            "Core index {} exceeds architecture bitwidth ({})",
                            core,
                            usize::BITS
                        );
                    }
                    mask |= 1 << core;
                }
            } else {
                let core: usize = part
                    .parse()
                    .with_context(|| format!("Invalid core index '{}'", part))?;
                if core >= usize::BITS as usize {
                    anyhow::bail!(
                        "Core index {} exceeds architecture bitwidth ({})",
                        core,
                        usize::BITS
                    );
                }
                mask |= 1 << core;
            }
        }
        if mask == 0 {
            anyhow::bail!("No cores specified in affinity mask '{}'", trimmed);
        }
        Ok(mask)
    }
}

/// Pin the calling thread to the specified CPU affinity mask and elevate thread priority.
pub fn apply_thread_affinity(mask: usize) -> Result<()> {
    unsafe {
        let thread = GetCurrentThread();
        let prev = SetThreadAffinityMask(thread, mask);
        if prev == 0 {
            let err = std::io::Error::last_os_error();
            anyhow::bail!("Failed to set thread affinity mask: {}", err);
        }
        let _ = SetThreadPriority(thread, THREAD_PRIORITY_HIGHEST);
    }
    Ok(())
}
