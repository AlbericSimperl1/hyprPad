// main rust
#![allow(dead_code)]
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
    description: String,
    width: u32,
    height: u32,
    #[serde(rename = "refreshRate")]
    fps: f64,
    x: i32,
    y: i32,
    scale: f32,
    #[serde(default)]
    vrr: bool,
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
        };
        app.log(
            "Hyprland Virtual Display controller started.",
            LogLevel::Info,
        );
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

    /// Refresh monitor list from Hyprland
    fn refresh(&mut self) {
        match hyprland::get_monitors() {
            Ok(monitors) => {
                let count = monitors.len();
                self.monitor_exists = monitors.iter().any(|m| m.name == self.config.name);
                self.monitors = monitors;
                self.last_refresh = Some(SystemTime::now());
                self.log(
                    format!("Refreshed — {count} monitor(s) active."),
                    LogLevel::Info,
                );
            }
            Err(e) => {
                self.log(format!("Failed to get monitors: {e}"), LogLevel::Error);
            }
        }
    }

    fn do_create(&mut self) {
        let name = self.config.name.clone();
        let kw = self.config.to_keyword();

        self.log(
            format!("▶ Creating virtual monitor '{name}'..."),
            LogLevel::Info,
        );
        self.log(format!("  $ hyprctl keyword monitor {kw}"), LogLevel::Info);
        self.log(
            format!("  $ hyprctl output create headless {name}"),
            LogLevel::Info,
        );

        match hyprland::create_monitor(&self.config) {
            Ok(out) => {
                self.log(
                    format!("✓ Monitor '{name}' created. {}", out.trim()),
                    LogLevel::Success,
                );
                self.refresh();
            }
            Err(e) => {
                self.log(format!("⚠ {e}"), LogLevel::Warning);
                self.refresh();
            }
        }
    }

    fn do_remove(&mut self) {
        let name = self.config.name.clone();

        self.log(
            format!("▶ Removing virtual monitor '{name}'..."),
            LogLevel::Info,
        );
        self.log(format!("  $ hyprctl output remove {name}"), LogLevel::Info);

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
            self.log("Capture already running.", LogLevel::Warning);
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
        self.log(format!("▶ Starting capture → {path}"), LogLevel::Info);
        self.log(
            "  A portal popup will appear — pick the virtual monitor.",
            LogLevel::Info,
        );

        match capture::CaptureSession::start(path.clone()) {
            Ok(session) => {
                self.capture = Some(session);
                self.log(
                    "✓ Capture session started. Select the monitor in the popup.",
                    LogLevel::Success,
                );
            }
            Err(e) => {
                self.log(format!("✗ Failed to start capture: {e}"), LogLevel::Error);
            }
        }
    }

    fn do_stop_capture(&mut self) {
        if let Some(mut session) = self.capture.take() {
            self.log(
                "▶ Stopping capture (finalizing MP4 in background)...",
                LogLevel::Info,
            );

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
                ui.strong("Monitor Configuration");
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

            // ═══ Capture ════════════════════════════════════════
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.strong("🎥  Capture");
                ui.add_space(4.0);

                let status = self
                    .capture
                    .as_ref()
                    .map(|c| c.status())
                    .unwrap_or(capture::CaptureStatus::Idle);

                let running = matches!(status, capture::CaptureStatus::Capturing { .. })
                    || matches!(status, capture::CaptureStatus::Starting(_));

                // Also consider "stopping" as busy — prevents double-clicks.
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
        });
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
