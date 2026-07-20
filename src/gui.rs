use crate::app::App;
use crate::capture::CaptureStatus;
use crate::types::LogLevel;
use eframe::egui;
use std::time::Duration;

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // — business logic tick (auto-refresh) —
        self.tick();
        if self.auto_refresh {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        // Houd de GUI vlot terwijl capture loopt of wordt gestopt, zodat
        // frame-counter en status updaten.
        let capturing = self.is_capturing();
        let stopping = self.is_stopping();
        if capturing || stopping {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        // Poll het achtergrond-stopresultaat (niet-blokkerend).
        self.poll_stop_result();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_width(460.0);

            self.render_header(ui);
            ui.separator();
            self.render_config(ui);
            ui.add_space(8.0);
            self.render_actions(ui);
            ui.add_space(8.0);
            self.render_capture(ui, stopping);
            ui.add_space(8.0);
            self.render_log(ui);
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

// ─── rendering helpers ────────────────────────────────────────

impl App {
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("🖥️  Hyprland Virtual Display");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🔄 Refresh").clicked() {
                    self.refresh();
                }
                ui.checkbox(&mut self.auto_refresh, "Auto");
            });
        });
    }

    fn render_config(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong("Monitor Configuration");
            ui.add_space(6.0);

            egui::Grid::new("cfg_grid")
                .num_columns(3)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    // — Name —
                    ui.label("Name:");
                    ui.add_enabled_ui(!self.monitor_exists, |ui| {
                        ui.text_edit_singleline(&mut self.config.name)
                    });
                    if self.monitor_exists {
                        ui.label(
                            egui::RichText::new("(active)")
                                .small()
                                .color(egui::Color32::from_rgb(100, 220, 100)),
                        );
                    } else {
                        ui.label("");
                    }
                    ui.end_row();

                    // — Resolution —
                    ui.label("Resolution:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.config.width)
                                .range(320..=7680)
                                .suffix(" px"),
                        );
                        ui.label("×");
                        ui.add(
                            egui::DragValue::new(&mut self.config.height)
                                .range(240..=4320)
                                .suffix(" px"),
                        );
                    });
                    ui.label("");
                    ui.end_row();

                    // — Refresh Rate —
                    ui.label("Refresh Rate:");
                    ui.add(
                        egui::DragValue::new(&mut self.config.fps)
                            .range(1..=240)
                            .suffix(" Hz"),
                    );
                    ui.label("");
                    ui.end_row();

                    // — Position —
                    ui.label("Position:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.config.x)
                                .range(-10000..=10000)
                                .prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.config.y)
                                .range(-10000..=10000)
                                .prefix("y:"),
                        );
                    });
                    ui.label(
                        egui::RichText::new("(off-screen)")
                            .small()
                            .color(egui::Color32::from_gray(120)),
                    );
                    ui.end_row();

                    // — Scale —
                    ui.label("Scale:");
                    ui.add(
                        egui::DragValue::new(&mut self.config.scale)
                            .range(0.5f32..=3.0f32)
                            .speed(0.1),
                    );
                    ui.label("");
                    ui.end_row();
                });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(" ").color(egui::Color32::from_gray(140)));
                ui.monospace(
                    egui::RichText::new(format!(
                        "hyprctl keyword monitor {}",
                        self.config.to_keyword()
                    ))
                    .small()
                    .color(egui::Color32::from_rgb(180, 180, 200)),
                );
            });
        });
    }

    fn render_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let can_create = !self.monitor_exists && !self.config.name.is_empty();
            let can_remove = self.monitor_exists;

            ui.add_enabled_ui(can_create, |ui| {
                if ui
                    .button(egui::RichText::new("✅  Create Virtual Monitor").strong())
                    .clicked()
                {
                    self.do_create();
                }
            });

            ui.add_enabled_ui(can_remove, |ui| {
                if ui
                    .button(egui::RichText::new("❌  Remove Virtual Monitor").strong())
                    .clicked()
                {
                    self.do_remove();
                }
            });
        });
    }

    fn render_capture(&mut self, ui: &mut egui::Ui, stopping: bool) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong("🎥  Capture");
            ui.add_space(4.0);

            let status = self
                .capture
                .as_ref()
                .map(|c| c.status())
                .unwrap_or(CaptureStatus::Idle);

            let running = matches!(status, CaptureStatus::Capturing { .. })
                || matches!(status, CaptureStatus::Starting(_));
            let busy = running || stopping;

            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    !busy && self.monitor_exists && !self.capture_output_path.is_empty(),
                    |ui| {
                        if ui
                            .button(egui::RichText::new("▶  Start Capture").strong())
                            .clicked()
                        {
                            self.do_start_capture();
                        }
                    },
                );
                ui.add_enabled_ui(running, |ui| {
                    if ui
                        .button(egui::RichText::new("⏹  Stop Capture").strong())
                        .clicked()
                    {
                        self.do_stop_capture();
                    }
                });
                if stopping {
                    ui.label(
                        egui::RichText::new("⏳ Finalizing...")
                            .small()
                            .color(egui::Color32::from_rgb(255, 200, 80)),
                    );
                }
            });

            ui.add_space(4.0);
            let (status_text, color) = match &status {
                CaptureStatus::Idle => ("○ Idle".to_string(), egui::Color32::from_gray(140)),
                CaptureStatus::Starting(msg) => {
                    (format!("… {msg}"), egui::Color32::from_rgb(255, 200, 80))
                }
                CaptureStatus::Capturing {
                    width,
                    height,
                    frames,
                    ..
                } => (
                    format!("● Capturing {width}×{height} — {frames} frames"),
                    egui::Color32::from_rgb(100, 220, 100),
                ),
                CaptureStatus::Finished { path, frames } => (
                    format!("✓ Saved {path} ({frames} frames)"),
                    egui::Color32::from_rgb(120, 200, 255),
                ),
                CaptureStatus::Error(e) => {
                    (format!("✗ {e}"), egui::Color32::from_rgb(255, 120, 120))
                }
            };
            ui.colored_label(color, status_text);
        });
    }

    fn render_log(&self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong("📜  Log");
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .max_height(180.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for entry in &self.log_entries {
                        let color = match entry.level {
                            LogLevel::Info => egui::Color32::from_gray(200),
                            LogLevel::Success => egui::Color32::from_rgb(100, 220, 100),
                            LogLevel::Warning => egui::Color32::from_rgb(255, 200, 80),
                            LogLevel::Error => egui::Color32::from_rgb(255, 120, 120),
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&entry.time)
                                    .small()
                                    .color(egui::Color32::from_gray(120)),
                            );
                            ui.label(egui::RichText::new(&entry.message).color(color));
                        });
                    }
                });
        });
    }
}

