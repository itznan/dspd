// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::engine::ioring::{
    IoRingApi, IoRingEngine, IORING_BUFFER_INFO, IORING_BUFFER_REF, IORING_CAPABILITIES,
    IORING_CQE, IORING_CREATE_FLAGS, IORING_HANDLE_REF,
};
use dspd::engine::profile::ALL_PROFILES;
use dspd::file::{open_benchmark_file, prepare_test_file, FileGuard};
use std::sync::atomic::AtomicBool;

#[test]
fn test_ioring_struct_sizes_and_alignments() {
    assert_eq!(std::mem::size_of::<IORING_CREATE_FLAGS>(), 8);
    assert_eq!(std::mem::size_of::<IORING_CAPABILITIES>(), 16);
    assert_eq!(std::mem::size_of::<IORING_HANDLE_REF>(), 16);
    assert_eq!(std::mem::size_of::<IORING_BUFFER_REF>(), 16);
    assert_eq!(std::mem::size_of::<IORING_BUFFER_INFO>(), 16);
    assert_eq!(std::mem::size_of::<IORING_CQE>(), 24);
}

#[test]
fn test_ioring_availability_query() {
    let available = IoRingEngine::is_available();
    println!("Windows 11 IoRing available on current OS: {}", available);

    let api_opt = IoRingApi::load();
    assert_eq!(available, api_opt.is_some());
}

#[test]
fn test_live_ioring_benchmark_pass_if_supported() {
    if !IoRingEngine::is_available() {
        println!("IoRing not available on this Windows OS; skipping live IoRing benchmark pass.");
        return;
    }

    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("dspd_test_ioring_live.dat");
    let test_size: u64 = 8 * 1024 * 1024; // 8 MiB

    let _guard = FileGuard {
        path: test_file.clone(),
    };

    prepare_test_file(&test_file, test_size).expect("Failed to prepare test file");
    let file_handle = open_benchmark_file(&test_file).expect("Failed to open test file");

    let profile = &ALL_PROFILES[0]; // SEQ1M Q8T1
    let mut engine = IoRingEngine::new(file_handle, test_size, profile)
        .expect("Failed to initialize IoRingEngine");

    let cancelled = AtomicBool::new(false);
    let pass_res = engine
        .run_pass(1, 0, false, None, &cancelled)
        .expect("IoRing benchmark pass failed");

    assert!(
        pass_res.bytes_xfer > 0,
        "No bytes transferred during IoRing pass"
    );
    assert!(
        pass_res.read_ios > 0,
        "No read I/Os completed during IoRing pass"
    );
    assert!(pass_res.throughput_mb_s() > 0.0);
    assert!(
        pass_res.cpu.is_some(),
        "CPU telemetry missing from IoRing pass result"
    );

    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(file_handle);
    }
}
