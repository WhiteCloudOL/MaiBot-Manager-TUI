use crate::{app::App, model::InstallMode, utils::*};
use anyhow::Result;
use dialoguer::{Input, Select};
use std::{
    fs,
    path::{Path, PathBuf},
};

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

    pub(crate) fn install_plugin_from_input(&self, input: &str) -> Result<()> {
        let (maibot_dir, plugins_dir, venv_activate, py_env) = self.plugin_context()?;
        let url = convert_github_url(input, "https://github.com");
        let plugin_name = url
            .rsplit('/')
            .next()
            .unwrap_or("plugin")
            .trim_end_matches(".git");
        let plugin_path = plugins_dir.join(plugin_name);
        self.clone_or_update_repo(&url, &plugin_path, None, InstallMode::Normal)?;
        let req = plugin_path.join("requirements.txt");
        if req.exists() {
            self.install_requirements(&maibot_dir, &venv_activate, &py_env, &req)?;
        }
        Ok(())
    }

    pub(crate) fn remove_plugin(&self, name: &str) -> Result<()> {
        let (_, plugins_dir, _, _) = self.plugin_context()?;
        fs::remove_dir_all(plugins_dir.join(name))?;
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
            match choice {
                0 => {
                    let input: String = Input::with_theme(&self.theme)
                        .with_prompt("输入 GitHub 插件地址或 username/repo")
                        .interact_text()?;
                    self.install_plugin_from_input(&input)?;
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
                    self.remove_plugin(&plugins[idx])?;
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
                    self.install_plugin_dependencies(&plugins[idx])?;
                }
                _ => break,
            }
            self.pause("操作已执行，按回车继续")?;
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
