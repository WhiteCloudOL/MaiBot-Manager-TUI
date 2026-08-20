use crate::{
    app::App,
    ui::{ActionItem, StatusCard},
    utils::*,
};
use anyhow::{Context, Result, bail};
use dialoguer::console::style;
use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

const MAIBOT_PID_FILE: &str = "maibot.pid";
const MAIBOT_LOG_FILE: &str = "maibot.log";

impl App {
    pub(crate) fn start_snowluma(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn stop_snowluma(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn restart_snowluma(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn rebuild_snowluma(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn recreate_snowluma_data(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn remove_snowluma_container(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn print_snowluma_logs(&self, _: usize, _: bool) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn print_snowluma_status(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }
    pub(crate) fn exec_snowluma_shell(&self) -> Result<()> {
        bail!("SnowLuma 当前仅支持 Linux Docker 部署")
    }

    pub(crate) fn warn_before_screen_attach(&self, session: &str) -> Result<()> {
        if session == "llbot" {
            return macos_protocol_note();
        }
        println!(
            "{}",
            style("macOS 版不使用 screen；这里会跟随 logs/maibot.log，不影响后台进程。").dim()
        );
        Ok(())
    }

    fn maibot_paths(&self) -> Result<(PathBuf, PathBuf, PathBuf, String, String, String)> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        Ok((
            root.clone(),
            root.join("MaiBot"),
            root.join("venv/bin/activate"),
            cfg.mai_python_env,
            cfg.pip_index,
            cfg.pip_host,
        ))
    }

    pub(crate) fn start_maibot_core(&self, attach: bool) -> Result<()> {
        let (root, maibot_dir, venv_activate, py_env, pip_index, pip_host) = self.maibot_paths()?;
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

        if attach {
            if self.cli_mode {
                return self.start_maibot_core_foreground(
                    &root,
                    &maibot_dir,
                    &venv_activate,
                    &py_env,
                    &pip_index,
                    &pip_host,
                    &pid_path,
                    &log_path,
                );
            }
            return self.start_maibot_core_terminal(
                &root,
                &maibot_dir,
                &venv_activate,
                &py_env,
                &pip_index,
                &pip_host,
                &logs_dir,
                &pid_path,
                &log_path,
            );
        }

        let run = maibot_run_command(&root, &venv_activate, &py_env, &pip_index, &pip_host);
        let launch = format!(
            "printf '%s\\n' $$ > '{pid}'; \
             printf '\\n===== MaiBot Manager macOS background session started =====\\n' >> '{log}'; \
             ({run}) >> '{log}' 2>&1; \
             status=$?; \
             printf '\\n===== MaiBot exited with status: %s =====\\n' \"$status\" >> '{log}'; \
             rm -f '{pid}'; \
             exit \"$status\"",
            pid = shell_escape(&pid_path),
            log = shell_escape(&log_path),
            run = run
        );
        let mut command = Command::new("/bin/zsh");
        command
            .arg("-lc")
            .arg(launch)
            .current_dir(&maibot_dir)
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command
            .spawn()
            .with_context(|| "启动 MaiBot 后台子进程失败")?;

        let pid = child.id();
        fs::write(&pid_path, format!("{pid}\n"))
            .with_context(|| format!("写入 PID 文件失败: {}", pid_path.display()))?;
        thread::sleep(Duration::from_millis(300));
        if let Some(status) = child
            .try_wait()
            .with_context(|| "检查 MaiBot 后台子进程状态失败")?
        {
            let _ = fs::remove_file(&pid_path);
            bail!(
                "MaiBot 启动后很快退出 (状态: {status})，请查看日志: {}",
                log_path.display()
            );
        }
        if pid_running(&pid_path)?.is_none() {
            bail!("MaiBot 启动后很快退出，请查看日志: {}", log_path.display());
        }
        thread::spawn(move || {
            let _ = child.wait();
        });

        println!();
        println!(
            "{} {}",
            style("▶").cyan().bold(),
            style(format!("MaiBot 已在后台启动 (PID {pid})")).cyan()
        );
        println!(
            "  {}",
            style("管理器退出后 MaiBot 会继续运行；日志写入:").dim()
        );
        println!("  {}", style(log_path.display().to_string()).dim());
        println!();

        Ok(())
    }

    fn start_maibot_core_terminal(
        &self,
        root: &Path,
        maibot_dir: &Path,
        venv_activate: &Path,
        py_env: &str,
        pip_index: &str,
        pip_host: &str,
        logs_dir: &Path,
        pid_path: &Path,
        log_path: &Path,
    ) -> Result<()> {
        let launcher_path = logs_dir.join("start-maibot-terminal.zsh");
        let run = maibot_run_command(root, venv_activate, py_env, pip_index, pip_host);
        let script = format!(
            r#"#!/bin/zsh
cd '{workdir}' || exit 1
export PYTHONUNBUFFERED=1
export PYTHONUTF8=1
export PYTHONIOENCODING=utf-8
printf '%s\n' $$ > '{pid}'
printf '\n===== MaiBot Manager macOS interactive terminal started =====\n' >> '{log}'
clear
cat <<'MAIBOT_MANAGER_HINT'
╭──────────────── MaiBot 交互终端 ────────────────╮
│ 首次启动 / EULA：请在此窗口中按提示输入确认。   │
│ 返回管理器：直接切回原 MaiBot Manager 窗口即可。│
│ 退出终端：Ctrl+C 或关闭窗口，会停止当前进程。   │
│ 后台运行：完成 EULA 后，回管理器选择后台启动。  │
╰─ 管理器未被占用，可直接切回原窗口继续操作 ─╯

MAIBOT_MANAGER_HINT
({run}) 2>&1 | tee -a '{log}'
status=${{pipestatus[1]}}
printf '\n===== MaiBot exited with status: %s =====\n' "$status" >> '{log}'
rm -f '{pid}'
echo
echo "MaiBot 已退出，状态: $status，窗口即将关闭..."
sleep 1
exit "$status"
"#,
            workdir = shell_escape(maibot_dir),
            pid = shell_escape(pid_path),
            log = shell_escape(log_path),
            run = run
        );
        fs::write(&launcher_path, script)
            .with_context(|| format!("写入 macOS 交互启动脚本失败: {}", launcher_path.display()))?;
        let terminal_command = format!("/bin/zsh '{}'; exit 0", shell_escape(&launcher_path));
        let osa = format!(
            "tell application \"Terminal\" to do script \"{}\"",
            applescript_escape(&terminal_command)
        );
        let status = Command::new("osascript")
            .arg("-e")
            .arg(osa)
            .status()
            .with_context(|| "打开 macOS Terminal 交互窗口失败")?;
        if !status.success() {
            bail!("打开 macOS Terminal 交互窗口失败，状态: {status}");
        }
        println!();
        println!(
            "{} {}",
            style("▶").cyan().bold(),
            style("已打开 MaiBot 交互终端").cyan()
        );
        println!(
            "  {}",
            style("首次启动/EULA 请在新 Terminal 窗口完成；管理器可继续使用。").dim()
        );
        println!(
            "  {}",
            style("返回管理器：切回原 MaiBot Manager 窗口；退出交互终端：Ctrl+C 或关闭窗口。")
                .dim()
        );
        println!("  {}", style(log_path.display().to_string()).dim());
        println!();
        Ok(())
    }

    fn start_maibot_core_foreground(
        &self,
        root: &Path,
        maibot_dir: &Path,
        venv_activate: &Path,
        py_env: &str,
        pip_index: &str,
        pip_host: &str,
        pid_path: &Path,
        log_path: &Path,
    ) -> Result<()> {
        let run = maibot_run_command(root, venv_activate, py_env, pip_index, pip_host);
        let launch = format!(
            "printf '%s\\n' $$ > '{pid}'; \
             printf '\\n===== MaiBot Manager macOS attached terminal started =====\\n' >> '{log}'; \
             printf '\\n╭──────────────── MaiBot 附加终端 ────────────────╮\\n'; \
             printf '│ 首次启动 / EULA：请在此终端中按提示输入确认。   │\\n'; \
             printf '│ 快捷退出：Ctrl+C 会停止当前 MaiBot 进程。       │\\n'; \
             printf '│ 后台运行：完成 EULA 后重新执行后台启动。        │\\n'; \
             printf '╰─ 当前为交互模式，不是后台托管模式 ─╯\\n\\n'; \
             ({run}) 2>&1 | tee -a '{log}'; \
             status=${{pipestatus[1]}}; \
             printf '\\n===== MaiBot exited with status: %s =====\\n' \"$status\" >> '{log}'; \
             rm -f '{pid}'; \
             exit \"$status\"",
            pid = shell_escape(pid_path),
            log = shell_escape(log_path),
            run = run
        );
        let status = Command::new("/bin/zsh")
            .arg("-lc")
            .arg(launch)
            .current_dir(maibot_dir)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| "启动 MaiBot 附加终端失败")?;
        if !status.success() {
            bail!("MaiBot 附加终端已退出，状态: {status}");
        }
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
            "terminate_tree() {{ for child in $(pgrep -P \"$1\" 2>/dev/null); do terminate_tree \"$child\"; done; kill -TERM \"$1\" 2>/dev/null || true; }}; kill -TERM -{pid} 2>/dev/null || true; terminate_tree {pid}"
        );
        self.run_shell(&cmd)?;
        thread::sleep(Duration::from_millis(800));
        if pid_running(&pid_path)?.is_some() {
            let cmd = format!(
                "kill -KILL -{pid} 2>/dev/null || true; kill -KILL {pid} 2>/dev/null || true"
            );
            self.run_shell(&cmd)?;
            thread::sleep(Duration::from_millis(300));
        }
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
        println!("napcat: macOS 版目前仅提供说明入口，暂不管理该协议端");
        Ok(())
    }

    pub(crate) fn print_llbot_status(&self) -> Result<()> {
        println!("llbot: macOS 版目前仅提供说明入口，暂不管理该协议端");
        Ok(())
    }

    pub(crate) fn print_napcat_logs(&self, _tail: usize, _follow: bool) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn print_llbot_logs(&self, _tail: usize, _follow: bool) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn start_napcat(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn stop_napcat(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn restart_napcat(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn rebuild_napcat(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn remove_napcat_container(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn exec_napcat_shell(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn start_llbot(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn stop_llbot(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn restart_llbot(&self) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn set_llbot_password(&self, _password: &str) -> Result<()> {
        macos_protocol_note()
    }

    pub(crate) fn manage_bot_protocol_menu(&self) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.print_section("协议端服务", "macOS 当前只启用 MaiBot 核心管理");
        let cards = [
            StatusCard::warning(
                "NapCatQQ",
                "说明",
                "当前平台仅提供说明入口，不显示不可执行的启停操作",
            ),
            StatusCard::warning(
                "LuckyLilliaBot",
                "说明",
                "当前平台仅提供说明入口，不显示不可执行的启停操作",
            ),
        ];
        self.print_status_cards("平台能力", &cards);
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
            self.print_section("MaiBot 核心", "后台子进程运行，日志写入 logs/maibot.log");
            self.print_kv("目录", &maibot_dir.display().to_string());
            let pid = pid_running(&pid_path)?;
            let cards = [if let Some(pid) = pid {
                StatusCard::running(
                    "MaiBot",
                    format!("后台子进程 PID {pid} · 退出管理器后继续运行"),
                )
            } else {
                StatusCard::stopped("MaiBot", "后台子进程未运行")
            }];
            self.print_status_cards("核心状态", &cards);
            let actions = [
                ActionItem::primary("启动 MaiBot", "选择后台模式或首次启动/EULA 交互终端"),
                ActionItem::destructive("停止 MaiBot", "结束后台进程组"),
                ActionItem::normal("查看实时日志", "跟随 logs/maibot.log"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择核心操作", &actions)?;
            let result = match choice {
                0 => {
                    let modes = [
                        ActionItem::primary("后台启动", "适合已完成 EULA，退出管理器后继续运行"),
                        ActionItem::normal(
                            "打开交互终端",
                            "首次启动/EULA，在 Terminal.app 中输入确认",
                        ),
                    ];
                    let mode = self.select_action_timeout(
                        "选择启动方式",
                        &modes,
                        0,
                        Duration::from_secs(10),
                    )?;
                    self.start_maibot_core(mode == 1)
                }
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
        macos_protocol_note()
    }

    pub(crate) fn manage_llbot_menu(&self) -> Result<()> {
        macos_protocol_note()
    }
}

fn macos_protocol_note() -> Result<()> {
    bail!("macOS 版目前只管理 MaiBot 核心与插件，协议端服务会在后续版本提供")
}

fn maibot_runtime_paths(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let logs_dir = root.join("logs");
    let pid_path = logs_dir.join(MAIBOT_PID_FILE);
    let log_path = logs_dir.join(MAIBOT_LOG_FILE);
    (logs_dir, pid_path, log_path)
}

fn maibot_run_command(
    root: &Path,
    venv_activate: &Path,
    py_env: &str,
    pip_index: &str,
    pip_host: &str,
) -> String {
    let pypi_env = pypi_runtime_env(pip_index, pip_host);
    if py_env == "uv" {
        format!("{} {pypi_env}uv run bot.py", macos_tools_prelude(root))
    } else {
        format!(
            "{} {pypi_env}. '{}' && python3 bot.py",
            macos_tools_prelude(root),
            shell_escape(venv_activate)
        )
    }
}

/// 运行时的源优先级高于项目配置；UV_NO_CONFIG 会屏蔽 pyproject.toml/uv.toml 的索引。
fn pypi_runtime_env(pip_index: &str, pip_host: &str) -> String {
    if pip_index.trim().is_empty() {
        return String::new();
    }

    let index = shell_escape_raw(pip_index);
    let trusted_host = if pip_host.trim().is_empty() {
        String::new()
    } else {
        format!(" PIP_TRUSTED_HOST='{}'", shell_escape_raw(pip_host))
    };
    format!(
        "export PIP_INDEX_URL='{index}' UV_DEFAULT_INDEX='{index}' UV_INDEX_URL='{index}' UV_NO_CONFIG=1{trusted_host}; "
    )
}

fn applescript_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
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

#[cfg(test)]
mod tests {
    use super::pypi_runtime_env;

    #[test]
    fn runtime_pypi_environment_overrides_project_configuration() {
        let env = pypi_runtime_env("https://pypi.example/simple", "pypi.example");
        assert!(env.contains("PIP_INDEX_URL='https://pypi.example/simple'"));
        assert!(env.contains("UV_DEFAULT_INDEX='https://pypi.example/simple'"));
        assert!(env.contains("UV_NO_CONFIG=1"));
        assert!(env.contains("PIP_TRUSTED_HOST='pypi.example'"));
    }

    #[test]
    fn runtime_pypi_environment_preserves_project_defaults_when_unset() {
        assert!(pypi_runtime_env("", "").is_empty());
    }
}
