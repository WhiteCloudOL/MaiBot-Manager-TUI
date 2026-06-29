use crate::app::App;
use anyhow::{Result, bail};
use dialoguer::Confirm;

pub(super) fn run(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "show" => app.print_access_info(),
        "init" => {
            let prompt = if cfg!(target_os = "macos") {
                "确认将 MaiBot WebUI 绑定到所有 IPv4/IPv6 地址？"
            } else {
                "确认将 MaiBot WebUI 绑定到所有 IPv4/IPv6 地址并启用 Napcat Adapter？"
            };
            let confirmed = args[1..].iter().any(|arg| arg == "--yes" || arg == "-y")
                || Confirm::with_theme(&app.theme)
                    .with_prompt(prompt)
                    .default(false)
                    .interact()?;
            if !confirmed {
                println!("已取消访问配置初始化");
                return Ok(());
            }
            app.apply_maibot_access_config()?;
            println!("初始化完成，请重启 MaiBot 后生效");
            Ok(())
        }
        "clear-data" => {
            let confirmed = args[1..].iter().any(|arg| arg == "--yes" || arg == "-y")
                || Confirm::with_theme(&app.theme)
                    .with_prompt("确认清空 MaiBot/data 中除 webui.json 外的所有内容？")
                    .default(false)
                    .interact()?;
            if !confirmed {
                println!("已取消清空数据文件");
                return Ok(());
            }
            let removed = app.clear_maibot_data_files()?;
            println!("已清理 {removed} 个数据条目，保留 webui.json");
            Ok(())
        }
        "adapter" => run_adapter(app, &args[1..]),
        "-h" | "--help" | "help" => {
            crate::cli::print_help();
            Ok(())
        }
        other => bail!("未知 access 命令: {other}"),
    }
}

fn run_adapter(app: &App, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "show" => app.print_adapter_config(),
        "group-mode" => {
            let mode = crate::cli::require_arg(args, 1, "group-mode <whitelist|blacklist>")?;
            app.set_adapter_list_mode("group_list_type", mode)
        }
        "private-mode" => {
            let mode = crate::cli::require_arg(args, 1, "private-mode <whitelist|blacklist>")?;
            app.set_adapter_list_mode("private_list_type", mode)
        }
        "group-add" => update(app, args, "group_list", true),
        "group-remove" => update(app, args, "group_list", false),
        "private-add" => update(app, args, "private_list", true),
        "private-remove" => update(app, args, "private_list", false),
        "ban-add" => update(app, args, "ban_user_id", true),
        "ban-remove" => update(app, args, "ban_user_id", false),
        "-h" | "--help" | "help" => {
            crate::cli::print_help();
            Ok(())
        }
        other => bail!("未知 adapter 命令: {other}"),
    }
}

fn update(app: &App, args: &[String], key: &str, add: bool) -> Result<()> {
    let number = crate::cli::require_arg(args, 1, "<号码>")?;
    app.update_adapter_numeric_list(key, number, add)
}
