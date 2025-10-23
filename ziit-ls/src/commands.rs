use crate::config::{read_config_file, write_config_file};
use anyhow::Result;

pub async fn set_api_key(api_key: String) -> Result<String> {
    let mut config = read_config_file().await?;
    config.api_key = Some(api_key);
    write_config_file(&config).await?;
    Ok("API key updated successfully".to_string())
}

pub async fn set_base_url(base_url: String) -> Result<String> {
    let mut config = read_config_file().await?;
    config.base_url = Some(base_url);
    write_config_file(&config).await?;
    Ok("Base URL updated successfully".to_string())
}

pub async fn get_dashboard_url() -> Result<String> {
    let config = read_config_file().await?;
    let base_url = config
        .base_url
        .unwrap_or_else(|| "https://ziit.app".to_string());
    let base_url = base_url.trim_end_matches('/');

    Ok(format!("{}/dashboard", base_url))
}

pub async fn get_config_status() -> Result<ConfigStatus> {
    let config = read_config_file().await?;

    Ok(ConfigStatus {
        has_api_key: config.api_key.is_some(),
        base_url: config
            .base_url
            .unwrap_or_else(|| "https://ziit.app".to_string()),
        config_path: get_config_path_string()?,
    })
}

#[derive(Debug)]
pub struct ConfigStatus {
    pub has_api_key: bool,
    pub base_url: String,
    pub config_path: String,
}

fn get_config_path_string() -> Result<String> {
    let config_dir = if let Ok(xdg_config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg_config_home.is_empty() {
            std::path::PathBuf::from(xdg_config_home).join("ziit")
        } else {
            let home_dir =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
            home_dir.join(".config").join("ziit")
        }
    } else {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        home_dir.join(".config").join("ziit")
    };

    Ok(config_dir.join("config.json").to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_dashboard_url() {
        let url = get_dashboard_url().await.unwrap();
        assert!(url.contains("/dashboard"));
    }
}
