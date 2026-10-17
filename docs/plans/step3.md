# Step 3 — Full Profiles + Drive Discovery + Reports

> **Goal:** All 4 CrystalDiskMark profiles × read + write, automated drive discovery with `IOCTL_STORAGE_QUERY_PROPERTY`, test-file `Drop` guard, Ctrl+C cleanup, and CSV + HTML reports saved to the Desktop.

---

## New / modified files

| File | Status |
|---|---|
| `src/engine/profile.rs` | MODIFY — add `run_all_profiles()` orchestrator |
| `src/drives.rs` | NEW — drive enumeration + IOCTL queries |
| `src/report/mod.rs` | NEW |
| `src/report/csv.rs` | NEW — `csv` crate output |
| `src/report/html.rs` | NEW — self-contained dark HTML |
| `src/main.rs` | MODIFY — wire drives, all profiles, reports, guards |

---

## `src/drives.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Drive enumeration and property queries via Win32 IOCTL.

use anyhow::Result;
use thiserror::Error;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, DRIVE_FIXED, DRIVE_REMOVABLE,
};
use windows::Win32::System::Ioctl::{
    DeviceIoControl, PropertyStandardQuery, StorageDeviceProperty,
    StorageDeviceSeekPenaltyProperty, DEVICE_SEEK_PENALTY_DESCRIPTOR,
    IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusType {
    NVMe,
    SATA,
    USB,
    Unknown(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    SSD,
    HDD,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub letter:     char,
    pub model:      String,
    pub bus_type:   BusType,
    pub media_type: MediaType,
    pub capacity:   u64,   // bytes
    pub free:       u64,   // bytes
}

#[derive(Debug, Error)]
pub enum DriveError {
    #[error("drive {0}: not enough free space ({1} GiB free, need {2} GiB)")]
    InsufficientSpace(char, u64, u64),
    #[error("drive {0}: cannot open handle — {1}")]
    OpenFailed(char, windows::core::Error),
    #[error("drive {0}: IOCTL failed — {1}")]
    IoctlFailed(char, windows::core::Error),
}

/// Return all fixed drives (+ removable if `include_removable`).
pub fn discover(include_removable: bool) -> Vec<DriveInfo> {
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();

    for bit in 0..26u32 {
        if mask & (1 << bit) == 0 { continue; }
        let letter = char::from(b'A' + bit as u8);
        let root   = format!("{letter}:\\\0");
        let root_w: Vec<u16> = root.encode_utf16().collect();

        let drive_type = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_w.as_ptr())) };
        let is_fixed    = drive_type == DRIVE_FIXED.0;
        let is_removable = drive_type == DRIVE_REMOVABLE.0;

        if !is_fixed && !(include_removable && is_removable) { continue; }

        match query_drive(letter) {
            Ok(info)  => drives.push(info),
            Err(e)    => eprintln!("  [WARN] Skipping drive {letter}: {e}"),
        }
    }
    drives
}

fn query_drive(letter: char) -> Result<DriveInfo, DriveError> {
    let path = format!("\\\\.\\{letter}:");
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    // Open a volume handle for IOCTL queries.
    let handle: HANDLE = unsafe {
        windows::Win32::Storage::FileSystem::CreateFileW(
            windows::core::PCWSTR(path_w.as_ptr()),
            0, // no read/write access needed for property queries
            windows::Win32::Storage::FileSystem::FILE_SHARE_READ
                | windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
            None,
            windows::Win32::Storage::FileSystem::OPEN_EXISTING,
            windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|e| DriveError::OpenFailed(letter, e))?
    };
    let _guard = HandleGuard(handle);

    // --- StorageDeviceProperty → model + bus type ---
    let (model, bus_type) = query_device_property(letter, handle)?;

    // --- StorageDeviceSeekPenaltyProperty → SSD vs HDD ---
    let media_type = query_seek_penalty(letter, handle)
        .unwrap_or(MediaType::Unknown);

    // --- Capacity + free space ---
    let root = format!("{letter}:\\\0");
    let root_w: Vec<u16> = root.encode_utf16().collect();
    let mut free_bytes  = 0u64;
    let mut total_bytes = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(
            windows::core::PCWSTR(root_w.as_ptr()),
            Some(&mut free_bytes),
            Some(&mut total_bytes),
            None,
        )
        .map_err(|e| DriveError::IoctlFailed(letter, e))?;
    }

    Ok(DriveInfo {
        letter,
        model,
        bus_type,
        media_type,
        capacity: total_bytes,
        free:     free_bytes,
    })
}

