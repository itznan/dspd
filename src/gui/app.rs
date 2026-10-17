// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Main graphical application state, widget rendering, and background thread orchestration.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use eframe::egui::{self, Color32, ProgressBar, RichText, Ui};

use crate::cli::{Args, VERSION};
use crate::drives::{self, DriveInfo};
use crate::engine::profile::ALL_PROFILES;
use crate::parse_size;
use crate::report::get_desktop_dir;
use crate::report::ReportRow;
use crate::runner::benchmark_drive;
use crate::ui::format_latency;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Benchmark,
    DriveDetails,
    Settings,
}

pub enum GuiMessage {
    Status(String),
    Progress { fraction: f32, text: String },
    RowCompleted(ReportRow),
    Finished(Result<PathBuf, String>),
}

pub struct DspdApp {
    drives: Vec<DriveInfo>,
    selected_drive_idx: usize,
    active_tab: ActiveTab,

    // Benchmark settings
    size_options: Vec<&'static str>,
    selected_size_idx: usize,
    passes: u32,
    duration: u64,
    warmup: u64,
    include_removable: bool,
    skip_wear_prompt: bool,

    // Profile selections (all 4 enabled by default)
    selected_profiles: Vec<bool>,

    // Execution state
    is_running: bool,
    progress_fraction: f32,
    progress_status: String,
    cancel_flag: Option<Arc<AtomicBool>>,
    rx: Option<Receiver<GuiMessage>>,

    // Results
    results: Vec<ReportRow>,
    last_report_path: Option<PathBuf>,
    error_message: Option<String>,
}

impl Default for DspdApp {
    fn default() -> Self {
        let drives = drives::discover(false);
        Self {
            drives,
            selected_drive_idx: 0,
            active_tab: ActiveTab::Benchmark,

            size_options: vec!["256 MiB", "512 MiB", "1 GiB", "2 GiB", "4 GiB", "8 GiB"],
            selected_size_idx: 2, // 1 GiB default
            passes: 3,
            duration: 5,
            warmup: 1,
            include_removable: false,
            skip_wear_prompt: true,

            selected_profiles: vec![true, true, true, true],

            is_running: false,
            progress_fraction: 0.0,
            progress_status: "Ready".to_string(),
            cancel_flag: None,
            rx: None,

            results: Vec::new(),
            last_report_path: None,
            error_message: None,
        }
    }
}

impl DspdApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    fn refresh_drives(&mut self) {
        self.drives = drives::discover(self.include_removable);
        if self.selected_drive_idx >= self.drives.len() {
            self.selected_drive_idx = 0;
        }
    }

    fn start_benchmark(&mut self) {
        if self.drives.is_empty() {
            self.error_message = Some("No supported drives found to benchmark.".to_string());
            return;
        }

        let drive = self.drives[self.selected_drive_idx].clone();
        let size_str = self.size_options[self.selected_size_idx];
        let test_size = match parse_size(size_str) {
            Ok(s) => s,
            Err(e) => {
                self.error_message = Some(format!("Invalid test size: {e}"));
                return;
            }
        };

        // Collect selected profiles
        let mut profiles = Vec::new();
        for (i, &selected) in self.selected_profiles.iter().enumerate() {
            if selected && i < ALL_PROFILES.len() {
                profiles.push(ALL_PROFILES[i].clone());
            }
        }

        if profiles.is_empty() {
            self.error_message = Some("Please select at least one benchmark profile.".to_string());
            return;
        }

        self.is_running = true;
        self.results.clear();
        self.error_message = None;
        self.progress_fraction = 0.0;
        self.progress_status = "Preparing test file...".to_string();

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel_flag = Some(cancel.clone());

        let (tx, rx): (Sender<GuiMessage>, Receiver<GuiMessage>) = channel();
        self.rx = Some(rx);

        let passes = self.passes;
        let duration = self.duration;
        let warmup = self.warmup;
        let output_dir = get_desktop_dir();

        thread::spawn(move || {
            let args = Args {
                drives: vec![drive.letter],
                size: size_str.to_string(),
                passes,
                duration,
                warmup,
                include_removable: false,
                profile: None,
                output_dir: Some(output_dir.clone()),
                json: false,
                yes: true,
                validate: false,
                gui: false,
            };

            let _ = tx.send(GuiMessage::Status(format!(
                "Benchmarking drive {}:",
                drive.letter
            )));

            match benchmark_drive(&drive, &profiles, test_size, &args, &output_dir) {
                Ok(_) => {
                    let _ = tx.send(GuiMessage::Finished(Ok(output_dir)));
                }
                Err(e) => {
                    let _ = tx.send(GuiMessage::Finished(Err(e.to_string())));
                }
            }
        });
    }

    fn cancel_benchmark(&mut self) {
        if let Some(cancel) = &self.cancel_flag {
            cancel.store(true, Ordering::SeqCst);
        }
        self.progress_status = "Cancelled by user.".to_string();
        self.is_running = false;
    }

    fn poll_messages(&mut self) {
        let mut finished = false;
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    GuiMessage::Status(s) => self.progress_status = s,
                    GuiMessage::Progress { fraction, text } => {
                        self.progress_fraction = fraction;
                        self.progress_status = text;
                    }
                    GuiMessage::RowCompleted(row) => self.results.push(row),
                    GuiMessage::Finished(res) => {
                        finished = true;
                        match res {
                            Ok(dir) => {
                                self.progress_status =
                                    "Benchmark completed successfully!".to_string();
                                self.progress_fraction = 1.0;
                                self.last_report_path = Some(dir);
                            }
                            Err(e) => {
                                self.error_message = Some(e);
                                self.progress_status = "Benchmark failed.".to_string();
                            }
                        }
                    }
                }
            }
        }
        if finished {
            self.is_running = false;
            self.cancel_flag = None;
        }
    }
}

