use crate::{
    app::App, model::DashboardPopup, plugins::NAPCAT_ADAPTER_PLUGIN_ID, theme::AppTheme,
    ui::ActionItem,
};
use anyhow::{Result, anyhow, bail};
use dialoguer::{Confirm, Input, Select};
use regex::Regex;
use serde_json::Value;
use std::{fs, path::PathBuf};
use toml_edit::{DocumentMut, Item, Value as TomlValue, value};

const CHAT_TABLE: &str = "chat";

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
    fn napcat_adapter_dir(&self) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        let plugins_dir = PathBuf::from(cfg.mai_path).join("MaiBot").join("plugins");
        self.require_plugin_dir_by_id(&plugins_dir, NAPCAT_ADAPTER_PLUGIN_ID)
    }

    fn adapter_config_path(&self) -> Result<PathBuf> {
        let path = self.napcat_adapter_dir()?.join("config.toml");
        if !path.exists() {
            bail!("未找到 Adapter 配置文件: {}", path.display());
        }
        Ok(path)
    }

    pub(crate) fn manage_config_access_menu(&self) -> Result<()> {
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("配置与访问", "集中维护 WebUI 入口、密钥和 Adapter 策略");
            let actions = [
                ActionItem::primary("查看访问信息", "汇总 MaiBot / NapCat / LLBot WebUI"),
                ActionItem::normal("初始化访问配置", "绑定 0.0.0.0 并启用 Adapter"),
                ActionItem::normal("黑白名单策略", "维护群聊、私聊和黑名单规则"),
                ActionItem::back("返回", "回到主菜单"),
            ];
            let choice = self.select_action("选择访问操作", &actions)?;
            let result = match choice {
                0 => self.show_access_info(),
                1 => self.initialize_maibot_access_config(),
                2 => self.modify_adapter_config(),
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
            if let Some((_, value)) = endpoint.fields.first() {
                self.print_kv(endpoint.title, value);
                for (label, value) in endpoint.fields.iter().skip(1) {
                    self.print_kv(label, value);
                }
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
        let public_ip = self.get_public_ip().unwrap_or_else(|_| "127.0.0.1".into());
        let mut endpoints = Vec::new();

        let bot_cfg = root.join("MaiBot").join("config").join("bot_config.toml");
        let webui_json = root.join("MaiBot").join("data").join("webui.json");
        if bot_cfg.exists() {
            let parsed: DocumentMut = fs::read_to_string(&bot_cfg)?.parse()?;
            let host = parsed["webui"]["host"].as_str().unwrap_or("0.0.0.0");
            let port = parsed["webui"]["port"].as_integer().unwrap_or(8001);
            let token = if webui_json.exists() {
                let data: Value = serde_json::from_str(&fs::read_to_string(webui_json)?)?;
                data["access_token"]
                    .as_str()
                    .unwrap_or("(未生成)")
                    .to_string()
            } else {
                "(未生成，请先启动 MaiBot)".into()
            };
            let display_host = if host == "127.0.0.1" || host == "localhost" {
                host.to_string()
            } else {
                public_ip.clone()
            };
            endpoints.push(AccessEndpoint {
                title: "MaiBot WebUI",
                fields: vec![
                    ("地址", format!("http://{display_host}:{port}")),
                    ("密钥", token),
                ],
            });
        }

        let napcat_cfg = root.join("NapCat").join("config").join("webui.json");
        if napcat_cfg.exists() {
            let data: Value = serde_json::from_str(&fs::read_to_string(napcat_cfg)?)?;
            endpoints.push(AccessEndpoint {
                title: "NapCat WebUI",
                fields: vec![
                    (
                        "地址",
                        format!(
                            "http://{}:{}",
                            public_ip,
                            data["port"].as_i64().unwrap_or(6099)
                        ),
                    ),
                    (
                        "密钥",
                        data["token"].as_str().unwrap_or("(未设置)").to_string(),
                    ),
                ],
            });
        }

        let llbot_settings = root.join("LLBot").join("app_settings.json");
        if llbot_settings.exists() {
            endpoints.push(AccessEndpoint {
                title: "LuckyLilliaBot Desktop",
                fields: vec![("目录", root.join("LLBot").display().to_string())],
            });
        }
        Ok(AccessInfoReport {
            subtitle: "集中查看 MaiBot、NapCat 与 LLBot 的访问入口",
            ip_label: "本机 / 公网 IP",
            public_ip,
            endpoints,
        })
    }

    pub(crate) fn print_adapter_config(&self) -> Result<()> {
        let path = self.adapter_config_path()?;
        let doc: DocumentMut = fs::read_to_string(path)?.parse()?;
        self.print_kv(
            "群聊模式",
            config_string(&doc, CHAT_TABLE, "group_list_type", "Unknown"),
        );
        self.print_kv(
            "群聊列表",
            &config_array_display(&doc, CHAT_TABLE, "group_list"),
        );
        self.print_kv(
            "私聊模式",
            config_string(&doc, CHAT_TABLE, "private_list_type", "Unknown"),
        );
        self.print_kv(
            "私聊列表",
            &config_array_display(&doc, CHAT_TABLE, "private_list"),
        );
        self.print_kv(
            "封禁 QQ",
            &config_array_display(&doc, CHAT_TABLE, "ban_user_id"),
        );
        Ok(())
    }

    pub(crate) fn set_adapter_list_mode(&self, key: &str, mode: &str) -> Result<()> {
        if !matches!(mode, "whitelist" | "blacklist") {
            bail!("名单模式只能是 whitelist 或 blacklist");
        }
        let path = self.adapter_config_path()?;
        let mut doc: DocumentMut = fs::read_to_string(&path)?.parse()?;
        set_table_value(&mut doc, CHAT_TABLE, key, value(mode));
        fs::write(path, doc.to_string())?;
        Ok(())
    }

    pub(crate) fn update_adapter_numeric_list(
        &self,
        key: &str,
        input: &str,
        add: bool,
    ) -> Result<()> {
        if !Regex::new(r"^\d+$")?.is_match(input) {
            bail!("号码必须为纯数字");
        }
        let path = self.adapter_config_path()?;
        let mut doc: DocumentMut = fs::read_to_string(&path)?.parse()?;
        update_numeric_array(&mut doc, CHAT_TABLE, key, input, add)?;
        fs::write(path, doc.to_string())?;
        Ok(())
    }

    pub(crate) fn initialize_maibot_access_config(&self) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.print_section(
            "初始化访问配置",
            "将 MaiBot WebUI 绑定到 0.0.0.0 并启用 Napcat Adapter",
        );
        self.print_hint("注意：监听 0.0.0.0 会让 WebUI 暴露在外部网络，请确认访问令牌和防火墙。");
        if Confirm::with_theme(&self.theme)
            .with_prompt("确认应用以上修改？")
            .default(false)
            .interact()?
        {
            self.apply_maibot_access_config()?;
            self.pause("初始化完成，请重启 MaiBot 后生效；按回车返回")?;
        }
        Ok(())
    }

    pub(crate) fn apply_maibot_access_config(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let bot_cfg = root.join("MaiBot").join("config").join("bot_config.toml");
        if bot_cfg.exists() {
            let mut doc: DocumentMut = fs::read_to_string(&bot_cfg)?.parse()?;
            if doc["webui"].is_none() {
                doc["webui"] = Item::Table(Default::default());
            }
            doc["webui"]["host"] = value("0.0.0.0");
            fs::write(&bot_cfg, doc.to_string())?;
        }
        let adapter_cfg = self.napcat_adapter_dir()?.join("config.toml");
        if adapter_cfg.exists() {
            let mut doc: DocumentMut = fs::read_to_string(&adapter_cfg)?.parse()?;
            if doc["plugin"].is_none() {
                doc["plugin"] = Item::Table(Default::default());
            }
            doc["plugin"]["enabled"] = value(true);
            fs::write(adapter_cfg, doc.to_string())?;
        }
        Ok(())
    }

    pub(crate) fn modify_adapter_config(&self) -> Result<()> {
        let path = self.adapter_config_path()?;
        loop {
            let mut doc: DocumentMut = fs::read_to_string(&path)?.parse()?;
            self.clear();
            self.print_header(None);
            self.print_section("Adapter 黑白名单", "查看并修改群聊、私聊和黑名单规则");
            self.print_kv(
                "群聊模式",
                config_string(&doc, CHAT_TABLE, "group_list_type", "Unknown"),
            );
            self.print_kv(
                "群聊列表",
                &config_array_display(&doc, CHAT_TABLE, "group_list"),
            );
            self.print_kv(
                "私聊模式",
                config_string(&doc, CHAT_TABLE, "private_list_type", "Unknown"),
            );
            self.print_kv(
                "私聊列表",
                &config_array_display(&doc, CHAT_TABLE, "private_list"),
            );
            self.print_kv(
                "封禁 QQ",
                &config_array_display(&doc, CHAT_TABLE, "ban_user_id"),
            );
            let choice = Select::with_theme(&self.theme)
                .with_prompt("Adapter 黑白名单管理")
                .items([
                    "切换群聊名单类型",
                    "添加群号到群聊列表",
                    "从群聊列表移除群号",
                    "切换私聊名单类型",
                    "添加 QQ 到私聊列表",
                    "从私聊列表移除 QQ",
                    "添加 QQ 到黑名单",
                    "从黑名单移除 QQ",
                    "返回",
                ])
                .default(0)
                .interact()?;
            let result = match choice {
                0 => {
                    toggle_string(
                        &mut doc,
                        CHAT_TABLE,
                        "group_list_type",
                        "whitelist",
                        "blacklist",
                    );
                    Ok(())
                }
                1 => prompt_modify_numeric_array(
                    &mut doc,
                    CHAT_TABLE,
                    "group_list",
                    true,
                    &self.theme,
                ),
                2 => prompt_modify_numeric_array(
                    &mut doc,
                    CHAT_TABLE,
                    "group_list",
                    false,
                    &self.theme,
                ),
                3 => {
                    toggle_string(
                        &mut doc,
                        CHAT_TABLE,
                        "private_list_type",
                        "whitelist",
                        "blacklist",
                    );
                    Ok(())
                }
                4 => prompt_modify_numeric_array(
                    &mut doc,
                    CHAT_TABLE,
                    "private_list",
                    true,
                    &self.theme,
                ),
                5 => prompt_modify_numeric_array(
                    &mut doc,
                    CHAT_TABLE,
                    "private_list",
                    false,
                    &self.theme,
                ),
                6 => prompt_modify_numeric_array(
                    &mut doc,
                    CHAT_TABLE,
                    "ban_user_id",
                    true,
                    &self.theme,
                ),
                7 => prompt_modify_numeric_array(
                    &mut doc,
                    CHAT_TABLE,
                    "ban_user_id",
                    false,
                    &self.theme,
                ),
                _ => break,
            };
            if self.handle_menu_result(
                result.and_then(|_| fs::write(&path, doc.to_string()).map_err(Into::into)),
            )? {
                self.pause("操作已执行，按回车继续")?;
            }
        }
        Ok(())
    }
}

