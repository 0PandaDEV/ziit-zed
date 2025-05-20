use std::sync::Arc;

use chrono::{DateTime, Local, TimeDelta};
use clap::{Arg, Command};
use serde_json::Value;
use tokio::io::{stdin as tokio_stdin, stdout as tokio_stdout};
use tokio::sync::{Mutex, OnceCell};
use tower_lsp::{jsonrpc, lsp_types::*, Client, LanguageServer, LspService, Server};
use url::Url;

mod api;
mod config;
mod heartbeat;

use config::ZiitConfig;
use heartbeat::HeartbeatManager;

const HEARTBEAT_DEBOUNCE_SECONDS: i64 = 120;

#[derive(Debug)]
struct LastHeartbeatInfo {
    uri: String,
    timestamp: DateTime<Local>,
    is_write: bool,
}

struct ZiitLanguageServer {
    client: Client,
    heartbeat_manager_cell: Arc<OnceCell<Arc<HeartbeatManager>>>,
    last_heartbeat_info: Mutex<Option<LastHeartbeatInfo>>,
    task_handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>
}

impl ZiitLanguageServer {
    fn new(client: Client) -> Self {
        Self {
            client,
            heartbeat_manager_cell: Arc::new(OnceCell::new()),
            last_heartbeat_info: Mutex::new(None),
            task_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn get_heartbeat_manager(&self) -> Option<Arc<HeartbeatManager>> {
        self.heartbeat_manager_cell.get().cloned()
    }

    async fn handle_activity(&self, uri_str: String, language_id: Option<String>, is_write: bool) {
        let now = Local::now();
        let mut last_hb_info_guard = self.last_heartbeat_info.lock().await;

        if let Some(ref last_info) = *last_hb_info_guard {
            if last_info.uri == uri_str
                && !is_write
                && !last_info.is_write
                && (now - last_info.timestamp) < TimeDelta::seconds(HEARTBEAT_DEBOUNCE_SECONDS)
            {
                self.client
                    .log_message(
                        MessageType::LOG,
                        format!("Ziit LS: Debounced event for {}", uri_str),
                    )
                    .await;
                return;
            }
        }

        *last_hb_info_guard = Some(LastHeartbeatInfo {
            uri: uri_str.clone(),
            timestamp: now,
            is_write,
        });
        drop(last_hb_info_guard);

        if let Some(hm) = self.get_heartbeat_manager().await {
            self.client
                .log_message(
                    MessageType::LOG,
                    format!(
                        "Ziit LS: Handling activity for {}: write={}",
                        uri_str, is_write
                    ),
                )
                .await;

            let file_path = if uri_str.starts_with("file://") {
                match Url::parse(&uri_str) {
                    Ok(parsed_url) => parsed_url
                        .to_file_path()
                        .ok()
                        .map(|p| p.to_string_lossy().into_owned()),
                    Err(_) => Some(uri_str),
                }
            } else {
                Some(uri_str)
            };

            if file_path.is_none() {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        "Ziit LS: Could not determine file path from URI for heartbeat.",
                    )
                    .await;
                return;
            }

            hm.handle_editor_activity(file_path, language_id, is_write)
                .await;
        } else {
            self.client
                .log_message(
                    MessageType::ERROR,
                    "Ziit LS: HeartbeatManager not initialized.",
                )
                .await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for ZiitLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> jsonrpc::Result<InitializeResult> {
        self.client
            .log_message(MessageType::INFO, "Ziit LS: Initializing...")
            .await;

        if let Some(init_options) = params.initialization_options {
            if let Ok(mut current_config) = config::read_config_file().await {
                self.client
                    .log_message(
                        MessageType::LOG,
                        format!("Ziit LS: Current config before init: {:?}", current_config),
                    )
                    .await;
                let mut config_changed = false;

                if let Some(api_key_val) = init_options.get("apiKey").and_then(Value::as_str) {
                    if current_config.api_key.as_deref() != Some(api_key_val) {
                        current_config.api_key = Some(api_key_val.to_string());
                        config_changed = true;
                        self.client
                            .log_message(
                                MessageType::INFO,
                                "Ziit LS: API key updated from initialization options.",
                            )
                            .await;
                    }
                }
                if let Some(base_url_val) = init_options.get("baseUrl").and_then(Value::as_str) {
                    if current_config.base_url.as_deref() != Some(base_url_val) {
                        current_config.base_url = Some(base_url_val.to_string());
                        config_changed = true;
                        self.client
                            .log_message(
                                MessageType::INFO,
                                "Ziit LS: Base URL updated from initialization options.",
                            )
                            .await;
                    }
                }

                if config_changed {
                    if let Err(e) = config::write_config_file(&current_config).await {
                        self.client
                            .log_message(
                                MessageType::ERROR,
                                format!("Ziit LS: Failed to write updated config: {}", e),
                            )
                            .await;
                    } else {
                        self.client
                            .log_message(
                                MessageType::INFO,
                                "Ziit LS: Config file updated successfully from init options.",
                            )
                            .await;
                    }
                }
            } else {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        "Ziit LS: Failed to read initial config during initialize.",
                    )
                    .await;
                let mut new_config = ZiitConfig::default();
                let mut new_config_populated = false;
                if let Some(api_key_val) = init_options.get("apiKey").and_then(Value::as_str) {
                    new_config.api_key = Some(api_key_val.to_string());
                    new_config_populated = true;
                }
                if let Some(base_url_val) = init_options.get("baseUrl").and_then(Value::as_str) {
                    new_config.base_url = Some(base_url_val.to_string());
                    new_config_populated = true;
                }
                if new_config_populated {
                    if let Err(e) = config::write_config_file(&new_config).await {
                        self.client
                            .log_message(
                                MessageType::ERROR,
                                format!(
                                    "Ziit LS: Failed to write new config from init options: {}",
                                    e
                                ),
                            )
                            .await;
                    }
                }
            }
        } else {
            self.client
                .log_message(
                    MessageType::WARNING,
                    "Ziit LS: No initialization options provided.",
                )
                .await;
        }

        match HeartbeatManager::new().await {
            Ok(hm) => {
                let hm_arc: Arc<HeartbeatManager> = Arc::new(hm);

                let hm_clone_for_tasks: Arc<HeartbeatManager> = Arc::clone(&hm_arc);
                let task_handles = hm_clone_for_tasks.start_background_tasks();
                
                let mut handles = self.task_handles.lock().await;
                handles.extend(task_handles);

                if self.heartbeat_manager_cell.set(hm_arc).is_err() {
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            "Ziit LS: HeartbeatManager already initialized.",
                        )
                        .await;
                    return Err(jsonrpc::Error::internal_error());
                }
                self.client
                    .log_message(
                        MessageType::INFO,
                        "Ziit LS: HeartbeatManager initialized successfully.",
                    )
                    .await;
            }
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Ziit LS: Failed to initialize HeartbeatManager: {}", e),
                    )
                    .await;
                return Err(jsonrpc::Error::internal_error());
            }
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "Ziit Language Server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "Ziit LS: Server initialized notification received.",
            )
            .await;
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        let mut handles = self.task_handles.lock().await;
        for handle in handles.drain(..) {
            handle.abort();
        }
        drop(handles);

