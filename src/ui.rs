use crate::{
    app::App,
    model::*,
    terminal::restore_terminal_state,
    utils::{display_width, pad_left},
};
use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, RestorePosition, SavePosition, Show},
    event::{Event, KeyCode, KeyModifiers, poll, read},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use dialoguer::Input;
use dialoguer::console::style;
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

const PANEL_WIDTH: usize = 78;
const KEY_WIDTH: usize = 14;

macro_rules! wln {
    () => { print!("\r\n") };
    ($($arg:tt)*) => { print!("{}\r\n", format_args!($($arg)*)) };
}

#[derive(Clone, Copy)]
pub(crate) enum ActionKind {
    Primary,
    Normal,
    Destructive,
    Back,
}

pub(crate) struct ActionItem<'a> {
    label: &'a str,
    detail: &'a str,
    kind: ActionKind,
}

impl<'a> ActionItem<'a> {
    pub(crate) fn primary(label: &'a str, detail: &'a str) -> Self {
        Self {
            label,
            detail,
            kind: ActionKind::Primary,
        }
    }

    pub(crate) fn normal(label: &'a str, detail: &'a str) -> Self {
        Self {
            label,
            detail,
            kind: ActionKind::Normal,
        }
    }

    pub(crate) fn destructive(label: &'a str, detail: &'a str) -> Self {
        Self {
            label,
            detail,
            kind: ActionKind::Destructive,
        }
    }

    pub(crate) fn back(label: &'a str, detail: &'a str) -> Self {
        Self {
            label,
            detail,
            kind: ActionKind::Back,
        }
    }

