use crate::heartbeat::Heartbeat;
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct DailySummaryResponse {
    pub summaries: Vec<SummaryEntry>,
    pub timezone: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SummaryEntry {
    pub date: String,
    #[serde(rename = "totalSeconds")]
    pub total_seconds: u64,
    #[serde(rename = "hourlyData")]
    pub hourly_data: Option<Vec<HourlyData>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HourlyData {
    pub seconds: u64,
}

pub async fn send_heartbeat_request(
    base_url: &str,
    api_key: &str,
    heartbeat: Heartbeat,
) -> Result<()> {
    let url = format!("{}/api/external/heartbeats", base_url);
    let client = reqwest::Client::new();

    log::debug!("Sending heartbeat to: {}", url);
    log::debug!("Heartbeat payload: {:?}", heartbeat);

    let json_body = serde_json::to_string_pretty(&heartbeat)?;
    log::info!("Heartbeat JSON being sent:\n{}", json_body);
    log::info!(
        "Authorization header: Bearer {}...",
        &api_key[..8.min(api_key.len())]
    );
    log::info!("Full request URL: {}", url);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&heartbeat)
        .send()
        .await?;

    log::info!("Response status: {}", response.status());

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        log::error!("Heartbeat failed with status {}: {}", status, error_body);
        log::error!("Failed request was: POST {} with body:\n{}", url, json_body);
        return Err(anyhow!("Failed to send heartbeat: HTTP {}", status));
    }

    log::info!("Heartbeat sent successfully!");
    Ok(())
}

pub async fn send_batch_heartbeats_request(
    base_url: &str,
    api_key: &str,
    heartbeats: Vec<Heartbeat>,
) -> Result<()> {
    let url = format!("{}/api/external/batch", base_url);
    let client = reqwest::Client::new();

    log::debug!(
        "Sending {} heartbeats in batch to: {}",
        heartbeats.len(),
        url
    );

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&heartbeats)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        log::error!(
            "Batch heartbeat failed with status {}: {}",
            status,
            error_body
        );
        return Err(anyhow!("Failed to send batch heartbeats: HTTP {}", status));
    }

    log::debug!("Batch heartbeats sent successfully");
    Ok(())
}

pub async fn fetch_daily_summary_request(
    base_url: &str,
    api_key: &str,
) -> Result<DailySummaryResponse> {
    let local_now = Local::now();
    let midnight_offset_seconds = local_now.offset().local_minus_utc();

    let url = format!(
        "{}/api/external/stats?timeRange=today&midnightOffsetSeconds={}&t={}",
        base_url,
        midnight_offset_seconds,
        Utc::now().timestamp_millis()
    );

    let client = reqwest::Client::new();

    log::debug!("Fetching daily summary from: {}", url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        log::error!(
            "Daily summary fetch failed with status {}: {}",
            status,
            error_body
        );
        return Err(anyhow!("Failed to fetch daily summary: HTTP {}", status));
    }

    let summary = response.json::<DailySummaryResponse>().await?;
    log::debug!("Daily summary fetched successfully");

    Ok(summary)
}
