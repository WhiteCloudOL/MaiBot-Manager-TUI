use crate::{app::App, data, model::DashboardPopup, ui::ActionItem};
use anyhow::Result;
use dialoguer::Confirm;

use dialoguer::console::style;
use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};
use toml_edit::{Array, DocumentMut, Item, value};

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
            lines.push("未找到可展示的 WebUI 入口。".to_string());
            lines.push("请确认安装目录内的配置文件已经生成。".to_string());
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
            self.print_section(
                "配置与访问",
                "集中维护 WebUI 入口、密钥和已安装 Adapter 策略",
            );
            let actions = [
                ActionItem::primary("查看访问信息", "汇总 MaiBot 与各协议端 WebUI"),
                ActionItem::normal(
                    "初始化访问配置",
                    "绑定 IPv4/IPv6 全地址并启用已安装 Adapter",
                ),
                ActionItem::normal("黑白名单策略", "维护群聊、私聊和黑名单规则"),
                ActionItem::destructive(
                    "清空数据文件",
                    "保留 webui.json，清理 MaiBot/data 其余内容",
                ),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择访问操作", &actions)?;
            let result = match choice {
                0 => self.show_access_info(),
                1 => self.initialize_maibot_access_config(),
                2 => self.modify_adapter_config(),
                3 => self.confirm_clear_maibot_data_files(),
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
                scroll: 0,
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
                scroll: 0,
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
            self.print_hint("未找到可展示的 WebUI 入口，请确认配置文件已经生成。");
        }
        Ok(())
    }

    fn access_info_report(&self) -> Result<AccessInfoReport> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let mut public_ip = None;
        let mut endpoints = Vec::new();
        let bot_cfg = root.join("MaiBot/config/bot_config.toml");
        let webui_json = root.join("MaiBot/data/webui.json");
        if bot_cfg.exists() {
            let doc = fs::read_to_string(&bot_cfg)?;
            let parsed: DocumentMut = doc.parse()?;
            let host = webui_host_display(&parsed);
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
                cached_public_ip(self, &mut public_ip)
            };
            endpoints.push(AccessEndpoint {
                title: "MaiBot WebUI",
                fields: vec![
                    ("地址", format!("http://{host_display}:{port}")),
                    ("密钥", token),
                ],
            });
        }
        let napcat_cfg = root.join("NapCat/config/webui.json");
        if napcat_cfg.exists() {
            let data: Value = serde_json::from_str(&fs::read_to_string(napcat_cfg)?)?;
            endpoints.push(AccessEndpoint {
                title: "NapCat WebUI",
                fields: vec![
                    (
                        "地址",
                        format!(
                            "http://{}:{}",
                            cached_public_ip(self, &mut public_ip),
                            data["port"].as_i64().unwrap_or(0)
                        ),
                    ),
                    (
                        "密钥",
                        data["token"].as_str().unwrap_or("(未设置)").to_string(),
                    ),
                ],
            });
        }
        let llbot_cfg = root.join("LLBot/bin/llbot/default_config.json");
        if llbot_cfg.exists() {
            let data: Value = serde_json::from_str(&fs::read_to_string(&llbot_cfg)?)?;
            let host = data["webui"]["host"].as_str().unwrap_or("0.0.0.0");
            let port = data["webui"]["port"].as_i64().unwrap_or(3080);
            let token_path = root.join("LLBot/bin/llbot/data/webui_token.txt");
            let token = fs::read_to_string(token_path)
                .unwrap_or_else(|_| "(未设置)".into())
                .trim()
                .to_string();
            let display_host = if host == "0.0.0.0" {
                cached_public_ip(self, &mut public_ip)
            } else {
                host.to_string()
            };
            endpoints.push(AccessEndpoint {
                title: "LuckyLilliaBot WebUI",
                fields: vec![
                    ("地址", format!("http://{display_host}:{port}")),
                    ("密码", token),
                ],
            });
        }
        let snowluma_dir = root.join("SnowLuma");
        if snowluma_dir.exists() {
            let host = cached_public_ip(self, &mut public_ip);
            let vnc_password = snowluma_vnc_password(&snowluma_dir)
                .unwrap_or_else(|| "未找到 .env 中的 VNC_PASSWD".to_string());
            endpoints.push(AccessEndpoint {
                title: "SnowLuma WebUI",
                fields: vec![
                    ("地址", format!("http://{host}:5099/")),
                    ("临时密码", snowluma_initial_password()),
                ],
            });
            endpoints.push(AccessEndpoint {
                title: "SnowLuma VNC",
                fields: vec![
                    ("地址", format!("http://{host}:6081/")),
                    ("密码", vnc_password),
                ],
            });
        }
        Ok(AccessInfoReport {
            subtitle: "集中查看 MaiBot、协议端与 SnowLuma 的访问入口",
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
            "将 MaiBot WebUI 绑定到所有 IPv4/IPv6 地址并启用已安装的 Adapter",
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
        let root = PathBuf::from(cfg.mai_path);
        let bot_cfg = root.join("MaiBot/config/bot_config.toml");
        if bot_cfg.exists() {
            let mut doc: DocumentMut = fs::read_to_string(&bot_cfg)?.parse()?;
            if doc["webui"].is_none() {
                doc["webui"] = Item::Table(Default::default());
            }
            doc["webui"]["host"] = webui_host_all_interfaces();
            fs::write(&bot_cfg, doc.to_string())?;
        }
        if let Some(adapter_dir) = self.qq_adapter_dir()? {
            let adapter_cfg = adapter_dir.join("config.toml");
            if adapter_cfg.exists() {
                let mut doc: DocumentMut = fs::read_to_string(&adapter_cfg)?.parse()?;
                if doc["plugin"].is_none() {
                    doc["plugin"] = Item::Table(Default::default());
                }
                doc["plugin"]["enabled"] = value(true);
                fs::write(&adapter_cfg, doc.to_string())?;
            }
        }
        Ok(())
    }

    pub(crate) fn confirm_clear_maibot_data_files(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let data_dir = data::maibot_data_dir(&cfg.mai_path);
        self.clear();
        self.print_header(None);
        self.print_section("清空数据文件", "保留 webui.json，删除 MaiBot/data 其余内容");
        self.print_kv("目标目录", &data_dir.display().to_string());
        self.print_hint("此操作会删除知识库缓存、运行数据和子目录，无法由管理器自动恢复。");
        self.print_line();
        if !Confirm::with_theme(&self.theme)
            .with_prompt("确认清空 MaiBot/data 中除 webui.json 外的所有内容？")
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
        let cfg = self.require_config()?;
        data::clear_maibot_data_dir(&data::maibot_data_dir(&cfg.mai_path))
    }
}

