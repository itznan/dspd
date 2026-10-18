// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::engine::iocp::IocpEngine;
use dspd::engine::profile::{
    RwMix, XorShift64Star, ALL_AVAILABLE_PROFILES, DIRECTSTORAGE, LLM_STREAM, OLTP_DB,
};
use dspd::engine::stats::PassResult;
use dspd::file::{open_benchmark_file, prepare_test_file};
use std::sync::atomic::AtomicBool;

#[test]
fn test_rw_mix_parsing_and_validation() {
    let mix7030 = RwMix::parse("70:30").unwrap();
    assert_eq!(mix7030.read_pct, 70);
    assert_eq!(mix7030.write_pct, 30);
    assert!(mix7030.is_mixed());

    let mix_slash = RwMix::parse("80/20").unwrap();
    assert_eq!(mix_slash.read_pct, 80);
    assert_eq!(mix_slash.write_pct, 20);

    let read_only = RwMix::parse("100:0").unwrap();
    assert_eq!(read_only.read_pct, 100);
    assert_eq!(read_only.write_pct, 0);
    assert!(!read_only.is_mixed());

    let write_only = RwMix::parse("0:100").unwrap();
    assert_eq!(write_only.read_pct, 0);
    assert_eq!(write_only.write_pct, 100);
    assert!(!write_only.is_mixed());

    // Fails when sum != 100
    assert!(RwMix::parse("70:20").is_err());
    assert!(RwMix::parse("60:50").is_err());
    assert!(RwMix::parse("invalid").is_err());
    assert!(RwMix::parse("70:30:10").is_err());
}

#[test]
fn test_xorshift64star_distribution() {
    let mut rng = XorShift64Star::new(0x123456789ABCDEF0);
    let mut read_count = 0;
    let total = 50_000;

    for _ in 0..total {
        if rng.decide_read(70) {
            read_count += 1;
        }
    }

    let read_pct = (read_count as f64 / total as f64) * 100.0;
    assert!(
        (read_pct - 70.0).abs() < 1.5,
        "Expected ~70% reads, got {:.2}%",
        read_pct
    );

    // Extreme boundaries
    for _ in 0..1000 {
        assert!(rng.decide_read(100));
        assert!(!rng.decide_read(0));
    }
}

