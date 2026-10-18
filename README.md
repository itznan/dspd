# dspd

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/itznan/dspd/actions/workflows/ci.yml/badge.svg)](https://github.com/itznan/dspd/actions/workflows/ci.yml)
[![Release](https://github.com/itznan/dspd/actions/workflows/release.yml/badge.svg)](https://github.com/itznan/dspd/actions/workflows/release.yml)

> Fast, safe, single-binary disk benchmarking and storage telemetry for Windows, written in Rust.

**dspd** is an open-source Windows storage benchmark and telemetry suite built from scratch in Rust. It features native Win32 I/O Completion Port (IOCP) and Windows 11 `IoRing` storage engines, direct unbuffered page-aligned I/O, CPU cycle efficiency accounting, NVMe SMART thermal health polling, and sustained write pseudo-SLC cache cliff analysis.

---

## Features

- **Dual Storage Engines**:
  - **Win32 IOCP** (`--engine iocp`, default): Zero-allocation asynchronous completion port scheduler running concurrently across queued operations.
  - **Windows 11 IoRing** (`--engine ioring`): Modern kernel ring buffer engine resolved dynamically at runtime (`kernelbase.dll`) with pre-registered file handles and buffer rings. Retains 100% backward compatibility with Windows 10.
- **Benchmark Profiles & Workload Presets**:
  - **Standard Profiles**: `SEQ1M Q8T1`, `SEQ1M Q1T1`, `RND4K Q32T1`, `RND4K Q1T1`.
  - **Domain Presets (`--profile <NAME>`)**:
    - `directstorage`: 32 KiB and 64 KiB random reads at Queue Depth 64 simulating modern GPU DirectStorage asset streaming.
    - `llm-stream`: 2 MiB sequential unbuffered reads at Queue Depth 8, reporting bandwidth and estimated load time for a 16 GiB LLM weight checkpoint.
    - `oltp-db`: 8 KiB random 70:30 mixed Read/Write workload at Queue Depth 32 with periodic disk cache flushes (`FlushFileBuffers`).
  - **Custom Mixed Workloads (`--rw-mix <READ>:<WRITE>`)**: User-defined Read/Write ratios (e.g. `70:30`, `80:20`) with independent latency histograms to detect read starvation under heavy write pressure.
- **Sustained Write Profiling & SLC Cliff Detection (`--sustained <SIZE_GIB>`)**:
  - Continuous unbuffered sequential write pass using 1-second fixed telemetry buckets.
  - Automatically identifies **Burst Write Speed** (pseudo-SLC plateau), **SLC Cache Cliff Point** (exact GiB written when throughput drops $\ge 35\%$), and **Steady-State Sustained TLC/QLC Speed**.
  - Renders inline Unicode block sparklines (`▁▂▃▄▅▆▇█`) in stdout.
- **CPU Cycle Accounting & Core Affinity**:
  - Measures total CPU utilization, clock cycles consumed per I/O (`Cycles/IO`), and normalized `IOPS / Core` efficiency via `QueryProcessCycleTime` and `GetProcessTimes`.
  - Lock benchmark execution to specific P-cores, E-cores, or CCX dies with `--affinity <HEX|LIST>` (e.g., `--affinity 0-3` or `--affinity 0x0F`).
- **NVMe Thermal & SMART Health Telemetry**:
  - Non-elevated SMART polling via Win32 `IOCTL_STORAGE_PROTOCOL_COMMAND`.
  - Captures initial, peak, and ending composite temperatures, available spare capacity, percentage lifespan used, and alerts on thermal throttling events ($\ge 70^\circ\text{C}$).
- **Direct Page-Aligned I/O**:
  - Bypasses Windows file cache using `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH | FILE_FLAG_OVERLAPPED`.
  - Memory buffers allocated via `VirtualAlloc` aligned to 64 KiB system allocation granularity.
- **Sub-Microsecond Latency Percentiles**: High-resolution `QueryPerformanceCounter` tracking recorded into `hdrhistogram` (reports Average, p50, p99, and p99.9 latencies).
- **Physical Drive Discovery**: Identifies drive model, bus interface (NVMe, SATA, USB), media type (SSD vs. HDD), total capacity, and free space via Win32 IOCTLs.
- **Safety Guards**: Automatically tracks and cleans up benchmark test files on normal exit, error, panic, or Ctrl+C.

---

## Quick Start

Open **PowerShell** and run:

```powershell
iex (irm https://github.com/itznan/dspd/releases/latest/download/dspd.ps1)
```

This downloads the latest `dspd` release, lists all connected drives (NVMe, SATA, USB), prompts for target selection, and launches the benchmark.

<details>
<summary>Prefer to inspect the script first?</summary>

```powershell
irm https://github.com/itznan/dspd/releases/latest/download/dspd.ps1 -OutFile dspd.ps1
notepad .\dspd.ps1
.\dspd.ps1
```
</details>

> [!NOTE]
> If running in PowerShell where script execution is restricted, invoke the native executable directly using `dspd.exe` or enable user script execution:
> ```powershell
> Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned
> ```

---

## Installation

### 1. Windows Setup Installer (`.exe`)
Download the latest installer from [GitHub Releases](https://github.com/itznan/dspd/releases/latest):
- **`dspd-Setup-v*-x86_64.exe`** (Standard 64-bit Windows)
- **`dspd-Setup-v*-arm64.exe`** (ARM64 Windows / Surface Pro)

The installer sets up `dspd` in `Program Files`, adds it to the system `PATH` for immediate terminal access, and adds Start Menu shortcuts.

### 2. Standalone Portable Archive (`.zip`)
Download `dspd-v*-x86_64-pc-windows-msvc.zip` from [Releases](https://github.com/itznan/dspd/releases/latest), unzip, and run `dspd.exe`.

### 3. Build From Source
```powershell
git clone https://github.com/itznan/dspd.git
cd dspd
cargo build --release
```
The optimized executable will be located at `target\release\dspd.exe`.

---

## CLI Usage

```text
Usage: dspd.exe [OPTIONS]

Options:
      --drives <DRIVES>            Drive letters to benchmark, comma-separated (e.g. C,D)
      --size <SIZE>                Test file size (e.g. 1GiB, 512MiB, 4GiB) [default: 1GiB]
      --passes <PASSES>            Number of passes per test [default: 3]
      --duration <DURATION>        Duration of each pass in seconds [default: 5]
      --warmup <WARMUP>            Warmup duration in seconds (discarded from metrics) [default: 1]
      --include-removable          Include removable drives (e.g. USB flash drives)
      --profile <PROFILE>          Run a single named profile or preset (e.g. "RND4K Q32T1", "llm-stream")
      --output-dir <OUTPUT_DIR>    Output directory for reports [default: Desktop]
      --json                       Output report in JSON format to console
      --yes                        Skip SSD wear confirmation prompt
      --validate                   Print benchmark profile specifications and test parameters
      --sustained <SIZE_GIB>       Sustained write profiling mode to measure SLC cache exhaustion (e.g. 50 or 50GiB)
      --sustained-duration <SECS>  Sustained write test maximum duration in seconds
      --rw-mix <READ:WRITE>        Mixed Read/Write workload ratio (e.g. 70:30)
      --engine <ENGINE>            Storage engine backend: "iocp" (default) or "ioring" (Windows 11) [default: iocp]
      --affinity <MASK_OR_LIST>    CPU affinity mask (hex e.g. 0xF) or core ranges (e.g. 0-3 or 0,2,4)
  -h, --help                       Print help
  -V, --version                    Print version
```

---

## Usage Examples

### Standard CrystalDiskMark-Style Benchmark
```powershell
dspd.exe --drives C
```

### Windows 11 IoRing Storage Engine
```powershell
dspd.exe --drives C --engine ioring
```

### Sustained Write pseudo-SLC Cache Profiling
Profile 50 GiB of continuous writes to detect SLC cache size and direct-to-TLC/QLC speeds:
```powershell
dspd.exe --drives C --sustained 50GiB
```

### Domain Workload Presets
```powershell
# Simulate GPU DirectStorage 64K random asset streaming at QD64
dspd.exe --drives C --profile directstorage

# Simulate 2 MiB sequential weights loading for LLM inference (reports 16 GiB load time)
dspd.exe --drives C --profile llm-stream

# Simulate OLTP database 8K 70:30 mixed R/W transactions with drive cache flushes
dspd.exe --drives C --profile oltp-db
```

### Custom Mixed Workloads & CPU Affinity
```powershell
# 80% Read / 20% Write mixed workload pinned to performance cores 0-3
dspd.exe --drives C --rw-mix 80:20 --affinity 0-3
```

### Validate Benchmark Configurations (Dry-Run)
```powershell
dspd.exe --validate
```

---

## Sample Console Output

### 1. Standard Benchmark & Telemetry Summary
```text
dspd v0.2.0 by itznan (github.com/itznan/dspd)

Drive: C: (Samsung SSD 990 PRO 2TB) [NVMe, SSD]
Capacity: 1863.0 GB | Free: 1240.5 GB

[1/4] SEQ1M Q8T1   Read: 7120.4 MB/s  |  Write: 6850.2 MB/s
[2/4] SEQ1M Q1T1   Read: 4420.1 MB/s  |  Write: 4180.7 MB/s
[3/4] RND4K Q32T1  Read: 1980.5 MB/s  |  Write: 1720.3 MB/s
[4/4] RND4K Q1T1   Read:   88.2 MB/s  |  Write:  240.1 MB/s

──────────────────────────────────────────────────────────────────────────────────────────────────────────
  Profile        │ Direction │    Throughput │          IOPS │   Avg Lat │    p50 Lat │    p99 Lat │  p99.9 Lat
──────────────────────────────────────────────────────────────────────────────────────────────────────────
  SEQ1M Q8T1     │ Read      │   7120.4 MB/s │          6791 │   1.17 ms │    1.12 ms │    2.40 ms │    3.10 ms
  SEQ1M Q8T1     │ Write     │   6850.2 MB/s │          6533 │   1.22 ms │    1.18 ms │    2.80 ms │    3.45 ms
  SEQ1M Q1T1     │ Read      │   4420.1 MB/s │          4215 │   0.23 ms │    0.22 ms │    0.45 ms │    0.62 ms
  SEQ1M Q1T1     │ Write     │   4180.7 MB/s │          3987 │   0.24 ms │    0.23 ms │    0.51 ms │    0.70 ms
  RND4K Q32T1    │ Read      │   1980.5 MB/s │        483520 │  66.18 µs │   62.00 µs │  182.00 µs │  245.00 µs
  RND4K Q32T1    │ Write     │   1720.3 MB/s │        419995 │  76.19 µs │   71.00 µs │  210.00 µs │  290.00 µs
  RND4K Q1T1     │ Read      │     88.2 MB/s │         21533 │  46.44 µs │   44.00 µs │   95.00 µs │  120.00 µs
  RND4K Q1T1     │ Write     │    240.1 MB/s │         58618 │  17.06 µs │   16.00 µs │   42.00 µs │   58.00 µs
──────────────────────────────────────────────────────────────────────────────────────────────────────────
  CPU Efficiency │ Direction │      CPU Util │     Cycles/IO │     IOPS / Core
──────────────────────────────────────────────────────────────────────────────────────────────────────────
  RND4K Q32T1    │ Read      │          6.0% │         18533 │          205101
  RND4K Q32T1    │ Write     │          4.1% │         23279 │          173083
  RND4K Q1T1     │ Read      │          1.0% │         65410 │           91319
  RND4K Q1T1     │ Write     │          3.1% │         68680 │           72538
──────────────────────────────────────────────────────────────────────────────────────────────────────────
  Thermal        │ Temp: 53°C -> 70°C peak (end: 61°C)
  SMART Health   │ Spare: 100% │ Life Used: 8% │ Crit Temp Time: 0 min
──────────────────────────────────────────────────────────────────────────────────────────────────────────
  [!] WARNING: Thermal throttling likely impacted the results!
```

### 2. Sustained Write Profile with SLC Cliff Detection
```text
════════════════════════════════════════════════════════════════════════
  dspd Sustained Write Profile — Drive C: (Samsung SSD 990 PRO 2TB)
────────────────────────────────────────────────────────────────────────
  Throughput Curve (50.0 GiB written over 18s):
  █████████████████████████████▅▃▂▂▂▂▂▂
────────────────────────────────────────────────────────────────────────
  Burst Write Speed     :   4850.2 MB/s (initial pseudo-SLC plateau)
  SLC Cache Cliff Point :     42.1 GiB (drop of 68.4% after 42.1 GiB written)
  Sustained Write Speed :   1532.8 MB/s (steady-state direct-to-TLC/QLC)
  Throughput Stability  : CV  34.2% (peak: 4920.1 MB/s, min: 1480.0 MB/s)
────────────────────────────────────────────────────────────────────────
  Thermal Trajectory    : Temp: 48°C -> 66°C peak (end: 60°C)
  SMART Health          : Spare: 100% │ Life Used: 8% │ Crit Temp Time: 0 min
════════════════════════════════════════════════════════════════════════
```

---

## Benchmark Specifications

Running `dspd.exe --validate` outputs the active specifications:

```text
dspd benchmark standard profile configurations:

  [SEQ1M Q8T1    Read       ] Block:   1 MiB | QD:  8 | Pattern: Sequential | Duration: 5s (warmup: 1s)
  [SEQ1M Q8T1    Write      ] Block:   1 MiB | QD:  8 | Pattern: Sequential | Duration: 5s (warmup: 1s)
  [SEQ1M Q1T1    Read       ] Block:   1 MiB | QD:  1 | Pattern: Sequential | Duration: 5s (warmup: 1s)
  [SEQ1M Q1T1    Write      ] Block:   1 MiB | QD:  1 | Pattern: Sequential | Duration: 5s (warmup: 1s)
  [RND4K Q32T1   Read       ] Block:   4 KiB | QD: 32 | Pattern: Random     | Duration: 5s (warmup: 1s)
  [RND4K Q32T1   Write      ] Block:   4 KiB | QD: 32 | Pattern: Random     | Duration: 5s (warmup: 1s)
  [RND4K Q1T1    Read       ] Block:   4 KiB | QD:  1 | Pattern: Random     | Duration: 5s (warmup: 1s)
  [RND4K Q1T1    Write      ] Block:   4 KiB | QD:  1 | Pattern: Random     | Duration: 5s (warmup: 1s)

dspd domain workload presets:

  [directstorage Read       ] Block: 32K/64K | QD: 64 | Pattern: Random     | Duration: 5s (warmup: 1s)
  [llm-stream    Read       ] Block:   2 MiB | QD:  8 | Pattern: Sequential | Duration: 5s (warmup: 1s)
  [oltp-db       Mixed 70:30] Block:   8 KiB | QD: 32 | Pattern: Random     | Duration: 5s (warmup: 1s)
```

---

## License

This project is licensed under the [MIT License](LICENSE).  
Copyright (c) 2026 itznan.
