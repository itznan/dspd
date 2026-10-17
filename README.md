# dspd

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> Fast, safe, single-binary disk benchmarking for Windows, written in Rust.

**dspd** is an open-source Windows disk benchmark tool built from scratch in Rust with its own native Win32 I/O Completion Port (IOCP) storage engine. It runs without any external tool dependencies and provides accurate throughput, IOPS, and latency metrics.

---

## Features

- **Native IOCP Engine**: Handcrafted Win32 I/O completion port scheduler managing concurrent in-flight requests.
- **Benchmark Profiles**:
  - `SEQ1M Q8T1`: Sequential, 1 MiB blocks, queue depth 8, 1 thread
  - `SEQ1M Q1T1`: Sequential, 1 MiB blocks, queue depth 1, 1 thread
  - `RND4K Q32T1`: Random, 4 KiB blocks, queue depth 32, 1 thread
  - `RND4K Q1T1`: Random, 4 KiB blocks, queue depth 1, 1 thread
- **Direct I/O**: Opens files with `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH | FILE_FLAG_OVERLAPPED` and buffers aligned to 64 KiB memory pages via `VirtualAlloc`.
- **Latency Percentiles**: Accurately measures individual I/O latency with `QueryPerformanceCounter` recorded in `hdrhistogram` (reports average and p99).
- **Physical Drive Discovery**: Identifies drive model, bus type (NVMe, SATA, USB), media type (SSD vs. HDD), total capacity, and free space via Win32 IOCTLs (`IOCTL_STORAGE_QUERY_PROPERTY`).
- **Safety Guards**: Automatically cleans up benchmark test files on normal exit, error, panic, or Ctrl+C.
- **Reports**: Generates timestamped CSV and structured JSON reports.
- **Validation**: Built-in `--validate` flag outputs profile test specifications and parameters.

---

## Quick Start (Interactive Drive Selector)

Scan all connected drives (internal NVMe/SATA SSDs/HDDs, USB pen drives, external storage), interactively select a drive, and benchmark immediately:

```powershell
irm https://raw.githubusercontent.com/itznan/dspd/main/dspd.ps1 | iex
```

Or run the local script:
```powershell
.\dspd.ps1
```

---

## Installation

