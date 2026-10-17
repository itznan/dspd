// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

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
