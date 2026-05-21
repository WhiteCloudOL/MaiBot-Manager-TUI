mod access;
mod app;
mod installer;
mod lpmm;
mod model;
mod plugins;
mod runtime;
mod services;
mod terminal;
mod ui;
mod utils;

use anyhow::{bail, Result};
use app::App;
use terminal::{install_terminal_cleanup_handler, restore_terminal_state};

fn main() -> Result<()> {
    ensure_linux()?;
    install_terminal_cleanup_handler()?;
    let mut app = App::new()?;
    let result = app.run();
    restore_terminal_state();
    result
}

fn ensure_linux() -> Result<()> {
    if std::env::consts::OS != "linux" {
        bail!("该程序面向 Linux 服务器环境，请在 Linux 中运行。");
    }
    Ok(())
}
