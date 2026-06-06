use crate::{
    app::App,
    model::*,
    plugins::{NAPCAT_ADAPTER_PLUGIN_ID, NAPCAT_ADAPTER_REPO_NAME},
    utils::*,
};
use anyhow::{Context, Result, anyhow, bail};
use dialoguer::{Confirm, Input, Select};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
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
        _current: &AppConfig,
        plan: &mut InstallPlan,
    ) -> Result<bool> {
        self.clear();
        self.print_header(Some(plan));
        self.print_section("Windows 安装计划", "Windows 10/11 · cmd/BAT 优先执行");
        self.print_kv("目录", &plan.install_path.display().to_string());
        self.print_kv("主程序分支", &plan.maibot_branch);
        self.print_kv("Python", plan.python_env.label());
        self.print_kv("协议端", &self.protocols_label(plan));
        self.print_line();

        let items = [
            "开始安装 / 更新",
            "修改安装目录",
            "切换 MaiBot 分支",
            "切换 Python 环境",
            "选择协议端",
            "取消返回",
        ];
        loop {
            let choice = Select::with_theme(&self.theme)
                .with_prompt("Windows 安装 / 更新")
                .items(items)
                .default(0)
                .interact()?;
            match choice {
                0 => return Ok(true),
                1 => {
                    let value: String = Input::with_theme(&self.theme)
                        .with_prompt("安装目录")
                        .default(plan.install_path.display().to_string())
                        .interact_text()?;
                    plan.install_path = normalize_path(&value)?;
                }
                2 => {
                    let branch = Select::with_theme(&self.theme)
                        .with_prompt("MaiBot 分支")
                        .items(["main（稳定版）", "dev（开发版）"])
                        .default(if plan.maibot_branch == "dev" { 1 } else { 0 })
                        .interact()?;
                    plan.maibot_branch = if branch == 1 { "dev" } else { "main" }.into();
                }
                3 => {
                    let py = Select::with_theme(&self.theme)
                        .with_prompt("Python 环境")
                        .items(["uv (推荐)", "系统 Python"])
                        .default(if plan.python_env == PythonEnv::System {
                            1
                        } else {
                            0
                        })
                        .interact()?;
                    plan.python_env = if py == 1 {
                        PythonEnv::System
                    } else {
                        PythonEnv::Uv
                    };
                }
                4 => {
                    let protocol = Select::with_theme(&self.theme)
                        .with_prompt("协议端")
                        .items(["NapCatQQ (Shell)", "LuckyLilliaBot Desktop", "暂不安装"])
                        .default(0)
                        .interact()?;
                    plan.bot_protocols = match protocol {
                        0 => vec![BotProtocol::NapCat],
                        1 => vec![BotProtocol::LuckyLilliaBot],
                        _ => Vec::new(),
                    };
                }
                _ => return Ok(false),
            }
            self.save_config(&self.plan_to_config(plan))?;
        }
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
            plan.github_proxy = self.select_github_proxy();
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

    fn select_github_proxy(&self) -> String {
        let mut candidates = github_mirrors().to_vec();
        candidates.push("https://github.com");
        let mut best: Option<(&str, f64)> = None;
        for candidate in candidates {
            let url = accelerate_github_url(TEST_FILE_PATH, candidate);
            let output = Command::new("cmd")
                .args([
                    "/C",
                    &format!(
                        "curl.exe -sL -o NUL -w \"%%{{time_total}}\" --connect-timeout 5 --max-time 15 {}",
                        bat_arg(&url)
                    ),
                ])
                .output();
            let Ok(output) = output else {
                continue;
            };
            if !output.status.success() {
                continue;
            }
            let elapsed = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<f64>()
                .unwrap_or(f64::MAX);
            if elapsed.is_finite() && best.is_none_or(|(_, best_elapsed)| elapsed < best_elapsed) {
                best = Some((candidate, elapsed));
            }
        }
        let selected = best.map(|(proxy, _)| proxy).unwrap_or("https://github.com");
        println!("GitHub 线路: {selected}");
        selected.to_string()
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
