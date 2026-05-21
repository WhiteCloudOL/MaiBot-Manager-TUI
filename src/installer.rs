use crate::{app::App, model::*, terminal::{TerminalUiGuard, restore_terminal_state}, utils::*};
use anyhow::{anyhow, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use dialoguer::console::style;
use dialoguer::{Confirm, Input, Select};
use serde_json::Value;
use std::{fmt, fs, path::{Path, PathBuf}, process::Command, thread, time::Duration};

/// 用户主动取消的标记错误。沿用 anyhow 链向上传播，由 install_update_flow
/// 捕获后转成温和提示返回主菜单，不弹红色 Error。
#[derive(Debug)]
pub(crate) struct UserCanceled(pub String);

impl fmt::Display for UserCanceled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UserCanceled {}

impl App {
    /// 在 destructive 提示出现之前清空 tty 输入缓冲。
    /// 用户上一次多按的 Enter 不应被下一个 prompt 立刻吃掉默认项。
    pub(crate) fn drain_pending_input(&self) {
        thread::sleep(Duration::from_millis(120));
        if crossterm::terminal::enable_raw_mode().is_err() {
            return;
        }
        while let Ok(true) = crossterm::event::poll(Duration::from_millis(0)) {
            if crossterm::event::read().is_err() {
                break;
            }
        }
        // 兜底再扫一轮，捕获稍慢一拍才到达的回车
        while let Ok(true) = crossterm::event::poll(Duration::from_millis(20)) {
            if crossterm::event::read().is_err() {
                break;
            }
        }
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

impl App {
    pub(crate) fn install_update_flow(&mut self) -> Result<()> {
        let current = self.load_config().unwrap_or_default();
        let mut plan = self.build_default_install_plan(&current)?;
        let should_install = self.install_planner(&current, &mut plan)?;
        if should_install {
            match self.run_install(&plan) {
                Ok(()) => {}
                Err(e) if e.downcast_ref::<UserCanceled>().is_some() => {
                    println!();
                    println!("  {} {}", style("✕").yellow(), style(e.to_string()).yellow());
                    println!("  {}", style("（已返回主菜单，未执行任何破坏性操作）").dim());
                }
                Err(e) => return Err(e),
            }
        }
        self.pause("安装流程结束，按回车返回主菜单")?;
        Ok(())
    }

    pub(crate) fn install_planner(&mut self, current: &AppConfig, plan: &mut InstallPlan) -> Result<bool> {
        let _guard = TerminalUiGuard::enter()?;
        let mut target: Option<PlannerEntry> = None;

        loop {
            let expanded = match &target {
                Some(PlannerEntry::Field(f)) => Some(*f),
                Some(PlannerEntry::Choice(f, _)) => Some(*f),
                _ => None,
            };
            let entries = self.build_planner_entries(plan, expanded);
            let mut selected = if let Some(t) = &target {
                entries.iter().position(|e| e == t).unwrap_or(0)
            } else {
                0
            };
            if selected >= entries.len() {
                selected = entries.len().saturating_sub(1);
            }

            self.clear();
            self.print_header(None);
            self.print_planner_view(plan, &entries, selected, expanded);

            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
                {
                    restore_terminal_state();
                    eprintln!("\n安装已被用户中断 (Ctrl+C)");
                    std::process::exit(130);
                }
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up => {
                        let next = selected.saturating_sub(1);
                        target = entries.get(next).cloned();
                    }
                    KeyCode::Down => {
                        let next = if selected + 1 < entries.len() {
                            selected + 1
                        } else {
                            selected
                        };
                        target = entries.get(next).cloned();
                    }
                    KeyCode::Home => {
                        target = entries.first().cloned();
                    }
                    KeyCode::End => {
                        target = entries.last().cloned();
                    }
                    KeyCode::Enter => match entries.get(selected).cloned() {
                        Some(PlannerEntry::Field(field)) => {
                            target = Some(PlannerEntry::Field(field));
                            if field == PlanField::InstallPath {
                                self.edit_install_path(plan)?;
                            }
                        }
                        Some(PlannerEntry::Choice(field, choice_idx)) => {
                            self.apply_planner_choice(current, plan, field, choice_idx)?;
                            target = Some(PlannerEntry::Choice(field, choice_idx));
                        }
                        Some(PlannerEntry::Action(PlanAction::StartInstall)) => {
                            return Ok(true);
                        }
                        Some(PlannerEntry::Action(PlanAction::ResetDefaults)) => {
                            *plan = self.build_default_install_plan(current)?;
                            target = None;
                        }
                        Some(PlannerEntry::Action(PlanAction::BackToMenu)) => {
                            return Ok(false);
                        }
                        None => {}
                    },
                    KeyCode::Esc => return Ok(false),
                    _ => {}
                }
            }
        }
    }

    pub(crate) fn build_planner_entries(
        &self,
        plan: &InstallPlan,
        expanded: Option<PlanField>,
    ) -> Vec<PlannerEntry> {
        let fields = [
            PlanField::InstallPath,
            PlanField::InstallMode,
            PlanField::PythonEnv,
            PlanField::VenvMode,
            PlanField::GithubProxy,
            PlanField::PipSource,
            PlanField::BotProtocols,
            PlanField::DockerMirror,
        ];

        let mut entries = Vec::new();
        for field in fields {
            entries.push(PlannerEntry::Field(field));
            if expanded == Some(field) {
                for idx in 0..self.planner_choices(plan, field).len() {
                    entries.push(PlannerEntry::Choice(field, idx));
                }
            }
        }
        entries.push(PlannerEntry::Action(PlanAction::StartInstall));
        entries.push(PlannerEntry::Action(PlanAction::ResetDefaults));
        entries.push(PlannerEntry::Action(PlanAction::BackToMenu));
        entries
    }

    pub(crate) fn planner_choices(&self, plan: &InstallPlan, field: PlanField) -> Vec<String> {
        match field {
            PlanField::InstallPath => vec!["按 Enter 输入自定义路径".into()],
            PlanField::InstallMode => vec!["正常更新/修复".into(), "全新安装（清空目标目录）".into()],
            PlanField::PythonEnv => vec!["本机 python3".into(), "uv (Python 3.14)".into()],
            PlanField::VenvMode => {
                if plan.install_mode == InstallMode::Clean {
                    vec!["全新安装时固定为：删除并重建环境".into()]
                } else if plan.python_env == PythonEnv::Uv {
                    vec!["保留现有 .venv".into(), "删除并重建 .venv".into()]
                } else {
                    vec!["保留现有环境".into(), "删除并重建环境".into()]
                }
            }
            PlanField::GithubProxy => {
                let mut items = vec!["自动测速选择最佳线路".into(), "官方直连".into()];
                items.extend(github_mirrors().iter().map(|v| (*v).to_string()));
                items.push("自定义镜像源".into());
                items
            }
            PlanField::PipSource => vec![
                "系统默认".into(),
                "阿里云".into(),
                "腾讯云".into(),
                "清华大学".into(),
                "中国科学技术大学".into(),
                "官方源".into(),
                "自定义镜像源".into(),
            ],
            PlanField::BotProtocols => vec![
                "仅 NapCatQQ".into(),
                "仅 LuckyLilliaBot".into(),
                "暂不安装附加协议端".into(),
            ],
            PlanField::DockerMirror => vec![
                "docker.1ms.run".into(),
                "docker.xuanyuan.me".into(),
                "官方源".into(),
                "保持不变".into(),
            ],
        }
    }

    pub(crate) fn planner_field_label(&self, field: PlanField) -> &'static str {
        match field {
            PlanField::InstallPath => "目录",
            PlanField::InstallMode => "模式",
            PlanField::PythonEnv => "Python",
            PlanField::VenvMode => "环境",
            PlanField::GithubProxy => "GitHub",
            PlanField::PipSource => "PyPI",
            PlanField::BotProtocols => "协议端",
            PlanField::DockerMirror => "Docker",
        }
    }

    pub(crate) fn planner_field_value(&self, plan: &InstallPlan, field: PlanField) -> String {
        match field {
            PlanField::InstallPath => plan.install_path.display().to_string(),
            PlanField::InstallMode => plan.install_mode.label().to_string(),
            PlanField::PythonEnv => plan.python_env.label().to_string(),
            PlanField::VenvMode => plan.venv_mode.label(plan.python_env).to_string(),
            PlanField::GithubProxy => {
                if plan.github_proxy.is_empty() {
                    "自动测速（执行时选择最佳线路）".into()
                } else {
                    plan.github_proxy.clone()
                }
            }
            PlanField::PipSource => {
                if plan.pip_display.is_empty() {
                    "系统默认".into()
                } else {
                    plan.pip_display.clone()
                }
            }
            PlanField::BotProtocols => {
                if plan.bot_protocols.is_empty() {
                    "暂不安装".into()
                } else {
                    plan.bot_protocols
                        .iter()
                        .map(|v| v.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            }
            PlanField::DockerMirror => plan.docker_mirror.label().to_string(),
        }
    }

    pub(crate) fn planner_choice_active(&self, plan: &InstallPlan, field: PlanField, choice_idx: usize) -> bool {
        match field {
            PlanField::InstallPath => false,
            PlanField::InstallMode => {
                matches!(
                    (choice_idx, plan.install_mode),
                    (0, InstallMode::Normal) | (1, InstallMode::Clean)
                )
            }
            PlanField::PythonEnv => {
                matches!(
                    (choice_idx, plan.python_env),
                    (0, PythonEnv::System) | (1, PythonEnv::Uv)
                )
            }
            PlanField::VenvMode => {
                matches!(
                    (choice_idx, plan.venv_mode),
                    (0, VenvMode::Keep) | (1, VenvMode::Recreate)
                )
            }
            PlanField::GithubProxy => {
                if choice_idx == 0 {
                    plan.github_proxy.is_empty()
                } else if choice_idx == 1 {
                    plan.github_proxy == "https://github.com"
                } else if choice_idx >= 2 && choice_idx < 2 + github_mirrors().len() {
                    plan.github_proxy == github_mirrors()[choice_idx - 2]
                } else {
                    !plan.github_proxy.is_empty()
                        && plan.github_proxy != "https://github.com"
                        && !github_mirrors().iter().any(|mirror| *mirror == plan.github_proxy)
                }
            }
            PlanField::PipSource => match choice_idx {
                0 => plan.pip_index.is_empty(),
                1 => plan.pip_display == "阿里云",
                2 => plan.pip_display == "腾讯云",
                3 => plan.pip_display == "清华大学",
                4 => plan.pip_display == "中国科学技术大学",
                5 => plan.pip_display == "官方源",
                _ => !plan.pip_index.is_empty()
                    && !["阿里云", "腾讯云", "清华大学", "中国科学技术大学", "官方源"]
                        .contains(&plan.pip_display.as_str()),
            },
            PlanField::BotProtocols => match choice_idx {
                0 => plan.bot_protocols == vec![BotProtocol::NapCat],
                1 => plan.bot_protocols == vec![BotProtocol::LuckyLilliaBot],
                _ => plan.bot_protocols.is_empty(),
            },
            PlanField::DockerMirror => {
                matches!(
                    (choice_idx, plan.docker_mirror),
                    (0, DockerMirror::OneMs)
                        | (1, DockerMirror::Xuanyuan)
                        | (2, DockerMirror::Official)
                        | (3, DockerMirror::Keep)
                )
            }
        }
    }

    pub(crate) fn print_planner_view(
        &self,
        plan: &InstallPlan,
        entries: &[PlannerEntry],
        selected: usize,
        expanded: Option<PlanField>,
    ) {
        self.print_section("安装计划", "↑/↓ 移动 · Enter 展开或应用 · Esc 返回");
        let mut printed_actions = false;
        for (idx, entry) in entries.iter().enumerate() {
            let active = idx == selected;
            match entry {
                PlannerEntry::Field(field) => {
                    let cursor = if active { "▶" } else { " " };
                    let is_expanded = expanded == Some(*field);
                    let expand_mark = if is_expanded { "▾" } else { "▸" };
                    let label = self.planner_field_label(*field);
                    let padded_label = pad_left(label, 8);
                    let value = self.planner_field_value(plan, *field);
                    let line = format!("  {cursor} {expand_mark} {padded_label}  {value}");
                    if active {
                        print!("{}\r\n", style(line).cyan().bold());
                    } else if is_expanded {
                        print!("{}\r\n", style(line).cyan());
                    } else {
                        print!("{}\r\n", style(line).white());
                    }
                }
                PlannerEntry::Choice(field, choice_idx) => {
                    let choice = &self.planner_choices(plan, *field)[*choice_idx];
                    let current = self.planner_choice_active(plan, *field, *choice_idx);
                    let marker = if current { "●" } else { "○" };
                    let cursor = if active { "▶" } else { " " };
                    let line = format!("      {cursor} {marker} {choice}");
                    if active {
                        print!("{}\r\n", style(line).green().bold());
                    } else if current {
                        print!("{}\r\n", style(line).cyan());
                    } else {
                        print!("{}\r\n", style(line).dim());
                    }
                }
                PlannerEntry::Action(action) => {
                    if !printed_actions {
                        self.print_line();
                        printed_actions = true;
                    }
                    let label = match action {
                        PlanAction::StartInstall => "▶ 开始安装 / 更新",
                        PlanAction::ResetDefaults => "↺ 恢复推荐默认",
                        PlanAction::BackToMenu => "← 返回主菜单",
                    };
                    let cursor = if active { "▶" } else { " " };
                    let text = format!("  {cursor} {label}");
                    let styled = match action {
                        PlanAction::StartInstall => style(text).green(),
                        PlanAction::ResetDefaults => style(text).yellow(),
                        PlanAction::BackToMenu => style(text).red(),
                    };
                    if active {
                        print!("{}\r\n", styled.bold());
                    } else {
                        print!("{}\r\n", styled.dim());
                    }
                }
            }
        }
        self.print_line();
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }

    pub(crate) fn apply_planner_choice(
        &mut self,
        current: &AppConfig,
        plan: &mut InstallPlan,
        field: PlanField,
        choice_idx: usize,
    ) -> Result<()> {
        match field {
            PlanField::InstallPath => self.edit_install_path(plan)?,
            PlanField::InstallMode => {
                plan.install_mode = if choice_idx == 1 {
                    InstallMode::Clean
                } else {
                    InstallMode::Normal
                };
                if plan.install_mode == InstallMode::Clean {
                    plan.venv_mode = VenvMode::Recreate;
                }
            }
            PlanField::PythonEnv => {
                plan.python_env = if choice_idx == 1 {
                    PythonEnv::Uv
                } else {
                    PythonEnv::System
                };
            }
            PlanField::VenvMode => {
                if plan.install_mode != InstallMode::Clean {
                    plan.venv_mode = if choice_idx == 1 {
                        VenvMode::Recreate
                    } else {
                        VenvMode::Keep
                    };
                }
            }
            PlanField::GithubProxy => match choice_idx {
                0 => plan.github_proxy.clear(),
                1 => plan.github_proxy = "https://github.com".into(),
                idx if idx >= 2 && idx < 2 + github_mirrors().len() => {
                    plan.github_proxy = github_mirrors()[idx - 2].to_string();
                }
                _ => {
                    let input: String = self.with_prompt_mode(|| {
                        Input::with_theme(&self.theme)
                            .with_prompt("输入自定义镜像源")
                            .interact_text()
                            .map_err(Into::into)
                    })?;
                    plan.github_proxy = normalize_url(&input);
                }
            },
            PlanField::PipSource => match choice_idx {
                0 => {
                    plan.pip_display = "系统默认".into();
                    plan.pip_index.clear();
                    plan.pip_host.clear();
                    plan.uv_index.clear();
                }
                1 => {
                    plan.pip_display = "阿里云".into();
                    plan.pip_index = "https://mirrors.aliyun.com/pypi/simple/".into();
                    plan.pip_host = "mirrors.aliyun.com".into();
                    plan.uv_index = plan.pip_index.clone();
                }
                2 => {
                    plan.pip_display = "腾讯云".into();
                    plan.pip_index = "http://mirrors.cloud.tencent.com/pypi/simple".into();
                    plan.pip_host = "mirrors.cloud.tencent.com".into();
                    plan.uv_index = plan.pip_index.clone();
                }
                3 => {
                    plan.pip_display = "清华大学".into();
                    plan.pip_index = "https://pypi.tuna.tsinghua.edu.cn/simple".into();
                    plan.pip_host = "pypi.tuna.tsinghua.edu.cn".into();
                    plan.uv_index = plan.pip_index.clone();
                }
                4 => {
                    plan.pip_display = "中国科学技术大学".into();
                    plan.pip_index = "https://pypi.mirrors.ustc.edu.cn/simple/".into();
                    plan.pip_host = "pypi.mirrors.ustc.edu.cn".into();
                    plan.uv_index = plan.pip_index.clone();
                }
                5 => {
                    plan.pip_display = "官方源".into();
                    plan.pip_index = "https://pypi.org/simple".into();
                    plan.pip_host = "pypi.org".into();
                    plan.uv_index = plan.pip_index.clone();
                }
                _ => {
                    let custom: String = self.with_prompt_mode(|| {
                        Input::with_theme(&self.theme)
                            .with_prompt("自定义 PyPI 镜像源")
                            .interact_text()
                            .map_err(Into::into)
                    })?;
                    let custom = normalize_url(&custom);
                    plan.pip_display = custom.clone();
                    plan.pip_host = custom
                        .split('/')
                        .nth(2)
                        .unwrap_or_default()
                        .to_string();
                    plan.pip_index = custom;
                    plan.uv_index = plan.pip_index.clone();
                }
            },
            PlanField::BotProtocols => {
                plan.bot_protocols = match choice_idx {
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
                    plan.docker_mirror = match choice_idx {
                        0 => DockerMirror::OneMs,
                        1 => DockerMirror::Xuanyuan,
                        2 => DockerMirror::Official,
                        _ => DockerMirror::Keep,
                    };
                } else {
                    self.with_prompt_mode(|| self.pause("当前未选择 NapCatQQ，无需配置 Docker 镜像；按回车继续"))?;
                }
            }
        }

        if matches!(field, PlanField::InstallMode | PlanField::PythonEnv)
            && plan.install_mode == InstallMode::Clean
        {
            plan.venv_mode = VenvMode::Recreate;
        }
        if matches!(field, PlanField::InstallPath) && plan.install_path.as_os_str().is_empty() {
            *plan = self.build_default_install_plan(current)?;
        }
        Ok(())
    }

    pub(crate) fn build_default_install_plan(&self, current: &AppConfig) -> Result<InstallPlan> {
        let install_path = if !current.mai_path.is_empty() {
            PathBuf::from(&current.mai_path)
        } else {
            dirs::home_dir()
                .ok_or_else(|| anyhow!("无法定位 HOME 目录"))?
                .join("maimai")
        };
        let bot_protocols = vec![BotProtocol::NapCat];

        Ok(InstallPlan {
            install_path,
            install_mode: InstallMode::Normal,
            python_env: if current.mai_python_env == "system" {
                PythonEnv::System
            } else {
                PythonEnv::Uv
            },
            venv_mode: VenvMode::Keep,
            github_proxy: String::new(),
            pip_display: "系统默认".into(),
            pip_index: String::new(),
            pip_host: String::new(),
            uv_index: String::new(),
            bot_protocols,
            docker_mirror: DockerMirror::Keep,
        })
    }

    pub(crate) fn edit_install_path(&self, plan: &mut InstallPlan) -> Result<()> {
        let path_input: String = self.with_prompt_mode(|| {
            Input::with_theme(&self.theme)
                .with_prompt("安装目录")
                .default(plan.install_path.display().to_string())
                .interact_text()
                .map_err(Into::into)
        })?;
        plan.install_path = normalize_path(&path_input)?;
        fs::create_dir_all(&plan.install_path)?;
        Ok(())
    }

    pub(crate) fn run_github_speedtest(&self) -> Result<String> {
        let mut mirrors = vec!["https://github.com".to_string()];
        mirrors.extend(github_mirrors().iter().map(|v| v.to_string()));
        println!("  {}", style("正在并行测速，请稍候...").dim());
        self.print_line();
        let handles: Vec<_> = mirrors
            .into_iter()
            .map(|mirror| {
                thread::spawn(move || {
                    let test_url = if mirror == "https://github.com" {
                        TEST_FILE_PATH.to_string()
                    } else {
                        format!("{mirror}/{TEST_FILE_PATH}")
                    };
                    let output = Command::new("bash")
                        .arg("-lc")
                        .arg(format!(
                            "curl -sL -o /dev/null --max-time 5 --connect-timeout 3 -w '%{{time_total}}' '{}'",
                            test_url
                        ))
                        .output();

                    match output {
                        Ok(output) if output.status.success() => {
                            let time = String::from_utf8_lossy(&output.stdout)
                                .trim()
                                .parse::<f64>()
                                .unwrap_or(9.999);
                            (mirror, time * 1000.0, true)
                        }
                        _ => (mirror, 9999.0, false),
                    }
                })
            })
            .collect();

        let mut results = Vec::new();
        for handle in handles {
            results.push(
                handle
                    .join()
                    .map_err(|_| anyhow!("GitHub 并行测速线程异常退出"))?,
            );
        }
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut best: Option<(String, f64)> = None;
        for (mirror, ms, ok) in &results {
            if *ok {
                println!("  {} {}", style(format!("{:>6.0} ms", ms)).green(), mirror);
                if best.as_ref().map(|b| *ms < b.1).unwrap_or(true) {
                    best = Some((mirror.clone(), *ms));
                }
            } else {
                println!("  {}     {}", style("  失败 ").red(), style(mirror).dim());
            }
        }
        self.print_line();

        match best {
            Some((url, ms)) => {
                println!("  {} {} ({:.0} ms)", style("✔ 已选择").green().bold(), style(&url).cyan(), ms);
                Ok(url)
            }
            None => {
                println!("  {}", style("✗ 全部线路连接失败").red().bold());
                let choice = self.with_prompt_mode(|| {
                    Select::with_theme(&self.theme)
                        .with_prompt("请选择回退方案")
                        .items(["重试测速", "使用 GitHub 官方直连", "取消安装"])
                        .default(0)
                        .interact()
                        .map_err(Into::into)
                })?;
                match choice {
                    0 => self.run_github_speedtest(),
                    1 => Ok("https://github.com".to_string()),
                    _ => Err(anyhow!("用户取消安装")),
                }
            }
        }
    }

    pub(crate) fn ensure_base_tools(&self, plan: &InstallPlan) -> Result<()> {
        let mut needed = Vec::<&str>::new();
        for tool in ["git", "curl", "screen", "unzip", "python3"] {
            if !command_exists(tool)? {
                needed.push(tool);
            }
        }
        if plan.bot_protocols.contains(&BotProtocol::LuckyLilliaBot) && !command_exists("wget")? {
            needed.push("wget");
        }
        if needed.is_empty() {
            return Ok(());
        }

        let pm = PkgManager::detect();
        self.clear();
        self.print_header(Some(plan));
        self.print_section(
            "依赖检查",
            &format!("检测到包管理器：{}", pm.label()),
        );
        self.print_kv("待安装", &needed.join(" "));
        self.print_line();

        if let Some(cmd) = pm.install_cmd(&needed) {
            self.run_shell(&cmd)?;
        } else {
            self.print_hint(
                "未识别系统包管理器，请手动安装上述工具后重试 (apt/dnf/yum/pacman/zypper/apk)。",
            );
            anyhow::bail!("缺少基础工具且无法自动安装: {}", needed.join(" "));
        }
        Ok(())
    }

    pub(crate) fn run_install(&self, plan: &InstallPlan) -> Result<()> {
        let mut plan = plan.clone();
        if plan.install_mode == InstallMode::Clean {
            plan.venv_mode = VenvMode::Recreate;
        }
        self.ensure_base_tools(&plan)?;
        if plan.github_proxy.is_empty() {
            self.clear();
            self.print_header(Some(&plan));
            self.print_section("GitHub 线路测速", "未手动指定线路，正在自动测速并选择最佳线路");
            plan.github_proxy = self.run_github_speedtest()?;
        }

        if plan.install_mode == InstallMode::Clean {
            clean_install_dir(&plan.install_path)?;
        }
        fs::create_dir_all(&plan.install_path)?;
        if plan.bot_protocols.contains(&BotProtocol::NapCat) {
            self.prepare_docker(&plan)?;
        }
        self.clone_or_update_repo(
            &repo_url(&plan.github_proxy, "MaiM-with-u/MaiBot"),
            &plan.install_path.join("MaiBot"),
            Some("main"),
            plan.install_mode,
        )?;
        self.clone_or_update_repo(
            &repo_url(
                &plan.github_proxy,
                "MaiM-with-u/MaiBot-Napcat-Adapter",
            ),
            &plan
                .install_path
                .join("MaiBot")
                .join("plugins")
                .join("MaiBot-Napcat-Adapter"),
            Some("main"),
            plan.install_mode,
        )?;
        self.setup_python_env(&plan)?;
        self.save_config(&AppConfig {
            user_install_path: plan.install_path.display().to_string(),
            mai_path: plan.install_path.display().to_string(),
            mai_python_env: match plan.python_env {
                PythonEnv::Uv => "uv".into(),
                PythonEnv::System => "system".into(),
            },
            mai_llbot_path: plan.install_path.join("LLBot").display().to_string(),
        })?;
        if plan.bot_protocols.contains(&BotProtocol::NapCat) {
            self.install_napcat(&plan)?;
        }
        if plan.bot_protocols.contains(&BotProtocol::LuckyLilliaBot) {
            self.install_llbot(&plan)?;
        }
        Ok(())
    }

    pub(crate) fn prepare_docker(&self, plan: &InstallPlan) -> Result<()> {
        if !command_exists("docker")? {
            let pm = PkgManager::detect();
            let installed = match pm {
                PkgManager::Apt | PkgManager::Dnf | PkgManager::Yum => {
                    self.run_shell(DOCKER_ONELINER).is_ok()
                }
                PkgManager::Pacman => {
                    self.run_shell("sudo pacman -Sy --noconfirm --needed docker docker-compose").is_ok()
                }
                PkgManager::Zypper => {
                    self.run_shell("sudo zypper --non-interactive install docker docker-compose && sudo systemctl enable --now docker").is_ok()
                }
                PkgManager::Apk => {
                    self.run_shell("sudo apk add --no-cache docker docker-compose && sudo rc-update add docker boot && sudo service docker start").is_ok()
                }
                PkgManager::Unknown => false,
            };
            if !installed {
                anyhow::bail!("Docker 安装失败或当前发行版未自动适配，请手动安装 Docker 后重试");
            }
        }
        match plan.docker_mirror {
            DockerMirror::OneMs => {
                self.configure_docker_daemon(Some("https://docker.1ms.run"))?;
            }
            DockerMirror::Xuanyuan => {
                self.configure_docker_daemon(Some("https://docker.xuanyuan.me"))?;
            }
            DockerMirror::Official => {
                self.configure_docker_daemon(None)?;
            }
            DockerMirror::Keep => {}
        }
        Ok(())
    }

    pub(crate) fn configure_docker_daemon(&self, mirror: Option<&str>) -> Result<()> {
        let content = match mirror {
            Some(url) => format!("{{\"registry-mirrors\":[\"{url}\"]}}"),
            None => "{}".to_string(),
        };
        let restart = "if command -v systemctl >/dev/null 2>&1; then sudo systemctl restart docker; elif command -v rc-service >/dev/null 2>&1; then sudo rc-service docker restart; elif command -v service >/dev/null 2>&1; then sudo service docker restart; else echo '无法自动重启 docker，请手动重启'; fi";
        let cmd = format!(
            "sudo mkdir -p /etc/docker && printf '%s' '{}' | sudo tee /etc/docker/daemon.json >/dev/null && {restart}",
            content.replace('\'', "'\\''")
        );
        self.run_shell(&cmd)
    }

    pub(crate) fn clone_or_update_repo(
        &self,
        url: &str,
        target: &Path,
        branch: Option<&str>,
        mode: InstallMode,
    ) -> Result<()> {
        if target.join(".git").exists() {
            if mode == InstallMode::Clean {
                fs::remove_dir_all(target)?;
            } else {
                self.ensure_clean_worktree(target)?;
                let branch = branch.unwrap_or("main");
                let cmd = format!(
                    "cd '{}' && git fetch --depth 1 '{}' '{}' && git checkout -fB '{}' FETCH_HEAD && git reset --hard FETCH_HEAD",
                    shell_escape(target),
                    url,
                    branch,
                    branch
                );
                return self.run_shell(&cmd);
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let branch_part = branch
            .map(|b| format!("-b '{}' ", shell_escape_raw(b)))
            .unwrap_or_default();
        let cmd = format!(
            "git clone --depth 1 {}'{}' '{}'",
            branch_part,
            url,
            shell_escape(target)
        );
        self.run_shell(&cmd)
    }

    /// 如果目标仓库工作区有本地修改或未跟踪文件，列出后请用户选择处理方式：
    /// 1) git stash 临时保存；2) 丢弃；3) 取消更新。
    /// 丢弃前还会再确认一次，避免误操作。
    fn ensure_clean_worktree(&self, target: &Path) -> Result<()> {
        let output = Command::new("bash")
            .arg("-lc")
            .arg(format!(
                "cd '{}' && git status --porcelain",
                shell_escape(target)
            ))
            .output()
            .with_context(|| format!("git status 执行失败: {}", target.display()))?;
        let porcelain = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = porcelain
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        if lines.is_empty() {
            return Ok(());
        }

        let bar = "═".repeat(58);
        println!();
        println!("{}", style(&bar).red().bold());
        println!("  {}", style("⚠  检测到本地修改 / 未跟踪文件").red().bold());
        println!("{}", style(&bar).red().bold());
        println!("  {} {}", style("仓库:").yellow(), target.display());
        println!();
        let show_count = 20usize.min(lines.len());
        for line in &lines[..show_count] {
            println!("    {}", style(line).red());
        }
        if lines.len() > show_count {
            println!(
                "    {}",
                style(format!("... 另有 {} 条未显示", lines.len() - show_count)).dim()
            );
        }

        // uv.lock 是 uv 自动生成的依赖锁文件，本地修改通常只是版本号刷新，
        // 直接丢弃同步上游是安全的合理默认。其他场景仍默认 "取消" 保护用户改动。
        let only_uv_lock = lines.len() == 1
            && lines[0]
                .get(3..)
                .map(|s| s.trim() == "uv.lock")
                .unwrap_or(false);
        if only_uv_lock {
            println!(
                "  {}",
                style("（仅 uv.lock 被改动，默认建议丢弃以同步上游锁文件）").green()
            );
        }
        println!("{}", style(&bar).red().bold());

        let items: Vec<String> = vec![
            "临时保存（git stash，含未跟踪文件，可后续恢复）".to_string(),
            style("丢弃本地改动并强制同步（不可恢复！）")
                .red()
                .bold()
                .to_string(),
            "取消本次更新".to_string(),
        ];
        self.drain_pending_input();
        let default_idx = if only_uv_lock { 1 } else { 2 };
        let choice = Select::with_theme(&self.theme)
            .with_prompt("如何处理这些本地改动？")
            .items(&items)
            .default(default_idx)
            .interact()
            .with_context(|| "读取选择失败")?;

        match choice {
            0 => {
                self.run_shell(&format!(
                    "cd '{}' && git stash push -u -m \"maibot-mgr-tui-$(date +%Y%m%d-%H%M%S)\"",
                    shell_escape(target)
                ))?;
                println!(
                    "  {}",
                    style("已保存到 git stash。日后恢复请执行: git stash list / git stash pop").dim()
                );
            }
            1 => {
                println!();
                println!("{}", style(&bar).red().bold());
                println!(
                    "  {}",
                    style("⚠  即将执行: git reset --hard HEAD && git clean -fd")
                        .red()
                        .bold()
                );
                println!(
                    "  {}",
                    style("以上列出的修改和未跟踪文件会被永久删除，无法恢复！")
                        .red()
                        .bold()
                );
                println!(
                    "  {}",
                    style("（.gitignore 内的文件如 venv、data、知识库等不受影响）").dim()
                );
                println!("{}", style(&bar).red().bold());
                self.drain_pending_input();
                let confirmed = Confirm::with_theme(&self.theme)
                    .with_prompt(
                        style("我已了解风险，确认丢弃？按 y 确认 / n 取消")
                            .red()
                            .bold()
                            .to_string(),
                    )
                    .default(true)
                    .interact()
                    .with_context(|| "读取确认失败")?;
                if !confirmed {
                    return Err(anyhow!(UserCanceled(
                        "已取消：未确认丢弃本地修改".to_string()
                    )));
                }
                self.run_shell(&format!(
                    "cd '{}' && git reset --hard HEAD && git clean -fd",
                    shell_escape(target)
                ))?;
            }
            _ => {
                return Err(anyhow!(UserCanceled(format!(
                    "已取消：{} 存在未保存的本地修改",
                    target.display()
                ))));
            }
        }
        Ok(())
    }

    pub(crate) fn setup_python_env(&self, plan: &InstallPlan) -> Result<()> {
        let root = &plan.install_path;
        let maibot_dir = root.join("MaiBot");
        match plan.python_env {
            PythonEnv::Uv => {
                if !command_exists("uv")? {
                    self.run_shell("curl -LsSf https://astral.sh/uv/install.sh | sh")?;
                }
                let lock_path = maibot_dir.join("uv.lock");
                if lock_path.exists() {
                    fs::remove_file(lock_path)?;
                }
                let venv = maibot_dir.join(".venv");
                if plan.venv_mode == VenvMode::Recreate && venv.exists() {
                    fs::remove_dir_all(&venv)?;
                }
                let mut prefix = String::new();
                if !plan.uv_index.is_empty() {
                    prefix.push_str(&format!(
                        "export UV_INDEX_URL='{}' PIP_INDEX_URL='{}'; ",
                        plan.uv_index, plan.uv_index
                    ));
                }
                if !venv.exists() {
                    self.run_shell(&format!(
                        "cd '{}' && {}uv venv --python 3.14",
                        shell_escape(&maibot_dir),
                        prefix
                    ))?;
                }
                self.run_shell(&format!(
                    "cd '{}' && {}uv sync",
                    shell_escape(&maibot_dir),
                    prefix
                ))?;
            }
            PythonEnv::System => {
                let venv_dir = root.join("venv");
                if plan.venv_mode == VenvMode::Recreate && venv_dir.exists() {
                    fs::remove_dir_all(&venv_dir)?;
                }
                if !venv_dir.exists() {
                    self.run_shell(&format!(
                        "cd '{}' && python3 -m venv venv",
                        shell_escape(root)
                    ))?;
                }
                if !plan.pip_index.is_empty() {
                    let pip_conf = venv_dir.join("pip.conf");
                    fs::write(
                        &pip_conf,
                        format!(
                            "[global]\nindex-url = {}\ntrusted-host = {}\n",
                            plan.pip_index, plan.pip_host
                        ),
                    )?;
                }
                let pip_prefix = if !plan.pip_index.is_empty() {
                    format!(
                        "export PIP_CONFIG_FILE='{}'; ",
                        shell_escape(&venv_dir.join("pip.conf"))
                    )
                } else {
                    String::new()
                };
                self.run_shell(&format!(
                    "cd '{}' && . venv/bin/activate && {}pip install --upgrade pip && if [ -f MaiBot/requirements.txt ]; then pip install -r MaiBot/requirements.txt; fi && if [ -f MaiBot/plugins/MaiBot-Napcat-Adapter/requirements.txt ]; then pip install -r MaiBot/plugins/MaiBot-Napcat-Adapter/requirements.txt; fi",
                    shell_escape(root),
                    pip_prefix
                ))?;
            }
        }
        Ok(())
    }

    pub(crate) fn install_napcat(&self, plan: &InstallPlan) -> Result<()> {
        let napcat_dir = plan.install_path.join("NapCat");
        fs::create_dir_all(&napcat_dir)?;
        let compose_path = napcat_dir.join("docker-compose.yml");
        if !compose_path.exists() {
            let compose = r#"services:
  napcat:
    image: mlikiowa/napcat-docker:latest
    container_name: napcat
    restart: always
    environment:
      - NAPCAT_UID=${NAPCAT_UID:-1000}
      - NAPCAT_GID=${NAPCAT_GID:-1000}
    volumes:
      - ./config:/app/napcat/config
      - ./qq_config:/app/.config/QQ
    network_mode: "host"
"#;
            fs::write(&compose_path, compose)?;
        }
        self.handle_napcat_container_conflict(&napcat_dir)?;
        self.run_shell(&format!(
            "cd '{}' && docker compose up -d",
            shell_escape(&napcat_dir)
        ))
    }

    /// `docker compose up -d` 在同名容器（非本 compose 项目托管）存在时会冲突。
    /// 这里先用 `docker ps -aq --filter name=^napcat$` 探测，命中就询问用户
    /// 是否 `docker rm -f` 后重建；选否则按用户取消处理。
    fn handle_napcat_container_conflict(&self, napcat_dir: &Path) -> Result<()> {
        let output = Command::new("bash")
            .arg("-lc")
            .arg("docker ps -aq --filter name=^napcat$ 2>/dev/null || true")
            .output()
            .with_context(|| "查询 napcat 容器状态失败")?;
        let id_blob = String::from_utf8_lossy(&output.stdout);
        let ids: Vec<&str> = id_blob
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .collect();
        if ids.is_empty() {
            return Ok(());
        }

        let status_out = Command::new("bash")
            .arg("-lc")
            .arg("docker ps -a --filter name=^napcat$ --format '{{.ID}}  {{.Status}}  {{.Image}}'")
            .output()
            .ok();
        let status = status_out
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        println!();
        println!(
            "  {}",
            style("⚠  检测到已存在同名 docker 容器: napcat")
                .yellow()
                .bold()
        );
        if !status.is_empty() {
            for line in status.lines() {
                println!("    {}", style(line).dim());
            }
        }
        println!(
            "    {}",
            style(format!(
                "compose 目录: {}",
                napcat_dir.display()
            ))
            .dim()
        );
        println!(
            "    {}",
            style("继续部署需先删除旧容器（镜像与挂载卷不会被动）").dim()
        );

        self.drain_pending_input();
        let confirmed = Confirm::with_theme(&self.theme)
            .with_prompt("删除旧容器并重新部署？")
            .default(true)
            .interact()
            .with_context(|| "读取确认失败")?;

        if !confirmed {
            return Err(anyhow!(UserCanceled(
                "已取消：保留旧 napcat 容器，未重新部署".to_string()
            )));
        }

        self.run_shell("docker rm -f napcat")?;
        Ok(())
    }

    pub(crate) fn install_llbot(&self, plan: &InstallPlan) -> Result<()> {
        let llbot_dir = plan.install_path.join("LLBot");
        fs::create_dir_all(&llbot_dir)?;
        let arch = detect_arch()?;
        let asset_name = format!("LLBot-CLI-linux-{arch}.zip");
        let script = format!(
            r#"set -e
api_url="https://api.github.com/repos/LLOneBot/LuckyLilliaBot/releases/latest"
asset_url=$(curl -fsSL "$api_url" | python3 -c 'import sys,json; data=json.load(sys.stdin); name=sys.argv[1]; print(next((a["browser_download_url"] for a in data.get("assets",[]) if a.get("name")==name),""))' "{asset_name}")
[ -n "$asset_url" ]
mkdir -p '{llbot}'
zip_path='{llbot}/{asset_name}'
curl -fL --retry 3 --connect-timeout 10 -o "$zip_path" "$asset_url"
rm -rf '{llbot}/bin' '{llbot}/llbot' '{llbot}/start.sh' '{llbot}/使用说明.txt' '{llbot}/更新日志.txt'
unzip -oq "$zip_path" -d '{llbot}'
chmod +x '{llbot}/start.sh' '{llbot}/llbot' 2>/dev/null || true
find '{llbot}/bin' -type f -exec chmod +x {{}} \; 2>/dev/null || true
"#,
            llbot = shell_escape(&llbot_dir)
        );
        self.run_shell(&script)?;
        self.install_linuxqq_for_llbot()?;
        self.update_llbot_default_config(&llbot_dir)?;
        Ok(())
    }

    pub(crate) fn install_linuxqq_for_llbot(&self) -> Result<()> {
        let script = r#"set -e
if [ -f /opt/QQ/qq ] || command -v linuxqq >/dev/null 2>&1; then
  echo "检测到系统已安装 LinuxQQ，跳过预安装。"
  exit 0
fi

machine="$(uname -m)"
case "$machine" in
  x86_64|amd64) arch_deb="amd64"; arch_rpm="x86_64" ;;
  aarch64|arm64) arch_deb="arm64"; arch_rpm="aarch64" ;;
  *)
    echo "当前主机架构暂不支持 LinuxQQ 自动预装: $machine"
    exit 0
    ;;
esac

QQ_VERSION="3.2.20-40768"
QQ_BASE="https://dldir1v6.qq.com/qqfile/qq/QQNT/ab90fdfa"

if command -v apt-get >/dev/null 2>&1; then
  echo
  echo "▶ 通过 apt 预安装 LinuxQQ..."
  sudo apt-get update -y
  sudo apt-get install -y wget
  qq_deb="/tmp/linuxqq_${arch_deb}.deb"
  if ! wget -O "$qq_deb" "${QQ_BASE}/linuxqq_${QQ_VERSION}_${arch_deb}.deb"; then
    echo "LinuxQQ 安装包下载失败。"
    exit 1
  fi
  lib_snd="libasound2"
  if apt-cache show libasound2t64 >/dev/null 2>&1; then
    lib_snd="libasound2t64"
  fi
  if ! sudo apt-get install -y "$qq_deb" x11-utils libgtk-3-0 libxcb-xinerama0 libgl1-mesa-dri libnotify4 libnss3 xdg-utils libsecret-1-0 libappindicator3-1 libgbm1 "$lib_snd" fonts-noto-cjk libxss1; then
    rm -f "$qq_deb"
    echo "LinuxQQ 预安装失败。"
    exit 1
  fi
  rm -f "$qq_deb"
  echo "LinuxQQ 预安装完成。"
  exit 0
fi

if command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1; then
  PM=$(command -v dnf >/dev/null 2>&1 && echo dnf || echo yum)
  echo
  echo "▶ 通过 $PM 预安装 LinuxQQ..."
  qq_rpm="${QQ_BASE}/linuxqq_${QQ_VERSION}_${arch_rpm}.rpm"
  sudo "$PM" install -y wget || true
  if ! sudo "$PM" install -y "$qq_rpm"; then
    echo "LinuxQQ rpm 安装失败。"
    exit 1
  fi
  echo "LinuxQQ 预安装完成。"
  exit 0
fi

if command -v zypper >/dev/null 2>&1; then
  echo
  echo "▶ 通过 zypper 预安装 LinuxQQ..."
  qq_rpm="${QQ_BASE}/linuxqq_${QQ_VERSION}_${arch_rpm}.rpm"
  sudo zypper --non-interactive install wget || true
  if ! sudo zypper --non-interactive --no-gpg-checks install "$qq_rpm"; then
    echo "LinuxQQ rpm 安装失败。"
    exit 1
  fi
  echo "LinuxQQ 预安装完成。"
  exit 0
fi

if command -v pacman >/dev/null 2>&1; then
  echo
  echo "▶ 通过 AUR 预安装 LinuxQQ..."
  if command -v yay >/dev/null 2>&1; then
    if ! yay -S --noconfirm linuxqq; then
      echo "LinuxQQ 预安装失败。"
      exit 1
    fi
    echo "LinuxQQ 预安装完成。"
    exit 0
  fi
  if command -v paru >/dev/null 2>&1; then
    if ! paru -S --noconfirm linuxqq; then
      echo "LinuxQQ 预安装失败。"
      exit 1
    fi
    echo "LinuxQQ 预安装完成。"
    exit 0
  fi
  echo "检测到 Arch 系系统，但未安装 yay/paru，跳过自动预装 LinuxQQ。"
  echo "可手动运行: yay -S linuxqq"
  exit 0
fi

if command -v apk >/dev/null 2>&1; then
  echo "Alpine (musl) 暂不支持 LinuxQQ 官方包，跳过预装。"
  exit 0
fi

echo "未识别的系统包管理器，跳过 LinuxQQ 预安装；首次启动 LLBot 时可按官方脚本提示手动安装。"
"#;
        self.run_shell(script)
    }

    pub(crate) fn update_llbot_default_config(&self, llbot_dir: &Path) -> Result<()> {
        let path = llbot_dir.join("bin/llbot/default_config.json");
        if !path.exists() {
            return Ok(());
        }
        let mut data: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        let webui = data
            .get_mut("webui")
            .and_then(Value::as_object_mut)
            .cloned()
            .unwrap_or_default();
        let port = webui.get("port").and_then(Value::as_u64).unwrap_or(3080);
        data["webui"] = serde_json::json!({
            "host": "0.0.0.0",
            "port": port,
            "enable": true
        });
        fs::write(&path, serde_json::to_string_pretty(&data)? + "\n")?;
        Ok(())
    }
}
