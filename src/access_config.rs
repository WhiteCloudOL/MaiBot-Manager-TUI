//! Compatible access settings for MaiBot's current HTTP and legacy WebUI schemas.
use anyhow::{Result, anyhow};
use toml_edit::{Array, DocumentMut, Item, Table, value};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WebuiBinding {
    host: String,
    port: u16,
}

fn binding_section(doc: &DocumentMut) -> &'static str {
    let legacy_binding = doc.get("webui").is_some_and(|webui| {
        webui.as_table_like().is_none()
            || webui.get("host").is_some()
            || webui.get("port").is_some()
    });
    if doc.get("http").is_some() || !legacy_binding {
        "http"
    } else {
        "webui"
    }
}

pub(crate) fn webui_binding(doc: &DocumentMut) -> WebuiBinding {
    let section = doc.get(binding_section(doc));
    let host = section.and_then(|item| item.get("host"));
    let hosts = host
        .and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| host.and_then(Item::as_str).into_iter().collect());
    let hosts = hosts
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let host = hosts
        .iter()
        .find(|host| matches!(**host, "0.0.0.0" | "::"))
        .or_else(|| hosts.first())
        .copied()
        .unwrap_or("127.0.0.1")
        .to_string();
    let port = section
        .and_then(|item| item.get("port"))
        .and_then(|port| {
            port.as_integer()
                .and_then(|port| u16::try_from(port).ok())
                .or_else(|| port.as_str().and_then(|port| port.trim().parse().ok()))
        })
        .filter(|port| *port != 0)
        .unwrap_or(8001);
    WebuiBinding { host, port }
}

impl WebuiBinding {
    pub(crate) fn url(&self, public_ip: impl FnOnce() -> String) -> String {
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
        format!("http://{host}:{}", self.port)
    }
}

