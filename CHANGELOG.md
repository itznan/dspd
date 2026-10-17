# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-17
### Added
- Independent native Win32 I/O Completion Port (IOCP) storage engine.
- Replicating CrystalDiskMark 9 default test profiles (`SEQ1M Q8T1`, `SEQ1M Q1T1`, `RND4K Q32T1`, `RND4K Q1T1`).
- Direct I/O with `FILE_FLAG_NO_BUFFERING` and `FILE_FLAG_WRITE_THROUGH` using sector-aligned `VirtualAlloc` buffers.
- Latency percentile tracking with `hdrhistogram` (average and p99 latency).
- Live progress reporting via `indicatif`.
- CSV and standalone dark-themed HTML report export.
- Physical storage query property detection via `IOCTL_STORAGE_QUERY_PROPERTY`.
- Embedded Administrator manifest via `winres`.
- Full CLI with validation flag (`--validate`) mapping to equivalent `diskspd` parameters.
