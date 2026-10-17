// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Graphical user interface (GUI) subsystem for dspd.

pub mod app;
pub mod theme;

use anyhow::Result;
use app::DspdApp;
use eframe::egui::Vec2;
use theme::apply_theme;

/// Launch the desktop GUI window.
pub fn run_gui() -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(820.0, 580.0))
            .with_min_inner_size(Vec2::new(720.0, 480.0))
            .with_title("dspd - Disk Benchmark"),
        ..Default::default()
    };

    eframe::run_native(
        "dspd",
        native_options,
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(DspdApp::new(cc)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI execution error: {e}"))?;

    Ok(())
}
