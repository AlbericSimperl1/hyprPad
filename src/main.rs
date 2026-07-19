// main rust
mod capture;

use eframe::egui;
use serde::Deserialize;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

// types

/// Hyprland: hyprctl monitors -j
#[derive(Debug, Clone, Deserialize)]
struct MonitorJson {
    id: u64,
    name: String,
    // description: String,
    width: u32,
    height: u32,
    #[serde(rename = "refreshRate")]
    fps: f64,
    x: i32,
    y: i32,
    scale: f32,
    // #[serde(default)]
    // vrr: bool,
}

/// monitor parameters
#[derive(Debug, Clone)]
struct MonitorConfig {
    name: String,
    width: u32,
    height: u32,
    fps: u32,
    x: i32,
    y: i32,
    scale: f32,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            name: "VIRTUAL1".into(),
            width: 1600, // ipad 4/3
            height: 1200,
            fps: 60,
            x: 0,
            y: 0,
            scale: 1.0,
        }
    }
}

impl MonitorConfig {
    // formats as name, WxH@fps, XxY, scale
    fn to_keyword(&self) -> String {
        format!(
            "{},{}x{}@{},{}x{},{}",
            self.name, self.width, self.height, self.fps, self.x, self.y, self.scale
        )
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

struct LogEntry {
    time: String,
    message: String,
    level: LogLevel,
}

// Hypr IPC wrapper
mod hyprland {
    use super::*;

    fn hyprctl(args: &[&str]) -> Result<String, String> {
        // run `hyprctl` with arguments
        let output = Command::new("hyprctl")
            .args(args)
            .output()
            .map_err(|e| format!("Failed to execute hyprctl: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let msg = if stderr.is_empty() { stdout } else { stderr };
            return Err(format!("hyprctl {}: {}", args.join(" "), msg.trim()));
        }

        Ok(stdout)
    }

    pub fn get_monitors() -> Result<Vec<MonitorJson>, String> {
        // get current monitors
        let json = hyprctl(&["monitors", "-j"])?;
        serde_json::from_str(&json).map_err(|e| format!("Failed to parse monitors JSON: {e}"))
    }

    pub fn create_monitor(cfg: &MonitorConfig) -> Result<String, String> {
        // create a virtual headless monitor

        //   1: set monitor keyword
        let kw = cfg.to_keyword();
        hyprctl(&["keyword", "monitor", &kw])?;

        //   2: create headless output
        let create_args = ["output", "create", "headless", &cfg.name];
        match hyprctl(&create_args[..]) {
            Ok(out) => Ok(out),
            Err(e) => Err(format!(
                "Keyword was set, but 'output create headless' failed:\n  {e}\n\
                 Your Hyprland version might not support this command.\n\
                 The monitor rule is saved and will apply when the output appears."
            )),
        }
    }

    /// Remove a virtual headless monitor and clean up its keyword rule
    pub fn remove_monitor(name: &str) -> Result<String, String> {
        let remove_args = ["output", "remove", name]; // Type-annotatie weggehaald; grootte klopt nu automatisch
        let out = hyprctl(&remove_args[..])?; // [..] zet de array om naar een &[&str] slice

        // Clean up: disable the keyword so it doesn't linger in config
        let disable_kw = format!("{name},disable");
        let kw_args = ["keyword", "monitor", &disable_kw];
        let _ = hyprctl(&kw_args[..]); // [..] zet de array om naar een &[&str] slice

        Ok(out)
    }
}

// ─── Application State ────────────────────────────────────────

struct App {
    config: MonitorConfig,
    monitors: Vec<MonitorJson>,
    monitor_exists: bool,
    log_entries: Vec<LogEntry>,
    auto_refresh: bool,
    last_refresh: Option<SystemTime>,
    // capture
    capture: Option<capture::CaptureSession>,
    capture_output_path: String,
    /// Receiver for the final status after a background stop completes.
    stop_result_rx: Option<mpsc::Receiver<capture::CaptureStatus>>,
    /// Toont het bevestigingsvenster voordat het scherm + capture starten.
    show_create_confirm: bool,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            config: MonitorConfig::default(),
            monitors: Vec::new(),
            monitor_exists: false,
            log_entries: Vec::new(),
            auto_refresh: true,
            last_refresh: None,
            capture: None,
            capture_output_path: default_capture_path(),
            stop_result_rx: None,
            show_create_confirm: false,
        };
        app.log("Hyprland Virtual Display controller started.", LogLevel::Info);
        app.refresh();
        app
    }

    fn timestamp() -> String {
        chrono::Local::now().format("%H:%M:%S").to_string()
    }

    fn log(&mut self, msg: impl Into<String>, level: LogLevel) {
        self.log_entries.push(LogEntry {
            time: Self::timestamp(),
            message: msg.into(),
            level,
        });
        if self.log_entries.len() > 200 {
            self.log_entries.remove(0);
        }
    }

    /// Refresh monitor list from Hyprland. Logt enkel of er al een tweede
    /// (virtueel) scherm is — geen ruis bij elke refresh.
    fn refresh(&mut self) {
        match hyprland::get_monitors() {
            Ok(monitors) => {
                self.monitor_exists = monitors.iter().any(|m| m.name == self.config.name);
                self.monitors = monitors;
                self.last_refresh = Some(SystemTime::now());

                if self.monitor_exists {
                    self.log(
                        format!("Virtual monitor '{}' is active.", self.config.name),
                        LogLevel::Success,
                    );
                } else {
                    self.log(
                        "No virtual monitor detected — second screen is not active.",
                        LogLevel::Info,
                    );
                }
            }
            Err(e) => {
                self.log(format!("Failed to get monitors: {e}"), LogLevel::Error);
            }
        }
    }

    /// "Create" knop → opent het bevestigingsvenster (geen actie nog).
    fn do_create(&mut self) {
        self.show_create_confirm = true;
    }

    /// Wordt pas aangeroepen nadat de gebruiker in het bevestigingsvenster op
    /// OK heeft geklikt. Maakt het scherm aan én start direct de capture.
    fn confirm_create(&mut self) {
        let name = self.config.name.clone();
        let kw = self.config.to_keyword();
        self.log(
            format!("▶ Creating virtual monitor '{name}'  [{kw}]"),
            LogLevel::Info,
        );

        match hyprland::create_monitor(&self.config) {
            Ok(out) => {
                self.log(
                    format!("✓ Monitor '{name}' created. {}", out.trim()),
                    LogLevel::Success,
                );
                self.refresh();
                // Geen reden om een monitor te maken zonder capture — dus meteen starten.
                self.do_start_capture();
            }
            Err(e) => {
                self.log(format!("⚠ {e}"), LogLevel::Warning);
                self.refresh();
            }
        }
    }

    fn do_remove(&mut self) {
        // Capture eerst netjes afsluiten (het virtuele scherm verdwijnt toch).
        if self.capture.as_ref().map_or(false, |c| c.is_running())
            || self.stop_result_rx.is_some()
        {
            self.do_stop_capture();
        }

        let name = self.config.name.clone();
        self.log(format!("▶ Removing virtual monitor '{name}'..."), LogLevel::Info);

        match hyprland::remove_monitor(&name) {
            Ok(out) => {
                self.log(
                    format!("✓ Monitor '{name}' removed. {}", out.trim()),
                    LogLevel::Success,
                );
                self.refresh();
            }
            Err(e) => {
                self.log(format!("✗ Failed to remove: {e}"), LogLevel::Error);
            }
        }
    }

    fn do_start_capture(&mut self) {
        if self.capture.as_ref().map_or(false, |c| c.is_running()) {
            return;
        }
        if !self.monitor_exists {
            self.log(
                "Create the virtual monitor first before capturing.",
                LogLevel::Warning,
            );
            return;
        }

        let path = self.capture_output_path.clone();
        match capture::CaptureSession::start(path.clone()) {
            Ok(session) => {
                self.capture = Some(session);
                self.log(format!("✓ Capture started → {path}"), LogLevel::Success);
            }
            Err(e) => {
                self.log(format!("✗ Failed to start capture: {e}"), LogLevel::Error);
            }
        }
    }

    fn do_stop_capture(&mut self) {
        if let Some(mut session) = self.capture.take() {
            let (tx, rx) = mpsc::channel();
            self.stop_result_rx = Some(rx);

            // Move the stop+finalize work OFF the GUI thread.
            // Previously session.stop() blocked the GUI thread on
            // h.join() → feeder.join() → child.wait(), freezing the
            // entire application.
            std::thread::spawn(move || {
                // CaptureSession::stop() blocks until all threads are
                // done (PipeWire quit + FFmpeg finalize with timeout).
                session.stop();
                let final_status = session.status();
                let _ = tx.send(final_status);
                // session dropped here — all threads already joined.
            });
        }
    }

    /// Poll the background stop result. Called every GUI frame.
    /// When the stop thread completes, logs the result and clears state.
    fn poll_stop_result(&mut self) {
        let rx = match &self.stop_result_rx {
            Some(rx) => rx,
            None => return,
        };
        // Non-blocking check — if the thread hasn't finished yet,
        // try_recv returns Err(TryRecvError::Empty).
        match rx.try_recv() {
            Ok(status) => {
                self.stop_result_rx = None;
                match status {
                    capture::CaptureStatus::Finished { path, frames } => {
                        self.log(
                            format!("✓ Capture saved: {path} ({frames} frames)"),
                            LogLevel::Success,
                        );
                    }
                    capture::CaptureStatus::Error(e) => {
                        self.log(format!("✗ Capture error: {e}"), LogLevel::Error);
                    }
                    _ => {
                        self.log("Capture stopped.", LogLevel::Info);
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Still stopping — keep waiting.
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                // Thread panicked or channel closed unexpectedly.
                self.stop_result_rx = None;
                self.log("✗ Capture stop thread ended unexpectedly.", LogLevel::Error);
            }
        }
    }
}

fn default_capture_path() -> String {
    let mut p = dirs_or_tmp();
    p.push("hyprpad_capture.mp4");
    p.to_string_lossy().into_owned()
}

fn dirs_or_tmp() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("XDG_VIDEOS_DIR") {
        return std::path::PathBuf::from(d);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = std::path::PathBuf::from(home);
        p.push("Videos");
        if p.exists() {
            return p;
        }
    }
    std::path::PathBuf::from("/tmp")
}

// ─── eframe::App — GUI Rendering ──────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.auto_refresh {
            if let Some(last) = self.last_refresh {
                if last.elapsed().unwrap_or_default() >= Duration::from_secs(2) {
                    self.refresh();
                }
            }
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        // Keep the GUI lively while capture is running so the frame counter
        // and status update.
        let capturing = self
            .capture
            .as_ref()
            .map(|c| c.is_running())
            .unwrap_or(false);
        let stopping = self.stop_result_rx.is_some();
        if capturing || stopping {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        // Poll the background stop result (non-blocking).
        self.poll_stop_result();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_width(460.0);

            // ═══ Header ═══════════════════════════════════════════
            ui.horizontal(|ui| {
                ui.heading("🖥️  Hyprland Virtual Display");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🔄 Refresh").clicked() {
                        self.refresh();
                    }
                    ui.checkbox(&mut self.auto_refresh, "Auto");
                });
            });
            ui.separator();

            // ═══ Configuration ════════════════════════════════════
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.strong("⚙️  Monitor Configuration");
                ui.add_space(6.0);

                egui::Grid::new("cfg_grid")
                    .num_columns(3)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        // — Name —
                        ui.label("Name:");
                        // Opgelost: add_enabled_ui gebruikt in plaats van add_enabled
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
                                .range(0.5f32..=3.0f32) // Opgelost: f32 types expliciet gemaakt
                                .speed(0.1),
                        );
                        ui.label("");
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("→").color(egui::Color32::from_gray(140)));
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

            ui.add_space(8.0);

            // ═══ Action Buttons ═══════════════════════════════════
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

            ui.add_space(8.0);

            // ═══ Status & Monitor Table ══════════════════════════
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.strong("📊  Status");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("Virtual Monitor:");
                    if self.monitor_exists {
                        ui.colored_label(egui::Color32::from_rgb(80, 220, 100), "● Active");
                        if let Some(m) = self.monitors.iter().find(|m| m.name == self.config.name) {
                            ui.label(
                                egui::RichText::new(format!(
                                    "— {}×{} @ {:.0}Hz  (id {}, pos {},{})",
                                    m.width, m.height, m.fps, m.id, m.x, m.y
                                ))
                                .color(egui::Color32::from_gray(160)),
                            );
                        }
                    } else {
                        ui.colored_label(egui::Color32::from_gray(120), "○ Inactive");
                    }
                });

                if !self.monitors.is_empty() {
                    ui.add_space(6.0);
                    egui::Grid::new("mon_table")
                        .num_columns(6)
                        .spacing([16.0, 4.0])
                        .striped(true)
                        .min_col_width(40.0)
                        .show(ui, |ui| {
                            ui.strong("ID");
                            ui.strong("Name");
                            ui.strong("Resolution");
                            ui.strong("Refresh");
                            ui.strong("Position");
                            ui.strong("Scale");
                            ui.end_row();

                            for m in &self.monitors {
                                ui.label(format!("{}", m.id));

                                if m.name == self.config.name {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(100, 200, 255),
                                        &m.name,
                                    );
                                } else {
                                    ui.label(&m.name);
                                }

                                ui.label(format!("{}×{}", m.width, m.height));
                                ui.label(format!("{:.0} Hz", m.fps));
                                ui.label(format!("{}, {}", m.x, m.y));
                                ui.label(format!("{:.1}", m.scale));
                                ui.end_row();
                            }
                        });
                }
            });

            ui.add_space(8.0);

            // ═══ Capture status (compact) ═══════════════════════
            // Geen aparte start/stop-knoppen meer: capture start automatisch
            // na het aanmaken van het scherm, en stopt als het scherm weggaat.
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.strong("🎥  Capture");
                ui.add_space(4.0);

                let status = self
                    .capture
                    .as_ref()
                    .map(|c| c.status())
                    .unwrap_or(capture::CaptureStatus::Idle);

                let (status_text, color) = match &status {
                    capture::CaptureStatus::Idle => {
                        ("○ Idle".to_string(), egui::Color32::from_gray(140))
                    }
                    capture::CaptureStatus::Starting(msg) => {
                        (format!("… {msg}"), egui::Color32::from_rgb(255, 200, 80))
                    }
                    capture::CaptureStatus::Capturing {
                        width,
                        height,
                        frames,
                        ..
                    } => (
                        format!("● Capturing {width}×{height} — {frames} frames"),
                        egui::Color32::from_rgb(100, 220, 100),
                    ),
                    capture::CaptureStatus::Finished { path, frames } => (
                        format!("✓ Saved {path} ({frames} frames)"),
                        egui::Color32::from_rgb(120, 200, 255),
                    ),
                    capture::CaptureStatus::Error(e) => {
                        (format!("✗ {e}"), egui::Color32::from_rgb(255, 120, 120))
                    }
                };
                ui.colored_label(color, status_text);
            });

            ui.add_space(8.0);

            // ═══ Log ═════════════════════════════════════════════
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.strong("📝  Log");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Clear").clicked() {
                            self.log_entries.clear();
                        }
                    });
                });
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for entry in &self.log_entries {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&entry.time)
                                        .color(egui::Color32::from_gray(100))
                                        .monospace()
                                        .small(),
                                );
                                let color = match entry.level {
                                    LogLevel::Info => egui::Color32::from_rgb(150, 200, 255),
                                    LogLevel::Success => egui::Color32::from_rgb(100, 220, 100),
                                    LogLevel::Warning => egui::Color32::from_rgb(255, 200, 80),
                                    LogLevel::Error => egui::Color32::from_rgb(255, 120, 120),
                                };
                                ui.colored_label(color, &entry.message);
                            });
                        }
                    });
            });
        });

        // ═══ Bevestigingsvenster: Create Virtual Monitor ════════
        if self.show_create_confirm {
            // Houd de naam lokaal bij zodat we de confirm na OK kunnen afhandelen
            // zonder door self-callbacks te vechten.
            let mut clicked_ok = false;
            let mut clicked_cancel = false;

            egui::Window::new("Create Virtual Monitor")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    ui.add_space(4.0);
                    ui.label("Review the monitor configuration:");
                    ui.add_space(8.0);

                    egui::Grid::new("confirm_grid")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong("Name");
                            ui.monospace(&self.config.name);
                            ui.end_row();

                            ui.strong("Resolution");
                            ui.monospace(format!(
                                "{} × {} px",
                                self.config.width, self.config.height
                            ));
                            ui.end_row();

                            ui.strong("Refresh rate");
                            ui.monospace(format!("{} Hz", self.config.fps));
                            ui.end_row();

                            ui.strong("Position");
                            ui.monospace(format!("{} × {}", self.config.x, self.config.y));
                            ui.end_row();

                            ui.strong("Scale");
                            ui.monospace(format!("{:.2}", self.config.scale));
                            ui.end_row();

                            ui.strong("Output");
                            ui.monospace(&self.capture_output_path);
                            ui.end_row();
                        });

                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "OK creates the monitor and immediately starts capture + streaming.",
                        )
                        .small()
                        .color(egui::Color32::from_gray(150)),
                    );
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(egui::RichText::new("✅  OK").strong())
                                .clicked()
                            {
                                clicked_ok = true;
                            }
                            if ui.button("Cancel").clicked() {
                                clicked_cancel = true;
                            }
                        });
                    });
                });

            if clicked_ok {
                self.show_create_confirm = false;
                self.confirm_create();
            } else if clicked_cancel {
                self.show_create_confirm = false;
            }
        }
    }

    // close vm on exit
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Stop capture first so FFmpeg finalizes the file.
        if let Some(mut session) = self.capture.take() {
            session.stop();
            println!("✓ Capture sessie gestopt bij afsluiten.");
        }
        if self.monitor_exists {
            let name = self.config.name.clone();
            match hyprland::remove_monitor(&name) {
                Ok(_) => println!("✓ Monitor succesvol opgeruimd."),
                Err(e) => eprintln!("✗ Fout bij opruimen monitor bij afsluiten: {e}"),
            }
        }
    }
}

// ─── Main ─────────────────────────────────────────────────────

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([540.0, 740.0])
            .with_min_inner_size([440.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Hyprland Virtual Display",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}
