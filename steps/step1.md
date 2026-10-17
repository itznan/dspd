# Step 1 — Repo Scaffold

> **Goal:** Create the bare project skeleton — metadata, license, manifest, build script, README, CHANGELOG, and .gitignore. No engine or I/O code yet. After this step, `cargo build` must succeed and produce a `dspd.exe` that prints the banner and `--help`.

---

## Files to create

| File | Purpose |
|---|---|
| `Cargo.toml` | Package metadata + all dependencies |
| `build.rs` | Embeds Windows manifest + VERSIONINFO via `winres` |
| `dspd.manifest` | UAC `requireAdministrator` + Win10/11 compat |
| `LICENSE` | MIT license text |
| `README.md` | Project documentation (badges, usage, disclaimer) |
| `CHANGELOG.md` | Version history stub |
| `.gitignore` | Rust + Windows standard ignores |
| `src/main.rs` | Entry point: banner + clap CLI skeleton |

---

## `Cargo.toml`

```toml
[package]
name        = "dspd"
version     = "0.1.0"
edition     = "2021"
authors     = ["itznan"]
repository  = "https://github.com/itznan/dspd"
license     = "MIT"
description = "Fast, safe, single-binary disk benchmarking for Windows, written in Rust."
keywords    = ["benchmark", "disk", "io", "windows", "nvme"]
categories  = ["command-line-utilities"]
build       = "build.rs"

[[bin]]
name = "dspd"

[dependencies]
anyhow       = "1.0"
thiserror    = "2.0"
clap         = { version = "4.5", features = ["derive"] }
indicatif    = "0.18"
hdrhistogram = "7.5"
rand_xoshiro = "0.8.1"
rand_core    = "0.10"
chrono       = "0.4"
csv          = "1.3"
serde        = { version = "1.0", features = ["derive"] }

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_Storage_FileSystem",
    "Win32_System_Com",
    "Win32_System_IO",
    "Win32_System_Ioctl",
    "Win32_System_Memory",
    "Win32_System_SystemInformation",
    "Win32_System_Threading",
    "Win32_UI_Shell",
] }

[build-dependencies]
winres = "0.1"

[profile.release]
opt-level     = 3
lto           = "fat"
codegen-units = 1
panic         = "abort"
strip         = true

[profile.dev]
opt-level = 1
```

---

## `build.rs`

```rust
// SPDX-License-Identifier: MIT
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("dspd.manifest");
        res.set("ProductName",     "dspd");
        res.set("FileDescription", "dspd disk benchmark");
        res.set("CompanyName",     "itznan");
        res.set("LegalCopyright",  "Copyright (c) 2026 itznan");
        res.compile().expect("winres compile failed");
    }
}
```

---

## `dspd.manifest`

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity
    version="0.1.0.0"
    processorArchitecture="*"
    name="itznan.dspd"
    type="win32"
  />
  <description>Fast, safe, single-binary disk benchmarking for Windows, written in Rust.</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 / 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
```

---

## `src/main.rs`

```rust
// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

use clap::Parser;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Parser)]
#[command(
    name    = "dspd",
    about   = "Fast, safe, single-binary disk benchmarking for Windows, written in Rust.",
    version = VERSION,
)]
struct Args {
    /// Drive letters to benchmark, comma-separated (e.g. C,D)
    #[arg(long, value_delimiter = ',')]
    drives: Vec<char>,

    /// Test file size (e.g. 1GiB, 512MiB)
    #[arg(long, default_value = "1GiB")]
    size: String,

    /// Number of passes per test
    #[arg(long, default_value_t = 3)]
    passes: u32,

    /// Duration of each pass in seconds
    #[arg(long, default_value_t = 5)]
    duration: u64,

    /// Warmup duration in seconds (discarded from results)
    #[arg(long, default_value_t = 1)]
    warmup: u64,

    /// Include removable drives
    #[arg(long)]
    include_removable: bool,

    /// Run a single named profile only
    #[arg(long)]
    profile: Option<String>,

    /// Output directory for reports (default: Desktop)
    #[arg(long)]
    output_dir: Option<std::path::PathBuf>,

    /// Skip HTML report generation
    #[arg(long)]
    no_html: bool,

    /// Skip SSD wear confirmation prompt
    #[arg(long)]
    yes: bool,

    /// Print equivalent DiskSpd command lines for each profile
    #[arg(long)]
    validate: bool,
}

fn main() -> anyhow::Result<()> {
    println!(
        "dspd v{} by itznan (github.com/itznan/dspd)",
        VERSION
    );

    let _args = Args::parse();

    // TODO (Step 2): wire engine
    println!("Engine not yet implemented — scaffold only.");
    Ok(())
}
```

---

## `LICENSE`

Standard MIT license text with `Copyright (c) 2026 itznan`.

---

## `README.md` (excerpt — key required sections)

- Badges: CI | License | Latest Release
- **Disclaimer**: "dspd is an independent open-source project, not affiliated with or endorsed by Microsoft (DiskSpd) or Crystal Dew World (CrystalDiskMark)."
- Install: download from Releases or `cargo install --git https://github.com/itznan/dspd`
- Usage examples
- Profile → CrystalDiskMark mapping table
- SSD wear warning section
- Validation guide (link to `--validate` flag)
- Contributing + License

---

## `CHANGELOG.md`

```markdown
# Changelog

## [Unreleased]

## [0.1.0] - 2026-10-17
### Added
- Initial scaffold: CLI skeleton, banner, manifest, build script.
```

---

## `.gitignore`

```
/target
Cargo.lock   # (remove this line if you want to commit the lockfile for binaries)
*.pdb
```

---

## Verification

```powershell
cargo build
.\target\debug\dspd.exe --help
# Expected: banner line + help text, exit 0
```

> **Note:** On first run, Windows will trigger a UAC prompt because the manifest requests `requireAdministrator`. This is expected.