fn display_array(arr: &toml_edit::Array) -> String {
    arr.iter()
        .filter_map(|v| v.as_integer())
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn config_string<'a>(doc: &'a DocumentMut, table: &str, key: &str, default: &'a str) -> &'a str {
    doc.get(table)
        .and_then(Item::as_table)
        .and_then(|table| table.get(key))
        .and_then(Item::as_str)
        .unwrap_or(default)
}

fn config_array_display(doc: &DocumentMut, table: &str, key: &str) -> String {
    doc.get(table)
        .and_then(Item::as_table)
        .and_then(|table| table.get(key))
        .and_then(Item::as_array)
        .map(display_array)
        .unwrap_or_default()
}

fn ensure_table(doc: &mut DocumentMut, table: &str) {
    if doc.get(table).and_then(Item::as_table).is_none() {
        doc[table] = Item::Table(Default::default());
    }
}

fn set_table_value(doc: &mut DocumentMut, table: &str, key: &str, new_value: Item) {
    ensure_table(doc, table);
    doc[table][key] = new_value;
}

fn toggle_string(doc: &mut DocumentMut, table: &str, key: &str, left: &str, right: &str) {
    let current = config_string(doc, table, key, left);
    set_table_value(
        doc,
        table,
        key,
        value(if current == left { right } else { left }),
    );
}

fn prompt_modify_numeric_array(
    doc: &mut DocumentMut,
    table: &str,
    key: &str,
    add: bool,
    theme: &AppTheme,
) -> Result<()> {
    let input: String = Input::with_theme(theme)
        .with_prompt(if add {
            "输入号码"
        } else {
            "输入要移除的号码"
        })
        .interact_text()?;
    if !Regex::new(r"^\d+$")?.is_match(&input) {
        bail!("号码必须为纯数字");
    }
    update_numeric_array(doc, table, key, &input, add)
}

fn update_numeric_array(
    doc: &mut DocumentMut,
    table: &str,
    key: &str,
    input: &str,
    add: bool,
) -> Result<()> {
    ensure_table(doc, table);
    if doc[table].get(key).map(Item::is_none).unwrap_or(true) {
        doc[table][key] = Item::Value(TomlValue::Array(Default::default()));
    }
    let arr = doc[table][key]
        .as_array_mut()
        .ok_or_else(|| anyhow!("{}.{} 不是数组", table, key))?;
    let values = arr
        .iter()
        .filter_map(|v| v.as_integer())
        .map(|v| v.to_string())
        .collect::<Vec<_>>();
    if add {
        if !values.iter().any(|value| value == input) {
            arr.push(input.parse::<i64>()?);
        }
    } else if let Some(pos) = values.iter().position(|v| v == input) {
        arr.remove(pos);
    }
    Ok(())
}
