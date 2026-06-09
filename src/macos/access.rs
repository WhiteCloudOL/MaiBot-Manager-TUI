use crate::{app::App, model::DashboardPopup, ui::ActionItem};
use anyhow::{Result, bail};
use dialoguer::Confirm;
use dialoguer::console::style;
use serde_json::Value;
use std::{fs, path::PathBuf};
use toml_edit::{DocumentMut, Item, value};

#[derive(Debug)]
struct AccessInfoReport {
    subtitle: &'static str,
    ip_label: &'static str,
    public_ip: String,
    endpoints: Vec<AccessEndpoint>,
}

#[derive(Debug)]
struct AccessEndpoint {
    title: &'static str,
    fields: Vec<(&'static str, String)>,
}

impl AccessInfoReport {
    fn popup_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("{} {}", self.ip_label, self.public_ip)];
        if self.endpoints.is_empty() {
            lines.push(String::new());
            lines.push("未找到 MaiBot WebUI 配置。".to_string());
            lines.push("请先完成安装，或启动 MaiBot 生成访问密钥。".to_string());
            return lines;
        }

        for endpoint in &self.endpoints {
            lines.push(String::new());
            lines.push(endpoint.title.to_string());
            for (label, value) in &endpoint.fields {
                lines.push(format!("{label} {value}"));
            }
        }
        lines
    }
}

impl App {
    pub(crate) fn manage_config_access_menu(&self) -> Result<()> {
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("配置与访问", "维护 MaiBot WebUI 入口和访问密钥");
            let actions = [
                ActionItem::primary("查看访问信息", "显示 WebUI 地址和 token"),
                ActionItem::normal("初始化访问配置", "将 WebUI 绑定到 0.0.0.0"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择访问操作", &actions)?;
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

    pub(crate) fn dashboard_access_summary_popup(&self) -> DashboardPopup {
        match self.access_info_report() {
            Ok(report) => DashboardPopup {
                title: "访问汇总".to_string(),
                subtitle: report.subtitle.to_string(),
                lines: report.popup_lines(),
                actions: vec!["取消".to_string()],
                selected: 0,
            },
            Err(error) => DashboardPopup {
                title: "访问汇总".to_string(),
                subtitle: "暂时无法读取访问入口".to_string(),
                lines: vec![
                    format!("无法读取访问配置: {error}"),
                    "请先完成部署，并确认安装目录仍可访问。".to_string(),
                ],
                actions: vec!["取消".to_string()],
                selected: 0,
            },
        }
    }

    pub(crate) fn print_access_info(&self) -> Result<()> {
        let report = self.access_info_report()?;
        self.print_section("访问汇总", report.subtitle);
        self.print_kv(report.ip_label, &report.public_ip);
        for endpoint in &report.endpoints {
            self.print_line();
            println!("{}", style(endpoint.title).cyan().bold());
            for (label, value) in &endpoint.fields {
                self.print_kv(label, value);
            }
        }
        if report.endpoints.is_empty() {
            self.print_hint("未找到 MaiBot/config/bot_config.toml，请先完成安装。");
        }
        Ok(())
    }

    fn access_info_report(&self) -> Result<AccessInfoReport> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let public_ip = self.get_public_ip().unwrap_or_else(|_| "127.0.0.1".into());
        let mut endpoints = Vec::new();
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
                public_ip.clone()
            };
            endpoints.push(AccessEndpoint {
                title: "MaiBot WebUI",
                fields: vec![
                    ("地址", format!("http://{host_display}:{port}")),
                    ("密钥", token),
                ],
            });
        }
        Ok(AccessInfoReport {
            subtitle: "集中查看 MaiBot WebUI 访问入口",
            ip_label: "本机 / 公网 IP",
            public_ip,
            endpoints,
        })
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
    bail!("macOS 版目前只开放 MaiBot WebUI 访问配置，Adapter 策略会随协议端能力一起提供")
}
