use crate::{
    app::App,
    ui::{ActionItem, StatusCard},
    utils::{bat_quote, windows_tools_path_prelude},
};
use anyhow::{Result, bail};
use dialoguer::{Confirm, Input};
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
        println!("Windows 版本使用独立控制台 / Desktop 窗口运行 {session}，无法附着到已打开窗口。");
        Ok(())
    }

    fn maibot_paths(&self) -> Result<(PathBuf, String)> {
        let cfg = self.require_config()?;
        Ok((
            PathBuf::from(cfg.mai_path).join("MaiBot"),
            cfg.mai_python_env,
        ))
    }

    pub(crate) fn start_maibot_core(&self, _attach: bool) -> Result<()> {
        let (maibot_dir, py_env) = self.maibot_paths()?;
        let root = maibot_dir.parent().unwrap_or(&maibot_dir).to_path_buf();
        let logs_dir = root.join("logs");
        fs::create_dir_all(&logs_dir)?;
        let launcher_path = logs_dir.join("start-maibot.bat");
        let pid_path = logs_dir.join("maibot.pid");
        if pid_running(&pid_path)?.unwrap_or(false) {
            bail!("MaiBot 已在运行中");
        }
        let _ = fs::remove_file(&pid_path);

        let tools_prelude = windows_tools_path_prelude(&root);
        let run = if py_env == "uv" {
            format!(
                "{tools_prelude}where uv >nul 2>nul || (echo [ERROR] uv was not found. Install uv or reinstall MaiBot. & pause & exit /b 1)\r\n\
             uv run bot.py"
            )
        } else {
            format!(
                "{tools_prelude}if not exist ..\\venv\\Scripts\\activate.bat (echo [ERROR] virtualenv was not found: ..\\venv\\Scripts\\activate.bat & pause & exit /b 1)\r\n\
             call ..\\venv\\Scripts\\activate.bat\r\n\
             python bot.py"
            )
        };
        fs::write(
            &launcher_path,
            format!(
                "@echo off\r\n\
                 chcp 65001 >nul\r\n\
                 setlocal EnableExtensions\r\n\
                 set PYTHONUTF8=1\r\n\
                 set PYTHONIOENCODING=utf-8\r\n\
                 set PYTHONUNBUFFERED=1\r\n\
                 title {MAIBOT_TITLE}\r\n\
                 cd /d {}\r\n\
                 echo MaiBot Manager launcher v2\r\n\
                 echo Workdir: {}\r\n\
                 echo Manager logs: {}\r\n\
                 echo ------------------------------------------------------------\r\n\
                 {run}\r\n\
                 set MAIBOT_EXIT_CODE=%ERRORLEVEL%\r\n\
                 echo ------------------------------------------------------------\r\n\
                 echo MaiBot exited with code: %MAIBOT_EXIT_CODE%\r\n\
                 pause\r\n",
                bat_quote(&maibot_dir),
                maibot_dir.display(),
                logs_dir.display()
            ),
        )?;
        if py_env != "uv" {
            let python = root.join("venv").join("Scripts").join("python.exe");
            if !python.exists() {
                bail!("未找到 Python 虚拟环境: {}", python.display());
            }
        }
        let pid = start_bat_in_new_window(&launcher_path, &maibot_dir)?;
        fs::write(&pid_path, format!("{pid}\n"))?;
        println!("MaiBot PID: {pid}");
        Ok(())
    }

    pub(crate) fn stop_maibot_core(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let pid_path = PathBuf::from(cfg.mai_path).join("logs").join("maibot.pid");
        stop_window_by_pid_or_title(&pid_path, MAIBOT_TITLE)
    }

    pub(crate) fn restart_maibot_core(&self) -> Result<()> {
        let _ = self.stop_maibot_core();
        self.start_maibot_core(false)
    }

    pub(crate) fn attach_screen(&self, session: &str) -> Result<()> {
        self.warn_before_screen_attach(session)
    }

    pub(crate) fn print_maibot_core_status(&self) -> Result<()> {
        if self.maibot_core_running()? {
            println!("maibot: running");
        } else {
            println!("maibot: stopped");
        }
        Ok(())
    }

    pub(crate) fn print_llbot_status(&self) -> Result<()> {
        if self.llbot_running()? {
            println!("llbot: running");
        } else {
            println!("llbot: stopped");
        }
        Ok(())
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
        stop_process_tree_by_image_or_title("NapCatWinBootMain.exe", "NapCat*")
    }

    pub(crate) fn restart_napcat(&self) -> Result<()> {
        let _ = self.stop_napcat();
        self.start_napcat()
    }

    pub(crate) fn rebuild_napcat(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let plan = self.build_default_install_plan(&cfg)?;
        self.redownload_napcat_shell(&plan)
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
        let cfg = self.require_config()?;
        let pid_path = PathBuf::from(cfg.mai_path).join("logs").join("maibot.pid");
        Ok(pid_running(&pid_path)?.unwrap_or(false) || window_running(MAIBOT_TITLE)?)
    }

    pub(crate) fn llbot_running(&self) -> Result<bool> {
        Ok(cmd_success_with_timeout(
            "tasklist /fi \"imagename eq llbot.exe\" | findstr /i \"llbot.exe\"",
            Duration::from_millis(800),
        )?
        .unwrap_or(false))
    }

    pub(crate) fn napcat_running(&self) -> Result<bool> {
        Ok(process_image_running("NapCatWinBootMain.exe")? || window_running("NapCat")?)
    }

    pub(crate) fn manage_bot_protocol_menu(&self) -> Result<()> {
        self.require_config()?;
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("协议端服务", "选择要维护的 Bot 协议端");
            let actions = [
                ActionItem::primary("NapCatQQ", "Windows NapCat Shell"),
                ActionItem::normal("LuckyLilliaBot", "LLBot Desktop 进程"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择协议端", &actions)?;
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
            self.print_section("MaiBot 核心", "Windows 独立控制台启动、PID 停止与日志查看");
            let running = self.maibot_core_running().unwrap_or(false);
            let cards = [if running {
                StatusCard::running("MaiBot", "独立控制台窗口运行中 · PID 文件可停止进程树")
            } else {
                StatusCard::stopped("MaiBot", "核心控制台未运行")
            }];
            self.print_status_cards("核心状态", &cards);
            let actions = [
                ActionItem::primary("启动 MaiBot", "打开独立控制台，首次启动/EULA 在窗口中确认"),
                ActionItem::destructive("停止 MaiBot", "读取 PID 并结束完整进程树"),
                ActionItem::normal("查看日志", "跟随 logs/maibot.log"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择核心操作", &actions)?;
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
            let running = self.napcat_running().unwrap_or(false);
            let cards = [if running {
                StatusCard::running("NapCatQQ", "NapCat Shell 进程或窗口已运行")
            } else {
                StatusCard::stopped("NapCatQQ", "NapCat Shell 未运行")
            }];
            self.print_status_cards("服务状态", &cards);
            let actions = [
                ActionItem::primary("启动 NapCat", "打开 NapCat Shell"),
                ActionItem::destructive("停止 NapCat", "结束 NapCat Shell 进程"),
                ActionItem::normal("重启 NapCat", "停止后重新启动"),
                ActionItem::normal("查看实时日志", "跟随 NapCat Shell 日志"),
                ActionItem::normal("重新下载 Shell 包", "获取最新 NapCat.Shell.zip"),
                ActionItem::back("返回", "回到协议端服务"),
            ];
            let choice = self.select_action("选择 NapCat 操作", &actions)?;
            let result = match choice {
                0 => self.start_napcat(),
                1 => self.stop_napcat(),
                2 => self.restart_napcat(),
                3 => self.print_napcat_logs(100, true),
                4 => self.rebuild_napcat(),
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
            let running = self.llbot_running().unwrap_or(false);
            let cards = [if running {
                StatusCard::running("LuckyLilliaBot", "LLBot Desktop 进程已运行")
            } else {
                StatusCard::stopped("LuckyLilliaBot", "LLBot Desktop 未运行")
            }];
            self.print_status_cards("服务状态", &cards);
            let actions = [
                ActionItem::primary("启动 LLBot", "打开 LLBot Desktop"),
                ActionItem::destructive("停止 LLBot", "结束 Desktop 进程"),
                ActionItem::normal("重启 LLBot", "停止后重新启动"),
                ActionItem::normal("查看日志", "读取 llbot.log"),
                ActionItem::normal("修改 WebUI 密码", "写入 LLBot WebUI token"),
                ActionItem::destructive("删除目录", "删除 LuckyLilliaBot 工作目录及数据"),
                ActionItem::back("返回", "回到协议端服务"),
            ];
            let choice = self.select_action("选择 LLBot 操作", &actions)?;
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
    let title_pattern = ps_single_quote(&format!("{title}*"));
    powershell_success_with_timeout(
        &format!(
            "if (Get-Process | Where-Object {{ $_.MainWindowTitle -like {title_pattern} }}) {{ exit 0 }} else {{ exit 1 }}"
        ),
        Duration::from_millis(800),
    )
}

fn stop_window_by_pid_or_title(pid_path: &Path, title: &str) -> Result<()> {
    if let Some(pid) = read_pid(pid_path)? {
        if maibot_pid_running(pid)? {
            let pid = pid.to_string();
            let status = Command::new("taskkill")
                .args(["/PID", &pid, "/T", "/F"])
                .status()?;
            if status.success() {
                let _ = fs::remove_file(pid_path);
                return Ok(());
            }
        } else {
            let _ = fs::remove_file(pid_path);
        }
    }

    if stop_process_tree_by_title_pattern(&format!("{title}*")).is_ok() {
        let _ = fs::remove_file(pid_path);
        return Ok(());
    }
    bail!("未找到运行中的窗口: {title}")
}

fn stop_process_tree_by_image_or_title(image_name: &str, title_pattern: &str) -> Result<()> {
    let status = Command::new("taskkill")
        .args(["/IM", image_name, "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        return Ok(());
    }
    stop_process_tree_by_title_pattern(title_pattern)
}

fn stop_process_tree_by_title_pattern(title_pattern: &str) -> Result<()> {
    let display_pattern = title_pattern.to_string();
    let title_pattern = ps_single_quote(title_pattern);
    let script = format!(
        "$p = Get-Process | Where-Object {{ $_.MainWindowTitle -like {title_pattern} }}; if ($p) {{ $p | ForEach-Object {{ taskkill /PID $_.Id /T /F | Out-Null }}; exit 0 }} else {{ exit 1 }}"
    );
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()?;
    if !status.success() {
        bail!("未找到匹配窗口: {display_pattern}");
    }
    Ok(())
}

fn pid_running(pid_path: &Path) -> Result<Option<bool>> {
    let Some(pid) = read_pid(pid_path)? else {
        return Ok(None);
    };
    Ok(Some(maibot_pid_running(pid)?))
}

fn maibot_pid_running(pid: u32) -> Result<bool> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()?;
    if !output.status.success() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    Ok(stdout.contains("\"uv.exe\"")
        || stdout.contains("\"python.exe\"")
        || stdout.contains("\"cmd.exe\""))
}

fn process_image_running(image_name: &str) -> Result<bool> {
    let output = Command::new("tasklist")
        .args([
            "/FI",
            &format!("IMAGENAME eq {image_name}"),
            "/FO",
            "CSV",
            "/NH",
        ])
        .output()?;
    if !output.status.success() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    Ok(stdout.contains(&format!("\"{}\"", image_name.to_ascii_lowercase())))
}

fn read_pid(pid_path: &Path) -> Result<Option<u32>> {
    if !pid_path.exists() {
        return Ok(None);
    }
    Ok(fs::read_to_string(pid_path)?.trim().parse::<u32>().ok())
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

fn start_bat_in_new_window(launcher: &Path, workdir: &Path) -> Result<u32> {
    let cmd_arg = format!("/C \"{}\"", launcher.display());
    let script = format!(
        "$ErrorActionPreference = 'Stop'; \
         $p = Start-Process -FilePath $env:ComSpec -ArgumentList {} -WorkingDirectory {} -WindowStyle Normal -PassThru; \
         Write-Output $p.Id",
        ps_single_quote(&cmd_arg),
        ps_single_quote(&workdir.display().to_string())
    );
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = stderr
            .lines()
            .next()
            .or_else(|| stdout.lines().next())
            .unwrap_or("Start-Process 执行失败")
            .trim()
            .to_string();
        bail!("启动 MaiBot 独立窗口失败: {detail}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().parse::<u32>().ok())
        .ok_or_else(|| anyhow::anyhow!("启动 MaiBot 后未获取到窗口 PID"))
}

fn ps_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn cmd_success_with_timeout(command: &str, timeout: Duration) -> Result<Option<bool>> {
    Ok(cmd_output_with_timeout(command, timeout)?.map(|output| output.status.success()))
}

fn cmd_output_with_timeout(command_text: &str, timeout: Duration) -> Result<Option<Output>> {
    let mut command = Command::new("cmd");
    command.args(["/C", command_text]);
    command_output_with_timeout(&mut command, timeout)
}

fn powershell_success_with_timeout(script: &str, timeout: Duration) -> Result<bool> {
    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    Ok(command_output_with_timeout(&mut command, timeout)?
        .map(|output| output.status.success())
        .unwrap_or(false))
}

fn command_output_with_timeout(command: &mut Command, timeout: Duration) -> Result<Option<Output>> {
    let mut child = command
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
