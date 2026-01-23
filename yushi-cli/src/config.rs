use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub default_connections: usize,
    pub default_max_tasks: usize,
    pub default_output_dir: PathBuf,
    pub user_agent: Option<String>,
    pub proxy: Option<String>,
    pub speed_limit: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_connections: 4,
            default_max_tasks: 2,
            default_output_dir: PathBuf::from("downloads"),
            user_agent: Some("YuShi/1.0".to_string()),
            proxy: None,
            speed_limit: None,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("无法获取配置目录"))?;
        Ok(config_dir.join("yushi").join("config.json"))
    }

    pub fn queue_state_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("无法获取配置目录"))?;
        Ok(config_dir.join("yushi").join("queue.json"))
    }
}
