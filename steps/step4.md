# Step 4 — CI & Release Workflows

> **Goal:** Automate quality checks on every push/PR and produce signed, checksummed release binaries for both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` on every version tag.

---

## New files

| File | Purpose |
|---|---|
| `.github/workflows/ci.yml` | fmt + clippy + tests on every push and PR |
| `.github/workflows/release.yml` | Build + zip + SHA256 + GitHub Release on tag `v*` |

---

## `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: ["**"]
  pull_request:
    branches: ["**"]

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: fmt / clippy / test
    runs-on: windows-latest

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust stable (with clippy + rustfmt)
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - name: Cache Cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: ${{ runner.os }}-cargo-

      - name: Check formatting
        run: cargo fmt --check

      - name: Clippy (deny warnings)
        run: cargo clippy -- -D warnings

      - name: Run tests
        run: cargo test --all-features
```

---

## `.github/workflows/release.yml`

```yaml
name: Release

on:
  push:
    tags:
      - "v*"

env:
  CARGO_TERM_COLOR: always

permissions:
  contents: write   # needed to create GitHub Release + upload assets

jobs:
  build:
    name: Build ${{ matrix.target }}
    runs-on: windows-latest
    strategy:
      fail-fast: false
      matrix:
        target:
          - x86_64-pc-windows-msvc
          - aarch64-pc-windows-msvc

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust stable + target
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Cache Cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
          key: ${{ runner.os }}-${{ matrix.target }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: ${{ runner.os }}-${{ matrix.target }}-cargo-

      - name: Build release binary
        run: cargo build --release --target ${{ matrix.target }}

      - name: Package artifact
        shell: pwsh
        run: |
          $ver     = "${{ github.ref_name }}"
          $target  = "${{ matrix.target }}"
          $dir     = "dspd-${ver}-${target}"
          New-Item -ItemType Directory -Force $dir | Out-Null
          Copy-Item "target\${target}\release\dspd.exe" $dir
          Copy-Item "README.md", "LICENSE" $dir
          $zip = "${dir}.zip"
          Compress-Archive -Path $dir -DestinationPath $zip -Force
          $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
          "${hash}  ${zip}" | Out-File -Encoding ascii "${zip}.sha256"
          Write-Host "Packaged: $zip  ($hash)"

      - name: Upload to GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            dspd-${{ github.ref_name }}-${{ matrix.target }}.zip
            dspd-${{ github.ref_name }}-${{ matrix.target }}.zip.sha256
          generate_release_notes: true
```

---

## Triggering a release

```bash
# Tag and push — release.yml fires automatically
git tag v0.1.0 -m "Initial release"
git push origin v0.1.0
```

After the workflow completes (~5–10 min), the GitHub Release page will contain:

```
dspd-v0.1.0-x86_64-pc-windows-msvc.zip
dspd-v0.1.0-x86_64-pc-windows-msvc.zip.sha256
dspd-v0.1.0-aarch64-pc-windows-msvc.zip
dspd-v0.1.0-aarch64-pc-windows-msvc.zip.sha256
```

---

## ARM64 cross-compilation notes

The `aarch64-pc-windows-msvc` target is cross-compiled from `windows-latest` (x64). The MSVC linker on GitHub-hosted runners includes the ARM64 cross-linker. No extra setup is needed beyond `rustup target add aarch64-pc-windows-msvc` (handled by the workflow above via `dtolnay/rust-toolchain`).

> [!NOTE]
> The `windows` crate compiles normally for ARM64 because it generates bindings at compile time and does not contain x64-only intrinsics.

> [!WARNING]
> You cannot run the ARM64 binary on the x64 GitHub runner — only the x64 binary is smoke-tested via `cargo test` in `ci.yml`. ARM64 correctness relies on the shared code path and Windows emulation layer on ARM devices.

---

## README badges (add at top of README.md)

```markdown
[![CI](https://github.com/itznan/dspd/actions/workflows/ci.yml/badge.svg)](https://github.com/itznan/dspd/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Latest Release](https://img.shields.io/github/v/release/itznan/dspd)](https://github.com/itznan/dspd/releases/latest)
```

---

## Verification

```powershell
# Local pre-release check
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build --release

# Confirm the release binary works
.\target\release\dspd.exe --version
# Expected: dspd 0.1.0

# Check file size (should be small — stripped + LTO)
(Get-Item .\target\release\dspd.exe).Length / 1MB
# Expect: ~2–5 MB depending on linked Windows APIs
```

### After tagging
1. Go to `https://github.com/itznan/dspd/actions` → confirm both matrix jobs are green.
2. Go to `https://github.com/itznan/dspd/releases` → confirm 4 assets are attached.
3. Download `dspd-v0.1.0-x86_64-pc-windows-msvc.zip`, extract, run `dspd.exe --version`.
4. Verify SHA256:
   ```powershell
   $expected = Get-Content .\dspd-v0.1.0-x86_64-pc-windows-msvc.zip.sha256
   $actual   = (Get-FileHash .\dspd-v0.1.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash.ToLower()
   $expected -match $actual   # must be True
   ```