// use crate::app::App;
// use crate::capture::CaptureStatus;
// use crate::types::LogLevel;
// use eframe::egui;
// use std::time::Duration;

// // ─── Omatunes-achtige Kleurenpalet ────────────────────────────
// const ACCENT: egui::Color32 = egui::Color32::from_rgb(129, 140, 248); // Zachs Indigo/Paars
// const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(165, 180, 252);
// const DANGER: egui::Color32 = egui::Color32::from_rgb(248, 113, 113); // Zacht Rood
// const DANGER_HOVER: egui::Color32 = egui::Color32::from_rgb(252, 165, 165);

// const BG_MAIN: egui::Color32 = egui::Color32::from_rgb(15, 15, 20);
// const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(23, 23, 31);
// const BG_TERMINAL: egui::Color32 = egui::Color32::from_rgb(10, 10, 14);

// const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(240, 240, 245);
// const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(148, 163, 184);

// impl eframe::App for App {
//     fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
//         // ─── 1. Custom Styling & Theme Setup ──────────────────────────
//         configure_style(ctx);

//         // — business logic tick (auto-refresh) —
//         self.tick();
//         if self.auto_refresh {
//             ctx.request_repaint_after(Duration::from_secs(1));
//         }

//         let capturing = self.is_capturing();
//         let stopping = self.is_stopping();
//         if capturing || stopping {
//             ctx.request_repaint_after(Duration::from_millis(250));
//         }

//         self.poll_stop_result();

//         // ─── 2. Layout Container ──────────────────────────────────────
//         egui::CentralPanel::default()
//             .frame(
//                 egui::Frame::none()
//                     .fill(BG_MAIN)
//                     .inner_margin(egui::Margin::same(24.0)),
//             )
//             .show(ctx, |ui| {
//                 egui::ScrollArea::vertical()
//                     .auto_shrink([false, false])
//                     .show(ui, |ui| {
//                         ui.set_min_width(ui.available_width() - 8.0);
//                         ui.spacing_mut().item_spacing.y = 16.0;

