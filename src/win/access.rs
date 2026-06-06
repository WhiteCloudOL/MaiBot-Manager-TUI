use crate::{app::App, plugins::NAPCAT_ADAPTER_PLUGIN_ID, theme::AppTheme};
use anyhow::{Result, anyhow, bail};
use dialoguer::{Confirm, Input, Select};
use regex::Regex;
use serde_json::Value;
use std::{fs, path::PathBuf};
use toml_edit::{DocumentMut, Item, Value as TomlValue, value};

const CHAT_TABLE: &str = "chat";

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
            let choice = Select::with_theme(&self.theme)
                .with_prompt("配置与访问")
                .items([
                    "查看 WebUI 访问信息",
                    "初始化 MaiBot 访问配置",
                    "修改 Adapter 黑白名单配置",
                    "返回",
                ])
                .default(0)
                .interact()?;
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

    pub(crate) fn print_access_info(&self) -> Result<()> {
        let cfg = self.require_config()?;
        let root = PathBuf::from(cfg.mai_path);
        let public_ip = self.get_public_ip().unwrap_or_else(|_| "127.0.0.1".into());
        self.print_section("访问汇总", "集中查看 MaiBot、NapCat 与 LLBot 的访问入口");
        self.print_kv("本机 / 公网 IP", &public_ip);

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
            self.print_line();
            self.print_kv("MaiBot WebUI", &format!("http://{display_host}:{port}"));
            self.print_kv("密钥", &token);
        }

        let napcat_cfg = root.join("NapCat").join("config").join("webui.json");
        if napcat_cfg.exists() {
            let data: Value = serde_json::from_str(&fs::read_to_string(napcat_cfg)?)?;
            self.print_line();
            self.print_kv(
                "NapCat WebUI",
                &format!(
                    "http://{}:{}",
                    public_ip,
                    data["port"].as_i64().unwrap_or(6099)
                ),
            );
            self.print_kv("密钥", data["token"].as_str().unwrap_or("(未设置)"));
        }

        let llbot_settings = root.join("LLBot").join("app_settings.json");
        if llbot_settings.exists() {
            self.print_line();
            self.print_kv(
                "LuckyLilliaBot Desktop",
                &root.join("LLBot").display().to_string(),
            );
        }
        Ok(())
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
    if !doc.get(table).and_then(Item::as_table).is_some() {
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
