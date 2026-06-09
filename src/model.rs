use std::path::PathBuf;
use std::sync::OnceLock;

pub const APP_VERSION: &str = env!("APP_VERSION");
pub const APP_HEADER_TITLE: &str = env!("APP_HEADER_TITLE");
pub const APP_HEADER_SUBTITLE: &str = env!("APP_HEADER_SUBTITLE");
pub const APP_HEADER_CREDIT: &str = env!("APP_HEADER_CREDIT");
pub const APP_HEADER_DOCS: &str = env!("APP_HEADER_DOCS");
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
    pub github_fallback: GithubFallbackMode,
    pub git_dirty_mode: GitDirtyMode,
    pub napcat_conflict_mode: NapcatConflictMode,
    pub llbot_update_mode: LlbotUpdateMode,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GithubFallbackMode {
    #[default]
    Ask,
    Direct,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GitDirtyMode {
    #[default]
    Ask,
    Stash,
    Discard,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NapcatConflictMode {
    #[default]
    Ask,
    Recreate,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LlbotUpdateMode {
    #[default]
    Prompt,
    Update,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Running,
    Stopped,
    Warning,
    Neutral,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DashboardTab {
    #[default]
    Overview,
    Deploy,
    Core,
    Protocol,
    Access,
    Plugins,
    About,
}

impl DashboardTab {
    pub const SIDEBAR: [Self; 7] = [
        Self::Overview,
        Self::Deploy,
        Self::Core,
        Self::Protocol,
        Self::Plugins,
        Self::Access,
        Self::About,
    ];

    pub fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Deploy => 1,
            Self::Core => 2,
            Self::Protocol => 3,
            Self::Access => 4,
            Self::Plugins => 5,
            Self::About => 6,
        }
    }

    pub fn sidebar_index(self) -> usize {
        Self::SIDEBAR
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Deploy => "部署与更新",
            Self::Core => "核心服务管理",
            Self::Protocol => "协议端服务",
            Self::Access => "设置",
            Self::Plugins => "插件中心",
            Self::About => "关于",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppMode {
    #[default]
    Navigation,
    ContentFocused,
    PopupActive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DashboardFocus {
    #[default]
    Sidebar,
    Content,
}

#[derive(Clone, Debug, Default)]
pub struct AppState {
    pub mode: AppMode,
    pub current_tab: usize,
    pub active_tab: DashboardTab,
    pub focus: DashboardFocus,
    pub search_query: String,
    pub status_message_override: Option<String>,
    pub deploy_plan: Option<InstallPlan>,
    pub popup: Option<DashboardPopup>,
    selections: [usize; 7],
}

pub type DashboardState = AppState;

impl AppState {
    pub fn selected(&self) -> usize {
        self.selections[self.active_tab.index()]
    }

    pub fn selected_for_len(&self, len: usize) -> usize {
        if len == 0 {
            0
        } else {
            self.selected().min(len - 1)
        }
    }

    pub fn set_selected(&mut self, value: usize) {
        self.selections[self.active_tab.index()] = value;
    }

    pub fn clamp_selected(&mut self, len: usize) {
        if len == 0 {
            self.set_selected(0);
        } else if self.selected() >= len {
            self.set_selected(len - 1);
        }
    }

    pub fn move_selection(&mut self, len: usize, delta: isize) {
        if len == 0 {
            self.set_selected(0);
            return;
        }
        let current = self.selected() as isize;
        let next = (current + delta).rem_euclid(len as isize) as usize;
        self.set_selected(next);
    }

    pub fn next_tab(&mut self) {
        let idx = (self.active_tab.sidebar_index() + 1) % DashboardTab::SIDEBAR.len();
        self.active_tab = DashboardTab::SIDEBAR[idx];
        self.current_tab = idx;
    }

    pub fn prev_tab(&mut self) {
        let idx = if self.active_tab.sidebar_index() == 0 {
            DashboardTab::SIDEBAR.len() - 1
        } else {
            self.active_tab.sidebar_index() - 1
        };
        self.active_tab = DashboardTab::SIDEBAR[idx];
        self.current_tab = idx;
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            DashboardFocus::Sidebar => DashboardFocus::Content,
            DashboardFocus::Content => DashboardFocus::Sidebar,
        };
        self.mode = match self.focus {
            DashboardFocus::Sidebar => AppMode::Navigation,
            DashboardFocus::Content => AppMode::ContentFocused,
        };
    }

    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message_override = Some(message.into());
    }

    pub fn clear_status_message(&mut self) {
        self.status_message_override = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashboardPopup {
    pub title: String,
    pub subtitle: String,
    pub lines: Vec<String>,
    pub actions: Vec<String>,
    pub selected: usize,
}

#[derive(Clone, Debug)]
pub struct DashboardCard {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub detail: String,
    pub kind: StatusKind,
}

#[derive(Clone, Debug)]
pub struct DashboardChoice {
    pub label: String,
    pub detail: String,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct DashboardView {
    pub mode: AppMode,
    pub active_tab: DashboardTab,
    pub focus: DashboardFocus,
    pub popup: Option<DashboardPopup>,
    pub page_title: String,
    pub detail_title: String,
    pub detail_subtitle: String,
    pub detail_lines: Vec<String>,
    pub detail_choices: Vec<DashboardChoice>,
    pub action_lines: Vec<String>,
    pub cards: Vec<DashboardCard>,
    pub selected: usize,
    pub empty_title: String,
    pub empty_detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardEvent {
    Activate,
    ClearSearch,
    Exit,
    ResetDeployPlan,
    RunDeployPlan,
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

pub fn deploy_card_field(id: &str) -> Option<PlanField> {
    match id {
        "deploy-path" => Some(PlanField::InstallPath),
        "deploy-branch" => Some(PlanField::MaiBotBranch),
        "deploy-mode" => Some(PlanField::InstallMode),
        "deploy-python" => Some(PlanField::PythonEnv),
        "deploy-venv" => Some(PlanField::VenvMode),
        "deploy-github" => Some(PlanField::GithubProxy),
        "deploy-pypi" => Some(PlanField::PipSource),
        "deploy-bots" => Some(PlanField::BotProtocols),
        "deploy-docker" => Some(PlanField::DockerMirror),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannerEntry {
    Field(PlanField),
    Choice(PlanField, usize),
    Action(PlanAction),
}

impl PlannerEntry {
    pub fn field(&self) -> Option<PlanField> {
        match self {
            Self::Field(field) | Self::Choice(field, _) => Some(*field),
            Self::Action(_) => None,
        }
    }
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
            Self::NapCat => "NapCatQQ",
            Self::LuckyLilliaBot => "LuckyLilliaBot",
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
