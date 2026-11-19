use crate::app::App;
use crate::terminal::restore_terminal_state;
use anyhow::{Context, Result, bail};
use std::{
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
