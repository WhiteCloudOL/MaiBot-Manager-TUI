use crate::app::App;
use crate::model::AppConfig;
use crate::terminal::restore_terminal_state;
use anyhow::{Context, Result, anyhow, bail};
use std::{
    collections::BTreeMap,
    fs,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

impl App {
    pub(crate) fn get_public_ip(&self) -> Result<String> {
        let output = Command::new("cmd")
            .args([
                "/C",
                "curl.exe -s4 --max-time 5 --connect-timeout 3 ifconfig.me",
            ])
            .output()?;
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if ip.is_empty() {
            bail!("公网 IP 查询失败（curl 超时或无结果）");
        }
        Ok(ip)
    }

    pub(crate) fn load_config(&self) -> Result<AppConfig> {
        let content = fs::read_to_string(&self.config_path)
            .with_context(|| format!("未找到配置文件: {}", self.config_path.display()))?;
        let mut map = BTreeMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim();
                let stripped = if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
                    || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2)
                {
                    &v[1..v.len() - 1]
                } else {
                    v
                };
                map.insert(k.trim().to_string(), stripped.to_string());
            }
        }
        Ok(AppConfig {
            user_install_path: map.get("USER_INSTALL_PATH").cloned().unwrap_or_default(),
            mai_path: map
                .get("MAI_PATH")
                .cloned()
                .or_else(|| map.get("USER_INSTALL_PATH").cloned())
                .unwrap_or_default(),
            mai_python_env: map
                .get("MAI_PYTHON_ENV")
                .cloned()
                .unwrap_or_else(|| "uv".into()),
            mai_llbot_path: map.get("MAI_LLBOT_PATH").cloned().unwrap_or_default(),
            mai_install_mode: map.get("MAI_INSTALL_MODE").cloned().unwrap_or_default(),
            mai_venv_mode: map.get("MAI_VENV_MODE").cloned().unwrap_or_default(),
            maibot_branch: map
                .get("MAIBOT_BRANCH")
                .cloned()
                .unwrap_or_else(|| "main".into()),
            pip_display: map.get("PIP_DISPLAY").cloned().unwrap_or_default(),
            pip_index: map.get("PIP_INDEX").cloned().unwrap_or_default(),
            pip_host: map.get("PIP_HOST").cloned().unwrap_or_default(),
            bot_protocols: map.get("BOT_PROTOCOLS").cloned().unwrap_or_default(),
        })
    }

    pub(crate) fn save_config(&self, cfg: &AppConfig) -> Result<()> {
        let content = format!(
            "USER_INSTALL_PATH=\"{}\"\n\
             MAI_PATH=\"{}\"\n\
             MAI_PYTHON_ENV=\"{}\"\n\
             MAI_LLBOT_PATH=\"{}\"\n\
             MAI_INSTALL_MODE=\"{}\"\n\
             MAI_VENV_MODE=\"{}\"\n\
             MAIBOT_BRANCH=\"{}\"\n\
             PIP_DISPLAY=\"{}\"\n\
             PIP_INDEX=\"{}\"\n\
             PIP_HOST=\"{}\"\n\
             BOT_PROTOCOLS=\"{}\"\n",
            cfg.user_install_path,
            cfg.mai_path,
            cfg.mai_python_env,
            cfg.mai_llbot_path,
            cfg.mai_install_mode,
            cfg.mai_venv_mode,
            cfg.maibot_branch,
            cfg.pip_display,
            cfg.pip_index,
            cfg.pip_host,
            cfg.bot_protocols,
        );
        fs::write(&self.config_path, content)?;
        Ok(())
    }

    pub(crate) fn require_config(&self) -> Result<AppConfig> {
        self.load_config()
            .map_err(|_| anyhow!("未找到安装配置，请先执行安装 / 更新"))
    }

    pub(crate) fn run_shell(&self, command: &str) -> Result<()> {
        self.print_command_start(command);
        let mut script = String::from("@echo off\r\nsetlocal EnableExtensions\r\n");
        script.push_str(command);
        if !command.ends_with('\n') {
            script.push_str("\r\n");
        }

        let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let path = std::env::temp_dir().join(format!("maibot-manager-{millis}.bat"));
        fs::write(&path, script)?;
        let status = Command::new("cmd")
            .arg("/C")
            .arg(&path)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("执行 BAT 失败: {}", path.display()))?;
        let _ = fs::remove_file(&path);

        if !status.success() {
            restore_terminal_state();
            bail!("命令执行失败: {command}");
        }
        Ok(())
    }
}
