mod access;
mod app;
mod cli;
mod installer;
mod model;
mod plugins;
mod runtime;
mod services;
mod terminal;
mod theme;
mod ui;
mod utils;

use anyhow::{Result, bail};
use app::App;
use terminal::{install_terminal_cleanup_handler, restore_terminal_state};

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if cli::is_help_request(&args) {
        cli::print_help();
        return Ok(());
    }

    ensure_linux()?;
    install_terminal_cleanup_handler()?;
    let mut app = App::new()?;
    let result = if args.is_empty() || args.first().is_some_and(|arg| arg == "tui") {
        app.run()
    } else {
        app.set_cli_mode();
        app.run_cli(&args)
    };
    restore_terminal_state();
    result
}

fn ensure_linux() -> Result<()> {
    if std::env::consts::OS != "linux" {
        bail!("该程序面向 Linux 服务器环境，请在 Linux 中运行。");
    }
    Ok(())
}
