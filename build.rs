// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();

        let is_release = std::env::var("PROFILE").as_deref() == Ok("release");
        let force_invoker = std::env::var("DSPD_AS_INVOKER").is_ok();

        if is_release && !force_invoker {
            res.set_manifest_file("dspd.manifest");
        } else {
            // For debug/dev builds or non-interactive testing environments
            res.set_manifest(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="0.2.0.0" processorArchitecture="*" name="itznan.dspd" type="win32"/>
  <description>Fast, safe, single-binary disk benchmarking for Windows, written in Rust.</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>"#);
        }

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