impl eframe::App for DspdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages();

        // Top Menu Bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Refresh Drives").clicked() {
                        self.refresh_drives();
                        ui.close_menu();
                    }
                    if ui.button("Open Reports Folder").clicked() {
                        let dir = get_desktop_dir();
                        let _ = std::process::Command::new("explorer").arg(dir).spawn();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Tabs", |ui| {
                    if ui
                        .selectable_value(&mut self.active_tab, ActiveTab::Benchmark, "Benchmark")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if ui
                        .selectable_value(
                            &mut self.active_tab,
                            ActiveTab::DriveDetails,
                            "Drive Details",
                        )
                        .clicked()
                    {
                        ui.close_menu();
                    }
                    if ui
                        .selectable_value(&mut self.active_tab, ActiveTab::Settings, "Settings")
                        .clicked()
                    {
                        ui.close_menu();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("dspd v{VERSION}")).weak());
                });
            });
        });

        // Tab Navigation Strip (styled like reference screenshot)
        egui::TopBottomPanel::top("tabs_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::Benchmark,
                    "  Tab 1: Benchmark  ",
                );
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::DriveDetails,
                    "  Tab 2: Drive Info  ",
                );
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::Settings,
                    "  Tab 3: Settings  ",
                );
            });
            ui.add_space(4.0);
        });

        // Main Content Panel
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.error_message {
                ui.colored_label(Color32::from_rgb(200, 30, 30), format!("⚠ Error: {err}"));
                ui.add_space(4.0);
            }

            match self.active_tab {
                ActiveTab::Benchmark => self.render_benchmark_tab(ui),
                ActiveTab::DriveDetails => self.render_drive_details_tab(ui),
                ActiveTab::Settings => self.render_settings_tab(ui),
            }
        });

        if self.is_running {
            ctx.request_repaint();
        }
    }
}

