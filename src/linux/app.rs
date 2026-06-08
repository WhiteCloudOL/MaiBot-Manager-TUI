use anyhow::{Error, Result, anyhow};
use dialoguer::console::style;
use std::path::PathBuf;
use std::process::Command;

use crate::theme::AppTheme;
use crate::ui::{ActionItem, StatusCard};
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
                ActionItem::primary("部署与更新", "安装、更新或调整 MaiBot 工作区"),
                ActionItem::normal("核心服务", "管理 MaiBot screen 会话和控制台"),
                ActionItem::normal("协议端服务", "管理 NapCatQQ / LuckyLilliaBot"),
                ActionItem::normal("访问配置", "查看 WebUI 地址、密钥和白名单"),
                ActionItem::normal("插件中心", "安装、卸载和修复插件依赖"),
                ActionItem::back("退出管理器", "后台服务保持当前状态"),
            ];
            let choice = self.select_action("选择工作区", &items)?;
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
                self.print_empty_state(
                    "未检测到 MaiBot 工作区",
                    "从「部署与更新」开始，完成后这里会显示服务健康状态。",
                );
                return;
            }
        };
        let maibot_running = screen_exists("maibot").unwrap_or(false);
        let llbot_running = screen_exists("llbot").unwrap_or(false);
        let napcat_running = napcat_running();
        let mut cards = Vec::new();
        cards.push(if maibot_running {
            StatusCard::running("MaiBot", "screen: maibot · 可进入控制台或查看日志")
        } else {
            StatusCard::stopped("MaiBot", "核心服务未启动")
        });
        cards.push(if napcat_installed(&cfg.mai_path) {
            if napcat_running {
                StatusCard::running("NapCatQQ", "Docker 容器已纳入管理")
            } else {
                StatusCard::stopped("NapCatQQ", "已安装，Docker 容器未运行")
            }
        } else {
            StatusCard::neutral("NapCatQQ", "未安装", "可在部署计划中启用")
        });
        cards.push(if llbot_installed(&cfg) {
            if llbot_running {
                StatusCard::running("LuckyLilliaBot", "screen: llbot · CLI 协议端运行中")
            } else {
                StatusCard::stopped("LuckyLilliaBot", "已安装，screen 会话未运行")
            }
        } else {
            StatusCard::neutral("LuckyLilliaBot", "未安装", "可在部署计划中启用")
        });
        self.print_status_cards("服务概览", &cards);
    }
}

pub(crate) fn napcat_running() -> bool {
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
