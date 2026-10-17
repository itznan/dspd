// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Theme and widget styling inspired by clean native desktop controls.

use eframe::egui::{self, Color32, Rounding, Stroke, Visuals};

/// Apply clean light desktop theme matching the reference UI styling.
pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::light();

    // Neutral desktop background matching reference UI
    visuals.panel_fill = Color32::from_rgb(238, 238, 238);
    visuals.window_fill = Color32::from_rgb(245, 245, 245);
    visuals.faint_bg_color = Color32::from_rgb(230, 230, 230);
    visuals.extreme_bg_color = Color32::from_rgb(255, 255, 255);

    // Accent cyan/blue (#0091ff) matching reference check/slider accents
    let accent_blue = Color32::from_rgb(0, 145, 255);
    visuals.selection.bg_fill = accent_blue;
    visuals.selection.stroke = Stroke::new(1.0_f32, accent_blue);

    // Inactive widget styling (white cards with light border)
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(255, 255, 255);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(190, 190, 190));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(30, 30, 30));
    visuals.widgets.inactive.rounding = Rounding::same(3.0);

    // Hovered widget styling
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(245, 250, 255);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent_blue);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0, 0, 0));
    visuals.widgets.hovered.rounding = Rounding::same(3.0);

    // Active widget styling
    visuals.widgets.active.bg_fill = Color32::from_rgb(230, 242, 255);
    visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, accent_blue);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0, 0, 0));
    visuals.widgets.active.rounding = Rounding::same(3.0);

    ctx.set_visuals(visuals);
}
