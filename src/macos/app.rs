use anyhow::{Error, Result, anyhow};
use dialoguer::{Select, console::style};
use std::path::PathBuf;

use crate::theme::AppTheme;
use crate::utils::pid_running;

pub(crate) struct App {
    pub(crate) theme: AppTheme,
    pub(crate) config_path: PathBuf,
    pub(crate) cli_mode: bool,
}

impl App {
    pub(crate) fn new() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位 HOME 目录"))?;
        Ok(Self {
            theme: AppTheme::new(),
            config_path: home.join(".maibot_config"),
            cli_mode: false,
        })
    }

    pub(crate) fn set_cli_mode(&mut self) {
        self.cli_mode = true;
    }

    pub(crate) fn run(&mut self) -> Result<()> {
        loop {
            self.clear();
            self.print_header(None);
            self.print_runtime_status();
            self.print_home_banner();
            let items = [
                "安装 / 更新 MaiBot",
                "管理 MaiBot 核心",
                "管理 Bot 协议端服务",
                "配置与访问",
                "插件管理",
                "退出",
            ];
            let choice = Select::with_theme(&self.theme)
                .with_prompt("主菜单")
                .items(items)
                .default(0)
                .interact()?;
            let result = match choice {
                0 => self.install_update_flow(),
                1 => self.manage_maibot_menu(),
                2 => self.manage_bot_protocol_menu(),
                3 => self.manage_config_access_menu(),
                4 => self.manage_plugins_menu(),
                _ => break,
            };
            self.handle_menu_result(result)?;
        }
        Ok(())
    }

    pub(crate) fn handle_menu_result(&self, result: Result<()>) -> Result<bool> {
        match result {
            Ok(()) => Ok(true),
            Err(error) => {
                self.print_menu_error(&error)?;
                Ok(false)
            }
        }
    }

    fn print_menu_error(&self, error: &Error) -> Result<()> {
        println!();
        println!(
            "  {} {}",
            style("!").red().bold(),
            style("操作失败").red().bold()
        );
        println!("  {}", style(error.to_string()).red());
        for cause in error.chain().skip(1) {
            println!("    {}", style(format!("原因: {cause}")).dim());
        }
        self.pause("按回车返回当前菜单")?;
        Ok(())
    }

    fn print_runtime_status(&self) {
        let cfg = match self.load_config() {
            Ok(cfg) if !cfg.mai_path.is_empty() => cfg,
            _ => {
                self.print_hint("未检测到安装，进入「安装 / 更新 MaiBot」开始部署");
                self.print_line();
                return;
            }
        };
        self.print_section("运行状态", "");
        let pid_path = PathBuf::from(&cfg.mai_path).join("logs").join("maibot.pid");
        let maibot_pid = pid_running(&pid_path).unwrap_or(None);
        let maibot_running = maibot_pid.is_some();
        self.print_status_dot(
            "MaiBot",
            &maibot_pid
                .map(|pid| format!("运行中 (pid: {pid})"))
                .unwrap_or_else(|| "未运行".into()),
            maibot_running,
        );
        if !cfg.bot_protocols.is_empty() && cfg.bot_protocols != "none" {
            self.print_hint("macOS 版暂未适配 NapCat / LLBot 协议端服务");
        }
        self.print_line();
    }
}
