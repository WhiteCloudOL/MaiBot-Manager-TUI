use anyhow::{Error, Result, anyhow, bail};
use std::{
    cell::RefCell,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::model::{
    DashboardCard, DashboardChoice, DashboardEvent, DashboardFocus, DashboardPopup, DashboardState,
    DashboardTab, DashboardView, InstallPlan, PlanField, StatusKind, deploy_card_field,
};
use crate::plugin_status::PluginUpdateCache;
use crate::plugins::windows_plugin_update_status;
use crate::theme::AppTheme;
use crate::ui::{ActionItem, StatusCard, planner_choices_for_plan};
use crate::utils::list_plugins;

pub(crate) struct App {
    pub(crate) theme: AppTheme,
    pub(crate) config_path: PathBuf,
    pub(crate) cli_mode: bool,
    dashboard_cache: RefCell<DashboardRuntimeCache>,
}

#[derive(Clone, Debug, Default)]
struct DashboardRuntimeSnapshot {
    config: crate::model::AppConfig,
    has_config: bool,
    maibot_running: bool,
    napcat_running: bool,
    llbot_running: bool,
    napcat_installed: bool,
    llbot_installed: bool,
    plugin_count: usize,
}

#[derive(Debug, Default)]
struct DashboardRuntimeCache {
    snapshot: Option<DashboardRuntimeSnapshot>,
    refreshed_at: Option<Instant>,
    plugin_cards: Option<Vec<DashboardCard>>,
    plugin_cards_refreshed_at: Option<Instant>,
    plugin_update_cache: PluginUpdateCache,
}

const DASHBOARD_STATUS_TTL: Duration = Duration::from_secs(10);
const LOG_SUMMARY_TAIL_BYTES: u64 = 64 * 1024;

impl App {
    pub(crate) fn new() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位用户目录"))?;
        Ok(Self {
            theme: AppTheme::new(),
            config_path: home.join(".maibot_config"),
            cli_mode: false,
            dashboard_cache: RefCell::new(DashboardRuntimeCache::default()),
        })
    }

    pub(crate) fn set_cli_mode(&mut self) {
        self.cli_mode = true;
    }

    fn dashboard_snapshot(&self) -> DashboardRuntimeSnapshot {
        {
            let cache = self.dashboard_cache.borrow();
            if let (Some(snapshot), Some(refreshed_at)) =
                (cache.snapshot.as_ref(), cache.refreshed_at)
                && refreshed_at.elapsed() < DASHBOARD_STATUS_TTL
            {
                return snapshot.clone();
            }
        }

        let config = self.load_config().unwrap_or_default();
        let has_config = !config.mai_path.is_empty();
        let root = PathBuf::from(&config.mai_path);
        let napcat_installed = has_config && root.join("NapCat").exists();
        let llbot_installed =
            has_config && (!config.mai_llbot_path.is_empty() || root.join("LLBot").exists());
        let plugin_count = if has_config {
            list_plugins(&root.join("MaiBot").join("plugins"))
                .map(|items| items.len())
                .unwrap_or(0)
        } else {
            0
        };
        let snapshot = DashboardRuntimeSnapshot {
            config,
            has_config,
            maibot_running: has_config && self.maibot_core_running().unwrap_or(false),
            napcat_running: has_config && self.napcat_running().unwrap_or(false),
            llbot_running: has_config && self.llbot_running().unwrap_or(false),
            napcat_installed,
            llbot_installed,
            plugin_count,
        };
        let mut cache = self.dashboard_cache.borrow_mut();
        cache.snapshot = Some(snapshot.clone());
        cache.refreshed_at = Some(Instant::now());
        snapshot
    }

    fn invalidate_dashboard_cache(&self) {
        let mut cache = self.dashboard_cache.borrow_mut();
        cache.snapshot = None;
        cache.refreshed_at = None;
        cache.plugin_cards = None;
        cache.plugin_cards_refreshed_at = None;
        cache.plugin_update_cache.clear();
    }

    fn recover_dashboard_error(&self, state: &mut DashboardState, error: &Error) {
        let mut lines = vec![format!("原因: {error}")];
        lines.extend(
            error
                .chain()
                .skip(1)
                .take(3)
                .map(|cause| format!("详情: {cause}")),
        );
        state.active_tab = DashboardTab::Overview;
        state.current_tab = DashboardTab::Overview.sidebar_index();
        state.focus = DashboardFocus::Sidebar;
        state.mode = crate::model::AppMode::Navigation;
        state.search_query.clear();
        state.set_selected(0);
        state.popup = Some(DashboardPopup {
            title: "任务执行失败".to_string(),
            subtitle: "已返回主界面；请根据原因修正后重试".to_string(),
            lines,
            actions: vec!["返回主界面".to_string()],
            selected: 0,
            scroll: 0,
        });
        state.set_status_message("任务失败，已返回主界面");
        self.invalidate_dashboard_cache();
    }

    pub(crate) fn run(&mut self) -> Result<()> {
        let mut dashboard = DashboardState::default();
        dashboard.focus = DashboardFocus::Sidebar;
        let current = self.load_config().unwrap_or_default();
        dashboard.deploy_plan = Some(self.build_default_install_plan(&current)?);
        loop {
            let event = self.dashboard_event_loop(
                &mut dashboard,
                |state| self.build_dashboard_view(state),
                |state, view| self.open_inline_dashboard_info_popup(state, view),
            )?;
            let event_result = (|| -> Result<bool> {
                match event {
                    DashboardEvent::ClearSearch => {
                        if !dashboard.search_query.is_empty() {
                            dashboard.search_query.clear();
                            let len = self
                                .dashboard_cards(&dashboard.active_tab, &dashboard.search_query)?
                                .len();
                            dashboard.clamp_selected(len);
                        }
                    }
                    DashboardEvent::Activate => {
                        dashboard.clear_status_message();
                        if matches!(dashboard.focus, DashboardFocus::Sidebar) {
                            dashboard.focus = DashboardFocus::Content;
                        } else if !self.activate_dashboard_selection(&mut dashboard)? {
                            return Ok(false);
                        }
                    }
                    DashboardEvent::CommitDeployChoice { field, choice_idx } => {
                        let current = self.load_config().unwrap_or_default();
                        let mut plan = dashboard.deploy_plan.take().unwrap_or_else(|| {
                            self.build_default_install_plan(&current)
                                .unwrap_or_else(|_| self.build_recommended_defaults())
                        });
                        self.apply_planner_choice(&current, &mut plan, field, choice_idx)?;
                        dashboard.deploy_plan = Some(plan);
                        dashboard.commit_deploy_choice_selection(field, choice_idx);
                        dashboard.set_status_message("部署选项已确认");
                    }
                    DashboardEvent::ResetDeployPlan => {
                        let reset = self.build_recommended_defaults();
                        dashboard.deploy_plan = Some(reset);
                        dashboard.reset_deploy_choice_selections();
                        dashboard.set_status_message("已恢复推荐默认部署配置");
                    }
                    DashboardEvent::RunDeployPlan => {
                        let plan = if let Some(plan) = dashboard.deploy_plan.as_ref() {
                            plan.clone()
                        } else {
                            let current = self.load_config().unwrap_or_default();
                            self.build_default_install_plan(&current)?
                        };
                        self.handle_menu_result(self.run_install(&plan))?;
                        dashboard.deploy_plan = Some(plan);
                        dashboard.set_status_message("安装流程已执行");
                        self.invalidate_dashboard_cache();
                    }
                    DashboardEvent::AttachTerminal { .. } => {
                        // Not supported on this platform — ignore
                    }
                    DashboardEvent::Exit => return Ok(false),
                }
                Ok(true)
            })();
            match event_result {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => self.recover_dashboard_error(&mut dashboard, &error),
            }
        }
        Ok(())
    }

    fn build_dashboard_view(&self, state: &DashboardState) -> Result<DashboardView> {
        let mut cards = match state.active_tab {
            DashboardTab::Deploy => {
                let plan = if let Some(plan) = state.deploy_plan.as_ref() {
                    plan.clone()
                } else {
                    let current = self.load_config().unwrap_or_default();
                    self.build_default_install_plan(&current)?
                };
                filter_cards(self.deploy_cards_from_plan(&plan), &state.search_query)
            }
            _ => self.dashboard_cards(&state.active_tab, &state.search_query)?,
        };
        let selected = state.selected_for_len(cards.len());
        let selected_card = cards.get(selected).cloned();
        let (page_title, _, _, _, detail_title, detail_subtitle) =
            self.dashboard_headers(state.active_tab, selected_card.as_ref());
        let detail_lines =
            self.dashboard_detail_lines(state, state.active_tab, selected_card.as_ref())?;
        let detail_choices =
            self.dashboard_detail_choices(state, cards.get(selected).map(|card| card.id))?;
        let action_lines = self.dashboard_action_lines(state.active_tab, selected_card.as_ref());
        Ok(DashboardView {
            mode: state.mode,
            active_tab: state.active_tab,
            focus: state.focus,
            popup: state.popup.clone(),
            page_title: page_title.to_string(),
            detail_title: detail_title.to_string(),
            detail_subtitle: detail_subtitle.to_string(),
            detail_lines,
            detail_choices,
            action_lines,
            cards: std::mem::take(&mut cards),
            selected,
            background_refresh: self.dashboard_background_refresh(state.active_tab),
            empty_title: "没有匹配项".to_string(),
            empty_detail: "试试清空筛选，或先完成部署与安装。".to_string(),
        })
    }

    fn dashboard_background_refresh(&self, tab: DashboardTab) -> bool {
        tab == DashboardTab::Plugins
            && self
                .dashboard_cache
                .borrow()
                .plugin_update_cache
                .is_scanning()
    }

    fn dashboard_headers<'a>(
        &self,
        tab: DashboardTab,
        selected: Option<&'a DashboardCard>,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'a str,
        &'a str,
    ) {
        match tab {
            DashboardTab::Overview => (
                "系统概览",
                "快速查看核心服务、协议端和插件健康状态。",
                "服务卡片",
                "服务与模块",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("服务详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("选择左侧卡片查看状态、日志和下一步动作。"),
            ),
            DashboardTab::Deploy => (
                "部署与更新",
                "围绕安装规划器组织部署流程和常用入口。",
                "部署步骤",
                "把高频流程收拢到一个上下文面板",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("部署详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("路径、分支与安装模式集中在此处调整。"),
            ),
            DashboardTab::Core => (
                "核心服务管理",
                "聚焦 MaiBot 核心进程、控制台与日志入口。",
                "核心动作",
                "启动、停止、控制台与日志",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("核心详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("启动、停止和日志入口都保留现有逻辑。"),
            ),
            DashboardTab::Protocol => (
                "协议端服务",
                "NapCat Shell 与 LuckyLilliaBot Desktop 统一收纳。",
                "协议端面板",
                "先看状态，再进入细项维护",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("协议端详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("支持按服务状态快速定位问题。"),
            ),
            DashboardTab::Access => (
                "访问配置",
                "集中查看 WebUI、令牌和 Adapter 策略入口。",
                "访问任务",
                "把常用访问相关操作集中起来",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("访问详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("WebUI、令牌与 Adapter 策略入口。"),
            ),
            DashboardTab::Plugins => (
                "插件中心",
                "以插件健康度和维护任务为中心组织操作。",
                "插件视图",
                "插件状态与管理项",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("插件详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("安装、更新与卸载集中管理。"),
            ),
            DashboardTab::About => (
                "关于",
                "版本、文档、作者与许可信息。",
                "信息面板",
                "清晰查看软件信息",
                selected.map(|card| card.title.as_str()).unwrap_or("关于"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("可在这里确认版本、文档和当前平台。"),
            ),
        }
    }

    fn dashboard_cards(&self, tab: &DashboardTab, search: &str) -> Result<Vec<DashboardCard>> {
        let cards = match tab {
            DashboardTab::Overview => self.overview_cards()?,
            DashboardTab::Deploy => self.deploy_cards()?,
            DashboardTab::Core => self.core_cards()?,
            DashboardTab::Protocol => self.protocol_cards()?,
            DashboardTab::Access => self.access_cards()?,
            DashboardTab::Plugins => self.plugin_cards()?,
            DashboardTab::About => self.about_cards(),
        };
        Ok(filter_cards(cards, search))
    }

    fn overview_cards(&self) -> Result<Vec<DashboardCard>> {
        let snapshot = self.dashboard_snapshot();
        if !snapshot.has_config {
            return Ok(vec![DashboardCard {
                id: "workspace",
                icon: "󰙅",
                title: "尚未部署 MaiBot".to_string(),
                subtitle: "从部署与更新开始".to_string(),
                badge: "未初始化".to_string(),
                detail: "当前还没有 .maibot_config 工作区。".to_string(),
                kind: StatusKind::Warning,
            }]);
        }
        let maibot_running = snapshot.maibot_running;
        let llbot_running = snapshot.llbot_running;
        let napcat_running = snapshot.napcat_running;
        let napcat_installed = snapshot.napcat_installed;
        let llbot_installed = snapshot.llbot_installed;
        let plugin_count = snapshot.plugin_count;
        Ok(vec![
            DashboardCard {
                id: "maibot",
                icon: "󱄩",
                title: "MaiBot Core".to_string(),
                subtitle: if maibot_running {
                    "独立控制台运行中".to_string()
                } else {
                    "核心服务当前未运行".to_string()
                },
                badge: if maibot_running {
                    "运行中"
                } else {
                    "已停止"
                }
                .to_string(),
                detail: "支持启动、停止与日志查看。".to_string(),
                kind: if maibot_running {
                    StatusKind::Running
                } else {
                    StatusKind::Stopped
                },
            },
            DashboardCard {
                id: "napcat",
                icon: "󰘨",
                title: "NapCatQQ".to_string(),
                subtitle: if napcat_running && !napcat_installed {
                    "Shell 进程运行中，配置未记录目录".to_string()
                } else if napcat_installed {
                    if napcat_running {
                        "NapCat Shell 已运行".to_string()
                    } else {
                        "已安装，等待启动".to_string()
                    }
                } else {
                    "部署计划中可启用".to_string()
                },
                badge: if napcat_running {
                    "运行中"
                } else if napcat_installed {
                    "待启动"
                } else {
                    "未安装"
                }
                .to_string(),
                detail: "协议端日志、重建和 Shell 启动入口。".to_string(),
                kind: if napcat_running && napcat_installed {
                    StatusKind::Running
                } else if napcat_running {
                    StatusKind::Warning
                } else if napcat_installed {
                    StatusKind::Stopped
                } else {
                    StatusKind::Neutral
                },
            },
            DashboardCard {
                id: "llbot",
                icon: "󰀻",
                title: "LuckyLilliaBot".to_string(),
                subtitle: if llbot_running && !llbot_installed {
                    "Desktop 进程运行中，配置未记录目录".to_string()
                } else if llbot_installed {
                    if llbot_running {
                        "Desktop 进程已运行".to_string()
                    } else {
                        "已安装，等待启动".to_string()
                    }
                } else {
                    "部署计划中可启用".to_string()
                },
                badge: if llbot_running {
                    "运行中"
                } else if llbot_installed {
                    "待启动"
                } else {
                    "未安装"
                }
                .to_string(),
                detail: "密码、日志和 Desktop 协议端控制入口。".to_string(),
                kind: if llbot_running && llbot_installed {
                    StatusKind::Running
                } else if llbot_running {
                    StatusKind::Warning
                } else if llbot_installed {
                    StatusKind::Stopped
                } else {
                    StatusKind::Neutral
                },
            },
            DashboardCard {
                id: "plugins",
                icon: "󰏗",
                title: "插件中心".to_string(),
                subtitle: format!("已检测到 {plugin_count} 个插件"),
                badge: if plugin_count > 0 {
                    "可维护"
                } else {
                    "空目录"
                }
                .to_string(),
                detail: "安装、更新和卸载都从这里进入。".to_string(),
                kind: if plugin_count > 0 {
                    StatusKind::Neutral
                } else {
                    StatusKind::Warning
                },
            },
        ])
    }

    fn deploy_cards(&self) -> Result<Vec<DashboardCard>> {
        let current = self.load_config().unwrap_or_default();
        let plan = self.build_default_install_plan(&current)?;
        Ok(self.deploy_cards_from_plan(&plan))
    }

    fn core_cards(&self) -> Result<Vec<DashboardCard>> {
        let running = self.dashboard_snapshot().maibot_running;
        Ok(vec![
            DashboardCard {
                id: "core-start",
                icon: "󰐊",
                title: "启动 MaiBot".to_string(),
                subtitle: if running {
                    "服务已运行，可重新聚焦启动窗口".to_string()
                } else {
                    "打开独立控制台并启动核心服务".to_string()
                },
                badge: if running { "已运行" } else { "可启动" }.to_string(),
                detail: "首次启动/EULA 会在独立窗口中确认。".to_string(),
                kind: StatusKind::Running,
            },
            DashboardCard {
                id: "core-stop",
                icon: "󰓛",
                title: "停止 MaiBot".to_string(),
                subtitle: "读取 PID 并结束完整进程树".to_string(),
                badge: if running { "可停止" } else { "未运行" }.to_string(),
                detail: "适合服务无响应或需要热重启时使用。".to_string(),
                kind: if running {
                    StatusKind::Warning
                } else {
                    StatusKind::Stopped
                },
            },
            DashboardCard {
                id: "core-console",
                icon: "󰆍",
                title: "控制台窗口".to_string(),
                subtitle: "独立窗口承载交互，当前不支持附着".to_string(),
                badge: if running { "说明" } else { "未启动" }.to_string(),
                detail: "保持真实平台能力，不伪装为可附着控制台。".to_string(),
                kind: if running {
                    StatusKind::Neutral
                } else {
                    StatusKind::Stopped
                },
            },
            DashboardCard {
                id: "core-logs",
                icon: "󰘷",
                title: "查看实时日志".to_string(),
                subtitle: "跟随 logs/maibot.log 输出".to_string(),
                badge: "诊断".to_string(),
                detail: "适合确认启动过程、排错和观察运行状态。".to_string(),
                kind: StatusKind::Neutral,
            },
        ])
    }

    fn protocol_cards(&self) -> Result<Vec<DashboardCard>> {
        let snapshot = self.dashboard_snapshot();
        let napcat_running = snapshot.napcat_running;
        let napcat_installed = snapshot.napcat_installed;
        let llbot_running = snapshot.llbot_running;
        let llbot_installed = snapshot.llbot_installed;
        Ok(vec![
            DashboardCard {
                id: "napcat",
                icon: "󰘨",
                title: "NapCatQQ".to_string(),
                subtitle: if napcat_running && !napcat_installed {
                    "Shell 运行中 · 配置待同步".to_string()
                } else if napcat_installed {
                    "Windows NapCat Shell".to_string()
                } else {
                    "尚未安装".to_string()
                },
                badge: if napcat_running {
                    "运行中"
                } else if napcat_installed {
                    "待启动"
                } else {
                    "未安装"
                }
                .to_string(),
                detail: "适合查看进程状态、日志和重下载操作。".to_string(),
                kind: if napcat_running && napcat_installed {
                    StatusKind::Running
                } else if napcat_running {
                    StatusKind::Warning
                } else if napcat_installed {
                    StatusKind::Stopped
                } else {
                    StatusKind::Neutral
                },
            },
            DashboardCard {
                id: "llbot",
                icon: "󰀻",
                title: "LuckyLilliaBot".to_string(),
                subtitle: if llbot_running && !llbot_installed {
                    "Desktop 运行中 · 配置待同步".to_string()
                } else if llbot_installed {
                    "Desktop 版协议端".to_string()
                } else {
                    "尚未安装".to_string()
                },
                badge: if llbot_running {
                    "运行中"
                } else if llbot_installed {
                    "待启动"
                } else {
                    "未安装"
                }
                .to_string(),
                detail: "密码、日志和 Desktop 控制入口。".to_string(),
                kind: if llbot_running && llbot_installed {
                    StatusKind::Running
                } else if llbot_running {
                    StatusKind::Warning
                } else if llbot_installed {
                    StatusKind::Stopped
                } else {
                    StatusKind::Neutral
                },
            },
        ])
    }

    fn access_cards(&self) -> Result<Vec<DashboardCard>> {
        let snapshot = self.dashboard_snapshot();
        Ok(vec![
            DashboardCard {
                id: "access-summary",
                icon: "󰢹",
                title: "访问汇总".to_string(),
                subtitle: "MaiBot / NapCat / LLBot WebUI".to_string(),
                badge: if !snapshot.has_config {
                    "未配置"
                } else {
                    "可查看"
                }
                .to_string(),
                detail: "公网 IP、端口和访问令牌会在详情中统一汇总。".to_string(),
                kind: if !snapshot.has_config {
                    StatusKind::Warning
                } else {
                    StatusKind::Neutral
                },
            },
            DashboardCard {
                id: "access-init",
                icon: "󰑮",
                title: "初始化远程访问".to_string(),
                subtitle: "绑定 IPv4/IPv6 全地址并启用 Adapter".to_string(),
                badge: "初始化".to_string(),
                detail: "适合首次部署后快速打开 WebUI 和 Adapter 外部访问。".to_string(),
                kind: StatusKind::Running,
            },
            DashboardCard {
                id: "access-clear-data",
                icon: "󰆴",
                title: "清空数据文件".to_string(),
                subtitle: "保留 webui.json，清理 MaiBot/data".to_string(),
                badge: "需确认".to_string(),
                detail: "删除 MaiBot/data 下除 webui.json 外的文件和子目录。".to_string(),
                kind: StatusKind::Warning,
            },
            DashboardCard {
                id: "adapter",
                icon: "󰙨",
                title: "Adapter 策略".to_string(),
                subtitle: "黑白名单与初始化配置".to_string(),
                badge: "安全".to_string(),
                detail: "适合先完成初始化，再维护群聊和私聊名单。".to_string(),
                kind: StatusKind::Neutral,
            },
        ])
    }

    fn plugin_cards(&self) -> Result<Vec<DashboardCard>> {
        {
            self.dashboard_cache
                .borrow_mut()
                .plugin_update_cache
                .drain();
        }
        {
            let cache = self.dashboard_cache.borrow();
            if let (Some(cards), Some(refreshed_at)) =
                (cache.plugin_cards.as_ref(), cache.plugin_cards_refreshed_at)
                && refreshed_at.elapsed() < DASHBOARD_STATUS_TTL
            {
                return Ok(cards.clone());
            }
        }

        let snapshot = self.dashboard_snapshot();
        let cfg = &snapshot.config;
        if !snapshot.has_config {
            return Ok(vec![DashboardCard {
                id: "plugins-empty",
                icon: "󰏓",
                title: "插件中心待启用".to_string(),
                subtitle: "先部署 MaiBot 工作区".to_string(),
                badge: "未初始化".to_string(),
                detail: "部署完成后这里会显示插件列表和维护入口。".to_string(),
                kind: StatusKind::Warning,
            }]);
        }

        let plugins_dir = PathBuf::from(&cfg.mai_path).join("MaiBot").join("plugins");
        let plugins = list_plugins(&plugins_dir).unwrap_or_default();
        let update_jobs = plugins
            .iter()
            .map(|plugin| (plugin.clone(), plugins_dir.join(plugin)))
            .filter(|(_, dir)| dir.join(".git").exists())
            .collect::<Vec<_>>();
        {
            self.dashboard_cache
                .borrow_mut()
                .plugin_update_cache
                .begin_scan(
                    Path::new(&cfg.mai_path),
                    update_jobs,
                    windows_plugin_update_status,
                );
        }
        let mut cards = vec![DashboardCard {
            id: "plugin-center",
            icon: "󰏗",
            title: "插件管理".to_string(),
            subtitle: "安装、更新与卸载".to_string(),
            badge: format!("{} 个插件", plugins.len()),
            detail: "集中安装、更新与卸载插件。".to_string(),
            kind: StatusKind::Neutral,
        }];
        for plugin in plugins {
            let plugin_dir = plugins_dir.join(&plugin);
            let summary = self.read_plugin_summary(&plugin_dir).ok();
            let update_status = self
                .dashboard_cache
                .borrow()
                .plugin_update_cache
                .status_for(&plugin, plugin_dir.join(".git").exists());
            cards.push(DashboardCard {
                id: "plugin-item",
                icon: "󰐱",
                title: summary
                    .as_ref()
                    .map(|summary| summary.name.clone())
                    .unwrap_or(plugin.clone()),
                subtitle: summary
                    .as_ref()
                    .map(|summary| format!("{} · {}", summary.version, summary.author))
                    .unwrap_or_else(|| "已安装插件 · 可在插件中心维护".to_string()),
                badge: update_status,
                detail: summary
                    .as_ref()
                    .map(|summary| summary.description.clone())
                    .unwrap_or_else(|| "支持更新、卸载和目录规范化。".to_string()),
                kind: StatusKind::Running,
            });
        }
        let mut cache = self.dashboard_cache.borrow_mut();
        cache.plugin_cards = Some(cards.clone());
        cache.plugin_cards_refreshed_at = None;
        Ok(cards)
    }

    fn about_cards(&self) -> Vec<DashboardCard> {
        vec![
            DashboardCard {
                id: "version",
                icon: "󰎆",
                title: "MaiBot Manager".to_string(),
                subtitle: format!("版本 {}", crate::model::APP_VERSION),
                badge: "版本".to_string(),
                detail: "用于安装、更新和管理 MaiBot。".to_string(),
                kind: StatusKind::Neutral,
            },
            DashboardCard {
                id: "docs",
                icon: "󰈙",
                title: "帮助与文档".to_string(),
                subtitle: crate::model::APP_HEADER_DOCS.to_string(),
                badge: "文档".to_string(),
                detail: "查看使用说明、安装指引和常见问题。".to_string(),
                kind: StatusKind::Neutral,
            },
            DashboardCard {
                id: "credits",
                icon: "󰨔",
                title: "作者与许可".to_string(),
                subtitle: crate::model::APP_HEADER_CREDIT.to_string(),
                badge: "许可".to_string(),
                detail: "感谢使用 MaiBot Manager。".to_string(),
                kind: StatusKind::Neutral,
            },
            DashboardCard {
                id: "platform",
                icon: "󰍹",
                title: "当前平台".to_string(),
                subtitle: "Windows 10/11".to_string(),
                badge: "平台".to_string(),
                detail: "支持独立窗口运行和图形化协议端管理。".to_string(),
                kind: StatusKind::Neutral,
            },
        ]
    }

    fn dashboard_detail_lines(
        &self,
        state: &DashboardState,
        tab: DashboardTab,
        selected: Option<&DashboardCard>,
    ) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        if let Some(card) = selected {
            lines.push(format!("状态: {}", card.badge));
            lines.push(format!("摘要: {}", card.detail));
        }
        match tab {
            DashboardTab::Overview => {
                let snapshot = self.dashboard_snapshot();
                let cfg = snapshot.config;
                if !snapshot.has_config {
                    lines.push("尚未检测到 ~/.maibot_config。".to_string());
                    lines.push("建议先切换到「部署与更新」并完成安装规划。".to_string());
                } else {
                    lines.push(format!("工作区: {}", cfg.mai_path));
                    lines.push(format!("主程序分支: {}", cfg.maibot_branch));
                    lines.push(format!("Python 环境: {}", cfg.mai_python_env));
                    if let Some(card) = selected {
                        match card.id {
                            "maibot" => {
                                let pid_path =
                                    PathBuf::from(&cfg.mai_path).join("logs").join("maibot.pid");
                                let log_path =
                                    PathBuf::from(&cfg.mai_path).join("logs").join("maibot.log");
                                lines.push(format!(
                                    "PID 文件: {}",
                                    if pid_path.exists() {
                                        "已存在"
                                    } else {
                                        "未写入"
                                    }
                                ));
                                lines.push(format!(
                                    "日志文件: {}",
                                    if log_path.exists() {
                                        "已存在"
                                    } else {
                                        "尚未生成"
                                    }
                                ));
                                lines.push("控制方式: 独立控制台运行，可从管理器停止".to_string());
                                lines.push("常用动作: 启动 / 停止 / 查看实时日志".to_string());
                                if let Some(snippet) = read_log_summary(&log_path, 2) {
                                    lines.push(format!("日志摘要: {snippet}"));
                                }
                            }
                            "napcat" => {
                                let log_path = PathBuf::from(&cfg.mai_path)
                                    .join("NapCat")
                                    .join("logs")
                                    .join("onebot.log");
                                lines.push("协议端类型: NapCat Shell".to_string());
                                lines.push("日志路径: NapCat/logs/onebot.log".to_string());
                                lines.push("常用动作: 启动 / 停止 / 重启 / 重下载".to_string());
                                if let Some(snippet) = read_log_summary(&log_path, 1) {
                                    lines.push(format!("日志摘要: {snippet}"));
                                }
                            }
                            "llbot" => {
                                let log_path = if cfg.mai_llbot_path.is_empty() {
                                    PathBuf::from(&cfg.mai_path).join("LLBot").join("llbot.log")
                                } else {
                                    PathBuf::from(&cfg.mai_llbot_path).join("llbot.log")
                                };
                                lines.push("协议端类型: LuckyLilliaBot Desktop".to_string());
                                lines.push("日志路径: LLBot/llbot.log".to_string());
                                lines.push("常用动作: 启动 / 停止 / 修改 WebUI 密码".to_string());
                                if let Some(snippet) = read_log_summary(&log_path, 1) {
                                    lines.push(format!("日志摘要: {snippet}"));
                                }
                            }
                            "plugins" => {
                                lines.push("插件目录会按 manifest id 自动规范化命名。".to_string());
                                lines.push("可在插件中心直接执行更新或卸载。".to_string());
                            }
                            _ => {}
                        }
                    }
                }
            }
            DashboardTab::Deploy => {
                lines.push("当前表单会实时组成下一次安装或更新计划。".to_string());
                lines.push("路径项可打开输入框，其余配置会即时更新到当前部署计划。".to_string());
                if let (Some(plan), Some(card)) = (state.deploy_plan.as_ref(), selected)
                    && let Some(field) = deploy_card_field(card.id)
                {
                    let choices = self.planner_choices(plan, field);
                    if !choices.is_empty() {
                        lines.push("可选项:".to_string());
                        for choice in choices.into_iter().take(4) {
                            lines.push(format!(" - {choice}"));
                        }
                    }
                }
            }
            DashboardTab::Core => {
                let running = self.dashboard_snapshot().maibot_running;
                lines.push(if running {
                    "󱄩 当前状态: 运行中".to_string()
                } else {
                    "󱄩 当前状态: 已停止".to_string()
                });
                lines.push("Windows 核心服务使用独立控制台启动。".to_string());
                lines.push("管理器通过 logs/maibot.pid 停止完整进程树。".to_string());
                if let Ok(cfg) = self.require_config() {
                    lines.push(format!("工作区: {}", cfg.mai_path));
                    lines.push(format!(
                        "日志: {}",
                        PathBuf::from(&cfg.mai_path)
                            .join("logs")
                            .join("maibot.log")
                            .display()
                    ));
                    lines.push(format!(
                        "PID: {}",
                        PathBuf::from(&cfg.mai_path)
                            .join("logs")
                            .join("maibot.pid")
                            .display()
                    ));
                }
                if let Some(card) = selected {
                    match card.id {
                        "core-start" => {
                            lines.push("动作说明: 打开独立控制台并启动核心服务。".to_string())
                        }
                        "core-stop" => {
                            lines.push("动作说明: 读取 PID 并结束完整进程树。".to_string())
                        }
                        "core-console" => lines.push(
                            "动作说明: Windows 仅保留独立控制台窗口，不支持附着。".to_string(),
                        ),
                        "core-logs" => {
                            lines.push("动作说明: 跟随 maibot.log 观察启动与运行输出。".to_string())
                        }
                        _ => {}
                    }
                }
            }
            DashboardTab::Protocol => {
                let snapshot = self.dashboard_snapshot();
                let cfg = snapshot.config;
                if let Some(card) = selected {
                    match card.id {
                        "napcat" => {
                            lines.push(format!(
                                "目录: {}",
                                PathBuf::from(&cfg.mai_path).join("NapCat").display()
                            ));
                            lines.push(format!(
                                "运行状态: {}",
                                if snapshot.napcat_running {
                                    "NapCat Shell 正在运行"
                                } else {
                                    "NapCat Shell 未运行"
                                }
                            ));
                            lines.push("日志: NapCat/logs/onebot.log".to_string());
                            lines.push("运行模型: Windows Shell 版，不使用 Docker。".to_string());
                        }
                        "llbot" => {
                            let llbot_dir = if cfg.mai_llbot_path.is_empty() {
                                PathBuf::from(&cfg.mai_path).join("LLBot")
                            } else {
                                PathBuf::from(&cfg.mai_llbot_path)
                            };
                            lines.push(format!("目录: {}", llbot_dir.display()));
                            lines.push(format!(
                                "运行状态: {}",
                                if snapshot.llbot_running {
                                    "LLBot Desktop 正在运行"
                                } else {
                                    "LLBot Desktop 未运行"
                                }
                            ));
                            lines.push("日志: LLBot/llbot.log".to_string());
                            lines.push("运行模型: Desktop 版，不使用 CLI zip。".to_string());
                        }
                        _ => {}
                    }
                } else {
                    lines.push("NapCat 使用 Windows Shell 版，不使用 Docker。".to_string());
                    lines.push("LLBot 使用 Desktop 版，不使用 CLI zip。".to_string());
                }
            }
            DashboardTab::Access => {
                let cfg = self.dashboard_snapshot().config;
                if let Some(card) = selected {
                    match card.id {
                        "access-summary" => {
                            lines.push(format!(
                                "MaiBot 配置: {}",
                                PathBuf::from(&cfg.mai_path)
                                    .join("MaiBot")
                                    .join("config")
                                    .join("bot_config.toml")
                                    .display()
                            ));
                            lines.push(format!(
                                "MaiBot token: {}",
                                PathBuf::from(&cfg.mai_path)
                                    .join("MaiBot")
                                    .join("data")
                                    .join("webui.json")
                                    .display()
                            ));
                            lines.push("完整访问汇总可查看 URL 与访问令牌。".to_string());
                        }
                        "access-init" => {
                            lines.push(
                                "会把 MaiBot WebUI host 改为 [\"0.0.0.0\", \"::\"]。".to_string(),
                            );
                            lines.push("会同时启用 NapCat Adapter 插件。".to_string());
                            lines.push("执行后需要重启 MaiBot 才会完全生效。".to_string());
                        }
                        "access-clear-data" => {
                            lines.push(format!(
                                "目标目录: {}",
                                PathBuf::from(&cfg.mai_path)
                                    .join("MaiBot")
                                    .join("data")
                                    .display()
                            ));
                            lines.push("会保留 webui.json，删除其余文件和子目录。".to_string());
                            lines.push("按 Enter 后会先弹出确认对话框。".to_string());
                        }
                        "adapter" => {
                            lines.push("可维护群聊白名单/黑名单、私聊名单和封禁 QQ。".to_string());
                            lines
                                .push("配置文件: MaiBot/plugins/<adapter>/config.toml".to_string());
                            lines.push("黑白名单编辑面板会维护群聊、私聊与封禁名单。".to_string());
                        }
                        _ => {}
                    }
                } else {
                    lines.push(
                        "初始化会把 WebUI 绑定到 IPv4/IPv6 全地址，并启用 Adapter。".to_string(),
                    );
                }
            }
            DashboardTab::Plugins => {
                let snapshot = self.dashboard_snapshot();
                if let Some(card) = selected {
                    lines.push(format!("插件项: {}", card.title));
                    if snapshot.has_config {
                        let plugins_dir = PathBuf::from(&snapshot.config.mai_path)
                            .join("MaiBot")
                            .join("plugins");
                        if let Some(dir) =
                            self.find_plugin_dir_by_card_title(&plugins_dir, &card.title)
                            && let Ok(summary) = self.read_plugin_summary(&dir)
                        {
                            lines.push(format!("ID: {}", summary.id));
                            lines.push(format!("作者: {}", summary.author));
                            lines.push(format!("版本: {}", summary.version));
                            lines.push(format!("更新状态: {}", card.badge));
                            lines.push(format!("目录名: {}", summary.dir_name));
                        }
                    }
                }
                lines.push("插件目录按 _manifest.json 中的 id 规范化命名。".to_string());
                lines.push("插件更新会拉取仓库最新提交。".to_string());
                if snapshot.has_config {
                    lines.push(format!(
                        "目录: {}",
                        PathBuf::from(snapshot.config.mai_path)
                            .join("MaiBot")
                            .join("plugins")
                            .display()
                    ));
                }
            }
            DashboardTab::About => {
                lines.push(format!("应用: {}", crate::model::APP_HEADER_TITLE));
                lines.push(format!("版本: {}", crate::model::APP_VERSION));
                lines.push(format!("说明: {}", crate::model::APP_HEADER_SUBTITLE));
                lines.push(crate::model::APP_HEADER_CREDIT.to_string());
                lines.push(crate::model::APP_HEADER_DOCS.to_string());
                lines.push("平台: Windows 10/11".to_string());
            }
        }
        Ok(lines)
    }

    fn dashboard_action_lines(
        &self,
        tab: DashboardTab,
        selected: Option<&DashboardCard>,
    ) -> Vec<String> {
        let mut actions = Vec::new();
        match tab {
            DashboardTab::Overview => {
                if let Some(card) = selected {
                    actions.push(format!("打开 {}", card.title));
                }
                actions.push("服务与模块索引".to_string());
            }
            DashboardTab::Deploy => {
                actions.push("当前配置会组成下一次安装或更新计划".to_string());
                actions.push("恢复默认会回到推荐路径、环境与协议端组合".to_string());
            }
            DashboardTab::Core => {
                actions.push("当前动作块会复用核心服务逻辑".to_string());
                if let Some(card) = selected {
                    match card.id {
                        "core-start" => {
                            actions.push("将打开独立控制台并沿用现有启动流程".to_string())
                        }
                        "core-stop" => actions.push("将读取 PID 文件并结束整个进程树".to_string()),
                        "core-console" => actions.push("将提示独立控制台无法直接附着".to_string()),
                        "core-logs" => {
                            actions.push("将直接进入 maibot.log 实时跟随视图".to_string())
                        }
                        _ => {}
                    }
                }
                actions.push("启动 / 停止 / 控制台说明 / 日志 已原生接入当前面板".to_string());
            }
            DashboardTab::Protocol => {
                if let Some(card) = selected {
                    match card.id {
                        "napcat" => {
                            actions.push("NapCat 管理支持启动、停止与日志查看".to_string());
                            actions.push("支持启动 / 停止 / 重启 / 日志 / 重下载".to_string());
                        }
                        "llbot" => {
                            actions.push("LLBot 管理支持日志与密码维护".to_string());
                            actions.push("支持启动 / 停止 / 重启 / 日志 / 密码修改".to_string());
                        }
                        _ => {
                            actions.push("协议端服务菜单包含平台专属操作".to_string());
                        }
                    }
                } else {
                    actions.push("协议端服务菜单包含平台专属操作".to_string());
                }
            }
            DashboardTab::Access => {
                if let Some(card) = selected {
                    match card.id {
                        "access-summary" => {
                            actions.push("完整访问信息包含 WebUI 地址与令牌".to_string());
                            actions.push("可确认 WebUI 地址、端口和 token".to_string());
                        }
                        "access-init" => {
                            actions.push("初始化远程访问会写入 WebUI/Adapter 配置".to_string());
                            actions.push("会提示确认后写入 WebUI/Adapter 配置".to_string());
                        }
                        "access-clear-data" => {
                            actions.push("清理前会弹出确认对话框".to_string());
                            actions.push("仅保留 MaiBot/data/webui.json".to_string());
                        }
                        "adapter" => {
                            actions.push("黑白名单策略编辑会维护群聊与私聊名单".to_string());
                            actions.push("可维护群聊、私聊与封禁名单".to_string());
                        }
                        _ => {
                            actions.push("访问配置菜单包含地址、令牌与策略操作".to_string());
                        }
                    }
                } else {
                    actions.push("访问配置菜单包含地址、令牌与策略操作".to_string());
                }
            }
            DashboardTab::Plugins => {
                if let Some(card) = selected {
                    if card.id == "plugin-item" {
                        actions.push("插件维护动作包含更新与卸载".to_string());
                        actions.push("可执行更新或卸载".to_string());
                    } else {
                        actions.push("完整插件中心包含安装、更新与卸载".to_string());
                    }
                } else {
                    actions.push("完整插件中心包含安装、更新与卸载".to_string());
                }
                actions.push("插件名称与 manifest 摘要可用于定位".to_string());
                actions.push("右侧面板现在会显示 manifest 摘要与依赖状态".to_string());
            }
            DashboardTab::About => {
                actions.push("只读信息页".to_string());
                actions.push("版本、文档、作者与许可信息".to_string());
            }
        }
        actions
    }

    fn dashboard_detail_choices(
        &self,
        state: &DashboardState,
        selected_id: Option<&str>,
    ) -> Result<Vec<DashboardChoice>> {
        if state.active_tab != DashboardTab::Deploy {
            return Ok(Vec::new());
        }
        let Some(card_id) = selected_id else {
            return Ok(Vec::new());
        };
        let Some(plan) = state.deploy_plan.as_ref() else {
            return Ok(Vec::new());
        };
        let Some(field) = deploy_card_field(card_id) else {
            return Ok(Vec::new());
        };
        let choices = planner_choices_for_plan(plan, field);
        Ok(choices
            .into_iter()
            .enumerate()
            .map(|(idx, label)| DashboardChoice {
                detail: deploy_choice_detail(field, idx, &label),
                active: self.planner_choice_active(plan, field, idx),
                selected: false,
                label,
            })
            .collect())
    }

    fn open_inline_dashboard_info_popup(
        &self,
        state: &mut DashboardState,
        view: &DashboardView,
    ) -> Result<bool> {
        let Some(card) = view.cards.get(view.selected) else {
            return Ok(false);
        };
        match (view.active_tab, card.id) {
            (DashboardTab::Access, "access-summary") => {
                state.popup = Some(self.dashboard_access_summary_popup());
                state.set_status_message("已生成访问汇总");
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn activate_dashboard_selection(&mut self, state: &mut DashboardState) -> Result<bool> {
        if let Some(popup) = state.popup.take() {
            return self.activate_dashboard_popup(state, popup.selected);
        }
        match state.active_tab {
            DashboardTab::Overview => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                match selected.map(|card| card.id) {
                    Some("maibot") => {
                        self.handle_menu_result(self.manage_maibot_menu())?;
                        self.invalidate_dashboard_cache();
                    }
                    Some("napcat") => {
                        self.handle_menu_result(self.manage_napcat_menu())?;
                        self.invalidate_dashboard_cache();
                    }
                    Some("llbot") => {
                        self.handle_menu_result(self.manage_llbot_menu())?;
                        self.invalidate_dashboard_cache();
                    }
                    Some("plugins") => {
                        self.handle_menu_result(self.manage_plugins_menu())?;
                        self.invalidate_dashboard_cache();
                    }
                    Some("workspace") => {
                        let result = self.install_update_flow();
                        self.handle_menu_result(result)?;
                        self.invalidate_dashboard_cache();
                    }
                    _ => {}
                };
            }
            DashboardTab::Deploy => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                if let Some(plan) = state.deploy_plan.as_ref() {
                    let Some(field) = selected.and_then(|card| deploy_card_field(card.id)) else {
                        return Ok(true);
                    };
                    if field == PlanField::InstallPath {
                        let mut new_plan = plan.clone();
                        self.edit_install_path(&mut new_plan)?;
                        state.deploy_plan = Some(new_plan.clone());
                        state.set_status_message(format!(
                            "目录已更新为 {}",
                            new_plan.install_path.display()
                        ));
                    }
                }
            }
            DashboardTab::Core => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                match selected.map(|card| card.id) {
                    Some("core-start") => {
                        self.handle_menu_result(self.start_maibot_core(false))?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已请求启动 MaiBot 核心");
                    }
                    Some("core-stop") => {
                        self.handle_menu_result(self.stop_maibot_core())?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已请求停止 MaiBot 核心");
                    }
                    Some("core-console") => {
                        self.handle_menu_result(self.attach_screen("maibot"))?;
                        state.set_status_message("Windows 无法附着已开窗口，已显示说明");
                    }
                    Some("core-logs") => {
                        self.handle_menu_result(self.print_maibot_core_logs(100, true))?;
                        state.set_status_message("已打开 MaiBot 实时日志");
                    }
                    _ => {
                        self.handle_menu_result(self.manage_maibot_menu())?;
                    }
                }
            }
            DashboardTab::Protocol => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                match selected.map(|card| card.id) {
                    Some("napcat") => {
                        self.handle_menu_result(self.manage_napcat_menu())?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已打开 NapCat 管理面板");
                    }
                    Some("llbot") => {
                        self.handle_menu_result(self.manage_llbot_menu())?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已打开 LLBot 管理面板");
                    }
                    _ => {
                        self.handle_menu_result(self.manage_bot_protocol_menu())?;
                        self.invalidate_dashboard_cache();
                    }
                }
            }
            DashboardTab::Access => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                match selected.map(|card| card.id) {
                    Some("access-summary") => {
                        state.popup = Some(self.dashboard_access_summary_popup());
                        state.set_status_message("已生成访问汇总");
                    }
                    Some("access-init") => {
                        self.handle_menu_result(self.initialize_maibot_access_config())?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已执行访问初始化流程");
                    }
                    Some("access-clear-data") => {
                        if let Some(card) = selected {
                            state.popup = Some(DashboardPopup {
                                title: card.title.clone(),
                                subtitle: card.subtitle.clone(),
                                lines: vec![
                                    format!("状态: {}", card.badge),
                                    card.detail.clone(),
                                    "会保留 MaiBot/data/webui.json。".to_string(),
                                    "确认后会删除其余文件和子目录。".to_string(),
                                ],
                                actions: vec!["确认清空数据".to_string(), "取消".to_string()],
                                selected: 0,
                                scroll: 0,
                            });
                        }
                    }
                    Some("adapter") => {
                        self.handle_menu_result(self.modify_adapter_config())?;
                        state.set_status_message("已打开 Adapter 策略编辑");
                    }
                    _ => {
                        self.handle_menu_result(self.manage_config_access_menu())?;
                    }
                }
            }
            DashboardTab::Plugins => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                match selected.map(|card| card.id) {
                    Some("plugin-item") => {
                        self.handle_menu_result(self.manage_plugin_from_dashboard(state))?;
                        self.invalidate_dashboard_cache();
                    }
                    _ => {
                        self.handle_menu_result(self.manage_plugins_menu())?;
                        self.invalidate_dashboard_cache();
                    }
                }
            }
            DashboardTab::About => {
                state.set_status_message("这里是只读信息页");
            }
        }
        Ok(true)
    }

    fn activate_dashboard_popup(
        &mut self,
        state: &mut DashboardState,
        action_idx: usize,
    ) -> Result<bool> {
        let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
        let selected = cards.get(state.selected_for_len(cards.len()));
        match state.active_tab {
            DashboardTab::Overview => {
                match selected.map(|card| card.id) {
                    Some("maibot") => state.active_tab = DashboardTab::Core,
                    Some("napcat") | Some("llbot") => state.active_tab = DashboardTab::Protocol,
                    Some("plugins") => state.active_tab = DashboardTab::Plugins,
                    Some("workspace") => state.active_tab = DashboardTab::Deploy,
                    _ => {}
                }
                state.focus = DashboardFocus::Content;
            }
            DashboardTab::Core => match selected.map(|card| card.id) {
                Some("core-start") if action_idx == 0 => {
                    self.handle_menu_result(self.start_maibot_core(false))?;
                    self.invalidate_dashboard_cache();
                    state.set_status_message("已请求启动 MaiBot 核心");
                }
                Some("core-stop") if action_idx == 0 => {
                    self.handle_menu_result(self.stop_maibot_core())?;
                    self.invalidate_dashboard_cache();
                    state.set_status_message("已请求停止 MaiBot 核心");
                }
                Some("core-console") if action_idx == 0 => {
                    self.handle_menu_result(self.manage_maibot_menu())?;
                }
                Some("core-logs") if action_idx == 0 => {
                    self.handle_menu_result(self.print_maibot_core_logs(100, true))?;
                    state.set_status_message("已打开 MaiBot 实时日志");
                }
                _ if action_idx == 1 => {
                    self.handle_menu_result(self.manage_maibot_menu())?;
                    self.invalidate_dashboard_cache();
                }
                _ => {}
            },
            DashboardTab::Protocol => {
                match selected.map(|card| card.id) {
                    Some("napcat") => match action_idx {
                        0 => {
                            self.handle_menu_result(self.start_napcat())?;
                        }
                        1 => {
                            self.handle_menu_result(self.stop_napcat())?;
                        }
                        2 => {
                            self.handle_menu_result(self.print_napcat_logs(100, true))?;
                        }
                        3 => {
                            self.handle_menu_result(self.manage_napcat_menu())?;
                        }
                        _ => {}
                    },
                    Some("llbot") => match action_idx {
                        0 => {
                            self.handle_menu_result(self.start_llbot())?;
                        }
                        1 => {
                            self.handle_menu_result(self.stop_llbot())?;
                        }
                        2 => {
                            self.handle_menu_result(self.print_llbot_logs(100, true))?;
                        }
                        3 => {
                            self.handle_menu_result(self.manage_llbot_menu())?;
                        }
                        _ => {}
                    },
                    _ => {}
                }
                self.invalidate_dashboard_cache();
                state.set_status_message("协议端操作已执行");
            }
            DashboardTab::Plugins => match selected.map(|card| card.id) {
                Some("plugin-item") => {
                    self.manage_plugin_action_from_dashboard(state, action_idx)?;
                    self.invalidate_dashboard_cache();
                }
                _ if action_idx == 0 => {
                    self.handle_menu_result(self.manage_plugins_menu())?;
                    self.invalidate_dashboard_cache();
                }
                _ => {}
            },
            DashboardTab::Access => match selected.map(|card| card.id) {
                Some("access-summary") if action_idx == 0 => {
                    state.popup = Some(self.dashboard_access_summary_popup());
                    state.set_status_message("已生成访问汇总");
                }
                Some("access-init") if action_idx == 0 => {
                    self.handle_menu_result(self.initialize_maibot_access_config())?;
                    self.invalidate_dashboard_cache();
                }
                Some("access-clear-data") if action_idx == 0 => {
                    match self.clear_maibot_data_files() {
                        Ok(removed) => {
                            state.set_status_message(format!("已清理 {removed} 个数据条目"));
                            self.invalidate_dashboard_cache();
                        }
                        Err(error) => return Err(error),
                    }
                }
                Some("adapter") if action_idx == 0 => {
                    self.handle_menu_result(self.modify_adapter_config())?;
                }
                _ => {}
            },
            DashboardTab::About | DashboardTab::Deploy => {}
        }
        Ok(true)
    }

    fn find_plugin_dir_by_card_title(&self, plugins_dir: &Path, title: &str) -> Option<PathBuf> {
        let plugins = list_plugins(plugins_dir).ok()?;
        for plugin in plugins {
            let dir = plugins_dir.join(&plugin);
            if let Ok(summary) = self.read_plugin_summary(&dir) {
                if summary.name == title || summary.id == title || summary.dir_name == title {
                    return Some(dir);
                }
            } else if plugin == title {
                return Some(dir);
            }
        }
        None
    }

    fn manage_plugin_from_dashboard(&self, state: &mut DashboardState) -> Result<()> {
        let cfg = self.require_config()?;
        let plugins_dir = PathBuf::from(&cfg.mai_path).join("MaiBot").join("plugins");
        let cards = self.dashboard_cards(&DashboardTab::Plugins, &state.search_query)?;
        let Some(card) = cards.get(state.selected_for_len(cards.len())) else {
            return Ok(());
        };
        let dir = match self.find_plugin_dir_by_card_title(&plugins_dir, &card.title) {
            Some(dir) => dir,
            None => bail!("未找到插件目录: {}", card.title),
        };
        let summary = self.read_plugin_summary(&dir)?;
        let actions = [
            ActionItem::primary("更新插件", "拉取该插件仓库的最新提交"),
            ActionItem::destructive("卸载插件", "删除该插件目录"),
            ActionItem::back("返回", "回到插件中心"),
        ];
        let choice = self.select_action(&format!("管理插件：{}", summary.name), &actions)?;
        let result = match choice {
            0 => self.update_plugin(&summary.dir_name),
            1 => self.remove_plugin(&summary.dir_name),
            _ => Ok(()),
        };
        if self.handle_menu_result(result)? {
            state.set_status_message(format!("插件 {} 操作已执行", summary.name));
        }
        Ok(())
    }

    fn manage_plugin_action_from_dashboard(
        &self,
        state: &mut DashboardState,
        action_idx: usize,
    ) -> Result<()> {
        let cfg = self.require_config()?;
        let plugins_dir = PathBuf::from(&cfg.mai_path).join("MaiBot").join("plugins");
        let cards = self.dashboard_cards(&DashboardTab::Plugins, &state.search_query)?;
        let Some(card) = cards.get(state.selected_for_len(cards.len())) else {
            return Ok(());
        };
        let dir = match self.find_plugin_dir_by_card_title(&plugins_dir, &card.title) {
            Some(dir) => dir,
            None => bail!("未找到插件目录: {}", card.title),
        };
        let summary = self.read_plugin_summary(&dir)?;
        let result = match action_idx {
            0 => self.update_plugin(&summary.dir_name),
            1 => self.remove_plugin(&summary.dir_name),
            _ => Ok(()),
        };
        if self.handle_menu_result(result)? {
            state.set_status_message(format!("插件 {} 操作已执行", summary.name));
        }
        Ok(())
    }

    fn deploy_cards_from_plan(&self, plan: &InstallPlan) -> Vec<DashboardCard> {
        let mut cards = Vec::new();
        for (idx, field) in deploy_fields().iter().enumerate() {
            cards.push(DashboardCard {
                id: match field {
                    PlanField::InstallPath => "deploy-path",
                    PlanField::MaiBotBranch => "deploy-branch",
                    PlanField::InstallMode => "deploy-mode",
                    PlanField::PythonEnv => "deploy-python",
                    PlanField::VenvMode => "deploy-venv",
                    PlanField::GithubProxy => "deploy-github",
                    PlanField::PipSource => "deploy-pypi",
                    PlanField::BotProtocols => "deploy-bots",
                    PlanField::DockerMirror => "deploy-docker",
                },
                icon: match idx {
                    0 => "󰉋",
                    1 => "󰘬",
                    2 => "󰙨",
                    3 => "󰌠",
                    4 => "󰆍",
                    5 => "󰊤",
                    6 => "󰏗",
                    _ => "󰘨",
                },
                title: self.planner_field_label(*field).to_string(),
                subtitle: self.planner_field_value(plan, *field),
                badge: if *field == PlanField::InstallPath {
                    "输入路径"
                } else {
                    "单选"
                }
                .to_string(),
                detail: planner_field_detail(*field).to_string(),
                kind: StatusKind::Neutral,
            });
        }
        cards
    }

    pub(crate) fn handle_menu_result(&self, result: Result<()>) -> Result<bool> {
        result.map(|()| true)
    }

    #[allow(dead_code)]
    fn print_runtime_status(&self) {
        let cfg = match self.load_config() {
            Ok(cfg) if !cfg.mai_path.is_empty() => cfg,
            _ => {
                self.print_empty_state(
                    "未检测到 MaiBot 工作区",
                    "从「部署与更新」开始，完成后这里会显示服务健康状态。",
                );
                return;
            }
        };
        let maibot_running = self.maibot_core_running().unwrap_or(false);
        let llbot_running = self.llbot_running().unwrap_or(false);
        let napcat_running = self.napcat_running().unwrap_or(false);
        let mut cards = Vec::new();
        cards.push(if maibot_running {
            StatusCard::running("MaiBot", "独立 Windows 控制台 · PID 文件可停止进程树")
        } else {
            StatusCard::stopped("MaiBot", "核心控制台未运行")
        });
        cards.push(if PathBuf::from(&cfg.mai_path).join("NapCat").exists() {
            if napcat_running {
                StatusCard::running("NapCatQQ", "NapCat Shell 窗口/进程已运行")
            } else {
                StatusCard::stopped("NapCatQQ", "已安装，NapCat Shell 未运行")
            }
        } else {
            StatusCard::neutral("NapCatQQ", "未安装", "可在部署计划中启用")
        });
        cards.push(
            if !cfg.mai_llbot_path.is_empty() || PathBuf::from(&cfg.mai_path).join("LLBot").exists()
            {
                if llbot_running {
                    StatusCard::running("LuckyLilliaBot", "Desktop 进程已运行")
                } else {
                    StatusCard::stopped("LuckyLilliaBot", "已安装，Desktop 进程未运行")
                }
            } else {
                StatusCard::neutral("LuckyLilliaBot", "未安装", "可在部署计划中启用")
            },
        );
        self.print_status_cards("服务概览", &cards);
    }
}

fn filter_cards(cards: Vec<DashboardCard>, search: &str) -> Vec<DashboardCard> {
    let needle = search.trim().to_lowercase();
    if needle.is_empty() {
        return cards;
    }
    cards
        .into_iter()
        .filter(|card| {
            let hay = format!(
                "{} {} {} {} {}",
                card.title, card.subtitle, card.badge, card.detail, card.id
            )
            .to_lowercase();
            hay.contains(&needle)
        })
        .collect()
}

fn deploy_fields() -> &'static [PlanField] {
    &[
        PlanField::InstallPath,
        PlanField::MaiBotBranch,
        PlanField::InstallMode,
        PlanField::PythonEnv,
        PlanField::VenvMode,
        PlanField::GithubProxy,
        PlanField::PipSource,
        PlanField::BotProtocols,
    ]
}

fn planner_field_detail(field: PlanField) -> &'static str {
    match field {
        PlanField::InstallPath => "MaiBot 工作区路径；首次会自动创建目录。",
        PlanField::MaiBotBranch => "在稳定版 main 和预览版 dev 之间切换。",
        PlanField::InstallMode => "决定是修复更新还是清空目录后全新安装。",
        PlanField::PythonEnv => "选择本机 Python 或由 uv 管理的独立环境。",
        PlanField::VenvMode => "控制是否保留现有虚拟环境，Clean 模式会强制重建。",
        PlanField::GithubProxy => "切换 GitHub 直连、自动测速或镜像源。",
        PlanField::PipSource => "设置 pip / uv 使用的 PyPI 镜像源。",
        PlanField::BotProtocols => "选择 NapCatQQ、LuckyLilliaBot 或暂不安装协议端。",
        PlanField::DockerMirror => "Windows 当前不使用 Docker。",
    }
}

