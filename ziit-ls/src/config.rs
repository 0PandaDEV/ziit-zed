use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

const CONFIG_FILE_NAME: &str = "config.json";
const LEGACY_CONFIG_FILE_NAME: &str = ".ziit.json";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ZiitConfig {
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
}

fn get_config_dir() -> Result<PathBuf> {
    if let Ok(xdg_config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg_config_home.is_empty() {
            return Ok(PathBuf::from(xdg_config_home).join("ziit"));
        }
    }

    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    Ok(home_dir.join(".config").join("ziit"))
}

fn get_config_path() -> Result<PathBuf> {
    let config_dir = get_config_dir()?;
    Ok(config_dir.join(CONFIG_FILE_NAME))
}

fn get_legacy_config_path() -> Result<PathBuf> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    Ok(home_dir.join(LEGACY_CONFIG_FILE_NAME))
}

fn ensure_config_dir() -> Result<()> {
    let config_dir = get_config_dir()?;
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }
    Ok(())
}

async fn migrate_legacy_config() -> Result<()> {
    let config_path = get_config_path()?;
    let legacy_config_path = get_legacy_config_path()?;

    if config_path.exists() {
        log::debug!("New config file already exists, skipping migration");
        return Ok(());
    }

    if !legacy_config_path.exists() {
        return Ok(());
    }

    log::info!(
        "Migrating legacy config from {:?} to {:?}",
        legacy_config_path,
        config_path
    );

    match fs::read_to_string(&legacy_config_path) {
        Ok(content) => {
            let legacy_config: ZiitConfig = serde_json::from_str(&content)?;

            ensure_config_dir()?;

            let new_content = serde_json::to_string_pretty(&legacy_config)?;
            fs::write(&config_path, new_content)?;

            if let Err(e) = fs::remove_file(&legacy_config_path) {
                log::warn!("Could not remove legacy config file: {}", e);
            } else {
                log::info!("Successfully migrated config and removed legacy file");
            }
        }
        Err(e) => {
            log::warn!("Could not read legacy config file: {}", e);
        }
    }

    Ok(())
}

pub async fn read_config_file() -> Result<ZiitConfig> {
    if let Err(e) = migrate_legacy_config().await {
        log::warn!("Migration failed: {}", e);
    }

    let config_path = get_config_path()?;

    if !config_path.exists() {
        ensure_config_dir()?;
        return Ok(ZiitConfig::default());
    }

    match fs::read_to_string(config_path) {
        Ok(content) => {
            let config: ZiitConfig = serde_json::from_str(&content)?;
            Ok(config)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(ZiitConfig::default()),
        Err(e) => Err(anyhow::Error::from(e)),
    }
}

pub async fn write_config_file(config: &ZiitConfig) -> Result<()> {
    let config_path = get_config_path()?;
    ensure_config_dir()?;

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