impl DspdApp {
    fn render_benchmark_tab(&mut self, ui: &mut Ui) {
        ui.columns(2, |columns| {
            // Left Column: Options & Profile Checkboxes
            columns[0].group(|ui| {
                ui.label(RichText::new("Benchmark Profiles").strong());
                ui.add_space(4.0);

                let profile_names = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];
                for (i, name) in profile_names.iter().enumerate() {
                    ui.checkbox(&mut self.selected_profiles[i], *name);
                }

                ui.separator();
                ui.label(RichText::new("Safety & Options").strong());
                ui.add_space(4.0);
                ui.checkbox(&mut self.skip_wear_prompt, "Skip SSD Wear Prompt");
                if ui
                    .checkbox(&mut self.include_removable, "Include Removable Drives")
                    .changed()
                {
                    self.refresh_drives();
                }
            });

            // Right Column: Controls & Dropdowns (matching Combo Box & Button in reference screenshot)
            columns[1].group(|ui| {
                ui.label(RichText::new("Target Drive").strong());
                if self.drives.is_empty() {
                    ui.label("No drives found.");
                } else {
                    let current_drive = &self.drives[self.selected_drive_idx];
                    let label = format!(
                        "{}: {} [{}]",
                        current_drive.letter, current_drive.model, current_drive.media_type
                    );
                    egui::ComboBox::from_id_salt("drive_select")
                        .width(260.0)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (idx, d) in self.drives.iter().enumerate() {
                                let item_label =
                                    format!("{}: {} [{}]", d.letter, d.model, d.media_type);
                                ui.selectable_value(&mut self.selected_drive_idx, idx, item_label);
                            }
                        });
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Size:");
                    egui::ComboBox::from_id_salt("size_select")
                        .width(100.0)
                        .selected_text(self.size_options[self.selected_size_idx])
                        .show_ui(ui, |ui| {
                            for (idx, &sz) in self.size_options.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_size_idx, idx, sz);
                            }
                        });

                    ui.label("Passes:");
                    ui.add(egui::Slider::new(&mut self.passes, 1..=9));
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if !self.is_running {
                        let btn =
                            egui::Button::new(RichText::new("  ▶  Start Benchmark  ").strong())
                                .fill(Color32::from_rgb(0, 145, 255));
                        if ui.add(btn).clicked() {
                            self.start_benchmark();
                        }
                    } else {
                        let btn = egui::Button::new(
                            RichText::new("  ⏹  Stop  ").strong().color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(220, 50, 50));
                        if ui.add(btn).clicked() {
                            self.cancel_benchmark();
                        }
                    }
                });
            });
        });

        ui.add_space(12.0);

        // Progress Bar & Percentage (matching reference slider/progress styling)
        ui.group(|ui| {
            ui.horizontal(|ui| {
                let pb = ProgressBar::new(self.progress_fraction)
                    .show_percentage()
                    .animate(self.is_running);
                ui.add_sized([ui.available_width() - 80.0, 20.0], pb);
                ui.label(format!("{:.0}%", self.progress_fraction * 100.0));
            });
            ui.label(
                RichText::new(&self.progress_status)
                    .small()
                    .color(Color32::from_rgb(90, 90, 90)),
            );
        });

        ui.add_space(10.0);

        // Real-Time Results Table
        ui.group(|ui| {
            ui.label(RichText::new("Performance Metrics").strong());
            ui.separator();

            egui::Grid::new("metrics_grid")
                .striped(true)
                .min_col_width(90.0)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Profile").strong());
                    ui.label(
                        RichText::new("Read (MB/s)")
                            .strong()
                            .color(Color32::from_rgb(0, 120, 215)),
                    );
                    ui.label(
                        RichText::new("Write (MB/s)")
                            .strong()
                            .color(Color32::from_rgb(46, 160, 67)),
                    );
                    ui.label(RichText::new("Read IOPS").strong());
                    ui.label(RichText::new("Write IOPS").strong());
                    ui.label(RichText::new("Read Latency").strong());
                    ui.label(RichText::new("Write Latency").strong());
                    ui.end_row();

                    let profiles = ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"];
                    for prof in profiles {
                        let read = self
                            .results
                            .iter()
                            .find(|r| r.profile == prof && r.direction == "Read");
                        let write = self
                            .results
                            .iter()
                            .find(|r| r.profile == prof && r.direction == "Write");

                        ui.label(prof);
                        ui.label(
                            read.map(|r| format!("{:.2}", r.mb_s))
                                .unwrap_or_else(|| "—".to_string()),
                        );
                        ui.label(
                            write
                                .map(|r| format!("{:.2}", r.mb_s))
                                .unwrap_or_else(|| "—".to_string()),
                        );
                        ui.label(
                            read.map(|r| format!("{:.0}", r.iops))
                                .unwrap_or_else(|| "—".to_string()),
                        );
                        ui.label(
                            write
                                .map(|r| format!("{:.0}", r.iops))
                                .unwrap_or_else(|| "—".to_string()),
                        );
                        ui.label(
                            read.map(|r| format_latency(r.avg_lat_us))
                                .unwrap_or_else(|| "—".to_string()),
                        );
                        ui.label(
                            write
                                .map(|r| format_latency(r.avg_lat_us))
                                .unwrap_or_else(|| "—".to_string()),
                        );
                        ui.end_row();
                    }
                });
        });
    }

    fn render_drive_details_tab(&mut self, ui: &mut Ui) {
        ui.heading("Physical Storage Devices");
        ui.add_space(8.0);

        if self.drives.is_empty() {
            ui.label("No storage devices detected.");
            return;
        }

        for d in &self.drives {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Drive {}:", d.letter))
                            .strong()
                            .size(18.0),
                    );
                    ui.label(RichText::new(&d.model).strong());
                    ui.label(format!("[{} / {}]", d.bus_type, d.media_type));
                });
                ui.separator();

                let total_gb = d.capacity as f64 / 1_000_000_000.0;
                let free_gb = d.free as f64 / 1_000_000_000.0;
                let used_gb = total_gb - free_gb;
                let frac = if total_gb > 0.0 {
                    (used_gb / total_gb) as f32
                } else {
                    0.0
                };

                ui.label(format!(
                    "Free: {free_gb:.1} GB / Total: {total_gb:.1} GB ({:.1}% used)",
                    frac * 100.0
                ));
                ui.add(ProgressBar::new(frac));
            });
            ui.add_space(6.0);
        }
    }

    fn render_settings_tab(&mut self, ui: &mut Ui) {
        ui.heading("Benchmark & Report Settings");
        ui.add_space(8.0);

        ui.group(|ui| {
            ui.label(RichText::new("Timing Parameters").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Pass Duration (seconds):");
                ui.add(egui::Slider::new(&mut self.duration, 1..=60));
            });

            ui.horizontal(|ui| {
                ui.label("Warmup Window (seconds):");
                ui.add(egui::Slider::new(&mut self.warmup, 0..=10));
            });
        });

        ui.add_space(10.0);
        ui.group(|ui| {
            ui.label(RichText::new("Reports Directory").strong());
            ui.add_space(4.0);
            let dir = get_desktop_dir();
            ui.label(format!("Default Output: {}", dir.display()));
            if ui.button("Open Folder in Explorer").clicked() {
                let _ = std::process::Command::new("explorer").arg(dir).spawn();
            }
        });
    }
}
