# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-11-26
### Added
- Windows 11 `IoRing` storage engine (`--engine ioring`) with runtime symbol resolution and Windows 10 IOCP fallback.
- Domain workload presets (`directstorage`, `llm-stream`, `oltp-db`) and custom mixed Read/Write ratios (`--rw-mix`).
- Sustained sequential write profiler (`--sustained`, `--sustained-duration`) with automatic pseudo-SLC cache cliff detection and inline Unicode sparklines.
- CPU cycle accounting (`Cycles/IO`, `CPU Util`, `IOPS / Core`) and thread affinity pinning (`--affinity`).
- Non-elevated NVMe SMART & thermal throttling telemetry (`NvmeThermalMonitor`).
- Interactive PowerShell launcher (`dspd.ps1`) and Inno Setup Windows installers (`x86_64` and `arm64`) with SHA-256 release checksums.

### Changed
- Switched embedded application manifest to `asInvoker` for non-elevated execution by default.

### Fixed
- Prevented cross-drive `%TEMP%` test-file fallback, dual-block divide-by-zero on undersized files, and multi-pass flush-count truncation.
- Added async IRP cancellation (`CancelIoEx`) and completion draining before buffer release, `OVERLAPPED.Internal` error checks, and upfront CLI parameter validation.

## [0.1.0] - 2026-10-17
### Added
- Independent native Win32 I/O Completion Port (IOCP) storage engine.
- Benchmark profiles (`SEQ1M Q8T1`, `SEQ1M Q1T1`, `RND4K Q32T1`, `RND4K Q1T1`).
- Direct I/O with `FILE_FLAG_NO_BUFFERING` and `FILE_FLAG_WRITE_THROUGH` using sector-aligned `VirtualAlloc` buffers.
- Latency percentile tracking with `hdrhistogram` (average and p99 latency).
- Live progress reporting via `indicatif`.
- CSV and structured JSON report export.
- Physical storage query property detection via `IOCTL_STORAGE_QUERY_PROPERTY`.
- Embedded Administrator manifest via `winres`.
- Full CLI with profile specification flag (`--validate`).
