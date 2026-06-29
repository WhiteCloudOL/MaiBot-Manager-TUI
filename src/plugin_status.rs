use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

pub(crate) const PLUGIN_UPDATE_PENDING: &str = "检查中";
pub(crate) const PLUGIN_UPDATE_NON_GIT: &str = "非 Git 仓库";

type PluginUpdateResult = (String, String);

#[derive(Default)]
pub(crate) struct PluginUpdateCache {
    statuses: HashMap<String, String>,
    scan_key: Option<String>,
    receiver: Option<Receiver<PluginUpdateResult>>,
}

impl std::fmt::Debug for PluginUpdateCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginUpdateCache")
            .field("statuses", &self.statuses)
            .field("scan_key", &self.scan_key)
            .field("has_receiver", &self.receiver.is_some())
            .finish()
    }
}

impl PluginUpdateCache {
    pub(crate) fn clear(&mut self) {
        self.statuses.clear();
        self.scan_key = None;
        self.receiver = None;
    }

    pub(crate) fn drain(&mut self) {
        let Some(receiver) = self.receiver.take() else {
            return;
        };
        let mut keep_receiver = true;
        loop {
            match receiver.try_recv() {
                Ok((name, status)) => {
                    self.statuses.insert(name, status);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    keep_receiver = false;
                    break;
                }
            }
        }
        if keep_receiver {
            self.receiver = Some(receiver);
        }
    }

    pub(crate) fn begin_scan(
        &mut self,
        root: &Path,
        jobs: Vec<(String, PathBuf)>,
        checker: fn(PathBuf, PathBuf) -> String,
    ) {
        let scan_key = scan_key(root, &jobs);
        if self.scan_key.as_deref() == Some(scan_key.as_str()) {
            return;
        }

        self.statuses.clear();
        self.scan_key = Some(scan_key);
        if jobs.is_empty() {
            self.receiver = None;
            return;
        }

        let root = root.to_path_buf();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for (name, dir) in jobs {
                let status = checker(root.clone(), dir);
                if sender.send((name, status)).is_err() {
                    break;
                }
            }
        });
        self.receiver = Some(receiver);
    }

    pub(crate) fn status_for(&self, plugin: &str, is_git_repo: bool) -> String {
        if !is_git_repo {
            return PLUGIN_UPDATE_NON_GIT.to_string();
        }
        self.statuses
            .get(plugin)
            .cloned()
            .unwrap_or_else(|| PLUGIN_UPDATE_PENDING.to_string())
    }

    pub(crate) fn is_scanning(&self) -> bool {
        self.receiver.is_some()
    }
}

fn scan_key(root: &Path, jobs: &[(String, PathBuf)]) -> String {
    let mut key = root.display().to_string();
    for (name, dir) in jobs {
        key.push('\n');
        key.push_str(name);
        key.push('\t');
        key.push_str(&dir.display().to_string());
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    fn slow_status(_root: PathBuf, _dir: PathBuf) -> String {
        thread::sleep(Duration::from_millis(80));
        "已最新".to_string()
    }

    #[test]
    fn plugin_update_scan_reports_pending_without_blocking() {
        let root = PathBuf::from("root");
        let mut cache = PluginUpdateCache::default();
        cache.begin_scan(
            &root,
            vec![("demo".to_string(), root.join("plugins").join("demo"))],
            slow_status,
        );

        assert_eq!(cache.status_for("demo", true), PLUGIN_UPDATE_PENDING);
        thread::sleep(Duration::from_millis(120));
        cache.drain();
        assert_eq!(cache.status_for("demo", true), "已最新");
    }

    #[test]
    fn non_git_plugins_do_not_wait_for_update_checks() {
        let cache = PluginUpdateCache::default();
        assert_eq!(cache.status_for("local", false), PLUGIN_UPDATE_NON_GIT);
    }
}
