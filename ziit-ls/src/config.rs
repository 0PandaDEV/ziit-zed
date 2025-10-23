use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

const CONFIG_FILE_NAME: &str = "config.json";
const LEGACY_CONFIG_FILE_NAMES: &[&str] = &[".ziit.json", ".ziit.cfg"];

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

fn get_legacy_config_paths() -> Result<Vec<PathBuf>> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    Ok(LEGACY_CONFIG_FILE_NAMES
        .iter()
        .map(|name| home_dir.join(name))
        .collect())
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

    if config_path.exists() {
        log::debug!("New config file already exists, skipping migration");
        return Ok(());
    }

    let legacy_config_paths = get_legacy_config_paths()?;
    for legacy_config_path in legacy_config_paths {
        if !legacy_config_path.exists() {
            continue;
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
                return Ok(());
            }
            Err(e) => {
                log::warn!(
                    "Could not read legacy config file {:?}: {}",
                    legacy_config_path,
                    e
                );
            }
        }
    }

    Ok(())
}

pub async fn read_config_file() -> Result<ZiitConfig> {
    if let Err(e) = migrate_legacy_config().await {
        log::warn!("Migration failed: {}", e);
    }

    let config_path = get_config_path()?;
    log::info!("Reading config from: {:?}", config_path);

    if !config_path.exists() {
        log::warn!("Config file does not exist at: {:?}", config_path);
        ensure_config_dir()?;
        return Ok(ZiitConfig::default());
    }

    match fs::read_to_string(&config_path) {
        Ok(content) => {
            log::info!(
                "Successfully read config file, content length: {}",
                content.len()
            );
            log::debug!("Config file content: {}", content);
            match serde_json::from_str::<ZiitConfig>(&content) {
                Ok(config) => {
                    log::info!(
                        "Successfully parsed config. Has API key: {}",
                        config.api_key.is_some()
                    );
                    log::info!("Base URL: {:?}", config.base_url);
                    Ok(config)
                }
                Err(e) => {
                    log::error!("Failed to parse config JSON: {}", e);
                    Err(anyhow::Error::from(e))
                }
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            log::warn!("Config file not found (NotFound error)");
            Ok(ZiitConfig::default())
        }
        Err(e) => {
            log::error!("Error reading config file: {}", e);
            Err(anyhow::Error::from(e))
        }
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
    log::debug!("get_api_key() called");
    let config = read_config_file().await?;
    log::debug!(
        "get_api_key() returning: {}",
        if config.api_key.is_some() {
            "Some(***)"
        } else {
            "None"
        }
    );
    Ok(config.api_key)
}

pub async fn get_base_url() -> Result<String> {
    log::debug!("get_base_url() called");
    let config = read_config_file().await?;
    let url = config
        .base_url
        .unwrap_or_else(|| "https://ziit.app".to_string());
    log::debug!("get_base_url() returning: {}", url);
    Ok(url)
}
