use crate::{
    access_config::{apply_access_configs, webui_binding},
    app::App,
    data,
    model::DashboardPopup,
    ui::ActionItem,
};
use anyhow::{Result, bail};
use dialoguer::Confirm;
use dialoguer::console::style;
use serde_json::Value;
use std::{fs, path::PathBuf};
use toml_edit::DocumentMut;

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
                ActionItem::normal("初始化访问配置", "绑定 IPv4/IPv6 全地址"),
                ActionItem::destructive("清空数据文件", "清理所选 Bot 的 data，保留宿主登录信息"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择访问操作", &actions)?;
            let result = match choice {
                0 => self.show_access_info(),
                1 => self.initialize_maibot_access_config(),
                2 => self.confirm_clear_maibot_data_files(),
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
                ..DashboardPopup::default()
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
                ..DashboardPopup::default()
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
            self.print_hint("未找到 MaiBot/host/config.toml，请先完成安装。");
        }
        Ok(())
    }

    fn access_info_report(&self) -> Result<AccessInfoReport> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let mut public_ip = None;
        let mut endpoints = Vec::new();
        let host_cfg = root.join("MaiBot/host/config.toml");
        let webui_json = root.join("MaiBot/host/webui.json");
        if host_cfg.exists() {
            let doc = fs::read_to_string(&host_cfg)?;
            let parsed: DocumentMut = doc.parse()?;
            let binding = webui_binding(&parsed)?;
            let token = if webui_json.exists() {
                let data: Value = serde_json::from_str(&fs::read_to_string(webui_json)?)?;
                data["access_token"]
                    .as_str()
                    .unwrap_or("(未生成)")
                    .to_string()
            } else {
                "(未生成，请先启动 MaiBot)".into()
            };
            let address = binding.url(|| cached_public_ip(self, &mut public_ip));
            endpoints.push(AccessEndpoint {
                title: "MaiBot WebUI",
                fields: vec![
                    ("地址", address),
                    ("密钥", token),
                    (
                        "状态",
                        if binding.enabled {
                            "已启用"
                        } else {
                            "已禁用"
                        }
                        .into(),
                    ),
                ],
            });
        }
        Ok(AccessInfoReport {
            subtitle: "集中查看 MaiBot WebUI 访问入口",
            ip_label: "本机 / 公网 IP",
            public_ip: public_ip.unwrap_or_else(|| "未读取（当前没有外部地址）".to_string()),
            endpoints,
        })
    }

    pub(crate) fn initialize_maibot_access_config(&self) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.print_section(
            "初始化访问配置",
            "将 MaiBot WebUI 绑定到所有 IPv4/IPv6 地址",
        );
        self.print_hint(
            "注意：监听 0.0.0.0 和 :: 会让 WebUI 暴露在外部网络，请确认已设置访问令牌或防火墙规则。",
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
        let host = PathBuf::from(cfg.mai_path).join("MaiBot/host/config.toml");
        apply_access_configs(&host, None)
    }

    pub(crate) fn confirm_clear_maibot_data_files(&self) -> Result<()> {
        let data_dir = data::maibot_data_dir(&self.bot_dir()?);
        self.clear();
        self.print_header(None);
        self.print_section("清空数据文件", "清理所选 Bot 的 data，保留宿主登录信息");
        self.print_kv("目标目录", &data_dir.display().to_string());
        self.print_hint("此操作会删除知识库缓存、运行数据和子目录，无法由管理器自动恢复。");
        self.print_line();
        if !Confirm::with_theme(&self.theme)
            .with_prompt("确认清空所选 Bot 的 data（保留 webui.json）？")
            .default(false)
            .interact()?
        {
            return Ok(());
        }
        let removed = self.clear_maibot_data_files()?;
        self.pause(&format!("已清理 {removed} 个条目，按回车返回"))?;
        Ok(())
    }

    pub(crate) fn clear_maibot_data_files(&self) -> Result<usize> {
        data::clear_maibot_data_dir(&data::maibot_data_dir(&self.bot_dir()?))
    }

    pub(crate) fn print_adapter_config(&self) -> Result<()> {
        macos_adapter_todo()
    }

    pub(crate) fn set_adapter_list_mode(
        &self,
        _key: &str,
        _mode: &str,
        _rule: Option<usize>,
    ) -> Result<()> {
        macos_adapter_todo()
    }

    pub(crate) fn update_adapter_numeric_list(
        &self,
        _key: &str,
        _input: &str,
        _add: bool,
        _rule: Option<usize>,
    ) -> Result<()> {
        macos_adapter_todo()
    }

    pub(crate) fn modify_adapter_config(&self) -> Result<()> {
        macos_adapter_todo()
    }
}

fn cached_public_ip(app: &App, cached: &mut Option<String>) -> String {
    cached
        .get_or_insert_with(|| app.get_public_ip().unwrap_or_else(|_| "127.0.0.1".into()))
        .clone()
}

fn macos_adapter_todo() -> Result<()> {
    bail!("macOS 版目前只开放 MaiBot WebUI 访问配置，Adapter 策略会随协议端能力一起提供")
}
