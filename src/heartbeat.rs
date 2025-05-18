use crate::api::{
    fetch_daily_summary_request, send_batch_heartbeats_request, send_heartbeat_request,
};
use crate::config::{get_api_key, get_base_url};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

const HEARTBEAT_INTERVAL_SECONDS: u64 = 120;
const OFFLINE_SYNC_INTERVAL_SECONDS: u64 = 30;
const DAILY_SUMMARY_INTERVAL_SECONDS: u64 = 15 * 60;
const OFFLINE_QUEUE_FILE_NAME: &str = "offline_heartbeats.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Heartbeat {
    timestamp: String,
    project: Option<String>,
    language: Option<String>,
    file: Option<String>,
    branch: Option<String>,
    editor: String,
    os: String,
}

impl Heartbeat {
    fn new(
        project: Option<String>,
        language: Option<String>,
        file: Option<String>,
        branch: Option<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            project,
            language,
            file,
            branch,
            editor: "Zed".to_string(),
            os: std::env::consts::OS.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct HeartbeatManager {
    last_heartbeat_time: Arc<Mutex<Option<DateTime<Utc>>>>,
    last_file: Arc<Mutex<Option<String>>>,
    offline_heartbeats: Arc<Mutex<VecDeque<Heartbeat>>>,
    offline_queue_path: PathBuf,
    is_online: Arc<Mutex<bool>>,
    has_valid_api_key: Arc<Mutex<bool>>,
}

fn get_zed_data_dir() -> Result<PathBuf> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let ziit_dir = home_dir.join(".ziit");
    Ok(ziit_dir)
}

impl HeartbeatManager {
    pub async fn new() -> Result<Self> {
        let data_dir = get_zed_data_dir()?;
        if !data_dir.exists() {
            fs::create_dir_all(&data_dir)?;
        }
        let offline_queue_path = data_dir.join(OFFLINE_QUEUE_FILE_NAME);

        let manager = Self {
            last_heartbeat_time: Arc::new(Mutex::new(None)),
            last_file: Arc::new(Mutex::new(None)),
            offline_heartbeats: Arc::new(Mutex::new(VecDeque::new())),
            offline_queue_path,
            is_online: Arc::new(Mutex::new(true)),
            has_valid_api_key: Arc::new(Mutex::new(true)),
        };

        manager.load_offline_heartbeats().await?;
        log::info!("HeartbeatManager initialized. Call start_background_tasks() explicitly.");
        Ok(manager)
    }

    pub fn start_background_tasks(self: &Arc<Self>) {
        let s = self.clone();
        tokio::spawn(async move {
            let mut timer = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECONDS));
            loop {
                timer.tick().await;
                s.handle_editor_activity(None, None, false).await;
            }
        });

        let s_sync = self.clone();
        tokio::spawn(async move {
            let mut timer = interval(Duration::from_secs(OFFLINE_SYNC_INTERVAL_SECONDS));
            loop {
                timer.tick().await;
                if let Err(e) = s_sync.sync_offline_heartbeats().await {
                    log::error!("Error syncing offline heartbeats: {}", e);
                }
            }
        });

