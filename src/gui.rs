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
