use crate::{
    app::App,
    model::*,
    plugins::{NAPCAT_ADAPTER_PLUGIN_ID, NAPCAT_ADAPTER_REPO_NAME},
    terminal::{TerminalUiGuard, restore_terminal_state},
    utils::*,
};
use anyhow::{Context, Result, anyhow, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use dialoguer::console::style;
use dialoguer::{Confirm, Input, Select};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Instant,
};

const LLBOT_RELEASE_TAG_FILE: &str = ".maibot-llbot-release";
const NAPCAT_RELEASE_TAG_FILE: &str = ".maibot-napcat-release";

#[derive(Debug)]
struct ReleaseAssetInfo {
    tag_name: String,
    asset_url: String,
}

impl App {
    pub(crate) fn install_update_flow(&mut self) -> Result<()> {
        let current = self.load_config().unwrap_or_default();
        let mut plan = self.build_default_install_plan(&current)?;
        let should_install = self.install_planner(&current, &mut plan)?;
        if should_install {
            self.run_install(&plan)?;
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
                        Some(PlannerEntry::Action(PlanAction::StartInstall)) => {
                            return Ok(true);
                        }
                        Some(PlannerEntry::Action(PlanAction::ResetDefaults)) => {
                            *plan = self.build_recommended_defaults();
                            self.save_config(&self.plan_to_config(plan))?;
                            target = None;
                            expanded = None;
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
            PlanField::MaiBotBranch,
            PlanField::InstallMode,
            PlanField::PythonEnv,
            PlanField::VenvMode,
            PlanField::GithubProxy,
            PlanField::PipSource,
            PlanField::BotProtocols,
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
            PlanField::MaiBotBranch => vec!["main（稳定版）".into(), "dev（开发版）".into()],
            PlanField::InstallMode => {
                vec!["正常更新/修复".into(), "全新安装（清空目标目录）".into()]
            }
            PlanField::PythonEnv => vec!["本机 Python".into(), "uv (Python 3.14)".into()],
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
                "仅 NapCatQQ Shell".into(),
                "仅 LuckyLilliaBot Desktop".into(),
                "暂不安装附加协议端".into(),
            ],
            PlanField::DockerMirror => vec!["Windows 不使用 Docker".into()],
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
            PlanField::BotProtocols => self.protocols_label(plan),
            PlanField::DockerMirror => "Windows 不使用 Docker".into(),
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
                } else if choice_idx >= 2 && choice_idx < 2 + github_mirrors().len() {
                    plan.github_proxy == github_mirrors()[choice_idx - 2]
                } else {
                    !plan.github_proxy.is_empty()
                        && plan.github_proxy != "https://github.com"
                        && !github_mirrors()
                            .iter()
                            .any(|mirror| *mirror == plan.github_proxy)
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
            PlanField::BotProtocols => match choice_idx {
                0 => plan.bot_protocols == vec![BotProtocol::NapCat],
                1 => plan.bot_protocols == vec![BotProtocol::LuckyLilliaBot],
                _ => plan.bot_protocols.is_empty(),
            },
            PlanField::DockerMirror => true,
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
            "Windows 安装计划",
            "↑/↓ 移动 · Enter 展开/收起 · Esc 返回 · Windows 10/11",
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
        let _ = std::io::stdout().flush();
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
                    plan.pip_display.clear();
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
                    plan.pip_host = custom.split('/').nth(2).unwrap_or_default().to_string();
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
                plan.docker_mirror = DockerMirror::Keep;
            }
            PlanField::DockerMirror => {
                plan.docker_mirror = DockerMirror::Keep;
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

    pub(crate) fn edit_install_path(&self, plan: &mut InstallPlan) -> Result<()> {
        let value: String = self.with_prompt_mode(|| {
            Input::with_theme(&self.theme)
                .with_prompt("安装目录")
                .default(plan.install_path.display().to_string())
                .interact_text()
                .map_err(Into::into)
        })?;
        plan.install_path = normalize_path(&value)?;
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
        plan.bot_protocols = match current.bot_protocols.as_str() {
            "llbot" => vec![BotProtocol::LuckyLilliaBot],
            "none" => Vec::new(),
            _ => vec![BotProtocol::NapCat],
        };
        Ok(plan)
    }

    pub(crate) fn build_recommended_defaults(&self) -> InstallPlan {
        let install_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\maimai"))
            .join("maimai");
        InstallPlan {
            install_path,
            install_mode: InstallMode::Normal,
            python_env: PythonEnv::Uv,
            venv_mode: VenvMode::Keep,
            maibot_branch: "main".into(),
            github_proxy: String::new(),
            pip_display: String::new(),
            pip_index: String::new(),
            pip_host: String::new(),
            uv_index: String::new(),
            bot_protocols: vec![BotProtocol::NapCat],
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
            mai_llbot_path: plan.install_path.join("LLBot").display().to_string(),
            mai_install_mode: match plan.install_mode {
                InstallMode::Normal => "normal".into(),
                InstallMode::Clean => "clean".into(),
            },
            mai_venv_mode: match plan.venv_mode {
                VenvMode::Keep => "keep".into(),
                VenvMode::Recreate => "recreate".into(),
            },
            maibot_branch: plan.maibot_branch.clone(),
            pip_display: plan.pip_display.clone(),
            pip_index: plan.pip_index.clone(),
            pip_host: plan.pip_host.clone(),
            bot_protocols: if plan.bot_protocols.is_empty() {
                "none".into()
            } else if plan.bot_protocols == vec![BotProtocol::LuckyLilliaBot] {
                "llbot".into()
            } else {
                "napcat".into()
            },
        }
    }

    pub(crate) fn run_install(&self, plan: &InstallPlan) -> Result<()> {
        let mut plan = plan.clone();
        if plan.github_proxy.trim().is_empty() {
            self.print_section("GitHub 线路测速", "自动选择最快的 GitHub / 镜像线路");
            plan.github_proxy = self.run_github_speedtest(plan.github_fallback)?;
        }
        if plan.install_mode == InstallMode::Clean {
            if self.cli_mode
                || Confirm::with_theme(&self.theme)
                    .with_prompt("确认清空安装目录后全新安装？")
                    .default(false)
                    .interact()?
            {
                clean_install_dir(&plan.install_path)?;
            } else {
                bail!("已取消全新安装");
            }
        }

        fs::create_dir_all(&plan.install_path)?;
        self.ensure_base_dependencies(&plan)?;
        self.clone_or_update_repo(
            &repo_url(
                &github_proxy_or_direct(&plan.github_proxy),
                "MaiM-with-u/MaiBot",
            ),
            &plan.install_path.join("MaiBot"),
            &plan.maibot_branch,
            plan.git_dirty_mode,
        )?;
        self.install_napcat_adapter(&plan)?;
        self.setup_python_env(&plan)?;
        for protocol in &plan.bot_protocols {
            match protocol {
                BotProtocol::NapCat => self.install_napcat(&plan)?,
                BotProtocol::LuckyLilliaBot => self.install_llbot(&plan)?,
            }
        }
        self.save_config(&self.plan_to_config(&plan))?;
        println!("Windows 安装 / 更新完成。");
        Ok(())
    }

    pub(crate) fn run_github_speedtest(&self, fallback: GithubFallbackMode) -> Result<String> {
        let mut mirrors = vec!["https://github.com".to_string()];
        mirrors.extend(github_mirrors().iter().map(|v| v.to_string()));
        println!("  {}", style("正在并行测试 git 访问，请稍候...").dim());
        self.print_line();

        let handles: Vec<_> = mirrors
            .into_iter()
            .map(|mirror| {
                thread::spawn(move || {
                    let repo = repo_url(&mirror, "MaiM-with-u/MaiBot");
                    let started = Instant::now();
                    let output = Command::new("git")
                        .args([
                            "-c",
                            "credential.helper=",
                            "-c",
                            "http.lowSpeedLimit=1",
                            "-c",
                            "http.lowSpeedTime=5",
                            "ls-remote",
                            "--heads",
                            &repo,
                            "main",
                        ])
                        .env("GIT_TERMINAL_PROMPT", "0")
                        .output();

                    match output {
                        Ok(output) if output.status.success() => (
                            mirror,
                            started.elapsed().as_secs_f64() * 1000.0,
                            true,
                            String::new(),
                        ),
                        Ok(output) => {
                            let detail = String::from_utf8_lossy(&output.stderr)
                                .lines()
                                .next()
                                .unwrap_or("git ls-remote 失败")
                                .trim()
                                .to_string();
                            (mirror, 9999.0, false, detail)
                        }
                        Err(e) => (mirror, 9999.0, false, format!("无法启动 git: {e}")),
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
        for (mirror, ms, ok, detail) in &results {
            if *ok {
                println!("  {} {}", style(format!("{:>6.0} ms", ms)).green(), mirror);
                if best.as_ref().map(|b| *ms < b.1).unwrap_or(true) {
                    best = Some((mirror.clone(), *ms));
                }
            } else if detail.is_empty() {
                println!("  {}     {}", style("  失败 ").red(), style(mirror).dim());
            } else {
                println!(
                    "  {}     {}  {}",
                    style("  失败 ").red(),
                    style(mirror).dim(),
                    style(detail).dim()
                );
            }
        }
        self.print_line();

        match best {
            Some((url, ms)) => {
                println!(
                    "  {} {} ({:.0} ms)",
                    style("✔ 已选择").green().bold(),
                    style(&url).cyan(),
                    ms
                );
                Ok(url)
            }
            None => {
                println!("  {}", style("✗ 全部线路连接失败").red().bold());
                match fallback {
                    GithubFallbackMode::Ask => {
                        let choice = Select::with_theme(&self.theme)
                            .with_prompt("请选择回退方案")
                            .items(["重试测速", "使用 GitHub 官方直连", "取消安装"])
                            .default(0)
                            .interact()?;
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

    fn ensure_base_dependencies(&self, plan: &InstallPlan) -> Result<()> {
        if !command_exists("git")? {
            self.run_shell(
                "where winget >nul 2>nul || (echo 未找到 git，也未找到 winget，请先安装 Git for Windows。 & exit /b 1)\r\nwinget install --id Git.Git -e --source winget --accept-package-agreements --accept-source-agreements",
            )?;
        }
        if plan.python_env == PythonEnv::Uv && !command_exists("uv")? {
            self.run_shell(
                "where winget >nul 2>nul || (echo 未找到 uv，也未找到 winget，请先安装 uv 或改用 --python system。 & exit /b 1)\r\nwinget install --id astral-sh.uv -e --source winget --accept-package-agreements --accept-source-agreements",
            )?;
        }
        if plan.python_env == PythonEnv::System
            && !command_exists("python")?
            && !command_exists("py")?
        {
            bail!("未找到 Python。请先安装 Python 3.12+，或使用 --python uv");
        }
        Ok(())
    }

    fn clone_or_update_repo(
        &self,
        repo: &str,
        target: &Path,
        branch: &str,
        dirty_mode: GitDirtyMode,
    ) -> Result<()> {
        if target.join(".git").exists() {
            self.handle_dirty_repo(target, dirty_mode)?;
            self.run_shell(&format!(
                "cd /d {}\r\ngit fetch --all --prune\r\ngit checkout {}\r\ngit pull --ff-only",
                bat_quote(target),
                bat_arg(branch)
            ))
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            self.run_shell(&format!(
                "git clone --branch {} --depth 1 {} {}",
                bat_arg(branch),
                bat_arg(repo),
                bat_quote(target)
            ))
        }
    }

    fn handle_dirty_repo(&self, target: &Path, dirty_mode: GitDirtyMode) -> Result<()> {
        let output = Command::new("cmd")
            .args([
                "/C",
                &format!("cd /d {} && git status --porcelain", bat_quote(target)),
            ])
            .output()?;
        let status = String::from_utf8_lossy(&output.stdout);
        if status.trim().is_empty() {
            return Ok(());
        }
        let only_uv_lock = status.lines().all(|line| line.ends_with(" uv.lock"));
        if target.file_name().and_then(|s| s.to_str()) == Some("MaiBot") && only_uv_lock {
            self.run_shell(&format!(
                "cd /d {}\r\ngit checkout -- uv.lock",
                bat_quote(target)
            ))?;
            return Ok(());
        }
        match dirty_mode {
            GitDirtyMode::Stash => self.run_shell(&format!(
                "cd /d {}\r\ngit stash push -u -m maibot-manager-windows",
                bat_quote(target)
            )),
            GitDirtyMode::Discard => self.run_shell(&format!(
                "cd /d {}\r\ngit reset --hard HEAD\r\ngit clean -fd",
                bat_quote(target)
            )),
            GitDirtyMode::Cancel => bail!("目标仓库存在本地改动: {}", target.display()),
            GitDirtyMode::Ask => {
                if self.cli_mode {
                    bail!("目标仓库存在本地改动，请使用 --git-dirty stash|discard|cancel");
                }
                let choice = Select::with_theme(&self.theme)
                    .with_prompt(format!("{} 存在本地改动，如何处理？", target.display()))
                    .items(["git stash 保存后继续", "丢弃本地改动", "取消"])
                    .default(2)
                    .interact()?;
                match choice {
                    0 => self.handle_dirty_repo(target, GitDirtyMode::Stash),
                    1 => self.handle_dirty_repo(target, GitDirtyMode::Discard),
                    _ => bail!("已取消：目标仓库存在本地改动"),
                }
            }
        }
    }

    fn install_napcat_adapter(&self, plan: &InstallPlan) -> Result<()> {
        let plugins_dir = plan.install_path.join("MaiBot").join("plugins");
        let target = plugins_dir.join(NAPCAT_ADAPTER_PLUGIN_ID);
        self.clone_or_update_repo(
            &repo_url(
                &plan.github_proxy,
                &format!("Mai-with-u/{NAPCAT_ADAPTER_REPO_NAME}"),
            ),
            &target,
            "main",
            plan.git_dirty_mode,
        )
    }

    pub(crate) fn setup_python_env(&self, plan: &InstallPlan) -> Result<()> {
        let root = &plan.install_path;
        let maibot_dir = root.join("MaiBot");
        let adapter_req = maibot_dir
            .join("plugins")
            .join(NAPCAT_ADAPTER_PLUGIN_ID)
            .join("requirements.txt");
        match plan.python_env {
            PythonEnv::Uv => {
                let index = if plan.uv_index.is_empty() {
                    String::new()
                } else {
                    format!(
                        "set UV_INDEX_URL={}\r\nset PIP_INDEX_URL={}\r\n",
                        plan.uv_index, plan.uv_index
                    )
                };
                if plan.venv_mode == VenvMode::Recreate && maibot_dir.join(".venv").exists() {
                    self.remove_env_dir_safely(&maibot_dir.join(".venv"), root)?;
                }
                self.run_shell(&format!(
                    "cd /d {}\r\n{}if not exist .venv uv venv --python 3.14\r\nuv sync\r\nif exist {} uv pip install -r {}",
                    bat_quote(&maibot_dir),
                    index,
                    bat_quote(&adapter_req),
                    bat_quote(&adapter_req)
                ))
            }
            PythonEnv::System => {
                let venv_dir = root.join("venv");
                if plan.venv_mode == VenvMode::Recreate && venv_dir.exists() {
                    self.remove_env_dir_safely(&venv_dir, root)?;
                }
                let pip_index = if plan.pip_index.is_empty() {
                    String::new()
                } else {
                    format!("set PIP_INDEX_URL={}\r\n", plan.pip_index)
                };
                self.run_shell(&format!(
                    "cd /d {}\r\nif not exist venv python -m venv venv\r\ncall venv\\Scripts\\activate.bat\r\n{}python -m pip install --upgrade pip\r\nif exist MaiBot\\requirements.txt pip install -r MaiBot\\requirements.txt\r\nif exist {} pip install -r {}",
                    bat_quote(root),
                    pip_index,
                    bat_quote(&adapter_req),
                    bat_quote(&adapter_req)
                ))
            }
        }
    }

    fn remove_env_dir_safely(&self, env_dir: &Path, install_root: &Path) -> Result<()> {
        if !env_dir.exists() {
            return Ok(());
        }
        let canonical_env = env_dir.canonicalize()?;
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
        fs::remove_dir_all(env_dir)?;
        Ok(())
    }

    pub(crate) fn install_napcat(&self, plan: &InstallPlan) -> Result<()> {
        let napcat_dir = plan.install_path.join("NapCat");
        fs::create_dir_all(&napcat_dir)?;
        let release = self.fetch_latest_release_asset(
            "NapNeko/NapCatQQ",
            &["NapCat.Shell.zip"],
            &plan.github_proxy,
        )?;
        let current_tag = self.current_release_tag(&napcat_dir, NAPCAT_RELEASE_TAG_FILE);
        if current_tag.as_deref() == Some(release.tag_name.as_str())
            && napcat_dir.join("launcher.bat").exists()
        {
            println!("NapCat Shell 已是最新版本 {}，跳过更新", release.tag_name);
            return Ok(());
        }
        let zip_path = napcat_dir.join("NapCat.Shell.zip");
        let backup_config = napcat_dir.join(".maibot-napcat-config-backup");
        let script = format!(
            "if exist {backup} rmdir /s /q {backup}\r\nif exist {config} xcopy {config} {backup}\\ /e /i /y >nul\r\ncurl.exe -fL --retry 3 --connect-timeout 10 -o {zip} {url}\r\nfor /d %%D in ({napcat}\\*) do if /i not \"%%~nxD\"==\".maibot-napcat-config-backup\" rmdir /s /q \"%%D\"\r\nfor %%F in ({napcat}\\*) do if /i not \"%%~nxF\"==\"NapCat.Shell.zip\" del /q \"%%F\"\r\ntar -xf {zip} -C {napcat}\r\nif exist {backup} xcopy {backup} {config}\\ /e /i /y >nul\r\nif exist {backup} rmdir /s /q {backup}",
            backup = bat_quote(&backup_config),
            config = bat_quote(&napcat_dir.join("config")),
            zip = bat_quote(&zip_path),
            url = bat_arg(&release.asset_url),
            napcat = bat_quote(&napcat_dir)
        );
        self.run_shell(&script)?;
        fs::write(
            napcat_dir.join(NAPCAT_RELEASE_TAG_FILE),
            format!("{}\n", release.tag_name),
        )?;
        Ok(())
    }

    pub(crate) fn install_llbot(&self, plan: &InstallPlan) -> Result<()> {
        let llbot_dir = plan.install_path.join("LLBot");
        fs::create_dir_all(&llbot_dir)?;
        let release = self.fetch_latest_release_asset(
            "LLOneBot/LuckyLilliaBot",
            &["LLBot-Desktop-win-x64.zip"],
            &plan.github_proxy,
        )?;
        let current_tag = self.current_release_tag(&llbot_dir, LLBOT_RELEASE_TAG_FILE);
        if current_tag.as_deref() == Some(release.tag_name.as_str())
            && llbot_dir.join("llbot.exe").exists()
        {
            println!(
                "LuckyLilliaBot Desktop 已是最新版本 {}，跳过更新",
                release.tag_name
            );
            return Ok(());
        }
        if llbot_dir.join("llbot.exe").exists() {
            match plan.llbot_update_mode {
                LlbotUpdateMode::Update => {}
                LlbotUpdateMode::Skip => return Ok(()),
                LlbotUpdateMode::Prompt if !self.cli_mode => {
                    if !Confirm::with_theme(&self.theme)
                        .with_prompt(format!(
                            "检测到 LuckyLilliaBot Desktop 可更新到 {}，是否更新？",
                            release.tag_name
                        ))
                        .default(false)
                        .interact()?
                    {
                        return Ok(());
                    }
                }
                LlbotUpdateMode::Prompt => {
                    bail!("LuckyLilliaBot 可更新，请使用 --llbot-update update|skip");
                }
            }
        }
        let zip_path = llbot_dir.join("LLBot-Desktop-win-x64.zip");
        let data_dir = llbot_dir.join("bin").join("llbot").join("data");
        let config_path = llbot_dir
            .join("bin")
            .join("llbot")
            .join("default_config.json");
        let backup_dir = llbot_dir.join(".maibot-llbot-backup");
        let script = format!(
            "if exist {backup} rmdir /s /q {backup}\r\nmkdir {backup}\r\nif exist {data} xcopy {data} {backup}\\data\\ /e /i /y >nul\r\nif exist {config} copy /y {config} {backup}\\default_config.json >nul\r\ncurl.exe -fL --retry 3 --connect-timeout 10 -o {zip} {url}\r\nfor /d %%D in ({llbot}\\*) do if /i not \"%%~nxD\"==\".maibot-llbot-backup\" rmdir /s /q \"%%D\"\r\nfor %%F in ({llbot}\\*) do if /i not \"%%~nxF\"==\"LLBot-Desktop-win-x64.zip\" del /q \"%%F\"\r\ntar -xf {zip} -C {llbot}\r\nif exist {backup}\\data xcopy {backup}\\data {data}\\ /e /i /y >nul\r\nif exist {backup}\\default_config.json copy /y {backup}\\default_config.json {config} >nul\r\nrmdir /s /q {backup}",
            backup = bat_quote(&backup_dir),
            data = bat_quote(&data_dir),
            config = bat_quote(&config_path),
            zip = bat_quote(&zip_path),
            url = bat_arg(&release.asset_url),
            llbot = bat_quote(&llbot_dir)
        );
        self.run_shell(&script)?;
        fs::write(
            llbot_dir.join(LLBOT_RELEASE_TAG_FILE),
            format!("{}\n", release.tag_name),
        )?;
        Ok(())
    }

    fn fetch_latest_release_asset(
        &self,
        repo: &str,
        asset_names: &[&str],
        github_proxy: &str,
    ) -> Result<ReleaseAssetInfo> {
        let api_url = format!("https://api.github.com/repos/{repo}/releases/latest");
        let accelerated_api_url = accelerate_github_url(&api_url, github_proxy);
        let output = Command::new("cmd")
            .args([
                "/C",
                &format!(
                    "curl.exe -fsSL -H \"Accept: application/vnd.github+json\" -H \"User-Agent: maibot-manager-tui\" {}",
                    bat_arg(&accelerated_api_url)
                ),
            ])
            .output()
            .with_context(|| format!("获取 GitHub 最新 release 失败: {api_url}"))?;
        if !output.status.success() {
            bail!("获取 GitHub 最新 release 失败: {api_url}");
        }
        let data: Value = serde_json::from_slice(&output.stdout)?;
        let tag_name = data
            .get("tag_name")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| anyhow!("GitHub release 缺少 tag_name: {repo}"))?
            .to_string();
        let asset_url = data
            .get("assets")
            .and_then(Value::as_array)
            .and_then(|assets| {
                assets.iter().find_map(|asset| {
                    let name = asset.get("name").and_then(Value::as_str)?;
                    asset_names
                        .contains(&name)
                        .then(|| asset.get("browser_download_url").and_then(Value::as_str))
                        .flatten()
                })
            })
            .ok_or_else(|| {
                anyhow!(
                    "GitHub release 未找到 Windows 资产包: {}",
                    asset_names.join(", ")
                )
            })?;
        Ok(ReleaseAssetInfo {
            tag_name,
            asset_url: accelerate_github_url(asset_url, github_proxy),
        })
    }

    fn current_release_tag(&self, dir: &Path, file_name: &str) -> Option<String> {
        fs::read_to_string(dir.join(file_name))
            .ok()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
    }

    fn protocols_label(&self, plan: &InstallPlan) -> String {
        if plan.bot_protocols.is_empty() {
            return "暂不安装".into();
        }
        plan.bot_protocols
            .iter()
            .map(|protocol| protocol.label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn github_proxy_or_direct(proxy: &str) -> String {
    if proxy.trim().is_empty() {
        "https://github.com".into()
    } else {
        proxy.to_string()
    }
}

fn accelerate_github_url(url: &str, proxy: &str) -> String {
    let proxy = github_proxy_or_direct(proxy);
    if proxy == "https://github.com" || !url.contains("github.com/") {
        url.to_string()
    } else {
        format!("{}/{}", proxy.trim_end_matches('/'), url)
    }
}
