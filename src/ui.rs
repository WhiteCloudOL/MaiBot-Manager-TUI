use crate::{app::App, model::*, utils::{display_width, pad_left}};
use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use dialoguer::console::style;
use dialoguer::Input;
use std::io::{self, Write};

const PANEL_WIDTH: usize = 60;
const KEY_WIDTH: usize = 12;

macro_rules! wln {
    () => { print!("\r\n") };
    ($($arg:tt)*) => { print!("{}\r\n", format_args!($($arg)*)) };
}

impl App {
    pub(crate) fn print_header(&self, plan: Option<&InstallPlan>) {
        let rule = "━".repeat(PANEL_WIDTH);
        wln!("{}", style(&rule).blue());
        let title = format!("MaiBot Manager TUI  v{}", APP_VERSION);
        let subtitle = APP_BUILD_LABEL;
        let credit = "作者：清蒸云鸭 · LICENSE：AGPL-3.0";
        let docs = "文档站: https://docs.meowyun.cn/index.html";
        let title_pad = (PANEL_WIDTH.saturating_sub(display_width(&title))) / 2;
        let sub_pad = (PANEL_WIDTH.saturating_sub(display_width(subtitle))) / 2;
        let credit_pad = (PANEL_WIDTH.saturating_sub(display_width(credit))) / 2;
        let docs_pad = (PANEL_WIDTH.saturating_sub(display_width(docs))) / 2;
        wln!("{}{}", " ".repeat(title_pad), style(&title).cyan().bold());
        if !subtitle.is_empty() {
            wln!("{}{}", " ".repeat(sub_pad), style(subtitle).blue().dim());
        }
        wln!("{}{}", " ".repeat(credit_pad), style(credit).dim());
        wln!("{}{}", " ".repeat(docs_pad), style(docs).cyan().dim());
        wln!("{}", style(&rule).blue());

        if let Some(plan) = plan {
            self.print_section("配置预览", "当前安装计划");
            self.print_kv("目录", &plan.install_path.display().to_string());
            self.print_kv("模式", plan.install_mode.label());
            self.print_kv("Python", plan.python_env.label());
            self.print_kv("环境", plan.venv_mode.label(plan.python_env));
            self.print_kv(
                "GitHub",
                if plan.github_proxy.is_empty() {
                    "自动测速（执行时选择最佳线路）"
                } else {
                    &plan.github_proxy
                },
            );
            self.print_kv(
                "PyPI",
                if plan.pip_display.is_empty() {
                    "系统默认"
                } else {
                    &plan.pip_display
                },
            );
            let protocol = if plan.bot_protocols.is_empty() {
                "暂不安装".to_string()
            } else {
                plan.bot_protocols
                    .iter()
                    .map(|v| match v {
                        BotProtocol::NapCat => "NapCatQQ",
                        BotProtocol::LuckyLilliaBot => "LuckyLilliaBot",
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            self.print_kv("协议端", &protocol);
            self.print_kv("Docker", plan.docker_mirror.label());
            self.print_line();
        } else if let Ok(cfg) = self.load_config() {
            if !cfg.mai_path.is_empty() {
                self.print_kv("安装目录", &cfg.mai_path);
                self.print_kv("Python", &cfg.mai_python_env);
                self.print_line();
            }
        }
    }

    pub(crate) fn clear(&self) {
        print!("\x1B[2J\x1B[1;1H");
        let _ = io::stdout().flush();
    }

    pub(crate) fn print_home_banner(&self) {
        wln!(
            "  {}",
            style("方向键移动 · Enter 确认 · Space 多选 · Ctrl+C 退出当前输入").dim()
        );
        self.print_line();
    }

    pub(crate) fn print_section(&self, title: &str, subtitle: &str) {
        wln!("  {}", style(title).cyan().bold());
        if !subtitle.is_empty() {
            wln!("  {}", style(subtitle).dim());
        }
        self.print_line();
    }

    pub(crate) fn print_kv(&self, key: &str, value: &str) {
        wln!(
            "  {} {}",
            style(pad_left(key, KEY_WIDTH)).blue(),
            style(value).white()
        );
    }

    pub(crate) fn print_status_dot(&self, key: &str, label: &str, ok: bool) {
        let dot = if ok {
            style("●").green()
        } else {
            style("○").red()
        };
        let text = if ok {
            style(label).green()
        } else {
            style(label).red().dim()
        };
        wln!("  {} {} {}", style(pad_left(key, KEY_WIDTH)).blue(), dot, text);
    }

    pub(crate) fn print_line(&self) {
        wln!("{}", style("─".repeat(PANEL_WIDTH)).dim());
    }

    pub(crate) fn print_hint(&self, msg: &str) {
        wln!("  {}", style(msg).dim());
    }

    pub(crate) fn pause(&self, msg: &str) -> Result<()> {
        let _: String = Input::with_theme(&self.theme)
            .with_prompt(msg)
            .allow_empty(true)
            .interact_text()?;
        Ok(())
    }

    pub(crate) fn with_prompt_mode<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, Show);
        let result = f();
        enable_raw_mode().context("重新启用终端 raw mode 失败")?;
        let mut stdout = io::stdout();
        execute!(stdout, Hide).context("重新隐藏终端光标失败")?;
        result
    }
}
