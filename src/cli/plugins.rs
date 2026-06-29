use crate::app::App;
use anyhow::{Result, bail};

pub(super) fn run(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "list" => app.print_plugins(),
        "install" => {
            let input = crate::cli::require_arg(args, 1, "install <GitHub地址或username/repo>")?;
            app.install_plugin_from_input(input)
        }
        "update" => {
            let name = crate::cli::require_arg(args, 1, "update <插件目录名>")?;
            app.update_plugin(name)
        }
        "remove" | "uninstall" => {
            let name = crate::cli::require_arg(args, 1, "remove <插件目录名>")?;
            app.remove_plugin(name)
        }
        "-h" | "--help" | "help" => {
            crate::cli::print_help();
            Ok(())
        }
        other => bail!("未知 plugin 命令: {other}"),
    }
}
