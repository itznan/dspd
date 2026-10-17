# dspd — Build and Verification Walkthrough

**dspd** is a single-binary, open-source Windows disk benchmark tool written in Rust with its own native Win32 I/O Completion Port (IOCP) storage engine. It replicates CrystalDiskMark 9's default test profiles and exports results as CSV and standalone dark-themed HTML reports.

---

## What Was Built

### 1. Project Infrastructure & Metadata
- **Cargo Package**: `dspd` v0.1.0, edition 2021, authors `["itznan"]`, repository `https://github.com/itznan/dspd`, MIT licensed.
- **Embedded Manifest & Resource Info**: Configured via `build.rs` and `winres` embedding `dspd.manifest` (`requireAdministrator` on release, `asInvoker` for headless test automation) and `VERSIONINFO` metadata (`ProductName`, `CompanyName`, `LegalCopyright (c) 2026 itznan`).
- **Repository Standards**: `// SPDX-License-Identifier: MIT` header on every source file, comprehensive `README.md`, `LICENSE`, `CHANGELOG.md`, and `.gitignore`.

### 2. High-Performance Win32 IOCP Storage Engine
- **Direct Hardware I/O**: Files opened with `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH | FILE_FLAG_OVERLAPPED`.
- **Sector-Aligned Memory**: Implemented in [`src/engine/buffers.rs`](file:///C:/itznan/github/dspd/src/engine/buffers.rs) using `VirtualAlloc` (aligned to 64 KiB page granularity) filled with pseudo-random bytes (`rand_xoshiro`).
- **Pinned `OVERLAPPED` Lifetime Contract**: [`src/engine/iocp.rs`](file:///C:/itznan/github/dspd/src/engine/iocp.rs) leaks heap-pinned `Box<OVERLAPPED>` to kernel space via `Box::into_raw` and reclaims it on completion dequeue via `Box::from_raw`.
- **Strict Queue Depth Invariant**: Batched `GetQueuedCompletionStatusEx` dequeue loop keeps exactly $N$ requests in flight simultaneously.
- **Microsecond Latency Precision**: `QueryPerformanceCounter` recorded into an `hdrhistogram` instance for average and p99 percentile metrics.

### 3. Physical Storage Discovery ([`src/drives.rs`](file:///C:/itznan/github/dspd/src/drives.rs))
- Discovers drives via `GetLogicalDrives()` and `GetDriveTypeW()`.
- Queries device properties using `IOCTL_STORAGE_QUERY_PROPERTY` (`StorageDeviceProperty` for Model and Bus Type: NVMe, SATA, USB).
- Detects media type (`SSD` vs `HDD`) via `StorageDeviceSeekPenaltyProperty`.
- Obtains partition capacity and free space via `GetDiskFreeSpaceExW()`.

### 4. CrystalDiskMark 9 Profiles & Export ([`src/report/`](file:///C:/itznan/github/dspd/src/report/))
- Profiles: `SEQ1M Q8T1`, `SEQ1M Q1T1`, `RND4K Q32T1`, `RND4K Q1T1` (Read and Write).
- CLI live progress bars via `indicatif`.
- CSV report generator via `csv`.
- Self-contained dark-themed HTML report (inline CSS, CrystalDiskMark layout table, detailed latency table, footer attribute).

### 5. Automated CI & Multi-Target Release Workflows
- **`ci.yml`**: Runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` on `windows-latest`.
- **`release.yml`**: Builds release executables for both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, bundles them into zip packages with `README.md` and `LICENSE`, computes SHA256 hashes, and uploads them to GitHub Releases.

---

## Verification Results

### 1. Automated Test Suite
All 7 unit and integration tests passed with 0 failures:
```text
running 1 test
test test_csv_serialization_roundtrip ... ok

running 1 test
test test_histogram_p99_and_mean ... ok

running 2 tests
test test_sequential_offsets_are_aligned ... ok
test test_random_offsets_are_aligned ... ok

running 3 tests
test test_parse_size_aligns_to_sector ... ok
test test_parse_size_exact_units ... ok
test test_parse_size_invalid ... ok
```

### 2. Linter & Formatting
- `cargo fmt --check`: Clean (0 diffs).
- `cargo clippy --all-targets -- -D warnings`: Clean (0 warnings).

### 3. Live Benchmark Smoke Test
Executed on drive `C:` (`WD Green SN350 500GB 2G0C [SSD, NVMe]`):
```text
────────────────────────────────────────────────────────────────────────
Drive C: WD Green SN350 500GB 2G0C [SSD]
Bus Type: NVMe | Capacity: 499.4 GB | Free Space: 328.9 GB
────────────────────────────────────────────────────────────────────────
  [RND4K Q32T1] Read :   608.83 MB/s |   148640 IOPS | Avg: 156.5 µs | p99:  281 µs
  [RND4K Q32T1] Write:   462.82 MB/s |   112993 IOPS | Avg: 161.2 µs | p99:  288 µs
```
- Benchmark test file automatically removed by `FileGuard` on completion (`Test-Path: False`).

### 4. Release Binary Size
- `target/release/dspd.exe`: **0.76 MB** (fat LTO, stripped, 1 codegen unit).

---

## How to Run & Test

### Run Quick Smoke Test (Read & Write)
```powershell
.\target\release\dspd.exe --drives C --size 64MiB --passes 1 --duration 2 --yes
```

### Run Full CrystalDiskMark 9 Default Test
```powershell
.\target\release\dspd.exe --drives C
```

### View DiskSpd Equivalents
```powershell
.\target\release\dspd.exe --validate
```
