use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use std::io::ErrorKind;

const CONFIG_FILE_NAME: &str = ".ziit.json";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ZiitConfig {
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
}

fn get_config_path() -> Result<PathBuf> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    Ok(home_dir.join(CONFIG_FILE_NAME))
}

pub async fn read_config_file() -> Result<ZiitConfig> {
    let config_path = get_config_path()?;
    if !config_path.exists() {
        if let Some(parent_dir) = config_path.parent() {
            if !parent_dir.exists() {
                fs::create_dir_all(parent_dir)?;
            }
        }
        return Ok(ZiitConfig::default());
    }

    match fs::read_to_string(config_path) {
        Ok(content) => {
            let config: ZiitConfig = serde_json::from_str(&content)?;
            Ok(config)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            Ok(ZiitConfig::default())
        }
        Err(e) => Err(anyhow::Error::from(e)),
    }
}

pub async fn write_config_file(config: &ZiitConfig) -> Result<()> {
    let config_path = get_config_path()?;
    if let Some(parent_dir) = config_path.parent() {
        if !parent_dir.exists() {
            fs::create_dir_all(parent_dir)?;
        }
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(config_path, content)?;
    log::info!("Config file updated: {}", CONFIG_FILE_NAME);
    Ok(())
}

pub async fn get_api_key() -> Result<Option<String>> {
    let config = read_config_file().await?;
    Ok(config.api_key)
}

pub async fn get_base_url() -> Result<String> {
    let config = read_config_file().await?;
    Ok(config
        .base_url
        .unwrap_or_else(|| "https://ziit.app".to_string()))
}
