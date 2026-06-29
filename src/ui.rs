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
    terminal::{
        Clear as TermClear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode, size,
    },
};
use dialoguer::Input;
use dialoguer::console::style;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear as TuiClear, List, ListItem, ListState, Padding,
        Paragraph, Wrap,
    },
};
use std::{
    io::{self, Write},
    sync::atomic::{AtomicU16, Ordering},
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

const MAX_CONTENT_WIDTH: usize = 112;
const PANEL_WIDTH: usize = 76;
const KEY_WIDTH: usize = 14;
const MENU_REDRAW_TICK: Duration = Duration::from_millis(100);
const DASHBOARD_BACKGROUND_TICK: Duration = Duration::from_millis(250);

const BG_BASE: Color = Color::Rgb(22, 17, 12);
const SURFACE_DIM: Color = Color::Rgb(30, 24, 16);
const TEXT_PRIMARY: Color = Color::Rgb(236, 220, 176);
const TEXT_MUTED: Color = Color::Rgb(160, 136, 104);
const TEXT_DIM: Color = Color::Rgb(96, 78, 56);
const DARK_TEXT: Color = Color::Rgb(22, 17, 12);
const ACCENT_PRIMARY: Color = Color::Rgb(226, 152, 28);
const ACCENT_SECONDARY: Color = Color::Rgb(206, 88, 36);
const ACCENT_TERTIARY: Color = Color::Rgb(180, 148, 82);
const STATUS_OK: Color = Color::Rgb(152, 195, 62);
const STATUS_WARN: Color = Color::Rgb(226, 192, 44);
const STATUS_ERROR: Color = Color::Rgb(198, 58, 54);
const BORDER_MUTED: Color = Color::Rgb(72, 56, 36);
const BORDER_ACTIVE: Color = Color::Rgb(226, 152, 28);

fn content_width() -> usize {
    let (term_width, _) = size().unwrap_or((80, 24));
    usize::from(term_width)
        .saturating_sub(4)
        .clamp(1, MAX_CONTENT_WIDTH)
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
        execute!(stdout, Show).context("显示终端光标失败")?;
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
                if remaining.is_zero() {
                    clear_action_menu(&mut stdout, last_start_row, last_drawn_rows)
                        .context("清理动作菜单失败")?;
                    set_printed_rows(menu_origin.1);
                    stdout.flush()?;
                    return Ok(default);
                }
                if !poll(remaining.min(MENU_REDRAW_TICK)).context("等待动作菜单按键失败")?
                {
                    continue;
                }
                read().context("读取动作菜单按键失败")?
            } else {
                read().context("读取动作菜单按键失败")?
            };

            match event {
                Event::Key(key) if is_key_input(key.kind) => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        restore_terminal_state();
                        println!("\r\n操作已被用户中断");
                        std::process::exit(130);
                    }
                    KeyCode::Up | KeyCode::Left | KeyCode::Char('k') | KeyCode::Char('h') => {
                        selected = wrap_index(selected, actions.len(), -1);
                    }
                    KeyCode::Down | KeyCode::Right | KeyCode::Char('j') | KeyCode::Char('l') => {
                        selected = wrap_index(selected, actions.len(), 1);
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
                Event::Resize(_, _) | Event::Key(_) => {}
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
        execute!(stdout, Show).context("重新显示终端光标失败")?;
        result
    }

    pub(crate) fn dashboard_event_loop<F, G>(
        &self,
        state: &mut DashboardState,
        mut render: F,
        mut open_inline_info_popup: G,
    ) -> Result<DashboardEvent>
    where
        F: FnMut(&DashboardState) -> Result<DashboardView>,
        G: FnMut(&mut DashboardState, &DashboardView) -> Result<bool>,
    {
        enable_raw_mode().context("启用 ratatui raw mode 失败")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, Hide).context("进入 ratatui 备用屏幕失败")?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).context("创建 ratatui 终端失败")?;
        terminal.clear().context("清空 ratatui 终端失败")?;
        let _guard = DashboardTerminalGuard;

        let mut view = refresh_dashboard_view(state, &mut render)?;
        terminal
            .draw(|frame| render_dashboard(frame, &view))
            .context("绘制 ratatui Dashboard 失败")?;

        loop {
            let mut should_draw = false;
            if !poll(DASHBOARD_BACKGROUND_TICK).context("等待 ratatui 按键失败")? {
                if dashboard_idle_should_rebuild(&view, state) {
                    view = refresh_dashboard_view(state, &mut render)?;
                    should_draw = true;
                }
                if should_draw {
                    terminal
                        .draw(|frame| render_dashboard(frame, &view))
                        .context("绘制 ratatui Dashboard 失败")?;
                }
                continue;
            }

            match read().context("读取 ratatui 按键失败")? {
                Event::Key(key) if is_key_input(key.kind) => {
                    match handle_dashboard_key(state, &view, key.code, key.modifiers) {
                        DashboardInputAction::Event(event) => return Ok(event),
                        DashboardInputAction::Rebuild => {
                            view = refresh_dashboard_view(state, &mut render)?;
                            should_draw = true;
                        }
                        DashboardInputAction::Redraw => {
                            sync_cached_dashboard_view(&mut view, state);
                            should_draw = true;
                        }
                        DashboardInputAction::OpenPopup => {
                            state.popup = popup_for_selection(&view);
                            sync_cached_dashboard_view(&mut view, state);
                            should_draw = state.popup.is_some();
                        }
                        DashboardInputAction::OpenInlineInfoPopup => {
                            state.popup = Some(inline_info_loading_popup(&view));
                            sync_cached_dashboard_view(&mut view, state);
                            terminal
                                .draw(|frame| render_dashboard(frame, &view))
                                .context("绘制 ratatui Dashboard 失败")?;
                            if open_inline_info_popup(state, &view)? {
                                sync_cached_dashboard_view(&mut view, state);
                                should_draw = true;
                            } else {
                                state.popup = popup_for_selection(&view);
                                sync_cached_dashboard_view(&mut view, state);
                                should_draw = state.popup.is_some();
                            }
                        }
                        DashboardInputAction::Idle => {}
                    }
                }
                Event::Resize(_, _) => {
                    sync_cached_dashboard_view(&mut view, state);
                    should_draw = true;
                }
                Event::Key(_) => {}
                _ => {}
            }

            if should_draw {
                terminal
                    .draw(|frame| render_dashboard(frame, &view))
                    .context("绘制 ratatui Dashboard 失败")?;
            }
        }
    }
}

fn render_dashboard(frame: &mut Frame<'_>, view: &DashboardView) {
    frame.render_widget(
        Block::default().style(Style::default().bg(BG_BASE)),
        frame.area(),
    );
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    render_header(frame, root[0]);
    render_body(frame, root[1], view);
    render_footer(frame, root[2], view);

    if let Some(popup) = &view.popup {
        render_popup(frame, popup_area(popup, frame.area()), popup);
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_MUTED))
        .style(Style::default().bg(SURFACE_DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let title = Line::from(vec![
        Span::styled(
            format!("  {APP_HEADER_TITLE}"),
            Style::default()
                .fg(ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  v{APP_VERSION}"),
            Style::default().fg(ACCENT_TERTIARY),
        ),
    ]);
    let subtitle = Line::from(Span::styled(
        APP_HEADER_SUBTITLE,
        Style::default().fg(TEXT_DIM),
    ));
    frame.render_widget(
        Paragraph::new(title).alignment(Alignment::Center),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(subtitle).alignment(Alignment::Center),
        chunks[1],
    );
}

fn render_body(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(22), Constraint::Percentage(78)])
        .margin(1)
        .split(area);
    render_sidebar(frame, chunks[0], view);
    render_content(frame, chunks[1], view);
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let focused = view.focus == DashboardFocus::Sidebar;
    let items = DashboardTab::SIDEBAR
        .iter()
        .map(|tab| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    tab.icon(),
                    Style::default()
                        .fg(ACCENT_SECONDARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(tab.label(), Style::default().fg(TEXT_MUTED)),
            ]))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    state.select(Some(view.active_tab.sidebar_index()));
    let list = List::new(items)
        .block(ethereal_block(Some("导航"), focused))
        .style(Style::default().fg(TEXT_PRIMARY))
        .highlight_style(selected_style())
        .highlight_symbol("");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_content(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    match view.active_tab {
        DashboardTab::Deploy => render_deployment(frame, area, view),
        DashboardTab::About => render_about(frame, area, view),
        DashboardTab::Core | DashboardTab::Protocol | DashboardTab::Plugins => {
            render_table_view(frame, area, view)
        }
        _ => render_overview(frame, area, view),
    }
}

fn render_overview(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(66), Constraint::Percentage(34)])
        .margin(1)
        .split(area);
    render_table(frame, chunks[0], view, "概览");

    let selected = view.cards.get(view.selected);
    let detail = selected
        .map(|card| {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        card.icon,
                        Style::default()
                            .fg(ACCENT_SECONDARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        card.title.clone(),
                        Style::default()
                            .fg(ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(card.subtitle.clone(), muted_style())),
                Line::from(Span::styled(
                    card.detail.clone(),
                    Style::default().fg(TEXT_PRIMARY),
                )),
            ];
            for line in &view.detail_lines {
                lines.push(Line::from(Span::styled(line.clone(), muted_style())));
            }
            Text::from(lines)
        })
        .unwrap_or_else(|| Text::from(view.empty_detail.clone()));
    let paragraph = Paragraph::new(detail)
        .block(ethereal_block(Some("详情"), false))
        .style(Style::default().fg(TEXT_PRIMARY))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, chunks[1]);
}

fn render_table_view(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    render_table(frame, area, view, view.page_title.as_str());
}

fn render_about(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let outer = ethereal_block(Some("关于"), view.focus == DashboardFocus::Content);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let (direction, constraints) = if inner.width < 72 {
        (
            Direction::Vertical,
            [Constraint::Percentage(44), Constraint::Percentage(56)],
        )
    } else {
        (
            Direction::Horizontal,
            [Constraint::Percentage(38), Constraint::Percentage(62)],
        )
    };
    let chunks = Layout::default()
        .direction(direction)
        .constraints(constraints)
        .split(inner);
    render_about_list(frame, chunks[0], view);
    render_about_detail(frame, chunks[1], view);
}

