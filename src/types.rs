use serde::Deserialize;

/// Hyprland: `hyprctl monitors -j`
#[derive(Debug, Clone, Deserialize)]
pub struct MonitorJson {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    #[serde(rename = "refreshRate")]
    pub fps: f64,
    pub x: i32,
    pub y: i32,
    pub scale: f32,
    #[serde(default)]
    pub vrr: bool,
}

/// Monitor parameters die de gebruiker in de GUI instelt.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub x: i32,
    pub y: i32,
    pub scale: f32,
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
    /// Formatteert als `name,WxH@fps,XxY,scale`.
    pub fn to_keyword(&self) -> String {
        format!(
            "{},{}x{}@{},{}x{},{}",
            self.name, self.width, self.height, self.fps, self.x, self.y, self.scale
        )
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

pub struct LogEntry {
    pub time: String,
    pub message: String,
    pub level: LogLevel,
}
