#![cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]

mod cli;
mod data;
mod model;
mod terminal;
mod theme;
mod ui;

#[cfg(target_os = "linux")]
#[path = "linux/access.rs"]
mod access;
#[cfg(target_os = "linux")]
#[path = "linux/app.rs"]
mod app;
#[cfg(target_os = "linux")]
#[path = "linux/installer.rs"]
mod installer;
#[cfg(target_os = "linux")]
#[path = "linux/plugins.rs"]
mod plugins;
#[cfg(target_os = "linux")]
#[path = "linux/runtime.rs"]
mod runtime;
#[cfg(target_os = "linux")]
#[path = "linux/services.rs"]
mod services;
#[cfg(target_os = "linux")]
#[path = "linux/utils.rs"]
mod utils;

#[cfg(target_os = "windows")]
#[path = "win/access.rs"]
mod access;
#[cfg(target_os = "windows")]
#[path = "win/app.rs"]
mod app;
#[cfg(target_os = "windows")]
#[path = "win/installer.rs"]
mod installer;
#[cfg(target_os = "windows")]
#[path = "win/plugins.rs"]
mod plugins;
#[cfg(target_os = "windows")]
#[path = "win/runtime.rs"]
mod runtime;
#[cfg(target_os = "windows")]
#[path = "win/services.rs"]
mod services;
#[cfg(target_os = "windows")]
#[path = "win/utils.rs"]
mod utils;

#[cfg(target_os = "macos")]
#[path = "macos/access.rs"]
mod access;
#[cfg(target_os = "macos")]
#[path = "macos/app.rs"]
mod app;
#[cfg(target_os = "macos")]
#[path = "macos/installer.rs"]
mod installer;
#[cfg(target_os = "macos")]
#[path = "macos/plugins.rs"]
mod plugins;
#[cfg(target_os = "macos")]
#[path = "macos/runtime.rs"]
mod runtime;
#[cfg(target_os = "macos")]
#[path = "macos/services.rs"]
mod services;
#[cfg(target_os = "macos")]
#[path = "macos/utils.rs"]
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

    ensure_supported_platform()?;
    let use_tui = args.is_empty() || args.first().is_some_and(|arg| arg == "tui");
    if use_tui {
        install_terminal_cleanup_handler()?;
    }
    let mut app = App::new()?;
    let result = if use_tui {
        app.run()
    } else {
        app.set_cli_mode();
        app.run_cli(&args)
    };
    if use_tui {
        restore_terminal_state();
    }
    result
}

fn ensure_supported_platform() -> Result<()> {
    if !matches!(std::env::consts::OS, "linux" | "windows" | "macos") {
        bail!("该程序仅支持 Linux、Windows 10/11 与 macOS 环境。");
    }
    Ok(())
}
