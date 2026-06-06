use crate::{app::App, utils::*};
use anyhow::{Result, bail};
use dialoguer::console::style;
use dialoguer::{Confirm, Input, Select};
use std::{fs, path::PathBuf};

impl App {
    /// 进入 screen 控制台前的醒目退出提示。
    /// 很多新手不知道 screen 的分离热键，看到 MaiBot 日志后下意识按 Ctrl+C，
    /// 会直接把进程杀掉。这里强制等待用户回车，确认看到提示再继续。
    pub(crate) fn warn_before_screen_attach(&self, session: &str) -> Result<()> {
        let bar = "═".repeat(60);
        println!();
        println!("{}", style(&bar).yellow().bold());
        println!(
            "  {} {}",
            style("⚠").yellow().bold(),
            style(format!("即将进入 screen 会话：{session}"))
                .yellow()
                .bold()
        );
        println!("{}", style(&bar).yellow().bold());
        println!(
            "  退出请按 {}（分离会话，{}）",
            style("Ctrl + A  然后按 D").green().bold(),
            style("进程会继续在后台运行").green()
        );
        println!(
            "  {}",
            style("⚠  千万不要按 Ctrl + C — 那会直接终止当前进程！")
                .red()
                .bold()
        );
        println!("{}", style(&bar).yellow().bold());
        if self.cli_mode {
            return Ok(());
        }
        self.pause("按回车进入控制台...")?;
        Ok(())
    }

