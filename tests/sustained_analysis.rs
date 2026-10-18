// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::drives::{BusType, DriveInfo, MediaType};
use dspd::engine::sustained::{
    analyze_sustained_buckets, render_sparkline, render_sparkline_resampled, SecondBucket,
    SPARK_BLOCKS,
};
use dspd::report::csv::write_sustained_csv;
use dspd::report::json::{to_json_string, write_sustained_json, JsonReport};

#[test]
fn test_sparkline_rendering_levels() {
    let values = vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0];
    let spark = render_sparkline(&values);

    assert_eq!(spark.chars().count(), 8);
    let chars: Vec<char> = spark.chars().collect();
    assert_eq!(chars[0], SPARK_BLOCKS[0]); // ' '
    assert_eq!(chars[7], SPARK_BLOCKS[7]); // '█'
}

#[test]
fn test_sparkline_flat_line() {
    let values = vec![3000.0; 10];
    let spark = render_sparkline(&values);
    assert_eq!(spark, "██████████");
}

#[test]
fn test_sparkline_resampled_downsampling() {
    let values = vec![1000.0; 120];
    let spark = render_sparkline_resampled(&values, 30);
    assert_eq!(spark.chars().count(), 30);
}

#[test]
fn test_slc_cliff_detection_clear_drop() {
    // 5 seconds at ~3000 MB/s (approx 3.0 GB/s), then drops to 900 MB/s for 5 seconds
    let mut buckets = Vec::new();
    let mut cumulative_bytes = 0u64;

    for sec in 1..=5 {
        let bytes = 3_000_000_000u64;
        cumulative_bytes += bytes;
        buckets.push(SecondBucket {
            second: sec,
            bytes,
            io_count: 3000,
            mb_s: 3000.0,
            p99_latency_us: 1200,
            cumulative_gib: cumulative_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        });
    }

    let pre_cliff_gib = buckets[4].cumulative_gib;

    for sec in 6..=10 {
        let bytes = 900_000_000u64;
        cumulative_bytes += bytes;
        buckets.push(SecondBucket {
            second: sec,
            bytes,
            io_count: 900,
            mb_s: 900.0,
            p99_latency_us: 4500,
            cumulative_gib: cumulative_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        });
    }

    let analysis = analyze_sustained_buckets(&buckets);

    // Initial burst speed should be 3000 MB/s
    assert!((analysis.burst_speed_mb_s - 3000.0).abs() < 1e-3);

    // SLC Cliff point should be the cumulative GiB before the drop
    assert!(analysis.slc_cliff_gib.is_some());
    let cliff_gib = analysis.slc_cliff_gib.unwrap();
    assert!((cliff_gib - pre_cliff_gib).abs() < 1e-3);

    // Sustained speed should be 900 MB/s
    assert!(analysis.sustained_speed_mb_s.is_some());
    assert!((analysis.sustained_speed_mb_s.unwrap() - 900.0).abs() < 1e-3);

    // Cliff drop percentage: (3000 - 900) / 3000 = 70%
    assert!(analysis.cliff_drop_percent.is_some());
    assert!((analysis.cliff_drop_percent.unwrap() - 70.0).abs() < 1e-3);

    // Coefficient of variation should be non-zero
    assert!(analysis.cv_percent > 10.0);

    // Sparkline should contain full blocks followed by lower blocks
    assert!(!analysis.sparkline.is_empty());
}

#[test]
fn test_slc_cliff_detection_transient_dip_does_not_trigger() {
    // 10 seconds total, with second 5 dipping to 1200 MB/s, but recovering in sec 6 to 3000 MB/s
    let mut buckets = Vec::new();
    let mut cumulative_bytes = 0u64;

    for sec in 1..=10 {
        let mb_s = if sec == 5 { 1200.0 } else { 3000.0 };
        let bytes = (mb_s * 1_000_000.0) as u64;
        cumulative_bytes += bytes;
        buckets.push(SecondBucket {
            second: sec,
            bytes,
            io_count: 1000,
            mb_s,
            p99_latency_us: 1000,
            cumulative_gib: cumulative_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        });
    }

    let analysis = analyze_sustained_buckets(&buckets);
    // Transient 1-second dip must NOT be falsely identified as a cliff
    assert!(analysis.slc_cliff_gib.is_none());
    assert!(analysis.sustained_speed_mb_s.is_none());
}

#[test]
fn test_slc_cliff_detection_no_exhaustion() {
    let mut buckets = Vec::new();
    let mut cumulative_bytes = 0u64;

    for sec in 1..=8 {
        let bytes = 2_500_000_000u64;
        cumulative_bytes += bytes;
        buckets.push(SecondBucket {
            second: sec,
            bytes,
            io_count: 2500,
            mb_s: 2500.0,
            p99_latency_us: 1100,
            cumulative_gib: cumulative_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        });
    }

    let analysis = analyze_sustained_buckets(&buckets);
    assert_eq!(analysis.burst_speed_mb_s, 2500.0);
    assert!(analysis.slc_cliff_gib.is_none());
    assert!(analysis.sustained_speed_mb_s.is_none());
    assert!(analysis.cv_percent < 0.1);
}

#[test]
fn test_sustained_csv_and_json_serialization() {
    let drive = DriveInfo {
        letter: 'C',
        model: "Crucial T700 2TB".to_string(),
        bus_type: BusType::NVMe,
        media_type: MediaType::SSD,
        capacity: 2_000_398_934_016,
        free: 1_200_000_000_000,
    };

    let buckets = vec![
        SecondBucket {
            second: 1,
            bytes: 3_200_000_000,
            io_count: 3200,
            mb_s: 3200.0,
            p99_latency_us: 900,
            cumulative_gib: 2.98,
        },
        SecondBucket {
            second: 2,
            bytes: 3_150_000_000,
            io_count: 3150,
            mb_s: 3150.0,
            p99_latency_us: 950,
            cumulative_gib: 5.91,
        },
    ];

    let analysis = analyze_sustained_buckets(&buckets);

    let temp_dir = std::env::temp_dir();
    let csv_file = temp_dir.join("test_sustained.csv");
    let json_file = temp_dir.join("test_sustained.json");

    // CSV roundtrip
    write_sustained_csv(&csv_file, &buckets).expect("Failed to write sustained CSV");
    assert!(csv_file.exists());
    let _ = std::fs::remove_file(&csv_file);

    // JSON roundtrip
    let report = write_sustained_json(
        &json_file,
        &drive,
        64 * 1024 * 1024 * 1024,
        300,
        &analysis,
        &buckets,
        None,
    )
    .expect("Failed to write sustained JSON");

    assert!(json_file.exists());
    assert!(report.sustained.is_some());
    let sustained = report.sustained.as_ref().unwrap();
    assert_eq!(sustained.buckets.len(), 2);
    assert_eq!(sustained.buckets[0].mb_s, 3200.0);

    let json_str = to_json_string(&report).expect("Serialization failed");
    assert!(json_str.contains("\"burst_speed_mb_s\""));
    assert!(json_str.contains("\"sparkline\""));

    let deserialized: JsonReport = serde_json::from_str(&json_str).expect("Deserialization failed");
    assert_eq!(deserialized.drive.letter, "C");
    assert!(deserialized.sustained.is_some());

    let _ = std::fs::remove_file(&json_file);
}