fn query_device_property(letter: char, handle: HANDLE) -> Result<(String, BusType), DriveError> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType:  PropertyStandardQuery,
        AdditionalParameters: [0; 1],
    };

    // First call: get the required size.
    let mut buf = vec![0u8; 4096];
    let mut bytes_returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            buf.len() as u32,
            Some(&mut bytes_returned),
            None,
        )
        .map_err(|e| DriveError::IoctlFailed(letter, e))?;
    }

    // SAFETY: the kernel wrote a valid STORAGE_DEVICE_DESCRIPTOR into buf.
    let desc: &STORAGE_DEVICE_DESCRIPTOR =
        unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };

    // Extract model string (null-terminated ASCII at byte offset ProductIdOffset).
    let model = if desc.ProductIdOffset != 0 {
        let start = desc.ProductIdOffset as usize;
        let end   = buf[start..].iter().position(|&b| b == 0).map_or(start, |n| start + n);
        String::from_utf8_lossy(&buf[start..end]).trim().to_string()
    } else {
        "Unknown".to_string()
    };

    // BusType values from STORAGE_BUS_TYPE enum:
    // BusTypeNvme = 0x11 (17), BusTypeNvmeof = 0x14, BusTypeSata = 0x0B (11), BusTypeAta = 0x03, BusTypeSas = 0x0A, BusTypeUsb = 0x07
    let bus_type = match desc.BusType.0 {
        0x11 | 0x14 => BusType::NVMe,
        0x0B | 0x0A | 0x03 | 0x01 => BusType::SATA,
        0x07 => BusType::USB,
        n    => BusType::Unknown(n as u32),
    };

    Ok((model, bus_type))
}

fn query_seek_penalty(letter: char, handle: HANDLE) -> Result<MediaType, DriveError> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType:  PropertyStandardQuery,
        AdditionalParameters: [0; 1],
    };
    let mut desc: DEVICE_SEEK_PENALTY_DESCRIPTOR = unsafe { std::mem::zeroed() };
    let mut bytes_returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut desc as *mut _ as *mut _),
            std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut bytes_returned),
            None,
        )
        .map_err(|e| DriveError::IoctlFailed(letter, e))?;
    }
    Ok(if desc.IncursSeekPenalty.as_bool() { MediaType::HDD } else { MediaType::SSD })
}

struct HandleGuard(HANDLE);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe { let _ = windows::Win32::Foundation::CloseHandle(self.0); }
    }
}
```

---

## Test file `Drop` guard

```rust
// in src/main.rs (or src/engine/mod.rs)
struct FileGuard(std::path::PathBuf);
impl Drop for FileGuard {
    fn drop(&mut self) {
        if self.0.exists() {
            if let Err(e) = std::fs::remove_file(&self.0) {
                eprintln!("[WARN] Could not delete test file {}: {e}", self.0.display());
            } else {
                println!("[INFO] Deleted test file {}", self.0.display());
            }
        }
    }
}
```

The `FileGuard` is created immediately after the test file is opened, ensuring deletion on:
- Normal exit
- `panic!` (Rust runs `Drop` on unwind)
- Ctrl+C (ctrlc handler sets an `AtomicBool`; main loop checks it and breaks, causing the guard to drop)

---

## `src/report/mod.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

