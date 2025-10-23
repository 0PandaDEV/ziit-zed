use std::path::{Path, PathBuf};
use std::process::Command;


pub fn detect_project(file_path: Option<&str>) -> Option<String> {
    if let Some(path) = file_path {
        if let Some(project) = get_project_from_git(path) {
            return Some(project);
        }
        if let Some(project) = get_project_from_path(path) {
            return Some(project);
        }
    }

    None
}


pub fn detect_branch(file_path: Option<&str>) -> Option<String> {
    if let Some(path) = file_path {
        if let Some(branch) = get_git_branch(path) {
            return Some(branch);
        }
    }

    None
}


fn get_project_from_git(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    if let Some(remote_url) = get_git_remote_url(&dir) {
        if let Some(project) = extract_project_from_remote_url(&remote_url) {
            log::debug!("Extracted project '{}' from git remote URL", project);
            return Some(project);
        }
    }
    if let Some(repo_root) = get_git_repo_root(&dir) {
        if let Some(dir_name) = repo_root.file_name() {
            let project = dir_name.to_string_lossy().to_string();
            log::debug!(
                "Using git repo root directory name as project: '{}'",
                project
            );
            return Some(project);
        }
    }

    None
}


fn get_git_remote_url(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(&["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;

    if output.status.success() {
        let url = String::from_utf8(output.stdout).ok()?;
        let url = url.trim().to_string();
        if !url.is_empty() {
            return Some(url);
        }
    }

    None
}


fn get_git_repo_root(dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(&["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;

    if output.status.success() {
        let path_str = String::from_utf8(output.stdout).ok()?;
        let path_str = path_str.trim();
        if !path_str.is_empty() {
            return Some(PathBuf::from(path_str));
        }
    }

    None
}


fn get_git_branch(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    let output = Command::new("git")
        .current_dir(&dir)
        .args(&["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        let branch = String::from_utf8(output.stdout).ok()?;
        let branch = branch.trim().to_string();
        if !branch.is_empty() && branch != "HEAD" {
            log::debug!("Detected git branch: '{}'", branch);
            return Some(branch);
        }
    }

    None
}






fn extract_project_from_remote_url(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.strip_suffix(".git").unwrap_or(url);
    if url.contains('@') && url.contains(':') {
        if let Some(after_colon) = url.split(':').last() {
            if let Some(project) = after_colon.split('/').last() {
                return Some(project.to_string());
            }
        }
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        if let Some(project) = url.split('/').last() {
            return Some(project.to_string());
        }
    }
    if let Some(project) = url.split('/').last() {
        if !project.is_empty() {
            return Some(project.to_string());
        }
    }

    None
}



fn get_project_from_path(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    let mut current = path;
    while let Some(parent) = current.parent() {
        if has_project_markers(parent) {
            if let Some(dir_name) = parent.file_name() {
                let project = dir_name.to_string_lossy().to_string();
                log::debug!("Detected project '{}' from path structure", project);
                return Some(project);
            }
        }
        current = parent;
    }
    let components: Vec<_> = path.components().collect();
    if components.len() >= 2 {
        if let Some(component) = components.get(components.len() - 2) {
            let project = component.as_os_str().to_string_lossy().to_string();
            log::debug!("Using parent directory as project: '{}'", project);
            return Some(project);
        }
    }

    None
}


fn has_project_markers(dir: &Path) -> bool {
    let markers = [
        ".git",
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "CMakeLists.txt",
        "Makefile",
        "setup.py",
        "pyproject.toml",
        ".project",
        "composer.json",
        "Gemfile",
    ];

    for marker in &markers {
        if dir.join(marker).exists() {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_project_from_remote_url() {
        assert_eq!(
            extract_project_from_remote_url("https://github.com/user/my-project.git"),
            Some("my-project".to_string())
        );
        assert_eq!(
            extract_project_from_remote_url("git@github.com:user/my-project.git"),
            Some("my-project".to_string())
        );
        assert_eq!(
            extract_project_from_remote_url("https://github.com/user/my-project"),
            Some("my-project".to_string())
        );
    }
}
