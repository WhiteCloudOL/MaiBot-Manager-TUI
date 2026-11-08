//! Shared QQ adapter configuration and host policy editing.
use crate::{app::App, model::InstallPlan};
use anyhow::{Context, Result, anyhow, bail};
use dialoguer::{Input, Select};
use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

pub(crate) const REPO_NAME: &str = "MaiBot-SnowLuma-Adapter";
pub(crate) const PLUGIN_ID: &str = "maibot-team.snowluma-adapter";

fn read_doc(path: &Path) -> Result<DocumentMut> {
    match fs::read_to_string(path) {
        Ok(content) => content
            .parse()
            .with_context(|| format!("解析配置失败: {}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(err) => Err(err).with_context(|| format!("读取配置失败: {}", path.display())),
    }
}

fn save_doc(path: &Path, doc: &DocumentMut) -> Result<()> {
    // Reparse before replacing a live policy: invalid TOML would disable host filtering.
    let content = doc.to_string();
    content.parse::<DocumentMut>()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let destination = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary =
        destination.with_file_name(format!(".qq-config-{}-{stamp}.tmp", std::process::id()));
    // The host reads policies for every incoming message. Atomic replacement avoids
    // exposing an empty/partial policy while it is being written.
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Ok(metadata) = fs::metadata(&destination) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.with_context(|| format!("保存配置失败: {}", path.display()))
}

fn table<'a>(parent: &'a mut Table, name: &str) -> Result<&'a mut Table> {
    if !parent.contains_key(name) {
        parent[name] = Item::Table(Table::new());
    }
    parent[name]
        .as_table_mut()
        .ok_or_else(|| anyhow!("{name} 不是配置表"))
}

fn ids(parent: &Table, key: &str) -> Result<Vec<String>> {
    let Some(item) = parent.get(key) else {
        return Ok(Vec::new());
    };
    let arr = item.as_array().ok_or_else(|| anyhow!("{key} 不是数组"))?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_integer().map(|n| n.to_string()))
                .ok_or_else(|| anyhow!("{key} 包含无效号码"))
        })
        .collect()
}

fn set_ids(parent: &mut Table, key: &str, numbers: &[String]) {
    let mut arr = Array::new();
    for id in numbers {
        arr.push(id.as_str());
    }
    parent[key] = value(arr);
}

fn edit_ids(parent: &mut Table, key: &str, input: &str, add: bool) -> Result<()> {
    if input.is_empty() || !input.bytes().all(|c| c.is_ascii_digit()) {
        bail!("号码必须为纯数字");
    }
    let mut numbers = ids(parent, key)?;
    numbers.retain(|id| id != input);
    if add {
        numbers.push(input.to_string());
    }
    set_ids(parent, key, &numbers);
    Ok(())
}

fn policy_indices(doc: &DocumentMut) -> Result<Vec<usize>> {
    let Some(item) = doc.get("adapters") else {
        return Ok(Vec::new());
    };
    let adapters = item
        .as_array_of_tables()
        .ok_or_else(|| anyhow!("adapters 必须为 [[adapters]] 数组"))?;
    Ok(adapters
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            (t.get("plugin_id").and_then(Item::as_str) == Some(PLUGIN_ID)).then_some(i)
        })
        .collect())
}

fn rule_label(index: usize, rule: &Table) -> String {
    let details = [
        ("account_id", "账号"),
        ("scope", "范围"),
        ("platform", "平台"),
        ("gateway_name", "网关"),
        ("adapter_id", "适配器"),
    ]
    .iter()
    .filter_map(|(key, label)| {
        rule.get(key)
            .and_then(Item::as_str)
            .map(|v| format!("{label}: {v}"))
    })
    .collect::<Vec<_>>()
    .join(" / ");
    format!(
        "规则 {} · {}",
        index + 1,
        if details.is_empty() {
            "所有 QQ 账号"
        } else {
            &details
        }
    )
}

