// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::engine::stats::{average_passes, PassResult};
use hdrhistogram::Histogram;

#[test]
fn test_histogram_p99_and_mean() {
    let mut hist = Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3).unwrap();

    // Record sample latencies from 1 to 100 µs
    for v in 1..=100u64 {
        hist.record(v).unwrap();
    }

    assert_eq!(hist.len(), 100);
    assert!((hist.mean() - 50.5).abs() < 1.0);
    assert!(hist.value_at_quantile(0.99) >= 98);
}

#[test]
fn test_average_passes_preserves_small_counts() {
    let mut passes = Vec::new();
    for _ in 0..3 {
        let mut p = PassResult::new().unwrap();
        p.elapsed_ns = 1_000_000_000;
        p.record_write(8192, 500);
        p.record_flush(1200);
        p.record_flush(1400);
        passes.push(p);
    }

    let avg = average_passes(&passes).unwrap();
    assert_eq!(avg.flush_count, 2);
    assert_eq!(avg.write_ios, 1);
    assert!((avg.avg_flush_us() - 1300.0).abs() < 1.0);
}
