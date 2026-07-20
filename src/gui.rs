use crate::app::App;
use crate::capture::CaptureStatus;
use crate::types::LogLevel;
use eframe::egui;
use std::time::Duration;

// ─── Omatunes-achtige Kleurenpalet ────────────────────────────
const ACCENT: egui::Color32 = egui::Color32::from_rgb(129, 140, 248); // Zacht Indigo/Paars
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(165, 180, 252);
const DANGER: egui::Color32 = egui::Color32::from_rgb(248, 113, 113); // Zacht Rood
const DANGER_HOVER: egui::Color32 = egui::Color32::from_rgb(252, 165, 165);

const BG_MAIN: egui::Color32 = egui::Color32::from_rgb(15, 15, 20);
const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(23, 23, 31);
const BG_TERMINAL: egui::Color32 = egui::Color32::from_rgb(10, 10, 14);

const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(240, 240, 245);
const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(148, 163, 184);

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ─── 1. Custom Styling & Theme Setup ──────────────────────────
        configure_style(ctx);

        // — business logic tick (auto-refresh) —
        self.tick();
        if self.auto_refresh {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        let capturing = self.is_capturing();
        let stopping = self.is_stopping();
        if capturing || stopping {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        self.poll_stop_result();

        // ─── 2. Layout Container ──────────────────────────────────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG_MAIN)
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width() - 8.0);
                        ui.spacing_mut().item_spacing.y = 16.0;

                        self.render_header(ui);
                        self.render_config_card(ui);
                        self.render_action_buttons(ui);
                        self.render_capture_card(ui, stopping);
                        self.render_log_card(ui);
                    });
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

// ─── UI Rendering Helpers ─────────────────────────────────────

impl App {
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(
                egui::RichText::new("Hyprland Virtual Display")
                    .color(TEXT_PRIMARY)
                    .size(22.0)
                    .strong(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;

                let refresh_color = if self.auto_refresh {
                    ACCENT
                } else {
                    TEXT_MUTED
                };
                if ui.add(ghost_button("🔄 Refresh", refresh_color)).clicked() {
                    self.refresh();
                }

                let toggle_text = if self.auto_refresh {
                    "Auto: ON"
                } else {
                    "Auto: OFF"
                };
                let toggle_color = if self.auto_refresh {
                    ACCENT
                } else {
                    TEXT_MUTED
                };
                if ui.add(ghost_button(toggle_text, toggle_color)).clicked() {
                    self.auto_refresh = !self.auto_refresh;
                }
            });
        });
    }

    fn render_config_card(&mut self, ui: &mut egui::Ui) {
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new("Monitor Configuration")
                    .color(TEXT_PRIMARY)
                    .strong(),
            );
            ui.add_space(12.0);

            egui::Grid::new("cfg_grid")
                .num_columns(3)
                .spacing([16.0, 12.0])
                .show(ui, |ui| {
                    // — Name —
                    ui.label(egui::RichText::new("Name").color(TEXT_MUTED));
                    ui.add_enabled_ui(!self.monitor_exists, |ui| {
                        ui.style_mut().visuals.widgets.inactive.bg_fill = BG_MAIN;
                        ui.text_edit_singleline(&mut self.config.name);
                    });
                    if self.monitor_exists {
                        ui.label(egui::RichText::new("● Active").color(ACCENT).small());
                    } else {
                        ui.label("");
                    }
                    ui.end_row();

                    // — Resolution —
                    ui.label(egui::RichText::new("Resolution").color(TEXT_MUTED));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.config.width)
                                .range(320..=7680)
                                .suffix(" px"),
                        );
                        ui.label(egui::RichText::new("×").color(TEXT_MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.config.height)
                                .range(240..=4320)
                                .suffix(" px"),
                        );
                    });
                    ui.label("");
                    ui.end_row();

                    // — Refresh Rate —
                    ui.label(egui::RichText::new("Refresh Rate").color(TEXT_MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.config.fps)
                            .range(1..=240)
                            .suffix(" Hz"),
                    );
                    ui.label("");
                    ui.end_row();

                    // — Position —
                    ui.label(egui::RichText::new("Position").color(TEXT_MUTED));
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
                            .color(TEXT_MUTED)
                            .small(),
                    );
                    ui.end_row();

                    // — Scale —
                    ui.label(egui::RichText::new("Scale").color(TEXT_MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.config.scale)
                            .range(0.5f32..=3.0f32)
                            .speed(0.1),
                    );
                    ui.label("");
                    ui.end_row();
                });

            ui.add_space(12.0);

            // Terminal-achtige weergave van het commando
            terminal_frame().show(ui, |ui| {
                ui.monospace(
                    egui::RichText::new(format!(
                        "$ hyprctl keyword monitor {}",
                        self.config.to_keyword()
                    ))
                    .color(egui::Color32::from_rgb(130, 200, 255)),
                );
            });
        });
    }

    fn render_action_buttons(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;

            let can_create = !self.monitor_exists && !self.config.name.is_empty();
            let can_remove = self.monitor_exists;

            // Create Button (Indigo)
            ui.scope(|ui| {
                let style = ui.style_mut();
                style.visuals.widgets.inactive.bg_fill = ACCENT;
                style.visuals.widgets.hovered.bg_fill = ACCENT_HOVER;
                style.visuals.widgets.active.bg_fill = ACCENT_HOVER;
                style.visuals.widgets.inactive.fg_stroke =
                    egui::Stroke::new(1.0, egui::Color32::WHITE);
                style.visuals.widgets.hovered.fg_stroke =
                    egui::Stroke::new(1.0, egui::Color32::WHITE);

                if ui
                    .add_enabled(
                        can_create,
                        egui::Button::new("✅  Create").min_size(egui::vec2(110.0, 34.0)),
                    )
                    .clicked()
                {
                    self.do_create();
                }
            });

            // Remove Button (Red)
            ui.scope(|ui| {
                let style = ui.style_mut();
                style.visuals.widgets.inactive.bg_fill = DANGER;
                style.visuals.widgets.hovered.bg_fill = DANGER_HOVER;
                style.visuals.widgets.active.bg_fill = DANGER_HOVER;
                style.visuals.widgets.inactive.fg_stroke =
                    egui::Stroke::new(1.0, egui::Color32::WHITE);
                style.visuals.widgets.hovered.fg_stroke =
                    egui::Stroke::new(1.0, egui::Color32::WHITE);

                if ui
                    .add_enabled(
                        can_remove,
                        egui::Button::new("❌  Remove").min_size(egui::vec2(110.0, 34.0)),
                    )
                    .clicked()
                {
                    self.do_remove();
                }
            });
        });
    }

    fn render_capture_card(&mut self, ui: &mut egui::Ui, stopping: bool) {
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Definieer status hier, zodat we het in de hele closure kunnen gebruiken
            let status = self
                .capture
                .as_ref()
                .map(|c| c.status())
                .unwrap_or(CaptureStatus::Idle);
            let running = matches!(status, CaptureStatus::Capturing { .. })
                || matches!(status, CaptureStatus::Starting(_));
            let busy = running || stopping;

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🎥  Capture")
                        .color(TEXT_PRIMARY)
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if stopping {
                        ui.label(
                            egui::RichText::new("⏳ Finalizing...")
                                .color(ACCENT)
                                .small(),
                        );
                    }

                    let stop_color = if running { DANGER } else { TEXT_MUTED };
                    if ui
                        .add_enabled(running, ghost_button("⏹  Stop", stop_color))
                        .clicked()
                    {
                        self.do_stop_capture();
                    }

                    // Start Button (Indigo)
                    ui.scope(|ui| {
                        let style = ui.style_mut();
                        style.visuals.widgets.inactive.bg_fill = ACCENT;
                        style.visuals.widgets.hovered.bg_fill = ACCENT_HOVER;
                        style.visuals.widgets.active.bg_fill = ACCENT_HOVER;
                        style.visuals.widgets.inactive.fg_stroke =
                            egui::Stroke::new(1.0, egui::Color32::WHITE);
                        style.visuals.widgets.hovered.fg_stroke =
                            egui::Stroke::new(1.0, egui::Color32::WHITE);

                        if ui
                            .add_enabled(
                                !busy
                                    && self.monitor_exists
                                    && !self.capture_output_path.is_empty(),
                                egui::Button::new("▶  Start").min_size(egui::vec2(110.0, 34.0)),
                            )
                            .clicked()
                        {
                            self.do_start_capture();
                        }
                    });
                });
            });

            ui.add_space(8.0);

            let (status_text, color) = match &status {
                CaptureStatus::Idle => ("○ Idle".to_string(), TEXT_MUTED),
                CaptureStatus::Starting(msg) => (format!("… {msg}"), ACCENT),
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
                CaptureStatus::Error(e) => (format!("✗ {e}"), DANGER),
            };

            ui.label(egui::RichText::new(status_text).color(color));
        });
    }

    fn render_log_card(&self, ui: &mut egui::Ui) {
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new("📜  Activity Log")
                    .color(TEXT_PRIMARY)
                    .strong(),
            );
            ui.add_space(8.0);

            terminal_frame().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for entry in &self.log_entries {
                            let color = match entry.level {
                                LogLevel::Info => TEXT_MUTED,
                                LogLevel::Success => egui::Color32::from_rgb(100, 220, 100),
                                LogLevel::Warning => egui::Color32::from_rgb(255, 200, 80),
                                LogLevel::Error => DANGER,
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&entry.time)
                                        .color(egui::Color32::from_rgb(80, 80, 100))
                                        .monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(&entry.message).color(color).monospace(),
                                );
                            });
                        }
                    });
            });
        });
    }
}