### 1. Windows Installer (`Setup.exe`)
Download the latest installer from [GitHub Releases](https://github.com/itznan/dspd/releases):
- **`dspd-Setup-v0.1.0-x86_64.exe`** (Standard 64-bit Windows)
- **`dspd-Setup-v0.1.0-arm64.exe`** (ARM64 Windows)

The setup wizard installs `dspd` into `Program Files`, adds `dspd` to the system `PATH` so you can use it anywhere, and registers Start Menu shortcuts.

### 2. Standalone Archive
Download and extract `dspd-v<version>-x86_64-pc-windows-msvc.zip` from [Releases](https://github.com/itznan/dspd/releases).

### 3. Build From Source
```powershell
git clone https://github.com/itznan/dspd.git
cd dspd
cargo build --release
```
The optimized executable will be located at `target\release\dspd.exe`.

---

## Usage

*Note: Benchmarking disks via direct hardware I/O and querying storage geometry requires Administrator privileges. An elevation manifest is embedded in `dspd.exe`.*

```text
Usage: dspd.exe [OPTIONS]

Options:
      --drives <DRIVES>            Drive letters to benchmark, comma-separated (e.g. C,D)
      --size <SIZE>                Test file size (e.g. 1GiB, 512MiB, 4GiB) [default: 1GiB]
      --passes <PASSES>            Number of passes per test [default: 3]
      --duration <DURATION>        Duration of each pass in seconds [default: 5]
      --warmup <WARMUP>            Warmup duration in seconds (discarded from metrics) [default: 1]
      --include-removable          Include removable storage devices (e.g. USB flash drives)
      --profile <PROFILE>          Run a single named profile only (e.g. "RND4K Q32T1")
      --output-dir <OUTPUT_DIR>    Output directory for reports [default: Desktop]
      --json                       Output report in JSON format to console
      --yes                        Skip the SSD-wear confirmation prompt
      --validate                   Print benchmark profile specifications and test parameters
  -h, --help                       Print help
  -V, --version                    Print version
```

### Examples

Benchmark drive `C:` with default settings (1 GiB, 3 passes, 5s duration):
```powershell
dspd.exe --drives C
```

Run a quick 5-second smoke test on drive `D:` without write confirmation:
```powershell
dspd.exe --drives D --passes 1 --duration 5 --yes
```

Run only random 4K queue depth 32 read/write:
```powershell
dspd.exe --drives C --profile "RND4K Q32T1"
```

Show profile benchmark specifications:
```powershell
dspd.exe --validate
```

---

## Sample Console Output

```text
dspd v0.1.0 by itznan (github.com/itznan/dspd)

Drive: C: (Samsung SSD 990 PRO 2TB) [NVMe, SSD]
Capacity: 1863.0 GB | Free: 1240.5 GB

[1/4] SEQ1M Q8T1   Read: 7120.4 MB/s  |  Write: 6850.2 MB/s
[2/4] SEQ1M Q1T1   Read: 4420.1 MB/s  |  Write: 4180.7 MB/s
[3/4] RND4K Q32T1  Read: 1980.5 MB/s  |  Write: 1720.3 MB/s
[4/4] RND4K Q1T1   Read:   88.2 MB/s  |  Write:  240.1 MB/s

────────────────────────────────────────────────────────────────────────
 Profile        │ Read (MB/s) │ Write (MB/s) │ Read IOPS │ Write IOPS
────────────────────────────────────────────────────────────────────────
 SEQ1M Q8T1     │     7120.40 │      6850.20 │      6791 │       6533
 SEQ1M Q1T1     │     4420.10 │      4180.70 │      4215 │       3987
 RND4K Q32T1    │     1980.50 │      1720.30 │    483520 │     419995
 RND4K Q1T1     │       88.20 │       240.10 │     21533 │      58618
────────────────────────────────────────────────────────────────────────

────────────────────────────────────────────────────────────────────────
 Profile        │ Read Avg Lat │ Read p99 Lat │ Write Avg Lat │ Write p99 Lat
────────────────────────────────────────────────────────────────────────
 SEQ1M Q8T1     │     1.17 ms  │     2.40 ms  │      1.22 ms  │      2.80 ms
 SEQ1M Q1T1     │     0.23 ms  │     0.45 ms  │      0.24 ms  │      0.51 ms
 RND4K Q32T1    │    66.18 µs  │   182.00 µs  │     76.19 µs  │    210.00 µs
 RND4K Q1T1     │    46.44 µs  │    95.00 µs  │     17.06 µs  │     42.00 µs
────────────────────────────────────────────────────────────────────────

Reports saved:
  CSV:  C:\Users\itznan\Desktop\dspd_report_20261017_190000.csv
  JSON: C:\Users\itznan\Desktop\dspd_report_20261017_190000.json
```

---

## Methodology & Design

- **Random Data**: The benchmark file and buffers are initialized with high-throughput PRNG (`rand_xoshiro`) pseudo-random data. This prevents transparent filesystem compression, deduplication, and SSD controller sparse optimizations from inflating numbers.
- **Pre-allocation & Sequential Write**: The entire test file is sequentially written to disk before read tests begin so that reads never encounter unallocated sparse blocks.
- **Unit Convention**: Standard storage benchmarking convention: $1\text{ MB/s} = 1{,}000{,}000\text{ bytes/s}$.
- **Warmup Discard**: The initial warmup window (default 1s) issues real I/O requests to prime controller caches and queues, but its metrics are discarded from measurement averages.
- **SLC Cache Caveat**: When testing large file sizes (e.g. 16 GiB+) on consumer SSDs, performance may drop once the drive's pseudo-SLC cache is exhausted. This reflects actual drive hardware behavior.

---

## SSD Wear Warning

Benchmarking SSDs involves writing continuous streams of data to NAND flash memory. For example, a 1 GiB test file across 3 passes of sequential and random write tests writes several gigabytes of data. **dspd** displays a confirmation prompt prior to starting write tests unless the `--yes` flag is specified.

---

## Profile Specifications

Running `dspd --validate` displays the exact benchmark profile specifications:

| Profile | Block Size | Queue Depth | Access Pattern | Description |
|---|---|---|---|---|
| `SEQ1M Q8T1` | 1 MiB | 8 | Sequential | Peak sequential throughput with deep queue |
| `SEQ1M Q1T1` | 1 MiB | 1 | Sequential | Single-queue sequential transfer rate |
| `RND4K Q32T1` | 4 KiB | 32 | Random | Peak random IOPS under heavy multi-request workload |
| `RND4K Q1T1` | 4 KiB | 1 | Random | Real-world application random access and low-queue latency |

---

## License

This project is licensed under the [MIT License](LICENSE).
`// SPDX-License-Identifier: MIT`
Copyright (c) 2026 itznan.