fn target_rule(doc: &mut DocumentMut, requested: Option<usize>) -> Result<usize> {
    let indices = policy_indices(doc)?;
    if let Some(number) = requested {
        return indices
            .into_iter()
            .find(|i| *i + 1 == number)
            .ok_or_else(|| {
                anyhow!("未找到统一 QQ 适配器规则 {number}；请先运行 access adapter show")
            });
    }
    match indices.as_slice() {
        [index] => Ok(*index),
        [] => {
            if doc.get("adapters").is_none() {
                doc["adapters"] = Item::ArrayOfTables(ArrayOfTables::new());
            }
            let adapters = doc["adapters"].as_array_of_tables_mut().unwrap();
            let mut rule = Table::new();
            rule["plugin_id"] = value(PLUGIN_ID);
            adapters.push(rule);
            Ok(adapters.len() - 1)
        }
        _ => bail!(
            "存在多条 QQ 账号/网关规则，请用 --rule <编号> 指定要编辑的规则（access adapter show 可查看）"
        ),
    }
}

fn chat_rule<'a>(doc: &'a mut DocumentMut, index: usize, chat: &str) -> Result<&'a mut Table> {
    let parent = doc["adapters"]
        .as_array_of_tables_mut()
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| anyhow!("适配器规则不存在"))?;
    let rule = table(parent, chat)?;
    Ok(rule)
}

fn set_mode(rule: &mut Table, mode: &str) -> Result<()> {
    match mode {
        "whitelist" => {
            rule["default_action"] = value("block");
        }
        "blacklist" => {
            rule["default_action"] = value("allow");
        }
        "inherit" => {
            rule.remove("default_action");
        }
        _ => bail!("名单模式只能是 whitelist、blacklist 或 inherit"),
    }
    // Keep explicit exceptions intact; switching modes must not invert their meaning.
    Ok(())
}

fn sync_active_policy_group(doc: &mut DocumentMut, index: usize, chat: &str) -> Result<()> {
    let rule = doc
        .get_mut("adapters")
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|rules| rules.get_mut(index))
        .ok_or_else(|| anyhow!("适配器规则不存在"))?;
    if rule.get("policy_groups").is_none() {
        return Ok(());
    }
    let active = rule
        .get("active_group")
        .and_then(Item::as_str)
        .ok_or_else(|| anyhow!("当前策略分组未设置，请先在 WebUI 选择分组"))?
        .to_string();
    let current = ["default_action", "allow_ids", "deny_ids", "disabled_ids"].map(|key| {
        (
            key,
            rule.get(chat).and_then(|policy| policy.get(key)).cloned(),
        )
    });
    let groups = rule
        .get_mut("policy_groups")
        .ok_or_else(|| anyhow!("策略分组不存在"))?;
    let selected: &mut dyn toml_edit::TableLike = match groups {
        Item::ArrayOfTables(groups) => groups
            .iter_mut()
            .find(|group| group.get("id").and_then(Item::as_str) == Some(active.as_str()))
            .ok_or_else(|| anyhow!("当前策略分组不存在: {active}"))?,
        Item::Value(toml_edit::Value::Array(groups)) => groups
            .iter_mut()
            .filter_map(toml_edit::Value::as_inline_table_mut)
            .find(|group| {
                group.get("id").and_then(toml_edit::Value::as_str) == Some(active.as_str())
            })
            .ok_or_else(|| anyhow!("当前策略分组不存在: {active}"))?,
        _ => bail!("policy_groups 必须是策略分组数组"),
    };
    let policy = selected
        .entry(chat)
        .or_insert_with(|| value(toml_edit::InlineTable::new()));
    let policy = policy
        .as_table_like_mut()
        .ok_or_else(|| anyhow!("当前分组的 {chat} 规则必须是配置表"))?;
    for (key, item) in current {
        if let Some(item) = item {
            policy.insert(key, item);
        } else {
            policy.remove(key);
        }
    }
    Ok(())
}

impl App {
    pub(crate) fn qq_adapter_dir(&self) -> Result<Option<PathBuf>> {
        self.resolve_plugin_dir_by_id(&self.bot_dir()?.join("plugins"), PLUGIN_ID)
    }