// ─── Aangepaste Egui Component Frames ─────────────────────────

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Algemene spacing
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);

    // Afgeronde hoeken voor alles
    style.visuals.window_rounding = egui::Rounding::same(10.0);
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.open.rounding = egui::Rounding::same(8.0);

    // Dark theme overrides
    style.visuals.dark_mode = true;
    style.visuals.code_bg_color = BG_MAIN;

    // Tekst kleuren
    style.visuals.override_text_color = Some(TEXT_PRIMARY);

    // Input velden (zoals text edits en dragvalues) iets donkerder dan de panelen
    style.visuals.widgets.inactive.bg_fill = BG_MAIN;
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.hovered.bg_fill = BG_MAIN;
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.bg_fill = BG_MAIN;
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, ACCENT);

    ctx.set_style(style);
}

fn card_frame() -> egui::Frame {
    egui::Frame {
        fill: BG_PANEL,
        rounding: egui::Rounding::same(12.0),
        inner_margin: egui::Margin::same(20.0),
        stroke: egui::Stroke::new(1.0, egui::Color32::from_rgb(35, 35, 45)),
        ..Default::default()
    }
}

fn terminal_frame() -> egui::Frame {
    egui::Frame {
        fill: BG_TERMINAL,
        rounding: egui::Rounding::same(8.0),
        inner_margin: egui::Margin::same(12.0),
        stroke: egui::Stroke::NONE,
        ..Default::default()
    }
}

// Geeft een widget terug die levenslang onafhankelijk is (geen lifetime errors meer)
fn ghost_button(text: impl Into<String>, color: egui::Color32) -> impl egui::Widget {
    egui::Button::new(egui::RichText::new(text.into()).color(color))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE)
        .min_size(egui::vec2(90.0, 32.0))
}
