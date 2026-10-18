// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::report::ReportRow;

#[test]
fn test_csv_serialization_roundtrip() {
    let row = ReportRow {
        profile: "SEQ1M Q8T1".to_string(),
        direction: "Read".to_string(),
        mb_s: 7120.45,
        iops: 6791.0,
        p50_lat_us: 1120,
        avg_lat_us: 1170.5,
        p99_lat_us: 2400,
        p999_lat_us: 3200,
    };

    let temp_file = std::env::temp_dir().join("dspd_test_roundtrip.csv");
    dspd::report::csv::write_csv(&temp_file, std::slice::from_ref(&row)).unwrap();

    let mut reader = csv::Reader::from_path(&temp_file).unwrap();
    let records: Vec<ReportRow> = reader.deserialize().map(|r| r.unwrap()).collect();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].profile, "SEQ1M Q8T1");
    assert_eq!(records[0].direction, "Read");
    assert!((records[0].mb_s - 7120.45).abs() < 0.001);
    assert_eq!(records[0].p50_lat_us, 1120);
    assert_eq!(records[0].p99_lat_us, 2400);
    assert_eq!(records[0].p999_lat_us, 3200);

    let _ = std::fs::remove_file(temp_file);
}