pub mod csv;
pub mod html;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ReportRow {
    pub profile:       String,
    pub direction:     String,   // "Read" | "Write"
    pub mb_s:          f64,
    pub iops:          f64,
    pub avg_lat_us:    f64,
    pub p99_lat_us:    u64,
}
```

---

## `src/report/csv.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use anyhow::Result;
use super::ReportRow;

pub fn write(path: &std::path::Path, rows: &[ReportRow]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    for row in rows {
        wtr.serialize(row)?;
    }
    wtr.flush()?;
    Ok(())
}
```

---

## `src/report/html.rs`

Self-contained dark-themed HTML. Highlights (inline CSS, no external dependencies):

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

pub fn write(path: &std::path::Path, rows: &[super::ReportRow], drive: &crate::drives::DriveInfo) -> anyhow::Result<()> {
    use std::fmt::Write as FmtWrite;
    let mut html = String::new();

    writeln!(html, r#"<!DOCTYPE html><html lang="en"><head><meta charset="UTF-8">
<title>dspd report</title>
<style>
  body {{ background:#1a1a2e; color:#eee; font-family:monospace; padding:2rem; }}
  h1   {{ color:#e94560; }}
  table {{ border-collapse:collapse; margin-top:1rem; width:100%; }}
  th   {{ background:#16213e; color:#0f3460; padding:.5rem 1rem; text-align:left; }}
  td   {{ padding:.4rem 1rem; border-bottom:1px solid #333; }}
  tr:nth-child(even) {{ background:#16213e44; }}
  .read  {{ color:#4ecca3; }}
  .write {{ color:#e94560; }}
  footer {{ margin-top:2rem; font-size:.75rem; color:#888; }}
</style></head><body>"#)?;

    writeln!(html, "<h1>dspd Disk Benchmark Report</h1>")?;
    writeln!(html, "<p>Drive: <b>{} ({})</b> — {} — {:.1} GB total</p>",
        drive.letter, drive.model,
        format!("{:?}", drive.bus_type),
        drive.capacity as f64 / 1e9)?;

    // Summary table (CrystalDiskMark layout)
    writeln!(html, "<h2>Results</h2><table><tr><th>Profile</th><th>Read MB/s</th><th>Write MB/s</th><th>Read IOPS</th><th>Write IOPS</th><th>Read avg lat µs</th><th>Write avg lat µs</th><th>Read p99 lat µs</th><th>Write p99 lat µs</th></tr>")?;

    // Group rows by profile name
    let profiles = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];
    for profile in profiles {
        let read  = rows.iter().find(|r| r.profile == profile && r.direction == "Read");
        let write = rows.iter().find(|r| r.profile == profile && r.direction == "Write");
        let fmt = |o: Option<&super::ReportRow>, f: fn(&super::ReportRow) -> String| {
            o.map(f).unwrap_or_else(|| "N/A".to_string())
        };
        writeln!(html,
            "<tr><td>{profile}</td><td class='read'>{}</td><td class='write'>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            fmt(read,  |r| format!("{:.1}", r.mb_s)),
            fmt(write, |r| format!("{:.1}", r.mb_s)),
            fmt(read,  |r| format!("{:.0}", r.iops)),
            fmt(write, |r| format!("{:.0}", r.iops)),
            fmt(read,  |r| format!("{:.1}", r.avg_lat_us)),
            fmt(write, |r| format!("{:.1}", r.avg_lat_us)),
            fmt(read,  |r| format!("{}", r.p99_lat_us)),
            fmt(write, |r| format!("{}", r.p99_lat_us)),
        )?;
    }

    writeln!(html, "</table>")?;
    writeln!(html, "<footer>Generated by dspd by itznan, github.com/itznan/dspd</footer>")?;
    writeln!(html, "</body></html>")?;

    std::fs::write(path, html)?;
    Ok(())
}
```

---

## Report file path

