use crate::{
    app::App,
    model::{GitDirtyMode, InstallMode},
    utils::*,
};
use anyhow::{Context, Result, bail};
use dialoguer::{Input, Select};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) const NAPCAT_ADAPTER_REPO_NAME: &str = "MaiBot-Napcat-Adapter";
pub(crate) const NAPCAT_ADAPTER_PLUGIN_ID: &str = "maibot-team.napcat-adapter";

impl App {
    fn plugin_context(&self) -> Result<(PathBuf, PathBuf, PathBuf, String)> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let maibot_dir = root.join("MaiBot");
        let plugins_dir = maibot_dir.join("plugins");
        let venv_activate = root.join("venv/bin/activate");
        fs::create_dir_all(&plugins_dir)?;
        Ok((maibot_dir, plugins_dir, venv_activate, cfg.mai_python_env))
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

    fn plugin_backup_path(&self, path: &Path) -> PathBuf {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin");
        for idx in 1.. {
            let candidate = parent.join(format!("{stem}.legacy-backup-{idx}"));
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

    pub(crate) fn resolve_plugin_dir_by_id(
        &self,
        plugins_dir: &Path,
        plugin_id: &str,
    ) -> Result<Option<PathBuf>> {
        let preferred = plugins_dir.join(plugin_id);
        if preferred.exists() {
            return Ok(Some(preferred));
        }
        if !plugins_dir.exists() {
            return Ok(None);
        }
        for entry in fs::read_dir(plugins_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match self.plugin_id_from_dir(&path) {
                Ok(id) if id == plugin_id => return Ok(Some(path)),
                Ok(_) => {}
                Err(_) => {}
            }
        }
        Ok(None)
    }

    pub(crate) fn sync_plugin_repo_with_manifest_dir(
        &self,
        url: &str,
        plugins_dir: &Path,
        repo_name: &str,
        branch: Option<&str>,
        mode: InstallMode,
    ) -> Result<PathBuf> {
        fs::create_dir_all(plugins_dir)?;
        let repo_path = plugins_dir.join(repo_name);
        let mut target = repo_path.clone();

        if repo_path.exists() {
            let plugin_id = self.plugin_id_from_dir(&repo_path)?;
            let canonical = plugins_dir.join(&plugin_id);
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
        self.clone_or_update_repo(url, &target, branch, mode, false, GitDirtyMode::Ask)?;

        let plugin_id = self.plugin_id_from_dir(&target)?;
        let canonical = plugins_dir.join(&plugin_id);
        if canonical == target {
            return Ok(canonical);
        }

        if canonical.exists() {
            if target_existed_before {
                self.move_plugin_dir_to_backup(&target)?;
            } else {
                fs::remove_dir_all(&target)
                    .with_context(|| format!("无法清理临时插件目录 {}", target.display()))?;
            }
            self.clone_or_update_repo(url, &canonical, branch, mode, false, GitDirtyMode::Ask)?;
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

    pub(crate) fn require_plugin_dir_by_id(
        &self,
        plugins_dir: &Path,
        plugin_id: &str,
    ) -> Result<PathBuf> {
        self.resolve_plugin_dir_by_id(plugins_dir, plugin_id)?
            .ok_or_else(|| anyhow::anyhow!("未找到插件目录: {plugin_id}"))
    }

    pub(crate) fn install_plugin_from_input(&self, input: &str) -> Result<()> {
        let (maibot_dir, plugins_dir, venv_activate, py_env) = self.plugin_context()?;
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
        let req = plugin_path.join("requirements.txt");
        if req.exists() {
            self.install_requirements(&maibot_dir, &venv_activate, &py_env, &req)?;
        }
        Ok(())
    }

    pub(crate) fn remove_plugin(&self, name: &str) -> Result<()> {
        let (_, plugins_dir, _, _) = self.plugin_context()?;
        let path = plugins_dir.join(name);
        if !path.exists() {
            bail!("插件目录不存在: {name}");
        }
        fs::remove_dir_all(path)?;
        Ok(())
    }

    pub(crate) fn install_plugin_dependencies(&self, name: &str) -> Result<()> {
        let (maibot_dir, plugins_dir, venv_activate, py_env) = self.plugin_context()?;
        let req = plugins_dir.join(name).join("requirements.txt");
        if req.exists() {
            self.install_requirements(&maibot_dir, &venv_activate, &py_env, &req)?;
        } else {
            println!("插件没有 requirements.txt: {name}");
        }
        Ok(())
    }

    pub(crate) fn print_plugins(&self) -> Result<()> {
        let (_, plugins_dir, _, _) = self.plugin_context()?;
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
        let (_, plugins_dir, _, _) = self.plugin_context()?;
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("插件管理", "安装、卸载和补装 Python 依赖");
            self.print_kv("插件目录", &plugins_dir.display().to_string());
            let plugins = list_plugins(&plugins_dir)?;
            if plugins.is_empty() {
                self.print_kv("已安装插件", "暂无");
            } else {
                self.print_kv("已安装插件", &plugins.join(", "));
            }
            let choice = Select::with_theme(&self.theme)
                .with_prompt("插件管理")
                .items(["安装插件", "卸载插件", "安装插件依赖", "返回"])
                .default(0)
                .interact()?;
            let result = match choice {
                0 => {
                    let input: String = Input::with_theme(&self.theme)
                        .with_prompt("输入 GitHub 插件地址或 username/repo")
                        .interact_text()?;
                    self.install_plugin_from_input(&input)
                }
                1 => {
                    let plugins = list_plugins(&plugins_dir)?;
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
                2 => {
                    let plugins = list_plugins(&plugins_dir)?;
                    if plugins.is_empty() {
                        self.pause("当前没有已安装插件，按回车继续")?;
                        continue;
                    }
                    let idx = Select::with_theme(&self.theme)
                        .with_prompt("选择要安装依赖的插件")
                        .items(&plugins)
                        .default(0)
                        .interact()?;
                    self.install_plugin_dependencies(&plugins[idx])
                }
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
        }
        Ok(())
    }

    pub(crate) fn install_requirements(
        &self,
        maibot_dir: &Path,
        venv_activate: &Path,
        py_env: &str,
        req: &Path,
    ) -> Result<()> {
        if py_env == "uv" {
            self.run_shell(&format!(
                "cd '{}' && uv pip install -r '{}'",
                shell_escape(maibot_dir),
                shell_escape(req)
            ))
        } else {
            self.run_shell(&format!(
                ". '{}' && pip install -r '{}'",
                shell_escape(venv_activate),
                shell_escape(req)
            ))
        }
    }
}