pub(crate) fn bind_webui_all_interfaces(doc: &mut DocumentMut) -> Result<()> {
    let section = binding_section(doc);
    let mut hosts = Array::new();
    hosts.push("0.0.0.0");
    hosts.push("::");
    set_table_value(doc, section, "host", value(hosts))
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

    #[test]
    fn current_http_settings_take_precedence_over_legacy_fields() {
        let doc = "[http]\nhost = [\"127.0.0.1\", \"0.0.0.0\"]\nport = 8123\n[webui]\nhost = \"localhost\"\nport = 9999\n".parse().unwrap();
        assert_eq!(
            webui_binding(&doc).url(|| "203.0.113.1".into()),
            "http://203.0.113.1:8123"
        );
    }

    #[test]
    fn missing_and_invalid_fields_use_defaults_without_panicking() {
        for source in [
            "",
            "[webui]\nenabled = true\n",
            "[webui]\nhost = [\"127.0.0.1\"]\n",
            "webui = false\n",
            "http = false\n",
            "[http]\nhost = []\nport = -1\n",
            "[http]\nhost = false\nport = 65536\n",
        ] {
            let doc = source.parse().unwrap();
            assert_eq!(
                webui_binding(&doc).url(|| panic!("loopback must not query public IP")),
                "http://127.0.0.1:8001",
                "{source}"
            );
        }
    }

    #[test]
    fn explicit_hosts_and_legacy_settings_are_preserved() {
        for (source, expected) in [
            (
                "[webui]\nhost = \"localhost\"\nport = 8124",
                "http://localhost:8124",
            ),
            ("[http]\nhost = [\"10.0.0.8\"]", "http://10.0.0.8:8001"),
            ("[http]\nhost = [\"::1\"]", "http://[::1]:8001"),
            ("[http]\nport = \"8126\"", "http://127.0.0.1:8126"),
        ] {
            assert_eq!(
                webui_binding(&source.parse().unwrap())
                    .url(|| panic!("explicit host must not query public IP")),
                expected
            );
        }
    }

    #[test]
    fn initialization_updates_the_effective_section_and_preserves_other_fields() {
        for source in [
            "[http]\nport = 8125\n[webui]\nsecure_cookie = true\n",
            "[webui]\nport = 8125\nsecure_cookie = true\n",
            "http = { port = 8125 }\n[webui]\nsecure_cookie = true\n",
        ] {
            let mut doc: DocumentMut = source.parse().unwrap();
            let section = binding_section(&doc);
            bind_webui_all_interfaces(&mut doc).unwrap();
            assert_eq!(
                webui_binding(&doc).url(|| "203.0.113.2".into()),
                "http://203.0.113.2:8125"
            );
            assert_eq!(
                doc.get("webui")
                    .and_then(|s| s.get("secure_cookie"))
                    .and_then(Item::as_bool),
                Some(true)
            );
            let hosts = doc
                .get(section)
                .and_then(|s| s.get("host"))
                .and_then(Item::as_array)
                .unwrap();
            assert_eq!(
                hosts.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
                ["0.0.0.0", "::"]
            );
            if section == "http" {
                assert!(doc.get("webui").and_then(|s| s.get("host")).is_none());
            }
        }
    }

    #[test]
    fn initialization_creates_missing_sections_and_rejects_invalid_tables() {
        let mut doc = DocumentMut::new();
        bind_webui_all_interfaces(&mut doc).unwrap();
        assert!(doc.get("http").is_some());
        enable_plugin(&mut doc).unwrap();
        assert_eq!(
            doc.get("plugin")
                .and_then(|s| s.get("enabled"))
                .and_then(Item::as_bool),
            Some(true)
        );
        for source in ["http = 123\n", "webui = false\n", "plugin = false\n"] {
            let mut doc: DocumentMut = source.parse().unwrap();
            let before = doc.to_string();
            let result = if source.starts_with("plugin") {
                enable_plugin(&mut doc)
            } else {
                bind_webui_all_interfaces(&mut doc)
            };
            assert!(result.is_err());
            assert_eq!(doc.to_string(), before);
        }
    }

    #[test]
    fn access_actions_handle_current_and_incomplete_configs() {
        use crate::{app::App, model::AppConfig};
        use std::{
            fs,
            path::PathBuf,
            time::{SystemTime, UNIX_EPOCH},
        };

        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cleanup = Cleanup(
            std::env::temp_dir().join(format!("maibot-access-test-{}-{stamp}", std::process::id())),
        );
        let root = &cleanup.0;
        fs::create_dir_all(root.join("MaiBot/config")).unwrap();
        fs::create_dir_all(root.join("MaiBot/data")).unwrap();
        fs::write(
            root.join("MaiBot/data/webui.json"),
            r#"{"access_token":"test-access-token"}"#,
        )
        .unwrap();
        let mut app = App::new().unwrap();
        app.config_path = root.join("manager-config");
        app.save_config(&AppConfig {
            mai_path: root.display().to_string(),
            ..Default::default()
        })
        .unwrap();
        let path = root.join("MaiBot/config/bot_config.toml");
        for (source, section, port) in [
            (
                "[http]\nhost = [\"127.0.0.1\"]\nport = 8127\n[webui]\nenabled = true\n",
                "http",
                8127,
            ),
            (
                "[http]\nhost = [\"127.0.0.1\"]\n[webui]\nenabled = true\n",
                "http",
                8001,
            ),
            ("[webui]\nenabled = true\n", "http", 8001),
            ("", "http", 8001),
            (
                "[webui]\nhost = \"127.0.0.1\"\nport = 8128\n",
                "webui",
                8128,
            ),
        ] {
            fs::write(&path, source).unwrap();
            let popup = app.dashboard_access_summary_popup();
            assert!(
                popup
                    .lines
                    .iter()
                    .any(|line| line.contains(&format!("http://127.0.0.1:{port}"))),
                "{popup:?}"
            );
            assert!(
                popup
                    .lines
                    .iter()
                    .any(|line| line.contains("test-access-token"))
            );
            app.apply_maibot_access_config().unwrap();
            let doc: DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
            assert_eq!(doc[section]["host"].as_array().unwrap().len(), 2);
            assert_eq!(webui_binding(&doc).port, port);
            if section == "http" {
                assert!(
                    doc.get("webui")
                        .and_then(|webui| webui.get("host"))
                        .is_none()
                );
            }
        }
        fs::write(&path, "http = false\n").unwrap();
        assert!(app.apply_maibot_access_config().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "http = false\n");
    }
}
