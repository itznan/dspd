// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use dspd::parse_size;

#[test]
fn test_parse_size_exact_units() {
    assert_eq!(parse_size("1GiB").unwrap(), 1024 * 1024 * 1024);
    assert_eq!(parse_size("512MiB").unwrap(), 512 * 1024 * 1024);
    assert_eq!(parse_size("4KiB").unwrap(), 4096);
    assert_eq!(parse_size("1024KiB").unwrap(), 1024 * 1024);
    assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
    assert_eq!(parse_size("256M").unwrap(), 256 * 1024 * 1024);
}

#[test]
fn test_parse_size_aligns_to_sector() {
    // 5000 bytes aligned up to 4096 boundary becomes 8192
    assert_eq!(parse_size("5000b").unwrap(), 8192);
}

#[test]
fn test_parse_size_invalid() {
    assert!(parse_size("").is_err());
    assert!(parse_size("abc").is_err());
    assert!(parse_size("10X").is_err());
}