    fn maibot_paths(&self) -> Result<(PathBuf, PathBuf, String)> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        Ok((
            root.join("MaiBot"),
            root.join("venv/bin/activate"),
            cfg.mai_python_env,
        ))
    }

    pub(crate) fn start_maibot_core(&self, attach: bool) -> Result<()> {
        let (maibot_dir, venv_activate, py_env) = self.maibot_paths()?;
        let body = if py_env == "uv" {
            format!("cd '{}' && uv run bot.py", shell_escape(&maibot_dir))
        } else {
            format!(
                "cd '{}' && . '{}' && python3 bot.py",
                shell_escape(&maibot_dir),
                shell_escape(&venv_activate)
            )
        };
        self.run_shell(&screen_launch_cmd("maibot", &body))?;
        if attach {
            self.attach_screen("maibot")?;
        }
        Ok(())
    }

    pub(crate) fn stop_maibot_core(&self) -> Result<()> {
        self.run_shell(&screen_quit_cmd("maibot"))
    }

    pub(crate) fn restart_maibot_core(&self) -> Result<()> {
        self.start_maibot_core(false)
    }

    pub(crate) fn attach_screen(&self, session: &str) -> Result<()> {
        self.warn_before_screen_attach(session)?;
        self.run_shell(&format!("screen -r {}", shell_escape_raw(session)))
    }

    pub(crate) fn print_screen_logs(&self, session: &str, tail: usize, follow: bool) -> Result<()> {
        if !screen_exists(session)? {
            bail!("screen 会话未运行: {session}");
        }
        let out = format!("/tmp/maibot-manager-{session}.log");
        let escaped_out = shell_escape_raw(&out);
        let escaped_session = shell_escape_raw(session);
        let cmd = if follow {
            format!(
                "while true; do screen -S '{escaped_session}' -X hardcopy -h '{escaped_out}' >/dev/null 2>&1 || exit 1; clear; tail -n {tail} '{escaped_out}' 2>/dev/null || true; sleep 2; done"
            )
        } else {
            format!(
                "screen -S '{escaped_session}' -X hardcopy -h '{escaped_out}' && tail -n {tail} '{escaped_out}'"
            )
        };
        self.run_shell(&cmd)
    }

    fn napcat_dir(&self) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        Ok(PathBuf::from(cfg.mai_path).join("NapCat"))
    }

    pub(crate) fn start_napcat(&self) -> Result<()> {
        let napcat_dir = self.napcat_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose up -d",
            shell_escape(&napcat_dir)
        ))
    }

    pub(crate) fn stop_napcat(&self) -> Result<()> {
        let napcat_dir = self.napcat_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose stop",
            shell_escape(&napcat_dir)
        ))
    }

    pub(crate) fn restart_napcat(&self) -> Result<()> {
        let napcat_dir = self.napcat_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose restart",
            shell_escape(&napcat_dir)
        ))
    }

    pub(crate) fn rebuild_napcat(&self) -> Result<()> {
        let napcat_dir = self.napcat_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose down && docker compose pull && docker compose up -d",
            shell_escape(&napcat_dir)
        ))
    }

    pub(crate) fn remove_napcat_container(&self) -> Result<()> {
        self.run_shell(
            "docker ps -a --format '{{.Names}}' | grep '^napcat' | xargs -r docker rm -f",
        )
    }

    pub(crate) fn print_napcat_logs(&self, tail: usize, follow: bool) -> Result<()> {
        let napcat_dir = self.napcat_dir()?;
        let follow_flag = if follow { "-f " } else { "" };
        self.run_shell(&format!(
            "cd '{}' && docker compose logs {follow_flag}--tail={tail}",
            shell_escape(&napcat_dir)
        ))
    }

    fn llbot_dir(&self) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        if cfg.mai_llbot_path.is_empty() {
            Ok(PathBuf::from(cfg.mai_path).join("LLBot"))
        } else {
            Ok(PathBuf::from(cfg.mai_llbot_path))
        }
    }

    pub(crate) fn start_llbot(&self) -> Result<()> {
        let llbot_dir = self.llbot_dir()?;
        let body = format!(
            "cd '{}' && chmod +x ./start.sh ./llbot 2>/dev/null || true; ./start.sh",
            shell_escape(&llbot_dir)
        );
        self.run_shell(&screen_launch_cmd("llbot", &body))
    }

    pub(crate) fn stop_llbot(&self) -> Result<()> {
        self.run_shell(&screen_quit_cmd("llbot"))
    }

    pub(crate) fn restart_llbot(&self) -> Result<()> {
        self.start_llbot()
    }

    pub(crate) fn set_llbot_password(&self, password: &str) -> Result<()> {
        let token_file = self.llbot_dir()?.join("bin/llbot/data/webui_token.txt");
        if let Some(parent) = token_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&token_file, format!("{password}\n"))?;
        Ok(())
    }

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
            let result = match choice {
                0 => self.manage_napcat_menu(),
                1 => self.manage_llbot_menu(),
                _ => break,
            };
            self.handle_menu_result(result)?;
        }
        Ok(())
    }

    pub(crate) fn manage_maibot_menu(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let maibot_dir = root.join("MaiBot");
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("MaiBot 核心", "管理主程序启动、停止与控制台进入");
            self.print_kv("目录", &maibot_dir.display().to_string());
            let running = screen_exists("maibot")?;
            self.print_status_dot(
                "运行状态",
                if running { "运行中" } else { "未运行" },
                running,
            );
            let choice = Select::with_theme(&self.theme)
                .with_prompt("MaiBot 核心管理")
                .items(["启动 MaiBot", "停止 MaiBot", "进入 Screen 控制台", "返回"])
                .default(0)
                .interact()?;
            let result = match choice {
                0 => {
                    let run_mode = Select::with_theme(&self.theme)
                        .with_prompt("启动方式")
                        .items(["正常后台启动", "启动并进入控制台（首次运行建议）"])
                        .default(0)
                        .interact()?;
                    self.start_maibot_core(run_mode == 1)
                }
                1 => self.stop_maibot_core(),
                2 => self.attach_screen("maibot"),
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
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
            let result = match choice {
                0 => self.start_napcat(),
                1 => self.stop_napcat(),
                2 => self.restart_napcat(),
                3 => self.print_napcat_logs(100, true),
                4 => self.rebuild_napcat(),
                5 => self.remove_napcat_container(),
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
                    Ok(())
                }
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
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
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("LuckyLilliaBot", "管理 CLI 协议端、密码和控制台");
            self.print_kv("目录", &llbot_dir.display().to_string());
            let running = screen_exists("llbot")?;
            self.print_status_dot(
                "运行状态",
                if running { "运行中" } else { "未运行" },
                running,
            );
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
            let result = match choice {
                0 => self.start_llbot(),
                1 => self.stop_llbot(),
                2 => self.restart_llbot(),
                3 => self.attach_screen("llbot"),
                4 => {
                    let password: String = Input::with_theme(&self.theme)
                        .with_prompt("新的 WebUI 密码")
                        .interact_text()?;
                    self.set_llbot_password(&password)
                }
                5 => {
                    if Confirm::with_theme(&self.theme)
                        .with_prompt("确认删除 LuckyLilliaBot 目录及其数据？")
                        .default(false)
                        .interact()?
                    {
                        let _ = self.stop_llbot();
                        fs::remove_dir_all(&llbot_dir).ok();
                    }
                    Ok(())
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
