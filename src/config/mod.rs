use std::path::PathBuf;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub capture_enabled: bool,
    pub flush_interval_seconds: u64,
}
impl Default for Config { fn default() -> Self { Self { capture_enabled: true, flush_interval_seconds: 5 } } }
pub fn app_dir() -> Result<PathBuf> { Ok(std::env::current_exe()?.parent().context("executable has no parent")?.to_path_buf()) }
pub fn load_or_create() -> Result<Config> {
    let path = app_dir()?.join("config.toml");
    if path.exists() { return Ok(toml::from_str(&std::fs::read_to_string(path)?)?); }
    let cfg = Config::default();
    std::fs::write(path, toml::to_string_pretty(&cfg)?)?;
    Ok(cfg)
}
