use crate::types::{MonitorConfig, MonitorJson};
use std::process::Command;

fn hyprctl(args: &[&str]) -> Result<String, String> {
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
    let json = hyprctl(&["monitors", "-j"])?;
    serde_json::from_str(&json).map_err(|e| format!("Failed to parse monitors JSON: {e}"))
}

pub fn create_monitor(cfg: &MonitorConfig) -> Result<String, String> {
    // 1: monitor keyword zetten
    let kw = cfg.to_keyword();
    hyprctl(&["keyword", "monitor", &kw])?;

    // 2: headless output aanmaken
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

pub fn remove_monitor(name: &str) -> Result<String, String> {
    let remove_args = ["output", "remove", name];
    let out = hyprctl(&remove_args[..])?;

    // Clean up: keyword uitschakelen zodat het niet in config blijft hangen
    let disable_kw = format!("{name},disable");
    let kw_args = ["keyword", "monitor", &disable_kw];
    let _ = hyprctl(&kw_args[..]);

    Ok(out)
}
