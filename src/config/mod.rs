use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub capture_enabled: bool,
    pub flush_interval_seconds: u64,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub dark_mode: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            capture_enabled: true,
            flush_interval_seconds: 5,
            start_with_windows: false,
            dark_mode: false,
        }
    }
}
pub fn app_dir() -> Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .context("executable has no parent")?
        .to_path_buf())
}
pub fn load_or_create() -> Result<Config> {
    let path = app_dir()?.join("config.toml");
    if path.exists() {
        return Ok(toml::from_str(
            &std::fs::read_to_string(&path)
                .with_context(|| format!("无法读取配置文件 {}", path.display()))?,
        )?);
    }
    let cfg = Config::default();
    std::fs::write(&path, toml::to_string_pretty(&cfg)?).with_context(|| {
        format!(
            "当前目录没有写入权限，请将 kb_count 移动到具有读写权限的文件夹。路径：{}",
            path.display()
        )
    })?;
    Ok(cfg)
}

pub fn save(config: &Config) -> Result<()> {
    let path = app_dir()?.join("config.toml");
    std::fs::write(&path, toml::to_string_pretty(config)?).with_context(|| {
        format!(
            "当前目录没有写入权限，请将 kb_count 移动到具有读写权限的文件夹。路径：{}",
            path.display()
        )
    })?;
    Ok(())
}
pub mod startup;

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn legacy_config_defaults_new_options() {
        let config: Config = toml::from_str("capture_enabled = true\nflush_interval_seconds = 5\n")
            .expect("legacy config should remain readable");

        assert!(config.capture_enabled);
        assert_eq!(config.flush_interval_seconds, 5);
        assert!(!config.start_with_windows);
        assert!(!config.dark_mode);
    }
}