fn render_about_list(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let items = if view.cards.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            view.empty_title.clone(),
            muted_style(),
        )))]
    } else {
        view.cards
            .iter()
            .map(|card| {
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            card.icon,
                            Style::default()
                                .fg(ACCENT_SECONDARY)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            card.title.clone(),
                            Style::default()
                                .fg(TEXT_PRIMARY)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(Span::styled(card.subtitle.clone(), muted_style())),
                ])
            })
            .collect()
    };
    let mut state = ListState::default();
    if !view.cards.is_empty() {
        state.select(Some(view.selected.min(view.cards.len() - 1)));
    }
    let list = List::new(items)
        .block(compact_block(Some("信息"), false))
        .style(Style::default().fg(TEXT_PRIMARY))
        .highlight_style(selected_style())
        .highlight_symbol("");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_about_detail(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let selected = view.cards.get(view.selected);
    let mut lines = Vec::new();
    if let Some(card) = selected {
        lines.push(Line::from(vec![
            Span::styled(
                card.icon,
                Style::default()
                    .fg(ACCENT_SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                card.title.clone(),
                Style::default()
                    .fg(ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            card.subtitle.clone(),
            muted_style(),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            card.detail.clone(),
            Style::default().fg(TEXT_PRIMARY),
        )));
        lines.push(Line::from(""));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            view.empty_detail.clone(),
            muted_style(),
        )));
    } else {
        for line in &view.detail_lines {
            lines.push(Line::from(Span::styled(line.clone(), muted_style())));
        }
    }
    let paragraph = Paragraph::new(Text::from(lines))
        .block(compact_block(Some("详情"), false))
        .style(Style::default().fg(TEXT_PRIMARY))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_table(frame: &mut Frame<'_>, area: Rect, view: &DashboardView, title: &str) {
    let items = if view.cards.is_empty() {
        vec![ListItem::new(vec![
            Line::from(Span::styled(
                view.empty_title.clone(),
                Style::default().fg(TEXT_MUTED),
            )),
            Line::from(Span::styled(
                view.empty_detail.clone(),
                Style::default().fg(TEXT_DIM),
            )),
        ])]
    } else {
        view.cards
            .iter()
            .map(|card| {
                let badge_color = status_color(card.kind);
                let line1 = Line::from(vec![
                    Span::styled(
                        card.icon,
                        Style::default()
                            .fg(ACCENT_SECONDARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        status_dot(card.kind),
                        Style::default()
                            .fg(badge_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        card.title.clone(),
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("[{}]", card.badge),
                        Style::default().fg(badge_color),
                    ),
                ]);
                let line2 = Line::from(vec![
                    Span::raw("     "),
                    Span::styled(card.subtitle.clone(), Style::default().fg(TEXT_MUTED)),
                ]);
                ListItem::new(vec![line1, line2])
            })
            .collect::<Vec<_>>()
    };
    let mut state = ListState::default();
    if !view.cards.is_empty() {
        state.select(Some(view.selected.min(view.cards.len() - 1)));
    }
    let list = List::new(items)
        .block(ethereal_block(
            Some(title),
            view.focus == DashboardFocus::Content,
        ))
        .style(Style::default().fg(TEXT_PRIMARY))
        .highlight_style(selected_style())
        .highlight_symbol("");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_deployment(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let outer = ethereal_block(Some("部署与更新"), view.focus == DashboardFocus::Content);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(5),
        ])
        .split(inner);
    render_step_bar(frame, chunks[0], view);
    render_deploy_wizard(frame, chunks[1], view);
    render_deploy_description(frame, chunks[2], view);
}

fn render_step_bar(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let current = view
        .cards
        .get(view.selected)
        .and_then(|card| deploy_card_field(card.id));
    let steps = view
        .cards
        .iter()
        .filter_map(|card| deploy_card_field(card.id).map(|field| (field, card.title.as_str())))
        .collect::<Vec<_>>();
    if steps.is_empty() {
        return;
    }
    let current_index = steps
        .iter()
        .position(|(field, _)| Some(*field) == current)
        .unwrap_or_else(|| view.selected.min(steps.len().saturating_sub(1)));
    let visible = if area.width < 58 {
        3
    } else if area.width < 96 {
        5
    } else {
        steps.len().min(7)
    };
    let start = scroll_start(steps.len(), visible, current_index);
    let end = (start + visible).min(steps.len());
    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::styled(" ... ", muted_style()));
        spans.push(Span::styled(" ─ ", Style::default().fg(BORDER_MUTED)));
    }
    for (idx, (field, _)) in steps.iter().enumerate().take(end).skip(start) {
        if idx > start {
            spans.push(Span::styled("  ───  ", Style::default().fg(BORDER_MUTED)));
        }
        let style = if current == Some(*field) {
            Style::default()
                .fg(DARK_TEXT)
                .bg(ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            muted_style()
        };
        spans.push(Span::styled(
            format!(" {}. {} ", idx + 1, deployment_step_label(*field)),
            style,
        ));
    }
    if end < steps.len() {
        spans.push(Span::styled(" ─ ", Style::default().fg(BORDER_MUTED)));
        spans.push(Span::styled(" ... ", muted_style()));
    }
    let paragraph = Paragraph::new(Line::from(spans))
        .block(compact_block(None, false))
        .alignment(Alignment::Center)
        .style(Style::default());
    frame.render_widget(paragraph, area);
}

fn render_deploy_wizard(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let (direction, constraints) = if area.width < 80 {
        (
            Direction::Vertical,
            [Constraint::Percentage(62), Constraint::Percentage(38)],
        )
    } else {
        (
            Direction::Horizontal,
            [Constraint::Percentage(65), Constraint::Percentage(35)],
        )
    };
    let chunks = Layout::default()
        .direction(direction)
        .constraints(constraints)
        .split(area);
    render_current_step_options(frame, chunks[0], view);
    render_deploy_summary(frame, chunks[1], view);
}

fn render_current_step_options(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let selected_card = view.cards.get(view.selected);
    let title = selected_card
        .map(|card| deploy_step_title(card.title.as_str()))
        .unwrap_or("选择配置");

    let items = if view.detail_choices.is_empty() && selected_card.is_some() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            selected_card
                .map(|card| card.subtitle.clone())
                .unwrap_or_default(),
            Style::default().fg(TEXT_PRIMARY),
        )]))]
    } else {
        view.detail_choices
            .iter()
            .map(|choice| {
                let suffix = if choice.active { "  ✔" } else { "" };
                ListItem::new(Line::from(vec![
                    Span::styled(choice.label.clone(), Style::default().fg(TEXT_PRIMARY)),
                    Span::styled(
                        suffix,
                        Style::default().fg(STATUS_OK).add_modifier(Modifier::BOLD),
                    ),
                ]))
            })
            .collect::<Vec<_>>()
    };
    let mut state = ListState::default();
    let selected_choice = view
        .detail_choices
        .iter()
        .position(|choice| choice.selected)
        .or_else(|| view.detail_choices.iter().position(|choice| choice.active))
        .unwrap_or(0);
    if !items.is_empty() {
        state.select(Some(selected_choice.min(items.len() - 1)));
    }
    let list = List::new(items)
        .block(ethereal_block(
            Some(title),
            view.focus == DashboardFocus::Content,
        ))
        .style(Style::default().fg(TEXT_PRIMARY))
        .highlight_style(selected_style())
        .highlight_symbol("");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_deploy_summary(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let summary = view
        .cards
        .iter()
        .filter(|card| deploy_card_field(card.id).is_some())
        .take(6)
        .map(|card| {
            Line::from(vec![
                Span::styled(card.title.clone(), muted_style()),
                Span::raw("  "),
                Span::styled(card.subtitle.clone(), Style::default().fg(TEXT_PRIMARY)),
            ])
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(Text::from(summary))
        .block(ethereal_block(Some("配置总览"), false))
        .style(Style::default().fg(TEXT_PRIMARY))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_deploy_description(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let text = selected_deploy_description(view)
        .unwrap_or_else(|| "确认配置后，当前表单会组成下一次部署计划。".to_string());
    let paragraph = Paragraph::new(Text::from(vec![Line::from(Span::styled(
        text,
        Style::default().fg(TEXT_MUTED),
    ))]))
    .block(ethereal_block(Some("说明"), false))
    .style(Style::default().fg(TEXT_MUTED))
    .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, view: &DashboardView) {
    let width = usize::from(area.width);

    // Show status message override prominently if set
    if let Some(status_msg) = &view.popup.as_ref().and(None::<&str>) {
        let _ = status_msg;
    }

    let prompt = match view.mode {
        AppMode::Navigation => {
            "[↑/↓] 导航   [Tab] 面板   [Enter] 确认   [Esc] 返回   [Ctrl+Q] 退出"
        }
        AppMode::ContentFocused => {
            if view.active_tab == DashboardTab::Deploy {
                "[←/→] 配置   [↑/↓] 选项   [Enter] 确认   [F5] 安装/更新   [Ctrl+R] 默认   [Ctrl+1] 导航   [Ctrl+Q] 退出"
            } else {
                "[↑/↓] 选择   [Tab] 面板   [Enter] 打开   [Esc] 返回   [Ctrl+1] 导航   [Ctrl+Q] 退出"
            }
        }
        AppMode::PopupActive => {
            "[←/→] 操作   [Enter] 执行   [Esc] 关闭   [Ctrl+1] 导航   [Ctrl+Q] 退出"
        }
    };
    let branch = "分支: main";
    let mut text = prompt.to_string();
    let branch_width = display_width(branch);
    let prompt_width = display_width(prompt);
    if width > branch_width + 2 {
        let spaces = width.saturating_sub(prompt_width + branch_width).max(1);
        text = format!("{prompt}{}{branch}", " ".repeat(spaces));
    }
    let paragraph = Paragraph::new(truncate_display(&text, width))
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(ACCENT_TERTIARY)
                .bg(SURFACE_DIM)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(paragraph, area);
}

fn render_popup(frame: &mut Frame<'_>, area: Rect, popup: &DashboardPopup) {
    frame.render_widget(TuiClear, area);
    frame.render_widget(
        Block::default().style(Style::default().fg(TEXT_PRIMARY).bg(BG_BASE)),
        area,
    );
    let block = modal_block(Some(popup.title.as_str()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let action_height = if popup.actions.is_empty() { 0 } else { 3 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(action_height)])
        .split(inner);

    let mut lines = Vec::new();
    if !popup.subtitle.is_empty() {
        lines.push(Line::from(Span::styled(
            popup.subtitle.clone(),
            Style::default()
                .fg(ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }
    for line in &popup.lines {
        lines.push(popup_body_line(line));
    }
    let body = Paragraph::new(Text::from(lines))
        .style(Style::default().fg(TEXT_PRIMARY).bg(SURFACE_DIM))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, chunks[0]);

    if action_height > 0 {
        render_popup_actions(frame, chunks[1], popup);
    }
}

fn render_popup_actions(frame: &mut Frame<'_>, area: Rect, popup: &DashboardPopup) {
    if popup.actions.is_empty() {
        return;
    }
    let chunks = popup_action_areas(area, &popup.actions);
    let last_idx = popup.actions.len().saturating_sub(1);
    for (idx, action) in popup.actions.iter().enumerate() {
        let active = idx == popup.selected;
        let is_destructive = action.contains("删除")
            || action.contains("移除")
            || action.contains("卸载")
            || action.contains("停止");
        let is_cancel = action == "取消" || idx == last_idx;
        let text_style = if active {
            Style::default()
                .fg(DARK_TEXT)
                .bg(ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else if is_destructive {
            Style::default().fg(STATUS_ERROR)
        } else if is_cancel {
            Style::default().fg(TEXT_DIM)
        } else if idx == 0 {
            Style::default()
                .fg(ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            muted_style()
        };
        let paragraph = Paragraph::new(Line::from(Span::styled(action.clone(), text_style)))
            .alignment(Alignment::Center)
            .style(Style::default().bg(SURFACE_DIM))
            .block(popup_action_block(active));
        if let Some(area) = chunks.get(idx) {
            frame.render_widget(paragraph, *area);
        }
    }
}

fn compact_block(title: Option<&str>, focused: bool) -> Block<'_> {
    styled_block(title, focused)
}

fn ethereal_block(title: Option<&str>, focused: bool) -> Block<'_> {
    styled_block(title, focused).padding(Padding::symmetric(1, 1))
}

fn modal_block(title: Option<&str>) -> Block<'_> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_ACTIVE))
        .style(Style::default().fg(TEXT_PRIMARY).bg(SURFACE_DIM))
        .padding(Padding::symmetric(2, 1));
    if let Some(title) = title {
        block = block.title(Span::styled(
            title.to_string(),
            Style::default()
                .fg(ACCENT_PRIMARY)
                .bg(SURFACE_DIM)
                .add_modifier(Modifier::BOLD),
        ));
    }
    block
}

fn popup_action_block(active: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { BORDER_ACTIVE } else { BORDER_MUTED }))
        .style(Style::default().fg(TEXT_PRIMARY).bg(SURFACE_DIM))
}

fn styled_block(title: Option<&str>, focused: bool) -> Block<'_> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused {
            ACCENT_PRIMARY
        } else {
            BORDER_MUTED
        }))
        .style(Style::default().fg(TEXT_PRIMARY));
    if let Some(title) = title {
        block = block.title(Span::styled(
            title.to_string(),
            Style::default()
                .fg(ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ));
    }
    block
}

fn popup_body_line(line: &str) -> Line<'static> {
    if line.trim().is_empty() {
        return Line::from("");
    }
    let style = if popup_line_looks_like_heading(line) {
        Style::default()
            .fg(ACCENT_SECONDARY)
            .bg(SURFACE_DIM)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("地址 ") || line.starts_with("密钥 ") {
        Style::default().fg(TEXT_PRIMARY).bg(SURFACE_DIM)
    } else {
        muted_style().bg(SURFACE_DIM)
    };
    Line::from(Span::styled(line.to_string(), style))
}

fn popup_line_looks_like_heading(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.contains(' ')
        && !trimmed.contains(':')
        && !trimmed.contains('：')
        && display_width(trimmed) <= 28
}

fn popup_area(popup: &DashboardPopup, area: Rect) -> Rect {
    let longest = popup
        .lines
        .iter()
        .chain(std::iter::once(&popup.title))
        .chain(std::iter::once(&popup.subtitle))
        .map(|line| display_width(line))
        .max()
        .unwrap_or(36);
    let action_width = popup
        .actions
        .iter()
        .map(|action| (display_width(action) + 6).clamp(10, 16))
        .sum::<usize>();
    let desired_width = (longest + 10).max(action_width + 6).clamp(42, 88) as u16;
    let max_width = area.width.saturating_sub(6).max(24);
    let width = desired_width.min(max_width);
    let body_width = usize::from(width.saturating_sub(6)).max(12);
    let subtitle_lines = if popup.subtitle.is_empty() { 0 } else { 2 };
    let body_lines = popup
        .lines
        .iter()
        .map(|line| wrapped_line_count(line, body_width))
        .sum::<usize>();
    let action_height = if popup.actions.is_empty() {
        0_usize
    } else {
        3_usize
    };
    let desired_height = (subtitle_lines + body_lines + action_height + 4).clamp(9, 22) as u16;
    let max_height = area.height.saturating_sub(4).max(7);
    let height = desired_height.min(max_height);
    centered_rect_cells(width, height, area)
}

fn popup_action_areas(area: Rect, actions: &[String]) -> Vec<Rect> {
    if actions.is_empty() || area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let constraints = actions
        .iter()
        .map(|action| Constraint::Length((display_width(action) + 6).clamp(10, 16) as u16))
        .collect::<Vec<_>>();
    let total = constraints
        .iter()
        .map(|constraint| match constraint {
            Constraint::Length(width) => *width,
            _ => 0,
        })
        .sum::<u16>();
    let available = area.width;
    if total > available {
        return Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Ratio(1, actions.len() as u32);
                actions.len()
            ])
            .split(area)
            .to_vec();
    }
    let x = area.x + available.saturating_sub(total) / 2;
    let centered = Rect::new(x, area.y, total.min(available), area.height);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(centered)
        .to_vec()
}

