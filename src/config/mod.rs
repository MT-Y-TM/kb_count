use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseAction {
    #[default]
    Background,
    Exit,
}
impl CloseAction {
    pub fn index(self) -> i32 {
        match self {
            Self::Background => 0,
            Self::Exit => 1,
        }
    }
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Background),
            1 => Some(Self::Exit),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub capture_enabled: bool,
    pub flush_interval_seconds: u64,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default)]
    pub remember_close_action: bool,
    #[serde(default)]
    pub close_action: CloseAction,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            capture_enabled: true,
            flush_interval_seconds: 5,
            start_with_windows: false,
            dark_mode: false,
            remember_close_action: false,
            close_action: CloseAction::Background,
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
    use super::{CloseAction, Config};

    #[test]
    fn legacy_config_defaults_new_options() {
        let config: Config = toml::from_str("capture_enabled = true\nflush_interval_seconds = 5\n")
            .expect("legacy config should remain readable");

        assert!(config.capture_enabled);
        assert_eq!(config.flush_interval_seconds, 5);
        assert!(!config.start_with_windows);
        assert!(!config.dark_mode);
        assert!(!config.remember_close_action);
        assert_eq!(config.close_action, CloseAction::Background);
    }
    #[test]
    fn close_preferences_round_trip_with_and_without_remembering() {
        for remember in [false, true] {
            for action in [CloseAction::Background, CloseAction::Exit] {
                let config = Config {
                    remember_close_action: remember,
                    close_action: action,
                    ..Config::default()
                };
                let saved = toml::to_string_pretty(&config).unwrap();
                let loaded: Config = toml::from_str(&saved).unwrap();
                assert_eq!(loaded.remember_close_action, remember);
                assert_eq!(loaded.close_action, action);
                assert_eq!(CloseAction::from_index(action.index()), Some(action));
            }
        }
        assert_eq!(CloseAction::from_index(-1), None);
    }
}