    fn qq_policy_path(&self) -> Result<PathBuf> {
        Ok(self.bot_dir()?.join("config/adapter_policy.toml"))
    }

    fn qq_filter_path(&self) -> Result<PathBuf> {
        Ok(self
            .qq_adapter_dir()?
            .ok_or_else(|| anyhow!("未找到统一 QQ 适配器，请先执行部署与更新"))?
            .join("config.toml"))
    }

    pub(crate) fn print_adapter_config(&self) -> Result<()> {
        let path = self.qq_policy_path()?;
        self.print_kv("策略文件", &path.display().to_string());
        let doc = read_doc(&path)?;
        for (chat, label) in [("group", "群聊全局默认"), ("private", "私聊全局默认")] {
            let action = doc
                .get("defaults")
                .and_then(|i| i.get(chat))
                .and_then(|i| i.get("default_action"))
                .and_then(Item::as_str)
                .unwrap_or("allow");
            self.print_kv(
                label,
                if action == "block" {
                    "拒绝"
                } else {
                    "放行"
                },
            );
        }
        let indices = policy_indices(&doc)?;
        if indices.is_empty() {
            self.print_hint("尚无 QQ 专属规则，使用全局默认策略（未配置时放行）。");
        }
        for index in indices {
            let rule = doc["adapters"]
                .as_array_of_tables()
                .unwrap()
                .get(index)
                .unwrap();
            self.print_line();
            self.print_hint(&rule_label(index, rule));
            for (chat, label) in [("group", "群聊"), ("private", "私聊")] {
                let typed = rule.get(chat).and_then(Item::as_table);
                let action = typed
                    .and_then(|t| t.get("default_action"))
                    .and_then(Item::as_str)
                    .unwrap_or("inherit");
                let mode = match action {
                    "block" => "白名单（其他拒绝）",
                    "allow" => "黑名单（其他放行）",
                    _ => "使用默认",
                };
                self.print_kv(&format!("{label}模式"), mode);
                if let Some(t) = typed {
                    for (key, text) in [
                        ("allow_ids", "允许"),
                        ("deny_ids", "拒绝"),
                        ("disabled_ids", "停用"),
                    ] {
                        self.print_kv(&format!("{label}{text}"), &ids(t, key)?.join(", "));
                    }
                }
            }
        }
        if let Some(dir) = self.qq_adapter_dir()? {
            let plugin = read_doc(&dir.join("config.toml"))?;
            let banned = plugin
                .get("filters")
                .and_then(Item::as_table)
                .map(|t| ids(t, "ban_user_id"))
                .transpose()?
                .unwrap_or_default();
            self.print_kv("发送者黑名单", &banned.join(", "));
        }
        self.print_hint(
            "群聊填群号，私聊填对方 QQ；发送者黑名单限制所有群聊和私聊。策略保存后无需重启。",
        );
        Ok(())
    }

    pub(crate) fn set_adapter_list_mode(
        &self,
        key: &str,
        mode: &str,
        requested: Option<usize>,
    ) -> Result<()> {
        let chat = match key {
            "group_list_type" => "group",
            "private_list_type" => "private",
            _ => bail!("未知名单字段: {key}"),
        };
        let path = self.qq_policy_path()?;
        let mut doc = read_doc(&path)?;
        let index = target_rule(&mut doc, requested)?;
        set_mode(chat_rule(&mut doc, index, chat)?, mode)?;
        sync_active_policy_group(&mut doc, index, chat)?;
        save_doc(&path, &doc)
    }