fn wrapped_line_count(line: &str, width: usize) -> usize {
    if line.is_empty() {
        return 1;
    }
    display_width(line).div_ceil(width.max(1)).max(1)
}

fn selected_style() -> Style {
    Style::default()
        .fg(DARK_TEXT)
        .bg(ACCENT_PRIMARY)
        .add_modifier(Modifier::BOLD)
}

fn muted_style() -> Style {
    Style::default().fg(TEXT_MUTED)
}

fn status_dot(kind: StatusKind) -> &'static str {
    match kind {
        StatusKind::Running => "●",
        StatusKind::Stopped => "●",
        StatusKind::Warning => "●",
        StatusKind::Neutral => "●",
    }
}

fn status_color(kind: StatusKind) -> Color {
    match kind {
        StatusKind::Running => STATUS_OK,
        StatusKind::Stopped => STATUS_ERROR,
        StatusKind::Warning => STATUS_WARN,
        StatusKind::Neutral => ACCENT_SECONDARY,
    }
}

fn deployment_step_label(field: PlanField) -> &'static str {
    match field {
        PlanField::InstallPath => "路径",
        PlanField::MaiBotBranch => "分支",
        PlanField::InstallMode => "模式",
        PlanField::PythonEnv => "Python",
        PlanField::VenvMode => "环境",
        PlanField::GithubProxy => "GitHub",
        PlanField::PipSource => "PyPI",
        PlanField::BotProtocols => "协议端",
        PlanField::DockerMirror => "Docker",
    }
}

fn deploy_step_title(fallback: &str) -> &str {
    match fallback {
        "GitHub 线路" | "GitHub 镜像" | "GitHub 代理" => "选择 GitHub 镜像源",
        "安装路径" => "选择安装目录",
        "MaiBot 分支" => "选择主程序分支",
        "安装模式" => "选择部署模式",
        "Python 环境" | "虚拟环境" => "配置核心环境",
        "PyPI 镜像源" => "选择 PyPI 镜像源",
        "协议端" => "选择协议端服务",
        _ => fallback,
    }
}

fn selected_deploy_description(view: &DashboardView) -> Option<String> {
    if let Some(choice) = view.detail_choices.iter().find(|choice| choice.selected) {
        return Some(choice.detail.clone());
    }
    view.cards
        .get(view.selected)
        .map(|card| card.detail.clone())
        .filter(|text| !text.is_empty())
}

fn direct_info_popup_card(view: &DashboardView) -> bool {
    let Some(card) = view.cards.get(view.selected) else {
        return false;
    };
    match view.active_tab {
        DashboardTab::Access => card.id == "access-summary" || card.id.ends_with("-note"),
        DashboardTab::Protocol => card.id.ends_with("-note"),
        _ => false,
    }
}

fn popup_for_selection(view: &DashboardView) -> Option<DashboardPopup> {
    let card = view.cards.get(view.selected)?;
    let mut actions = match view.active_tab {
        DashboardTab::Core => match card.id {
            "core-start" => vec![
                "后台启动".to_string(),
                "启动并进入终端".to_string(),
                "取消".to_string(),
            ],
            "core-stop" => vec!["确认停止 MaiBot".to_string(), "取消".to_string()],
            "core-console" => vec!["进入 screen 控制台".to_string(), "取消".to_string()],
            "core-logs" => vec![
                "查看最近 100 行".to_string(),
                "实时跟随日志".to_string(),
                "取消".to_string(),
            ],
            _ => vec!["执行".to_string(), "取消".to_string()],
        },
        DashboardTab::Protocol => {
            if card.id.ends_with("-note") {
                vec!["查看说明".to_string(), "取消".to_string()]
            } else if card.id == "napcat" {
                vec![
                    "启动".to_string(),
                    "停止".to_string(),
                    "重启".to_string(),
                    "查看日志".to_string(),
                    "重建容器".to_string(),
                    "取消".to_string(),
                ]
            } else if card.id == "llbot" {
                vec![
                    "启动".to_string(),
                    "停止".to_string(),
                    "重启".to_string(),
                    "进入控制台".to_string(),
                    "修改密码".to_string(),
                    "取消".to_string(),
                ]
            } else {
                vec!["启动".to_string(), "停止".to_string(), "取消".to_string()]
            }
        }
        DashboardTab::Plugins => {
            if card.id == "plugin-item" {
                vec![
                    "更新插件".to_string(),
                    "卸载".to_string(),
                    "取消".to_string(),
                ]
            } else {
                vec!["打开插件中心".to_string(), "取消".to_string()]
            }
        }
        DashboardTab::Overview => vec!["打开".to_string(), "取消".to_string()],
        DashboardTab::Access => {
            if card.id.ends_with("-note") {
                vec!["查看说明".to_string(), "取消".to_string()]
            } else if card.id == "access-clear-data" {
                vec!["确认清空数据".to_string(), "取消".to_string()]
            } else if card.id == "access-init" {
                vec!["确认执行".to_string(), "取消".to_string()]
            } else {
                vec!["打开".to_string(), "取消".to_string()]
            }
        }
        DashboardTab::About => return None,
        DashboardTab::Deploy => return None,
    };
    ensure_popup_actions(&mut actions);
    let mut lines = vec![format!("状态: {}", card.badge), card.detail.clone()];
    lines.extend(view.detail_lines.iter().take(8).cloned());
    Some(DashboardPopup {
        title: card.title.clone(),
        subtitle: card.subtitle.clone(),
        lines,
        actions,
        selected: 0,
    })
}

fn inline_info_loading_popup(view: &DashboardView) -> DashboardPopup {
    let (title, subtitle) = view
        .cards
        .get(view.selected)
        .map(|card| {
            (
                card.title.clone(),
                if card.id == "access-summary" {
                    "正在整理访问入口".to_string()
                } else {
                    "正在整理说明内容".to_string()
                },
            )
        })
        .unwrap_or_else(|| ("信息".to_string(), "正在整理内容".to_string()));
    DashboardPopup {
        title,
        subtitle,
        lines: vec!["请稍候。".to_string()],
        actions: Vec::new(),
        selected: 0,
    }
}

fn ensure_popup_actions(actions: &mut Vec<String>) {
    actions.retain(|action| !action.trim().is_empty());
    if actions.is_empty() {
        actions.push("打开".to_string());
    }
    if !actions.iter().any(|action| action == "取消") {
        actions.push("取消".to_string());
    }
}

fn centered_rect_cells(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

struct DashboardTerminalGuard;

impl Drop for DashboardTerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
        let _ = stdout.flush();
    }
}

struct ActionMenuGuard;

impl Drop for ActionMenuGuard {
    fn drop(&mut self) {
        restore_terminal_state();
    }
}

