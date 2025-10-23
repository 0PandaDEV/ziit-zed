use std::fs;
use std::path::Path;
use zed_extension_api::{
    self as zed, settings::LspSettings, Command, Extension, LanguageServerId, Result, Worktree,
};

struct ZiitExtension {
    cached_binary_path: Option<String>,
}

impl ZiitExtension {
    fn target_triple(&self) -> Result<String, String> {
        let (platform, arch) = zed::current_platform();
        let (arch, os) = {
            let arch = match arch {
                zed::Architecture::Aarch64 => "aarch64",
                zed::Architecture::X8664 => "x86_64",
                _ => return Err(format!("unsupported architecture: {arch:?}")),
            };

            let os = match platform {
                zed::Os::Mac => "apple-darwin",
                zed::Os::Linux => "unknown-linux-gnu",
                zed::Os::Windows => "pc-windows-msvc",
            };

            (arch, os)
        };

        Ok(format!("{}-{}", arch, os))
    }

    fn download(
        &self,
        language_server_id: &LanguageServerId,
        binary: &str,
        repo: &str,
    ) -> Result<String> {
        let release = zed::latest_github_release(
            repo,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let target_triple = self.target_triple()?;
        let asset_name = format!("{binary}-{target_triple}.zip");
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("no asset found matching {:?}", asset_name))?;

        let version_dir = format!("{binary}-{}", release.version);
        let binary_path = if target_triple.ends_with("pc-windows-msvc") {
            Path::new(&version_dir)
                .join(format!("{binary}.exe"))
                .to_string_lossy()
                .to_string()
        } else {
            Path::new(&version_dir)
                .join(binary)
                .to_string_lossy()
                .to_string()
        };

        if !fs::metadata(&binary_path).map_or(false, |stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &version_dir,
                zed::DownloadedFileType::Zip,
            )
            .map_err(|err| format!("failed to download file: {err}"))?;

            let entries = fs::read_dir(".")
                .map_err(|err| format!("failed to list working directory {err}"))?;

            for entry in entries {
                let entry = entry.map_err(|err| format!("failed to load directory entry {err}"))?;
                if let Some(file_name) = entry.file_name().to_str() {
                    if file_name.starts_with(binary) && file_name != version_dir {
                        fs::remove_dir_all(entry.path()).ok();
                    }
                }
            }
        }

        zed::make_file_executable(&binary_path)?;

        if !fs::metadata(&binary_path).map_or(false, |stat| stat.is_file()) {
            return Err(format!("Binary not available after download: {}", binary_path).into());
        }

        log::info!("Successfully prepared binary at: {}", binary_path);
        Ok(binary_path)
    }

    fn language_server_binary_path(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<String> {
        let ls_name = if cfg!(windows) {
            "ziit-ls.exe"
        } else {
            "ziit-ls"
        };

        log::debug!("Looking for language server binary: {}", ls_name);



        let dev_paths = vec![
            format!(
                "/home/pandadev/Developer/Extensions/ziit-zed/ziit-ls/target/release/{}",
                ls_name
            ),
            format!(
                "/home/pandadev/Developer/Extensions/ziit-zed/target/release/{}",
                ls_name
            ),
        ];

        for dev_path in &dev_paths {
            if fs::metadata(dev_path).map_or(false, |stat| stat.is_file()) {
                log::info!("Using local development binary at: {}", dev_path);
                return Ok(dev_path.clone());
            }
        }


        let worktree_root = worktree.root_path();
        let local_binary = format!("{}/target/release/{}", worktree_root, ls_name);
        if fs::metadata(&local_binary).map_or(false, |stat| stat.is_file()) {
            log::info!(
                "Using local development binary from worktree at: {}",
                local_binary
            );
            return Ok(local_binary);
        }

        let local_binary_subdir = format!("{}/ziit-ls/target/release/{}", worktree_root, ls_name);
        if fs::metadata(&local_binary_subdir).map_or(false, |stat| stat.is_file()) {
            log::info!(
                "Using local development binary from worktree subdir at: {}",
                local_binary_subdir
            );
            return Ok(local_binary_subdir);
        }

        if let Some(path) = worktree.which(ls_name) {
            log::debug!("Found language server in PATH: {}", path);
            return Ok(path.clone());
        }

        let target_triple = self.target_triple()?;
        if let Some(path) = worktree.which(&target_triple) {
            log::debug!("Found language server via target triple: {}", path);
            return Ok(path.clone());
        }

        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).map_or(false, |stat| stat.is_file()) {
                log::debug!("Using cached language server path: {}", path);
                return Ok(path.clone());
            }
        }

        if let Ok(entries) = fs::read_dir(".") {
            for entry in entries.flatten() {
                if let Some(dir_name) = entry.file_name().to_str() {
                    if dir_name.starts_with("ziit-ls-v") {
                        let potential_binary = entry.path().join(ls_name);
                        if potential_binary.exists() && potential_binary.is_file() {
                            let binary_path_str = potential_binary.to_string_lossy().to_string();
                            log::info!(
                                "Found existing language server binary at: {}",
                                binary_path_str
                            );
                            self.cached_binary_path = Some(binary_path_str.clone());
                            return Ok(binary_path_str);
                        }
                    }
                }
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        log::debug!("Downloading language server binary from GitHub");
        let binary_path = self.download(language_server_id, "ziit-ls", "0PandaDEV/ziit-zed")?;
        log::debug!("Downloaded language server to: {}", binary_path);

        self.cached_binary_path = Some(binary_path.clone());

        Ok(binary_path)
    }
}

impl Extension for ZiitExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let binary_path = self.language_server_binary_path(language_server_id, worktree)?;

        if let Err(err) = fs::metadata(&binary_path) {
            return Err(format!("Binary not found at path {}: {}", binary_path, err).into());
        }

        log::info!("Executing language server binary: {}", binary_path);

        let args = vec!["--standalone".to_string()];

        Ok(Command {
            command: binary_path,
            args,
            env: worktree.shell_env(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        let settings = LspSettings::for_worktree("ziit-ls", worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.initialization_options.clone());

        if let Some(options) = &settings {
            log::info!(
                "Passing initialization options to language server: {:?}",
                options
            );
            return Ok(Some(options.clone()));
        }

        log::warn!("No initialization options found in Zed settings.");
        log::info!("Attempting to read config from XDG config directory as fallback...");

        let config_path = if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg_config.is_empty() {
                format!("{}/ziit/config.json", xdg_config)
            } else {
                format!(
                    "{}/.config/ziit/config.json",
                    std::env::var("HOME").unwrap_or_default()
                )
            }
        } else {
            format!(
                "{}/.config/ziit/config.json",
                std::env::var("HOME").unwrap_or_default()
            )
        };

        if let Ok(config_content) = std::fs::read_to_string(&config_path) {
            if let Ok(config_json) =
                zed::serde_json::from_str::<zed::serde_json::Value>(&config_content)
            {
                log::info!("Successfully read config from file: {}", config_path);
                log::info!("Config from file: {:?}", config_json);
                return Ok(Some(config_json));
            }
        }

        log::warn!("Could not read config from file: {}", config_path);
        log::warn!(
            "Please configure apiKey either in Zed settings or in {}",
            config_path
        );

        Ok(None)
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        let settings = LspSettings::for_worktree("ziit-ls", worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.settings.clone());

        Ok(settings)
    }
}

zed::register_extension!(ZiitExtension);
