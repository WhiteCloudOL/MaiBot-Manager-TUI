use crate::{app::App, utils::bat_quote};
use anyhow::{Result, bail};
use dialoguer::{Confirm, Input, Select};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAIBOT_TITLE: &str = "MaiBot maibot";
const LLBOT_TITLE: &str = "MaiBot llbot";

impl App {
    pub(crate) fn warn_before_screen_attach(&self, session: &str) -> Result<()> {
        println!("Windows 版本使用独立 cmd 窗口运行 {session}，无需 screen 分离热键。");
        Ok(())
    }

    fn maibot_paths(&self) -> Result<(PathBuf, String)> {
        let cfg = self.require_config()?;
        Ok((
            PathBuf::from(cfg.mai_path).join("MaiBot"),
            cfg.mai_python_env,
        ))
    }

    pub(crate) fn start_maibot_core(&self, attach: bool) -> Result<()> {
        let (maibot_dir, py_env) = self.maibot_paths()?;
        let logs_dir = maibot_dir.parent().unwrap_or(&maibot_dir).join("logs");
        fs::create_dir_all(&logs_dir)?;
        let log_path = logs_dir.join("maibot.log");
        let launcher_path = logs_dir.join("start-maibot.bat");
        let run = if py_env == "uv" {
            format!(
                "where uv >nul 2>nul || (echo [ERROR] 未找到 uv，请先安装 uv 或重新安装 MaiBot。 & pause & exit /b 1)\r\n\
                 powershell -NoProfile -ExecutionPolicy Bypass -Command \"& {{ uv run bot.py 2^>^&1 | Tee-Object -FilePath {} -Append }}\"",
                ps_single_quote(&log_path)
            )
        } else {
            format!(
                "if not exist ..\\venv\\Scripts\\activate.bat (echo [ERROR] 未找到虚拟环境: ..\\venv\\Scripts\\activate.bat & pause & exit /b 1)\r\n\
                 call ..\\venv\\Scripts\\activate.bat\r\n\
                 powershell -NoProfile -ExecutionPolicy Bypass -Command \"& {{ python bot.py 2^>^&1 | Tee-Object -FilePath {} -Append }}\"",
                ps_single_quote(&log_path)
            )
        };
        fs::write(
            &launcher_path,
            format!(
                "@echo off\r\n\
                 setlocal EnableExtensions\r\n\
                 title {MAIBOT_TITLE}\r\n\
                 cd /d {}\r\n\
                 echo MaiBot 启动目录: {}\r\n\
                 echo 日志文件: {}\r\n\
                 echo ------------------------------------------------------------\r\n\
                 {run}\r\n\
                 echo ------------------------------------------------------------\r\n\
                 echo MaiBot 进程已退出，退出码: %ERRORLEVEL%\r\n\
                 pause\r\n",
                bat_quote(&maibot_dir),
                maibot_dir.display(),
                log_path.display()
            ),
        )?;
        let window_flag = if attach { "" } else { "/MIN" };
        self.run_shell(&format!(
            "start \"{MAIBOT_TITLE}\" {window_flag} /D {} cmd.exe /K call {}",
            bat_quote(&maibot_dir),
            bat_quote(&launcher_path)
        ))
    }

    pub(crate) fn stop_maibot_core(&self) -> Result<()> {
        stop_window(MAIBOT_TITLE)
    }

    pub(crate) fn restart_maibot_core(&self) -> Result<()> {
        let _ = self.stop_maibot_core();
        self.start_maibot_core(false)
    }

    pub(crate) fn attach_screen(&self, session: &str) -> Result<()> {
        self.warn_before_screen_attach(session)
    }

    pub(crate) fn print_maibot_core_status(&self) -> Result<()> {
        print_window_status("maibot", MAIBOT_TITLE)
    }

    pub(crate) fn print_llbot_status(&self) -> Result<()> {
        print_window_status("llbot", LLBOT_TITLE)
    }

    pub(crate) fn print_maibot_core_logs(&self, tail: usize, follow: bool) -> Result<()> {
        let cfg = self.require_config()?;
        self.print_log_file(
            &PathBuf::from(cfg.mai_path).join("logs/maibot.log"),
            tail,
            follow,
        )
    }