struct ActionMenuDrawState {
    start_row: u16,
    rows: usize,
}

fn is_key_input(kind: KeyEventKind) -> bool {
    matches!(kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DashboardInputAction {
    Idle,
    Redraw,
    Rebuild,
    OpenPopup,
    OpenInlineInfoPopup,
    Event(DashboardEvent),
}

fn refresh_dashboard_view<F>(state: &mut DashboardState, render: &mut F) -> Result<DashboardView>
where
    F: FnMut(&DashboardState) -> Result<DashboardView>,
{
    sync_app_mode(state);
    let mut view = render(state)?;
    view.popup = state.popup.clone();
    Ok(view)
}

fn sync_app_mode(state: &mut DashboardState) {
    state.mode = if state.popup.is_some() {
        AppMode::PopupActive
    } else {
        match state.focus {
            DashboardFocus::Sidebar => AppMode::Navigation,
            DashboardFocus::Content => AppMode::ContentFocused,
        }
    };
}

fn dashboard_idle_should_rebuild(view: &DashboardView, state: &DashboardState) -> bool {
    view.background_refresh && view.active_tab == DashboardTab::Plugins && state.popup.is_none()
}

fn sync_cached_dashboard_view(view: &mut DashboardView, state: &mut DashboardState) {
    sync_app_mode(state);
    view.mode = state.mode;
    view.focus = state.focus;
    view.popup = state.popup.clone();
    if view.active_tab != state.active_tab {
        return;
    }

    if view.active_tab == DashboardTab::Deploy {
        sync_deploy_cards(view, state);
    }

    let selected = state.selected_for_len(view.cards.len());
    let selected_changed = view.selected != selected;
    view.selected = selected;
    if let Some(card) = view.cards.get(selected)
        && (selected_changed || view.active_tab == DashboardTab::Deploy)
    {
        view.detail_title = card.title.clone();
        view.detail_subtitle = card.subtitle.clone();
    }
    if view.active_tab == DashboardTab::Deploy {
        sync_deploy_detail_choices(view, state);
    } else if selected_changed {
        if view.active_tab == DashboardTab::About {
            view.detail_choices.clear();
        } else {
            sync_card_summary_lines(view);
            view.detail_choices.clear();
            view.action_lines.clear();
        }
    }
}

fn sync_card_summary_lines(view: &mut DashboardView) {
    view.detail_lines.clear();
    if let Some(card) = view.cards.get(view.selected) {
        view.detail_lines.push(format!("状态: {}", card.badge));
        view.detail_lines.push(format!("摘要: {}", card.detail));
    }
}

fn sync_deploy_cards(view: &mut DashboardView, state: &DashboardState) {
    let Some(plan) = state.deploy_plan.as_ref() else {
        return;
    };
    for card in &mut view.cards {
        if let Some(field) = deploy_card_field(card.id) {
            card.subtitle = cached_planner_field_value(plan, field);
        }
    }
}

fn sync_deploy_detail_choices(view: &mut DashboardView, state: &mut DashboardState) {
    view.detail_lines.clear();
    view.action_lines.clear();
    let Some(plan) = state.deploy_plan.as_ref() else {
        view.detail_choices.clear();
        return;
    };
    let Some(card) = view.cards.get(view.selected) else {
        view.detail_choices.clear();
        return;
    };
    let Some(field) = deploy_card_field(card.id) else {
        view.detail_choices.clear();
        return;
    };
    view.detail_choices = planner_choices_for_plan(plan, field)
        .into_iter()
        .enumerate()
        .map(|(idx, label)| DashboardChoice {
            detail: deploy_choice_detail_for_cache(field, idx, &label),
            active: planner_choice_active_for_plan(plan, field, idx),
            selected: false,
            label,
        })
        .collect();
    let active_idx = view
        .detail_choices
        .iter()
        .position(|choice| choice.active)
        .unwrap_or(0);
    let selected_idx =
        state.sync_deploy_choice_selection(field, view.detail_choices.len(), active_idx);
    if let Some(choice) = view.detail_choices.get_mut(selected_idx) {
        choice.selected = true;
    }
}

fn cached_planner_field_value(plan: &InstallPlan, field: PlanField) -> String {
    match field {
        PlanField::InstallPath => plan.install_path.display().to_string(),
        PlanField::InstallMode => plan.install_mode.label().to_string(),
        PlanField::PythonEnv => plan.python_env.label().to_string(),
        PlanField::VenvMode => plan.venv_mode.label(plan.python_env).to_string(),
        PlanField::MaiBotBranch => plan.maibot_branch.clone(),
        PlanField::GithubProxy => {
            if plan.github_proxy.is_empty() {
                "自动测速（执行时选择最佳线路）".to_string()
            } else {
                plan.github_proxy.clone()
            }
        }
        PlanField::PipSource => {
            if plan.pip_display.is_empty() {
                "系统默认".to_string()
            } else {
                plan.pip_display.clone()
            }
        }
        PlanField::BotProtocols => {
            if plan.bot_protocols.is_empty() {
                "暂不安装".to_string()
            } else {
                plan.bot_protocols
                    .iter()
                    .map(|protocol| protocol.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }
        PlanField::DockerMirror => plan.docker_mirror.label().to_string(),
    }
}

fn planner_choices_for_plan(plan: &InstallPlan, field: PlanField) -> Vec<String> {
    match field {
        PlanField::InstallPath => vec![plan.install_path.display().to_string()],
        PlanField::InstallMode => vec![
            InstallMode::Normal.label().to_string(),
            InstallMode::Clean.label().to_string(),
        ],
        PlanField::PythonEnv => vec![
            PythonEnv::System.label().to_string(),
            PythonEnv::Uv.label().to_string(),
        ],
        PlanField::VenvMode => {
            if plan.install_mode == InstallMode::Clean {
                vec!["固定: 删除并重建环境".to_string()]
            } else {
                vec![
                    VenvMode::Keep.label(plan.python_env).to_string(),
                    VenvMode::Recreate.label(plan.python_env).to_string(),
                ]
            }
        }
        PlanField::MaiBotBranch => vec!["main".to_string(), "dev".to_string()],
        PlanField::GithubProxy => {
            let mut choices = vec!["自动测速选择".to_string(), "官方直连".to_string()];
            choices.extend(github_mirrors().iter().map(|mirror| (*mirror).to_string()));
            if !plan.github_proxy.is_empty()
                && plan.github_proxy != "https://github.com"
                && !github_mirrors()
                    .iter()
                    .any(|mirror| *mirror == plan.github_proxy)
            {
                choices.push(format!("自定义: {}", plan.github_proxy));
            }
            choices.push("输入自定义镜像".to_string());
            choices
        }
        PlanField::PipSource => {
            let mut choices = vec![
                "系统默认".to_string(),
                "阿里云".to_string(),
                "腾讯云".to_string(),
                "清华大学".to_string(),
                "中国科学技术大学".to_string(),
                "官方源".to_string(),
            ];
            if !plan.pip_display.is_empty()
                && ![
                    "系统默认",
                    "阿里云",
                    "腾讯云",
                    "清华大学",
                    "中国科学技术大学",
                    "官方源",
                ]
                .contains(&plan.pip_display.as_str())
            {
                choices.push(format!("当前: {}", plan.pip_display));
            }
            choices.push("输入自定义 PyPI".to_string());
            choices
        }
        PlanField::BotProtocols => vec![
            BotProtocol::NapCat.label().to_string(),
            BotProtocol::LuckyLilliaBot.label().to_string(),
            "暂不安装协议端".to_string(),
        ],
        PlanField::DockerMirror => vec![
            DockerMirror::OneMs.label().to_string(),
            DockerMirror::Xuanyuan.label().to_string(),
            DockerMirror::Official.label().to_string(),
            DockerMirror::Keep.label().to_string(),
        ],
    }
}

fn planner_choice_active_for_plan(plan: &InstallPlan, field: PlanField, idx: usize) -> bool {
    match field {
        PlanField::InstallPath => idx == 0,
        PlanField::InstallMode => {
            matches!(
                (plan.install_mode, idx),
                (InstallMode::Normal, 0) | (InstallMode::Clean, 1)
            )
        }
        PlanField::PythonEnv => {
            matches!(
                (plan.python_env, idx),
                (PythonEnv::System, 0) | (PythonEnv::Uv, 1)
            )
        }
        PlanField::VenvMode => {
            if plan.install_mode == InstallMode::Clean {
                idx == 0
            } else {
                matches!(
                    (plan.venv_mode, idx),
                    (VenvMode::Keep, 0) | (VenvMode::Recreate, 1)
                )
            }
        }
        PlanField::MaiBotBranch => {
            (idx == 0 && plan.maibot_branch != "dev") || (idx == 1 && plan.maibot_branch == "dev")
        }
        PlanField::GithubProxy => {
            (idx == 0 && plan.github_proxy.is_empty())
                || (idx == 1 && plan.github_proxy == "https://github.com")
                || (idx >= 2
                    && idx < 2 + github_mirrors().len()
                    && github_mirrors()[idx - 2] == plan.github_proxy)
                || (idx == github_proxy_custom_choice_idx(plan)
                    && !plan.github_proxy.is_empty()
                    && plan.github_proxy != "https://github.com"
                    && !github_mirrors()
                        .iter()
                        .any(|mirror| *mirror == plan.github_proxy))
        }
        PlanField::PipSource => {
            matches!(
                (plan.pip_display.as_str(), plan.pip_index.as_str(), idx),
                (_, "", 0)
                    | ("系统默认", _, 0)
                    | ("阿里云", _, 1)
                    | ("腾讯云", _, 2)
                    | ("清华大学", _, 3)
                    | ("中国科学技术大学", _, 4)
                    | ("官方源", _, 5)
            )
        }
        PlanField::BotProtocols => {
            (idx == 0 && plan.bot_protocols.as_slice() == [BotProtocol::NapCat])
                || (idx == 1 && plan.bot_protocols.as_slice() == [BotProtocol::LuckyLilliaBot])
                || (idx == 2 && plan.bot_protocols.is_empty())
        }
        PlanField::DockerMirror => {
            matches!(
                (plan.docker_mirror, idx),
                (DockerMirror::OneMs, 0)
                    | (DockerMirror::Xuanyuan, 1)
                    | (DockerMirror::Official, 2)
                    | (DockerMirror::Keep, 3)
            )
        }
    }
}

fn deploy_choice_detail_for_cache(field: PlanField, idx: usize, label: &str) -> String {
    match field {
        PlanField::InstallPath => "打开路径输入框".to_string(),
        PlanField::MaiBotBranch => {
            if idx == 0 {
                "推荐稳定环境使用。".to_string()
            } else {
                "适合跟进新功能和预发布改动。".to_string()
            }
        }
        PlanField::InstallMode => {
            if idx == 0 {
                "保留现有工作区并执行更新/修复。".to_string()
            } else {
                "清空目标目录后重新部署。".to_string()
            }
        }
        PlanField::PythonEnv => {
            if idx == 0 {
                "使用本机 Python 解释器。".to_string()
            } else {
                "由 uv 管理隔离环境与 Python 版本。".to_string()
            }
        }
        PlanField::VenvMode => {
            if label.contains("固定") {
                "由当前安装模式自动锁定。".to_string()
            } else if idx == 0 {
                "尽量复用现有环境，减少重装时间。".to_string()
            } else {
                "重建环境以消除历史依赖残留。".to_string()
            }
        }
        PlanField::GithubProxy => {
            if idx == 0 {
                "执行时测速后自动选最快线路。".to_string()
            } else if idx == 1 {
                "直接访问官方 GitHub。".to_string()
            } else if label.contains("自定义") {
                "选择后会提示输入自定义镜像地址。".to_string()
            } else {
                "切换到预设 GitHub 镜像源。".to_string()
            }
        }
        PlanField::PipSource => {
            if idx == 0 {
                "保持系统默认 Python 包源。".to_string()
            } else if label.contains("自定义") {
                "选择后会提示输入自定义 PyPI 地址。".to_string()
            } else {
                "为 pip 和 uv 设置统一镜像源。".to_string()
            }
        }
        PlanField::BotProtocols => match idx {
            0 => "默认推荐，启用 NapCatQQ Shell。".to_string(),
            1 => "切换到 LuckyLilliaBot Desktop。".to_string(),
            _ => "暂不安装附加协议端。".to_string(),
        },
        PlanField::DockerMirror => "当前平台可能不使用 Docker。".to_string(),
    }
}

fn handle_dashboard_key(
    state: &mut DashboardState,
    view: &DashboardView,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> DashboardInputAction {
    if matches!(code, KeyCode::Char('q') | KeyCode::Char('c'))
        && modifiers.contains(KeyModifiers::CONTROL)
    {
        return DashboardInputAction::Event(DashboardEvent::Exit);
    }

    if code == KeyCode::Char('1') && modifiers.contains(KeyModifiers::CONTROL) {
        if state.focus == DashboardFocus::Sidebar && state.popup.is_none() {
            return DashboardInputAction::Idle;
        }
        state.popup = None;
        state.focus = DashboardFocus::Sidebar;
        return DashboardInputAction::Redraw;
    }

    if let Some(popup) = state.popup.as_mut() {
        match code {
            KeyCode::Esc => {
                state.popup = None;
                DashboardInputAction::Redraw
            }
            KeyCode::Up | KeyCode::Left => {
                if popup.actions.is_empty() {
                    popup.actions.push("取消".to_string());
                }
                popup.selected = wrap_index(popup.selected, popup.actions.len(), -1);
                DashboardInputAction::Redraw
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                if popup.actions.is_empty() {
                    popup.actions.push("取消".to_string());
                }
                popup.selected = wrap_index(popup.selected, popup.actions.len(), 1);
                DashboardInputAction::Redraw
            }
            KeyCode::Enter => {
                let cancel = popup
                    .actions
                    .get(popup.selected)
                    .is_none_or(|action| action == "取消");
                if cancel {
                    state.popup = None;
                    DashboardInputAction::Redraw
                } else {
                    DashboardInputAction::Event(DashboardEvent::Activate)
                }
            }
            _ => DashboardInputAction::Idle,
        }
    } else {
        match code {
            KeyCode::Tab | KeyCode::BackTab => {
                state.toggle_focus();
                DashboardInputAction::Redraw
            }
            KeyCode::Esc => {
                if state.focus == DashboardFocus::Content {
                    state.focus = DashboardFocus::Sidebar;
                    DashboardInputAction::Redraw
                } else {
                    DashboardInputAction::Idle
                }
            }
            KeyCode::Up => match state.focus {
                DashboardFocus::Sidebar => {
                    state.prev_tab();
                    DashboardInputAction::Rebuild
                }
                DashboardFocus::Content => {
                    if view.active_tab == DashboardTab::Deploy {
                        adjust_deploy_choice(state, view, -1)
                    } else {
                        state.move_selection(view.cards.len(), -1);
                        DashboardInputAction::Redraw
                    }
                }
            },
            KeyCode::Down => match state.focus {
                DashboardFocus::Sidebar => {
                    state.next_tab();
                    DashboardInputAction::Rebuild
                }
                DashboardFocus::Content => {
                    if view.active_tab == DashboardTab::Deploy {
                        adjust_deploy_choice(state, view, 1)
                    } else {
                        state.move_selection(view.cards.len(), 1);
                        DashboardInputAction::Redraw
                    }
                }
            },
            KeyCode::Left => match state.focus {
                DashboardFocus::Sidebar => DashboardInputAction::Idle,
                DashboardFocus::Content => {
                    if view.active_tab == DashboardTab::Deploy {
                        state.move_selection(view.cards.len(), -1);
                        DashboardInputAction::Redraw
                    } else {
                        DashboardInputAction::Idle
                    }
                }
            },
            KeyCode::Right => match state.focus {
                DashboardFocus::Sidebar => DashboardInputAction::Idle,
                DashboardFocus::Content => {
                    if view.active_tab == DashboardTab::Deploy {
                        state.move_selection(view.cards.len(), 1);
                        DashboardInputAction::Redraw
                    } else {
                        DashboardInputAction::Idle
                    }
                }
            },
            KeyCode::F(5)
                if view.active_tab == DashboardTab::Deploy
                    && state.focus == DashboardFocus::Content =>
            {
                DashboardInputAction::Event(DashboardEvent::RunDeployPlan)
            }
            KeyCode::Enter => match state.focus {
                DashboardFocus::Sidebar => {
                    state.focus = DashboardFocus::Content;
                    DashboardInputAction::Redraw
                }
                DashboardFocus::Content => {
                    if view.active_tab == DashboardTab::Deploy {
                        if let Some(selected) = view.cards.get(view.selected) {
                            match deploy_card_field(selected.id) {
                                Some(PlanField::InstallPath) => {
                                    DashboardInputAction::Event(DashboardEvent::Activate)
                                }
                                Some(field) => commit_deploy_choice(state, view, field),
                                None => DashboardInputAction::Idle,
                            }
                        } else {
                            DashboardInputAction::Idle
                        }
                    } else {
                        if view.active_tab == DashboardTab::About {
                            DashboardInputAction::Idle
                        } else if direct_info_popup_card(view) {
                            DashboardInputAction::OpenInlineInfoPopup
                        } else {
                            DashboardInputAction::OpenPopup
                        }
                    }
                }
            },
            KeyCode::Char('r')
                if modifiers.contains(KeyModifiers::CONTROL)
                    && view.active_tab == DashboardTab::Deploy =>
            {
                if state.focus == DashboardFocus::Content {
                    DashboardInputAction::Event(DashboardEvent::ResetDeployPlan)
                } else {
                    DashboardInputAction::Idle
                }
            }
            KeyCode::Backspace => DashboardInputAction::Event(DashboardEvent::ClearSearch),
            _ => DashboardInputAction::Idle,
        }
    }
}

fn adjust_deploy_choice(
    state: &mut DashboardState,
    view: &DashboardView,
    delta: isize,
) -> DashboardInputAction {
    if view.detail_choices.is_empty()
        || view
            .cards
            .get(view.selected)
            .and_then(|card| deploy_card_field(card.id))
            == Some(PlanField::InstallPath)
    {
        return DashboardInputAction::Idle;
    }
    let Some(card) = view.cards.get(view.selected) else {
        return DashboardInputAction::Idle;
    };
    let Some(field) = deploy_card_field(card.id) else {
        return DashboardInputAction::Idle;
    };
    let current_idx = view
        .detail_choices
        .iter()
        .position(|choice| choice.selected)
        .unwrap_or_else(|| state.deploy_choice_selection(field));
    let next = wrap_index(current_idx, view.detail_choices.len(), delta);
    state.set_deploy_choice_selection(field, next);
    DashboardInputAction::Redraw
}

fn commit_deploy_choice(
    state: &mut DashboardState,
    view: &DashboardView,
    field: PlanField,
) -> DashboardInputAction {
    if view.detail_choices.is_empty() {
        return DashboardInputAction::Idle;
    }
    let idx = view
        .detail_choices
        .iter()
        .position(|choice| choice.selected)
        .unwrap_or_else(|| state.deploy_choice_selection(field))
        .min(view.detail_choices.len() - 1);
    state.commit_deploy_choice_selection(field, idx);
    DashboardInputAction::Event(DashboardEvent::CommitDeployChoice {
        field,
        choice_idx: idx,
    })
}

#[cfg(test)]
fn apply_cached_planner_choice(plan: &mut InstallPlan, field: PlanField, idx: usize) {
    match field {
        PlanField::InstallPath => {}
        PlanField::InstallMode => {
            plan.install_mode = if idx == 0 {
                InstallMode::Normal
            } else {
                InstallMode::Clean
            };
            if plan.install_mode == InstallMode::Clean {
                plan.venv_mode = VenvMode::Recreate;
            }
        }
        PlanField::PythonEnv => {
            plan.python_env = if idx == 0 {
                PythonEnv::System
            } else {
                PythonEnv::Uv
            };
        }
        PlanField::VenvMode => {
            if plan.install_mode != InstallMode::Clean {
                plan.venv_mode = if idx == 0 {
                    VenvMode::Keep
                } else {
                    VenvMode::Recreate
                };
            }
        }
        PlanField::MaiBotBranch => {
            plan.maibot_branch = if idx == 0 { "main" } else { "dev" }.to_string();
        }
        PlanField::GithubProxy => {
            if idx == 0 {
                plan.github_proxy.clear();
            } else if idx == 1 {
                plan.github_proxy = "https://github.com".to_string();
            } else if let Some(mirror) = github_mirrors().get(idx.saturating_sub(2)) {
                plan.github_proxy = (*mirror).to_string();
            }
        }
        PlanField::PipSource => match idx {
            0 => {
                plan.pip_display = "系统默认".to_string();
                plan.pip_index.clear();
                plan.pip_host.clear();
                plan.uv_index.clear();
            }
            1 => set_cached_pip_source(
                plan,
                "阿里云",
                "https://mirrors.aliyun.com/pypi/simple/",
                "mirrors.aliyun.com",
            ),
            2 => set_cached_pip_source(
                plan,
                "腾讯云",
                "http://mirrors.cloud.tencent.com/pypi/simple",
                "mirrors.cloud.tencent.com",
            ),
            3 => set_cached_pip_source(
                plan,
                "清华大学",
                "https://pypi.tuna.tsinghua.edu.cn/simple",
                "pypi.tuna.tsinghua.edu.cn",
            ),
            4 => set_cached_pip_source(
                plan,
                "中国科学技术大学",
                "https://pypi.mirrors.ustc.edu.cn/simple/",
                "pypi.mirrors.ustc.edu.cn",
            ),
            5 => set_cached_pip_source(plan, "官方源", "https://pypi.org/simple", "pypi.org"),
            _ => {}
        },
        PlanField::BotProtocols => {
            plan.bot_protocols = match idx {
                0 => vec![BotProtocol::NapCat],
                1 => vec![BotProtocol::LuckyLilliaBot],
                _ => Vec::new(),
            };
            if !plan.bot_protocols.contains(&BotProtocol::NapCat) {
                plan.docker_mirror = DockerMirror::Keep;
            }
        }
        PlanField::DockerMirror => {
            if plan.bot_protocols.contains(&BotProtocol::NapCat) {
                plan.docker_mirror = match idx {
                    0 => DockerMirror::OneMs,
                    1 => DockerMirror::Xuanyuan,
                    2 => DockerMirror::Official,
                    _ => DockerMirror::Keep,
                };
            } else {
                plan.docker_mirror = DockerMirror::Keep;
            }
        }
    }
}

fn github_proxy_custom_choice_idx(_plan: &InstallPlan) -> usize {
    2 + github_mirrors().len()
}

#[cfg(test)]
fn set_cached_pip_source(plan: &mut InstallPlan, display: &str, index: &str, host: &str) {
    plan.pip_display = display.to_string();
    plan.pip_index = index.to_string();
    plan.pip_host = host.to_string();
    plan.uv_index = index.to_string();
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

fn wrap_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(len as isize) as usize
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
    let term_width = term_width.max(20);
    let max_width = usize::from(term_width).min(PANEL_WIDTH);
    let content_width = max_width.saturating_sub(4).max(10);
    let label_width = if content_width < 34 {
        (content_width / 2).clamp(8, 14)
    } else {
        18
    };
    let detail_width = content_width.saturating_sub(label_width + 3);
    let protected_rows = origin.1.min(if term_height < 12 { 1 } else { 3 });
    let available_from_origin = if origin.1 < term_height {
        usize::from(term_height - origin.1)
    } else {
        0
    };
    let fallback_rows = usize::from(term_height.saturating_sub(protected_rows)).max(1);
    let max_menu_rows = if available_from_origin >= 5 {
        available_from_origin
    } else {
        fallback_rows
    }
    .min(usize::from(term_height))
    .max(1);

    let fixed_rows = 3_usize;
    let mut visible_actions = max_menu_rows
        .saturating_sub(fixed_rows)
        .max(1)
        .min(actions.len());
    let mut action_start = scroll_start(actions.len(), visible_actions, selected);
    for _ in 0..4 {
        let ellipsis_rows = usize::from(action_start > 0)
            + usize::from(action_start + visible_actions < actions.len());
        let next_visible = max_menu_rows
            .saturating_sub(fixed_rows + ellipsis_rows)
            .max(1)
            .min(actions.len());
        let next_start = scroll_start(actions.len(), next_visible, selected);
        if next_visible == visible_actions && next_start == action_start {
            break;
        }
        visible_actions = next_visible;
        action_start = next_start;
    }
    let action_end = (action_start + visible_actions).min(actions.len());
    let has_upper_ellipsis = action_start > 0;
    let has_lower_ellipsis = action_end < actions.len();
    let mut lines = Vec::with_capacity(max_menu_rows);

    lines.push(format!(
        "  {} {}",
        style("▌").yellow().bright().bold(),
        style(prompt).yellow().bright().bold()
    ));
    lines.push(format!(
        "  {}",
        style("─".repeat(content_width)).color256(136).bright()
    ));

    if has_upper_ellipsis {
        lines.push(format!(
            "  {}",
            style(pad_right("... 上方还有操作 ...", content_width))
                .yellow()
                .bright()
        ));
    }
    for (index, action) in actions
        .iter()
        .enumerate()
        .skip(action_start)
        .take(action_end.saturating_sub(action_start))
    {
        let active = index == selected;
        let cursor = if active { "›" } else { " " };
        let title = truncate_display(
            &format!("{} {}", action.marker(), action.label),
            label_width,
        );
        let row = if detail_width > 4 {
            format!(
                "{cursor} {} {}",
                pad_right(&title, label_width),
                truncate_display(action.detail, detail_width)
            )
        } else {
            format!("{cursor} {title}")
        };
        let row = pad_right(&row, content_width);
        lines.push(if active {
            format!("  {}", style(row).black().on_yellow().bright().bold())
        } else {
            format!("  {}", style(row).color256(179).bright())
        });
    }
    if has_lower_ellipsis {
        lines.push(format!(
            "  {}",
            style(pad_right("... 下方还有操作 ...", content_width))
                .yellow()
                .bright()
        ));
    }
    let footer = if let Some((default, remaining)) = timeout_hint {
        format!("{remaining}s 后默认：{}", actions[default].label)
    } else {
        String::new()
    };
    if !footer.is_empty() {
        lines.push(format!(
            "  {}",
            style(truncate_display(&footer, content_width))
                .yellow()
                .bright()
                .bold()
        ));
    }

    let rows_needed = lines.len().max(last_drawn_rows).max(1);
    let printable_rows = lines.len().min(usize::from(term_height));
    let start_row = if available_from_origin >= printable_rows {
        origin.1
    } else {
        term_height.saturating_sub(printable_rows as u16)
    };
    let clear_rows = rows_needed.min(usize::from(term_height));
    let clear_start_row = previous_start_row.min(start_row);
    let clear_end_row = previous_start_row
        .saturating_add(last_drawn_rows as u16)
        .max(start_row.saturating_add(clear_rows as u16))
        .min(term_height);
    for row in clear_start_row..clear_end_row {
        queue!(stdout, MoveTo(0, row), TermClear(ClearType::CurrentLine))
            .context("清理动作菜单行失败")?;
    }
    for (row, line) in lines.iter().take(printable_rows).enumerate() {
        queue!(
            stdout,
            MoveTo(0, start_row.saturating_add(row as u16)),
            Print(line)
        )
        .context("绘制动作菜单行失败")?;
    }
    let active_screen_row = start_row
        .saturating_add(2)
        .saturating_add(u16::from(has_upper_ellipsis))
        .saturating_add(selected.saturating_sub(action_start) as u16)
        .min(term_height.saturating_sub(1));
    queue!(stdout, MoveTo(2, active_screen_row)).context("移动动作菜单光标失败")?;
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
        queue!(stdout, MoveTo(0, row), TermClear(ClearType::CurrentLine))
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
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > limit {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn scroll_start(total: usize, visible: usize, active: usize) -> usize {
    if total <= visible || visible == 0 {
        return 0;
    }
    let half = visible / 2;
    active.saturating_sub(half).min(total - visible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn render_buffer_text<F>(width: u16, height: u16, draw: F) -> String
    where
        F: Fn(&mut Frame<'_>),
    {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame))
            .expect("test render should succeed");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    fn render_buffer<F>(width: u16, height: u16, draw: F) -> ratatui::buffer::Buffer
    where
        F: Fn(&mut Frame<'_>),
    {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame))
            .expect("test render should succeed");
        terminal.backend().buffer().clone()
    }

    fn compact_visible_text(text: &str) -> String {
        text.chars().filter(|ch| !ch.is_whitespace()).collect()
    }

    fn sample_dashboard_view(selected: usize) -> DashboardView {
        let cards = vec![
            DashboardCard {
                id: "core-start",
                icon: "C",
                title: "启动 MaiBot".to_string(),
                subtitle: "核心服务进程状态".to_string(),
                badge: "可启动".to_string(),
                detail: "核心服务详情".to_string(),
                kind: StatusKind::Running,
            },
            DashboardCard {
                id: "napcat",
                icon: "N",
                title: "NapCatQQ".to_string(),
                subtitle: "协议端服务状态".to_string(),
                badge: "未运行".to_string(),
                detail: "协议端详情".to_string(),
                kind: StatusKind::Stopped,
            },
        ];
        DashboardView {
            mode: AppMode::ContentFocused,
            active_tab: DashboardTab::Core,
            focus: DashboardFocus::Content,
            popup: None,
            page_title: "核心服务".to_string(),
            detail_title: cards[selected].title.clone(),
            detail_subtitle: cards[selected].subtitle.clone(),
            detail_lines: vec!["状态: 正常".to_string()],
            detail_choices: Vec::new(),
            action_lines: Vec::new(),
            cards,
            selected,
            background_refresh: false,
            empty_title: "没有匹配项".to_string(),
            empty_detail: "清空筛选后重试".to_string(),
        }
    }

    #[test]
    fn padding_and_truncation_respect_display_columns() {
        let padded = pad_right("LuckyLilliaBot 未运行", 28);
        assert_eq!(display_width(&padded), 28);

        let truncated = truncate_display("󰏗 插件中心：长长长长长长长长", 12);
        assert!(display_width(&truncated) <= 12);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn compact_three_line_blocks_keep_visible_text() {
        let header = render_buffer_text(80, 4, |frame| {
            render_header(frame, Rect::new(0, 0, 80, 4));
        });
        assert!(header.contains(APP_HEADER_TITLE));
        assert!(header.contains(APP_VERSION));

        let popup = DashboardPopup {
            title: "启动 MaiBot".to_string(),
            subtitle: "核心服务进程状态".to_string(),
            lines: vec!["状态: 可启动".to_string()],
            actions: vec![
                "后台启动".to_string(),
                "启动并进入终端".to_string(),
                "取消".to_string(),
            ],
            selected: 0,
        };
        let actions = render_buffer_text(60, 3, |frame| {
            render_popup_actions(frame, Rect::new(0, 0, 60, 3), &popup);
        });
        let actions = compact_visible_text(&actions);
        assert!(actions.contains("启动"));
        assert!(actions.contains("进入终端"));
        assert!(actions.contains("取消"));

        let deploy_view = DashboardView {
            mode: AppMode::ContentFocused,
            active_tab: DashboardTab::Deploy,
            focus: DashboardFocus::Content,
            popup: None,
            page_title: "部署与更新".to_string(),
            detail_title: "安装路径".to_string(),
            detail_subtitle: "当前路径".to_string(),
            detail_lines: Vec::new(),
            detail_choices: Vec::new(),
            action_lines: Vec::new(),
            cards: vec![
                DashboardCard {
                    id: "deploy-path",
                    icon: "P",
                    title: "安装路径".to_string(),
                    subtitle: "当前路径".to_string(),
                    badge: "路径".to_string(),
                    detail: "路径详情".to_string(),
                    kind: StatusKind::Neutral,
                },
                DashboardCard {
                    id: "deploy-branch",
                    icon: "B",
                    title: "MaiBot 分支".to_string(),
                    subtitle: "main".to_string(),
                    badge: "分支".to_string(),
                    detail: "分支详情".to_string(),
                    kind: StatusKind::Neutral,
                },
            ],
            selected: 0,
            background_refresh: false,
            empty_title: String::new(),
            empty_detail: String::new(),
        };
        let steps = render_buffer_text(96, 3, |frame| {
            render_step_bar(frame, Rect::new(0, 0, 96, 3), &deploy_view);
        });
        let steps = compact_visible_text(&steps);
        assert!(steps.contains("路径"));
        assert!(steps.contains("分支"));
    }

    #[test]
    fn warm_palette_matches_dashboard_contract() {
        assert_eq!(BG_BASE, Color::Rgb(22, 17, 12));
        assert_eq!(TEXT_PRIMARY, Color::Rgb(236, 220, 176));
        assert_eq!(ACCENT_PRIMARY, Color::Rgb(226, 152, 28));
        assert_eq!(ACCENT_SECONDARY, Color::Rgb(206, 88, 36));
        assert_eq!(STATUS_OK, Color::Rgb(152, 195, 62));
        assert_eq!(STATUS_WARN, Color::Rgb(226, 192, 44));
        assert_eq!(STATUS_ERROR, Color::Rgb(198, 58, 54));
        assert_eq!(BORDER_MUTED, Color::Rgb(72, 56, 36));
    }

    #[test]
    fn service_card_list_shows_title_and_status() {
        let view = sample_dashboard_view(0);
        let rendered = render_buffer_text(96, 14, |frame| {
            render_table(frame, Rect::new(0, 0, 96, 14), &view, "核心服务管理");
        });
        let visible = compact_visible_text(&rendered);
        assert!(visible.contains("启动MaiBot"));
        assert!(visible.contains("NapCatQQ"));
        assert!(!visible.contains("运行模式"));
        assert!(!visible.contains("快捷操作"));
    }

    #[test]
    fn service_card_selection_uses_high_contrast_highlight() {
        let view = sample_dashboard_view(0);
        let buffer = render_buffer(96, 14, |frame| {
            render_table(frame, Rect::new(0, 0, 96, 14), &view, "核心服务管理");
        });
        let selected_cell = buffer.cell((4, 3)).expect("selected row cell should exist");

        assert_eq!(selected_cell.fg, DARK_TEXT);
        assert_eq!(selected_cell.bg, ACCENT_PRIMARY);
        assert!(selected_cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn sidebar_includes_about_entry() {
        assert_eq!(DashboardTab::SIDEBAR.last(), Some(&DashboardTab::About));
        assert_eq!(
            DashboardTab::SIDEBAR,
            [
                DashboardTab::Overview,
                DashboardTab::Deploy,
                DashboardTab::Core,
                DashboardTab::Protocol,
                DashboardTab::Plugins,
                DashboardTab::Access,
                DashboardTab::About,
            ],
            "sidebar should expose every main menu tab in the intended order"
        );

        let mut view = sample_dashboard_view(0);
        view.active_tab = DashboardTab::About;
        view.focus = DashboardFocus::Sidebar;
        let rendered = render_buffer_text(42, 18, |frame| {
            render_sidebar(frame, Rect::new(0, 0, 42, 18), &view);
        });
        let visible = compact_visible_text(&rendered);
        assert!(visible.contains("概览"));
        assert!(visible.contains("插件中心"));
        assert!(visible.contains("关于"));
    }

    #[test]
    fn about_page_uses_information_layout_not_service_table() {
        let mut view = sample_dashboard_view(0);
        view.active_tab = DashboardTab::About;
        view.page_title = "关于".to_string();
        view.detail_lines = vec![
            "应用: MaiBot-Manager-TUI".to_string(),
            "版本: 0.3.0".to_string(),
            "作者: 清蒸云鸭".to_string(),
            "许可: AGPL-3.0".to_string(),
            "文档: https://docs.meowyun.cn/index.html".to_string(),
        ];
        view.cards = vec![
            DashboardCard {
                id: "version",
                icon: "V",
                title: "MaiBot Manager".to_string(),
                subtitle: "版本 0.3.0".to_string(),
                badge: "版本".to_string(),
                detail: "用于安装、更新和管理 MaiBot。".to_string(),
                kind: StatusKind::Neutral,
            },
            DashboardCard {
                id: "credits",
                icon: "C",
                title: "作者与许可".to_string(),
                subtitle: "清蒸云鸭 · AGPL-3.0".to_string(),
                badge: "许可".to_string(),
                detail: "感谢使用 MaiBot Manager。".to_string(),
                kind: StatusKind::Neutral,
            },
        ];

        let rendered = render_buffer_text(96, 24, |frame| {
            render_content(frame, Rect::new(0, 0, 96, 24), &view);
        });
        let visible = compact_visible_text(&rendered);
        assert!(visible.contains("信息"));
        assert!(visible.contains("详情"));
        assert!(visible.contains("MaiBotManager"));
        assert!(visible.contains("作者与许可"));
        assert!(visible.contains("文档:https://docs.meowyun.cn/index.html"));
        assert!(!visible.contains("服务名称"));
        assert!(!visible.contains("当前状态"));
        assert!(!visible.contains("运行模式"));
        assert!(!visible.contains("打开"));
    }

    #[test]
    fn dashboard_navigation_uses_sidebar_and_content_focus() {
        let view = sample_dashboard_view(0);
        let mut state = DashboardState::default();
        state.active_tab = DashboardTab::Overview;
        state.focus = DashboardFocus::Sidebar;

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Down, KeyModifiers::empty()),
            DashboardInputAction::Rebuild
        );
        assert_eq!(state.active_tab, DashboardTab::Deploy);

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Tab, KeyModifiers::empty()),
            DashboardInputAction::Redraw
        );
        assert_eq!(state.focus, DashboardFocus::Content);
        sync_app_mode(&mut state);
        assert_eq!(state.mode, AppMode::ContentFocused);
    }

    #[test]
    fn content_navigation_uses_cached_redraw_path() {
        let mut view = sample_dashboard_view(0);
        let mut state = DashboardState::default();
        state.active_tab = DashboardTab::Core;
        state.focus = DashboardFocus::Content;

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Down, KeyModifiers::empty()),
            DashboardInputAction::Redraw
        );
        sync_cached_dashboard_view(&mut view, &mut state);
        assert_eq!(state.selected(), 1);
        assert_eq!(view.selected, 1);
        assert_eq!(view.detail_title, "NapCatQQ");
        assert_eq!(view.detail_subtitle, "协议端服务状态");
        assert_eq!(
            view.detail_lines,
            vec!["状态: 未运行".to_string(), "摘要: 协议端详情".to_string()]
        );

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Char('1'), KeyModifiers::CONTROL),
            DashboardInputAction::Redraw
        );
        assert_eq!(state.focus, DashboardFocus::Sidebar);
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Char('q'), KeyModifiers::CONTROL),
            DashboardInputAction::Event(DashboardEvent::Exit)
        );
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Char('c'), KeyModifiers::CONTROL),
            DashboardInputAction::Event(DashboardEvent::Exit)
        );
    }

    #[test]
    fn plugin_page_idle_tick_rebuilds_for_background_status() {
        let mut view = sample_dashboard_view(0);
        let mut state = DashboardState::default();
        view.active_tab = DashboardTab::Plugins;
        view.background_refresh = true;
        state.active_tab = DashboardTab::Plugins;

        assert!(dashboard_idle_should_rebuild(&view, &state));

        state.popup = Some(DashboardPopup {
            title: "操作".to_string(),
            subtitle: String::new(),
            lines: Vec::new(),
            actions: vec!["返回".to_string()],
            selected: 0,
        });
        assert!(!dashboard_idle_should_rebuild(&view, &state));

        state.popup = None;
        view.active_tab = DashboardTab::Core;
        assert!(!dashboard_idle_should_rebuild(&view, &state));
    }

    #[test]
    fn deploy_left_right_switch_steps_and_up_down_adjust_choices() {
        let mut view = DashboardView {
            mode: AppMode::ContentFocused,
            active_tab: DashboardTab::Deploy,
            focus: DashboardFocus::Content,
            popup: None,
            page_title: "部署与更新".to_string(),
            detail_title: "安装路径".to_string(),
            detail_subtitle: "当前路径".to_string(),
            detail_lines: Vec::new(),
            detail_choices: Vec::new(),
            action_lines: Vec::new(),
            cards: vec![
                DashboardCard {
                    id: "deploy-path",
                    icon: "P",
                    title: "安装路径".to_string(),
                    subtitle: "当前路径".to_string(),
                    badge: "路径".to_string(),
                    detail: "路径详情".to_string(),
                    kind: StatusKind::Neutral,
                },
                DashboardCard {
                    id: "deploy-branch",
                    icon: "B",
                    title: "MaiBot 分支".to_string(),
                    subtitle: "main".to_string(),
                    badge: "分支".to_string(),
                    detail: "分支详情".to_string(),
                    kind: StatusKind::Neutral,
                },
                DashboardCard {
                    id: "deploy-mode",
                    icon: "M",
                    title: "模式".to_string(),
                    subtitle: "正常更新/修复".to_string(),
                    badge: "模式".to_string(),
                    detail: "模式详情".to_string(),
                    kind: StatusKind::Neutral,
                },
            ],
            selected: 0,
            background_refresh: false,
            empty_title: String::new(),
            empty_detail: String::new(),
        };
        let mut state = DashboardState::default();
        state.active_tab = DashboardTab::Deploy;
        state.focus = DashboardFocus::Content;
        state.deploy_plan = Some(InstallPlan {
            maibot_branch: "main".to_string(),
            github_proxy: String::new(),
            pip_display: "系统默认".to_string(),
            bot_protocols: vec![BotProtocol::NapCat],
            ..InstallPlan::default()
        });

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Right, KeyModifiers::empty()),
            DashboardInputAction::Redraw
        );
        sync_cached_dashboard_view(&mut view, &mut state);

        assert_eq!(view.selected, 1);
        assert_eq!(view.detail_title, "MaiBot 分支");
        assert_eq!(view.detail_choices.len(), 2);
        assert!(view.detail_choices[0].active);
        assert!(view.detail_choices[0].selected);
        assert_eq!(view.detail_choices[0].label, "main");

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Down, KeyModifiers::empty()),
            DashboardInputAction::Redraw
        );
        sync_cached_dashboard_view(&mut view, &mut state);
        assert!(view.detail_choices[0].active);
        assert!(view.detail_choices[1].selected);
        assert_eq!(
            state
                .deploy_plan
                .as_ref()
                .map(|plan| plan.maibot_branch.as_str()),
            Some("main")
        );
        assert_eq!(view.cards[1].subtitle, "main");

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Enter, KeyModifiers::empty()),
            DashboardInputAction::Event(DashboardEvent::CommitDeployChoice {
                field: PlanField::MaiBotBranch,
                choice_idx: 1,
            })
        );
        apply_cached_planner_choice(
            state.deploy_plan.as_mut().unwrap(),
            PlanField::MaiBotBranch,
            1,
        );
        state.commit_deploy_choice_selection(PlanField::MaiBotBranch, 1);
        sync_cached_dashboard_view(&mut view, &mut state);
        assert!(view.detail_choices[1].active);
        assert!(view.detail_choices[1].selected);
        assert_eq!(
            state
                .deploy_plan
                .as_ref()
                .map(|plan| plan.maibot_branch.as_str()),
            Some("dev")
        );
        assert_eq!(view.cards[1].subtitle, "dev");

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Left, KeyModifiers::empty()),
            DashboardInputAction::Redraw
        );
        sync_cached_dashboard_view(&mut view, &mut state);
        assert_eq!(view.selected, 0);
        assert_eq!(view.detail_title, "安装路径");

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::F(5), KeyModifiers::empty()),
            DashboardInputAction::Event(DashboardEvent::RunDeployPlan)
        );
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Char('r'), KeyModifiers::CONTROL),
            DashboardInputAction::Event(DashboardEvent::ResetDeployPlan)
        );
        state.focus = DashboardFocus::Sidebar;
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::F(5), KeyModifiers::empty()),
            DashboardInputAction::Idle
        );
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Char('r'), KeyModifiers::CONTROL),
            DashboardInputAction::Idle
        );
    }

    #[test]
    fn deploy_cached_choice_keeps_dependent_fields_consistent() {
        let mut plan = InstallPlan {
            bot_protocols: vec![BotProtocol::NapCat],
            docker_mirror: DockerMirror::OneMs,
            pip_display: String::new(),
            pip_index: String::new(),
            ..InstallPlan::default()
        };

        assert!(planner_choice_active_for_plan(
            &plan,
            PlanField::PipSource,
            0
        ));
        apply_cached_planner_choice(&mut plan, PlanField::BotProtocols, 2);

        assert!(plan.bot_protocols.is_empty());
        assert_eq!(plan.docker_mirror, DockerMirror::Keep);
        apply_cached_planner_choice(&mut plan, PlanField::DockerMirror, 0);
        assert_eq!(plan.docker_mirror, DockerMirror::Keep);
    }

    #[test]
    fn deploy_github_arrow_moves_cursor_and_enter_confirms() {
        let mut view = DashboardView {
            mode: AppMode::ContentFocused,
            active_tab: DashboardTab::Deploy,
            focus: DashboardFocus::Content,
            popup: None,
            page_title: "部署与更新".to_string(),
            detail_title: "GitHub".to_string(),
            detail_subtitle: "自动测速".to_string(),
            detail_lines: Vec::new(),
            detail_choices: Vec::new(),
            action_lines: Vec::new(),
            cards: vec![DashboardCard {
                id: "deploy-github",
                icon: "G",
                title: "GitHub".to_string(),
                subtitle: "自动测速".to_string(),
                badge: "单选".to_string(),
                detail: "GitHub 线路".to_string(),
                kind: StatusKind::Neutral,
            }],
            selected: 0,
            background_refresh: false,
            empty_title: String::new(),
            empty_detail: String::new(),
        };
        let mut state = DashboardState::default();
        state.active_tab = DashboardTab::Deploy;
        state.focus = DashboardFocus::Content;
        state.deploy_plan = Some(InstallPlan {
            github_proxy: String::new(),
            ..InstallPlan::default()
        });

        sync_cached_dashboard_view(&mut view, &mut state);
        assert!(view.detail_choices[0].active);
        assert!(view.detail_choices[0].selected);

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Down, KeyModifiers::empty()),
            DashboardInputAction::Redraw
        );
        sync_cached_dashboard_view(&mut view, &mut state);
        assert!(view.detail_choices[0].active);
        assert!(view.detail_choices[1].selected);
        assert_eq!(
            state
                .deploy_plan
                .as_ref()
                .map(|plan| plan.github_proxy.as_str()),
            Some("")
        );

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Enter, KeyModifiers::empty()),
            DashboardInputAction::Event(DashboardEvent::CommitDeployChoice {
                field: PlanField::GithubProxy,
                choice_idx: 1,
            })
        );
    }

    #[test]
    fn deploy_github_first_mirror_has_only_one_active_check() {
        let mirror = github_mirrors()
            .first()
            .expect("at least one GitHub mirror should be configured");
        let plan = InstallPlan {
            github_proxy: (*mirror).to_string(),
            ..InstallPlan::default()
        };
        let choices = planner_choices_for_plan(&plan, PlanField::GithubProxy);
        let active_indices = (0..choices.len())
            .filter(|idx| planner_choice_active_for_plan(&plan, PlanField::GithubProxy, *idx))
            .collect::<Vec<_>>();

        assert_eq!(choices.get(2).map(String::as_str), Some(*mirror));
        assert_eq!(active_indices, vec![2]);
        assert!(!planner_choice_active_for_plan(
            &plan,
            PlanField::GithubProxy,
            0
        ));
        assert!(!planner_choice_active_for_plan(
            &plan,
            PlanField::GithubProxy,
            1
        ));
    }

    #[test]
    fn content_enter_requests_popup_then_popup_can_activate() {
        let view = sample_dashboard_view(0);
        let mut state = DashboardState::default();
        state.active_tab = DashboardTab::Core;
        state.focus = DashboardFocus::Content;

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Enter, KeyModifiers::empty()),
            DashboardInputAction::OpenPopup
        );
        state.popup = popup_for_selection(&view);
        assert!(state.popup.is_some());
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Right, KeyModifiers::empty()),
            DashboardInputAction::Redraw
        );
        // With new core-start popup: ["后台启动", "启动并进入终端", "取消"]
        // Right moves from idx 0 to idx 1 ("启动并进入终端")
        assert_eq!(state.popup.as_ref().map(|popup| popup.selected), Some(1));
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Enter, KeyModifiers::empty()),
            DashboardInputAction::Event(DashboardEvent::Activate)
        );
    }

    #[test]
    fn about_page_enter_stays_read_only_without_popup() {
        let mut view = sample_dashboard_view(0);
        view.active_tab = DashboardTab::About;
        let mut state = DashboardState::default();
        state.active_tab = DashboardTab::About;
        state.focus = DashboardFocus::Content;

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Enter, KeyModifiers::empty()),
            DashboardInputAction::Idle
        );
        assert!(popup_for_selection(&view).is_none());
    }

    #[test]
    fn protocol_popup_keeps_unavailable_platform_actions_clear() {
        let mut view = sample_dashboard_view(0);
        view.active_tab = DashboardTab::Protocol;
        view.cards = vec![DashboardCard {
            id: "napcat-note",
            icon: "N",
            title: "NapCatQQ".to_string(),
            subtitle: "macOS 版目前仅提供说明入口".to_string(),
            badge: "说明".to_string(),
            detail: "会清晰提示当前平台限制，不显示不可执行的操作。".to_string(),
            kind: StatusKind::Warning,
        }];
        view.selected = 0;
        let unavailable = popup_for_selection(&view).expect("unavailable protocol popup");
        assert_eq!(unavailable.actions, vec!["查看说明", "取消"]);

        view.cards[0].id = "napcat";
        let supported = popup_for_selection(&view).expect("supported protocol popup");
        assert_eq!(
            supported.actions,
            vec!["启动", "停止", "重启", "查看日志", "重建容器", "取消"]
        );
    }

    #[test]
    fn info_cards_open_inline_without_leaving_dashboard() {
        let mut view = sample_dashboard_view(0);
        view.active_tab = DashboardTab::Access;
        view.cards = vec![DashboardCard {
            id: "access-summary",
            icon: "A",
            title: "访问汇总".to_string(),
            subtitle: "MaiBot WebUI".to_string(),
            badge: "可查看".to_string(),
            detail: "集中查看访问入口。".to_string(),
            kind: StatusKind::Neutral,
        }];
        view.selected = 0;

        let mut state = DashboardState::default();
        state.active_tab = DashboardTab::Access;
        state.focus = DashboardFocus::Content;

        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Enter, KeyModifiers::empty()),
            DashboardInputAction::OpenInlineInfoPopup
        );

        view.cards[0].id = "access-note";
        let popup = popup_for_selection(&view).expect("access note popup action");
        assert_eq!(popup.actions, vec!["查看说明", "取消"]);
        assert_eq!(
            handle_dashboard_key(&mut state, &view, KeyCode::Enter, KeyModifiers::empty()),
            DashboardInputAction::OpenInlineInfoPopup
        );
    }

    #[test]
    fn access_clear_data_uses_confirmation_popup() {
        let mut view = sample_dashboard_view(0);
        view.active_tab = DashboardTab::Access;
        view.cards = vec![DashboardCard {
            id: "access-clear-data",
            icon: "D",
            title: "清空数据文件".to_string(),
            subtitle: "保留 webui.json，清理 MaiBot/data".to_string(),
            badge: "需确认".to_string(),
            detail: "删除 MaiBot/data 下除 webui.json 外的文件和子目录。".to_string(),
            kind: StatusKind::Warning,
        }];
        view.selected = 0;

        assert!(!direct_info_popup_card(&view));
        let popup = popup_for_selection(&view).expect("clear data should open a popup");
        assert_eq!(popup.actions, vec!["确认清空数据", "取消"]);
        assert!(popup.lines.iter().any(|line| line.contains("webui.json")));
    }

    #[test]
    fn popup_area_tracks_content_without_half_screen_sprawl() {
        let popup = DashboardPopup {
            title: "访问汇总".to_string(),
            subtitle: "集中查看 MaiBot、NapCat 与 LLBot 的访问入口".to_string(),
            lines: vec![
                "本机 / 公网 IP 127.0.0.1".to_string(),
                String::new(),
                "MaiBot WebUI".to_string(),
                "地址 http://127.0.0.1:8001".to_string(),
                "密钥 token".to_string(),
            ],
            actions: vec!["取消".to_string()],
            selected: 0,
        };
        let area = popup_area(&popup, Rect::new(0, 0, 132, 42));
        assert!(area.width <= 72, "info popup should stay compact: {area:?}");
        assert!(
            area.height <= 14,
            "info popup should fit its content: {area:?}"
        );
        assert_eq!(area.x, (132 - area.width) / 2);

        let rendered = render_buffer_text(132, 42, |frame| {
            render_popup(frame, area, &popup);
        });
        let visible = compact_visible_text(&rendered);
        assert!(visible.contains("访问汇总"));
        assert!(visible.contains("MaiBotWebUI"));
        assert!(visible.contains("取消"));
    }

    #[test]
    fn inline_info_loading_popup_has_no_actions_or_legacy_prompt() {
        let mut view = sample_dashboard_view(0);
        view.active_tab = DashboardTab::Access;
        view.cards = vec![DashboardCard {
            id: "access-summary",
            icon: "A",
            title: "访问汇总".to_string(),
            subtitle: "MaiBot WebUI".to_string(),
            badge: "可查看".to_string(),
            detail: "集中查看访问入口。".to_string(),
            kind: StatusKind::Neutral,
        }];
        view.selected = 0;

        let popup = inline_info_loading_popup(&view);
        assert_eq!(popup.title, "访问汇总");
        assert_eq!(popup.subtitle, "正在整理访问入口");
        assert!(popup.actions.is_empty());
        assert!(!popup.lines.join("\n").contains("按回车返回"));
    }

    #[test]
    fn popup_actions_are_never_empty() {
        let mut actions = Vec::new();
        ensure_popup_actions(&mut actions);
        assert_eq!(actions, vec!["打开", "取消"]);

        let mut actions = vec!["".to_string(), "启动".to_string()];
        ensure_popup_actions(&mut actions);
        assert_eq!(actions, vec!["启动", "取消"]);
    }

    #[test]
    fn wrap_index_handles_empty_and_edges() {
        assert_eq!(wrap_index(0, 0, 1), 0);
        assert_eq!(wrap_index(0, 4, -1), 3);
        assert_eq!(wrap_index(3, 4, 1), 0);
    }
}
