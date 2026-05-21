use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::io::{self, Write};

pub fn install_terminal_cleanup_handler() -> Result<()> {
    ctrlc::set_handler(|| {
        restore_terminal_state();
        eprintln!("\n操作已被用户中断 (Ctrl+C)");
        std::process::exit(130);
    })
    .context("注册 Ctrl+C 清理处理失败")?;
    Ok(())
}

pub fn restore_terminal_state() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, Show);
    let _ = write!(stdout, "\x1B[0m\x1B[?25h\r\n");
    let _ = stdout.flush();
}

pub struct TerminalUiGuard;

impl TerminalUiGuard {
    pub fn enter() -> Result<Self> {
        enable_raw_mode().context("启用终端 raw mode 失败")?;
        let mut stdout = io::stdout();
        execute!(stdout, Hide).context("隐藏终端光标失败")?;
        Ok(Self)
    }
}

impl Drop for TerminalUiGuard {
    fn drop(&mut self) {
        restore_terminal_state();
    }
}
