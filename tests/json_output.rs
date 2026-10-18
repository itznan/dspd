// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::drives::{BusType, DriveInfo, MediaType};
use dspd::report::json::{build_json_report, to_json_string, write_json, JsonReport};
use dspd::report::ReportRow;

#[test]
fn test_json_serialization_and_roundtrip() {
    let drive = DriveInfo {
        letter: 'C',
        model: "Samsung SSD 990 PRO 1TB".to_string(),
        bus_type: BusType::NVMe,
        media_type: MediaType::SSD,
        capacity: 1_000_204_886_016,
        free: 500_102_443_008,
    };

    let rows = vec![
        ReportRow {
            profile: "SEQ1M Q8T1".to_string(),
            direction: "Read".to_string(),
            mb_s: 7120.45,
            iops: 6790.6,
            avg_lat_us: 1150.2,
            p99_lat_us: 1420,
        },
        ReportRow {
            profile: "SEQ1M Q8T1".to_string(),
            direction: "Write".to_string(),
            mb_s: 6850.12,
            iops: 6532.8,
            avg_lat_us: 1210.0,
            p99_lat_us: 1530,
        },
        ReportRow {
            profile: "RND4K Q32T1".to_string(),
            direction: "Read".to_string(),
            mb_s: 450.78,
            iops: 110050.0,
            avg_lat_us: 285.5,
            p99_lat_us: 420,
        },
    ];

    let report = build_json_report(&rows, &drive, 1024 * 1024 * 1024, 3, 5, 1, None);
    let json_str = to_json_string(&report).expect("Serialization failed");

    assert!(json_str.contains("\"tool\": \"dspd\""));
    assert!(json_str.contains("\"letter\": \"C\""));
    assert!(json_str.contains("\"model\": \"Samsung SSD 990 PRO 1TB\""));
    assert!(json_str.contains("\"bus_type\": \"NVMe\""));
    assert!(json_str.contains("\"media_type\": \"SSD\""));
    assert!(json_str.contains("\"SEQ1M Q8T1\""));

    let deserialized: JsonReport = serde_json::from_str(&json_str).expect("Deserialization failed");
    assert_eq!(deserialized.drive.letter, "C");
    assert_eq!(deserialized.results.len(), 3);
    assert_eq!(deserialized.results[0].mb_s, 7120.45);
    assert_eq!(deserialized.thermal, None);

    // Test writing to file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("dspd_test_report.json");
    let written_report = write_json(&temp_file, &rows, &drive, 1024 * 1024 * 1024, 3, 5, 1, None)
        .expect("Failed to write JSON file");

    assert_eq!(written_report.drive.model, drive.model);
    assert!(temp_file.exists());
    let _ = std::fs::remove_file(&temp_file);
}

#[test]
fn test_json_serialization_with_thermal_telemetry() {
    use dspd::drives::ThermalTelemetry;

    let drive = DriveInfo {
        letter: 'C',
        model: "Samsung SSD 990 PRO 1TB".to_string(),
        bus_type: BusType::NVMe,
        media_type: MediaType::SSD,
        capacity: 1_000_204_886_016,
        free: 500_102_443_008,
    };

    let rows = vec![ReportRow {
        profile: "SEQ1M Q8T1".to_string(),
        direction: "Read".to_string(),
        mb_s: 7120.45,
        iops: 6790.6,
        avg_lat_us: 1150.2,
        p99_lat_us: 1420,
    }];

    let thermal = ThermalTelemetry {
        start_temp_c: 42,
        peak_temp_c: 74,
        end_temp_c: 68,
        throttled: true,
        available_spare_pct: 100,
        percentage_used_pct: 5,
        warning_temp_time_mins: 0,
        critical_temp_time_start_mins: 0,
        critical_temp_time_end_mins: 1,
    };

    let report = build_json_report(
        &rows,
        &drive,
        1024 * 1024 * 1024,
        3,
        5,
        1,
        Some(thermal.clone()),
    );
    let json_str = to_json_string(&report).expect("Serialization failed");

    assert!(json_str.contains("\"start_temp_c\": 42"));
    assert!(json_str.contains("\"peak_temp_c\": 74"));
    assert!(json_str.contains("\"end_temp_c\": 68"));
    assert!(json_str.contains("\"throttled\": true"));
    assert!(json_str.contains("\"available_spare_pct\": 100"));
    assert!(json_str.contains("\"percentage_used_pct\": 5"));

    let deserialized: JsonReport = serde_json::from_str(&json_str).expect("Deserialization failed");
    assert_eq!(deserialized.thermal, Some(thermal));
}