    pub(crate) fn update_adapter_numeric_list(
        &self,
        key: &str,
        input: &str,
        add: bool,
        requested: Option<usize>,
    ) -> Result<()> {
        if key == "ban_user_id" {
            let path = self.qq_filter_path()?;
            let mut doc = read_doc(&path)?;
            edit_ids(table(doc.as_table_mut(), "filters")?, key, input, add)?;
            return save_doc(&path, &doc);
        }
        let (chat, explicit) = match key {
            "group_list" => ("group", None),
            "private_list" => ("private", None),
            "group_allow" => ("group", Some("allow_ids")),
            "group_deny" => ("group", Some("deny_ids")),
            "private_allow" => ("private", Some("allow_ids")),
            "private_deny" => ("private", Some("deny_ids")),
            _ => bail!("未知名单字段: {key}"),
        };
        let path = self.qq_policy_path()?;
        let mut doc = read_doc(&path)?;
        let index = target_rule(&mut doc, requested)?;
        let rule = chat_rule(&mut doc, index, chat)?;
        let field = match explicit {
            Some(field) => field,
            None => match rule.get("default_action").and_then(Item::as_str) {
                Some("block") => "allow_ids",
                Some("allow") => "deny_ids",
                _ => bail!(
                    "请先设置 {chat}-mode，或使用 {chat}-allow-add / {chat}-deny-add 明确编辑允许或拒绝名单"
                ),
            },
        };
        edit_ids(rule, field, input, add)?;
        // An explicit edit reactivates this entry, matching Host's override editor.
        if rule.contains_key("disabled_ids") {
            edit_ids(rule, "disabled_ids", input, false)?;
        }
        if add {
            edit_ids(
                rule,
                if field == "allow_ids" {
                    "deny_ids"
                } else {
                    "allow_ids"
                },
                input,
                false,
            )?;
        }
        sync_active_policy_group(&mut doc, index, chat)?;
        save_doc(&path, &doc)
    }

    fn choose_qq_rule(&self) -> Result<Option<usize>> {
        let doc = read_doc(&self.qq_policy_path()?)?;
        let indices = policy_indices(&doc)?;
        if indices.len() <= 1 {
            return Ok(indices.first().map(|i| i + 1));
        }
        let adapters = doc["adapters"].as_array_of_tables().unwrap();
        let labels = indices
            .iter()
            .map(|i| {
                crate::ui::truncate_display(
                    &rule_label(*i, adapters.get(*i).unwrap()),
                    crossterm::terminal::size()
                        .unwrap_or((80, 24))
                        .0
                        .saturating_sub(4) as usize,
                )
            })
            .collect::<Vec<_>>();
        let selected = Select::with_theme(&self.theme)
            .with_prompt("选择要编辑的 QQ 账号 / 网关规则")
            .items(&labels)
            .default(0)
            .interact()?;
        Ok(Some(indices[selected] + 1))
    }

