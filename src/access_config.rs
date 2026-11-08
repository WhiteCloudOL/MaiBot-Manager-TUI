//! Shared WebUI settings in MaiBot 1.4.2's host/config.toml.
use anyhow::{Result, anyhow, bail};
use toml_edit::{Array, DocumentMut, Item, Table, value};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WebuiBinding {
    host: String,
    port: u16,
    https: bool,
    public_base_url: String,
    pub(crate) enabled: bool,
}

pub(crate) fn webui_binding(doc: &DocumentMut) -> Result<WebuiBinding> {
    let section = doc.get("webui");
    if section.is_some_and(|section| section.as_table_like().is_none()) {
        bail!("[webui] 必须是 TOML 配置表");
    }
    let hosts = match section.and_then(|section| section.get("hosts")) {
        None => vec!["127.0.0.1", "::1"],
        Some(hosts) => {
            let hosts = hosts
                .as_array()
                .ok_or_else(|| anyhow!("webui.hosts 必须是地址数组"))?;
            let hosts = hosts
                .iter()
                .map(|host| {
                    host.as_str()
                        .map(str::trim)
                        .filter(|host| !host.is_empty())
                        .ok_or_else(|| anyhow!("webui.hosts 包含无效地址"))
                })
                .collect::<Result<Vec<_>>>()?;
            if hosts.is_empty() {
                bail!("webui.hosts 不能为空");
            }
            hosts
        }
    };
    let host = hosts
        .iter()
        .find(|host| matches!(**host, "0.0.0.0" | "::"))
        .unwrap_or(&hosts[0])
        .to_string();
    let port = match section.and_then(|section| section.get("port")) {
        None => 8001,
        Some(port) => port
            .as_integer()
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| anyhow!("webui.port 必须是 1 到 65535 的整数"))?,
    };
    let text = |key| -> Result<&str> {
        section
            .and_then(|section| section.get(key))
            .map_or(Ok(""), |item| {
                item.as_str()
                    .ok_or_else(|| anyhow!("webui.{key} 必须是字符串"))
            })
    };
    let cert = text("ssl_certfile")?;
    let key = text("ssl_keyfile")?;
    if cert.is_empty() != key.is_empty() {
        bail!("WebUI 的 TLS 证书和私钥必须同时配置");
    }
    let public_base_url = text("public_base_url")?
        .trim()
        .trim_end_matches('/')
        .to_string();
    let enabled = section
        .and_then(|section| section.get("enabled"))
        .map_or(Ok(true), |item| {
            item.as_bool()
                .ok_or_else(|| anyhow!("webui.enabled 必须是布尔值"))
        })?;
    Ok(WebuiBinding {
        host,
        port,
        https: !cert.is_empty(),
        public_base_url,
        enabled,
    })
}

impl WebuiBinding {
    pub(crate) fn url(&self, public_ip: impl FnOnce() -> String) -> String {
        if !self.public_base_url.is_empty() {
            return self.public_base_url.clone();
        }
        let host = if matches!(self.host.as_str(), "0.0.0.0" | "::") {
            public_ip()
        } else {
            self.host.clone()
        };
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host
        };
        let scheme = if self.https { "https" } else { "http" };
        format!("{scheme}://{host}:{}", self.port)
    }
}

pub(crate) fn bind_webui_all_interfaces(doc: &mut DocumentMut) -> Result<()> {
    let mut hosts = Array::new();
    hosts.push("0.0.0.0");
    hosts.push("::");
    set_table_value(doc, "webui", "hosts", value(hosts))
}

pub(crate) fn enable_plugin(doc: &mut DocumentMut) -> Result<()> {
    set_table_value(doc, "plugin", "enabled", value(true))
}

