use crate::{
    app::App,
    model::InstallMode,
    ui::ActionItem,
    utils::{bat_arg, bat_quote, convert_github_url, list_plugins, with_windows_tools_path},
};
use anyhow::{Context, Result, bail};
use dialoguer::{Input, Select};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct PluginSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) version: String,
    pub(crate) description: String,
    pub(crate) has_requirements: bool,
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
            has_requirements: dir.join("requirements.txt").exists(),
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
        let preferred = plugins_dir.join(plugin_id);
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

    pub(crate) fn sync_plugin_repo_with_manifest_dir(
        &self,
        url: &str,
        plugins_dir: &Path,
        repo_name: &str,
        _branch: Option<&str>,
        _mode: InstallMode,
    ) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        fs::create_dir_all(plugins_dir)?;
        let repo_path = plugins_dir.join(repo_name);
        self.clone_or_update_plugin(&root, url, &repo_path)?;
        let plugin_id = self.plugin_id_from_dir(&repo_path)?;
        let canonical = plugins_dir.join(plugin_id);
        if canonical != repo_path {
            if canonical.exists() {
                fs::remove_dir_all(&repo_path)?;
            } else {
                fs::rename(&repo_path, &canonical)?;
            }
        }
        Ok(canonical)
    }

    pub(crate) fn install_plugin_from_input(&self, input: &str) -> Result<()> {
        let (root, maibot_dir, plugins_dir, py_env) = self.plugin_context()?;
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
            self.install_requirements(&root, &maibot_dir, &py_env, &req)?;
        }
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

    pub(crate) fn install_plugin_dependencies(&self, name: &str) -> Result<()> {
        let (root, maibot_dir, plugins_dir, py_env) = self.plugin_context()?;
        let req = plugins_dir.join(name).join("requirements.txt");
        if req.exists() {
            self.install_requirements(&root, &maibot_dir, &py_env, &req)?;
        } else {
            println!("插件没有 requirements.txt: {name}");
        }
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
            self.print_section("插件管理", "安装、卸载和补装 Python 依赖");
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
                ActionItem::primary("安装插件", "从 GitHub 仓库安装或更新插件"),
                ActionItem::destructive("卸载插件", "删除选定插件目录"),
                ActionItem::normal("修复依赖", "为选定插件安装 requirements"),
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
                        .with_prompt("选择要卸载的插件")
                        .items(&plugins)
                        .default(0)
                        .interact()?;
                    self.remove_plugin(&plugins[idx])
                }
                2 => {
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

    fn install_requirements(
        &self,
        root: &Path,
        maibot_dir: &Path,
        py_env: &str,
        req: &Path,
    ) -> Result<()> {
        if py_env == "uv" {
            self.run_shell(&with_windows_tools_path(
                root,
                &format!(
                    "cd /d {}\r\nuv pip install -r {}",
                    bat_quote(maibot_dir),
                    bat_quote(req)
                ),
            ))
        } else {
            let python = root.join("venv").join("Scripts").join("python.exe");
            self.run_shell(&with_windows_tools_path(root, &format!(
                "cd /d {}\r\nif not exist {} (echo 未找到 Python 虚拟环境: {} & exit /b 1)\r\n{} -m pip install -r {}",
                bat_quote(maibot_dir),
                bat_quote(&python),
                python.display(),
                bat_quote(&python),
                bat_quote(req)
            )))
        }
    }
}
