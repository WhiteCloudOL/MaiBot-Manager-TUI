use crate::app::App;
use crate::terminal::restore_terminal_state;
use anyhow::{Context, Result, bail};
use std::{
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
};

impl App {
    pub(crate) fn get_public_ip(&self) -> Result<String> {
        let output = Command::new("bash")
            .arg("-lc")
            .arg("curl -s4 --max-time 5 --connect-timeout 3 ifconfig.me")
            .output()?;
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if ip.is_empty() {
            bail!("公网 IP 查询失败（curl 超时或无结果）");
        }
        Ok(ip)
    }

    pub(crate) fn run_shell(&self, command: &str) -> Result<()> {
        self.print_command_start(command);
        let status = Command::new("bash")
            .arg("-lc")
            .arg(command)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("执行命令失败: {command}"))?;
        if matches!(status.signal(), Some(2) | Some(15)) {
            restore_terminal_state();
            eprintln!("\n操作已被用户中断 (Ctrl+C)");
            std::process::exit(130);
        }
        if !status.success() {
            bail!("命令执行失败: {command}");
        }
        Ok(())
    }
}
