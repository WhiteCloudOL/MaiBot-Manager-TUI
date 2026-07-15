use crate::{app::App, cli::parse_tail};
use anyhow::{Result, bail};

pub(super) fn run_core(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "start" => app.start_maibot_core(args.iter().any(|arg| arg == "--exec"))?,
        "stop" => app.stop_maibot_core()?,
        "restart" => app.restart_maibot_core()?,
        "status" => app.print_maibot_core_status()?,
        "logs" => {
            let (tail, follow) = parse_tail(&args[1..], 100)?;
            app.print_maibot_core_logs(tail, follow)?;
        }
        "exec" => app.attach_screen("maibot")?,
        "-h" | "--help" | "help" => crate::cli::print_help(),
        other => bail!("未知 core 命令: {other}"),
    }
    Ok(())
}

pub(super) fn run_napcat(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "start" => app.start_napcat()?,
        "stop" => app.stop_napcat()?,
        "restart" => app.restart_napcat()?,
        "status" => app.print_napcat_status()?,
        "logs" => {
            let (tail, follow) = parse_tail(&args[1..], 100)?;
            app.print_napcat_logs(tail, follow)?;
        }
        "rebuild" => app.rebuild_napcat()?,
        "remove-container" => app.remove_napcat_container()?,
        "exec" => app.exec_napcat_shell()?,
        "-h" | "--help" | "help" => crate::cli::print_help(),
        other => bail!("未知 napcat 命令: {other}"),
    }
    Ok(())
}

pub(super) fn run_llbot(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "start" => app.start_llbot()?,
        "stop" => app.stop_llbot()?,
        "restart" => app.restart_llbot()?,
        "status" => app.print_llbot_status()?,
        "logs" => {
            let (tail, follow) = parse_tail(&args[1..], 100)?;
            app.print_llbot_logs(tail, follow)?;
        }
        "exec" => app.attach_screen("llbot")?,
        "password" => {
            let password = crate::cli::require_arg(args, 1, "password <新密码>")?;
            app.set_llbot_password(password)?;
        }
        "-h" | "--help" | "help" => crate::cli::print_help(),
        other => bail!("未知 llbot 命令: {other}"),
    }
    Ok(())
}

pub(super) fn run_snowluma(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "start" => app.start_snowluma()?,
        "stop" => app.stop_snowluma()?,
        "restart" => app.restart_snowluma()?,
        "status" => app.print_snowluma_status()?,
        "logs" => {
            let (tail, follow) = parse_tail(&args[1..], 100)?;
            app.print_snowluma_logs(tail, follow)?;
        }
        "rebuild" => app.rebuild_snowluma()?,
        "recreate-data" => app.recreate_snowluma_data()?,
        "remove-container" => app.remove_snowluma_container()?,
        "exec" => app.exec_snowluma_shell()?,
        "-h" | "--help" | "help" => crate::cli::print_help(),
        other => bail!("未知 snowluma 命令: {other}"),
    }
    Ok(())
}

pub(super) fn run_protocol(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "napcat" => run_napcat(app, &args[1..]),
        "llbot" => run_llbot(app, &args[1..]),
        "snowluma" => run_snowluma(app, &args[1..]),
        "-h" | "--help" | "help" => {
            crate::cli::print_help();
            Ok(())
        }
        other => bail!("未知协议端: {other}"),
    }
}
