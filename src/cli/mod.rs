mod access;
mod help;
mod install;
mod plugins;
mod services;

use crate::app::App;
use anyhow::{Result, bail};

pub(crate) use help::print_help;

impl App {
    pub(crate) fn run_cli(&mut self, args: &[String]) -> Result<()> {
        match args.first().map(String::as_str) {
            Some("install" | "update") => install::run(self, args),
            Some("core") => services::run_core(self, &args[1..]),
            Some("napcat") => services::run_napcat(self, &args[1..]),
            Some("llbot") => services::run_llbot(self, &args[1..]),
            Some("protocol" | "bot") => services::run_protocol(self, &args[1..]),
            Some("access" | "config") => access::run(self, &args[1..]),
            Some("plugin" | "plugins") => plugins::run(self, &args[1..]),
            Some("-h" | "--help" | "help") | None => {
                help::print_help();
                Ok(())
            }
            Some(command) => {
                help::print_help();
                bail!("未知命令: {command}");
            }
        }
    }
}

pub(crate) fn is_help_request(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("-h" | "--help" | "help")
    )
}

fn require_arg<'a>(args: &'a [String], index: usize, label: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("缺少参数: {label}"))
}

fn parse_tail(args: &[String], default_tail: usize) -> Result<(usize, bool)> {
    let mut tail = default_tail;
    let mut follow = false;
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "-f" | "--follow" => {
                follow = true;
                idx += 1;
            }
            "--tail" => {
                let value = require_arg(args, idx + 1, "--tail <行数>")?;
                tail = value.parse()?;
                idx += 2;
            }
            other => bail!("未知日志参数: {other}"),
        }
    }
    Ok((tail, follow))
}
