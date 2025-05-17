use crate::heartbeat::Heartbeat;
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use zed_extension_api::http_client::{self, HttpMethod, HttpRequest, HttpResponse};

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

fn make_request_inner<T: for<'de> Deserialize<'de> + Send + 'static>(
    base_url: String,
    path: String,
    method: HttpMethod,
    api_key: String,
    body_json_string: Option<String>,
    query_params: Option<Vec<(String, String)>>,
) -> Result<T> {
    let mut full_url = format!("{}{}", base_url, path);
    if let Some(params) = query_params {
        let query_string = params
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<String>>()
            .join("&");
        if !query_string.is_empty() {
            full_url.push('?');
            full_url.push_str(&query_string);
        }
    }

    let mut builder = http_client::HttpRequestBuilder::new();
    builder = builder.method(method);
    builder = builder.url(&full_url);
    builder = builder.header("Authorization", &format!("Bearer {}", api_key));

    if let Some(json_string) = body_json_string {
        builder = builder.header("Content-Type", "application/json");
        builder = builder.body(json_string.into_bytes());
    }

    let request_build_result = builder.build();
    let request: HttpRequest = match request_build_result {
        Ok(req) => req,
        Err(e_str) => return Err(anyhow::anyhow!("Failed to build HTTP request: {}", e_str)),
    };

    log::debug!("Sending HTTP {:?} request to {}", request.method, full_url);

    let fetch_result = http_client::fetch(&request);
    let response: HttpResponse = match fetch_result {
        Ok(res) => res,
        Err(e_str) => return Err(anyhow::anyhow!("HTTP fetch failed: {}", e_str)),
    };

    log::warn!("HTTP status code check is bypassed in make_request_inner due to API limitations!");
    let response_body_bytes = response.body;
    let response_body_str = String::from_utf8(response_body_bytes.clone())?;
    log::debug!("Response body snippet: {:.100}", response_body_str);
    match serde_json::from_str::<T>(&response_body_str) {
        Ok(parsed) => Ok(parsed),
        Err(e) => {
            log::error!("JSON parsing error: {} for body: {}", e, response_body_str);
            Err(anyhow!(
                "Failed to parse JSON response (status check bypassed): {}",
                e
            ))
        }
    }
}

pub async fn send_heartbeat_request(
    base_url: &str,
    api_key: &str,
    heartbeat: Heartbeat,
) -> Result<()> {
    let body_str = serde_json::to_string(&heartbeat)?;
    let base_url_owned = base_url.to_string();
    let api_key_owned = api_key.to_string();

    tokio::task::spawn_blocking(move || {
        make_request_inner::<serde_json::Value>(
            base_url_owned,
            "/api/external/heartbeats".to_string(),
            HttpMethod::Post,
            api_key_owned,
            Some(body_str),
            None,
        )
    })
    .await??;
    Ok(())
}

pub async fn send_batch_heartbeats_request(
    base_url: &str,
    api_key: &str,
    heartbeats: Vec<Heartbeat>,
) -> Result<()> {
    let body_str = serde_json::to_string(&heartbeats)?;
    let base_url_owned = base_url.to_string();
    let api_key_owned = api_key.to_string();

    tokio::task::spawn_blocking(move || {
        make_request_inner::<serde_json::Value>(
            base_url_owned,
            "/api/external/batch".to_string(),
            HttpMethod::Post,
            api_key_owned,
            Some(body_str),
            None,
        )
    })
    .await??;
    Ok(())
}

pub async fn fetch_daily_summary_request(
    base_url: &str,
    api_key: &str,
) -> Result<DailySummaryResponse> {
    let local_now = Local::now();
    let midnight_offset_seconds = local_now.offset().local_minus_utc();
    let base_url_owned = base_url.to_string();
    let api_key_owned = api_key.to_string();

    let params = vec![
        ("timeRange".to_string(), "today".to_string()),
        (
            "midnightOffsetSeconds".to_string(),
            midnight_offset_seconds.to_string(),
        ),
        ("t".to_string(), Utc::now().timestamp_millis().to_string()),
    ];

    tokio::task::spawn_blocking(move || {
        make_request_inner::<DailySummaryResponse>(
            base_url_owned,
            "/api/external/stats".to_string(),
            HttpMethod::Get,
            api_key_owned,
            None,
            Some(params),
        )
    })
    .await?
}
