// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::engine::cpu::{apply_thread_affinity, parse_affinity_mask, CpuMetrics, CpuSnapshot};

#[test]
fn test_parse_affinity_mask_hex() {
    assert_eq!(parse_affinity_mask("0x1").unwrap(), 1);
    assert_eq!(parse_affinity_mask("0xF").unwrap(), 15);
    assert_eq!(parse_affinity_mask("0x55").unwrap(), 0x55);
    assert_eq!(parse_affinity_mask("0XAA").unwrap(), 0xAA);
}

#[test]
fn test_parse_affinity_mask_indices_and_ranges() {
    // Single core
    assert_eq!(parse_affinity_mask("0").unwrap(), 1);
    assert_eq!(parse_affinity_mask("3").unwrap(), 1 << 3);

    // Comma-separated list
    assert_eq!(
        parse_affinity_mask("0,2,4").unwrap(),
        (1 << 0) | (1 << 2) | (1 << 4)
    );

    // Ranges
    assert_eq!(parse_affinity_mask("0-3").unwrap(), 0xF);
    assert_eq!(
        parse_affinity_mask("2-5").unwrap(),
        (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5)
    );

    // Combined ranges and indices with whitespace
    assert_eq!(
        parse_affinity_mask(" 0, 2-3, 5 ").unwrap(),
        (1 << 0) | (1 << 2) | (1 << 3) | (1 << 5)
    );
}

#[test]
fn test_parse_affinity_mask_invalid() {
    assert!(parse_affinity_mask("").is_err());
    assert!(parse_affinity_mask("   ").is_err());
    assert!(parse_affinity_mask("0x").is_err());
    assert!(parse_affinity_mask("0xGHI").is_err());
    assert!(parse_affinity_mask("abc").is_err());
    assert!(parse_affinity_mask("4-2").is_err()); // inverted range
    assert!(parse_affinity_mask("999").is_err()); // exceeds bitwidth
}

#[test]
fn test_cpu_snapshot_and_metrics_calculation() {
    let before = CpuSnapshot::take();
    let after = CpuSnapshot {
        user_time_100ns: before.user_time_100ns + 5_000_000, // 500ms user time
        kernel_time_100ns: before.kernel_time_100ns + 2_000_000, // 200ms kernel time
        cycle_time: before.cycle_time + 10_000_000,          // 10M cycles
    };

    let total_ios = 2_000;
    let elapsed_ns = 1_000_000_000; // 1 second

    let metrics = CpuMetrics::compute(&before, &after, total_ios, elapsed_ns);

    assert!(metrics.cpu_util_pct > 0.0);
    assert_eq!(metrics.cycles_per_io, 10_000_000 / 2_000); // 5000 cycles / IO
    assert!(metrics.iops_per_core > 0.0);

    // Edge cases: zero I/Os or zero elapsed time should not panic
    let zero_ios = CpuMetrics::compute(&before, &after, 0, elapsed_ns);
    assert_eq!(zero_ios.cycles_per_io, 0);

    let zero_time = CpuMetrics::compute(&before, &after, total_ios, 0);
    assert_eq!(zero_time.cpu_util_pct, 0.0);
}

#[test]
fn test_apply_thread_affinity_execution() {
    // Pin to core 0 and verify call succeeds without error
    let res = apply_thread_affinity(1);
    assert!(
        res.is_ok(),
        "apply_thread_affinity(1) failed: {:?}",
        res.err()
    );
}
