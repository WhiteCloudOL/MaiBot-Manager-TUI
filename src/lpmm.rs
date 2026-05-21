use crate::{app::App, utils::*};
use anyhow::Result;
use dialoguer::Select;
use std::{fs, path::{Path, PathBuf}};

impl App {
    pub(crate) fn manage_lpmm_menu(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let maibot_dir = root.join("MaiBot");
        let data_dir = maibot_dir.join("data");
        let raw_dir = data_dir.join("lpmm_raw_data");
        let openie_dir = data_dir.join("openie");
        let venv_activate = root.join("venv/bin/activate");
        let info_script = maibot_dir.join("scripts/info_extraction.py");
        let import_script = maibot_dir.join("scripts/import_openie.py");
        let py_env = cfg.mai_python_env;
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("LPMM 知识库", "初始化目录、执行提取、导入知识库");
            self.print_kv("数据目录", &data_dir.display().to_string());
            let choice = Select::with_theme(&self.theme)
                .with_prompt("LPMM 知识库")
                .items([
                    "初始化目录结构",
                    "文本分割与实体提取（前台）",
                    "文本分割与实体提取（后台 screen）",
                    "关闭后台文本分割与实体提取",
                    "导入知识库（前台）",
                    "导入知识库（后台 screen）",
                    "关闭后台知识库导入",
                    "返回",
                ])
                .default(0)
                .interact()?;
            match choice {
                0 => {
                    fs::create_dir_all(&raw_dir)?;
                    fs::create_dir_all(&openie_dir)?;
                }
                1 => self.run_python_script(&maibot_dir, &venv_activate, &py_env, &info_script)?,
                2 => self.run_python_script_screen(
                    &maibot_dir,
                    &venv_activate,
                    &py_env,
                    &info_script,
                    "mai-lpmm-info",
                )?,
                3 => self.run_shell(&screen_quit_cmd("mai-lpmm-info"))?,
                4 => self.run_python_script(&maibot_dir, &venv_activate, &py_env, &import_script)?,
                5 => self.run_python_script_screen(
                    &maibot_dir,
                    &venv_activate,
                    &py_env,
                    &import_script,
                    "mai-lpmm-import",
                )?,
                6 => self.run_shell(&screen_quit_cmd("mai-lpmm-import"))?,
                _ => break,
            }
            self.pause("操作已执行，按回车继续")?;
        }
        Ok(())
    }

    pub(crate) fn run_python_script(
        &self,
        maibot_dir: &Path,
        venv_activate: &Path,
        py_env: &str,
        script: &Path,
    ) -> Result<()> {
        if py_env == "uv" {
            self.run_shell(&format!(
                "cd '{}' && uv run '{}'",
                shell_escape(maibot_dir),
                script
                    .strip_prefix(maibot_dir)
                    .unwrap_or(script)
                    .display()
            ))
        } else {
            self.run_shell(&format!(
                ". '{}' && python '{}'",
                shell_escape(venv_activate),
                shell_escape(script)
            ))
        }
    }

    pub(crate) fn run_python_script_screen(
        &self,
        maibot_dir: &Path,
        venv_activate: &Path,
        py_env: &str,
        script: &Path,
        screen_name: &str,
    ) -> Result<()> {
        let body = if py_env == "uv" {
            format!(
                "cd '{}' && uv run '{}'",
                shell_escape(maibot_dir),
                shell_escape(script.strip_prefix(maibot_dir).unwrap_or(script))
            )
        } else {
            format!(
                ". '{}' && python '{}'",
                shell_escape(venv_activate),
                shell_escape(script)
            )
        };
        self.run_shell(&screen_launch_cmd(screen_name, &body))
    }
}
