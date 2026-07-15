use crate::{
    app::{App, napcat_running, snowluma_running},
    ui::{ActionItem, StatusCard},
    utils::*,
};
use anyhow::{Result, bail};
use dialoguer::console::style;
use dialoguer::{Confirm, Input};
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
            style("进入后底部状态栏会常驻显示快捷退出方式。").yellow()
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
        let hint = "MaiBot Manager | 快捷退出: Ctrl+A 然后 D | Ctrl+C 会停止当前服务进程";
        self.run_shell(&format!(
            "screen -S {} -X hardstatus alwayslastline '{}' 2>/dev/null || true; screen -r {}",
            shell_escape_raw(session),
            shell_escape_raw(hint),
            shell_escape_raw(session)
        ))
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

    pub(crate) fn print_maibot_core_status(&self) -> Result<()> {
        print_screen_status("maibot")
    }

    pub(crate) fn print_llbot_status(&self) -> Result<()> {
        print_screen_status("llbot")
    }

    pub(crate) fn print_maibot_core_logs(&self, tail: usize, follow: bool) -> Result<()> {
        self.print_screen_logs("maibot", tail, follow)
    }

    pub(crate) fn print_llbot_logs(&self, tail: usize, follow: bool) -> Result<()> {
        self.print_screen_logs("llbot", tail, follow)
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

    pub(crate) fn print_napcat_status(&self) -> Result<()> {
        let output = std::process::Command::new("bash")
            .arg("-lc")
            .arg("docker ps --filter name=^napcat$ --filter status=running --format '{{.Names}}' 2>/dev/null")
            .output()?;
        if String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            println!("napcat: stopped");
        } else {
            println!("napcat: running");
        }
        Ok(())
    }

    pub(crate) fn exec_napcat_shell(&self) -> Result<()> {
        self.run_shell("docker exec -it napcat /bin/sh")
    }

    fn snowluma_dir(&self) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        Ok(PathBuf::from(cfg.mai_path).join("SnowLuma"))
    }

    pub(crate) fn start_snowluma(&self) -> Result<()> {
        let dir = self.snowluma_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose up -d",
            shell_escape(&dir)
        ))
    }

    pub(crate) fn stop_snowluma(&self) -> Result<()> {
        let dir = self.snowluma_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose stop",
            shell_escape(&dir)
        ))
    }

    pub(crate) fn restart_snowluma(&self) -> Result<()> {
        let dir = self.snowluma_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose restart",
            shell_escape(&dir)
        ))
    }

    pub(crate) fn rebuild_snowluma(&self) -> Result<()> {
        let dir = self.snowluma_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose down && docker compose pull && docker compose up -d",
            shell_escape(&dir)
        ))
    }

    pub(crate) fn recreate_snowluma_data(&self) -> Result<()> {
        let dir = self.snowluma_dir()?;
        self.run_shell(&format!(
            "cd '{}' && docker compose down && rm -rf snowluma-data snowluma-qq-config snowluma-qq-data && docker compose up -d",
            shell_escape(&dir)
        ))
    }

    pub(crate) fn remove_snowluma_container(&self) -> Result<()> {
        self.run_shell(
            "docker ps -a --format '{{.Names}}' | grep '^snowluma$' | xargs -r docker rm -f",
        )
    }

    pub(crate) fn print_snowluma_logs(&self, tail: usize, follow: bool) -> Result<()> {
        let dir = self.snowluma_dir()?;
        let follow_flag = if follow { "-f " } else { "" };
        self.run_shell(&format!(
            "cd '{}' && docker compose logs {follow_flag}--tail={tail}",
            shell_escape(&dir)
        ))
    }

    pub(crate) fn print_snowluma_status(&self) -> Result<()> {
        println!(
            "snowluma: {}",
            if snowluma_running() {
                "running"
            } else {
                "stopped"
            }
        );
        Ok(())
    }

    pub(crate) fn exec_snowluma_shell(&self) -> Result<()> {
        self.run_shell("docker exec -it snowluma /bin/sh")
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

    pub(crate) fn manage_llbot_password(&self) -> Result<()> {
        self.with_prompt_mode(|| {
            let password: String = dialoguer::Input::with_theme(&self.theme)
                .with_prompt("新的 LLBot WebUI 密码")
                .interact_text()?;
            self.set_llbot_password(&password)
        })
    }

    pub(crate) fn manage_bot_protocol_menu(&self) -> Result<()> {
        self.require_config()?;
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("协议端服务", "选择要维护的 Bot 协议端");
            let actions = [
                ActionItem::primary("NapCatQQ", "Docker Compose 协议端"),
                ActionItem::normal("LuckyLilliaBot", "screen 托管的 CLI 协议端"),
                ActionItem::normal("SnowLuma", "Linux Docker Compose 协议端"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择协议端", &actions)?;
            let result = match choice {
                0 => self.manage_napcat_menu(),
                1 => self.manage_llbot_menu(),
                2 => self.manage_snowluma_menu(),
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
            let cards = [if running {
                StatusCard::running("MaiBot", "screen: maibot · 可分离控制台保持后台运行")
            } else {
                StatusCard::stopped("MaiBot", "screen 会话未启动")
            }];
            self.print_status_cards("核心状态", &cards);
            let actions = [
                ActionItem::primary("启动 MaiBot", "创建 screen: maibot 后台会话"),
                ActionItem::destructive("停止 MaiBot", "结束 screen 会话和核心进程"),
                ActionItem::normal("进入控制台", "screen -r maibot，使用 Ctrl+A D 分离"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择核心操作", &actions)?;
            let result = match choice {
                0 => {
                    let modes = [
                        ActionItem::primary("后台启动", "适合已完成 EULA，启动后立即返回管理器"),
                        ActionItem::normal(
                            "启动并进入终端",
                            "首次启动/EULA，在 screen 中输入确认；Ctrl+A D 分离",
                        ),
                    ];
                    let run_mode = self.select_action_timeout(
                        "选择启动方式",
                        &modes,
                        0,
                        std::time::Duration::from_secs(10),
                    )?;
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
            let running = napcat_running();
            let cards = [if running {
                StatusCard::running("NapCatQQ", "Docker 容器 napcat 正在运行")
            } else {
                StatusCard::stopped("NapCatQQ", "Docker 容器未运行")
            }];
            self.print_status_cards("服务状态", &cards);
            let actions = [
                ActionItem::primary("启动 NapCat", "docker compose up -d"),
                ActionItem::destructive("停止 NapCat", "docker compose down"),
                ActionItem::normal("重启 NapCat", "重新加载容器进程"),
                ActionItem::normal("查看实时日志", "跟随 docker compose logs"),
                ActionItem::normal("重建容器", "down + pull + up -d"),
                ActionItem::destructive("移除容器", "删除现有 napcat 容器"),
                ActionItem::destructive("删除目录", "删除 NapCat 工作目录及数据"),
                ActionItem::back("返回", "回到协议端服务"),
            ];
            let choice = self.select_action("选择 NapCat 操作", &actions)?;
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
            let cards = [if running {
                StatusCard::running("LuckyLilliaBot", "screen: llbot · CLI 协议端运行中")
            } else {
                StatusCard::stopped("LuckyLilliaBot", "screen 会话未启动")
            }];
            self.print_status_cards("服务状态", &cards);
            let actions = [
                ActionItem::primary("启动 LLBot", "启动 screen: llbot 后台会话"),
                ActionItem::destructive("停止 LLBot", "结束 screen 会话"),
                ActionItem::normal("重启 LLBot", "重新拉起协议端进程"),
                ActionItem::normal("进入控制台", "screen -r llbot，使用 Ctrl+A D 分离"),
                ActionItem::normal("修改 WebUI 密码", "写入 webui_token.txt"),
                ActionItem::destructive("删除目录", "删除 LuckyLilliaBot 工作目录及数据"),
                ActionItem::back("返回", "回到协议端服务"),
            ];
            let choice = self.select_action("选择 LLBot 操作", &actions)?;
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

    pub(crate) fn manage_snowluma_menu(&self) -> Result<()> {
        let dir = self.snowluma_dir()?;
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("SnowLuma", "Docker 容器、数据重建与日志管理");
            self.print_kv("目录", &dir.display().to_string());
            let cards = [if snowluma_running() {
                StatusCard::running("SnowLuma", "Docker 容器 snowluma 正在运行")
            } else {
                StatusCard::stopped("SnowLuma", "Docker 容器未运行")
            }];
            self.print_status_cards("服务状态", &cards);
            let actions = [
                ActionItem::primary("启动 SnowLuma", "docker compose up -d"),
                ActionItem::destructive("停止 SnowLuma", "docker compose stop"),
                ActionItem::normal("重启 SnowLuma", "重启容器进程"),
                ActionItem::normal("查看实时日志", "跟随 docker compose logs"),
                ActionItem::normal("重建容器", "down + pull + up -d，保留数据"),
                ActionItem::destructive("删除数据并重建", "清空数据目录，新的首次启动密码会生成"),
                ActionItem::destructive("移除容器", "删除 snowluma 容器，保留数据目录"),
                ActionItem::back("返回", "回到协议端服务"),
            ];
            let choice = self.select_action("选择 SnowLuma 操作", &actions)?;
            let result = match choice {
                0 => self.start_snowluma(),
                1 => self.stop_snowluma(),
                2 => self.restart_snowluma(),
                3 => self.print_snowluma_logs(100, true),
                4 => self.rebuild_snowluma(),
                5 => {
                    if Confirm::with_theme(&self.theme)
                        .with_prompt("确认删除 SnowLuma 的全部数据并重建？")
                        .default(false)
                        .interact()?
                    {
                        self.recreate_snowluma_data()
                    } else {
                        Ok(())
                    }
                }
                6 => self.remove_snowluma_container(),
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
        }
        Ok(())
    }
}

fn print_screen_status(session: &str) -> Result<()> {
    if screen_exists(session)? {
        println!("{session}: running");
    } else {
        println!("{session}: stopped");
    }
    Ok(())
}