    pub(crate) fn print_llbot_logs(&self, tail: usize, follow: bool) -> Result<()> {
        let cfg = self.require_config()?;
        let llbot_dir = if cfg.mai_llbot_path.is_empty() {
            PathBuf::from(cfg.mai_path).join("LLBot")
        } else {
            PathBuf::from(cfg.mai_llbot_path)
        };
        self.print_log_file(&llbot_dir.join("llbot.log"), tail, follow)
    }

    fn print_log_file(&self, path: &Path, tail: usize, follow: bool) -> Result<()> {
        let mut script = format!(
            "if not exist {} (echo 日志文件不存在: {} & exit /b 1)\r\n",
            bat_quote(path),
            path.display()
        );
        if follow {
            script.push_str(&format!(
                ":loop\r\ncls\r\npowershell -NoProfile -Command \"Get-Content -Tail {} -Path '{}'\"\r\ntimeout /t 2 /nobreak >nul\r\ngoto loop",
                tail,
                path.display().to_string().replace('\'', "''")
            ));
        } else {
            script.push_str(&format!(
                "powershell -NoProfile -Command \"Get-Content -Tail {} -Path '{}'\"",
                tail,
                path.display().to_string().replace('\'', "''")
            ));
        }
        self.run_shell(&script)
    }

