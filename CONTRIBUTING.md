# Contributing to dspd

Thank you for your interest in contributing to **dspd**! We welcome bug reports, feature requests, documentation improvements, and code contributions.

---

## Code of Conduct

Please be respectful, constructive, and collaborative in all discussions and code reviews.

---

## Development Setup

### Prerequisites
- Windows 10 or Windows 11 (x64 or ARM64)
- Rust stable toolchain (`stable-x86_64-pc-windows-msvc` or `stable-aarch64-pc-windows-msvc`)
- Visual Studio C++ Build Tools with Windows SDK (for MSVC resource compiler `rc.exe`)

### Building from Source
```powershell
# Clone the repository
git clone https://github.com/itznan/dspd.git
cd dspd

# Build in debug mode
cargo build

# Run unit and integration tests
cargo test

# Run linter and formatter
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

---

## Guidelines for Pull Requests

1. **SPDX Headers**: Every new source file must include the SPDX license identifier on line 1:
   ```rust
   // SPDX-License-Identifier: MIT
   // Copyright (c) 2026 itznan
   ```
2. **Safety & Unsafe**: All Win32 direct I/O interactions must isolate `unsafe` blocks with detailed `// SAFETY:` comments explaining invariants.
3. **Quality Bar**:
   - `cargo fmt --check` must be clean.
   - `cargo clippy --all-targets -- -D warnings` must produce zero warnings.
   - `cargo test` must pass all test suites.
   - No `unwrap()` in non-test runtime code (use `anyhow::Result` or `thiserror`).

---

## Submitting Changes

1. Fork the repository and create your feature branch from `main`:
   ```bash
   git checkout -b feature/my-new-feature
   ```
2. Commit your changes with clear, descriptive commit messages.
3. Push to your fork and submit a Pull Request.
