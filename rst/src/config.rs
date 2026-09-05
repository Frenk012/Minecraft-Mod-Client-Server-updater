use anyhow::{Context, Result};
use std::fs;
use crate::models::AppConfig;

/// Directory of the executable — config.toml and state.json live next to the exe,
/// so the app works regardless of the working directory it was launched from.
pub fn app_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

pub fn config_path() -> String {
    app_dir().join("config.toml").to_string_lossy().to_string()
}

pub fn load_config(path: &str) -> Result<AppConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Cannot read config file '{path}'"))?;
    let config: AppConfig = toml::from_str(&content)
        .context("Invalid config.toml format")?;
    Ok(config)
}

pub fn save_config(path: &str, config: &AppConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).context("Cannot serialize config")?;
    fs::write(path, content).with_context(|| format!("Cannot write '{path}'"))?;
    Ok(())
}