    fn marker(&self) -> &'static str {
        match self.kind {
            ActionKind::Primary => "◆",
            ActionKind::Normal => "◇",
            ActionKind::Destructive => "!",
            ActionKind::Back => "←",
        }
    }

    fn is_back(&self) -> bool {
        matches!(self.kind, ActionKind::Back)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StatusKind {
    Running,
    Stopped,
    #[allow(dead_code)]
    Warning,
    Neutral,
}

pub(crate) struct StatusCard {
    title: String,
    state: String,
    detail: String,
    kind: StatusKind,
}

impl StatusCard {
    pub(crate) fn new(
        title: impl Into<String>,
        state: impl Into<String>,
        detail: impl Into<String>,
        kind: StatusKind,
    ) -> Self {
        Self {
            title: title.into(),
            state: state.into(),
            detail: detail.into(),
            kind,
        }
    }

    pub(crate) fn running(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(title, "运行中", detail, StatusKind::Running)
    }

    pub(crate) fn stopped(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(title, "未运行", detail, StatusKind::Stopped)
    }

    #[allow(dead_code)]
    pub(crate) fn warning(
        title: impl Into<String>,
        state: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(title, state, detail, StatusKind::Warning)
    }

    pub(crate) fn neutral(
        title: impl Into<String>,
        state: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(title, state, detail, StatusKind::Neutral)
    }
}

impl App {
    pub(crate) fn print_header(&self, plan: Option<&InstallPlan>) {
        let rule = "━".repeat(PANEL_WIDTH);
        wln!("{}", style(&rule).blue());
        print_centered_line(&format!("{}  v{}", APP_HEADER_TITLE, APP_VERSION), true);
        print_centered_line(APP_HEADER_SUBTITLE, false);
        print_centered_line(APP_HEADER_CREDIT, false);
        print_centered_line(APP_HEADER_DOCS, false);
        wln!("{}", style(&rule).blue());

        if let Some(plan) = plan {
            self.print_section("部署计划", "执行前请确认路径、分支和镜像策略");
            self.print_kv("目录", &plan.install_path.display().to_string());
            self.print_kv("分支", &plan.maibot_branch);
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
                self.print_section("工作区", "当前管理器配置");
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
            style("↑/↓ 选择 · Enter 执行 · Esc 返回 · Ctrl+C 中断当前系统步骤").dim()
        );
        self.print_line();
    }

    pub(crate) fn print_section(&self, title: &str, subtitle: &str) {
        wln!();
        wln!(
            "  {} {}",
            style("▌").cyan().bold(),
            style(title).cyan().bold()
        );
        if !subtitle.is_empty() {
            wln!("    {}", style(subtitle).dim());
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

    pub(crate) fn print_line(&self) {
        wln!("  {}", style("─".repeat(PANEL_WIDTH - 4)).dim());
    }

    pub(crate) fn print_hint(&self, msg: &str) {
        wln!("  {}", style(msg).dim());
    }

    pub(crate) fn print_empty_state(&self, title: &str, detail: &str) {
        wln!("  {}", style("╭─ 当前状态").blue().dim());
        wln!(
            "  {} {}",
            style("│").blue().dim(),
            style(title).white().bold()
        );
        wln!("  {} {}", style("│").blue().dim(), style(detail).dim());
        wln!(
            "  {}",
            style("╰────────────────────────────────────────────────────────")
                .blue()
                .dim()
        );
    }

    pub(crate) fn print_status_cards(&self, title: &str, cards: &[StatusCard]) {
        self.print_section(title, "");
        for card in cards {
            let marker = match card.kind {
                StatusKind::Running => style("●").green().bold(),
                StatusKind::Stopped => style("●").red().dim(),
                StatusKind::Warning => style("●").yellow().bold(),
                StatusKind::Neutral => style("●").blue().dim(),
            };
            let state = match card.kind {
                StatusKind::Running => style(&card.state).green().bold(),
                StatusKind::Stopped => style(&card.state).dim(),
                StatusKind::Warning => style(&card.state).yellow().bold(),
                StatusKind::Neutral => style(&card.state).blue().dim(),
            };
            wln!(
                "  {}  {}  {}",
                marker,
                style(pad_right(&card.title, 18)).white().bold(),
                state
            );
            if !card.detail.is_empty() {
                wln!(
                    "      {}",
                    style(truncate_display(&card.detail, PANEL_WIDTH - 10)).dim()
                );
            }
        }
        self.print_line();
    }

    pub(crate) fn select_action(&self, prompt: &str, actions: &[ActionItem<'_>]) -> Result<usize> {
        self.select_action_inner(prompt, actions, None)
    }

    pub(crate) fn select_action_timeout(
        &self,
        prompt: &str,
        actions: &[ActionItem<'_>],
        default: usize,
        timeout: Duration,
    ) -> Result<usize> {
        self.select_action_inner(prompt, actions, Some((default, timeout)))
    }

    fn select_action_inner(
        &self,
        prompt: &str,
        actions: &[ActionItem<'_>],
        timeout: Option<(usize, Duration)>,
    ) -> Result<usize> {
        if actions.is_empty() {
            return Ok(0);
        }
        let mut selected = 0_usize;
        let timeout = timeout.map(|(default, duration)| (default.min(actions.len() - 1), duration));
        let started = Instant::now();
        let back_index = actions
            .iter()
            .position(ActionItem::is_back)
            .unwrap_or(actions.len() - 1);
        enable_raw_mode().context("启用动作菜单 raw mode 失败")?;
        let mut stdout = io::stdout();
        execute!(stdout, Hide).context("隐藏终端光标失败")?;
        let _guard = ActionMenuGuard;
        execute!(stdout, SavePosition).context("保存动作菜单光标位置失败")?;

        loop {
            execute!(
                io::stdout(),
                RestorePosition,
                Clear(ClearType::FromCursorDown)
            )
            .context("刷新动作菜单失败")?;
            let timeout_hint = timeout.map(|(default, duration)| {
                let elapsed = started.elapsed();
                let remaining = duration.saturating_sub(elapsed);
                (
                    default,
                    remaining
                        .as_secs()
                        .saturating_add(if remaining.subsec_millis() > 0 { 1 } else { 0 }),
                )
            });
            draw_action_menu(prompt, actions, selected, timeout_hint);
            io::stdout().flush()?;

            let event = if let Some((default, duration)) = timeout {
                let remaining = duration.saturating_sub(started.elapsed());
                if remaining.is_zero() || !poll(remaining).context("等待动作菜单按键失败")?
                {
                    execute!(
                        io::stdout(),
                        RestorePosition,
                        Clear(ClearType::FromCursorDown)
                    )
                    .context("清理动作菜单失败")?;
                    io::stdout().flush()?;
                    return Ok(default);
                }
                read().context("读取动作菜单按键失败")?
            } else {
                read().context("读取动作菜单按键失败")?
            };

            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        restore_terminal_state();
                        println!("\r\n操作已被用户中断 (Ctrl+C)");
                        std::process::exit(130);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = if selected == 0 {
                            actions.len() - 1
                        } else {
                            selected - 1
                        };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected = (selected + 1) % actions.len();
                    }
                    KeyCode::Enter => {
                        execute!(
                            io::stdout(),
                            RestorePosition,
                            Clear(ClearType::FromCursorDown)
                        )
                        .context("清理动作菜单失败")?;
                        io::stdout().flush()?;
                        return Ok(selected);
                    }
                    KeyCode::Esc => {
                        execute!(
                            io::stdout(),
                            RestorePosition,
                            Clear(ClearType::FromCursorDown)
                        )
                        .context("清理动作菜单失败")?;
                        io::stdout().flush()?;
                        return Ok(back_index);
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    pub(crate) fn print_command_start(&self, command: &str) {
        wln!();
        wln!(
            "  {} {}",
            style("RUN").cyan().bold(),
            style("正在执行系统步骤").white().bold()
        );
        wln!(
            "  {}",
            style(truncate_display(command, PANEL_WIDTH - 4)).dim()
        );
        self.print_line();
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

struct ActionMenuGuard;

impl Drop for ActionMenuGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, Show);
    }
}

fn draw_action_menu(
    prompt: &str,
    actions: &[ActionItem<'_>],
    selected: usize,
    timeout_hint: Option<(usize, u64)>,
) {
    wln!();
    wln!(
        "  {} {}",
        style("▌").cyan().bold(),
        style(prompt).cyan().bold()
    );
    wln!("  {}", style("─".repeat(PANEL_WIDTH - 4)).dim());
    for (index, action) in actions.iter().enumerate() {
        let active = index == selected;
        let cursor = if active { "▸" } else { " " };
        let title = format!("{} {}", action.marker(), pad_right(action.label, 16));
        let detail = truncate_display(action.detail, PANEL_WIDTH - 30);
        if active {
            wln!(
                "  {} {} {}",
                style(cursor).cyan().bold(),
                style(&title).cyan().bold(),
                style(detail).white()
            );
        } else {
            wln!(
                "  {} {} {}",
                style(cursor).dim(),
                style(&title).white(),
                style(detail).dim()
            );
        }
    }
    let footer = if let Some((default, remaining)) = timeout_hint {
        format!(
            "左下角提示  ↑/↓ 或 j/k 移动  Enter 执行  Esc 返回  ·  {remaining}s 后默认：{}",
            actions[default].label
        )
    } else {
        "左下角提示  ↑/↓ 或 j/k 移动  Enter 执行  Esc 返回".to_string()
    };
    wln!("  {}", style(footer).blue().dim());
}

fn print_centered_line(text: &str, primary: bool) {
    let text = truncate_display(text, PANEL_WIDTH);
    let width = display_width(&text);
    let left = (PANEL_WIDTH.saturating_sub(width)) / 2;
    let text = if primary {
        style(text).cyan().bold()
    } else {
        style(text).dim()
    };
    wln!("{}{}", " ".repeat(left), text);
}

fn pad_right(input: &str, width: usize) -> String {
    let len = display_width(input);
    if len >= width {
        truncate_display(input, width)
    } else {
        format!("{input}{}", " ".repeat(width - len))
    }
}

fn truncate_display(input: &str, max_width: usize) -> String {
    if display_width(input) <= max_width {
        return input.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    let marker_width = 1;
    let limit = max_width.saturating_sub(marker_width);
    for ch in input.chars() {
        let w = if ch.is_ascii() || ch.is_control() {
            1
        } else {
            2
        };
        if used + w > limit {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}