//                         self.render_header(ui);
//                         self.render_config_card(ui);
//                         self.render_action_buttons(ui);
//                         self.render_capture_card(ui, stopping);
//                         self.render_log_card(ui);
//                     });
//             });
//     }

//     fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
//         self.shutdown();
//     }
// }

// // ─── UI Rendering Helpers ─────────────────────────────────────

// impl App {
//     fn render_header(&mut self, ui: &mut egui::Ui) {
//         ui.horizontal(|ui| {
//             ui.heading(
//                 egui::RichText::new("Hyprland Virtual Display")
//                     .color(TEXT_PRIMARY)
//                     .size(22.0)
//                     .strong(),
//             );

//             ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
//                 ui.spacing_mut().item_spacing.x = 12.0;

//                 if ui
//                     .add(ghost_button("🔄 Refresh", self.auto_refresh))
//                     .clicked()
//                 {
//                     self.refresh();
//                 }

//                 // Custom toggle switch for Auto
//                 let toggle_text = if self.auto_refresh {
//                     "Auto: ON"
//                 } else {
//                     "Auto: OFF"
//                 };
//                 let toggle_color = if self.auto_refresh {
//                     ACCENT
//                 } else {
//                     TEXT_MUTED
//                 };
//                 if ui
//                     .add(ghost_button_colored(toggle_text, toggle_color))
//                     .clicked()
//                 {
//                     self.auto_refresh = !self.auto_refresh;
//                 }
//             });
//         });
//     }

//     fn render_config_card(&mut self, ui: &mut egui::Ui) {
//         card_frame().show(ui, |ui| {
//             ui.set_width(ui.available_width());
//             ui.label(
//                 egui::RichText::new("Monitor Configuration")
//                     .color(TEXT_PRIMARY)
//                     .strong(),
//             );
//             ui.add_space(12.0);

//             egui::Grid::new("cfg_grid")
//                 .num_columns(3)
//                 .spacing([16.0, 12.0])
//                 .show(ui, |ui| {
//                     // — Name —
//                     ui.label(egui::RichText::new("Name").color(TEXT_MUTED));
//                     ui.add_enabled_ui(!self.monitor_exists, |ui| {
//                         ui.style_mut().visuals.widgets.inactive.bg_fill = BG_MAIN;
//                         ui.text_edit_singleline(&mut self.config.name);
//                     });
//                     if self.monitor_exists {
//                         ui.label(egui::RichText::new("● Active").color(ACCENT).small());
//                     } else {
//                         ui.label("");
//                     }
//                     ui.end_row();

//                     // — Resolution —
//                     ui.label(egui::RichText::new("Resolution").color(TEXT_MUTED));
//                     ui.horizontal(|ui| {
//                         ui.add(
//                             egui::DragValue::new(&mut self.config.width)
//                                 .range(320..=7680)
//                                 .suffix(" px"),
//                         );
//                         ui.label(egui::RichText::new("×").color(TEXT_MUTED));
//                         ui.add(
//                             egui::DragValue::new(&mut self.config.height)
//                                 .range(240..=4320)
//                                 .suffix(" px"),
//                         );
//                     });
//                     ui.label("");
//                     ui.end_row();

//                     // — Refresh Rate —
//                     ui.label(egui::RichText::new("Refresh Rate").color(TEXT_MUTED));
//                     ui.add(
//                         egui::DragValue::new(&mut self.config.fps)
//                             .range(1..=240)
//                             .suffix(" Hz"),
//                     );
//                     ui.label("");
//                     ui.end_row();

//                     // — Position —
//                     ui.label(egui::RichText::new("Position").color(TEXT_MUTED));
//                     ui.horizontal(|ui| {
//                         ui.add(
//                             egui::DragValue::new(&mut self.config.x)
//                                 .range(-10000..=10000)
//                                 .prefix("x:"),
//                         );
//                         ui.add(
//                             egui::DragValue::new(&mut self.config.y)
//                                 .range(-10000..=10000)
//                                 .prefix("y:"),
//                         );
//                     });
//                     ui.label(
//                         egui::RichText::new("(off-screen)")
//                             .color(TEXT_MUTED)
//                             .small(),
//                     );
//                     ui.end_row();