```rust
// Resolve Desktop folder via SHGetKnownFolderPath(FOLDERID_Desktop)
use windows::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_Desktop, KF_FLAG_DEFAULT};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::PWSTR;

pub fn desktop_path() -> std::path::PathBuf {
    unsafe {
        let mut path_ptr = PWSTR::null();
        if SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None, &mut path_ptr).is_ok() && !path_ptr.is_null() {
            let path_str = path_ptr.to_string().unwrap_or_default();
            CoTaskMemFree(Some(path_ptr.as_ptr() as *mut _));
            if !path_str.is_empty() {
                return std::path::PathBuf::from(path_str);
            }
        }
    }
    // Fallback if Shell API fails
    std::env::var_os("USERPROFILE")
        .map(|p| std::path::PathBuf::from(p).join("Desktop"))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
```

Reports are saved as:
```
%USERPROFILE%\Desktop\dspd_report_20261017_184438.csv
%USERPROFILE%\Desktop\dspd_report_20261017_184438.html
```

---

## Unit tests added in `tests/`

### `tests/offset_alignment.rs`
```rust
#[test]
fn random_offset_is_block_aligned() {
    // Generate 10_000 random offsets; all must be divisible by block_size.
    let block_size = 4096u64;
    let file_size  = 1 << 30u64;
    let num_blocks = file_size / block_size;
    let mut rng = rand_xoshiro::Xoshiro256PlusPlus::seed_from_u64(42);
    use rand_core::RngCore;
    for _ in 0..10_000 {
        let offset = (rng.next_u64() % num_blocks) * block_size;
        assert_eq!(offset % block_size, 0);
        assert!(offset + block_size <= file_size);
    }
}
```

### `tests/histogram_stats.rs`
```rust
#[test]
fn p99_above_mean() {
    let mut hist = hdrhistogram::Histogram::<u64>::new_with_bounds(1, 3_600_000_000, 3).unwrap();
    for v in 1..=100u64 { hist.record(v).unwrap(); }
    assert!(hist.value_at_quantile(0.99) >= hist.mean() as u64);
}
```

### `tests/size_parse.rs`
```rust
// parse_size("1GiB") == 1 << 30
// parse_size("512MiB") == 512 * 1024 * 1024
// parse_size("4KiB") == 4096
#[test]
fn parse_gib() { assert_eq!(parse_size("1GiB").unwrap(), 1u64 << 30); }
#[test]
fn parse_mib() { assert_eq!(parse_size("512MiB").unwrap(), 512 * 1024 * 1024); }
```

### `tests/csv_output.rs`
```rust
#[test]
fn csv_roundtrip() {
    // write a ReportRow, read it back, assert fields match.
}
```

---

## Verification

```powershell
# All profiles, all directions
cargo run -- --drives C --passes 3 --yes

# Expect after ~5 minutes:
# - 8 result rows (4 profiles × Read/Write)
# - dspd_report_*.csv on Desktop
# - dspd_report_*.html on Desktop (open in browser)

# Unit tests
cargo test

# Clippy must be clean
cargo clippy -- -D warnings
cargo fmt --check
```

### Expected console output format
```
─────────────────────────────────────────────────────────────
 dspd v0.1.0 — C: Samsung SSD 990 Pro [NVMe, SSD, 2.0 TB]
─────────────────────────────────────────────────────────────
 Profile        │  Read MB/s │ Write MB/s
 SEQ1M  Q8T1   │    6800.4  │   5100.2
 SEQ1M  Q1T1   │    5200.1  │   4800.9
 RND4K Q32T1   │    1842.3  │   1601.7
 RND4K  Q1T1   │      72.4  │      68.1

 Profile        │  Read IOPS │ Read avg µs │ Read p99 µs
 RND4K Q32T1   │   449,780  │        71.2 │         380
 RND4K  Q1T1   │    17,676  │        56.5 │         210
─────────────────────────────────────────────────────────────
 Reports saved:
   C:\Users\itznan\Desktop\dspd_report_20261017_184438.csv
   C:\Users\itznan\Desktop\dspd_report_20261017_184438.html
```
