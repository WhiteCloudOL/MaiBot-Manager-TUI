//! MaiBot 1.4.2 host registry and per-Bot paths. The registry is always read-only.
use crate::app::App;
use anyhow::{Context, Result, anyhow, bail};
use dialoguer::Select;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct BotInstance {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) directory: PathBuf,
    initialization: String,
}

#[derive(Default)]
pub(crate) struct WorkspaceSelection {
    bot_id: Option<String>,
    cached: Option<(PathBuf, std::result::Result<BotInstance, String>)>,
}

pub(crate) fn read_bots(workspace: &Path) -> Result<Vec<BotInstance>> {
    // The installed interpreter supplies SQLite without adding native libraries to
    // the manager's static musl builds. No MaiBot modules are imported or executed.
    let suffix = if cfg!(target_os = "windows") {
        "Scripts/python.exe"
    } else {
        "bin/python"
    };
    let python = [
        workspace.join(".venv").join(suffix),
        workspace
            .parent()
            .unwrap_or(workspace)
            .join("venv")
            .join(suffix),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .ok_or_else(|| anyhow!("未找到 MaiBot Python 环境；请先完成安装/更新"))?;
    let mut command = Command::new(python);
    command
        .args(["-I", "-S", "-c", REGISTRY_QUERY])
        .arg(workspace);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().context("无法启动宿主注册表只读查询")?;
    if !output.status.success() {
        bail!(
            "读取宿主注册表 {} 失败: {}",
            workspace.join("host/registry.sqlite3").display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("解析宿主 Bot 列表失败")
}

const REGISTRY_QUERY: &str = r#"
from contextlib import closing
from pathlib import Path
import json
import sqlite3
import sys

root = Path(sys.argv[1]).resolve()
path = root / "host" / "registry.sqlite3"
with closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=1)) as connection:
    version = connection.execute("PRAGMA user_version").fetchone()[0]
    if version != 1:
        raise ValueError(f"Unsupported MaiBot registry schema: {version}")
    rows = connection.execute(
        "SELECT bot_id, name, directory, initialization FROM bots ORDER BY created_at, bot_id"
    ).fetchall()
print(json.dumps([
    {"id": row[0], "name": row[1], "directory": str((root / row[2]).resolve()), "initialization": row[3]}
    for row in rows
]))
"#;

fn resolve_bot(workspace: &Path, requested: Option<&str>) -> Result<BotInstance> {
    let bots = read_bots(workspace)?;
    let bot = match requested {
        Some(id) => bots
            .into_iter()
            .find(|bot| bot.id == id)
            .ok_or_else(|| anyhow!("未找到 Bot {id}；请用 maibot bots 查看 ID"))?,
        None => match bots.as_slice() {
            [bot] => bot.clone(),
            [] => bail!("宿主工作区中没有 Bot，请先在 MaiBot WebUI 创建 Bot"),
            _ => bail!("工作区中有多个 Bot，请用 maibot bots 查看 ID，再通过 --bot <ID> 指定目标"),
        },
    };
    if bot.initialization != "initialized" {
        bail!(
            "Bot {} 尚未初始化完成（{}），请在 MaiBot WebUI 修复后重试",
            bot.name,
            bot.initialization
        );
    }
    if !bot.directory.join("config/bot_config.toml").is_file() {
        bail!(
            "Bot 配置不存在: {}",
            bot.directory.join("config/bot_config.toml").display()
        );
    }
    Ok(bot)
}

impl App {
    pub(crate) fn select_bot_id(&self, id: Option<String>) {
        let mut selection = self.workspace.borrow_mut();
        selection.bot_id = id;
        selection.cached = None;
    }

    pub(crate) fn invalidate_bot_cache(&self) {
        self.workspace.borrow_mut().cached = None;
    }

    pub(crate) fn bot_dir_at(&self, install_root: &Path) -> Result<PathBuf> {
        let workspace = install_root.join("MaiBot");
        let mut selection = self.workspace.borrow_mut();
        if selection
            .cached
            .as_ref()
            .is_none_or(|(root, _)| root != &workspace)
        {
            let bot = resolve_bot(&workspace, selection.bot_id.as_deref())
                .map_err(|error| format!("{error:#}"));
            selection.cached = Some((workspace, bot));
        }
        selection
            .cached
            .as_ref()
            .unwrap()
            .1
            .as_ref()
            .map(|bot| bot.directory.clone())
            .map_err(|message| anyhow!("{message}"))
    }

    pub(crate) fn bot_dir(&self) -> Result<PathBuf> {
        self.bot_dir_at(Path::new(&self.require_config()?.mai_path))
    }

    pub(crate) fn print_bots(&self) -> Result<()> {
        let cfg = self.require_config()?;
        for bot in read_bots(&PathBuf::from(cfg.mai_path).join("MaiBot"))? {
            println!(
                "{}  {}  {}  {}",
                bot.id,
                bot.name,
                bot.initialization,
                bot.directory.display()
            );
        }
        Ok(())
    }

    pub(crate) fn choose_tui_bot(&self) -> Result<()> {
        if self.workspace.borrow().bot_id.is_some() {
            return Ok(());
        }
        let cfg = self.load_config()?;
        if cfg.mai_path.is_empty() {
            return Ok(());
        }
        let root = PathBuf::from(cfg.mai_path).join("MaiBot");
        if !root.join("host/registry.sqlite3").is_file() {
            return Ok(());
        }
        let bots = read_bots(&root)?;
        if bots.len() > 1 {
            let labels = bots
                .iter()
                .map(|bot| format!("{} · {}", bot.name, bot.id))
                .collect::<Vec<_>>();
            let index = Select::with_theme(&self.theme)
                .with_prompt("选择要管理插件和配置的 Bot")
                .items(&labels)
                .default(0)
                .interact()?;
            self.select_bot_id(Some(bots[index].id.clone()));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    pub(crate) struct Fixture(pub(crate) PathBuf);
    impl Fixture {
        pub(crate) fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let fixture = Self(
                std::env::temp_dir()
                    .join(format!("maibot-workspace-{}-{stamp}", std::process::id())),
            );
            fs::create_dir_all(fixture.0.join("MaiBot/host")).unwrap();
            let output = Command::new(if cfg!(windows) { "python" } else { "python3" })
                .args(["-I", "-m", "venv", "--without-pip"])
                .arg(fixture.0.join("venv"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            fixture.write_registry("import sqlite3, sys\nwith sqlite3.connect(sys.argv[1]) as db:\n db.executescript(\"PRAGMA user_version = 1; CREATE TABLE bots (bot_id TEXT, name TEXT, directory TEXT, initialization TEXT, created_at TEXT);\")", &[]);
            fixture
        }
        fn write_registry(&self, script: &str, args: &[&str]) {
            let suffix = if cfg!(windows) {
                "Scripts/python.exe"
            } else {
                "bin/python"
            };
            let output = Command::new(self.0.join("venv").join(suffix))
                .args(["-I", "-S", "-c", script])
                .arg(self.0.join("MaiBot/host/registry.sqlite3"))
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        pub(crate) fn add_bot(&self, id: &str, directory: &str) -> PathBuf {
            let dir = self.0.join("MaiBot").join(directory);
            fs::create_dir_all(dir.join("config")).unwrap();
            fs::write(
                dir.join("config/bot_config.toml"),
                "[plugin]\nenabled = true\n",
            )
            .unwrap();
            self.write_registry("import sqlite3, sys\nwith sqlite3.connect(sys.argv[1]) as db:\n db.execute(\"INSERT INTO bots VALUES (?, ?, ?, 'initialized', '')\", (sys.argv[2], sys.argv[2], sys.argv[3]))", &[id, directory]);
            dir.components().collect()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn registry_controls_paths_and_multiple_bots_require_selection() {
        let f = Fixture::new();
        let workspace = f.0.join("MaiBot");
        let first = f.add_bot("one", "bots/one");
        fs::create_dir_all(workspace.join("bots/orphan/config")).unwrap();
        assert_eq!(resolve_bot(&workspace, None).unwrap().directory, first);
        let second = f.add_bot("two", "bots/two");
        assert!(resolve_bot(&workspace, None).is_err());
        assert_eq!(
            resolve_bot(&workspace, Some("two")).unwrap().directory,
            second
        );
        assert!(resolve_bot(&workspace, Some("orphan")).is_err());
        fs::remove_file(second.join("config/bot_config.toml")).unwrap();
        assert!(resolve_bot(&workspace, Some("two")).is_err());
    }

    #[test]
    fn registered_root_and_external_directories_are_current_layouts() {
        let f = Fixture::new();
        let workspace = f.0.join("MaiBot");
        let root = f.add_bot("root", ".");
        assert_eq!(
            resolve_bot(&workspace, Some("root")).unwrap().directory,
            root
        );
        let external = f.0.join("external");
        f.add_bot("external", external.to_str().unwrap());
        assert_eq!(
            resolve_bot(&workspace, Some("external")).unwrap().directory,
            external
        );
    }

    #[test]
    fn missing_registry_is_never_created() {
        let f = Fixture::new();
        fs::remove_file(f.0.join("MaiBot/host/registry.sqlite3")).unwrap();
        assert!(read_bots(&f.0.join("MaiBot")).is_err());
        assert!(!f.0.join("MaiBot/host/registry.sqlite3").exists());
    }
}
