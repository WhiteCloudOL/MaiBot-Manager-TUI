use std::path::PathBuf;
use std::sync::OnceLock;

pub const APP_VERSION: &str = env!("APP_VERSION");
pub const APP_BUILD_LABEL: &str = env!("APP_BUILD_LABEL");
pub const TEST_FILE_PATH: &str = env!("APP_GITHUB_TEST_PATH");
pub const DOCKER_ONELINER: &str = env!("APP_DOCKER_ONELINER");

const GITHUB_MIRRORS_RAW: &str = env!("APP_GITHUB_MIRRORS");

pub fn github_mirrors() -> &'static [&'static str] {
    static CACHE: OnceLock<Vec<&'static str>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            GITHUB_MIRRORS_RAW
                .split('|')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect()
        })
        .as_slice()
}

#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    pub user_install_path: String,
    pub mai_path: String,
    pub mai_python_env: String,
    pub mai_llbot_path: String,
    pub mai_install_mode: String,
    pub mai_venv_mode: String,
    pub maibot_branch: String,
    pub pip_display: String,
    pub pip_index: String,
    pub pip_host: String,
    pub bot_protocols: String,
}

#[derive(Clone, Debug, Default)]
pub struct InstallPlan {
    pub install_path: PathBuf,
    pub install_mode: InstallMode,
    pub python_env: PythonEnv,
    pub venv_mode: VenvMode,
    pub maibot_branch: String,
    pub github_proxy: String,
    pub pip_display: String,
    pub pip_index: String,
    pub pip_host: String,
    pub uv_index: String,
    pub bot_protocols: Vec<BotProtocol>,
    pub docker_mirror: DockerMirror,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstallMode {
    #[default]
    Normal,
    Clean,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PythonEnv {
    #[default]
    System,
    Uv,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VenvMode {
    #[default]
    Keep,
    Recreate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotProtocol {
    NapCat,
    LuckyLilliaBot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DockerMirror {
    OneMs,
    Xuanyuan,
    Official,
    #[default]
    Keep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanField {
    InstallPath,
    InstallMode,
    PythonEnv,
    VenvMode,
    MaiBotBranch,
    GithubProxy,
    PipSource,
    BotProtocols,
    DockerMirror,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanAction {
    StartInstall,
    ResetDefaults,
    BackToMenu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannerEntry {
    Field(PlanField),
    Choice(PlanField, usize),
    Action(PlanAction),
}

impl InstallMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "正常更新/修复",
            Self::Clean => "清空目录并全新安装",
        }
    }
}

impl PythonEnv {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "本机 python3",
            Self::Uv => "uv (Python 3.14)",
        }
    }
}

impl VenvMode {
    pub fn label(self, py: PythonEnv) -> &'static str {
        match (self, py) {
            (Self::Keep, PythonEnv::Uv) => "保留现有 .venv",
            (Self::Recreate, PythonEnv::Uv) => "删除并重建 .venv",
            (Self::Keep, PythonEnv::System) => "保留现有环境",
            (Self::Recreate, PythonEnv::System) => "删除并重建环境",
        }
    }
}

impl BotProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::NapCat => "NapCatQQ (Docker)",
            Self::LuckyLilliaBot => "LuckyLilliaBot (LinuxCLI)",
        }
    }
}

impl DockerMirror {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneMs => "docker.1ms.run",
            Self::Xuanyuan => "docker.xuanyuan.me",
            Self::Official => "官方源",
            Self::Keep => "保持不变",
        }
    }
}
