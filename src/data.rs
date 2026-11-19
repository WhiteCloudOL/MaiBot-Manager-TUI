use anyhow::{Result, bail};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn maibot_data_dir(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("data")
}

pub(crate) fn clear_maibot_data_dir(data_dir: &Path) -> Result<usize> {
    let metadata = fs::symlink_metadata(data_dir).map_err(|error| {
        anyhow::anyhow!("无法读取 MaiBot 数据目录 {}: {error}", data_dir.display())
    })?;
    let parent = data_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("数据目录没有父目录"))?
        .canonicalize()?;
    if metadata.file_type().is_symlink()
        || data_dir.canonicalize()?.parent() != Some(parent.as_path())
    {
        bail!("数据目录不能是指向其他位置的链接: {}", data_dir.display());
    }
    if !data_dir.exists() {
        bail!("未找到 MaiBot 数据目录: {}", data_dir.display());
    }
    if !data_dir.is_dir() {
        bail!("MaiBot 数据路径不是目录: {}", data_dir.display());
    }

    let mut removed = 0;
    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        if entry.file_name() == OsStr::new("webui.json") {
            continue;
        }

        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() && !file_type.is_symlink() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn clear_maibot_data_dir_preserves_webui_json_only() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("maibot-data-clear-test-{suffix}"));
        let data_dir = maibot_data_dir(&root);
        fs::create_dir_all(data_dir.join("nested")).expect("create test data dir");
        fs::write(data_dir.join("webui.json"), "{}").expect("write webui token");
        fs::write(data_dir.join("memory.db"), "data").expect("write data file");
        fs::write(data_dir.join("nested").join("item.txt"), "data").expect("write nested file");

        let removed = clear_maibot_data_dir(&data_dir).expect("clear data dir");

        assert_eq!(removed, 2);
        assert!(data_dir.join("webui.json").exists());
        assert!(!data_dir.join("memory.db").exists());
        assert!(!data_dir.join("nested").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn clear_data_rejects_a_link_to_another_bots_data() {
        let fixture = crate::workspace::tests::Fixture::new();
        let first = fixture.add_bot("one", "bots/one");
        let second = fixture.add_bot("two", "bots/two");
        fs::create_dir_all(second.join("data")).unwrap();
        fs::write(second.join("data/memory.db"), "keep").unwrap();
        std::os::unix::fs::symlink(second.join("data"), first.join("data")).unwrap();
        assert!(clear_maibot_data_dir(&first.join("data")).is_err());
        assert_eq!(
            fs::read_to_string(second.join("data/memory.db")).unwrap(),
            "keep"
        );
    }
}
