//! Shared manager configuration. Missing setup state is distinct from invalid state.
use crate::{app::App, model::AppConfig};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

impl App {
    pub(crate) fn load_config(&self) -> Result<AppConfig> {
        let content = fs::read_to_string(&self.config_path)
            .with_context(|| format!("读取配置文件失败: {}", self.config_path.display()))?;
        parse_config(&content)
            .with_context(|| format!("解析配置文件失败: {}", self.config_path.display()))
    }

    pub(crate) fn load_config_for_setup(&self) -> Result<AppConfig> {
        match self.load_config() {
            Ok(config) => Ok(config),
            Err(error)
                if error
                    .downcast_ref::<io::Error>()
                    .is_some_and(|cause| cause.kind() == io::ErrorKind::NotFound) =>
            {
                Ok(AppConfig::default())
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn save_config(&self, config: &AppConfig) -> Result<()> {
        let fields = [
            ("USER_INSTALL_PATH", &config.user_install_path),
            ("MAI_PATH", &config.mai_path),
            ("MAI_PYTHON_ENV", &config.mai_python_env),
            ("MAI_LLBOT_PATH", &config.mai_llbot_path),
            ("MAI_INSTALL_MODE", &config.mai_install_mode),
            ("MAI_VENV_MODE", &config.mai_venv_mode),
            ("MAIBOT_BRANCH", &config.maibot_branch),
            ("PIP_DISPLAY", &config.pip_display),
            ("PIP_INDEX", &config.pip_index),
            ("PIP_HOST", &config.pip_host),
            ("BOT_PROTOCOLS", &config.bot_protocols),
        ];
        let mut content = String::new();
        for (key, value) in fields {
            if value
                .chars()
                .any(|character| matches!(character, '\n' | '\r' | '\0'))
            {
                bail!("配置字段 {key} 不能包含换行或 NUL");
            }
            content.push_str(key);
            content.push_str("=\"");
            for character in value.chars() {
                if matches!(character, '\\' | '"' | '$' | '`') {
                    content.push('\\');
                }
                content.push(character);
            }
            content.push_str("\"\n");
        }
        atomic_write(&self.config_path, content.as_bytes())
    }

    pub(crate) fn require_config(&self) -> Result<AppConfig> {
        let config = self.load_config_for_setup()?;
        if config.mai_path.trim().is_empty() {
            bail!("未找到安装配置，请先执行安装 / 更新");
        }
        Ok(config)
    }
}

fn config_value(raw: &str) -> Result<String> {
    if let Some(raw) = raw.strip_prefix('\'') {
        let Some((value, tail)) = raw.split_once('\'') else {
            bail!("单引号未闭合");
        };
        if !tail.trim().is_empty() && !tail.trim_start().starts_with('#') {
            bail!("引号后包含无效内容");
        }
        return Ok(value.into());
    }
    if let Some(raw) = raw.strip_prefix('"') {
        let mut characters = raw.chars();
        let mut value = String::new();
        while let Some(character) = characters.next() {
            match character {
                '"' => {
                    let tail = characters.as_str().trim();
                    if !tail.is_empty() && !tail.starts_with('#') {
                        bail!("引号后包含无效内容");
                    }
                    return Ok(value);
                }
                '\\' => {
                    let Some(next) = characters.next() else {
                        bail!("双引号未闭合");
                    };
                    if !matches!(next, '\\' | '"' | '$' | '`') {
                        value.push('\\');
                    }
                    value.push(next);
                }
                other => value.push(other),
            }
        }
        bail!("双引号未闭合");
    }
    Ok(raw
        .split_once(" #")
        .map_or(raw, |(value, _)| value)
        .trim_end()
        .into())
}

fn parse_config(content: &str) -> Result<AppConfig> {
    let mut map = BTreeMap::new();
    for (number, line) in content.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, raw)) = line.split_once('=') else {
            bail!("第 {} 行不是 KEY=value", number + 1);
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .all(|character| character.is_ascii_alphanumeric() || character == b'_')
        {
            bail!("第 {} 行的配置键无效", number + 1);
        }
        let value =
            config_value(raw.trim()).with_context(|| format!("第 {} 行格式错误", number + 1))?;
        map.insert(key.to_owned(), value);
    }
    Ok(AppConfig {
        user_install_path: map.get("USER_INSTALL_PATH").cloned().unwrap_or_default(),
        mai_path: map
            .get("MAI_PATH")
            .or_else(|| map.get("USER_INSTALL_PATH"))
            .cloned()
            .unwrap_or_default(),
        mai_python_env: map
            .get("MAI_PYTHON_ENV")
            .cloned()
            .unwrap_or_else(|| "uv".into()),
        mai_llbot_path: map.get("MAI_LLBOT_PATH").cloned().unwrap_or_default(),
        mai_install_mode: map.get("MAI_INSTALL_MODE").cloned().unwrap_or_default(),
        mai_venv_mode: map.get("MAI_VENV_MODE").cloned().unwrap_or_default(),
        maibot_branch: map
            .get("MAIBOT_BRANCH")
            .cloned()
            .unwrap_or_else(|| "main".into()),
        pip_display: map.get("PIP_DISPLAY").cloned().unwrap_or_default(),
        pip_index: map.get("PIP_INDEX").cloned().unwrap_or_default(),
        pip_host: map.get("PIP_HOST").cloned().unwrap_or_default(),
        bot_protocols: map.get("BOT_PROTOCOLS").cloned().unwrap_or_default(),
    })
}

pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let destination = if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary =
        destination.with_file_name(format!(".maibot-config-{}-{stamp}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        if let Ok(metadata) = fs::metadata(&destination) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.with_context(|| format!("保存配置文件失败: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::Fixture;

    #[test]
    fn missing_manager_config_allows_tui_setup_without_creating_a_file() {
        let fixture = Fixture::new();
        let mut app = App::new().unwrap();
        app.config_path = fixture.0.join("absent-config");
        assert!(app.load_config_for_setup().unwrap().mai_path.is_empty());
        app.choose_tui_bot().unwrap();
        let plan = app
            .build_default_install_plan(&app.load_config_for_setup().unwrap())
            .unwrap();
        assert_eq!(plan.install_path, dirs::home_dir().unwrap().join("maimai"));
        assert!(!app.config_path.exists());
        assert!(app.require_config().is_err());
    }

    #[test]
    fn damaged_or_unreadable_config_never_becomes_a_fresh_install() {
        let fixture = Fixture::new();
        let mut app = App::new().unwrap();
        app.config_path = fixture.0.join("manager-config");
        for content in [
            b"MAI_PATH=\"unfinished".as_slice(),
            b"\xff",
            b"[not-shell-config]",
        ] {
            fs::write(&app.config_path, content).unwrap();
            assert!(app.load_config_for_setup().is_err());
            assert!(app.choose_tui_bot().is_err());
            assert!(app.require_config().is_err());
            assert_eq!(fs::read(&app.config_path).unwrap(), content);
        }
        fs::remove_file(&app.config_path).unwrap();
        fs::create_dir(&app.config_path).unwrap();
        assert!(app.load_config_for_setup().is_err());
    }

    #[test]
    fn quoted_paths_round_trip_and_invalid_values_do_not_replace_existing_config() {
        let fixture = Fixture::new();
        let mut app = App::new().unwrap();
        app.config_path = fixture.0.join("manager-config");
        let mut config = AppConfig {
            mai_path: "C:\\Users\\O'Reilly\\\"test\"$value`name".into(),
            ..Default::default()
        };
        app.save_config(&config).unwrap();
        assert_eq!(app.load_config().unwrap().mai_path, config.mai_path);
        let before = fs::read(&app.config_path).unwrap();
        config.mai_path = "unsafe\nMAI_PATH=elsewhere".into();
        assert!(app.save_config(&config).is_err());
        assert_eq!(fs::read(&app.config_path).unwrap(), before);
        assert!(parse_config("export MAI_PATH='quoted path' # comment").is_ok());
        assert!(
            parse_config("MAI_PATH=\"C:\\new\\project\"")
                .unwrap()
                .mai_path
                .contains("\\new")
        );
    }
}