    pub(crate) fn modify_adapter_config(&self) -> Result<()> {
        loop {
            self.clear();
            self.print_header(None);
            self.print_section("QQ 黑白名单", "NapCat / SnowLuma 共用名单策略");
            self.print_kv_fit("策略文件", &self.qq_policy_path()?.display().to_string());
            self.print_hint_fit("群聊填写群号，私聊填写对方 QQ；策略保存后无需重启。");
            let selected = Select::with_theme(&self.theme)
                .with_prompt("选择名单")
                .items(["群聊规则", "私聊规则", "发送者黑名单", "返回"])
                .default(0)
                .interact()?;
            if selected == 3 {
                return Ok(());
            }
            let sender_bans = selected == 2;
            let requested = if sender_bans {
                None
            } else {
                self.choose_qq_rule()?
            };
            loop {
                self.clear();
                self.print_header(None);
                self.print_section(
                    ["群聊规则", "私聊规则", "发送者黑名单"][selected],
                    if sender_bans {
                        "过滤该 QQ 在所有群聊和私聊中的聊天消息"
                    } else {
                        "允许 / 拒绝例外与默认动作分别保存"
                    },
                );
                let chat = if selected == 0 { "group" } else { "private" };
                if sender_bans {
                    let config = read_doc(&self.qq_filter_path()?)?;
                    let banned = config
                        .get("filters")
                        .and_then(Item::as_table)
                        .map(|t| ids(t, "ban_user_id"))
                        .transpose()?
                        .unwrap_or_default();
                    self.print_kv_fit("封禁 QQ", &banned.join(", "));
                } else {
                    let mut doc = read_doc(&self.qq_policy_path()?)?;
                    let index = target_rule(&mut doc, requested)?;
                    self.print_hint_fit(&rule_label(
                        index,
                        doc["adapters"]
                            .as_array_of_tables()
                            .unwrap()
                            .get(index)
                            .unwrap(),
                    ));
                    let rule = chat_rule(&mut doc, index, chat)?;
                    let mode = match rule.get("default_action").and_then(Item::as_str) {
                        Some("block") => "白名单（其他拒绝）",
                        Some("allow") => "黑名单（其他放行）",
                        _ => "使用默认",
                    };
                    self.print_kv_fit("默认动作", mode);
                    self.print_kv_fit("允许名单", &ids(rule, "allow_ids")?.join(", "));
                    self.print_kv_fit("拒绝名单", &ids(rule, "deny_ids")?.join(", "));
                    self.print_kv_fit("停用条目", &ids(rule, "disabled_ids")?.join(", "));
                }
                let actions = if sender_bans {
                    vec!["添加 QQ", "移除 QQ", "返回"]
                } else {
                    vec![
                        "设置默认动作",
                        "添加允许号码",
                        "移除允许号码",
                        "添加拒绝号码",
                        "移除拒绝号码",
                        "返回",
                    ]
                };
                let action = Select::with_theme(&self.theme)
                    .with_prompt("选择操作")
                    .items(&actions)
                    .max_length(6)
                    .default(0)
                    .interact()?;
                if action == actions.len() - 1 {
                    break;
                }
                let result = if !sender_bans && action == 0 {
                    let mode = Select::with_theme(&self.theme)
                        .with_prompt("默认动作（保留已有名单）")
                        .items([
                            "白名单：其他聊天拒绝",
                            "黑名单：其他聊天放行",
                            "使用默认：继承后续规则",
                        ])
                        .default(0)
                        .interact()?;
                    self.set_adapter_list_mode(
                        if selected == 0 {
                            "group_list_type"
                        } else {
                            "private_list_type"
                        },
                        ["whitelist", "blacklist", "inherit"][mode],
                        requested,
                    )
                } else {
                    let key = if sender_bans {
                        "ban_user_id".to_string()
                    } else {
                        format!("{chat}_{}", if action <= 2 { "allow" } else { "deny" })
                    };
                    let add = if sender_bans {
                        action == 0
                    } else {
                        action == 1 || action == 3
                    };
                    let input: String = Input::with_theme(&self.theme)
                        .with_prompt("输入群号 / QQ 号（纯数字）")
                        .interact_text()?;
                    self.update_adapter_numeric_list(&key, &input, add, requested)
                };
                if self.handle_menu_result(result)? {
                    self.pause("已保存，按回车继续")?;
                }
            }
        }
    }

    pub(crate) fn finish_qq_adapter_install(
        &self,
        plan: &InstallPlan,
        adapter_dir: &Path,
    ) -> Result<()> {
        configure_install(&self.bot_dir_at(&plan.install_path)?, adapter_dir)
    }
}

