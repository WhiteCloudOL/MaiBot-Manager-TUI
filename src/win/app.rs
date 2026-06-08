use anyhow::{Error, Result, anyhow};
use dialoguer::console::style;
use std::path::PathBuf;

use crate::theme::AppTheme;
use crate::ui::{ActionItem, StatusCard};

pub(crate) struct App {
    pub(crate) theme: AppTheme,
    pub(crate) config_path: PathBuf,
    pub(crate) cli_mode: bool,
}

impl App {
    pub(crate) fn new() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位用户目录"))?;
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
                ActionItem::normal("核心服务", "管理独立控制台和 PID 停止"),
                ActionItem::normal("协议端服务", "管理 NapCat Shell / LLBot Desktop"),
                ActionItem::normal("访问配置", "查看 WebUI 地址、密钥和白名单"),
                ActionItem::normal("插件中心", "安装、卸载和修复插件依赖"),
                ActionItem::back("退出管理器", "已启动窗口保持当前状态"),
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
        let maibot_running = self.maibot_core_running().unwrap_or(false);
        let llbot_running = self.llbot_running().unwrap_or(false);
        let napcat_running = self.napcat_running().unwrap_or(false);
        let mut cards = Vec::new();
        cards.push(if maibot_running {
            StatusCard::running("MaiBot", "独立 Windows 控制台 · PID 文件可停止进程树")
        } else {
            StatusCard::stopped("MaiBot", "核心控制台未运行")
        });
        cards.push(if PathBuf::from(&cfg.mai_path).join("NapCat").exists() {
            if napcat_running {
                StatusCard::running("NapCatQQ", "NapCat Shell 窗口/进程已运行")
            } else {
                StatusCard::stopped("NapCatQQ", "已安装，NapCat Shell 未运行")
            }
        } else {
            StatusCard::neutral("NapCatQQ", "未安装", "可在部署计划中启用")
        });
        cards.push(
            if !cfg.mai_llbot_path.is_empty() || PathBuf::from(&cfg.mai_path).join("LLBot").exists()
            {
                if llbot_running {
                    StatusCard::running("LuckyLilliaBot", "Desktop 进程已运行")
                } else {
                    StatusCard::stopped("LuckyLilliaBot", "已安装，Desktop 进程未运行")
                }
            } else {
                StatusCard::neutral("LuckyLilliaBot", "未安装", "可在部署计划中启用")
            },
        );
        self.print_status_cards("服务概览", &cards);
    }
}