fn deploy_choice_detail(field: PlanField, idx: usize, label: &str) -> String {
    match field {
        PlanField::InstallPath => "打开路径输入框".to_string(),
        PlanField::MaiBotBranch => {
            if idx == 0 {
                "推荐稳定环境使用。".to_string()
            } else {
                "适合跟进新功能和预发布改动。".to_string()
            }
        }
        PlanField::InstallMode => {
            if idx == 0 {
                "保留现有工作区并执行更新/修复。".to_string()
            } else {
                "清空目标目录后重新部署。".to_string()
            }
        }
        PlanField::PythonEnv => {
            if idx == 0 {
                "使用本机 Python 解释器。".to_string()
            } else {
                "由 uv 管理隔离环境与 Python 版本。".to_string()
            }
        }
        PlanField::VenvMode => {
            if label.contains("固定") {
                "由当前安装模式自动锁定。".to_string()
            } else if idx == 0 {
                "尽量复用现有环境，减少重装时间。".to_string()
            } else {
                "重建环境以消除历史依赖残留。".to_string()
            }
        }
        PlanField::GithubProxy => {
            if idx == 0 {
                "执行时测速后自动选最快线路。".to_string()
            } else if idx == 1 {
                "直接访问官方 GitHub。".to_string()
            } else if label.contains("自定义") {
                "选择后会提示输入自定义镜像地址。".to_string()
            } else {
                "切换到预设 GitHub 镜像源。".to_string()
            }
        }
        PlanField::PipSource => {
            if idx == 0 {
                "保持系统默认 Python 包源。".to_string()
            } else if label.contains("自定义") {
                "选择后会提示输入自定义 PyPI 地址。".to_string()
            } else {
                "为 pip 和 uv 设置统一镜像源。".to_string()
            }
        }
        PlanField::BotProtocols => match idx {
            0 => "默认推荐，启用 NapCatQQ Shell。".to_string(),
            1 => "切换到 LuckyLilliaBot Desktop。".to_string(),
            _ => "暂不安装附加协议端。".to_string(),
        },
        PlanField::DockerMirror => "Windows 协议端不使用 Docker。".to_string(),
    }
}

fn read_log_summary(path: &PathBuf, lines: usize) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(LOG_SUMMARY_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let summary = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(lines)
        .collect::<Vec<_>>();
    if summary.is_empty() {
        None
    } else {
        Some(summary.into_iter().rev().collect::<Vec<_>>().join(" | "))
    }
}