fn set_table_value(doc: &mut DocumentMut, section: &str, key: &str, item: Item) -> Result<()> {
    let table = doc
        .entry(section)
        .or_insert_with(|| Item::Table(Table::new()));
    let table = table
        .as_table_like_mut()
        .ok_or_else(|| anyhow!("[{section}] 必须是 TOML 配置表"))?;
    table.insert(key, item);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::App, model::AppConfig, workspace::tests::Fixture};
    use std::fs;

    #[test]
    fn host_schema_defaults_ipv6_tls_and_public_address() {
        for (source, expected) in [
            ("", "http://127.0.0.1:8001"),
            (
                "[webui]\nhosts = [\"::1\"]\nport = 8124",
                "http://[::1]:8124",
            ),
            (
                "[webui]\nhosts = [\"10.0.0.8\"]\nssl_certfile = \"cert\"\nssl_keyfile = \"key\"",
                "https://10.0.0.8:8001",
            ),
            (
                "[webui]\nhosts = [\"0.0.0.0\"]\npublic_base_url = \"https://bot.example/\"",
                "https://bot.example",
            ),
            (
                "[http]\nhost = [\"0.0.0.0\"]\nport = 9999\n[webui]\nhost = [\"0.0.0.0\"]",
                "http://127.0.0.1:8001",
            ),
        ] {
            assert_eq!(
                webui_binding(&source.parse().unwrap())
                    .unwrap()
                    .url(|| panic!("no public IP lookup needed")),
                expected
            );
        }
        let binding = webui_binding(
            &"[webui]\nhosts = [\"127.0.0.1\", \"::\"]\nport = 8125"
                .parse()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            binding.url(|| "2001:db8::1".into()),
            "http://[2001:db8::1]:8125"
        );
    }

    #[test]
    fn invalid_current_host_config_is_reported() {
        for source in [
            "webui = false",
            "[webui]\nhosts = \"localhost\"",
            "[webui]\nhosts = []",
            "[webui]\nport = 0",
            "[webui]\nport = \"8125\"",
            "[webui]\nssl_certfile = \"cert\"",
        ] {
            assert!(webui_binding(&source.parse().unwrap()).is_err(), "{source}");
        }
    }

    #[test]
    fn initialization_preserves_host_security_and_port() {
        let mut doc = "[webui]\nport = 8125\nsecure_cookie = true\npublic_base_url = \"https://bot.example\"\nanti_crawler_mode = \"strict\"\n".parse().unwrap();
        bind_webui_all_interfaces(&mut doc).unwrap();
        assert_eq!(doc["webui"]["hosts"].as_array().unwrap().len(), 2);
        assert_eq!(doc["webui"]["port"].as_integer(), Some(8125));
        assert_eq!(doc["webui"]["secure_cookie"].as_bool(), Some(true));
        assert_eq!(doc["webui"]["anti_crawler_mode"].as_str(), Some("strict"));
        assert!(doc.get("http").is_none());
        let mut invalid = "webui = false".parse().unwrap();
        assert!(bind_webui_all_interfaces(&mut invalid).is_err());
    }

    #[test]
    fn access_actions_use_host_files_and_only_the_selected_bot() {
        let fixture = Fixture::new();
        let root = &fixture.0;
        let bot = fixture.add_bot("one", "bots/one");
        let other = fixture.add_bot("two", "bots/two");
        let host = root.join("MaiBot/host/config.toml");
        fs::write(&host, "[webui]\nhosts = [\"127.0.0.1\"]\nport = 8127\n").unwrap();
        fs::write(
            root.join("MaiBot/host/webui.json"),
            r#"{"access_token":"host-token"}"#,
        )
        .unwrap();
        let mut app = App::new().unwrap();
        app.config_path = root.join("manager-config");
        app.select_bot_id(Some("one".into()));
        app.save_config(&AppConfig {
            mai_path: root.display().to_string(),
            ..Default::default()
        })
        .unwrap();
        let popup = app.dashboard_access_summary_popup();
        assert!(
            popup
                .lines
                .iter()
                .any(|line| line.contains("http://127.0.0.1:8127")),
            "{popup:?}"
        );
        assert!(popup.lines.iter().any(|line| line.contains("host-token")));
        let before = fs::read_to_string(bot.join("config/bot_config.toml")).unwrap();
        let other_before = fs::read_to_string(other.join("config/bot_config.toml")).unwrap();
        app.apply_maibot_access_config().unwrap();
        let doc: DocumentMut = fs::read_to_string(&host).unwrap().parse().unwrap();
        assert_eq!(doc["webui"]["hosts"].as_array().unwrap().len(), 2);
        assert_eq!(doc["webui"]["port"].as_integer(), Some(8127));
        assert_eq!(
            fs::read_to_string(bot.join("config/bot_config.toml")).unwrap(),
            before
        );
        assert_eq!(
            fs::read_to_string(other.join("config/bot_config.toml")).unwrap(),
            other_before
        );
        fs::write(&host, "webui = false\n").unwrap();
        assert!(app.apply_maibot_access_config().is_err());
        assert_eq!(fs::read_to_string(&host).unwrap(), "webui = false\n");
    }
}
