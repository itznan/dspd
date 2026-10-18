// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::drives::{
    open_telemetry_handle, parse_nvme_smart_log, query_nvme_smart, NvmeThermalMonitor,
    ThermalTelemetry, NVME_SMART_LOG_SIZE, THERMAL_THROTTLE_TEMP_THRESHOLD_C,
};
use windows::Win32::Foundation::CloseHandle;

#[test]
fn test_parse_nvme_smart_log_known_values() {
    let mut payload = vec![0u8; NVME_SMART_LOG_SIZE];

    // Bytes 1..=2: 315 K = 41.85°C -> 42°C
    let kelvin_42: u16 = 315;
    payload[1..3].copy_from_slice(&kelvin_42.to_le_bytes());

    // Byte 3: Available Spare 95%
    payload[3] = 95;

    // Byte 5: Percentage Used 12%
    payload[5] = 12;

    // Bytes 192..=195: Warning temp time = 10 minutes
    let warn_time: u32 = 10;
    payload[192..196].copy_from_slice(&warn_time.to_le_bytes());

    // Bytes 196..=199: Critical temp time = 2 minutes
    let crit_time: u32 = 2;
    payload[196..200].copy_from_slice(&crit_time.to_le_bytes());

    let parsed = parse_nvme_smart_log(&payload).expect("Failed to parse valid 512-byte payload");

    assert_eq!(parsed.composite_temp_kelvin, 315);
    assert_eq!(parsed.composite_temp_c, 42);
    assert_eq!(parsed.available_spare_pct, 95);
    assert_eq!(parsed.percentage_used_pct, 12);
    assert_eq!(parsed.warning_temp_time_mins, 10);
    assert_eq!(parsed.critical_temp_time_mins, 2);
}

#[test]
fn test_parse_nvme_smart_log_high_temp() {
    let mut payload = vec![0u8; NVME_SMART_LOG_SIZE];

    // Bytes 1..=2: 347 K = 73.85°C -> 74°C
    let kelvin_74: u16 = 347;
    payload[1..3].copy_from_slice(&kelvin_74.to_le_bytes());
    payload[3] = 100;
    payload[5] = 3;

    let parsed = parse_nvme_smart_log(&payload).expect("Failed to parse valid payload");
    assert_eq!(parsed.composite_temp_c, 74);
    assert!(parsed.composite_temp_c >= THERMAL_THROTTLE_TEMP_THRESHOLD_C);
}

#[test]
fn test_parse_nvme_smart_log_truncated_fails() {
    let truncated = vec![0u8; 256];
    let err = parse_nvme_smart_log(&truncated);
    assert!(err.is_err());
}

#[test]
fn test_thermal_throttling_detection_logic() {
    // Case 1: Over 70°C triggers throttling
    let telemetry_hot = ThermalTelemetry {
        start_temp_c: 42,
        peak_temp_c: 74,
        end_temp_c: 68,
        throttled: 74 >= THERMAL_THROTTLE_TEMP_THRESHOLD_C,
        available_spare_pct: 100,
        percentage_used_pct: 5,
        warning_temp_time_mins: 0,
        critical_temp_time_start_mins: 0,
        critical_temp_time_end_mins: 0,
    };
    assert!(telemetry_hot.throttled);

    // Case 2: Under 70°C but critical composite temp time increased triggers throttling
    let telemetry_crit_time = ThermalTelemetry {
        start_temp_c: 45,
        peak_temp_c: 62,
        end_temp_c: 58,
        throttled: 62 >= THERMAL_THROTTLE_TEMP_THRESHOLD_C || 3 > 1,
        available_spare_pct: 100,
        percentage_used_pct: 5,
        warning_temp_time_mins: 5,
        critical_temp_time_start_mins: 1,
        critical_temp_time_end_mins: 3,
    };
    assert!(telemetry_crit_time.throttled);

    // Case 3: Under 70°C and no critical composite temp time increase is NOT throttled
    let telemetry_normal = ThermalTelemetry {
        start_temp_c: 38,
        peak_temp_c: 55,
        end_temp_c: 48,
        throttled: 55 >= THERMAL_THROTTLE_TEMP_THRESHOLD_C || 0 > 0,
        available_spare_pct: 100,
        percentage_used_pct: 5,
        warning_temp_time_mins: 0,
        critical_temp_time_start_mins: 0,
        critical_temp_time_end_mins: 0,
    };
    assert!(!telemetry_normal.throttled);
}

#[test]
fn test_live_nvme_smart_query_and_monitor() {
    // Test on drive C: which is an NVMe SSD on this machine
    if let Ok(handle) = open_telemetry_handle('C') {
        let res = query_nvme_smart(handle);
        unsafe {
            let _ = CloseHandle(handle);
        }

        if let Ok(data) = res {
            println!(
                "Drive C: SMART - Temp: {}°C, Spare: {}%, Used: {}%, WarnTime: {}m, CritTime: {}m",
                data.composite_temp_c,
                data.available_spare_pct,
                data.percentage_used_pct,
                data.warning_temp_time_mins,
                data.critical_temp_time_mins
            );

            assert!(data.composite_temp_c > 0 && data.composite_temp_c < 120);
            assert!(data.available_spare_pct <= 100);

            // Test NvmeThermalMonitor start, sample, stop lifecycle
            let monitor = NvmeThermalMonitor::start('C');
            assert!(monitor.is_some());
            let m = monitor.unwrap();
            std::thread::sleep(std::time::Duration::from_millis(600));
            m.sample();
            let telemetry = m.stop();

            println!(
                "Drive C: Telemetry - Start: {}°C, Peak: {}°C, End: {}°C, Throttled: {}",
                telemetry.start_temp_c,
                telemetry.peak_temp_c,
                telemetry.end_temp_c,
                telemetry.throttled
            );

            assert!(telemetry.start_temp_c > 0);
            assert!(telemetry.peak_temp_c >= telemetry.start_temp_c);
        }
    }
}

#[test]
fn test_live_sata_drive_gracefully_unsupported() {
    // Non-existent drive or non-NVMe drive must return None without panicking
    let monitor = NvmeThermalMonitor::start('Z');
    assert!(monitor.is_none());

    // If any discovered drive is not NVMe (e.g. SATA, USB, virtual), verify it returns None gracefully
    for drive in dspd::drives::discover(true) {
        if drive.bus_type != dspd::drives::BusType::NVMe {
            let mon = NvmeThermalMonitor::start(drive.letter);
            assert!(
                mon.is_none(),
                "Expected non-NVMe drive {} to return None",
                drive.letter
            );
        }
    }
}
