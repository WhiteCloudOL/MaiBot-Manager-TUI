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
            PlanField::InstallPath => vec!["自定义路径".into()],
            PlanField::MaiBotBranch => vec!["main（稳定版）".into(), "dev（预览版）".into()],
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
            "↑/↓ 移动 · ←/→ 展开收起 · Enter/Space 应用 · Esc 返回 · Windows 10/11",
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
            &plan.install_path,
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
        println!("  {}", style("正在并行测试 GitHub 访问，请稍候...").dim());
        self.print_line();

        let handles: Vec<_> = mirrors
            .into_iter()
            .map(|mirror| {
                thread::spawn(move || {
                    let test_url = accelerate_github_url(TEST_FILE_PATH, &mirror);
                    let started = Instant::now();
                    let output = Command::new("curl.exe")
                        .args([
                            "-fsSL",
                            "--max-time",
                            "8",
                            "--connect-timeout",
                            "4",
                            "-o",
                            "NUL",
                            "-w",
                            "%{time_total}",
                            &test_url,
                        ])
                        .output();

                    match output {
                        Ok(output) if output.status.success() => {
                            let measured = String::from_utf8_lossy(&output.stdout)
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .map(|sec| sec * 1000.0)
                                .unwrap_or_else(|| started.elapsed().as_secs_f64() * 1000.0);
                            (mirror, measured, true, String::new())
                        }
                        Ok(output) => {
                            let detail = String::from_utf8_lossy(&output.stderr)
                                .lines()
                                .next()
                                .unwrap_or("curl 探测失败")
                                .trim()
                                .to_string();
                            (mirror, 9999.0, false, detail)
                        }
                        Err(e) => (mirror, 9999.0, false, format!("无法启动 curl.exe: {e}")),
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
        let root = &plan.install_path;
        fs::create_dir_all(tools_dir(root))?;

        if portable_git_exe(root).exists() {
            println!("使用安装目录内 Git: {}", portable_git_exe(root).display());
        } else if command_exists("git")? {
            println!("使用系统 Git；如需完全便携，可删除系统 Git 后重新执行安装。");
        } else {
            self.install_portable_git(plan)?;
        }

        let needs_uv = plan.python_env == PythonEnv::Uv
            || (plan.python_env == PythonEnv::System
                && !command_exists_with_tools(root, "python")?
                && !command_exists_with_tools(root, "py")?);
        if needs_uv {
            if portable_uv_exe(root).exists() {
                println!("使用安装目录内 uv: {}", portable_uv_exe(root).display());
            } else if command_exists("uv")? {
                println!("使用系统 uv，并将 uv 缓存/Python 下载目录固定到安装目录。");
            } else {
                self.install_portable_uv(plan)?;
            }
        }

        if plan.python_env == PythonEnv::System
            && !command_exists_with_tools(root, "python")?
            && !command_exists_with_tools(root, "py")?
        {
            println!("未找到本机 Python，将使用安装目录内 uv 创建本地 Python 虚拟环境。");
        }

        Ok(())
    }

    fn install_portable_git(&self, plan: &InstallPlan) -> Result<()> {
        let root = &plan.install_path;
        let tools = tools_dir(root);
        let git_dir = portable_git_dir(root);
        let git_tmp = tools.join("git-extract");
        let zip_path = tools.join("MinGit.zip");
        let release = self.fetch_latest_release_asset_matching(
            "git-for-windows/git",
            "MinGit-*-64-bit.zip",
            &plan.github_proxy,
            |name| {
                name.starts_with("MinGit-")
                    && name.ends_with("-64-bit.zip")
                    && !name.contains("busybox")
            },
        )?;
        println!("正在下载便携 Git: {}", release.tag_name);
        let script = format!(
            "if not exist {tools} mkdir {tools}\r\n\
             if exist {tmp} rmdir /s /q {tmp}\r\n\
             if exist {zip} del /q {zip}\r\n\
             mkdir {tmp}\r\n\
             curl.exe -fL --retry 3 --connect-timeout 10 -o {zip} {url} || exit /b 1\r\n\
             tar -xf {zip} -C {tmp} || exit /b 1\r\n\
             if not exist {tmp_git} (echo Git 便携包结构异常，未找到 cmd\\git.exe & exit /b 1)\r\n\
             if exist {git} rmdir /s /q {git}\r\n\
             move /y {tmp} {git} >nul || exit /b 1\r\n\
             if not exist {git_exe} (echo Git 安装失败，未找到 git.exe & exit /b 1)\r\n\
             del /q {zip}",
            tools = bat_quote(&tools),
            tmp = bat_quote(&git_tmp),
            zip = bat_quote(&zip_path),
            url = bat_arg(&release.asset_url),
            tmp_git = bat_quote(&git_tmp.join("cmd").join("git.exe")),
            git = bat_quote(&git_dir),
            git_exe = bat_quote(&portable_git_exe(root)),
        );
        self.run_shell(&script)
    }

    fn install_portable_uv(&self, plan: &InstallPlan) -> Result<()> {
        let root = &plan.install_path;
        let tools = tools_dir(root);
        let uv_dir = portable_uv_dir(root);
        let uv_tmp = tools.join("uv-extract");
        let zip_path = tools.join("uv-x86_64-pc-windows-msvc.zip");
        let release = self.fetch_latest_release_asset(
            "astral-sh/uv",
            &["uv-x86_64-pc-windows-msvc.zip"],
            &plan.github_proxy,
        )?;
        println!("正在下载便携 uv: {}", release.tag_name);
        let script = format!(
            "if not exist {tools} mkdir {tools}\r\n\
             if exist {tmp} rmdir /s /q {tmp}\r\n\
             if exist {zip} del /q {zip}\r\n\
             if exist {uv_dir} rmdir /s /q {uv_dir}\r\n\
             mkdir {tmp}\r\n\
             mkdir {uv_dir}\r\n\
             curl.exe -fL --retry 3 --connect-timeout 10 -o {zip} {url} || exit /b 1\r\n\
             tar -xf {zip} -C {tmp} || exit /b 1\r\n\
             for /r {tmp} %%F in (uv.exe) do if not exist {uv_exe} copy /y \"%%F\" {uv_exe} >nul\r\n\
             for /r {tmp} %%F in (uvx.exe) do if not exist {uvx_exe} copy /y \"%%F\" {uvx_exe} >nul\r\n\
             if not exist {uv_exe} (echo uv 便携包结构异常，未找到 uv.exe & exit /b 1)\r\n\
             rmdir /s /q {tmp}\r\n\
             del /q {zip}",
            tools = bat_quote(&tools),
            tmp = bat_quote(&uv_tmp),
            zip = bat_quote(&zip_path),
            uv_dir = bat_quote(&uv_dir),
            url = bat_arg(&release.asset_url),
            uv_exe = bat_quote(&portable_uv_exe(root)),
            uvx_exe = bat_quote(&uv_dir.join("uvx.exe")),
        );
        self.run_shell(&script)
    }

    fn clone_or_update_repo(
        &self,
        root: &Path,
        repo: &str,
        target: &Path,
        branch: &str,
        dirty_mode: GitDirtyMode,
    ) -> Result<()> {
        if target.join(".git").exists() {
            self.handle_dirty_repo(root, target, dirty_mode)?;
            self.run_shell(&with_windows_tools_path(
                root,
                &format!(
                    "cd /d {}\r\ngit fetch --all --prune\r\ngit checkout {}\r\ngit pull --ff-only",
                    bat_quote(target),
                    bat_arg(branch)
                ),
            ))
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            self.run_shell(&with_windows_tools_path(
                root,
                &format!(
                    "git clone --branch {} --depth 1 {} {}",
                    bat_arg(branch),
                    bat_arg(repo),
                    bat_quote(target)
                ),
            ))
        }
    }

    fn handle_dirty_repo(
        &self,
        root: &Path,
        target: &Path,
        dirty_mode: GitDirtyMode,
    ) -> Result<()> {
        let mut command = Command::new(git_executable(root));
        apply_windows_tools_env(&mut command, root);
        let output = command
            .args(["status", "--porcelain"])
            .current_dir(target)
            .output()?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("git status 执行失败")
                .trim()
                .to_string();
            bail!("检查 Git 状态失败: {} ({detail})", target.display());
        }
        let status = String::from_utf8_lossy(&output.stdout);
        if status.trim().is_empty() {
            return Ok(());
        }
        let only_uv_lock = status.lines().all(|line| line.ends_with(" uv.lock"));
        if target.file_name().and_then(|s| s.to_str()) == Some("MaiBot") && only_uv_lock {
            self.run_shell(&with_windows_tools_path(
                root,
                &format!(
                    "cd /d {}\r\ngit reset -- uv.lock\r\ngit checkout -- uv.lock",
                    bat_quote(target)
                ),
            ))?;
            return Ok(());
        }
        match dirty_mode {
            GitDirtyMode::Stash => self.run_shell(&with_windows_tools_path(
                root,
                &format!(
                    "cd /d {}\r\ngit stash push -u -m maibot-manager-windows",
                    bat_quote(target)
                ),
            )),
            GitDirtyMode::Discard => self.run_shell(&with_windows_tools_path(
                root,
                &format!(
                    "cd /d {}\r\ngit reset --hard HEAD\r\ngit clean -fd",
                    bat_quote(target)
                ),
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
                    0 => self.handle_dirty_repo(root, target, GitDirtyMode::Stash),
                    1 => self.handle_dirty_repo(root, target, GitDirtyMode::Discard),
                    _ => bail!("已取消：目标仓库存在本地改动"),
                }
            }
        }
    }

    fn install_napcat_adapter(&self, plan: &InstallPlan) -> Result<()> {
        let plugins_dir = plan.install_path.join("MaiBot").join("plugins");
        let target = plugins_dir.join(NAPCAT_ADAPTER_PLUGIN_ID);
        self.clone_or_update_repo(
            &plan.install_path,
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
                self.run_shell(&with_windows_tools_path(root, &format!(
                    "cd /d {}\r\n{}if not exist .venv uv venv --python 3.14\r\nuv sync\r\nif exist {} uv pip install -r {}",
                    bat_quote(&maibot_dir),
                    index,
                    bat_quote(&adapter_req),
                    bat_quote(&adapter_req)
                )))
            }
            PythonEnv::System => {
                let venv_dir = root.join("venv");
                let python = venv_dir.join("Scripts").join("python.exe");
                if plan.venv_mode == VenvMode::Recreate && venv_dir.exists() {
                    self.remove_env_dir_safely(&venv_dir, root)?;
                }
                let pip_index = if plan.pip_index.is_empty() {
                    String::new()
                } else {
                    format!("set PIP_INDEX_URL={}\r\n", plan.pip_index)
                };
                self.run_shell(&with_windows_tools_path(root, &format!(
                    "cd /d {}\r\n\
                     if not exist {} (\r\n\
                     where python >nul 2>nul\r\n\
                     if not errorlevel 1 (\r\n\
                     python -m venv venv\r\n\
                     ) else (\r\n\
                     where py >nul 2>nul\r\n\
                     if not errorlevel 1 (\r\n\
                     py -3 -m venv venv\r\n\
                     ) else (\r\n\
                     uv venv --python 3.14 venv\r\n\
                     )\r\n\
                     )\r\n\
                     )\r\n\
                     if not exist {} (echo Python 虚拟环境创建失败: {} & exit /b 1)\r\n\
                     {}{} -m pip install --upgrade pip\r\n\
                     if exist MaiBot\\requirements.txt {} -m pip install -r MaiBot\\requirements.txt\r\n\
                     if exist {} {} -m pip install -r {}",
                    bat_quote(root),
                    bat_quote(&python),
                    bat_quote(&python),
                    python.display(),
                    pip_index,
                    bat_quote(&python),
                    bat_quote(&python),
                    bat_quote(&adapter_req),
                    bat_quote(&python),
                    bat_quote(&adapter_req)
                )))
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
            "if exist {backup} rmdir /s /q {backup}\r\nif exist {config} xcopy {config} {backup} /e /i /y >nul || exit /b 1\r\ncurl.exe -fL --retry 3 --connect-timeout 10 -o {zip} {url} || exit /b 1\r\nfor /d %%D in ({napcat}\\*) do if /i not \"%%~nxD\"==\".maibot-napcat-config-backup\" rmdir /s /q \"%%D\"\r\nfor %%F in ({napcat}\\*) do if /i not \"%%~nxF\"==\"NapCat.Shell.zip\" del /q \"%%F\"\r\ntar -xf {zip} -C {napcat} || exit /b 1\r\nif exist {backup} xcopy {backup} {config} /e /i /y >nul || exit /b 1\r\nif exist {backup} rmdir /s /q {backup}",
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

    pub(crate) fn redownload_napcat_shell(&self, plan: &InstallPlan) -> Result<()> {
        let napcat_dir = plan.install_path.join("NapCat");
        let _ = fs::remove_file(napcat_dir.join(NAPCAT_RELEASE_TAG_FILE));
        self.install_napcat(plan)
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
        let backup_data_dir = backup_dir.join("data");
        let backup_config_path = backup_dir.join("default_config.json");
        let script = format!(
            "if exist {backup} rmdir /s /q {backup}\r\nmkdir {backup}\r\nif exist {data} xcopy {data} {backup_data} /e /i /y >nul || exit /b 1\r\nif exist {config} copy /y {config} {backup_config} >nul || exit /b 1\r\ncurl.exe -fL --retry 3 --connect-timeout 10 -o {zip} {url} || exit /b 1\r\nfor /d %%D in ({llbot}\\*) do if /i not \"%%~nxD\"==\".maibot-llbot-backup\" rmdir /s /q \"%%D\"\r\nfor %%F in ({llbot}\\*) do if /i not \"%%~nxF\"==\"LLBot-Desktop-win-x64.zip\" del /q \"%%F\"\r\ntar -xf {zip} -C {llbot} || exit /b 1\r\nif exist {backup_data} xcopy {backup_data} {data} /e /i /y >nul || exit /b 1\r\nif exist {backup_config} copy /y {backup_config} {config} >nul || exit /b 1\r\nrmdir /s /q {backup}",
            backup = bat_quote(&backup_dir),
            backup_data = bat_quote(&backup_data_dir),
            backup_config = bat_quote(&backup_config_path),
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
        self.fetch_latest_release_asset_matching(
            repo,
            &asset_names.join(", "),
            github_proxy,
            |name| asset_names.contains(&name),
        )
    }

    fn fetch_latest_release_asset_matching<F>(
        &self,
        repo: &str,
        asset_description: &str,
        github_proxy: &str,
        matches_asset: F,
    ) -> Result<ReleaseAssetInfo>
    where
        F: Fn(&str) -> bool,
    {
        let api_url = format!("https://api.github.com/repos/{repo}/releases/latest");
        let mut errors = Vec::new();

        for candidate in github_proxy_candidates(github_proxy) {
            let accelerated_api_url = accelerate_github_url(&api_url, &candidate);
            let output = Command::new("curl.exe")
                .args([
                    "-fsSL",
                    "--retry",
                    "2",
                    "--connect-timeout",
                    "10",
                    "--max-time",
                    "45",
                    "-H",
                    "Accept: application/vnd.github+json",
                    "-H",
                    "User-Agent: maibot-manager-tui",
                    &accelerated_api_url,
                ])
                .output()
                .with_context(|| format!("获取 GitHub 最新 release 失败: {api_url}"))?;
            if !output.status.success() {
                let detail = String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("curl 请求失败")
                    .trim()
                    .to_string();
                errors.push(format!("{candidate}: {detail}"));
                continue;
            }
            let data: Value = match serde_json::from_slice(&output.stdout) {
                Ok(data) => data,
                Err(error) => {
                    errors.push(format!("{candidate}: release JSON 解析失败: {error}"));
                    continue;
                }
            };
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
                        matches_asset(name)
                            .then(|| asset.get("browser_download_url").and_then(Value::as_str))
                            .flatten()
                    })
                })
                .ok_or_else(|| {
                    anyhow!(
                        "GitHub release 未找到 Windows 资产包: {}",
                        asset_description
                    )
                })?;
            return Ok(ReleaseAssetInfo {
                tag_name,
                asset_url: accelerate_github_url(asset_url, &candidate),
            });
        }

        if errors.is_empty() {
            bail!("获取 GitHub 最新 release 失败: {api_url}");
        } else {
            let detail = errors.into_iter().take(4).collect::<Vec<_>>().join("; ");
            bail!("获取 GitHub 最新 release 失败: {api_url} ({detail})");
        }
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

fn github_proxy_candidates(primary: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut push_unique = |value: String| {
        if !candidates.iter().any(|existing| existing == &value) {
            candidates.push(value);
        }
    };

    push_unique(github_proxy_or_direct(primary));
    push_unique("https://github.com".to_string());
    for mirror in github_mirrors() {
        push_unique((*mirror).to_string());
    }
    candidates
}

fn accelerate_github_url(url: &str, proxy: &str) -> String {
    let proxy = github_proxy_or_direct(proxy);
    let is_github_url = url.contains("github.com/") || url.contains("githubusercontent.com/");
    if proxy == "https://github.com" || !is_github_url {
        url.to_string()
    } else {
        format!("{}/{}", proxy.trim_end_matches('/'), url)
    }
}
