use crate::{
    app::App,
    model::{
        BotProtocol, DockerMirror, GitDirtyMode, GithubFallbackMode, InstallMode, InstallPlan,
        LlbotUpdateMode, NapcatConflictMode, PythonEnv, VenvMode,
    },
    utils::{normalize_path, normalize_url},
};
use anyhow::{Result, bail};

pub(super) fn run(app: &mut App, args: &[String]) -> Result<()> {
    if args[1..]
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help" | "help"))
    {
        crate::cli::print_help();
        return Ok(());
    }

    let current = app.load_config().unwrap_or_default();
    let mut plan = app.build_default_install_plan(&current)?;
    parse_options(&mut plan, &args[1..])?;
    app.print_header(Some(&plan));
    app.run_install(&plan)
}

fn parse_options(plan: &mut InstallPlan, args: &[String]) -> Result<()> {
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--path" => {
                plan.install_path = normalize_path(value(args, idx, "--path <目录>")?)?;
                idx += 2;
            }
            "--branch" => {
                let branch = value(args, idx, "--branch <main|dev>")?;
                if !matches!(branch, "main" | "dev") {
                    bail!("--branch 只能是 main 或 dev");
                }
                plan.maibot_branch = branch.to_string();
                idx += 2;
            }
            "--mode" => {
                plan.install_mode = match value(args, idx, "--mode <normal|clean>")? {
                    "normal" => InstallMode::Normal,
                    "clean" => InstallMode::Clean,
                    other => bail!("未知安装模式: {other}"),
                };
                if plan.install_mode == InstallMode::Clean {
                    plan.venv_mode = VenvMode::Recreate;
                }
                idx += 2;
            }
            "--python" => {
                plan.python_env = match value(args, idx, "--python <system|uv>")? {
                    "system" => PythonEnv::System,
                    "uv" => PythonEnv::Uv,
                    other => bail!("未知 Python 环境: {other}"),
                };
                idx += 2;
            }
            "--venv" => {
                plan.venv_mode = match value(args, idx, "--venv <keep|recreate>")? {
                    "keep" => VenvMode::Keep,
                    "recreate" => VenvMode::Recreate,
                    other => bail!("未知虚拟环境选项: {other}"),
                };
                idx += 2;
            }
            "--github" => {
                let github = value(args, idx, "--github <auto|direct|URL>")?;
                plan.github_proxy = match github {
                    "auto" => String::new(),
                    "direct" => "https://github.com".into(),
                    other => normalize_url(other),
                };
                idx += 2;
            }
            "--pip" => {
                apply_pip(plan, value(args, idx, "--pip <system|aliyun|...|URL>")?);
                idx += 2;
            }
            "--protocol" => {
                plan.bot_protocols = match value(args, idx, "--protocol <napcat|llbot|none>")? {
                    "napcat" => vec![BotProtocol::NapCat],
                    "llbot" => vec![BotProtocol::LuckyLilliaBot],
                    "none" => Vec::new(),
                    other => bail!("未知协议端: {other}"),
                };
                idx += 2;
            }
            "--docker" => {
                plan.docker_mirror =
                    match value(args, idx, "--docker <one-ms|xuanyuan|official|keep>")? {
                        "one-ms" | "1ms" => DockerMirror::OneMs,
                        "xuanyuan" => DockerMirror::Xuanyuan,
                        "official" => DockerMirror::Official,
                        "keep" => DockerMirror::Keep,
                        other => bail!("未知 Docker 镜像选项: {other}"),
                    };
                idx += 2;
            }
            "--github-fallback" => {
                plan.github_fallback = match value(args, idx, "--github-fallback <direct|cancel>")?
                {
                    "direct" => GithubFallbackMode::Direct,
                    "cancel" => GithubFallbackMode::Cancel,
                    other => bail!("未知 GitHub 失败回退选项: {other}"),
                };
                idx += 2;
            }
            "--git-dirty" => {
                plan.git_dirty_mode = match value(args, idx, "--git-dirty <stash|discard|cancel>")?
                {
                    "stash" => GitDirtyMode::Stash,
                    "discard" => GitDirtyMode::Discard,
                    "cancel" => GitDirtyMode::Cancel,
                    other => bail!("未知 Git 本地改动处理选项: {other}"),
                };
                idx += 2;
            }
            "--napcat-conflict" => {
                plan.napcat_conflict_mode =
                    match value(args, idx, "--napcat-conflict <recreate|cancel>")? {
                        "recreate" => NapcatConflictMode::Recreate,
                        "cancel" => NapcatConflictMode::Cancel,
                        other => bail!("未知 NapCat 冲突处理选项: {other}"),
                    };
                idx += 2;
            }
            "--llbot-update" => {
                plan.llbot_update_mode = match value(args, idx, "--llbot-update <update|skip>")? {
                    "update" => LlbotUpdateMode::Update,
                    "skip" => LlbotUpdateMode::Skip,
                    other => bail!("未知 LLBot 更新选项: {other}"),
                };
                idx += 2;
            }
            other => bail!("未知安装参数: {other}"),
        }
    }
    Ok(())
}

fn value<'a>(args: &'a [String], idx: usize, label: &str) -> Result<&'a str> {
    args.get(idx + 1)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("缺少参数: {label}"))
}

fn apply_pip(plan: &mut InstallPlan, pip: &str) {
    let (display, index, host) = match pip {
        "system" => ("系统默认".to_string(), String::new(), String::new()),
        "aliyun" => (
            "阿里云".to_string(),
            "https://mirrors.aliyun.com/pypi/simple/".to_string(),
            "mirrors.aliyun.com".to_string(),
        ),
        "tencent" => (
            "腾讯云".to_string(),
            "http://mirrors.cloud.tencent.com/pypi/simple".to_string(),
            "mirrors.cloud.tencent.com".to_string(),
        ),
        "tsinghua" => (
            "清华大学".to_string(),
            "https://pypi.tuna.tsinghua.edu.cn/simple".to_string(),
            "pypi.tuna.tsinghua.edu.cn".to_string(),
        ),
        "ustc" => (
            "中国科学技术大学".to_string(),
            "https://pypi.mirrors.ustc.edu.cn/simple/".to_string(),
            "pypi.mirrors.ustc.edu.cn".to_string(),
        ),
        "official" => (
            "官方源".to_string(),
            "https://pypi.org/simple".to_string(),
            "pypi.org".to_string(),
        ),
        other => {
            let url = normalize_url(other);
            let host = url.split('/').nth(2).unwrap_or_default().to_string();
            (url.clone(), url, host)
        }
    };
    plan.pip_display = display;
    plan.pip_index = index;
    plan.pip_host = host;
    plan.uv_index = plan.pip_index.clone();
}
