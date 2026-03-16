use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use directories::ProjectDirs;
use std::fs;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AbaConfig {
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub use_openai_oauth: Option<bool>,
    pub default_model: Option<String>,
}

impl AbaConfig {
    pub fn get_config_path() -> PathBuf {
        if let Some(proj_dirs) = ProjectDirs::from("", "", "ABA") {
            proj_dirs.config_dir().join("config.toml")
        } else {
            PathBuf::from(".aba_config.toml")
        }
    }

    pub fn load() -> Self {
        let path = Self::get_config_path();
        if path.exists()
            && let Ok(content) = fs::read_to_string(&path)
            && let Ok(config) = toml::from_str(&content)
        {
            return config;
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        let path = Self::get_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}
