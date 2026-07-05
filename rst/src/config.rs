use anyhow::{Context, Result};
use std::fs;
use crate::models::AppConfig;

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
