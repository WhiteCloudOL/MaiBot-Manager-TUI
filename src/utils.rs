use anyhow::{Result, anyhow, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

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
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn command_exists(name: &str) -> Result<bool> {
    Ok(Command::new("bash")
        .arg("-lc")
        .arg(format!("command -v {name} >/dev/null"))
        .status()?
        .success())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PkgManager {
    Apt,
    Dnf,
    Yum,
    Pacman,
    Zypper,
    Apk,
    Unknown,
}

impl PkgManager {
    pub fn detect() -> Self {
        for (cmd, pm) in [
            ("apt-get", PkgManager::Apt),
            ("dnf", PkgManager::Dnf),
            ("yum", PkgManager::Yum),
            ("pacman", PkgManager::Pacman),
            ("zypper", PkgManager::Zypper),
            ("apk", PkgManager::Apk),
        ] {
            if command_exists(cmd).unwrap_or(false) {
                return pm;
            }
        }
        PkgManager::Unknown
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Apt => "apt (Debian/Ubuntu)",
            Self::Dnf => "dnf (Fedora/RHEL 8+)",
            Self::Yum => "yum (RHEL/CentOS)",
            Self::Pacman => "pacman (Arch)",
            Self::Zypper => "zypper (openSUSE)",
            Self::Apk => "apk (Alpine)",
            Self::Unknown => "未识别",
        }
    }

    /// 把一组逻辑包名映射成当前 PM 的实际包名。约定输入用 Debian 风格。
    pub fn map_packages(self, pkgs: &[&str]) -> Vec<String> {
        pkgs.iter()
            .map(|p| match (self, *p) {
                (Self::Pacman, "python3") => "python".to_string(),
                (Self::Apk, "python3") => "python3".to_string(),
                (Self::Pacman, "ca-certificates") => "ca-certificates".to_string(),
                _ => (*p).to_string(),
            })
            .collect()
    }

    /// 生成「确保 N 个包已安装」的一行 shell。空 / 已安装 / Unknown 均安全。
    pub fn install_cmd(self, pkgs: &[&str]) -> Option<String> {
        if pkgs.is_empty() {
            return None;
        }
        let mapped = self.map_packages(pkgs);
        let joined = mapped.join(" ");
        let cmd = match self {
            Self::Apt => format!("sudo apt-get update -y && sudo apt-get install -y {joined}"),
            Self::Dnf => format!("sudo dnf install -y {joined}"),
            Self::Yum => format!("sudo yum install -y {joined}"),
            Self::Pacman => format!("sudo pacman -Sy --noconfirm --needed {joined}"),
            Self::Zypper => format!("sudo zypper --non-interactive install {joined}"),
            Self::Apk => format!("sudo apk add --no-cache {joined}"),
            Self::Unknown => return None,
        };
        Some(cmd)
    }
}

pub fn detect_arch() -> Result<&'static str> {
    match env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        other => bail!("当前架构不支持 LuckyLilliaBot 自动安装: {other}"),
    }
}

pub fn screen_exists(name: &str) -> Result<bool> {
    Ok(Command::new("bash")
        .arg("-lc")
        .arg(format!("screen -list | grep -q '\\.{name}[[:space:]]'"))
        .status()?
        .success())
}

/// 构造一条 `先杀掉同名 screen，再后台启动新 screen` 的 shell 命令。
pub fn screen_launch_cmd(name: &str, body: &str) -> String {
    let quoted = format!("'{}'", shell_escape_raw(&format!("{body}; exec bash")));
    format!("screen -S {name} -X quit >/dev/null 2>&1 || true; screen -dmS {name} bash -lc {quoted}")
}

pub fn screen_quit_cmd(name: &str) -> String {
    format!("screen -S {name} -X quit")
}

pub fn shell_escape(path: &Path) -> String {
    shell_escape_raw(&path.display().to_string())
}

pub fn shell_escape_raw(s: &str) -> String {
    s.replace('\'', "'\\''")
}

pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            if c.is_ascii() || c.is_control() {
                1
            } else {
                2
            }
        })
        .sum()
}

pub fn pad_left(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        " ".repeat(width - w) + s
    }
}