    fn napcat_dir(&self) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        Ok(PathBuf::from(cfg.mai_path).join("NapCat"))
    }

    pub(crate) fn start_napcat(&self) -> Result<()> {
        let napcat_dir = self.napcat_dir()?;
        let launcher = napcat_dir.join("launcher.bat");
        if !launcher.exists() {
            bail!("未找到 NapCat 启动脚本: {}", launcher.display());
        }
        self.run_shell(&run_as_admin_script(&launcher, &napcat_dir, "NapCat Shell"))
    }

    pub(crate) fn stop_napcat(&self) -> Result<()> {
        self.run_shell("taskkill /im node.exe /fi \"WINDOWTITLE eq NapCat*\" /t /f")
    }

    pub(crate) fn restart_napcat(&self) -> Result<()> {
        let _ = self.stop_napcat();
        self.start_napcat()
    }

    pub(crate) fn rebuild_napcat(&self) -> Result<()> {
        bail!(
            "Windows NapCat Shell 不支持 Docker rebuild；请执行 install/update 重新下载最新 Shell 包"
        )
    }

    pub(crate) fn remove_napcat_container(&self) -> Result<()> {
        bail!("Windows NapCat Shell 不使用 Docker 容器")
    }

    pub(crate) fn print_napcat_logs(&self, tail: usize, follow: bool) -> Result<()> {
        let napcat_dir = self.napcat_dir()?;
        self.print_log_file(&napcat_dir.join("logs").join("onebot.log"), tail, follow)
    }

    pub(crate) fn print_napcat_status(&self) -> Result<()> {
        if self.napcat_running()? {
            println!("napcat: running");
        } else {
            println!("napcat: stopped");
        }
        Ok(())
    }

    pub(crate) fn exec_napcat_shell(&self) -> Result<()> {
        self.start_napcat()
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
        let exe = llbot_dir.join("llbot.exe");
        if !exe.exists() {
            bail!("未找到 LuckyLilliaBot Desktop 启动文件: {}", exe.display());
        }
        self.run_shell(&run_as_admin_script(
            &exe,
            &llbot_dir,
            "LuckyLilliaBot Desktop",
        ))
    }

    pub(crate) fn stop_llbot(&self) -> Result<()> {
        self.run_shell("taskkill /im llbot.exe /t /f")
    }

    pub(crate) fn restart_llbot(&self) -> Result<()> {
        let _ = self.stop_llbot();
        self.start_llbot()
    }

    pub(crate) fn set_llbot_password(&self, password: &str) -> Result<()> {
        let token_file = self
            .llbot_dir()?
            .join("bin")
            .join("llbot")
            .join("data")
            .join("webui_token.txt");
        if let Some(parent) = token_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(token_file, format!("{password}\n"))?;
        Ok(())
    }

    pub(crate) fn maibot_core_running(&self) -> Result<bool> {
        window_running(MAIBOT_TITLE)
    }

    pub(crate) fn llbot_running(&self) -> Result<bool> {
        Ok(cmd_success_with_timeout(
            "tasklist /fi \"imagename eq llbot.exe\" | findstr /i \"llbot.exe\"",
            Duration::from_millis(800),
        )?
        .unwrap_or(false))
    }

    pub(crate) fn napcat_running(&self) -> Result<bool> {
        Ok(cmd_output_with_timeout(
            "tasklist /v | findstr /i \"NapCat\"",
            Duration::from_millis(800),
        )?
        .map(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty())
        .unwrap_or(false))
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
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("MaiBot 核心", "Windows cmd 窗口启动、停止与日志查看");
            self.print_maibot_core_status()?;
            let choice = Select::with_theme(&self.theme)
                .with_prompt("MaiBot 核心管理")
                .items(["启动 MaiBot", "停止 MaiBot", "查看日志", "返回"])
                .default(0)
                .interact()?;
            let result = match choice {
                0 => self.start_maibot_core(false),
                1 => self.stop_maibot_core(),
                2 => self.print_maibot_core_logs(100, true),
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
        }
        Ok(())
    }

    pub(crate) fn manage_napcat_menu(&self) -> Result<()> {
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("NapCatQQ", "Shell 版启动、停止与日志查看");
            let choice = Select::with_theme(&self.theme)
                .with_prompt("NapCat 管理")
                .items([
                    "启动 NapCat",
                    "停止 NapCat",
                    "重启 NapCat",
                    "查看实时日志",
                    "重新下载最新 Shell 包",
                    "说明：Windows 版不使用 Docker",
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
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
        }
        Ok(())
    }

    pub(crate) fn manage_llbot_menu(&self) -> Result<()> {
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("LuckyLilliaBot", "Desktop 版启动、停止与日志查看");
            self.print_llbot_status()?;
            let choice = Select::with_theme(&self.theme)
                .with_prompt("LuckyLilliaBot 管理")
                .items([
                    "启动 LuckyLilliaBot",
                    "停止 LuckyLilliaBot",
                    "重启 LuckyLilliaBot",
                    "查看日志",
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
                3 => self.print_llbot_logs(100, true),
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
                        fs::remove_dir_all(self.llbot_dir()?).ok();
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

fn window_running(title: &str) -> Result<bool> {
    Ok(cmd_success_with_timeout(
        &format!("tasklist /v /fi \"WINDOWTITLE eq {title}*\" | findstr /i \"cmd.exe\""),
        Duration::from_millis(800),
    )?
    .unwrap_or(false))
}

fn print_window_status(name: &str, title: &str) -> Result<()> {
    if window_running(title)? {
        println!("{name}: running");
    } else {
        println!("{name}: stopped");
    }
    Ok(())
}

fn stop_window(title: &str) -> Result<()> {
    let status = Command::new("cmd")
        .args([
            "/C",
            &format!("taskkill /fi \"WINDOWTITLE eq {title}*\" /t /f"),
        ])
        .status()?;
    if !status.success() {
        bail!("未找到运行中的窗口: {title}");
    }
    Ok(())
}

fn run_as_admin_script(target: &Path, workdir: &Path, title: &str) -> String {
    let target = target.display().to_string().replace('\'', "''");
    let workdir = workdir.display().to_string().replace('\'', "''");
    if target.to_ascii_lowercase().ends_with(".bat") {
        format!(
            "echo 将请求管理员权限启动 {title}...\r\npowershell -NoProfile -ExecutionPolicy Bypass -Command \"Start-Process -FilePath 'cmd.exe' -ArgumentList '/C \"\"{target}\"\"' -WorkingDirectory '{workdir}' -Verb RunAs\""
        )
    } else {
        format!(
            "echo 将请求管理员权限启动 {title}...\r\npowershell -NoProfile -ExecutionPolicy Bypass -Command \"Start-Process -FilePath '{target}' -WorkingDirectory '{workdir}' -Verb RunAs\""
        )
    }
}

fn ps_single_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

fn cmd_success_with_timeout(command: &str, timeout: Duration) -> Result<Option<bool>> {
    Ok(cmd_output_with_timeout(command, timeout)?.map(|output| output.status.success()))
}

fn cmd_output_with_timeout(command: &str, timeout: Duration) -> Result<Option<Output>> {
    let mut child = Command::new("cmd")
        .args(["/C", command])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(Some(child.wait_with_output()?));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}
