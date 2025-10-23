use std::sync::Arc;

use chrono::{DateTime, Local, TimeDelta};
use clap::{Arg, Command};
use serde_json::Value;
use tokio::io::{stdin as tokio_stdin, stdout as tokio_stdout};
use tokio::sync::{Mutex, OnceCell};
use tower_lsp::{jsonrpc, lsp_types::*, Client, LanguageServer, LspService, Server};
use url::Url;

mod api;
mod commands;
mod config;
mod heartbeat;
mod language;
mod project;

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
    task_handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    focused_file: Arc<Mutex<Option<String>>>,
    opened_files: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl ZiitLanguageServer {
    fn new(client: Client) -> Self {
        Self {
            client,
            heartbeat_manager_cell: Arc::new(OnceCell::new()),
            last_heartbeat_info: Mutex::new(None),
            task_handles: Arc::new(Mutex::new(Vec::new())),
            focused_file: Arc::new(Mutex::new(None)),
            opened_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    async fn get_heartbeat_manager(&self) -> Option<Arc<HeartbeatManager>> {
        self.heartbeat_manager_cell.get().cloned()
    }

    async fn handle_activity(&self, uri_str: String, language_id: Option<String>, is_write: bool) {
        let now = Local::now();
        let mut last_hb_info_guard = self.last_heartbeat_info.lock().await;
        if !is_write {
            if let Some(ref last_info) = *last_hb_info_guard {
                if last_info.uri == uri_str
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
                        "Ziit LS: Handling activity for {}: write={}, force_send={}",
                        uri_str, is_write, is_write
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
        log::info!("=== Ziit LS: initialize() called ===");
        self.client
            .log_message(MessageType::INFO, "Ziit LS: Initializing...")
            .await;

        log::info!(
            "Initialization params: workspace folders: {:?}",
            params.workspace_folders
        );
        log::info!("Initialization params: root_uri: {:?}", params.root_uri);

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
                log::info!("=== HeartbeatManager initialized and background tasks started ===");
            }
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Ziit LS: Failed to initialize HeartbeatManager: {}", e),
                    )
                    .await;
                log::error!("Failed to initialize HeartbeatManager: {}", e);
                return Err(jsonrpc::Error::internal_error());
            }
        }

        log::info!("=== Returning InitializeResult ===");
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "Ziit Language Server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "ziit.setApiKey".to_string(),
                        "ziit.setBaseUrl".to_string(),
                        "ziit.openDashboard".to_string(),
                        "ziit.showStatus".to_string(),
                    ],
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        log::info!("=== Ziit LS: initialized() notification received ===");
        self.client
            .log_message(
                MessageType::INFO,
                "Ziit LS: Server initialized notification received.",
            )
            .await;
        log::info!("Language server is now fully initialized and ready to receive events");
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
        log::info!("=== did_open called for: {} ===", params.text_document.uri);
        log::info!("Language ID: {}", params.text_document.language_id);
        self.client
            .log_message(
                MessageType::LOG,
                format!("Ziit LS: did_open: {}", params.text_document.uri),
            )
            .await;

        // Track opened files - we'll send heartbeat when user first interacts with them
        let uri_string = params.text_document.uri.to_string();
        let mut opened = self.opened_files.lock().await;
        opened.insert(uri_string.clone());
        drop(opened);

        log::debug!("File opened and tracked: {}", uri_string);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        log::debug!(
            "=== did_change called for: {} ===",
            params.text_document.uri
        );
        self.client
            .log_message(
                MessageType::LOG,
                format!("Ziit LS: did_change: {}", params.text_document.uri),
            )
            .await;

        // did_change only fires for the focused/active file
        let uri_string = params.text_document.uri.to_string();

        // Check if this is a newly focused file
        let mut opened = self.opened_files.lock().await;
        let was_just_opened = opened.remove(&uri_string);
        drop(opened);

        // Update focused file tracker
        let mut focused = self.focused_file.lock().await;
        let focus_changed = focused.as_ref() != Some(&uri_string);
        *focused = Some(uri_string.clone());
        drop(focused);

        if was_just_opened || focus_changed {
            log::info!("File became focused (first edit): {}", uri_string);
        } else {
            log::debug!("Continuing work on focused file: {}", uri_string);
        }

        self.handle_activity(uri_string, None, false).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        log::info!("=== did_save called for: {} ===", params.text_document.uri);
        self.client
            .log_message(
                MessageType::LOG,
                format!("Ziit LS: did_save: {}", params.text_document.uri),
            )
            .await;

        // Save event confirms this file is focused
        let uri_string = params.text_document.uri.to_string();

        // Remove from opened files if it was just opened
        let mut opened = self.opened_files.lock().await;
        opened.remove(&uri_string);
        drop(opened);

        // Update focused file tracker
        let mut focused = self.focused_file.lock().await;
        *focused = Some(uri_string.clone());
        drop(focused);

        log::info!("File saved (focused): {}", uri_string);
        self.handle_activity(uri_string, None, true).await;
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> jsonrpc::Result<Option<Value>> {
        self.client
            .log_message(
                MessageType::LOG,
                format!("Ziit LS: execute_command: {}", params.command),
            )
            .await;

        match params.command.as_str() {
            "ziit.setApiKey" => {
                if let Some(Value::String(api_key)) = params.arguments.get(0) {
                    match commands::set_api_key(api_key.clone()).await {
                        Ok(msg) => {
                            self.client
                                .log_message(MessageType::INFO, format!("Ziit LS: {}", msg))
                                .await;
                            Ok(Some(Value::String(msg)))
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to set API key: {}", e);
                            self.client
                                .log_message(MessageType::ERROR, format!("Ziit LS: {}", error_msg))
                                .await;
                            Err(jsonrpc::Error::internal_error())
                        }
                    }
                } else {
                    Err(jsonrpc::Error::invalid_params("API key parameter required"))
                }
            }
            "ziit.setBaseUrl" => {
                if let Some(Value::String(base_url)) = params.arguments.get(0) {
                    match commands::set_base_url(base_url.clone()).await {
                        Ok(msg) => {
                            self.client
                                .log_message(MessageType::INFO, format!("Ziit LS: {}", msg))
                                .await;
                            Ok(Some(Value::String(msg)))
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to set base URL: {}", e);
                            self.client
                                .log_message(MessageType::ERROR, format!("Ziit LS: {}", error_msg))
                                .await;
                            Err(jsonrpc::Error::internal_error())
                        }
                    }
                } else {
                    Err(jsonrpc::Error::invalid_params(
                        "Base URL parameter required",
                    ))
                }
            }
            "ziit.openDashboard" => match commands::get_dashboard_url().await {
                Ok(url) => {
                    self.client
                        .log_message(
                            MessageType::INFO,
                            format!("Ziit LS: Dashboard URL: {}", url),
                        )
                        .await;
                    Ok(Some(Value::String(url)))
                }
                Err(e) => {
                    let error_msg = format!("Failed to get dashboard URL: {}", e);
                    self.client
                        .log_message(MessageType::ERROR, format!("Ziit LS: {}", error_msg))
                        .await;
                    Err(jsonrpc::Error::internal_error())
                }
            },
            "ziit.showStatus" => match commands::get_config_status().await {
                Ok(status) => {
                    let status_msg = format!(
                        "Config: {}\nAPI Key: {}\nBase URL: {}",
                        status.config_path,
                        if status.has_api_key { "Set" } else { "Not Set" },
                        status.base_url
                    );
                    self.client
                        .log_message(MessageType::INFO, format!("Ziit LS: {}", status_msg))
                        .await;
                    Ok(Some(Value::String(status_msg)))
                }
                Err(e) => {
                    let error_msg = format!("Failed to get status: {}", e);
                    self.client
                        .log_message(MessageType::ERROR, format!("Ziit LS: {}", error_msg))
                        .await;
                    Err(jsonrpc::Error::internal_error())
                }
            },
            _ => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("Ziit LS: Unknown command: {}", params.command),
                    )
                    .await;
                Err(jsonrpc::Error::method_not_found())
            }
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .init();

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
        eprintln!(
            "Ziit Language Server v{} starting in standalone mode...",
            env!("CARGO_PKG_VERSION")
        );
        log::info!(
            "Ziit Language Server v{} starting in standalone mode",
            env!("CARGO_PKG_VERSION")
        );
    } else {
        eprintln!(
            "Ziit Language Server v{} starting...",
            env!("CARGO_PKG_VERSION")
        );
        log::info!(
            "Ziit Language Server v{} starting",
            env!("CARGO_PKG_VERSION")
        );
    }

    let stdin = tokio_stdin();
    let stdout = tokio_stdout();

    let (service, socket) = LspService::build(ZiitLanguageServer::new).finish();

    log::info!("=== LSP service built, starting server loop ===");
    log::info!("Waiting for LSP initialize request from client...");
    Server::new(stdin, stdout, socket).serve(service).await;
    log::info!("=== Server stopped ===");
}
