use anyhow::{Error, Result, anyhow};
use dialoguer::{Select, console::style};
use std::path::PathBuf;
use std::process::Command;

use crate::theme::AppTheme;
use crate::utils::screen_exists;

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
                .items(&items)
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
        let maibot_running = screen_exists("maibot").unwrap_or(false);
        let llbot_running = screen_exists("llbot").unwrap_or(false);
        let napcat_running = napcat_running();
        self.print_status_dot(
            "MaiBot",
            if maibot_running {
                "运行中 (screen: maibot)"
            } else {
                "未运行"
            },
            maibot_running,
        );
        if napcat_installed(&cfg.mai_path) {
            self.print_status_dot(
                "NapCat",
                if napcat_running {
                    "运行中 (docker)"
                } else {
                    "未运行"
                },
                napcat_running,
            );
        }
        if llbot_installed(&cfg) {
            self.print_status_dot(
                "LLBot",
                if llbot_running {
                    "运行中 (screen: llbot)"
                } else {
                    "未运行"
                },
                llbot_running,
            );
        }
        self.print_line();
    }
}

fn napcat_running() -> bool {
    let output = Command::new("bash")
        .arg("-lc")
        .arg("docker ps --filter name=^napcat$ --filter status=running --format '{{.Names}}' 2>/dev/null")
        .output();
    match output {
        Ok(out) => !String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        Err(_) => false,
    }
}

fn napcat_installed(mai_path: &str) -> bool {
    !mai_path.is_empty() && PathBuf::from(mai_path).join("NapCat").exists()
}

fn llbot_installed(cfg: &crate::model::AppConfig) -> bool {
    if !cfg.mai_llbot_path.is_empty() && PathBuf::from(&cfg.mai_llbot_path).exists() {
        return true;
    }
    !cfg.mai_path.is_empty() && PathBuf::from(&cfg.mai_path).join("LLBot").exists()
}