//                     // — Scale —
//                     ui.label(egui::RichText::new("Scale").color(TEXT_MUTED));
//                     ui.add(
//                         egui::DragValue::new(&mut self.config.scale)
//                             .range(0.5f32..=3.0f32)
//                             .speed(0.1),
//                     );
//                     ui.label("");
//                     ui.end_row();
//                 });

//             ui.add_space(12.0);

//             // Terminal-achtige weergave van het commando
//             terminal_frame().show(ui, |ui| {
//                 ui.monospace(
//                     egui::RichText::new(format!(
//                         "$ hyprctl keyword monitor {}",
//                         self.config.to_keyword()
//                     ))
//                     .color(egui::Color32::from_rgb(130, 200, 255)),
//                 );
//             });
//         });
//     }

//     fn render_action_buttons(&mut self, ui: &mut egui::Ui) {
//         ui.horizontal(|ui| {
//             ui.spacing_mut().item_spacing.x = 12.0;

//             let can_create = !self.monitor_exists && !self.config.name.is_empty();
//             let can_remove = self.monitor_exists;

//             if ui
//                 .add_enabled(
//                     can_create,
//                     accent_button("✅  Create", ACCENT, ACCENT_HOVER),
//                 )
//                 .clicked()
//             {
//                 self.do_create();
//             }

//             if ui
//                 .add_enabled(
//                     can_remove,
//                     accent_button("❌  Remove", DANGER, DANGER_HOVER),
//                 )
//                 .clicked()
//             {
//                 self.do_remove();
//             }
//         });
//     }

//     fn render_capture_card(&mut self, ui: &mut egui::Ui, stopping: bool) {
//         card_frame().show(ui, |ui| {
//             ui.set_width(ui.available_width());
//             ui.horizontal(|ui| {
//                 ui.label(
//                     egui::RichText::new("🎥  Capture")
//                         .color(TEXT_PRIMARY)
//                         .strong(),
//                 );

//                 let status = self
//                     .capture
//                     .as_ref()
//                     .map(|c| c.status())
//                     .unwrap_or(CaptureStatus::Idle);
//                 let running = matches!(status, CaptureStatus::Capturing { .. })
//                     || matches!(status, CaptureStatus::Starting(_));
//                 let busy = running || stopping;

//                 ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
//                     if stopping {
//                         ui.label(
//                             egui::RichText::new("⏳ Finalizing...")
//                                 .color(ACCENT)
//                                 .small(),
//                         );
//                     }
//                     if ui
//                         .add_enabled(running, ghost_button("⏹  Stop", true))
//                         .clicked()
//                     {
//                         self.do_stop_capture();
//                     }
//                     if ui
//                         .add_enabled(
//                             !busy && self.monitor_exists && !self.capture_output_path.is_empty(),
//                             accent_button("▶  Start", ACCENT, ACCENT_HOVER),
//                         )
//                         .clicked()
//                     {
//                         self.do_start_capture();
//                     }
//                 });
//             });

//             ui.add_space(8.0);

//             let status = self
//                 .capture
//                 .as_ref()
//                 .map(|c| c.status())
//                 .unwrap_or(CaptureStatus::Idle);
//             let (status_text, color) = match &status {
//                 CaptureStatus::Idle => ("○ Idle".to_string(), TEXT_MUTED),
//                 CaptureStatus::Starting(msg) => (format!("… {msg}"), ACCENT),
//                 CaptureStatus::Capturing {
//                     width,
//                     height,
//                     frames,
//                     ..
//                 } => (
//                     format!("● Capturing {width}×{height} — {frames} frames"),
//                     egui::Color32::from_rgb(100, 220, 100),
//                 ),
//                 CaptureStatus::Finished { path, frames } => (
//                     format!("✓ Saved {path} ({frames} frames)"),
//                     egui::Color32::from_rgb(120, 200, 255),
//                 ),
//                 CaptureStatus::Error(e) => (format!("✗ {e}"), DANGER),
//             };

//             ui.label(egui::RichText::new(status_text).color(color));
//         });
//     }

//     fn render_log_card(&self, ui: &mut egui::Ui) {
//         card_frame().show(ui, |ui| {
//             ui.set_width(ui.available_width());
//             ui.label(
//                 egui::RichText::new("📜  Activity Log")
//                     .color(TEXT_PRIMARY)
//                     .strong(),
//             );
//             ui.add_space(8.0);

