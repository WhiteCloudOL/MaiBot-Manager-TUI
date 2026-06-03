use crate::{app::App, cli::parse_tail, utils::screen_exists};
use anyhow::{Result, bail};
use std::process::Command;

pub(super) fn run_core(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "start" => app.start_maibot_core(args.iter().any(|arg| arg == "--exec"))?,
        "stop" => app.stop_maibot_core()?,
        "restart" => app.restart_maibot_core()?,
        "status" => print_screen_status("maibot")?,
        "logs" => {
            let (tail, follow) = parse_tail(&args[1..], 100)?;
            app.print_screen_logs("maibot", tail, follow)?;
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
        "status" => print_napcat_status()?,
        "logs" => {
            let (tail, follow) = parse_tail(&args[1..], 100)?;
            app.print_napcat_logs(tail, follow)?;
        }
        "rebuild" => app.rebuild_napcat()?,
        "remove-container" => app.remove_napcat_container()?,
        "exec" => app.run_shell("docker exec -it napcat /bin/sh")?,
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
        "status" => print_screen_status("llbot")?,
        "logs" => {
            let (tail, follow) = parse_tail(&args[1..], 100)?;
            app.print_screen_logs("llbot", tail, follow)?;
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

pub(super) fn run_protocol(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "napcat" => run_napcat(app, &args[1..]),
        "llbot" => run_llbot(app, &args[1..]),
        "-h" | "--help" | "help" => {
            crate::cli::print_help();
            Ok(())
        }
        other => bail!("未知协议端: {other}"),
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

fn print_napcat_status() -> Result<()> {
    let output = Command::new("bash")
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
