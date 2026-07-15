use crate::{app::App, data, model::DashboardPopup, ui::ActionItem};
use anyhow::{Result, anyhow, bail};
use dialoguer::{Confirm, Input, Select};

use crate::plugins::{NAPCAT_ADAPTER_PLUGIN_ID, SNOWLUMA_ADAPTER_PLUGIN_ID};
use crate::theme::AppTheme;
use dialoguer::console::style;
use regex::Regex;
use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};
use toml_edit::{Array, DocumentMut, Item, Value as TomlValue, value};

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
    fn adapter_dir(&self) -> Result<Option<PathBuf>> {
        let cfg = self.require_config()?;
        let plugins_dir = PathBuf::from(cfg.mai_path).join("MaiBot/plugins");
        let mut preferred = Vec::new();
        for protocol in cfg.bot_protocols.split(',').map(str::trim) {
            match protocol {
                "snowluma" => preferred.push(SNOWLUMA_ADAPTER_PLUGIN_ID),
                "napcat" => preferred.push(NAPCAT_ADAPTER_PLUGIN_ID),
                _ => {}
            }
        }
        for plugin_id in [SNOWLUMA_ADAPTER_PLUGIN_ID, NAPCAT_ADAPTER_PLUGIN_ID] {
            if !preferred.contains(&plugin_id) {
                preferred.push(plugin_id);
            }
        }
        for plugin_id in preferred {
            if let Some(path) = self.resolve_plugin_dir_by_id(&plugins_dir, plugin_id)? {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn adapter_config_path(&self) -> Result<PathBuf> {
        let path = self
            .adapter_dir()?
            .ok_or_else(|| anyhow!("未找到已安装的 NapCat 或 SnowLuma Adapter"))?
            .join("config.toml");
        if !path.exists() {
            bail!("未找到 Adapter 配置文件: {}", path.display());
        }
        Ok(path)
    }

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

    pub(crate) fn print_adapter_config(&self) -> Result<()> {
        let path = self.adapter_config_path()?;
        let doc: DocumentMut = fs::read_to_string(&path)?.parse()?;
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
        fs::write(&path, doc.to_string())?;
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
        fs::write(&path, doc.to_string())?;
        Ok(())
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
        if let Some(adapter_dir) = self.adapter_dir()? {
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

    pub(crate) fn modify_adapter_config(&self) -> Result<()> {
        let path = self.adapter_config_path()?;
        loop {
            let content = fs::read_to_string(&path)?;
            let mut doc: DocumentMut = content.parse()?;
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

fn display_array(arr: &toml_edit::Array) -> String {
    arr.iter()
        .filter_map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_integer().map(|number| number.to_string()))
        })
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
    let uses_string_values = doc["luma_client"].is_table();
    if doc[table].get(key).map(Item::is_none).unwrap_or(true) {
        doc[table][key] = Item::Value(TomlValue::Array(Default::default()));
    }
    let arr = doc[table][key]
        .as_array_mut()
        .ok_or_else(|| anyhow!("{}.{} 不是数组", table, key))?;
    let values = arr
        .iter()
        .filter_map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_integer().map(|number| number.to_string()))
        })
        .collect::<Vec<_>>();
    if add {
        if !values.iter().any(|value| value == input) {
            if uses_string_values {
                arr.push(input);
            } else {
                arr.push(input.parse::<i64>()?);
            }
        }
    } else if let Some(pos) = values.iter().position(|v| v == input) {
        arr.remove(pos);
    }
    Ok(())
}