fn snowluma_vnc_password(dir: &std::path::Path) -> Option<String> {
    fs::read_to_string(dir.join(".env"))
        .ok()?
        .lines()
        .find_map(|line| line.trim().strip_prefix("VNC_PASSWD=").map(str::to_string))
}

fn snowluma_initial_password() -> String {
    let output = Command::new("bash")
        .arg("-lc")
        .arg("docker logs snowluma 2>&1 | sed -nE 's/.*(临时密码: |initial credentials: user=admin password=)([^[:space:]]+).*/\\2/p' | tail -n 1")
        .output();
    let password = output
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    if password.is_empty() {
        "未找到；临时密码只会在全新数据首次启动时输出，永久密码不会显示".to_string()
    } else {
        password
    }
}

fn webui_host_all_interfaces() -> Item {
    let mut host = Array::default();
    host.push("0.0.0.0");
    host.push("::");
    value(host)
}

fn webui_host_display(doc: &DocumentMut) -> String {
    let host = &doc["webui"]["host"];
    if let Some(value) = host.as_str() {
        return value.to_string();
    }
    if let Some(array) = host.as_array() {
        return array
            .iter()
            .filter_map(|value| value.as_str())
            .find(|value| *value == "0.0.0.0" || *value == "::")
            .unwrap_or("127.0.0.1")
            .to_string();
    }
    "0.0.0.0".to_string()
}

fn cached_public_ip(app: &App, cached: &mut Option<String>) -> String {
    cached
        .get_or_insert_with(|| app.get_public_ip().unwrap_or_else(|_| "127.0.0.1".into()))
        .clone()
}
