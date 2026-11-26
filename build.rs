// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

fn main() {
    println!("cargo:rerun-if-changed=dspd.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("dspd.manifest");
        res.set("ProductName", "dspd");
        res.set(
            "FileDescription",
            "Fast, safe, single-binary disk benchmarking for Windows, written in Rust.",
        );
        res.set("CompanyName", "itznan");
        res.set("LegalCopyright", "Copyright (c) 2026 itznan");
        res.set("FileVersion", env!("CARGO_PKG_VERSION"));
        res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        res.compile().expect("winres resource compilation failed");
    }
}
