use zed_extension_api::{self as zed, Command, Extension, LanguageServerId, Result, Worktree};

pub mod api;
pub mod config;
pub mod heartbeat;

const ZIIT_LANGUAGE_SERVER_NAME: &str = "ziit";

struct ZiitExtension {}

impl Extension for ZiitExtension {
    fn new() -> Self {
        log::info!("Ziit Zed Extension Initialized");
        Self {}
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        _worktree: &Worktree,
    ) -> Result<Command> {
        if language_server_id.as_ref() != ZIIT_LANGUAGE_SERVER_NAME {
            return Err("Unsupported language server".into());
        }

        let ls_binary_name = if cfg!(windows) {
            "ziit-ls.exe"
        } else {
            "ziit-ls"
        };

        log::info!(
            "Requesting Zed to start language server: {}",
            ls_binary_name
        );
        Ok(Command {
            command: ls_binary_name.to_string(),
            args: vec![],
            env: Default::default(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &LanguageServerId,
        _worktree: &Worktree,
    ) -> Result<Option<serde_json::Value>> {
        if language_server_id.as_ref() != ZIIT_LANGUAGE_SERVER_NAME {
            return Err("Unsupported language server for initialization options".into());
        }

        let options_future = async {
            match config::read_config_file().await {
                Ok(config) => {
                    log::info!(
                        "Passing initialization options: apiKey present={}, baseUrl present={}",
                        config.api_key.is_some(),
                        config.base_url.is_some()
                    );
                    match serde_json::to_value(config) {
                        Ok(val) => Some(val),
                        Err(e) => {
                            log::error!(
                                "Failed to serialize config to JSON for LSP init options: {}",
                                e
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to read config for LSP init options: {}", e);
                    None
                }
            }
        };

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => Ok(handle.block_on(options_future)),
            Err(_) => {
                log::warn!(
                    "Not in a Tokio runtime for LSP init options, creating one shot runtime."
                );
                Ok(tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(options_future))
            }
        }
    }
}

zed::register_extension!(ZiitExtension);
