use crate::{
    app::App,
    model::*,
    terminal::restore_terminal_state,
    utils::{display_width, pad_left},
};
use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers, poll, read},
    execute, queue,
    style::Print,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use dialoguer::Input;
use dialoguer::console::{StyledObject, style};
use std::{
    io::{self, Write},
    sync::atomic::{AtomicU16, Ordering},
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

const PANEL_WIDTH: usize = 78;
const KEY_WIDTH: usize = 14;

fn content_width() -> usize {
    let (term_width, _) = size().unwrap_or((80, 24));
    usize::from(term_width).saturating_sub(4).clamp(1, 136)
}

macro_rules! wln {
    () => {{
        print!("\r\n");
        record_printed_line();
    }};
    ($($arg:tt)*) => {{
        print!("{}\r\n", format_args!($($arg)*));
        record_printed_line();
    }};
}

static PRINTED_ROWS: AtomicU16 = AtomicU16::new(0);

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
        let rule = "━".repeat(content_width());
        wln!("{}", style(&rule).cyan().bright().bold());
        print_centered_line(&format!("{}  v{}", APP_HEADER_TITLE, APP_VERSION), true);
        print_centered_line(APP_HEADER_SUBTITLE, false);
        print_centered_line(APP_HEADER_CREDIT, false);
        print_centered_line(APP_HEADER_DOCS, false);
        wln!("{}", style(&rule).cyan().bright().bold());

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
        }
    }

    pub(crate) fn clear(&self) {
        print!("\x1B[2J\x1B[1;1H");
        reset_printed_rows();
        let _ = io::stdout().flush();
    }

    pub(crate) fn print_section(&self, title: &str, subtitle: &str) {
        wln!();
        wln!(
            "  {} {}",
            style("▌").green().bright().bold(),
            style(title).cyan().bright().bold()
        );
        if !subtitle.is_empty() {
            wln!("    {}", style(subtitle).white().bright());
        }
        self.print_line();
    }

    pub(crate) fn print_kv(&self, key: &str, value: &str) {
        wln!(
            "  {} {}",
            style(pad_left(key, KEY_WIDTH)).magenta().bright().bold(),
            style(value).white().bright()
        );
    }

    pub(crate) fn print_line(&self) {
        wln!(
            "  {}",
            style("─".repeat(content_width().saturating_sub(2)))
                .blue()
                .bright()
        );
    }

    pub(crate) fn print_hint(&self, msg: &str) {
        wln!("  {}", style(msg).yellow().bright());
    }

    pub(crate) fn print_empty_state(&self, title: &str, detail: &str) {
        let rule = "─".repeat(content_width().saturating_sub(8).max(12));
        wln!("  {}", style("╭─ 当前状态").cyan().bright().bold());
        wln!(
            "  {} {}",
            style("│").cyan().bright(),
            style(title).yellow().bright().bold()
        );
        wln!(
            "  {} {}",
            style("│").cyan().bright(),
            style(detail).white().bright()
        );
        wln!("  {}", style(format!("╰{rule}")).cyan().bright());
    }

    pub(crate) fn print_status_cards(&self, title: &str, cards: &[StatusCard]) {
        let detail_limit = content_width().saturating_sub(34).max(16);
        wln!(
            "  {} {}",
            style("▌").green().bright().bold(),
            style(title).cyan().bright().bold()
        );
        self.print_line();
        for card in cards {
            let marker = match card.kind {
                StatusKind::Running => style("●").green().bright().bold(),
                StatusKind::Stopped => style("●").red().bright().bold(),
                StatusKind::Warning => style("●").yellow().bright().bold(),
                StatusKind::Neutral => style("●").magenta().bright().bold(),
            };
            let state = match card.kind {
                StatusKind::Running => style(&card.state).green().bright().bold(),
                StatusKind::Stopped => style(&card.state).red().bright(),
                StatusKind::Warning => style(&card.state).yellow().bright().bold(),
                StatusKind::Neutral => style(&card.state).magenta().bright(),
            };
            let detail = if card.detail.is_empty() {
                String::new()
            } else {
                format!(
                    "  {} {}",
                    style("·").cyan().bright(),
                    style(truncate_display(&card.detail, detail_limit))
                        .white()
                        .bright()
                )
            };
            wln!(
                "  {}  {}  {}{}",
                marker,
                style(pad_right(&card.title, 18)).white().bright().bold(),
                state,
                detail
            );
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
        stdout.flush().context("刷新动作菜单前置内容失败")?;
        let menu_origin = (0, printed_rows());
        let mut last_drawn_rows = 0_usize;
        let mut last_start_row = menu_origin.1;

        loop {
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
            let draw_state = draw_action_menu(
                &mut stdout,
                ActionMenuDrawInput {
                    origin: menu_origin,
                    previous_start_row: last_start_row,
                    last_drawn_rows,
                    prompt,
                    actions,
                    selected,
                    timeout_hint,
                },
            )?;
            last_start_row = draw_state.start_row;
            last_drawn_rows = draw_state.rows;
            stdout.flush()?;

            let event = if let Some((default, duration)) = timeout {
                let remaining = duration.saturating_sub(started.elapsed());
                if remaining.is_zero() || !poll(remaining).context("等待动作菜单按键失败")?
                {
                    clear_action_menu(&mut stdout, last_start_row, last_drawn_rows)
                        .context("清理动作菜单失败")?;
                    set_printed_rows(menu_origin.1);
                    stdout.flush()?;
                    return Ok(default);
                }
                read().context("读取动作菜单按键失败")?
            } else {
                read().context("读取动作菜单按键失败")?
            };

            match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
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
                        clear_action_menu(&mut stdout, last_start_row, last_drawn_rows)
                            .context("清理动作菜单失败")?;
                        set_printed_rows(menu_origin.1);
                        stdout.flush()?;
                        return Ok(selected);
                    }
                    KeyCode::Esc => {
                        clear_action_menu(&mut stdout, last_start_row, last_drawn_rows)
                            .context("清理动作菜单失败")?;
                        set_printed_rows(menu_origin.1);
                        stdout.flush()?;
                        return Ok(back_index);
                    }
                    _ => {}
                },
                Event::Key(_) => {}
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    pub(crate) fn print_command_start(&self, command: &str) {
        wln!();
        wln!(
            "  {} {}",
            style("RUN").green().bright().bold(),
            style("正在执行系统步骤").yellow().bright().bold()
        );
        wln!(
            "  {}",
            style(truncate_display(command, content_width().saturating_sub(2)))
                .white()
                .bright()
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

    pub(crate) fn dashboard_event_loop<F>(
        &self,
        state: &mut DashboardState,
        mut render: F,
    ) -> Result<DashboardEvent>
    where
        F: FnMut(&DashboardState) -> Result<DashboardView>,
    {
        enable_raw_mode().context("启用现代 TUI raw mode 失败")?;
        let mut stdout = io::stdout();
        execute!(stdout, Hide).context("隐藏现代 TUI 光标失败")?;
        let _guard = ActionMenuGuard;

        loop {
            let view = render(state)?;
            self.draw_dashboard(&view)?;

            let event = read().context("读取现代 TUI 按键失败")?;
            match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(DashboardEvent::Exit);
                    }
                    KeyCode::Left => {
                        if matches!(state.focus, DashboardFocus::Tabs) {
                            return Ok(DashboardEvent::PrevTab);
                        }
                        return Ok(DashboardEvent::AdjustLeft);
                    }
                    KeyCode::Right => {
                        if matches!(state.focus, DashboardFocus::Tabs) {
                            return Ok(DashboardEvent::NextTab);
                        }
                        return Ok(DashboardEvent::AdjustRight);
                    }
                    KeyCode::BackTab => {
                        if matches!(state.focus, DashboardFocus::List)
                            && state.active_tab == DashboardTab::Core
                        {
                            return Ok(DashboardEvent::MoveUp);
                        }
                        return Ok(DashboardEvent::ToggleFocus);
                    }
                    KeyCode::Tab => {
                        if matches!(state.focus, DashboardFocus::List)
                            && state.active_tab == DashboardTab::Core
                        {
                            return Ok(DashboardEvent::MoveDown);
                        }
                        return Ok(DashboardEvent::ToggleFocus);
                    }
                    KeyCode::Up | KeyCode::Char('k') => return Ok(DashboardEvent::MoveUp),
                    KeyCode::Down | KeyCode::Char('j') => return Ok(DashboardEvent::MoveDown),
                    KeyCode::Enter => return Ok(DashboardEvent::Activate),
                    KeyCode::Char('/') => return Ok(DashboardEvent::EditSearch),
                    KeyCode::Esc => {
                        if matches!(state.focus, DashboardFocus::List) {
                            state.focus = DashboardFocus::Tabs;
                        }
                    }
                    KeyCode::Backspace => return Ok(DashboardEvent::ClearSearch),
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    pub(crate) fn prompt_dashboard_search(&self, title: &str, current: &str) -> Result<String> {
        self.with_prompt_mode(|| {
            let mut input = Input::with_theme(&self.theme);
            input = input.with_prompt(title).allow_empty(true);
            if !current.is_empty() {
                input = input.with_initial_text(current.to_string());
            }
            input.interact_text().map_err(Into::into)
        })
    }

    pub(crate) fn draw_dashboard(&self, view: &DashboardView) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.draw_tabs(view);
        self.draw_dashboard_intro(view);
        self.draw_dashboard_body(view);
        self.draw_dashboard_status_bar(view);
        Ok(())
    }

    fn draw_tabs(&self, view: &DashboardView) {
        let max_width = content_width().saturating_sub(2).max(16);
        let mut used = 0_usize;
        print!("  ");
        used += 2;
        for tab in DashboardTab::ALL {
            let label = format!("{} {}", tab.icon(), tab.label());
            let rendered = format!(" {label} ");
            let rendered_width = display_width(&rendered);
            if used > 2 && used + rendered_width + 1 > max_width {
                print!("\r\n  ");
                record_printed_line();
                used = 2;
            }
            if tab == view.active_tab {
                print!(
                    "{} ",
                    if matches!(view.focus, DashboardFocus::Tabs) {
                        style(rendered.clone()).black().on_magenta().bright().bold()
                    } else {
                        style(rendered.clone()).black().on_blue().bright().bold()
                    }
                );
            } else {
                print!("{} ", style(rendered).cyan().bright());
            }
            used += rendered_width + 1;
        }
        print!("\r\n");
        record_printed_line();
        let hint = match view.focus {
            DashboardFocus::Tabs => "导航层: ←/→ 切换标签  Tab 进入工作区  Ctrl+C 退出",
            DashboardFocus::List => &view.context_hint,
        };
        wln!(
            "  {}",
            style(pad_right(hint, content_width().saturating_sub(2)))
                .black()
                .on_blue()
                .bold()
        );
        self.print_line();
    }

    fn draw_dashboard_intro(&self, view: &DashboardView) {
        wln!(
            "  {} {}",
            style("▌").green().bright().bold(),
            style(&view.page_title).cyan().bright().bold()
        );
        if !view.page_subtitle.is_empty() {
            wln!("    {}", style(&view.page_subtitle).white().dim());
        }
        let focus = match view.focus {
            DashboardFocus::Tabs => "焦点: 顶部标签",
            DashboardFocus::List => "焦点: 工作区",
        };
        let search = if view.search_query.is_empty() {
            "筛选: 全部".to_string()
        } else {
            format!(
                "筛选: {}",
                truncate_display(&view.search_query, content_width().saturating_sub(26))
            )
        };
        let scope = if view.cards.is_empty() {
            "项目: 0 / 0".to_string()
        } else {
            format!("项目: {} / {}", view.selected + 1, view.cards.len())
        };
        let layout = if content_width() < 84 {
            "布局: 紧凑堆叠"
        } else {
            "布局: 双栏面板"
        };
        wln!(
            "    {}  {}  {}",
            style(focus).yellow().bright(),
            style(search).magenta().bright(),
            style(scope).cyan().bright()
        );
        wln!("    {}", style(layout).white().dim());
        self.print_line();
    }

    fn draw_dashboard_body(&self, view: &DashboardView) {
        let (term_width, _) = size().unwrap_or((80, 24));
        let content_width = usize::from(term_width).saturating_sub(7).clamp(1, 136);
        if content_width < 84 {
            let panel_width = content_width;
            let left = self.build_left_panel_lines(view, panel_width);
            let right = self.build_right_panel_lines(view, panel_width);

            wln!("  {}", style("导航面板").cyan().bright().bold());
            for line in left {
                wln!("  {}", style_left_panel_line(view, &line, panel_width));
            }
            self.print_line();
            wln!("  {}", style("详情面板").magenta().bright().bold());
            for line in right {
                wln!("  {}", style_right_panel_line(view, &line, panel_width));
            }
            self.print_line();
            return;
        }
        let left_width = match view.active_tab {
            DashboardTab::Core => (content_width * 29 / 100).clamp(24, 32),
            DashboardTab::Deploy => (content_width * 33 / 100).clamp(24, 36),
            _ => (content_width * 37 / 100).clamp(26, 42),
        };
        let right_width = content_width
            .saturating_sub(left_width)
            .saturating_sub(3)
            .max(28);
        let left = self.build_left_panel_lines(view, left_width);
        let right = self.build_right_panel_lines(view, right_width);
        let height = left.len().max(right.len());

        for idx in 0..height {
            let left_line = left
                .get(idx)
                .cloned()
                .unwrap_or_else(|| " ".repeat(left_width));
            let right_line = right
                .get(idx)
                .cloned()
                .unwrap_or_else(|| " ".repeat(right_width));
            wln!(
                "  {} {} {}",
                style_left_panel_line(view, &left_line, left_width),
                style("|").blue().bright(),
                style_right_panel_line(view, &right_line, right_width)
            );
        }
        self.print_line();
    }

    fn build_left_panel_lines(&self, view: &DashboardView, width: usize) -> Vec<String> {
        match view.active_tab {
            DashboardTab::Overview => return build_overview_left_panel_lines(view, width),
            DashboardTab::Core => return build_core_left_panel_lines(view, width),
            DashboardTab::Deploy => return build_deploy_left_panel_lines(view, width),
            DashboardTab::Plugins => return build_plugins_left_panel_lines(view, width),
            _ => {}
        }

        let mut lines = Vec::new();
        lines.push(frame_top(&view.list_title, width));
        lines.push(frame_line(&view.list_subtitle, width));
        lines.push(frame_rule(width));
        let search = if view.search_query.is_empty() {
            "/ 搜索服务、步骤或插件".to_string()
        } else {
            format!(
                "/ {}",
                truncate_display(&view.search_query, width.saturating_sub(4))
            )
        };
        lines.push(frame_line(&search, width));
        lines.push(frame_rule(width));

        if view.cards.is_empty() {
            lines.push(frame_line(&view.empty_title, width));
            lines.push(frame_line(&view.empty_detail, width));
            lines.push(frame_bottom(width));
            return lines;
        }

        for (idx, card) in view.cards.iter().enumerate() {
            let active = idx == view.selected;
            let title = truncate_display(
                &format!("{} {}", card.icon, card.title),
                width.saturating_sub(3),
            );
            let badge = truncate_display(
                &format!("{} {}", status_glyph(card.kind), card.badge),
                width.saturating_sub(3),
            );
            let subtitle = truncate_display(&card.subtitle, width.saturating_sub(3));
            let detail = truncate_display(&card.detail, width.saturating_sub(3));
            lines.push(format!("{} {}", if active { ">" } else { " " }, title));
            lines.push(format!("  {}", badge));
            lines.push(format!("  {}", subtitle));
            if active {
                lines.push(format!("  {}", detail));
            }
            if idx + 1 != view.cards.len() {
                lines.push(frame_rule(width));
            }
        }
        lines.push(frame_bottom(width));
        lines
    }

    fn build_right_panel_lines(&self, view: &DashboardView, width: usize) -> Vec<String> {
        match view.active_tab {
            DashboardTab::Overview => return build_overview_right_panel_lines(view, width),
            DashboardTab::Core => return build_core_right_panel_lines(view, width),
            DashboardTab::Deploy => return build_deploy_right_panel_lines(view, width),
            DashboardTab::Plugins => return build_plugins_right_panel_lines(view, width),
            _ => {}
        }

        let mut lines = vec![
            frame_top(&view.detail_title, width),
            frame_line(&view.detail_subtitle, width),
            frame_rule(width),
            frame_line(":: 状态摘要", width),
        ];
        for line in &view.detail_lines {
            lines.push(frame_line(line, width));
        }
        if !view.action_lines.is_empty() {
            lines.push(frame_rule(width));
            lines.push(frame_line(":: 下一步", width));
            for action in &view.action_lines {
                lines.push(frame_line(&format!("◇ {action}"), width));
            }
        }
        lines.push(frame_bottom(width));
        lines
    }

    fn draw_dashboard_status_bar(&self, view: &DashboardView) {
        let total = content_width().saturating_sub(2);
        let left = truncate_display(
            &format!("{} {}", view.active_tab.icon(), view.status_message),
            total / 2,
        );
        let right = truncate_display(&view.context_hint, total / 2);
        let gap = total.saturating_sub(display_width(&left) + display_width(&right));
        wln!(
            "  {}{}{}",
            style(left).black().on_green().bold(),
            style(" ".repeat(gap.max(1))).on_blue(),
            style(right).white().on_blue().bold()
        );
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

struct ActionMenuDrawState {
    start_row: u16,
    rows: usize,
}

fn record_printed_line() {
    PRINTED_ROWS
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |rows| {
            Some(rows.saturating_add(1))
        })
        .ok();
}

fn reset_printed_rows() {
    PRINTED_ROWS.store(0, Ordering::Relaxed);
}

fn set_printed_rows(rows: u16) {
    PRINTED_ROWS.store(rows, Ordering::Relaxed);
}

fn printed_rows() -> u16 {
    PRINTED_ROWS.load(Ordering::Relaxed)
}

fn style_left_panel_line(view: &DashboardView, line: &str, width: usize) -> StyledObject<String> {
    let padded = pad_right(line, width);
    if is_active_panel_line(view, line) {
        match view.active_tab {
            DashboardTab::Core => style(padded).black().on_green().bright().bold(),
            DashboardTab::Deploy => style(padded).black().on_yellow().bright().bold(),
            _ => style(padded).black().on_cyan().bright().bold(),
        }
    } else if line.starts_with('+') {
        style(padded).cyan().bright().bold()
    } else if line.starts_with("| /") {
        style(padded).yellow().bright()
    } else if line.trim().is_empty() || line.starts_with("  ") {
        style(padded).white().dim()
    } else {
        style(padded).white().bright()
    }
}

fn style_right_panel_line(view: &DashboardView, line: &str, width: usize) -> StyledObject<String> {
    let padded = pad_right(line, width);
    if let Some(section) = line.strip_prefix("| :: ") {
        let rendered = format!("| {section}");
        match view.active_tab {
            DashboardTab::Core => style(pad_right(&rendered, width)).green().bright().bold(),
            DashboardTab::Deploy => style(pad_right(&rendered, width)).yellow().bright().bold(),
            _ => style(pad_right(&rendered, width)).magenta().bright().bold(),
        }
    } else if line.starts_with('+') {
        style(padded).cyan().bright().bold()
    } else if line.trim().is_empty() {
        style(padded).white().dim()
    } else if line.starts_with("| ✓ ") {
        style(padded).black().on_green().bright().bold()
    } else if line.starts_with("| ○ ") {
        style(padded).yellow().bright()
    } else if line.starts_with("| · ") {
        style(padded).white().dim()
    } else {
        style(padded).white().bright()
    }
}

fn is_active_panel_line(view: &DashboardView, line: &str) -> bool {
    match view.active_tab {
        DashboardTab::Overview => line.starts_with("| >"),
        DashboardTab::Core => line.starts_with("> "),
        DashboardTab::Deploy => line.starts_with("> "),
        _ => line.starts_with("> "),
    }
}

fn build_overview_left_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(frame_top("服务卡片", width));
    lines.push(frame_line("运行态 / 摘要 / 可进入模块", width));
    lines.push(frame_rule(width));
    let search = if view.search_query.is_empty() {
        "/ 搜索服务、协议端或插件".to_string()
    } else {
        format!(
            "/ {}",
            truncate_display(&view.search_query, width.saturating_sub(4))
        )
    };
    lines.push(frame_line(&search, width));
    lines.push(frame_rule(width));

    if view.cards.is_empty() {
        lines.push(frame_line(&view.empty_title, width));
        lines.push(frame_line(&view.empty_detail, width));
        lines.push(frame_bottom(width));
        return lines;
    }

    for (idx, card) in view.cards.iter().enumerate() {
        let active = idx == view.selected;
        let prefix = if active { ">" } else { " " };
        let title = truncate_display(
            &format!("{prefix} {} {}", card.icon, card.title),
            width.saturating_sub(2),
        );
        let badge = truncate_display(
            &format!("{} {}", status_glyph(card.kind), card.badge),
            width.saturating_sub(4),
        );
        lines.push(frame_line(&title, width));
        lines.push(frame_line(&format!("状态  {badge}"), width));
        lines.push(frame_line(
            &format!(
                "摘要  {}",
                truncate_display(&card.subtitle, width.saturating_sub(8))
            ),
            width,
        ));
        if active {
            lines.push(frame_line(
                &format!(
                    "入口  {}",
                    truncate_display(&card.detail, width.saturating_sub(8))
                ),
                width,
            ));
        }
        if idx + 1 != view.cards.len() {
            lines.push(frame_rule(width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

fn build_core_left_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(frame_top("核心控制块", width));
    lines.push(frame_line("启动 / 停止 / 控制台 / 日志", width));
    lines.push(frame_rule(width));
    let search = if view.search_query.is_empty() {
        "/ 搜索核心动作".to_string()
    } else {
        format!(
            "/ {}",
            truncate_display(&view.search_query, width.saturating_sub(4))
        )
    };
    lines.push(frame_line(&search, width));
    lines.push(frame_rule(width));

    for (idx, card) in view.cards.iter().enumerate() {
        let active = idx == view.selected;
        let prefix = if active { "> " } else { "  " };
        let title = truncate_display(
            &format!("{} {}", card.icon, card.title),
            width.saturating_sub(3),
        );
        let subtitle = truncate_display(&card.subtitle, width.saturating_sub(3));
        let badge = truncate_display(
            &format!("{} {}", status_glyph(card.kind), card.badge),
            width.saturating_sub(3),
        );
        let detail = truncate_display(&card.detail, width.saturating_sub(3));
        lines.push(format!("{prefix}{title}"));
        lines.push(format!("  {badge}"));
        lines.push(format!("  {subtitle}"));
        if active {
            lines.push(format!("  {detail}"));
        }
        if idx + 1 != view.cards.len() {
            lines.push(frame_rule(width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

fn build_deploy_left_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(frame_top("步骤指示器", width));
    lines.push(frame_line("目录 -> 分支 -> 模式 -> 环境 -> 镜像", width));
    lines.push(frame_rule(width));
    let search = if view.search_query.is_empty() {
        "/ 搜索步骤或动作".to_string()
    } else {
        format!(
            "/ {}",
            truncate_display(&view.search_query, width.saturating_sub(4))
        )
    };
    lines.push(frame_line(&search, width));
    lines.push(frame_rule(width));

    for (idx, card) in view.cards.iter().enumerate() {
        let active = idx == view.selected;
        let selector = if active { ">" } else { " " };
        let step = match card.id {
            "deploy-start" => "GO".to_string(),
            "deploy-reset" => "DF".to_string(),
            "deploy-back" => "BK".to_string(),
            _ => format!("{:02}", idx + 1),
        };
        let title = truncate_display(&card.title, width.saturating_sub(7));
        let subtitle = truncate_display(&card.subtitle, width.saturating_sub(6));
        let badge = truncate_display(
            &format!("{} {}", status_glyph(card.kind), card.badge),
            width.saturating_sub(4),
        );
        let rail = if matches!(card.id, "deploy-start" | "deploy-reset" | "deploy-back") {
            "◆"
        } else {
            "│"
        };
        lines.push(format!("{selector} {rail} [{step}] {title}"));
        lines.push(format!("  当前值  {subtitle}"));
        lines.push(format!("  交互项  {badge}"));
        if active {
            let detail = truncate_display(&card.detail, width.saturating_sub(3));
            lines.push(format!("  说明    {detail}"));
        }
        if idx + 1 != view.cards.len() {
            lines.push(frame_rule(width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

fn build_core_right_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let status_line = view
        .detail_lines
        .iter()
        .find(|line| line.contains("当前状态"))
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "{} {}",
                status_glyph(StatusKind::Neutral),
                view.detail_subtitle
            )
        });
    lines.push(frame_top("核心状态面板", width));
    lines.push(frame_line(&status_line, width));
    lines.push(frame_rule(width));
    lines.push(frame_line(":: 运行快照", width));
    lines.push(frame_line(&view.detail_title, width));
    lines.push(frame_line(&view.detail_subtitle, width));
    for line in view.detail_lines.iter().take(5) {
        lines.push(frame_line(line, width));
    }
    lines.push(frame_rule(width));
    lines.push(frame_line(":: 动作块", width));
    lines.push(frame_line("Tab 在顶部标签与动作区间切换焦点", width));
    for (idx, card) in view.cards.iter().enumerate() {
        let selected = idx == view.selected;
        let lead = if selected { "▣" } else { "□" };
        let badge = if selected {
            "当前焦点"
        } else {
            &card.badge
        };
        let block = format!(
            "{lead} {} {}  {}",
            card.icon,
            card.title,
            status_glyph(card.kind)
        );
        lines.push(frame_line(&block, width));
        lines.push(frame_line(
            &format!(
                "   {}",
                truncate_display(&card.subtitle, width.saturating_sub(5))
            ),
            width,
        ));
        lines.push(frame_line(
            &format!("   {}", truncate_display(badge, width.saturating_sub(5))),
            width,
        ));
    }
    if !view.action_lines.is_empty() {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 执行提示", width));
        for action in &view.action_lines {
            lines.push(frame_line(&format!("◇ {action}"), width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

fn build_overview_right_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let status = view
        .cards
        .get(view.selected)
        .map(|card| format!("{} {}  {}", card.icon, card.title, status_glyph(card.kind)))
        .unwrap_or_else(|| view.detail_title.clone());
    lines.push(frame_top("服务详情", width));
    lines.push(frame_line(&status, width));
    lines.push(frame_rule(width));
    lines.push(frame_line(":: 运行状态", width));
    lines.push(frame_line(&view.detail_subtitle, width));
    for line in view.detail_lines.iter().take(5) {
        lines.push(frame_line(line, width));
    }
    if view.detail_lines.len() > 5 {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 日志与环境", width));
        for line in view.detail_lines.iter().skip(5) {
            lines.push(frame_line(line, width));
        }
    }
    if !view.action_lines.is_empty() {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 快捷动作", width));
        for action in &view.action_lines {
            lines.push(frame_line(&format!("◇ {action}"), width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

fn build_deploy_right_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let selected = view.cards.get(view.selected);
    let is_action = selected
        .is_some_and(|card| matches!(card.id, "deploy-start" | "deploy-reset" | "deploy-back"));
    lines.push(frame_top("步骤编辑器", width));
    lines.push(frame_line(&view.detail_title, width));
    lines.push(frame_rule(width));
    if is_action {
        lines.push(frame_line(":: 当前动作", width));
        lines.push(frame_line(
            &format!("目标: {}", view.detail_subtitle),
            width,
        ));
    } else {
        lines.push(frame_line(":: 当前值", width));
        lines.push(frame_line(
            &format!("已选: {}", view.detail_subtitle),
            width,
        ));
    }
    for line in view.detail_lines.iter().take(4) {
        lines.push(frame_line(line, width));
    }
    if !is_action {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 可选值", width));
        if view.detail_choices.is_empty() {
            lines.push(frame_line("当前项没有额外候选值。", width));
        } else {
            for choice in &view.detail_choices {
                let marker = if choice.active { "✓" } else { "○" };
                lines.push(frame_line(
                    &format!(
                        "{marker} {}",
                        truncate_display(&choice.label, width.saturating_sub(8))
                    ),
                    width,
                ));
                lines.push(frame_line(
                    &format!(
                        "· {}",
                        truncate_display(&choice.detail, width.saturating_sub(8))
                    ),
                    width,
                ));
            }
        }
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 可选操作", width));
        if let Some(card) = selected {
            lines.push(frame_line(&format!("主操作: {}", card.badge), width));
            if card.badge.contains("输入") {
                lines.push(frame_line("Enter 打开路径输入框", width));
            } else {
                lines.push(frame_line("← / → 切换当前配置值", width));
            }
            lines.push(frame_line(
                &format!(
                    "高亮步骤: {}",
                    truncate_display(&card.title, width.saturating_sub(10))
                ),
                width,
            ));
        }
    }
    if view.detail_lines.len() > 4 {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 配置说明", width));
        for line in view.detail_lines.iter().skip(4) {
            lines.push(frame_line(line, width));
        }
    }
    if !view.action_lines.is_empty() {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 执行队列", width));
        for action in &view.action_lines {
            lines.push(frame_line(&format!("◇ {action}"), width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

fn build_plugins_left_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(frame_top("插件卡片", width));
    lines.push(frame_line("名称 / 状态 / 摘要", width));
    lines.push(frame_rule(width));
    let search = if view.search_query.is_empty() {
        "/ 搜索插件名称或作者".to_string()
    } else {
        format!(
            "/ {}",
            truncate_display(&view.search_query, width.saturating_sub(4))
        )
    };
    lines.push(frame_line(&search, width));
    lines.push(frame_rule(width));

    if view.cards.is_empty() {
        lines.push(frame_line(&view.empty_title, width));
        lines.push(frame_line(&view.empty_detail, width));
        lines.push(frame_bottom(width));
        return lines;
    }

    for (idx, card) in view.cards.iter().enumerate() {
        let active = idx == view.selected;
        let title = truncate_display(
            &format!("{} {}", card.icon, card.title),
            width.saturating_sub(3),
        );
        let badge = truncate_display(
            &format!("{} {}", status_glyph(card.kind), card.badge),
            width.saturating_sub(3),
        );
        let subtitle = truncate_display(&card.subtitle, width.saturating_sub(3));
        let detail = truncate_display(&card.detail, width.saturating_sub(3));
        lines.push(format!("{} {}", if active { ">" } else { " " }, title));
        lines.push(format!("  {}", badge));
        lines.push(format!("  {}", subtitle));
        if active {
            lines.push(format!("  {}", detail));
        }
        if idx + 1 != view.cards.len() {
            lines.push(frame_rule(width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

fn build_plugins_right_panel_lines(view: &DashboardView, width: usize) -> Vec<String> {
    let mut lines = vec![
        frame_top("插件详情", width),
        frame_line(&view.detail_title, width),
        frame_rule(width),
        frame_line(":: 插件摘要", width),
        frame_line(&view.detail_subtitle, width),
    ];
    for line in view.detail_lines.iter().take(5) {
        lines.push(frame_line(line, width));
    }
    if view.detail_lines.len() > 5 {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 维护信息", width));
        for line in view.detail_lines.iter().skip(5) {
            lines.push(frame_line(line, width));
        }
    }
    if !view.action_lines.is_empty() {
        lines.push(frame_rule(width));
        lines.push(frame_line(":: 可执行项", width));
        for action in &view.action_lines {
            lines.push(frame_line(&format!("◇ {action}"), width));
        }
    }
    lines.push(frame_bottom(width));
    lines
}

struct ActionMenuDrawInput<'a, 'b> {
    origin: (u16, u16),
    previous_start_row: u16,
    last_drawn_rows: usize,
    prompt: &'a str,
    actions: &'a [ActionItem<'b>],
    selected: usize,
    timeout_hint: Option<(usize, u64)>,
}

fn draw_action_menu(
    stdout: &mut io::Stdout,
    input: ActionMenuDrawInput<'_, '_>,
) -> Result<ActionMenuDrawState> {
    let ActionMenuDrawInput {
        origin,
        previous_start_row,
        last_drawn_rows,
        prompt,
        actions,
        selected,
        timeout_hint,
    } = input;
    let (term_width, term_height) = size().unwrap_or((80, 24));
    let term_height = term_height.max(1);
    let term_width = term_width.max(20);
    let max_width = usize::from(term_width).min(PANEL_WIDTH);
    let content_width = max_width.saturating_sub(4).max(10);
    let detail_width = max_width.saturating_sub(30).max(8);
    let mut lines = Vec::with_capacity(actions.len() + 4);

    lines.push(String::new());
    lines.push(format!(
        "  {} {}",
        style("▌").green().bright().bold(),
        style(truncate_display(prompt, content_width))
            .cyan()
            .bright()
            .bold()
    ));
    lines.push(format!(
        "  {}",
        style("─".repeat(content_width)).blue().bright()
    ));
    for (index, action) in actions.iter().enumerate() {
        let active = index == selected;
        let cursor = if active { "▸" } else { " " };
        let title = format!("{} {}", action.marker(), pad_right(action.label, 16));
        let detail = truncate_display(action.detail, detail_width);
        if active {
            lines.push(format!(
                "  {} {} {}",
                style(cursor).green().bright().bold(),
                style(&title).yellow().bright().bold(),
                style(detail).white().bright().bold()
            ));
        } else {
            lines.push(format!(
                "  {} {} {}",
                style(cursor).blue().bright(),
                style(&title).cyan().bright(),
                style(detail).white().bright()
            ));
        }
    }
    let footer = if let Some((default, remaining)) = timeout_hint {
        format!(
            "↑/↓ 或 j/k 移动  Enter 执行  Esc 返回  ·  {remaining}s 后默认：{}",
            actions[default].label
        )
    } else {
        "↑/↓ 或 j/k 移动  Enter 执行  Esc 返回".to_string()
    };
    lines.push(format!(
        "  {}",
        style(truncate_display(&footer, content_width))
            .blue()
            .bright()
            .bold()
    ));

    let rows_needed = lines.len().max(last_drawn_rows).max(1);
    let available_from_origin = usize::from(term_height.saturating_sub(origin.1));
    let start_row = if rows_needed <= available_from_origin {
        origin.1
    } else {
        term_height.saturating_sub(rows_needed as u16)
    };
    let clear_rows = rows_needed.min(usize::from(term_height));
    let clear_start_row = previous_start_row.min(start_row);
    let clear_end_row = previous_start_row
        .saturating_add(last_drawn_rows as u16)
        .max(start_row.saturating_add(clear_rows as u16))
        .min(term_height);
    for row in clear_start_row..clear_end_row {
        queue!(stdout, MoveTo(0, row), Clear(ClearType::CurrentLine))
            .context("清理动作菜单行失败")?;
    }
    let printable_rows = lines.len().min(usize::from(term_height));
    for (row, line) in lines.iter().take(printable_rows).enumerate() {
        queue!(
            stdout,
            MoveTo(0, start_row.saturating_add(row as u16)),
            Print(line)
        )
        .context("绘制动作菜单行失败")?;
    }
    Ok(ActionMenuDrawState {
        start_row,
        rows: lines.len(),
    })
}

fn clear_action_menu(stdout: &mut io::Stdout, start_row: u16, rows: usize) -> Result<()> {
    let (_, term_height) = size().unwrap_or((80, 24));
    let clear_end_row = start_row
        .saturating_add(rows as u16)
        .min(term_height.max(1));
    for row in start_row..clear_end_row {
        queue!(stdout, MoveTo(0, row), Clear(ClearType::CurrentLine))
            .context("清理动作菜单行失败")?;
    }
    Ok(())
}

fn print_centered_line(text: &str, primary: bool) {
    let width_limit = content_width();
    let text = truncate_display(text, width_limit);
    let width = display_width(&text);
    let left = (width_limit.saturating_sub(width)) / 2;
    let text = if primary {
        style(text).cyan().bright().bold()
    } else {
        style(text).white().bright()
    };
    wln!("{}{}", " ".repeat(left), text);
}

fn frame_top(title: &str, width: usize) -> String {
    let inner = width.saturating_sub(4).max(4);
    format!("+ {}", truncate_display(title, inner))
}

fn frame_line(text: &str, width: usize) -> String {
    let inner = width.saturating_sub(3).max(1);
    format!("| {}", truncate_display(text, inner))
}

fn frame_rule(width: usize) -> String {
    format!("+{}", "-".repeat(width.saturating_sub(1)))
}

fn frame_bottom(width: usize) -> String {
    format!("+{}", "=".repeat(width.saturating_sub(1)))
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
        let w = UnicodeWidthChar::width(ch).unwrap_or(1);
        if used + w > limit {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn status_glyph(kind: StatusKind) -> &'static str {
    match kind {
        StatusKind::Running => "󰄬",
        StatusKind::Stopped => "󰅖",
        StatusKind::Warning => "󰀪",
        StatusKind::Neutral => "󰋽",
    }
}
