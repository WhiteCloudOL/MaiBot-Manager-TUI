use crate::{app::App, utils::*};
use anyhow::{Context, Result, anyhow, bail};
use dialoguer::Select;
use dialoguer::console::style;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const MAIBOT_PID_FILE: &str = "maibot.pid";
const MAIBOT_LOG_FILE: &str = "maibot.log";

impl App {
    pub(crate) fn warn_before_screen_attach(&self, session: &str) -> Result<()> {
        if session == "llbot" {
            return macos_protocol_todo();
        }
        println!(
            "{}",
            style("macOS 版不使用 screen；这里会直接跟随 logs/maibot.log。").dim()
        );
        Ok(())
    }

    fn maibot_paths(&self) -> Result<(PathBuf, PathBuf, PathBuf, String)> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        Ok((
            root.clone(),
            root.join("MaiBot"),
            root.join("venv/bin/activate"),
            cfg.mai_python_env,
        ))
    }

    pub(crate) fn start_maibot_core(&self, attach: bool) -> Result<()> {
        let (root, maibot_dir, venv_activate, py_env) = self.maibot_paths()?;
        if !maibot_dir.exists() {
            bail!("未找到 MaiBot 目录: {}", maibot_dir.display());
        }
        if py_env != "uv" && !venv_activate.exists() {
            bail!("未找到 Python 虚拟环境: {}", venv_activate.display());
        }

        let (logs_dir, pid_path, log_path) = maibot_runtime_paths(&root);
        fs::create_dir_all(&logs_dir)?;
        if let Some(pid) = pid_running(&pid_path)? {
            bail!("MaiBot 已在运行中 (PID {pid})");
        }
        let _ = fs::remove_file(&pid_path);

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("打开日志文件失败: {}", log_path.display()))?;
        let log_file = Arc::new(Mutex::new(log_file));
        write_log_marker(&log_file, "MaiBot Manager macOS session started")?;

        let run = if py_env == "uv" {
            format!("{} exec uv run bot.py", macos_tools_prelude(&root))
        } else {
            format!(
                "{} . '{}' && exec python3 bot.py",
                macos_tools_prelude(&root),
                shell_escape(&venv_activate)
            )
        };
        let mut child = Command::new("/bin/zsh")
            .arg("-lc")
            .arg(run)
            .current_dir(&maibot_dir)
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| "启动 MaiBot 子进程失败")?;

        let pid = child.id();
        fs::write(&pid_path, format!("{pid}\n"))
            .with_context(|| format!("写入 PID 文件失败: {}", pid_path.display()))?;
        println!();
        println!(
            "{} {}",
            style("▶").cyan().bold(),
            style(format!("MaiBot 已作为当前管理器的子进程启动 (PID {pid})")).cyan()
        );
        println!(
            "  {}",
            style("按 Ctrl+C 可结束当前会话；日志同步写入:").dim()
        );
        println!("  {}", style(log_path.display().to_string()).dim());
        if attach {
            println!(
                "  {}",
                style("--exec 在 macOS 下等同于前台日志会话。").dim()
            );
        }
        println!();

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("无法读取 MaiBot stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("无法读取 MaiBot stderr"))?;
        let stdout_thread = pipe_child_output(stdout, false, Arc::clone(&log_file));
        let stderr_thread = pipe_child_output(stderr, true, Arc::clone(&log_file));

        let status = child.wait().with_context(|| "等待 MaiBot 子进程退出失败")?;
        join_output_thread(stdout_thread)?;
        join_output_thread(stderr_thread)?;
        let _ = fs::remove_file(&pid_path);
        write_log_marker(&log_file, &format!("MaiBot exited with status: {status}"))?;

        if !status.success() {
            bail!("MaiBot 已退出，状态: {status}");
        }
        println!("{}", style("MaiBot 进程已正常退出。").green());
        Ok(())
    }

    pub(crate) fn stop_maibot_core(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let (_, pid_path, _) = maibot_runtime_paths(&root);
        let Some(pid) = pid_running(&pid_path)? else {
            bail!("MaiBot 未在运行");
        };
        let cmd = format!(
            "terminate_tree() {{ for child in $(pgrep -P \"$1\" 2>/dev/null); do terminate_tree \"$child\"; done; kill -TERM \"$1\" 2>/dev/null || true; }}; terminate_tree {pid}"
        );
        self.run_shell(&cmd)?;
        thread::sleep(Duration::from_millis(800));
        if pid_running(&pid_path)?.is_some() {
            bail!("已发送停止信号，但 MaiBot 进程仍在运行 (PID {pid})");
        }
        let _ = fs::remove_file(pid_path);
        Ok(())
    }

    pub(crate) fn restart_maibot_core(&self) -> Result<()> {
        let _ = self.stop_maibot_core();
        self.start_maibot_core(false)
    }

    pub(crate) fn attach_screen(&self, session: &str) -> Result<()> {
        self.warn_before_screen_attach(session)?;
        self.print_maibot_core_logs(200, true)
    }

    pub(crate) fn print_maibot_core_status(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let (_, pid_path, _) = maibot_runtime_paths(&root);
        if let Some(pid) = pid_running(&pid_path)? {
            println!("maibot: running (pid: {pid})");
        } else {
            println!("maibot: stopped");
        }
        Ok(())
    }

    pub(crate) fn print_maibot_core_logs(&self, tail: usize, follow: bool) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let (_, _, log_path) = maibot_runtime_paths(&root);
        print_log_file(self, &log_path, tail, follow)
    }

    pub(crate) fn print_napcat_status(&self) -> Result<()> {
        println!("napcat: unsupported on macOS (TODO)");
        Ok(())
    }

    pub(crate) fn print_llbot_status(&self) -> Result<()> {
        println!("llbot: unsupported on macOS (TODO)");
        Ok(())
    }

    pub(crate) fn print_napcat_logs(&self, _tail: usize, _follow: bool) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn print_llbot_logs(&self, _tail: usize, _follow: bool) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn start_napcat(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn stop_napcat(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn restart_napcat(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn rebuild_napcat(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn remove_napcat_container(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn exec_napcat_shell(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn start_llbot(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn stop_llbot(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn restart_llbot(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn set_llbot_password(&self, _password: &str) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn manage_bot_protocol_menu(&self) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.print_section("Bot 协议端服务", "macOS 版暂未适配 NapCat / LLBot");
        self.print_hint("TODO: 后续再接入 macOS 原生协议端部署与管理。");
        self.print_line();
        self.pause("按回车返回主菜单")?;
        Ok(())
    }

    pub(crate) fn manage_maibot_menu(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let maibot_dir = root.join("MaiBot");
        let (_, pid_path, _) = maibot_runtime_paths(&root);
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("MaiBot 核心", "前台启动并在当前 TUI 中显示日志");
            self.print_kv("目录", &maibot_dir.display().to_string());
            let running = pid_running(&pid_path)?.is_some();
            self.print_status_dot(
                "运行状态",
                if running { "运行中" } else { "未运行" },
                running,
            );
            let choice = Select::with_theme(&self.theme)
                .with_prompt("MaiBot 核心管理")
                .items(["启动并显示日志", "停止 MaiBot", "查看实时日志", "返回"])
                .default(0)
                .interact()?;
            let result = match choice {
                0 => self.start_maibot_core(false),
                1 => self.stop_maibot_core(),
                2 => self.print_maibot_core_logs(200, true),
                _ => break,
            };
            if self.handle_menu_result(result)? {
                self.pause("操作已执行，按回车继续")?;
            }
        }
        Ok(())
    }

    pub(crate) fn manage_napcat_menu(&self) -> Result<()> {
        macos_protocol_todo()
    }

    pub(crate) fn manage_llbot_menu(&self) -> Result<()> {
        macos_protocol_todo()
    }
}

fn macos_protocol_todo() -> Result<()> {
    bail!("macOS 版暂未适配 NapCat / LLBot 协议端，这部分已留作 TODO")
}

fn maibot_runtime_paths(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let logs_dir = root.join("logs");
    let pid_path = logs_dir.join(MAIBOT_PID_FILE);
    let log_path = logs_dir.join(MAIBOT_LOG_FILE);
    (logs_dir, pid_path, log_path)
}

fn write_log_marker(log_file: &Arc<Mutex<File>>, marker: &str) -> Result<()> {
    let mut log = log_file.lock().map_err(|_| anyhow!("日志写入锁已损坏"))?;
    writeln!(log)?;
    writeln!(log, "===== {marker} =====")?;
    log.flush()?;
    Ok(())
}

fn pipe_child_output<R>(
    mut reader: R,
    stderr: bool,
    log_file: Arc<Mutex<File>>,
) -> thread::JoinHandle<Result<()>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if stderr {
                let mut out = io::stderr().lock();
                out.write_all(&buf[..n])?;
                out.flush()?;
            } else {
                let mut out = io::stdout().lock();
                out.write_all(&buf[..n])?;
                out.flush()?;
            }
            let mut log = log_file.lock().map_err(|_| anyhow!("日志写入锁已损坏"))?;
            log.write_all(&buf[..n])?;
            log.flush()?;
        }
        Ok(())
    })
}

fn join_output_thread(handle: thread::JoinHandle<Result<()>>) -> Result<()> {
    handle.join().map_err(|_| anyhow!("日志输出线程异常退出"))?
}

fn print_log_file(app: &App, path: &Path, tail: usize, follow: bool) -> Result<()> {
    if !path.exists() {
        bail!("日志文件不存在: {}", path.display());
    }
    let escaped = shell_escape(path);
    let cmd = if follow {
        format!(
            "{} while true; do clear; tail -n {tail} '{}'; sleep 2; done",
            macos_path_export(),
            escaped
        )
    } else {
        format!("{} tail -n {tail} '{}'", macos_path_export(), escaped)
    };
    app.run_shell(&cmd)
}
