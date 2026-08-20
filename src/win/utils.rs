use anyhow::{Result, anyhow, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位用户目录"))?;
    let path = if let Some(rest) = input
        .strip_prefix("~/")
        .or_else(|| input.strip_prefix("~\\"))
    {
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
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let home = dirs::home_dir().and_then(|p| p.canonicalize().ok());
    let drive_root = canonical
        .ancestors()
        .last()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| canonical.clone());
    if canonical == drive_root || home.as_ref() == Some(&canonical) {
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
    Ok(Command::new("cmd")
        .args(["/C", "where", name])
        .status()?
        .success())
}

pub fn command_exists_with_tools(root: &Path, name: &str) -> Result<bool> {
    let mut command = Command::new("where.exe");
    apply_windows_tools_env(&mut command, root);
    Ok(command
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

pub fn tools_dir(root: &Path) -> PathBuf {
    root.join("tools")
}

pub fn portable_git_dir(root: &Path) -> PathBuf {
    tools_dir(root).join("git")
}

pub fn portable_git_exe(root: &Path) -> PathBuf {
    portable_git_dir(root).join("cmd").join("git.exe")
}

pub fn git_executable(root: &Path) -> PathBuf {
    let portable = portable_git_exe(root);
    if portable.exists() {
        portable
    } else {
        PathBuf::from("git")
    }
}

pub fn portable_uv_dir(root: &Path) -> PathBuf {
    tools_dir(root).join("uv")
}

pub fn portable_uv_exe(root: &Path) -> PathBuf {
    portable_uv_dir(root).join("uv.exe")
}

pub fn uv_executable(root: &Path) -> PathBuf {
    let portable = portable_uv_exe(root);
    if portable.exists() {
        portable
    } else {
        PathBuf::from("uv")
    }
}

pub fn windows_tools_path_entries(root: &Path) -> Vec<PathBuf> {
    let git = portable_git_dir(root);
    vec![
        git.join("cmd"),
        git.join("bin"),
        git.join("usr").join("bin"),
        portable_uv_dir(root),
    ]
}

pub fn windows_tools_path_prelude(root: &Path) -> String {
    let entries = windows_tools_path_entries(root)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(";");
    let tools = tools_dir(root);
    format!(
        "set \"PATH={entries};%PATH%\"\r\n\
         set \"UV_CACHE_DIR={}\"\r\n\
         set \"UV_PYTHON_INSTALL_DIR={}\"\r\n",
        tools.join("uv-cache").display(),
        tools.join("python").display()
    )
}

pub fn with_windows_tools_path(root: &Path, command: &str) -> String {
    let mut script = windows_tools_path_prelude(root);
    script.push_str(command);
    script
}

pub fn apply_windows_tools_env(command: &mut Command, root: &Path) {
    let current_path = env::var_os("PATH").unwrap_or_default();
    let mut paths = windows_tools_path_entries(root);
    paths.extend(env::split_paths(&current_path));
    if let Ok(joined) = env::join_paths(paths) {
        command.env("PATH", joined);
    }
    command.env("UV_CACHE_DIR", tools_dir(root).join("uv-cache"));
    command.env("UV_PYTHON_INSTALL_DIR", tools_dir(root).join("python"));
}

pub fn detect_arch() -> Result<&'static str> {
    match env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        other => bail!("当前 Windows 架构暂不支持自动安装 LuckyLilliaBot: {other}"),
    }
}

pub fn bat_quote(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('"', "\"\""))
}

pub fn bat_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
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