        let s_summary = self.clone();
        tokio::spawn(async move {
            let mut timer = interval(Duration::from_secs(DAILY_SUMMARY_INTERVAL_SECONDS));
            loop {
                timer.tick().await;
                if let Err(e) = s_summary.fetch_daily_summary().await {
                    log::error!("Error fetching daily summary: {}", e);
                }
            }
        });
        log::info!("HeartbeatManager background tasks started.");
    }

    async fn load_offline_heartbeats(&self) -> Result<()> {
        if self.offline_queue_path.exists() {
            match fs::read_to_string(&self.offline_queue_path) {
                Ok(data) => match serde_json::from_str::<VecDeque<Heartbeat>>(&data) {
                    Ok(heartbeats) => {
                        let mut queue = self.offline_heartbeats.lock().await;
                        *queue = heartbeats;
                        log::info!("Loaded {} offline heartbeats.", queue.len());
                    }
                    Err(e) => {
                        log::error!(
                            "Error parsing offline heartbeats file: {}. Creating new queue.",
                            e
                        );
                        let _ = fs::remove_file(&self.offline_queue_path);
                    }
                },
                Err(e) => {
                    log::error!("Error reading offline heartbeats file: {}", e);
                }
            }
        }
        Ok(())
    }

    async fn save_offline_heartbeats(&self) -> Result<()> {
        let queue = self.offline_heartbeats.lock().await;
        let data = serde_json::to_string_pretty(&*queue)?;
        if let Some(parent_dir) = self.offline_queue_path.parent() {
            if !parent_dir.exists() {
                fs::create_dir_all(parent_dir)?;
            }
        }
        fs::write(&self.offline_queue_path, data)?;
        Ok(())
    }

    async fn set_online_status(&self, online: bool) {
        let mut is_online = self.is_online.lock().await;
        if *is_online != online {
            *is_online = online;
            log::info!(
                "Online status changed to: {}",
                if online { "online" } else { "offline" }
            );
        }
    }

    async fn set_api_key_status(&self, valid: bool) {
        let mut has_valid_key = self.has_valid_api_key.lock().await;
        if *has_valid_key != valid {
            *has_valid_key = valid;
            log::info!(
                "API key status changed to: {}",
                if valid { "valid" } else { "invalid" }
            );
        }
    }

    pub async fn handle_editor_activity(
        &self,
        file_path: Option<String>,
        language_id: Option<String>,
        force_send: bool,
    ) {
        let project_name = None;
        let branch_name = None;

        let mut last_hb_time = self.last_heartbeat_time.lock().await;
        let mut last_f = self.last_file.lock().await;

        let now = Utc::now();
        let current_file_path_str = file_path.clone();

        let file_changed = match (&*last_f, &current_file_path_str) {
            (Some(ref old), Some(ref new)) => old != new,
            (None, Some(_)) => true,
            _ => false,
        };

        let time_threshold_passed = match *last_hb_time {
            Some(last_time) => (now - last_time).num_seconds() >= HEARTBEAT_INTERVAL_SECONDS as i64,
            None => true,
        };

        if force_send || file_changed || time_threshold_passed {
            log::info!("Sufficient activity, attempting to send heartbeat.");
            let heartbeat = Heartbeat::new(project_name, language_id, file_path, branch_name);
            if let Err(e) = self.process_heartbeat(heartbeat).await {
                log::error!("Error processing heartbeat: {}", e);
            }
            *last_hb_time = Some(now);
            *last_f = current_file_path_str;
        } else {
            log::debug!("Skipping heartbeat: not enough activity or time passed.");
        }
    }

    async fn process_heartbeat(&self, heartbeat: Heartbeat) -> Result<()> {
        let api_key_opt = get_api_key().await?;
        let base_url = get_base_url().await?;

        if api_key_opt.is_none() || base_url.is_empty() {
            log::warn!("API key or base URL not set. Queuing heartbeat.");
            self.queue_offline_heartbeat(heartbeat).await?;
            self.set_api_key_status(false).await;
            return Ok(());
        }

        let key = api_key_opt.unwrap();

        if !*self.is_online.lock().await {
            log::info!("Currently offline. Queuing heartbeat.");
            self.queue_offline_heartbeat(heartbeat).await?;
            return Ok(());
        }

        match send_heartbeat_request(&base_url, &key, heartbeat.clone()).await {
            Ok(_) => {
                log::info!("Heartbeat sent successfully.");
                self.set_online_status(true).await;
                self.set_api_key_status(true).await;
            }
            Err(e) => {
                log::error!("Failed to send heartbeat: {}. Queuing offline.", e);
                self.set_online_status(false).await;
                if e.to_string().contains("401")
                    || e.to_string().to_lowercase().contains("invalid api key")
                {
                    self.set_api_key_status(false).await;
                }
                self.queue_offline_heartbeat(heartbeat).await?;
            }
        }
        Ok(())
    }

    async fn queue_offline_heartbeat(&self, heartbeat: Heartbeat) -> Result<()> {
        let mut queue = self.offline_heartbeats.lock().await;
        queue.push_back(heartbeat);
        log::debug!("Heartbeat added to offline queue. Size: {}", queue.len());
        let _ = self.save_offline_heartbeats().await;
        Ok(())
    }

    pub async fn sync_offline_heartbeats(&self) -> Result<()> {
        let is_online = *self.is_online.lock().await;
        let mut queue = self.offline_heartbeats.lock().await;

        if !is_online || queue.is_empty() {
            return Ok(());
        }

        let api_key_opt = get_api_key().await?;
        let base_url = get_base_url().await?;

        if api_key_opt.is_none() || base_url.is_empty() {
            log::warn!("Cannot sync offline heartbeats: API key or base URL not set.");
            self.set_api_key_status(false).await;
            return Ok(());
        }
        let key = api_key_opt.unwrap();

        let batch: Vec<Heartbeat> = queue.drain(..).collect();
        if batch.is_empty() {
            return Ok(());
        }
        log::info!("Attempting to sync {} offline heartbeats.", batch.len());

        match send_batch_heartbeats_request(&base_url, &key, batch.clone()).await {
            Ok(_) => {
                log::info!("Successfully synced {} offline heartbeats.", batch.len());
                self.set_online_status(true).await;
                self.set_api_key_status(true).await;
                self.save_offline_heartbeats().await?;
                self.fetch_daily_summary().await?;
            }
            Err(e) => {
                log::error!("Error syncing offline heartbeats: {}. Re-queuing.", e);
                let mut queue_for_readd = self.offline_heartbeats.lock().await;
                for hb in batch.into_iter().rev() {
                    queue_for_readd.push_front(hb);
                }
                drop(queue_for_readd);
                self.set_online_status(false).await;
                if e.to_string().contains("401")
                    || e.to_string().to_lowercase().contains("invalid api key")
                {
                    self.set_api_key_status(false).await;
                }
                self.save_offline_heartbeats().await?;
            }
        }
        Ok(())
    }

    pub async fn fetch_daily_summary(&self) -> Result<()> {
        let api_key_opt = get_api_key().await?;
        let base_url = get_base_url().await?;

        if api_key_opt.is_none() || base_url.is_empty() {
            log::warn!("Cannot fetch daily summary: API key or base URL not set.");
            self.set_api_key_status(false).await;
            return Ok(());
        }
        let api_key = api_key_opt.unwrap();

        match fetch_daily_summary_request(&base_url, &api_key).await {
            Ok(summary_response) => {
                self.set_online_status(true).await;
                self.set_api_key_status(true).await;
                if let Some(today_summary) = summary_response.summaries.first() {
                    log::info!(
                        "Today's total coding time: {} seconds",
                        today_summary.total_seconds
                    );
                } else {
                    log::info!("No summary data for today.");
                }
            }
            Err(e) => {
                log::error!("Error fetching daily summary: {}", e);
                if e.to_string().contains("401")
                    || e.to_string().to_lowercase().contains("invalid api key")
                {
                    self.set_api_key_status(false).await;
                } else {
                    self.set_online_status(false).await;
                }
            }
        }
        Ok(())
    }
}