        if let Some(hm) = self.get_heartbeat_manager().await {
            if let Err(e) = hm.save_offline_heartbeats().await {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("Failed to save offline heartbeats during shutdown: {}", e),
                    )
                    .await;
            }
        }

        self.client
            .log_message(MessageType::INFO, "Ziit LS: Shutdown requested.")
            .await;
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client
            .log_message(
                MessageType::LOG,
                format!("Ziit LS: did_open: {}", params.text_document.uri),
            )
            .await;
        self.handle_activity(
            params.text_document.uri.to_string(),
            Some(params.text_document.language_id),
            false,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.client
            .log_message(
                MessageType::LOG,
                format!("Ziit LS: did_change: {}", params.text_document.uri),
            )
            .await;
        self.handle_activity(params.text_document.uri.to_string(), None, false)
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.client
            .log_message(
                MessageType::LOG,
                format!("Ziit LS: did_save: {}", params.text_document.uri),
            )
            .await;
        self.handle_activity(params.text_document.uri.to_string(), None, true)
            .await;
    }
}

#[tokio::main]
async fn main() {
    let matches = Command::new("ziit-ls")
        .version(env!("CARGO_PKG_VERSION"))
        .author("PandaDEV <contact@pandadev.net>")
        .about("Ziit language server for Zed")
        .arg(
            Arg::new("standalone")
                .long("standalone")
                .help("Run in standalone mode")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    if matches.get_flag("standalone") {
        eprintln!("Ziit Language Server starting in standalone mode...");
    } else {
        eprintln!("Ziit Language Server starting...");
    }

    let stdin = tokio_stdin();
    let stdout = tokio_stdout();

    let (service, socket) = LspService::build(ZiitLanguageServer::new).finish();

    Server::new(stdin, stdout, socket).serve(service).await;
} 