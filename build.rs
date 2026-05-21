use std::fs;
use std::path::Path;

fn main() {
    let path = Path::new("app.toml");
    println!("cargo:rerun-if-changed=app.toml");
    println!("cargo:rerun-if-changed=build.rs");

    let content = fs::read_to_string(path).expect("无法读取 app.toml");
    let doc: toml_edit::DocumentMut = content.parse().expect("app.toml 不是合法 TOML");

    let version = doc
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string();
    let build_label = doc
        .get("build_label")
        .and_then(|v| v.as_str())
        .unwrap_or("Rust · MaiBot管理面板")
        .to_string();
    let github_test_path = doc
        .get("github_test_path")
        .and_then(|v| v.as_str())
        .expect("app.toml 缺少 github_test_path")
        .to_string();
    let docker_oneliner = doc
        .get("docker_oneliner")
        .and_then(|v| v.as_str())
        .unwrap_or("bash <(curl -sSL https://linuxmirrors.cn/docker.sh)")
        .to_string();

    let mirrors: Vec<String> = doc
        .get("github_mirrors")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    // Cargo 的 cargo:rustc-env 指令按行解析，值中带换行只会留住第一行。
    // 用管道符当分隔（URL 不会出现 '|'），运行时再按 '|' 切回字符串列表。
    let mirrors_joined = mirrors.join("|");

    println!("cargo:rustc-env=APP_VERSION={}", version);
    println!("cargo:rustc-env=APP_BUILD_LABEL={}", build_label);
    println!("cargo:rustc-env=APP_GITHUB_TEST_PATH={}", github_test_path);
    println!("cargo:rustc-env=APP_DOCKER_ONELINER={}", docker_oneliner);
    println!("cargo:rustc-env=APP_GITHUB_MIRRORS={}", mirrors_joined);
}
