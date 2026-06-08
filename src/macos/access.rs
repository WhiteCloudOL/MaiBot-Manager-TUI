use crate::app::App;
use anyhow::{Result, bail};
use dialoguer::console::style;
use dialoguer::{Confirm, Select};
use serde_json::Value;
use std::{fs, path::PathBuf};
use toml_edit::{DocumentMut, Item, value};

impl App {
    pub(crate) fn manage_config_access_menu(&self) -> Result<()> {
        loop {
            self.clear();
            self.print_header(None);
            let choice = Select::with_theme(&self.theme)
                .with_prompt("配置与访问")
                .items(["查看 WebUI 访问信息", "初始化 MaiBot 访问配置", "返回"])
                .default(0)
                .interact()?;
            let result = match choice {
                0 => self.show_access_info(),
                1 => self.initialize_maibot_access_config(),
                _ => break,
            };
            self.handle_menu_result(result)?;
        }
        Ok(())
    }

    pub(crate) fn show_access_info(&self) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.print_access_info()?;
        self.pause("按回车返回")?;
        Ok(())
    }

    pub(crate) fn print_access_info(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let public_ip = self.get_public_ip().unwrap_or_else(|_| "127.0.0.1".into());
        self.print_section("访问汇总", "集中查看 MaiBot WebUI 访问入口");
        self.print_kv("公网 IP", &public_ip);
        let bot_cfg = root.join("MaiBot/config/bot_config.toml");
        let webui_json = root.join("MaiBot/data/webui.json");
        if bot_cfg.exists() {
            let doc = fs::read_to_string(&bot_cfg)?;
            let parsed: DocumentMut = doc.parse()?;
            let host = parsed["webui"]["host"]
                .as_str()
                .unwrap_or("0.0.0.0")
                .to_string();
            let port = parsed["webui"]["port"]
                .as_integer()
                .unwrap_or(8001)
                .to_string();
            let token = if webui_json.exists() {
                let data: Value = serde_json::from_str(&fs::read_to_string(webui_json)?)?;
                data["access_token"]
                    .as_str()
                    .unwrap_or("(未生成)")
                    .to_string()
            } else {
                "(未生成，请先启动 MaiBot)".into()
            };
            let host_display = if host == "127.0.0.1" || host == "localhost" {
                host
            } else {
                public_ip
            };
            self.print_line();
            println!("{}", style("MaiBot WebUI").cyan().bold());
            self.print_kv("地址", &format!("http://{host_display}:{port}"));
            self.print_kv("密钥", &token);
        } else {
            self.print_hint("未找到 MaiBot/config/bot_config.toml，请先完成安装。");
        }
        Ok(())
    }

    pub(crate) fn initialize_maibot_access_config(&self) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.print_section("初始化访问配置", "将 MaiBot WebUI 绑定到 0.0.0.0");
        self.print_hint(
            "注意：监听 0.0.0.0 会让 WebUI 暴露在外部网络，请确认已设置访问令牌或防火墙规则。",
        );
        self.print_line();

        if !Confirm::with_theme(&self.theme)
            .with_prompt("确认应用以上修改？")
            .default(false)
            .interact()?
        {
            return Ok(());
        }

        self.apply_maibot_access_config()?;
        self.pause("初始化完成，请重启 MaiBot 后生效；按回车返回")?;
        Ok(())
    }

    pub(crate) fn apply_maibot_access_config(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let bot_cfg = root.join("MaiBot/config/bot_config.toml");
        if bot_cfg.exists() {
            let mut doc: DocumentMut = fs::read_to_string(&bot_cfg)?.parse()?;
            if doc["webui"].is_none() {
                doc["webui"] = Item::Table(Default::default());
            }
            doc["webui"]["host"] = value("0.0.0.0");
            fs::write(&bot_cfg, doc.to_string())?;
        }
        Ok(())
    }

    pub(crate) fn print_adapter_config(&self) -> Result<()> {
        macos_adapter_todo()
    }

    pub(crate) fn set_adapter_list_mode(&self, _key: &str, _mode: &str) -> Result<()> {
        macos_adapter_todo()
    }

    pub(crate) fn update_adapter_numeric_list(
        &self,
        _key: &str,
        _input: &str,
        _add: bool,
    ) -> Result<()> {
        macos_adapter_todo()
    }

    pub(crate) fn modify_adapter_config(&self) -> Result<()> {
        macos_adapter_todo()
    }
}

fn macos_adapter_todo() -> Result<()> {
    bail!("macOS 版暂未适配 Napcat Adapter 配置，这部分随协议端一起留作 TODO")
}
