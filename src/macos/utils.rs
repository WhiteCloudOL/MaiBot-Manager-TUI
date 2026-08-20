use anyhow::{Context, Result, anyhow, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use unicode_width::UnicodeWidthStr;

pub fn list_plugins(dir: &Path) -> Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() && name != "__pycache__" && name != "data" {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

/// Maps a plugin manifest id to the directory name required by MaiBot.
pub fn plugin_dir_name(plugin_id: &str) -> String {
    plugin_id.replace('.', "_")
}

pub fn normalize_path(input: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位 HOME 目录"))?;
    let path = if let Some(rest) = input.strip_prefix("~/") {
        home.join(rest)
    } else if input == "~" {
        home.clone()
    } else {
        PathBuf::from(input)
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(home.join(path))
    }
}

pub fn normalize_url(input: &str) -> String {
    if input.starts_with("http://") || input.starts_with("https://") {
        input.to_string()
    } else {
        format!("https://{input}")
    }
}

pub fn repo_url(proxy: &str, repo: &str) -> String {
    if proxy == "https://github.com" {
        format!("https://github.com/{repo}.git")
    } else {
        format!("{proxy}/https://github.com/{repo}.git")
    }
}

pub fn convert_github_url(input: &str, default_proxy: &str) -> String {
    let input = if input.starts_with("http://") || input.starts_with("https://") {
        input.to_string()
    } else {
        format!("https://github.com/{input}")
    };
    let mut url = input.trim_end_matches('/').to_string();
    if !url.ends_with(".git") {
        url.push_str(".git");
    }
    if default_proxy == "https://github.com" || !url.contains("github.com/") {
        url
    } else {
        format!("{default_proxy}/{url}")
    }
}

pub fn clean_install_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let protected = dirs::home_dir()
        .ok_or_else(|| anyhow!("无法定位 HOME 目录"))?
        .canonicalize()
        .ok();
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if canonical == PathBuf::from("/") || protected.as_ref() == Some(&canonical) {
        bail!("拒绝清空危险目录: {}", dir.display());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn command_exists(name: &str) -> Result<bool> {
    let cmd = format!(
        "{} command -v '{}' >/dev/null 2>&1",
        macos_path_export(),
        shell_escape_raw(name)
    );
    Ok(Command::new("/bin/zsh")
        .arg("-lc")
        .arg(cmd)
        .status()?
        .success())
}

pub fn tools_dir(root: &Path) -> PathBuf {
    root.join("tools")
}

pub fn macos_path_entries() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
    ]
}

pub fn macos_path_export() -> String {
    let entries = macos_path_entries()
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    format!("export PATH='{}':\"$PATH\";", shell_escape_raw(&entries))
}

pub fn macos_tools_prelude(root: &Path) -> String {
    format!(
        "{} export UV_CACHE_DIR='{}'; export UV_PYTHON_INSTALL_DIR='{}'; ",
        macos_path_export(),
        shell_escape(&tools_dir(root).join("uv-cache")),
        shell_escape(&tools_dir(root).join("python"))
    )
}

pub fn with_macos_tools_path(root: &Path, command: &str) -> String {
    format!("{}{}", macos_tools_prelude(root), command)
}

pub fn brew_executable() -> Option<PathBuf> {
    let output = Command::new("/bin/zsh")
        .arg("-lc")
        .arg(format!("{} command -v brew", macos_path_export()))
        .output()
        .ok();
    if let Some(output) = output {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

pub fn brew_command() -> String {
    brew_executable()
        .map(|path| format!("'{}'", shell_escape(&path)))
        .unwrap_or_else(|| "brew".into())
}

pub fn brew_install_cmd(pkgs: &[&str]) -> Option<String> {
    if pkgs.is_empty() {
        return None;
    }
    Some(format!("{} install {}", brew_command(), pkgs.join(" ")))
}

pub fn detect_arch() -> Result<&'static str> {
    match env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        other => bail!("当前 macOS 架构暂不支持自动安装协议端: {other}"),
    }
}

pub fn read_pid_file(path: &Path) -> Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("读取 PID 文件失败: {}", path.display()))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let pid = trimmed
        .parse::<u32>()
        .with_context(|| format!("PID 文件内容无效: {}", path.display()))?;
    Ok(Some(pid))
}

pub fn pid_running(path: &Path) -> Result<Option<u32>> {
    let Some(pid) = read_pid_file(path)? else {
        return Ok(None);
    };
    let running = Command::new("/bin/zsh")
        .arg("-lc")
        .arg(format!("kill -0 {pid} >/dev/null 2>&1"))
        .status()?
        .success();
    if running {
        Ok(Some(pid))
    } else {
        let _ = fs::remove_file(path);
        Ok(None)
    }
}

pub fn shell_escape(path: &Path) -> String {
    shell_escape_raw(&path.display().to_string())
}

pub fn shell_escape_raw(s: &str) -> String {
    s.replace('\'', "'\\''")
}

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn pad_left(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        " ".repeat(width - w) + s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_adds_https() {
        assert_eq!(normalize_url("github.com"), "https://github.com");
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
    }

    #[test]
    fn display_width_counts_cjk_as_two_columns() {
        assert_eq!(display_width("Mac"), 3);
        assert_eq!(display_width("目录"), 4);
    }

    #[test]
    fn macos_path_export_includes_homebrew_prefixes() {
        let export = macos_path_export();
        assert!(export.contains("/opt/homebrew/bin"));
        assert!(export.contains("/usr/local/bin"));
    }
}
