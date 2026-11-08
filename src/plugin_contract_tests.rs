use crate::{
    app::App,
    model::{AppConfig, InstallMode},
    utils::list_plugins,
    workspace::tests::Fixture,
};
use std::{fs, path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn plugin_sync_uses_selected_bot_manifest_id_and_preserves_configuration() {
    let f = Fixture::new();
    let first = f.add_bot("one", "bots/one");
    let second = f.add_bot("two", "bots/two");
    let source = f.0.join("different-repository-name");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("_manifest.json"), r#"{"manifest_version":2,"id":"example.sample-plugin","name":"Sample","version":"1.0.0","author":{"name":"Author"}}"#).unwrap();
    fs::write(source.join(".gitignore"), "config.toml\n").unwrap();
    fs::write(
        source.join("requirements.txt"),
        "this-must-not-be-installed\n",
    )
    .unwrap();
    git(&source, &["init", "-b", "main"]);
    git(&source, &["add", "."]);
    git(
        &source,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--no-gpg-sign",
            "-m",
            "fixture",
        ],
    );
    let mut app = App::new().unwrap();
    app.config_path = f.0.join("manager-config");
    app.save_config(&AppConfig {
        mai_path: f.0.display().to_string(),
        ..Default::default()
    })
    .unwrap();
    app.select_bot_id(Some("two".into()));
    assert_eq!(app.bot_dir().unwrap(), second);
    let plugins = second.join("plugins");
    // Local Git avoids external network and exercises each platform's real shell path.
    let installed = app
        .sync_plugin_repo_with_manifest_dir(
            source.to_str().unwrap(),
            &plugins,
            "different-repository-name",
            Some("main"),
            InstallMode::Normal,
        )
        .unwrap();
    assert_eq!(installed, plugins.join("example_sample-plugin"));
    let config = "[plugin]\nenabled = false\n[client]\nport = 3123\n";
    fs::write(installed.join("config.toml"), config).unwrap();
    fs::write(source.join("feature.txt"), "updated").unwrap();
    git(&source, &["add", "."]);
    git(
        &source,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--no-gpg-sign",
            "-m",
            "update",
        ],
    );
    let updated = app
        .sync_plugin_repo_with_manifest_dir(
            source.to_str().unwrap(),
            &plugins,
            "different-repository-name",
            Some("main"),
            InstallMode::Normal,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(updated.join("config.toml")).unwrap(),
        config
    );
    assert_eq!(
        fs::read_to_string(updated.join("feature.txt")).unwrap(),
        "updated"
    );
    assert_eq!(app.read_plugin_summary(&updated).unwrap().author, "Author");
    assert!(!first.join("plugins").exists());
    for reserved in [".update_backups", "DATA", "__PYCACHE__", "empty"] {
        fs::create_dir_all(plugins.join(reserved)).unwrap();
        if reserved != "empty" {
            fs::write(plugins.join(reserved).join("_manifest.json"), "{}").unwrap();
        }
    }
    assert_eq!(list_plugins(&plugins).unwrap(), ["example_sample-plugin"]);
    // A duplicate repository-named directory must be backed up outside discovery.
    let duplicate = plugins.join("different-repository-name");
    fs::create_dir_all(&duplicate).unwrap();
    fs::write(
        duplicate.join("_manifest.json"),
        fs::read(source.join("_manifest.json")).unwrap(),
    )
    .unwrap();
    let restored = app
        .sync_plugin_repo_with_manifest_dir(
            source.to_str().unwrap(),
            &plugins,
            "different-repository-name",
            Some("main"),
            InstallMode::Normal,
        )
        .unwrap();
    assert_eq!(restored, installed);
    assert!(
        second
            .join("plugin-backups/different-repository-name.legacy-backup-1/_manifest.json")
            .is_file()
    );
    assert_eq!(list_plugins(&plugins).unwrap(), ["example_sample-plugin"]);
}

#[test]
fn data_clear_is_scoped_to_selected_bot_and_preserves_host_credentials() {
    let f = Fixture::new();
    let first = f.add_bot("one", "bots/one");
    let second = f.add_bot("two", "bots/two");
    for bot in [&first, &second] {
        fs::create_dir_all(bot.join("data/nested")).unwrap();
        fs::write(bot.join("data/memory.db"), "data").unwrap();
        fs::write(bot.join("data/webui.json"), "preserved").unwrap();
    }
    let token = f.0.join("MaiBot/host/webui.json");
    fs::write(&token, "host-credentials").unwrap();
    let mut app = App::new().unwrap();
    app.config_path = f.0.join("manager-config");
    app.save_config(&AppConfig {
        mai_path: f.0.display().to_string(),
        ..Default::default()
    })
    .unwrap();
    assert!(app.clear_maibot_data_files().is_err());
    assert!(first.join("data/memory.db").exists());
    app.select_bot_id(Some("two".into()));
    assert_eq!(app.clear_maibot_data_files().unwrap(), 2);
    assert!(first.join("data/memory.db").exists());
    assert!(second.join("data/webui.json").exists());
    assert!(!second.join("data/memory.db").exists());
    assert_eq!(fs::read_to_string(token).unwrap(), "host-credentials");
}