#[test]
fn test_pass_result_dual_histogram_and_metrics() {
    let mut res = PassResult::new().unwrap();
    res.elapsed_ns = 1_000_000_000; // 1 second

    // 700 reads of 8 KiB at 100 µs (with 1 outlier at 5000 µs)
    for _ in 0..699 {
        res.record_read(8192, 100);
    }
    res.record_read(8192, 5000);

    // 300 writes of 8 KiB at 2500 µs
    for _ in 0..300 {
        res.record_write(8192, 2500);
    }

    // 5 cache flushes of 1500 µs each
    for _ in 0..5 {
        res.record_flush(1500);
    }

    assert_eq!(res.read_ios, 700);
    assert_eq!(res.write_ios, 300);
    assert_eq!(res.bytes_read, 700 * 8192);
    assert_eq!(res.bytes_written, 300 * 8192);
    assert_eq!(res.bytes_xfer, 1000 * 8192);

    // Throughput & IOPS
    assert!((res.read_iops() - 700.0).abs() < 0.01);
    assert!((res.write_iops() - 300.0).abs() < 0.01);
    assert!((res.iops(8192) - 1000.0).abs() < 0.01);

    // Read p50 should reflect pure read latency (around 100 µs)
    assert!(res.read_p50_us() <= 150);
    // Write p50 should reflect write latency (around 2500 µs)
    assert!(res.write_p50_us() >= 2000);

    // Read p99.9 should capture the 5000 µs spike
    assert!(res.read_p999_us() >= 4500);

    // Cache flush metrics
    assert_eq!(res.flush_count, 5);
    assert!((res.avg_flush_us() - 1500.0).abs() < 0.01);
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn test_domain_presets_specs() {
    assert_eq!(DIRECTSTORAGE.name, "directstorage");
    assert_eq!(DIRECTSTORAGE.block_size, 64 * 1024);
    assert_eq!(DIRECTSTORAGE.block_size_alt, Some(32 * 1024));
    assert_eq!(DIRECTSTORAGE.queue_depth, 64);
    assert!(DIRECTSTORAGE.is_random);
    assert!(DIRECTSTORAGE.is_read_only);

    assert_eq!(LLM_STREAM.name, "llm-stream");
    assert_eq!(LLM_STREAM.block_size, 2 * 1024 * 1024);
    assert_eq!(LLM_STREAM.block_size_alt, None);
    assert_eq!(LLM_STREAM.queue_depth, 8);
    assert!(!LLM_STREAM.is_random);
    assert!(LLM_STREAM.is_llm_stream);
    assert!(LLM_STREAM.is_read_only);

    assert_eq!(OLTP_DB.name, "oltp-db");
    assert_eq!(OLTP_DB.block_size, 8 * 1024);
    assert_eq!(OLTP_DB.queue_depth, 32);
    assert!(OLTP_DB.is_random);
    assert_eq!(
        OLTP_DB.default_rw_mix,
        Some(RwMix {
            read_pct: 70,
            write_pct: 30
        })
    );
    assert_eq!(OLTP_DB.flush_interval_ios, Some(64));
    assert!(!OLTP_DB.is_read_only);

    // Verify presets are in ALL_AVAILABLE_PROFILES
    assert!(ALL_AVAILABLE_PROFILES
        .iter()
        .any(|p| p.name == "directstorage"));
    assert!(ALL_AVAILABLE_PROFILES
        .iter()
        .any(|p| p.name == "llm-stream"));
    assert!(ALL_AVAILABLE_PROFILES.iter().any(|p| p.name == "oltp-db"));

    // Verify 16 GiB model load time estimation
    let mb_s = 6500.0;
    let est_secs = (16.0 * 1024.0 * 1024.0 * 1024.0) / (mb_s * 1_000_000.0);
    assert!(est_secs > 2.6 && est_secs < 2.7);
}

#[test]
fn test_live_iocp_mixed_and_presets_execution() {
    let temp_file = std::env::temp_dir().join("dspd_test_live_presets.dat");
    let file_size = 32 * 1024 * 1024; // 32 MiB
    prepare_test_file(&temp_file, file_size).unwrap();

    let cancel = AtomicBool::new(false);

    // Test 1: DirectStorage preset (dual 32K/64K block size, QD 64)
    {
        let handle = open_benchmark_file(&temp_file).unwrap();
        let mut engine = IocpEngine::new(handle, file_size, &DIRECTSTORAGE).unwrap();
        let res = engine.run_pass(1, 0, false, None, &cancel).unwrap();
        assert!(res.bytes_read > 0);
        assert_eq!(res.bytes_written, 0);
        assert!(res.read_ios > 0);
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }

    // Test 2: LLM Stream preset (2 MiB sequential reads, QD 8)
    {
        let handle = open_benchmark_file(&temp_file).unwrap();
        let mut engine = IocpEngine::new(handle, file_size, &LLM_STREAM).unwrap();
        let res = engine.run_pass(1, 0, false, None, &cancel).unwrap();
        assert!(res.bytes_read > 0);
        assert_eq!(res.bytes_written, 0);
        assert!(res.read_ios > 0);
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }

    // Test 3: OLTP DB preset (8 KiB random 70:30 R/W mix, QD 32, periodic flush)
    {
        let handle = open_benchmark_file(&temp_file).unwrap();
        let mut engine = IocpEngine::new(handle, file_size, &OLTP_DB).unwrap();
        let res = engine.run_pass(1, 0, false, None, &cancel).unwrap();
        assert!(res.bytes_read > 0);
        assert!(res.bytes_written > 0);
        assert!(res.read_ios > 0);
        assert!(res.write_ios > 0);
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }

    let _ = std::fs::remove_file(temp_file);
}
