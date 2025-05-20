use std::fs;
use std::path::Path;
use zed_extension_api::{self as zed, Command, Extension, LanguageServerId, Result, Worktree};

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
            Path::new(&version_dir).join(format!("{binary}.exe")).to_string_lossy().to_string()
        } else {
            Path::new(&version_dir).join(binary).to_string_lossy().to_string()
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
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let ls_name = if cfg!(windows) { "ziit-ls.exe" } else { "ziit-ls" };
        
        log::debug!("Looking for language server binary: {}", ls_name);

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

        log::debug!("Downloading language server binary");
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
}

zed::register_extension!(ZiitExtension);
