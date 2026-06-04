use crate::app::App;
use anyhow::{Result, anyhow, bail};
use dialoguer::{Confirm, Input, Select};

use crate::plugins::NAPCAT_ADAPTER_PLUGIN_ID;
use crate::theme::AppTheme;
use dialoguer::console::style;
use regex::Regex;
use serde_json::Value;
use std::{fs, path::PathBuf};
use toml_edit::{DocumentMut, Item, Value as TomlValue, value};

impl App {
    fn napcat_adapter_dir(&self) -> Result<PathBuf> {
        let cfg = self.require_config()?;
        let plugins_dir = PathBuf::from(cfg.mai_path).join("MaiBot/plugins");
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
            match choice {
                0 => self.show_access_info()?,
                1 => self.initialize_maibot_access_config()?,
                2 => self.modify_adapter_config()?,
                _ => break,
            }
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
                public_ip.clone()
            };
            self.print_line();
            println!("{}", style("MaiBot WebUI").cyan().bold());
            self.print_kv("地址", &format!("http://{host_display}:{port}"));
            self.print_kv("密钥", &token);
        }
        let napcat_cfg = root.join("NapCat/config/webui.json");
        if napcat_cfg.exists() {
            let data: Value = serde_json::from_str(&fs::read_to_string(napcat_cfg)?)?;
            self.print_line();
            println!("{}", style("NapCat WebUI").cyan().bold());
            self.print_kv(
                "地址",
                &format!(
                    "http://{}:{}",
                    public_ip,
                    data["port"].as_i64().unwrap_or(0)
                ),
            );
            self.print_kv("密钥", data["token"].as_str().unwrap_or("(未设置)"));
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
                public_ip.clone()
            } else {
                host.to_string()
            };
            self.print_line();
            println!("{}", style("LuckyLilliaBot WebUI").cyan().bold());
            self.print_kv("地址", &format!("http://{display_host}:{port}"));
            self.print_kv("密码", &token);
        }
        Ok(())
    }

    pub(crate) fn print_adapter_config(&self) -> Result<()> {
        let path = self.adapter_config_path()?;
        let doc: DocumentMut = fs::read_to_string(&path)?.parse()?;
        self.print_kv(
            "群聊模式",
            doc["group_list_type"].as_str().unwrap_or("Unknown"),
        );
        self.print_kv(
            "群聊列表",
            &doc["group_list"]
                .as_array()
                .map(display_array)
                .unwrap_or_default(),
        );
        self.print_kv(
            "私聊模式",
            doc["private_list_type"].as_str().unwrap_or("Unknown"),
        );
        self.print_kv(
            "私聊列表",
            &doc["private_list"]
                .as_array()
                .map(display_array)
                .unwrap_or_default(),
        );
        self.print_kv(
            "封禁 QQ",
            &doc["ban_user_id"]
                .as_array()
                .map(display_array)
                .unwrap_or_default(),
        );
        Ok(())
    }

    pub(crate) fn set_adapter_list_mode(&self, key: &str, mode: &str) -> Result<()> {
        if !matches!(mode, "whitelist" | "blacklist") {
            bail!("名单模式只能是 whitelist 或 blacklist");
        }
        let path = self.adapter_config_path()?;
        let mut doc: DocumentMut = fs::read_to_string(&path)?.parse()?;
        doc[key] = value(mode);
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
        update_numeric_array(&mut doc, key, input, add)?;
        fs::write(&path, doc.to_string())?;
        Ok(())
    }

    pub(crate) fn initialize_maibot_access_config(&self) -> Result<()> {
        self.clear();
        self.print_header(None);
        self.print_section(
            "初始化访问配置",
            "将 MaiBot WebUI 绑定到 0.0.0.0 并启用 Napcat 适配器",
        );
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
        let adapter_cfg = self.napcat_adapter_dir()?.join("config.toml");
        if bot_cfg.exists() {
            let mut doc: DocumentMut = fs::read_to_string(&bot_cfg)?.parse()?;
            if doc["webui"].is_none() {
                doc["webui"] = Item::Table(Default::default());
            }
            doc["webui"]["host"] = value("0.0.0.0");
            fs::write(&bot_cfg, doc.to_string())?;
        }
        if adapter_cfg.exists() {
            let mut doc: DocumentMut = fs::read_to_string(&adapter_cfg)?.parse()?;
            if doc["plugin"].is_none() {
                doc["plugin"] = Item::Table(Default::default());
            }
            doc["plugin"]["enabled"] = value(true);
            fs::write(&adapter_cfg, doc.to_string())?;
        }
        Ok(())
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
                doc["group_list_type"].as_str().unwrap_or("Unknown"),
            );
            self.print_kv(
                "群聊列表",
                &doc["group_list"]
                    .as_array()
                    .map(display_array)
                    .unwrap_or_default(),
            );
            self.print_kv(
                "私聊模式",
                doc["private_list_type"].as_str().unwrap_or("Unknown"),
            );
            self.print_kv(
                "私聊列表",
                &doc["private_list"]
                    .as_array()
                    .map(display_array)
                    .unwrap_or_default(),
            );
            self.print_kv(
                "封禁 QQ",
                &doc["ban_user_id"]
                    .as_array()
                    .map(display_array)
                    .unwrap_or_default(),
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
            match choice {
                0 => toggle_string(&mut doc, "group_list_type", "whitelist", "blacklist"),
                1 => prompt_modify_numeric_array(&mut doc, "group_list", true, &self.theme)?,
                2 => prompt_modify_numeric_array(&mut doc, "group_list", false, &self.theme)?,
                3 => toggle_string(&mut doc, "private_list_type", "whitelist", "blacklist"),
                4 => prompt_modify_numeric_array(&mut doc, "private_list", true, &self.theme)?,
                5 => prompt_modify_numeric_array(&mut doc, "private_list", false, &self.theme)?,
                6 => prompt_modify_numeric_array(&mut doc, "ban_user_id", true, &self.theme)?,
                7 => prompt_modify_numeric_array(&mut doc, "ban_user_id", false, &self.theme)?,
                _ => break,
            }
            fs::write(&path, doc.to_string())?;
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

fn toggle_string(doc: &mut DocumentMut, key: &str, left: &str, right: &str) {
    let current = doc[key].as_str().unwrap_or(left);
    doc[key] = value(if current == left { right } else { left });
}

fn prompt_modify_numeric_array(
    doc: &mut DocumentMut,
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
    update_numeric_array(doc, key, &input, add)
}

fn update_numeric_array(doc: &mut DocumentMut, key: &str, input: &str, add: bool) -> Result<()> {
    if doc[key].is_none() {
        doc[key] = Item::Value(TomlValue::Array(Default::default()));
    }
    let arr = doc[key]
        .as_array_mut()
        .ok_or_else(|| anyhow!("{} 不是数组", key))?;
    let values = arr
        .iter()
        .filter_map(|v| v.as_integer())
        .map(|v| v.to_string())
        .collect::<Vec<_>>();
    if add {
        if !values.iter().any(|value| value == input) {
            arr.push(input.parse::<i64>()?);
        }
    } else if let Some(pos) = values.iter().position(|v| v == &input) {
        arr.remove(pos);
    }
    Ok(())
}
