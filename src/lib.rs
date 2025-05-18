use std::fs;
use zed_extension_api::{self as zed, Command, Extension, LanguageServerId, Result, Worktree};

struct ZiitExtension {
    cached_binary_path: Option<String>,
}

impl ZiitExtension {
    fn target_triple(&self, binary: &str) -> Result<String, String> {
        let (platform, arch) = zed::current_platform();
        let (arch, os) = {
            let arch = match arch {
                zed::Architecture::Aarch64 if binary == "ziit-ls" => "aarch64",
                zed::Architecture::X8664 if binary == "ziit-ls" => "x86_64",
                _ => return Err(format!("unsupported architecture: {arch:?}")),
            };

            let os = match platform {
                zed::Os::Mac if binary == "ziit-ls" => "apple-darwin",
                zed::Os::Linux if binary == "ziit-ls" => "unknown-linux-gnu",
                zed::Os::Windows if binary == "ziit-ls" => "pc-windows-msvc",
                _ => return Err("unsupported platform".to_string()),
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

        let target_triple = self.target_triple(binary)?;

        let asset_name = format!("{target_triple}.zip");
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("no asset found matching {:?}", asset_name))?;

        let version_dir = format!("{binary}-{}", release.version);
        let binary_path = if binary == "wakatime-cli" {
            format!("{version_dir}/{target_triple}")
        } else {
            format!("{version_dir}/{binary}")
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

        if let Some(path) = worktree.which("ziit-ls") {
            return Ok(path.clone());
        }

        let target_triple = self.target_triple("ziit-ls")?;
        if let Some(path) = worktree.which(&target_triple) {
            return Ok(path.clone());
        }

        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).map_or(false, |stat| stat.is_file()) {
                return Ok(path.clone());
            }
        }

        let binary_path =
            self.download(language_server_id, "ziit-ls", "0PandaDEV/ziit-zed")?;

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

        let args = vec!["--standalone".to_string()];

        Ok(Command {
            command: binary_path,
            args,
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(ZiitExtension);