fn configure_install(bot_dir: &Path, adapter_dir: &Path) -> Result<()> {
    let config_path = adapter_dir.join("config.toml");
    let mut config = read_doc(&config_path)?;
    let policy_path = bot_dir.join("config/adapter_policy.toml");
    let fresh_install = config.is_empty() && !policy_path.exists();
    let mut policy = read_doc(&policy_path)?;
    policy_indices(&policy)?;
    if fresh_install {
        let index = target_rule(&mut policy, None)?;
        for chat in ["group", "private"] {
            let rule = chat_rule(&mut policy, index, chat)?;
            set_mode(rule, "whitelist")?;
            set_ids(rule, "allow_ids", &[]);
            set_ids(rule, "deny_ids", &[]);
        }
    }
    if config.get("plugin").is_none() {
        table(config.as_table_mut(), "plugin")?["enabled"] = value(true);
    }
    if config.get("client").is_none() {
        let client = table(config.as_table_mut(), "client")?;
        client["server"] = value("127.0.0.1");
        client["port"] = value(3001);
        client["token"] = value("");
        client["client_type"] = value("auto");
    }
    if fresh_install {
        save_doc(&policy_path, &policy)?;
    }
    save_doc(&config_path, &config)?;
    println!("统一 QQ 适配器已配置（NapCat / SnowLuma 自动识别）。");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppConfig;

    struct Fixture {
        root: PathBuf,
        app: App,
        _workspace: crate::workspace::tests::Fixture,
    }
    impl Fixture {
        fn new() -> Self {
            let workspace = crate::workspace::tests::Fixture::new();
            let root = workspace.0.clone();
            workspace.add_bot("test", "bots/test");
            let mut app = App::new().unwrap();
            app.config_path = root.join("manager-config");
            app.save_config(&AppConfig {
                mai_path: root.display().to_string(),
                ..Default::default()
            })
            .unwrap();
            Self {
                root,
                app,
                _workspace: workspace,
            }
        }
        fn adapter(&self) -> PathBuf {
            let dir = self
                .root
                .join("MaiBot/bots/test/plugins/maibot-team_snowluma-adapter");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("_manifest.json"),
                format!(r#"{{"id":"{PLUGIN_ID}"}}"#),
            )
            .unwrap();
            dir
        }
        fn policy(&self) -> DocumentMut {
            read_doc(
                &self
                    .root
                    .join("MaiBot/bots/test/config/adapter_policy.toml"),
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn policy_modes_and_explicit_lists_keep_their_meaning() {
        let fixture = Fixture::new();
        fixture
            .app
            .set_adapter_list_mode("group_list_type", "whitelist", None)
            .unwrap();
        fixture
            .app
            .update_adapter_numeric_list("group_list", "123", true, None)
            .unwrap();
        fixture
            .app
            .set_adapter_list_mode("group_list_type", "blacklist", None)
            .unwrap();
        fixture
            .app
            .update_adapter_numeric_list("group_list", "456", true, None)
            .unwrap();
        fixture
            .app
            .update_adapter_numeric_list("group_deny", "123", true, None)
            .unwrap();
        let mut doc = fixture.policy();
        let group = chat_rule(&mut doc, 0, "group").unwrap();
        assert_eq!(group["default_action"].as_str(), Some("allow"));
        assert!(ids(group, "allow_ids").unwrap().is_empty());
        assert_eq!(ids(group, "deny_ids").unwrap(), ["456", "123"]);
        fixture
            .app
            .set_adapter_list_mode("group_list_type", "inherit", None)
            .unwrap();
        let before = fixture.policy().to_string();
        assert!(
            fixture
                .app
                .update_adapter_numeric_list("group_list", "789", true, None)
                .is_err()
        );
        assert_eq!(fixture.policy().to_string(), before);
        assert!(
            fixture
                .app
                .update_adapter_numeric_list("group_allow", "12x", true, None)
                .is_err()
        );
        assert_eq!(fixture.policy().to_string(), before);
        let mut doc = fixture.policy();
        assert!(
            !chat_rule(&mut doc, 0, "group")
                .unwrap()
                .contains_key("default_action")
        );
    }

    #[test]
    fn editing_lists_syncs_only_the_active_group_for_both_group_formats() {
        for inline in [false, true] {
            let fixture = Fixture::new();
            let path = fixture
                .root
                .join("MaiBot/bots/test/config/adapter_policy.toml");
            let mut content = format!(
                "[defaults.group]\ndefault_action = \"allow\"\n[[adapters]]\nplugin_id = \"{PLUGIN_ID}\"\naccount_id = \"111\"\nactive_group = \"live\"\n"
            );
            if inline {
                content.push_str("policy_groups = [{id = \"live\", name = \"current\", private = {deny_ids = [\"888\"]}}, {id = \"backup\", group = {allow_ids = [\"777\"]}}]\n");
            }
            content.push_str("[adapters.group]\ndefault_action = \"block\"\nallow_ids = [\"123\"]\ndeny_ids = [\"999\"]\ndisabled_ids = [\"123\", \"999\", \"456\"]\n[adapters.private]\ndeny_ids = [\"888\"]\n");
            if !inline {
                content.push_str("[[adapters.policy_groups]]\nid = \"live\"\nname = \"current\"\n[adapters.policy_groups.private]\ndeny_ids = [\"888\"]\n[[adapters.policy_groups]]\nid = \"backup\"\n[adapters.policy_groups.group]\nallow_ids = [\"777\"]\n");
            }
            fs::write(&path, content).unwrap();
            let before = fixture.policy();
            let before_rule = &before["adapters"][0];
            let backup = before_rule["policy_groups"][1].to_string();
            let private = before_rule["policy_groups"][0]["private"].to_string();
            fixture
                .app
                .set_adapter_list_mode("group_list_type", "inherit", None)
                .unwrap();
            fixture
                .app
                .update_adapter_numeric_list("group_allow", "456", true, None)
                .unwrap();
            fixture
                .app
                .update_adapter_numeric_list("group_deny", "999", false, None)
                .unwrap();
            let doc = fixture.policy();
            let rule = &doc["adapters"][0];
            let snapshot = &rule["policy_groups"][0]["group"];
            assert!(snapshot.get("default_action").is_none());
            assert_eq!(
                snapshot["allow_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>(),
                ["123", "456"]
            );
            assert!(snapshot["deny_ids"].as_array().unwrap().is_empty());
            assert_eq!(snapshot["disabled_ids"][0].as_str(), Some("123"));
            assert_eq!(snapshot["disabled_ids"].as_array().unwrap().len(), 1);
            assert_eq!(rule["group"]["disabled_ids"][0].as_str(), Some("123"));
            assert_eq!(rule["policy_groups"][1].to_string(), backup);
            assert_eq!(rule["policy_groups"][0]["private"].to_string(), private);
            assert_eq!(rule["account_id"].as_str(), Some("111"));
            assert_eq!(
                doc["defaults"]["group"]["default_action"].as_str(),
                Some("allow")
            );
            fixture
                .app
                .set_adapter_list_mode("group_list_type", "blacklist", None)
                .unwrap();
            fixture
                .app
                .update_adapter_numeric_list("group_list", "555", true, None)
                .unwrap();
            let doc = fixture.policy();
            let snapshot = &doc["adapters"][0]["policy_groups"][0]["group"];
            assert_eq!(snapshot["default_action"].as_str(), Some("allow"));
            assert_eq!(snapshot["deny_ids"][0].as_str(), Some("555"));
        }
    }

    #[test]
    fn invalid_active_policy_group_does_not_write_partial_changes() {
        for group in [
            "policy_groups = []\n",
            "active_group = \"missing\"\npolicy_groups = [{id = \"other\"}]\n",
            "active_group = \"live\"\npolicy_groups = [{id = \"live\", group = false}]\n",
        ] {
            let fixture = Fixture::new();
            let path = fixture
                .root
                .join("MaiBot/bots/test/config/adapter_policy.toml");
            let content = format!(
                "[[adapters]]\nplugin_id = \"{PLUGIN_ID}\"\n{group}[adapters.group]\ndefault_action = \"block\"\nallow_ids = [\"123\"]\n"
            );
            fs::write(&path, &content).unwrap();
            assert!(
                fixture
                    .app
                    .set_adapter_list_mode("group_list_type", "inherit", None)
                    .is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
            assert!(
                fixture
                    .app
                    .update_adapter_numeric_list("group_allow", "456", true, None)
                    .is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
    }

    #[test]
    fn account_rule_selection_preserves_other_rules_and_global_defaults() {
        let fixture = Fixture::new();
        let content = format!(
            r#"# keep this comment
[defaults.group]
default_action = "block"
[[adapters]]
plugin_id = "other.plugin"
[adapters.group]
allow_ids = ["other"]
[[adapters]]
plugin_id = "{PLUGIN_ID}"
account_id = "111"
[adapters.group]
allow_ids = ["123"]
[[adapters]]
plugin_id = "{PLUGIN_ID}"
account_id = "222"
[adapters.group]
deny_ids = ["456"]
"#
        );
        fs::write(
            fixture
                .root
                .join("MaiBot/bots/test/config/adapter_policy.toml"),
            &content,
        )
        .unwrap();
        assert!(
            fixture
                .app
                .set_adapter_list_mode("group_list_type", "whitelist", None)
                .is_err()
        );
        assert_eq!(fixture.policy().to_string(), content);
        fixture
            .app
            .set_adapter_list_mode("group_list_type", "whitelist", Some(2))
            .unwrap();
        let doc = fixture.policy();
        assert_eq!(
            doc["defaults"]["group"]["default_action"].as_str(),
            Some("block")
        );
        let adapters = doc["adapters"].as_array_of_tables().unwrap();
        assert_eq!(adapters.get(1).unwrap()["account_id"].as_str(), Some("111"));
        assert_eq!(
            adapters.get(1).unwrap()["group"]["default_action"].as_str(),
            Some("block")
        );
        assert_eq!(
            adapters.get(2).unwrap()["group"]["deny_ids"]
                .as_array()
                .unwrap()
                .get(0)
                .unwrap()
                .as_str(),
            Some("456")
        );
        assert!(doc.to_string().contains("# keep this comment"));
    }

    #[test]
    fn sender_blacklist_is_a_string_array_in_plugin_filters() {
        let fixture = Fixture::new();
        let dir = fixture.adapter();
        fs::write(
            dir.join("config.toml"),
            "[filters]\nignore_self_message = true\nban_user_id = [123, \"456\"]\n",
        )
        .unwrap();
        fixture
            .app
            .update_adapter_numeric_list("ban_user_id", "123", false, None)
            .unwrap();
        fixture
            .app
            .update_adapter_numeric_list("ban_user_id", "789", true, None)
            .unwrap();
        fixture
            .app
            .update_adapter_numeric_list("ban_user_id", "789", true, None)
            .unwrap();
        let doc = read_doc(&dir.join("config.toml")).unwrap();
        assert_eq!(
            ids(doc["filters"].as_table().unwrap(), "ban_user_id").unwrap(),
            ["456", "789"]
        );
        assert_eq!(doc["filters"]["ignore_self_message"].as_bool(), Some(true));
        assert!(!doc.as_table().contains_key("chat"));
        assert!(
            !fixture
                .root
                .join("MaiBot/bots/test/config/adapter_policy.toml")
                .exists()
        );
    }

    #[test]
    fn malformed_policy_is_never_replaced_during_installation() {
        let fixture = Fixture::new();
        let dir = fixture.adapter();
        let path = fixture
            .root
            .join("MaiBot/bots/test/config/adapter_policy.toml");
        fs::write(&path, "[[adapters]\n").unwrap();
        assert!(configure_install(&fixture.app.bot_dir().unwrap(), &dir).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "[[adapters]\n");
        assert!(!dir.join("config.toml").exists());
    }

    #[test]
    fn fresh_install_keeps_empty_whitelists_until_targets_are_configured() {
        let fixture = Fixture::new();
        let dir = fixture.adapter();
        configure_install(&fixture.app.bot_dir().unwrap(), &dir).unwrap();
        let mut policy = fixture.policy();
        for chat in ["group", "private"] {
            let rule = chat_rule(&mut policy, 0, chat).unwrap();
            assert_eq!(rule["default_action"].as_str(), Some("block"));
            assert!(ids(rule, "allow_ids").unwrap().is_empty());
            assert!(ids(rule, "deny_ids").unwrap().is_empty());
        }
        let config = read_doc(&dir.join("config.toml")).unwrap();
        assert_eq!(config["plugin"]["enabled"].as_bool(), Some(true));
        assert_eq!(config["client"]["client_type"].as_str(), Some("auto"));
        assert_eq!(config["client"]["port"].as_integer(), Some(3001));
    }
}
