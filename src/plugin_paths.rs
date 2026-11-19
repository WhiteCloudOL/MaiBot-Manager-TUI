//! Filesystem boundaries shared by plugin actions on every platform.
use crate::app::App;
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(crate) fn validate_plugin_id(id: &str) -> Result<()> {
    if !id.contains(['.', '-'])
        || id.split(['.', '-']).any(|part| {
            part.is_empty() || !part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        })
    {
        bail!("插件 id 必须是有效的 manifest v2 标识，不能包含路径或特殊字符");
    }
    Ok(())
}

pub(crate) fn plugin_child(root: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.starts_with('.')
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        || matches!(name.to_ascii_lowercase().as_str(), "data" | "__pycache__")
    {
        bail!("插件参数必须是 plugins 下的单个非保留目录名");
    }
    if root
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        bail!("plugins 目录不能是符号链接");
    }
    let candidate = root.join(name);
    match fs::symlink_metadata(&candidate) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("插件目录不能是链接或普通文件: {name}");
            }
            let canonical_root = root.canonicalize()?;
            if candidate.canonicalize()?.parent() != Some(canonical_root.as_path()) {
                bail!("插件目录超出了 plugins 边界: {name}");
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("读取插件目录失败"),
    }
    Ok(candidate)
}

impl App {
    pub(crate) fn verify_plugin_destination(&self, path: &Path, id: &str) -> Result<()> {
        if path.exists() && self.plugin_id_from_dir(path)? != id {
            bail!("目标目录中的插件 id 与正在同步的插件不符");
        }
        Ok(())
    }

    pub(crate) fn plugin_action_path(&self, root: &Path, name: &str) -> Result<PathBuf> {
        let direct = plugin_child(root, name)?;
        if direct.is_dir() {
            self.plugin_id_from_dir(&direct)?;
            return Ok(direct);
        }
        if validate_plugin_id(name).is_ok()
            && let Some(path) = self.resolve_plugin_dir_by_id(root, name)?
        {
            return Ok(path);
        }
        bail!("插件目录不存在: {name}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::AppConfig, workspace::tests::Fixture};

    #[test]
    fn plugin_actions_reject_paths_and_reserved_directories_without_removing_data() {
        let fixture = Fixture::new();
        let bot = fixture.add_bot("one", "bots/one");
        let mut app = App::new().unwrap();
        app.config_path = fixture.0.join("manager-config");
        app.save_config(&AppConfig {
            mai_path: fixture.0.display().to_string(),
            ..Default::default()
        })
        .unwrap();
        let protected = bot.join("data/protected");
        fs::create_dir_all(protected.parent().unwrap()).unwrap();
        fs::write(&protected, "keep").unwrap();
        for name in [
            "..",
            "../data",
            "data",
            "DATA",
            ".update_backups",
            "C:\\outside",
            "a/b",
            "a\\b",
            "a:stream",
            "",
        ] {
            assert!(app.remove_plugin(name).is_err(), "{name}");
            assert!(app.update_plugin(name).is_err(), "{name}");
            assert_eq!(fs::read_to_string(&protected).unwrap(), "keep");
        }
        for id in [
            "../escape",
            "bad/id",
            "C:\\outside",
            ".bad",
            "bad..id",
            "missing_namespace",
        ] {
            assert!(crate::utils::plugin_dir_name(id).is_err(), "{id}");
        }
        let plugins = bot.join("plugins");
        let directory = plugins.join("example_sample-plugin");
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("_manifest.json"),
            r#"{"id":"example.sample-plugin"}"#,
        )
        .unwrap();
        assert_eq!(
            app.plugin_action_path(&plugins, "example_sample-plugin")
                .unwrap(),
            directory
        );
        assert_eq!(
            app.plugin_action_path(&plugins, "example.sample-plugin")
                .unwrap(),
            directory
        );
        fs::write(directory.join("_manifest.json"), r#"{"id":"wrong.plugin"}"#).unwrap();
        assert!(
            app.verify_plugin_destination(&directory, "example.sample-plugin")
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn plugin_links_cannot_update_or_remove_an_external_directory() {
        let fixture = Fixture::new();
        let bot = fixture.add_bot("one", "bots/one");
        let plugins = bot.join("plugins");
        fs::create_dir(&plugins).unwrap();
        let external = fixture.0.join("outside");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("protected"), "keep").unwrap();
        fs::write(
            external.join("_manifest.json"),
            r#"{"id":"example.plugin"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&external, plugins.join("example_plugin")).unwrap();
        let app = App::new().unwrap();
        assert!(app.plugin_action_path(&plugins, "example_plugin").is_err());
        assert!(
            app.resolve_plugin_dir_by_id(&plugins, "example.plugin")
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(external.join("protected")).unwrap(),
            "keep"
        );
    }
}