//             terminal_frame().show(ui, |ui| {
//                 egui::ScrollArea::vertical()
//                     .max_height(160.0)
//                     .auto_shrink([false, true])
//                     .show(ui, |ui| {
//                         ui.spacing_mut().item_spacing.y = 4.0;
//                         for entry in &self.log_entries {
//                             let color = match entry.level {
//                                 LogLevel::Info => TEXT_MUTED,
//                                 LogLevel::Success => egui::Color32::from_rgb(100, 220, 100),
//                                 LogLevel::Warning => egui::Color32::from_rgb(255, 200, 80),
//                                 LogLevel::Error => DANGER,
//                             };
//                             ui.horizontal(|ui| {
//                                 ui.label(
//                                     egui::RichText::new(&entry.time)
//                                         .color(egui::Color32::from_rgb(80, 80, 100))
//                                         .monospace(),
//                                 );
//                                 ui.label(
//                                     egui::RichText::new(&entry.message).color(color).monospace(),
//                                 );
//                             });
//                         }
//                     });
//             });
//         });
//     }
// }

// // ─── Aangepaste Egui Component Frames ─────────────────────────

// fn configure_style(ctx: &egui::Context) {
//     let mut style = (*ctx.style()).clone();

//     // Algemene spacing
//     style.spacing.item_spacing = egui::vec2(8.0, 8.0);
//     style.spacing.button_padding = egui::vec2(14.0, 8.0);

//     // Afgeronde hoeken voor alles
//     style.visuals.window_rounding = egui::Rounding::same(10.0);
//     style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
//     style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
//     style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
//     style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
//     style.visuals.widgets.open.rounding = egui::Rounding::same(8.0);

//     // Dark theme overrides
//     style.visuals.dark_mode = true;
//     // style.visuals.panel_background = BG_MAIN;

//     // Tekst kleuren
//     style.visuals.override_text_color = Some(TEXT_PRIMARY);

//     // Input velden (zoals text edits en dragvalues) iets donkerder dan de panelen
//     style.visuals.widgets.inactive.bg_fill = BG_MAIN;
//     style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
//     style.visuals.widgets.hovered.bg_fill = BG_MAIN;
//     style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ACCENT);
//     style.visuals.widgets.active.bg_fill = BG_MAIN;
//     style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, ACCENT);

//     ctx.set_style(style);
// }

// fn card_frame() -> egui::Frame {
//     egui::Frame {
//         fill: BG_PANEL,
//         rounding: egui::Rounding::same(12.0),
//         inner_margin: egui::Margin::same(20.0),
//         stroke: egui::Stroke::new(1.0, egui::Color32::from_rgb(35, 35, 45)),
//         ..Default::default()
//     }
// }

// fn terminal_frame() -> egui::Frame {
//     egui::Frame {
//         fill: BG_TERMINAL,
//         rounding: egui::Rounding::same(8.0),
//         inner_margin: egui::Margin::same(12.0),
//         stroke: egui::Stroke::NONE,
//         ..Default::default()
//     }
// }

// // Custom knoppen die vloeiend overerven op de stijl
// fn accent_button(
//     text: impl Into<String>,
//     color: egui::Color32,
//     hover: egui::Color32,
// ) -> egui::Button {
//     egui::Button::new(
//         egui::RichText::new(text)
//             .color(egui::Color32::WHITE)
//             .strong(),
//     )
//     .fill(color)
//     .stroke(egui::Stroke::NONE)
//     .min_size(egui::vec2(110.0, 34.0))
//     .sense(egui::Sense::click()) // Zorgt dat we hover state kunnen afvangen
//     .ui(|btn| {
//         btn.fill = if btn.hovered() { hover } else { color };
//     })
// }

// // Knop zonder achtergrondkleur (ghost)
// fn ghost_button(text: impl Into<String>, active: bool) -> egui::Button {
//     let color = if active { TEXT_PRIMARY } else { TEXT_MUTED };
//     egui::Button::new(egui::RichText::new(text).color(color))
//         .fill(egui::Color32::TRANSPARENT)
//         .stroke(egui::Stroke::NONE)
//         .min_size(egui::vec2(90.0, 32.0))
// }

// fn ghost_button_colored(text: impl Into<String>, color: egui::Color32) -> egui::Button {
//     egui::Button::new(egui::RichText::new(text).color(color))
//         .fill(egui::Color32::TRANSPARENT)
//         .stroke(egui::Stroke::NONE)
//         .min_size(egui::vec2(90.0, 32.0))
// }
