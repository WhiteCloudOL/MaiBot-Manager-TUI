use crate::{app::App, utils::*};
use anyhow::Result;
use dialoguer::{Confirm, Input, Select};
use std::{fs, path::PathBuf};

impl App {
    pub(crate) fn manage_bot_protocol_menu(&self) -> Result<()> {
        self.require_config()?;
        loop {
            self.clear();
            self.print_header(None);
            let choice = Select::with_theme(&self.theme)
                .with_prompt("Bot 协议端服务")
                .items(["管理 NapCatQQ", "管理 LuckyLilliaBot", "返回"])
                .default(0)
                .interact()?;
            match choice {
                0 => self.manage_napcat_menu()?,
                1 => self.manage_llbot_menu()?,
                _ => break,
            }
        }
        Ok(())
    }

    pub(crate) fn manage_maibot_menu(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let maibot_dir = root.join("MaiBot");
        let venv_activate = root.join("venv/bin/activate");
        let py_env = cfg.mai_python_env;
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("MaiBot 核心", "管理主程序启动、停止与控制台进入");
            self.print_kv("目录", &maibot_dir.display().to_string());
            self.print_status_line("运行状态", screen_exists("maibot")?);
            let choice = Select::with_theme(&self.theme)
                .with_prompt("MaiBot 核心管理")
                .items(["启动 MaiBot", "停止 MaiBot", "进入 Screen 控制台", "返回"])
                .default(0)
                .interact()?;
            match choice {
                0 => {
                    let run_mode = Select::with_theme(&self.theme)
                        .with_prompt("启动方式")
                        .items(["正常后台启动", "启动并进入控制台（首次运行建议）"])
                        .default(0)
                        .interact()?;
                    let cmd = if py_env == "uv" {
                        format!(
                            "screen -S maibot -X quit >/dev/null 2>&1 || true; cd '{}' && screen -dmS maibot bash -lc \"cd '{}' && uv run bot.py; exec bash\"",
                            shell_escape(&maibot_dir),
                            shell_escape(&maibot_dir)
                        )
                    } else {
                        format!(
                            "screen -S maibot -X quit >/dev/null 2>&1 || true; cd '{}' && screen -dmS maibot bash -lc \". '{}' && python3 bot.py; exec bash\"",
                            shell_escape(&maibot_dir),
                            shell_escape(&venv_activate)
                        )
                    };
                    self.run_shell(&cmd)?;
                    if run_mode == 1 {
                        self.run_shell("screen -r maibot")?;
                    }
                }
                1 => self.run_shell("screen -S maibot -X quit")?,
                2 => self.run_shell("screen -r maibot")?,
                _ => break,
            }
            self.pause("操作已执行，按回车继续")?;
        }
        Ok(())
    }

    pub(crate) fn manage_napcat_menu(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let napcat_dir = PathBuf::from(cfg.mai_path).join("NapCat");
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("NapCatQQ", "Docker 容器管理与日志查看");
            self.print_kv("目录", &napcat_dir.display().to_string());
            let choice = Select::with_theme(&self.theme)
                .with_prompt("NapCat 管理")
                .items([
                    "启动 NapCat",
                    "停止 NapCat",
                    "重启 NapCat",
                    "查看实时日志",
                    "重建容器",
                    "移除现有 napcat 容器",
                    "删除 NapCat 目录",
                    "返回",
                ])
                .default(0)
                .interact()?;
            match choice {
                0 => self.run_shell(&format!(
                    "cd '{}' && docker compose up -d",
                    shell_escape(&napcat_dir)
                ))?,
                1 => self.run_shell(&format!(
                    "cd '{}' && docker compose stop",
                    shell_escape(&napcat_dir)
                ))?,
                2 => self.run_shell(&format!(
                    "cd '{}' && docker compose restart",
                    shell_escape(&napcat_dir)
                ))?,
                3 => self.run_shell(&format!(
                    "cd '{}' && docker compose logs -f --tail=100",
                    shell_escape(&napcat_dir)
                ))?,
                4 => self.run_shell(&format!(
                    "cd '{}' && docker compose down && docker compose pull && docker compose up -d",
                    shell_escape(&napcat_dir)
                ))?,
                5 => self.run_shell(
                    "docker ps -a --format '{{.Names}}' | grep '^napcat' | xargs -r docker rm -f",
                )?,
                6 => {
                    if Confirm::with_theme(&self.theme)
                        .with_prompt("确认删除 NapCat 目录及其数据？")
                        .default(false)
                        .interact()?
                    {
                        let _ = self.run_shell(&format!(
                            "cd '{}' && docker compose down",
                            shell_escape(&napcat_dir)
                        ));
                        fs::remove_dir_all(&napcat_dir).ok();
                    }
                }
                _ => break,
            }
            self.pause("操作已执行，按回车继续")?;
        }
        Ok(())
    }

    pub(crate) fn manage_llbot_menu(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let llbot_dir = if cfg.mai_llbot_path.is_empty() {
            PathBuf::from(cfg.mai_path).join("LLBot")
        } else {
            PathBuf::from(cfg.mai_llbot_path)
        };
        let token_file = llbot_dir.join("bin/llbot/data/webui_token.txt");
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("LuckyLilliaBot", "管理 CLI 协议端、密码和控制台");
            self.print_kv("目录", &llbot_dir.display().to_string());
            self.print_status_line("运行状态", screen_exists("llbot")?);
            let choice = Select::with_theme(&self.theme)
                .with_prompt("LuckyLilliaBot 管理")
                .items([
                    "启动 LuckyLilliaBot",
                    "停止 LuckyLilliaBot",
                    "重启 LuckyLilliaBot",
                    "进入 Screen 控制台",
                    "修改 WebUI 密码",
                    "删除 LuckyLilliaBot 目录",
                    "返回",
                ])
                .default(0)
                .interact()?;
            match choice {
                0 => self.run_shell(&format!(
                    "screen -S llbot -X quit >/dev/null 2>&1 || true; cd '{}' && screen -dmS llbot bash -lc 'chmod +x ./start.sh ./llbot 2>/dev/null || true; ./start.sh; exec bash'",
                    shell_escape(&llbot_dir)
                ))?,
                1 => self.run_shell("screen -S llbot -X quit")?,
                2 => {
                    self.run_shell(&format!(
                        "screen -S llbot -X quit >/dev/null 2>&1 || true; cd '{}' && screen -dmS llbot bash -lc 'chmod +x ./start.sh ./llbot 2>/dev/null || true; ./start.sh; exec bash'",
                        shell_escape(&llbot_dir)
                    ))?;
                }
                3 => self.run_shell("screen -r llbot")?,
                4 => {
                    let password: String = Input::with_theme(&self.theme)
                        .with_prompt("新的 WebUI 密码")
                        .interact_text()?;
                    if let Some(parent) = token_file.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&token_file, format!("{password}\n"))?;
                }
                5 => {
                    if Confirm::with_theme(&self.theme)
                        .with_prompt("确认删除 LuckyLilliaBot 目录及其数据？")
                        .default(false)
                        .interact()?
                    {
                        let _ = self.run_shell("screen -S llbot -X quit");
                        fs::remove_dir_all(&llbot_dir).ok();
                    }
                }
                _ => break,
            }
            self.pause("操作已执行，按回车继续")?;
        }
        Ok(())
    }
}
