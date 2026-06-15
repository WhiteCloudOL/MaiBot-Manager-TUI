use crate::{
    app::App,
    model::*,
    terminal::{TerminalUiGuard, restore_terminal_state},
    utils::*,
};
use anyhow::{Context, Result, anyhow, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use dialoguer::console::style;
use dialoguer::{Confirm, Input, Select};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(crate) struct UserCanceled(pub String);

impl fmt::Display for UserCanceled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UserCanceled {}

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
                    println!(
                        "  {} {}",
                        style("x").yellow(),
                        style(e.to_string()).yellow()
                    );
                    println!(
                        "  {}",
                        style("（已返回主菜单，未执行任何破坏性操作）").dim()
                    );
                }
                Err(e) => return Err(e),
            }
        }
        self.pause("安装流程结束，按回车返回主菜单")?;
        Ok(())
    }

    pub(crate) fn install_planner(
        &mut self,
        current: &AppConfig,
        plan: &mut InstallPlan,
    ) -> Result<bool> {
        let _guard = TerminalUiGuard::enter()?;
        let mut target: Option<PlannerEntry> = None;
        let mut expanded: Option<PlanField> = None;

        loop {
            let entries = self.build_planner_entries(plan, expanded);
            let mut selected = target
                .as_ref()
                .and_then(|t| entries.iter().position(|e| e == t))
                .unwrap_or(0);
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
                        target = entries.get(selected.saturating_sub(1)).cloned();
                    }
                    KeyCode::Down => {
                        let next = if selected + 1 < entries.len() {
                            selected + 1
                        } else {
                            selected
                        };
                        target = entries.get(next).cloned();
                    }
                    KeyCode::Home => target = entries.first().cloned(),
                    KeyCode::End => target = entries.last().cloned(),
                    KeyCode::Left => {
                        if let Some(field) = entries.get(selected).and_then(|entry| entry.field()) {
                            expanded = None;
                            target = Some(PlannerEntry::Field(field));
                        }
                    }
                    KeyCode::Right => {
                        if let Some(field) = entries.get(selected).and_then(|entry| entry.field()) {
                            expanded = Some(field);
                            target = Some(PlannerEntry::Field(field));
                        }
                    }
                    KeyCode::Char(' ') => match entries.get(selected).cloned() {
                        Some(PlannerEntry::Choice(field, choice_idx)) => {
                            self.apply_planner_choice(current, plan, field, choice_idx)?;
                            self.save_config(&self.plan_to_config(plan))?;
                            target = Some(PlannerEntry::Choice(field, choice_idx));
                        }
                        Some(PlannerEntry::Field(field)) if field != PlanField::InstallPath => {
                            expanded = Some(field);
                            target = Some(PlannerEntry::Field(field));
                        }
                        _ => {}
                    },
                    KeyCode::Enter => match entries.get(selected).cloned() {
                        Some(PlannerEntry::Field(field)) => {
                            if field == PlanField::InstallPath {
                                target = Some(PlannerEntry::Field(field));
                                self.edit_install_path(plan)?;
                            } else if expanded == Some(field) {
                                expanded = None;
                                target = Some(PlannerEntry::Field(field));
                            } else {
                                expanded = Some(field);
                                target = Some(PlannerEntry::Field(field));
                            }
                        }
                        Some(PlannerEntry::Choice(field, choice_idx)) => {
                            self.apply_planner_choice(current, plan, field, choice_idx)?;
                            self.save_config(&self.plan_to_config(plan))?;
                            target = Some(PlannerEntry::Choice(field, choice_idx));
                        }
                        Some(PlannerEntry::Action(PlanAction::StartInstall)) => return Ok(true),
                        Some(PlannerEntry::Action(PlanAction::ResetDefaults)) => {
                            *plan = self.build_recommended_defaults();
                            self.save_config(&self.plan_to_config(plan))?;
                            target = None;
                            expanded = None;
                        }
                        Some(PlannerEntry::Action(PlanAction::BackToMenu)) => return Ok(false),
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
            PlanField::MaiBotBranch,
            PlanField::InstallMode,
            PlanField::PythonEnv,
            PlanField::VenvMode,
            PlanField::GithubProxy,
            PlanField::PipSource,
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
            PlanField::InstallPath => vec!["自定义路径".into()],
            PlanField::MaiBotBranch => vec!["main（稳定版）".into(), "dev（预览版）".into()],
            PlanField::InstallMode => {
                vec!["正常更新/修复".into(), "全新安装（清空目标目录）".into()]
            }
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
                items.push("自定义镜像源".into());
                items.extend(github_mirrors().iter().map(|v| (*v).to_string()));
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
            PlanField::BotProtocols => vec!["macOS 暂不安装协议端".into()],
            PlanField::DockerMirror => vec!["macOS 暂不使用 Docker 部署协议端".into()],
        }
    }

    pub(crate) fn planner_field_label(&self, field: PlanField) -> &'static str {
        match field {
            PlanField::InstallPath => "目录",
            PlanField::MaiBotBranch => "主程序分支",
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
            PlanField::MaiBotBranch => plan.maibot_branch.clone(),
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
            PlanField::BotProtocols => "暂不安装".into(),
            PlanField::DockerMirror => "不使用".into(),
        }
    }

    pub(crate) fn planner_choice_active(
        &self,
        plan: &InstallPlan,
        field: PlanField,
        choice_idx: usize,
    ) -> bool {
        match field {
            PlanField::InstallPath => false,
            PlanField::MaiBotBranch => {
                matches!(
                    (choice_idx, plan.maibot_branch.as_str()),
                    (0, "main") | (1, "dev")
                )
            }
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
                } else if choice_idx == 2 {
                    !plan.github_proxy.is_empty()
                        && plan.github_proxy != "https://github.com"
                        && !github_mirrors()
                            .iter()
                            .any(|mirror| *mirror == plan.github_proxy)
                } else if choice_idx >= 3 && choice_idx < 3 + github_mirrors().len() {
                    plan.github_proxy == github_mirrors()[choice_idx - 3]
                } else {
                    false
                }
            }
            PlanField::PipSource => match choice_idx {
                0 => plan.pip_index.is_empty(),
                1 => plan.pip_display == "阿里云",
                2 => plan.pip_display == "腾讯云",
                3 => plan.pip_display == "清华大学",
                4 => plan.pip_display == "中国科学技术大学",
                5 => plan.pip_display == "官方源",
                _ => {
                    !plan.pip_index.is_empty()
                        && !["阿里云", "腾讯云", "清华大学", "中国科学技术大学", "官方源"]
                            .contains(&plan.pip_display.as_str())
                }
            },
            PlanField::BotProtocols | PlanField::DockerMirror => true,
        }
    }

    pub(crate) fn print_planner_view(
        &self,
        plan: &InstallPlan,
        entries: &[PlannerEntry],
        selected: usize,
        expanded: Option<PlanField>,
    ) {
        self.print_section(
            "macOS 安装计划",
            "↑/↓ 移动 · ←/→ 展开收起 · Enter/Space 应用 · Esc 返回 · 协议端暂不安装",
        );
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
                        PlanAction::StartInstall => "执行安装 / 更新",
                        PlanAction::ResetDefaults => "恢复推荐默认",
                        PlanAction::BackToMenu => "返回主菜单",
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
            PlanField::MaiBotBranch => {
                plan.maibot_branch = if choice_idx == 1 { "dev" } else { "main" }.into();
            }
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
                2 => {
                    let input: String = self.with_prompt_mode(|| {
                        Input::with_theme(&self.theme)
                            .with_prompt("输入自定义镜像源")
                            .interact_text()
                            .map_err(Into::into)
                    })?;
                    plan.github_proxy = normalize_url(&input);
                }
                idx if idx >= 3 && idx < 3 + github_mirrors().len() => {
                    plan.github_proxy = github_mirrors()[idx - 3].to_string();
                }
                _ => {}
            },
            PlanField::PipSource => apply_pip_choice(self, plan, choice_idx)?,
            PlanField::BotProtocols => plan.bot_protocols.clear(),
            PlanField::DockerMirror => plan.docker_mirror = DockerMirror::Keep,
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

    pub(crate) fn build_default_install_plan(&self, current: &AppConfig) -> Result<InstallPlan> {
        let mut plan = self.build_recommended_defaults();
        if !current.user_install_path.is_empty() {
            plan.install_path = PathBuf::from(&current.user_install_path);
        } else if !current.mai_path.is_empty() {
            plan.install_path = PathBuf::from(&current.mai_path);
        }
        if current.maibot_branch == "dev" {
            plan.maibot_branch = "dev".into();
        }
        if current.mai_python_env == "system" {
            plan.python_env = PythonEnv::System;
        }
        if current.mai_venv_mode == "recreate" {
            plan.venv_mode = VenvMode::Recreate;
        }
        if !current.pip_index.is_empty() {
            plan.pip_display = current.pip_display.clone();
            plan.pip_index = current.pip_index.clone();
            plan.pip_host = current.pip_host.clone();
            plan.uv_index = current.pip_index.clone();
        }
        plan.bot_protocols.clear();
        plan.docker_mirror = DockerMirror::Keep;
        Ok(plan)
    }

    pub(crate) fn build_recommended_defaults(&self) -> InstallPlan {
        InstallPlan {
            install_path: dirs::home_dir()
                .map(|h| h.join("maimai"))
                .unwrap_or_default(),
            install_mode: InstallMode::Normal,
            python_env: PythonEnv::Uv,
            venv_mode: VenvMode::Keep,
            maibot_branch: "main".into(),
            github_proxy: String::new(),
            pip_display: "系统默认".into(),
            pip_index: String::new(),
            pip_host: String::new(),
            uv_index: String::new(),
            bot_protocols: Vec::new(),
            docker_mirror: DockerMirror::Keep,
            github_fallback: GithubFallbackMode::Ask,
            git_dirty_mode: GitDirtyMode::Ask,
            napcat_conflict_mode: NapcatConflictMode::Ask,
            llbot_update_mode: LlbotUpdateMode::Prompt,
        }
    }

    pub(crate) fn plan_to_config(&self, plan: &InstallPlan) -> AppConfig {
        AppConfig {
            user_install_path: plan.install_path.display().to_string(),
            mai_path: plan.install_path.display().to_string(),
            mai_python_env: match plan.python_env {
                PythonEnv::Uv => "uv".into(),
                PythonEnv::System => "system".into(),
            },
            mai_llbot_path: String::new(),
            mai_install_mode: match plan.install_mode {
                InstallMode::Clean => "clean".into(),
                InstallMode::Normal => "normal".into(),
            },
            mai_venv_mode: match plan.venv_mode {
                VenvMode::Recreate => "recreate".into(),
                VenvMode::Keep => "keep".into(),
            },
            maibot_branch: plan.maibot_branch.clone(),
            pip_display: plan.pip_display.clone(),
            pip_index: plan.pip_index.clone(),
            pip_host: plan.pip_host.clone(),
            bot_protocols: "none".into(),
        }
    }

    pub(crate) fn run_install(&self, plan: &InstallPlan) -> Result<()> {
        let mut plan = plan.clone();
        if !plan.bot_protocols.is_empty() {
            bail!("macOS 版目前只安装 MaiBot 核心，请使用 --protocol none");
        }
        if plan.install_mode == InstallMode::Clean {
            plan.venv_mode = VenvMode::Recreate;
        }
        self.ensure_base_tools(&plan)?;
        if plan.github_proxy.is_empty() {
            self.clear();
            self.print_header(Some(&plan));
            self.print_section(
                "GitHub 线路测速",
                "未手动指定线路，正在自动测速并选择最佳线路",
            );
            plan.github_proxy = self.run_github_speedtest(plan.github_fallback)?;
        }

        if plan.install_mode == InstallMode::Clean {
            clean_install_dir(&plan.install_path)?;
        }
        fs::create_dir_all(&plan.install_path)?;
        self.clone_or_update_repo(
            &repo_url(&plan.github_proxy, "MaiM-with-u/MaiBot"),
            &plan.install_path.join("MaiBot"),
            Some(&plan.maibot_branch),
            plan.install_mode,
            true,
            plan.git_dirty_mode,
        )?;
        self.setup_python_env(&plan)?;
        self.save_config(&self.plan_to_config(&plan))?;
        println!("macOS 安装 / 更新完成。协议端 NapCat / LLBot 暂未安装。");
        Ok(())
    }

    pub(crate) fn run_github_speedtest(&self, fallback: GithubFallbackMode) -> Result<String> {
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
                    let started = Instant::now();
                    let output = Command::new("/bin/zsh")
                        .arg("-lc")
                        .arg(format!(
                            "{} curl -sL -o /dev/null --max-time 5 --connect-timeout 3 -w '%{{time_total}}' '{}'",
                            macos_path_export(),
                            test_url.replace('\'', "'\\''")
                        ))
                        .output();

                    match output {
                        Ok(output) if output.status.success() => {
                            let measured = String::from_utf8_lossy(&output.stdout)
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .map(|sec| sec * 1000.0)
                                .unwrap_or_else(|| started.elapsed().as_secs_f64() * 1000.0);
                            (mirror, measured, true)
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
                println!(
                    "  {} {} ({:.0} ms)",
                    style("已选择").green().bold(),
                    style(&url).cyan(),
                    ms
                );
                Ok(url)
            }
            None => {
                println!("  {}", style("全部线路连接失败").red().bold());
                match fallback {
                    GithubFallbackMode::Ask => {
                        let choice = self.with_prompt_mode(|| {
                            Select::with_theme(&self.theme)
                                .with_prompt("请选择回退方案")
                                .items(["重试测速", "使用 GitHub 官方直连", "取消安装"])
                                .default(0)
                                .interact()
                                .map_err(Into::into)
                        })?;
                        match choice {
                            0 => self.run_github_speedtest(fallback),
                            1 => Ok("https://github.com".to_string()),
                            _ => Err(anyhow!("用户取消安装")),
                        }
                    }
                    GithubFallbackMode::Direct => Ok("https://github.com".to_string()),
                    GithubFallbackMode::Cancel => Err(anyhow!(
                        "GitHub 线路测速全部失败；CLI 可使用 --github-fallback direct 改为直连继续"
                    )),
                }
            }
        }
    }

    pub(crate) fn ensure_base_tools(&self, plan: &InstallPlan) -> Result<()> {
        fs::create_dir_all(tools_dir(&plan.install_path))?;
        let mut formulae = Vec::<&str>::new();
        for (tool, formula) in [("git", "git"), ("curl", "curl"), ("unzip", "unzip")] {
            if !command_exists(tool)? {
                formulae.push(formula);
            }
        }
        if plan.python_env == PythonEnv::Uv && !command_exists("uv")? {
            formulae.push("uv");
        }
        if plan.python_env == PythonEnv::System && !command_exists("python3")? {
            formulae.push("python");
        }
        formulae.sort_unstable();
        formulae.dedup();
        if formulae.is_empty() {
            return Ok(());
        }

        self.ensure_homebrew()?;
        self.clear();
        self.print_header(Some(plan));
        self.print_section("依赖检查", "使用 Homebrew 补齐 macOS 原生工具");
        self.print_kv("待安装", &formulae.join(" "));
        self.print_line();
        if let Some(cmd) = brew_install_cmd(&formulae) {
            self.run_shell(&cmd)?;
        }
        Ok(())
    }

    fn ensure_homebrew(&self) -> Result<()> {
        if brew_executable().is_some() {
            return Ok(());
        }
        self.print_section("Homebrew", "未检测到 Homebrew，正在调用官方安装脚本");
        self.run_shell(
            "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"",
        )?;
        if brew_executable().is_none() {
            bail!("Homebrew 安装脚本执行完成，但仍未找到 brew，请检查终端 PATH 后重试");
        }
        Ok(())
    }

    pub(crate) fn clone_or_update_repo(
        &self,
        url: &str,
        target: &Path,
        branch: Option<&str>,
        mode: InstallMode,
        auto_discard_single_uv_lock: bool,
        dirty_mode: GitDirtyMode,
    ) -> Result<()> {
        if target.join(".git").exists() {
            if mode == InstallMode::Clean {
                fs::remove_dir_all(target)?;
            } else {
                self.ensure_clean_worktree(target, auto_discard_single_uv_lock, dirty_mode)?;
                let branch = branch.unwrap_or("main");
                let cmd = format!(
                    "cd '{}' && git fetch --depth 1 '{}' '{}' && git checkout -fB '{}' FETCH_HEAD && git reset --hard FETCH_HEAD",
                    shell_escape(target),
                    url,
                    branch,
                    branch
                );
                return self.run_shell(&format!("{} {}", macos_path_export(), cmd));
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
        self.run_shell(&format!("{} {}", macos_path_export(), cmd))
    }

    fn ensure_clean_worktree(
        &self,
        target: &Path,
        auto_discard_single_uv_lock: bool,
        dirty_mode: GitDirtyMode,
    ) -> Result<()> {
        let output = Command::new("/bin/zsh")
            .arg("-lc")
            .arg(format!(
                "{} cd '{}' && git status --porcelain",
                macos_path_export(),
                shell_escape(target)
            ))
            .output()
            .with_context(|| format!("git status 执行失败: {}", target.display()))?;
        let porcelain = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = porcelain.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            return Ok(());
        }

        let only_uv_lock = lines.len() == 1
            && lines[0]
                .get(3..)
                .map(|s| s.trim() == "uv.lock")
                .unwrap_or(false);
        if only_uv_lock && auto_discard_single_uv_lock {
            self.run_shell(&format!(
                "{} cd '{}' && (git checkout -- uv.lock 2>/dev/null || true) && git clean -f -- uv.lock",
                macos_path_export(),
                shell_escape(target)
            ))?;
            return Ok(());
        }

        match dirty_mode {
            GitDirtyMode::Stash => self.run_shell(&format!(
                "{} cd '{}' && git stash push -u -m \"maibot-manager-macos-$(date +%Y%m%d-%H%M%S)\"",
                macos_path_export(),
                shell_escape(target)
            )),
            GitDirtyMode::Discard => self.run_shell(&format!(
                "{} cd '{}' && git reset --hard HEAD && git clean -fd",
                macos_path_export(),
                shell_escape(target)
            )),
            GitDirtyMode::Cancel => Err(anyhow!(UserCanceled(format!(
                "已取消：{} 存在未保存的本地修改；CLI 可使用 --git-dirty stash 或 --git-dirty discard 指定处理方式",
                target.display()
            )))),
            GitDirtyMode::Ask if self.cli_mode => Err(anyhow!(UserCanceled(format!(
                "目标仓库存在本地改动: {}；请使用 --git-dirty stash|discard|cancel",
                target.display()
            )))),
            GitDirtyMode::Ask => {
                println!();
                println!("  {}", style("检测到本地修改 / 未跟踪文件").red().bold());
                println!("  {} {}", style("仓库:").yellow(), target.display());
                for line in lines.iter().take(20) {
                    println!("    {}", style(line).red());
                }
                if lines.len() > 20 {
                    println!(
                        "    {}",
                        style(format!("... 另有 {} 条未显示", lines.len() - 20)).dim()
                    );
                }
                self.drain_pending_input();
                let choice = Select::with_theme(&self.theme)
                    .with_prompt("如何处理这些本地改动？")
                    .items([
                        "git stash 保存后继续",
                        "丢弃本地改动并强制同步",
                        "取消本次更新",
                    ])
                    .default(if only_uv_lock { 1 } else { 2 })
                    .interact()
                    .with_context(|| "读取选择失败")?;
                match choice {
                    0 => self.ensure_clean_worktree(target, false, GitDirtyMode::Stash),
                    1 => {
                        self.drain_pending_input();
                        let confirmed = Confirm::with_theme(&self.theme)
                            .with_prompt("确认丢弃本地改动？")
                            .default(false)
                            .interact()
                            .with_context(|| "读取确认失败")?;
                        if !confirmed {
                            return Err(anyhow!(UserCanceled(
                                "已取消：未确认丢弃本地修改".to_string()
                            )));
                        }
                        self.ensure_clean_worktree(target, false, GitDirtyMode::Discard)
                    }
                    _ => Err(anyhow!(UserCanceled(format!(
                        "已取消：{} 存在未保存的本地修改",
                        target.display()
                    )))),
                }
            }
        }
    }

    pub(crate) fn setup_python_env(&self, plan: &InstallPlan) -> Result<()> {
        let root = &plan.install_path;
        let maibot_dir = root.join("MaiBot");
        match plan.python_env {
            PythonEnv::Uv => {
                if !command_exists("uv")? {
                    self.ensure_homebrew()?;
                    if let Some(cmd) = brew_install_cmd(&["uv"]) {
                        self.run_shell(&cmd)?;
                    }
                }
                let lock_path = maibot_dir.join("uv.lock");
                if lock_path.exists() {
                    fs::remove_file(lock_path)?;
                }
                let venv = maibot_dir.join(".venv");
                if plan.venv_mode == VenvMode::Recreate && venv.exists() {
                    self.remove_env_dir_safely(&venv, root)?;
                }
                let index = if plan.uv_index.is_empty() {
                    String::new()
                } else {
                    format!(
                        "export UV_INDEX_URL='{}' PIP_INDEX_URL='{}'; ",
                        shell_escape_raw(&plan.uv_index),
                        shell_escape_raw(&plan.uv_index)
                    )
                };
                if !venv.exists() {
                    self.run_shell(&with_macos_tools_path(
                        root,
                        &format!(
                            "cd '{}' && {}uv venv --python 3.14",
                            shell_escape(&maibot_dir),
                            index
                        ),
                    ))?;
                }
                self.run_shell(&with_macos_tools_path(
                    root,
                    &format!("cd '{}' && {}uv sync", shell_escape(&maibot_dir), index),
                ))?;
            }
            PythonEnv::System => {
                if !command_exists("python3")? {
                    self.ensure_homebrew()?;
                    if let Some(cmd) = brew_install_cmd(&["python"]) {
                        self.run_shell(&cmd)?;
                    }
                }
                let venv_dir = root.join("venv");
                if plan.venv_mode == VenvMode::Recreate && venv_dir.exists() {
                    self.remove_env_dir_safely(&venv_dir, root)?;
                }
                if !venv_dir.exists() {
                    self.run_shell(&with_macos_tools_path(
                        root,
                        &format!("cd '{}' && python3 -m venv venv", shell_escape(root)),
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
                self.run_shell(&with_macos_tools_path(
                    root,
                    &format!(
                        "cd '{}' && . venv/bin/activate && {}pip install --upgrade pip && if [ -f MaiBot/requirements.txt ]; then pip install -r MaiBot/requirements.txt; fi",
                        shell_escape(root),
                        pip_prefix
                    ),
                ))?;
            }
        }
        Ok(())
    }

    fn remove_env_dir_safely(&self, env_dir: &Path, install_root: &Path) -> Result<()> {
        if !env_dir.exists() {
            return Ok(());
        }
        let canonical_env = env_dir
            .canonicalize()
            .with_context(|| format!("解析虚拟环境目录失败: {}", env_dir.display()))?;
        let canonical_root = install_root
            .canonicalize()
            .unwrap_or_else(|_| install_root.to_path_buf());
        let canonical_maibot = install_root
            .join("MaiBot")
            .canonicalize()
            .unwrap_or_else(|_| install_root.join("MaiBot"));
        if canonical_env == canonical_root || canonical_env == canonical_maibot {
            bail!(
                "拒绝删除虚拟环境目录：{} 实际指向了安装目录或程序本体",
                env_dir.display()
            );
        }
        if !canonical_env.starts_with(&canonical_root) {
            bail!(
                "拒绝删除安装目录之外的虚拟环境：{} -> {}",
                env_dir.display(),
                canonical_env.display()
            );
        }
        let metadata = fs::symlink_metadata(env_dir)
            .with_context(|| format!("读取虚拟环境目录属性失败: {}", env_dir.display()))?;
        if metadata.file_type().is_symlink() {
            fs::remove_file(env_dir)?;
        } else {
            fs::remove_dir_all(env_dir)?;
        }
        Ok(())
    }

    pub(crate) fn install_napcat(&self, _plan: &InstallPlan) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn redownload_napcat_shell(&self, _plan: &InstallPlan) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn install_llbot(&self, _plan: &InstallPlan) -> Result<()> {
        macos_protocol_note()
    }

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
        while let Ok(true) = crossterm::event::poll(Duration::from_millis(20)) {
            if crossterm::event::read().is_err() {
                break;
            }
        }
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn apply_pip_choice(app: &App, plan: &mut InstallPlan, choice_idx: usize) -> Result<()> {
    match choice_idx {
        0 => {
            plan.pip_display = "系统默认".into();
            plan.pip_index.clear();
            plan.pip_host.clear();
            plan.uv_index.clear();
        }
        1 => set_pip(plan, "阿里云", "https://mirrors.aliyun.com/pypi/simple/"),
        2 => set_pip(
            plan,
            "腾讯云",
            "http://mirrors.cloud.tencent.com/pypi/simple",
        ),
        3 => set_pip(plan, "清华大学", "https://pypi.tuna.tsinghua.edu.cn/simple"),
        4 => set_pip(
            plan,
            "中国科学技术大学",
            "https://pypi.mirrors.ustc.edu.cn/simple/",
        ),
        5 => set_pip(plan, "官方源", "https://pypi.org/simple"),
        _ => {
            let custom: String = app.with_prompt_mode(|| {
                Input::with_theme(&app.theme)
                    .with_prompt("自定义 PyPI 镜像源")
                    .interact_text()
                    .map_err(Into::into)
            })?;
            let custom = normalize_url(&custom);
            plan.pip_display = custom.clone();
            plan.pip_host = custom.split('/').nth(2).unwrap_or_default().to_string();
            plan.pip_index = custom;
            plan.uv_index = plan.pip_index.clone();
        }
    }
    Ok(())
}

fn set_pip(plan: &mut InstallPlan, display: &str, index: &str) {
    plan.pip_display = display.into();
    plan.pip_index = index.into();
    plan.pip_host = index.split('/').nth(2).unwrap_or_default().to_string();
    plan.uv_index = plan.pip_index.clone();
}

fn macos_protocol_note() -> Result<()> {
    bail!("macOS 版目前只安装 MaiBot 核心，请使用 --protocol none")
}
