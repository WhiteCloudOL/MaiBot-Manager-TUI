use crate::{
    app::App,
    model::InstallMode,
    ui::ActionItem,
    utils::{
        apply_windows_tools_env, bat_arg, bat_quote, convert_github_url, list_plugins,
        plugin_dir_name, with_windows_tools_path,
    },
};
use anyhow::{Context, Result, bail};
use dialoguer::{Input, Select};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(crate) struct PluginSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) version: String,
    pub(crate) description: String,
    pub(crate) dir_name: String,
}

pub(crate) const NAPCAT_ADAPTER_REPO_NAME: &str = "MaiBot-Napcat-Adapter";
pub(crate) const NAPCAT_ADAPTER_PLUGIN_ID: &str = "maibot-team.napcat-adapter";

impl App {
    fn plugin_context(&self) -> Result<(PathBuf, PathBuf, PathBuf, String)> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let maibot_dir = root.join("MaiBot");
        let plugins_dir = maibot_dir.join("plugins");
        fs::create_dir_all(&plugins_dir)?;
        Ok((root, maibot_dir, plugins_dir, cfg.mai_python_env))
    }

    pub(crate) fn plugin_id_from_dir(&self, dir: &Path) -> Result<String> {
        let manifest_path = dir.join("_manifest.json");
        let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)
            .with_context(|| format!("解析插件清单失败: {}", manifest_path.display()))?;
        manifest["id"]
            .as_str()
            .map(str::to_string)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("插件清单缺少有效 id: {}", manifest_path.display()))
    }

    pub(crate) fn read_plugin_summary(&self, dir: &Path) -> Result<PluginSummary> {
        let manifest_path = dir.join("_manifest.json");
        let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)
            .with_context(|| format!("解析插件清单失败: {}", manifest_path.display()))?;
        let id = manifest["id"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let name = manifest["name"]
            .as_str()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| {
                if id.is_empty() {
                    "未命名插件"
                } else {
                    &id
                }
            })
            .to_string();
        let author = manifest["author"]
            .as_str()
            .unwrap_or("未知作者")
            .to_string();
        let version = manifest["version"].as_str().unwrap_or("未标注").to_string();
        let description = manifest["description"]
            .as_str()
            .or_else(|| manifest["desc"].as_str())
            .unwrap_or("未提供描述")
            .to_string();
        Ok(PluginSummary {
            id,
            name,
            author,
            version,
            description,
            dir_name: dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string(),
        })
    }

    pub(crate) fn resolve_plugin_dir_by_id(
        &self,
        plugins_dir: &Path,
        plugin_id: &str,
    ) -> Result<Option<PathBuf>> {
        let preferred = plugins_dir.join(plugin_dir_name(plugin_id));
        if preferred.exists() {
            return Ok(Some(preferred));
        }
        if !plugins_dir.exists() {
            return Ok(None);
        }
        for entry in fs::read_dir(plugins_dir)? {
            let path = entry?.path();
            if path.is_dir() && self.plugin_id_from_dir(&path).ok().as_deref() == Some(plugin_id) {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    pub(crate) fn require_plugin_dir_by_id(
        &self,
        plugins_dir: &Path,
        plugin_id: &str,
    ) -> Result<PathBuf> {
        self.resolve_plugin_dir_by_id(plugins_dir, plugin_id)?
            .ok_or_else(|| anyhow::anyhow!("未找到插件目录: {plugin_id}"))
    }

    fn clone_or_update_plugin(&self, root: &Path, url: &str, target: &Path) -> Result<()> {
        if target.join(".git").exists() {
            self.run_shell(&with_windows_tools_path(
                root,
                &format!("cd /d {}\r\ngit pull --ff-only", bat_quote(target)),
            ))
        } else {
            self.run_shell(&with_windows_tools_path(
                root,
                &format!("git clone {} {}", bat_arg(url), bat_quote(target)),
            ))
        }
    }

    fn plugin_backup_path(&self, path: &Path) -> PathBuf {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin");
        for index in 1.. {
            let candidate = parent.join(format!("{stem}.legacy-backup-{index}"));
            if !candidate.exists() {
                return candidate;
            }
        }
        unreachable!()
    }

    fn move_plugin_dir_to_backup(&self, path: &Path) -> Result<PathBuf> {
        let backup = self.plugin_backup_path(path);
        fs::rename(path, &backup).with_context(|| {
            format!(
                "插件目录冲突，无法将 {} 迁移到备份目录 {}",
                path.display(),
                backup.display()
            )
        })?;
        println!(
            "检测到插件目录冲突，已将旧目录移动到备份位置: {}",
            backup.display()
        );
        Ok(backup)
    }

    pub(crate) fn sync_plugin_repo_with_manifest_dir_at_root(
        &self,
        root: &Path,
        url: &str,
        plugins_dir: &Path,
        repo_name: &str,
        _branch: Option<&str>,
        _mode: InstallMode,
    ) -> Result<PathBuf> {
        fs::create_dir_all(plugins_dir)?;
        let repo_path = plugins_dir.join(repo_name);
        let mut target = repo_path.clone();

        if repo_path.exists() {
            let plugin_id = self.plugin_id_from_dir(&repo_path)?;
            let canonical = plugins_dir.join(plugin_dir_name(&plugin_id));
            if canonical != repo_path {
                if canonical.exists() {
                    self.move_plugin_dir_to_backup(&repo_path)?;
                    target = canonical;
                } else {
                    fs::rename(&repo_path, &canonical).with_context(|| {
                        format!(
                            "无法将旧插件目录 {} 重命名为 {}",
                            repo_path.display(),
                            canonical.display()
                        )
                    })?;
                    target = canonical;
                }
            }
        }

        let target_existed_before = target.exists();
        self.clone_or_update_plugin(root, url, &target)?;
        let plugin_id = self.plugin_id_from_dir(&target)?;
        let canonical = plugins_dir.join(plugin_dir_name(&plugin_id));
        if canonical == target {
            return Ok(canonical);
        }

        let legacy_canonical = plugins_dir.join(&plugin_id);
        if legacy_canonical != canonical && legacy_canonical != target && legacy_canonical.exists()
        {
            if target_existed_before {
                self.move_plugin_dir_to_backup(&target)?;
            } else {
                fs::remove_dir_all(&target)
                    .with_context(|| format!("无法清理临时插件目录 {}", target.display()))?;
            }
            self.clone_or_update_plugin(root, url, &legacy_canonical)?;
            fs::rename(&legacy_canonical, &canonical).with_context(|| {
                format!(
                    "无法将旧插件目录 {} 重命名为 {}",
                    legacy_canonical.display(),
                    canonical.display()
                )
            })?;
            return Ok(canonical);
        }

        if canonical.exists() {
            if target_existed_before {
                self.move_plugin_dir_to_backup(&target)?;
            } else {
                fs::remove_dir_all(&target)
                    .with_context(|| format!("无法清理临时插件目录 {}", target.display()))?;
            }
            self.clone_or_update_plugin(root, url, &canonical)?;
            return Ok(canonical);
        }

        fs::rename(&target, &canonical).with_context(|| {
            format!(
                "无法将插件目录 {} 重命名为 {}",
                target.display(),
                canonical.display()
            )
        })?;
        Ok(canonical)
    }

    pub(crate) fn sync_plugin_repo_with_manifest_dir(
        &self,
        url: &str,
        plugins_dir: &Path,
        repo_name: &str,
        branch: Option<&str>,
        _mode: InstallMode,
    ) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        self.sync_plugin_repo_with_manifest_dir_at_root(
            &root,
            url,
            plugins_dir,
            repo_name,
            branch,
            _mode,
        )
    }

    pub(crate) fn install_plugin_from_input(&self, input: &str) -> Result<()> {
        let (_, _, plugins_dir, _) = self.plugin_context()?;
        let url = convert_github_url(input, "https://github.com");
        let repo_name = url
            .rsplit('/')
            .next()
            .unwrap_or("plugin")
            .trim_end_matches(".git");
        let plugin_path = self.sync_plugin_repo_with_manifest_dir(
            &url,
            &plugins_dir,
            repo_name,
            None,
            InstallMode::Normal,
        )?;
        println!("插件已同步: {}", plugin_path.display());
        Ok(())
    }

    pub(crate) fn update_plugin(&self, name: &str) -> Result<()> {
        let (root, _, plugins_dir, _) = self.plugin_context()?;
        let path = self
            .require_plugin_dir_by_id(&plugins_dir, name)
            .or_else(|_| {
                let path = plugins_dir.join(name);
                if path.exists() {
                    Ok(path)
                } else {
                    bail!("插件目录不存在: {name}")
                }
            })?;
        if !path.join(".git").exists() {
            bail!("插件不是 Git 仓库，无法自动更新: {}", path.display());
        }
        self.clone_or_update_plugin(&root, "", &path)?;
        println!("插件已更新: {}", path.display());
        Ok(())
    }

    pub(crate) fn remove_plugin(&self, name: &str) -> Result<()> {
        let (_, _, plugins_dir, _) = self.plugin_context()?;
        let path = plugins_dir.join(name);
        if !path.exists() {
            bail!("插件目录不存在: {name}");
        }
        fs::remove_dir_all(path)?;
        Ok(())
    }

    pub(crate) fn print_plugins(&self) -> Result<()> {
        let (_, _, plugins_dir, _) = self.plugin_context()?;
        let plugins = list_plugins(&plugins_dir)?;
        if plugins.is_empty() {
            println!("暂无已安装插件");
        } else {
            for plugin in plugins {
                println!("{plugin}");
            }
        }
        Ok(())
    }

    pub(crate) fn manage_plugins_menu(&self) -> Result<()> {
        let (_, _, plugins_dir, _) = self.plugin_context()?;
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("插件管理", "安装、更新和卸载插件");
            self.print_kv("插件目录", &plugins_dir.display().to_string());
            let plugins = list_plugins(&plugins_dir)?;
            let plugin_display = if plugins.is_empty() {
                "暂无".to_string()
            } else {
                plugins.join(", ")
            };
            self.print_kv("插件数量", &plugins.len().to_string());
            self.print_kv("已安装插件", &plugin_display);
            let actions = [
                ActionItem::primary("安装插件", "从 GitHub 仓库安装或同步插件"),
                ActionItem::normal("更新插件", "拉取选定插件仓库的最新提交"),
                ActionItem::destructive("卸载插件", "删除选定插件目录"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择插件操作", &actions)?;
            let result = match choice {
                0 => {
                    let input: String = Input::with_theme(&self.theme)
                        .with_prompt("输入 GitHub 插件地址或 username/repo")
                        .interact_text()?;
                    self.install_plugin_from_input(&input)
                }
                1 => {
                    if plugins.is_empty() {
                        self.pause("当前没有已安装插件，按回车继续")?;
                        continue;
                    }
                    let idx = Select::with_theme(&self.theme)
                        .with_prompt("选择要更新的插件")
                        .items(&plugins)
                        .default(0)
                        .interact()?;
                    self.update_plugin(&plugins[idx])
                }
                2 => {
                    if plugins.is_empty() {
                        self.pause("当前没有已安装插件，按回车继续")?;
                        continue;
                    }
                    let idx = Select::with_theme(&self.theme)
                        .with_prompt("选择要卸载的插件")
                        .items(&plugins)
                        .default(0)
                        .interact()?;
                    self.remove_plugin(&plugins[idx])
                }
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
        }
        Ok(())
    }
}

pub(crate) fn windows_plugin_update_status(root: PathBuf, dir: PathBuf) -> String {
    plugin_update_status(&root, &dir)
}

fn plugin_update_status(root: &Path, dir: &Path) -> String {
    if !dir.join(".git").exists() {
        return "非 Git 仓库".to_string();
    }
    let local = git_output(root, dir, ["rev-parse", "HEAD"]);
    let upstream = git_output(
        root,
        dir,
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    if let Some(upstream) = upstream {
        let _ = run_git(root, dir, ["fetch", "--quiet"]);
        if let Some(remote) = git_output(root, dir, ["rev-parse", upstream.trim()]) {
            if local.as_deref() == Some(remote.trim()) {
                "已最新".to_string()
            } else {
                "有更新".to_string()
            }
        } else {
            "更新状态未知".to_string()
        }
    } else {
        "更新状态未知".to_string()
    }
}

fn git_output<const N: usize>(root: &Path, dir: &Path, args: [&str; N]) -> Option<String> {
    run_git(root, dir, args)
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|value| !value.is_empty())
}

fn run_git<const N: usize>(
    root: &Path,
    dir: &Path,
    args: [&str; N],
) -> std::io::Result<std::process::Output> {
    let mut command = Command::new("git");
    apply_windows_tools_env(&mut command, root);
    command
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command_output_with_timeout(&mut command, Duration::from_secs(6))
}

fn command_output_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<std::process::Output> {
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            return child.wait_with_output();
        }
        thread::sleep(Duration::from_millis(20));
    }
}
