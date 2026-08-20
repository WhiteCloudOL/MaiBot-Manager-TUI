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
use crate::plugins::macos_plugin_update_status;
use crate::theme::AppTheme;
use crate::ui::{ActionItem, StatusCard, planner_choices_for_plan};
use crate::utils::{list_plugins, pid_running};

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
    plugin_count: usize,
}

#[derive(Debug, Default)]
struct DashboardRuntimeCache {
    snapshot: Option<DashboardRuntimeSnapshot>,
    snapshot_refreshed_at: Option<Instant>,
    plugin_cards: Option<Vec<DashboardCard>>,
    plugin_cards_refreshed_at: Option<Instant>,
    plugin_update_cache: PluginUpdateCache,
}

const DASHBOARD_STATUS_TTL: Duration = Duration::from_secs(10);
const LOG_SUMMARY_TAIL_BYTES: u64 = 64 * 1024;

impl App {
    pub(crate) fn new() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位 HOME 目录"))?;
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
                (cache.snapshot.as_ref(), cache.snapshot_refreshed_at)
                && refreshed_at.elapsed() < DASHBOARD_STATUS_TTL
            {
                return snapshot.clone();
            }
        }

        let config = self.load_config().unwrap_or_default();
        let has_config = !config.mai_path.is_empty();
        let root = PathBuf::from(&config.mai_path);
        let pid_path = root.join("logs").join("maibot.pid");
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
            maibot_running: has_config && pid_running(&pid_path).unwrap_or(None).is_some(),
            plugin_count,
        };
        let mut cache = self.dashboard_cache.borrow_mut();
        cache.snapshot = Some(snapshot.clone());
        cache.snapshot_refreshed_at = Some(Instant::now());
        snapshot
    }

    fn invalidate_dashboard_cache(&self) {
        let mut cache = self.dashboard_cache.borrow_mut();
        cache.snapshot = None;
        cache.snapshot_refreshed_at = None;
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
                "快速查看核心服务、访问配置和插件健康状态。",
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
                "聚焦 MaiBot 核心进程、日志与启停控制。",
                "核心动作",
                "启动、停止、交互终端与日志",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("核心详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("启动、停止和日志入口都保留现有逻辑。"),
            ),
            DashboardTab::Protocol => (
                "协议端服务",
                "macOS 版目前提供协议端说明入口。",
                "协议端面板",
                "先看状态，再进入细项说明",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("协议端详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("清晰标注当前平台可用范围。"),
            ),
            DashboardTab::Access => (
                "访问配置",
                "集中查看 WebUI、令牌和初始化入口。",
                "访问任务",
                "把常用访问相关操作集中起来",
                selected
                    .map(|card| card.title.as_str())
                    .unwrap_or("访问详情"),
                selected
                    .map(|card| card.subtitle.as_str())
                    .unwrap_or("WebUI、令牌与初始化入口。"),
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
        let plugin_count = snapshot.plugin_count;
        Ok(vec![
            DashboardCard {
                id: "maibot",
                icon: "󱄩",
                title: "MaiBot Core".to_string(),
                subtitle: if maibot_running {
                    "后台子进程正在运行".to_string()
                } else {
                    "核心服务当前未运行".to_string()
                },
                badge: if maibot_running {
                    "运行中"
                } else {
                    "已停止"
                }
                .to_string(),
                detail: "支持后台启动、交互启动和日志跟随。".to_string(),
                kind: if maibot_running {
                    StatusKind::Running
                } else {
                    StatusKind::Stopped
                },
            },
            DashboardCard {
                id: "protocol",
                icon: "󰀪",
                title: "协议端服务".to_string(),
                subtitle: "NapCat / LLBot 当前仅提供说明入口".to_string(),
                badge: "说明".to_string(),
                detail: "保留入口用于查看当前平台能力范围。".to_string(),
                kind: StatusKind::Warning,
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
                    "服务已运行，可查看日志或切换交互模式".to_string()
                } else {
                    "启动后台子进程或打开交互终端".to_string()
                },
                badge: if running { "已运行" } else { "可启动" }.to_string(),
                detail: "首次启动/EULA 可用 Terminal.app 交互模式完成。".to_string(),
                kind: StatusKind::Running,
            },
            DashboardCard {
                id: "core-stop",
                icon: "󰓛",
                title: "停止 MaiBot".to_string(),
                subtitle: "结束后台进程组".to_string(),
                badge: if running { "可停止" } else { "未运行" }.to_string(),
                detail: "会先发 TERM，再在必要时升级为 KILL。".to_string(),
                kind: if running {
                    StatusKind::Warning
                } else {
                    StatusKind::Stopped
                },
            },
            DashboardCard {
                id: "core-console",
                icon: "󰆍",
                title: "打开交互终端".to_string(),
                subtitle: "Terminal.app 中完成首次启动或 EULA".to_string(),
                badge: "交互".to_string(),
                detail: "仅在需要附加终端时使用，不影响后台模式。".to_string(),
                kind: StatusKind::Neutral,
            },
            DashboardCard {
                id: "core-logs",
                icon: "󰘷",
                title: "查看实时日志".to_string(),
                subtitle: "跟随 logs/maibot.log".to_string(),
                badge: "诊断".to_string(),
                detail: "适合确认后台启动过程和排查异常退出。".to_string(),
                kind: StatusKind::Neutral,
            },
        ])
    }

    fn protocol_cards(&self) -> Result<Vec<DashboardCard>> {
        Ok(vec![
            DashboardCard {
                id: "napcat-note",
                icon: "󰘨",
                title: "NapCatQQ".to_string(),
                subtitle: "macOS 版目前仅提供说明入口".to_string(),
                badge: "说明".to_string(),
                detail: "当前平台仅管理 MaiBot 核心与插件。".to_string(),
                kind: StatusKind::Warning,
            },
            DashboardCard {
                id: "llbot-note",
                icon: "󰀻",
                title: "LuckyLilliaBot".to_string(),
                subtitle: "macOS 版目前仅提供说明入口".to_string(),
                badge: "说明".to_string(),
                detail: "当前平台仅管理 MaiBot 核心与插件。".to_string(),
                kind: StatusKind::Warning,
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
                subtitle: "MaiBot WebUI".to_string(),
                badge: if snapshot.has_config {
                    "可查看"
                } else {
                    "未配置"
                }
                .to_string(),
                detail: "公网 IP、端口和访问令牌会在详情中统一汇总。".to_string(),
                kind: if snapshot.has_config {
                    StatusKind::Neutral
                } else {
                    StatusKind::Warning
                },
            },
            DashboardCard {
                id: "init",
                icon: "󰑮",
                title: "初始化访问配置".to_string(),
                subtitle: "绑定 IPv4/IPv6 全地址".to_string(),
                badge: "初始化".to_string(),
                detail: "适合首次部署后快速打开远程访问。".to_string(),
                kind: StatusKind::Neutral,
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
                id: "access-note",
                icon: "󰋽",
                title: "访问策略说明".to_string(),
                subtitle: "协议端未适配，当前仅管理 MaiBot WebUI".to_string(),
                badge: "说明".to_string(),
                detail: "帮助确认当前 macOS 版可管理的访问范围。".to_string(),
                kind: StatusKind::Warning,
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

        let plugins_dir = PathBuf::from(&snapshot.config.mai_path)
            .join("MaiBot")
            .join("plugins");
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
                    Path::new(&snapshot.config.mai_path),
                    update_jobs,
                    macos_plugin_update_status,
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
                subtitle: "macOS".to_string(),
                badge: "平台".to_string(),
                detail: "支持核心服务管理，协议端能力会在界面中明确标注。".to_string(),
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
                                lines.push(
                                    "控制方式: 后台子进程 + Terminal.app 交互模式".to_string(),
                                );
                                lines.push(
                                    "常用动作: 后台启动 / 交互终端 / 停止 / 查看日志".to_string(),
                                );
                                if let Some(snippet) = read_log_summary(&log_path, 2) {
                                    lines.push(format!("日志摘要: {snippet}"));
                                }
                            }
                            "protocol" => {
                                lines.push("协议端状态: macOS 暂未提供启停管理。".to_string());
                                lines.push("当前入口用于查看平台支持范围。".to_string());
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
                let snapshot = self.dashboard_snapshot();
                let cfg = snapshot.config;
                let pid_path = PathBuf::from(&cfg.mai_path).join("logs").join("maibot.pid");
                let log_path = PathBuf::from(&cfg.mai_path).join("logs").join("maibot.log");
                let running = snapshot.maibot_running;
                lines.push(if running {
                    "󱄩 当前状态: 运行中".to_string()
                } else {
                    "󱄩 当前状态: 已停止".to_string()
                });
                lines.push("macOS 默认通过后台子进程启动核心服务。".to_string());
                lines.push("CLI 支持附加终端，TUI 支持 Terminal.app 交互入口。".to_string());
                if !cfg.mai_path.is_empty() {
                    lines.push(format!("工作区: {}", cfg.mai_path));
                    lines.push(format!("PID: {}", pid_path.display()));
                    lines.push(format!("日志: {}", log_path.display()));
                }
                if let Some(card) = selected {
                    match card.id {
                        "core-start" => {
                            lines.push("动作说明: 后台拉起独立进程组并立即返回管理器。".to_string())
                        }
                        "core-stop" => {
                            lines.push("动作说明: 先发 TERM，再在必要时升级为 KILL。".to_string())
                        }
                        "core-console" => lines
                            .push("动作说明: 打开 Terminal.app 处理首次启动或 EULA。".to_string()),
                        "core-logs" => {
                            lines.push("动作说明: 跟随 maibot.log 观察后台进程输出。".to_string())
                        }
                        _ => {}
                    }
                }
            }
            DashboardTab::Protocol => {
                if let Some(card) = selected {
                    match card.id {
                        "napcat-note" => {
                            lines.push("当前能力: macOS 版目前仅提供说明入口".to_string());
                            lines.push("NapCat 安装与进程托管暂不在本版本开放。".to_string());
                            lines.push("界面会隐藏不可执行的启停操作。".to_string());
                        }
                        "llbot-note" => {
                            lines.push("当前能力: macOS 版目前仅提供说明入口".to_string());
                            lines.push(
                                "LuckyLilliaBot 安装与进程托管暂不在本版本开放。".to_string(),
                            );
                            lines.push("界面会隐藏不可执行的启停操作。".to_string());
                        }
                        _ => {}
                    }
                } else {
                    lines.push("NapCat 和 LLBot 当前在 macOS 上仅提供说明入口。".to_string());
                    lines.push("这里用于集中查看平台支持范围。".to_string());
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
                            lines.push("完整汇总可确认 WebUI 地址与 token。".to_string());
                        }
                        "init" => {
                            lines.push(
                                "会把 MaiBot WebUI host 改为 [\"0.0.0.0\", \"::\"]。".to_string(),
                            );
                            lines.push("适合局域网或远程终端访问 WebUI。".to_string());
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
                        "access-note" => {
                            lines.push("当前只管理 MaiBot WebUI 访问入口。".to_string());
                            lines.push(
                                "NapCat / LLBot 的访问配置会在对应管理能力可用后开放。".to_string(),
                            );
                            lines.push("这样可以避免让用户误以为 macOS 已支持协议端。".to_string());
                        }
                        _ => {}
                    }
                } else {
                    lines.push("初始化会把 WebUI 绑定到 IPv4/IPv6 全地址。".to_string());
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
                        {
                            if let Ok(summary) = self.read_plugin_summary(&dir) {
                                lines.push(format!("ID: {}", summary.id));
                                lines.push(format!("作者: {}", summary.author));
                                lines.push(format!("版本: {}", summary.version));
                                lines.push(format!("更新状态: {}", card.badge));
                                lines.push(format!("目录名: {}", summary.dir_name));
                            }
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
                lines.push("平台: macOS".to_string());
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
                actions.push("恢复默认会回到推荐路径与核心环境组合".to_string());
            }
            DashboardTab::Core => {
                actions.push("当前动作块会复用核心服务逻辑".to_string());
                if let Some(card) = selected {
                    match card.id {
                        "core-start" => actions.push("将启动 MaiBot 并保留日志记录".to_string()),
                        "core-stop" => actions.push("将停止正在运行的 MaiBot 核心服务".to_string()),
                        "core-console" => actions.push("将打开 Terminal.app 交互窗口".to_string()),
                        "core-logs" => actions.push("将直接进入日志跟随视图".to_string()),
                        _ => {}
                    }
                }
                actions.push("启动 / 停止 / 交互终端 / 日志 已原生接入当前面板".to_string());
            }
            DashboardTab::Protocol => {
                if let Some(card) = selected {
                    actions.push(format!("查看 {} 的限制说明", card.title));
                } else {
                    actions.push("协议端状态与限制说明".to_string());
                }
                actions.push("说明当前平台支持范围，避免进入不可执行操作".to_string());
            }
            DashboardTab::Access => {
                if let Some(card) = selected {
                    match card.id {
                        "access-summary" => {
                            actions.push("完整访问信息包含 WebUI 地址与令牌".to_string());
                            actions.push("可确认 MaiBot WebUI 地址与 token".to_string());
                        }
                        "init" => {
                            actions.push("初始化远程访问会写入 WebUI 配置".to_string());
                            actions.push("会提示确认后写入 WebUI 配置".to_string());
                        }
                        "access-clear-data" => {
                            actions.push("清理前会弹出确认对话框".to_string());
                            actions.push("仅保留 MaiBot/data/webui.json".to_string());
                        }
                        "access-note" => {
                            actions.push("当前 macOS 访问能力边界说明".to_string());
                            actions.push("帮助确认协议端访问配置暂未开放".to_string());
                        }
                        _ => {
                            actions.push("访问配置菜单包含地址与初始化操作".to_string());
                        }
                    }
                } else {
                    actions.push("访问配置菜单包含地址与初始化操作".to_string());
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
            (DashboardTab::Access, "access-note") => {
                state.popup = Some(macos_access_note_popup());
                state.set_status_message("macOS 当前仅支持 MaiBot WebUI 访问配置");
                Ok(true)
            }
            (DashboardTab::Protocol, "napcat-note") => {
                state.popup = Some(macos_protocol_popup("NapCatQQ"));
                state.set_status_message("已显示 NapCat 的 macOS 说明");
                Ok(true)
            }
            (DashboardTab::Protocol, "llbot-note") => {
                state.popup = Some(macos_protocol_popup("LuckyLilliaBot"));
                state.set_status_message("已显示 LLBot 的 macOS 说明");
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
                    Some("protocol") => {
                        state.active_tab = DashboardTab::Protocol;
                        state.focus = DashboardFocus::Content;
                        state.set_status_message("已打开协议端能力说明");
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
                        state.set_status_message("已请求后台启动 MaiBot 核心");
                    }
                    Some("core-stop") => {
                        self.handle_menu_result(self.stop_maibot_core())?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已请求停止 MaiBot 核心");
                    }
                    Some("core-console") => {
                        self.handle_menu_result(self.start_maibot_core(true))?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已打开 MaiBot 交互终端");
                    }
                    Some("core-logs") => {
                        self.handle_menu_result(self.print_maibot_core_logs(200, true))?;
                        state.set_status_message("已打开 MaiBot 实时日志");
                    }
                    _ => {
                        self.handle_menu_result(self.manage_maibot_menu())?;
                        self.invalidate_dashboard_cache();
                    }
                }
            }
            DashboardTab::Protocol => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                match selected.map(|card| card.id) {
                    Some("napcat-note") => {
                        state.popup = Some(macos_protocol_popup("NapCatQQ"));
                        state.set_status_message("已显示 NapCat 的 macOS 说明");
                    }
                    Some("llbot-note") => {
                        state.popup = Some(macos_protocol_popup("LuckyLilliaBot"));
                        state.set_status_message("已显示 LLBot 的 macOS 说明");
                    }
                    _ => {
                        state.popup = Some(macos_protocol_overview_popup());
                        state.set_status_message("已显示协议端能力说明");
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
                    Some("init") => {
                        self.handle_menu_result(self.initialize_maibot_access_config())?;
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
                    Some("access-note") => {
                        state.popup = Some(macos_access_note_popup());
                        state.set_status_message("macOS 当前仅支持 MaiBot WebUI 访问配置");
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
                    }
                    _ => {
                        self.handle_menu_result(self.manage_plugins_menu())?;
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
                    Some("protocol") => state.active_tab = DashboardTab::Protocol,
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
                    state.set_status_message("已请求后台启动 MaiBot 核心");
                }
                Some("core-stop") if action_idx == 0 => {
                    self.handle_menu_result(self.stop_maibot_core())?;
                    self.invalidate_dashboard_cache();
                    state.set_status_message("已请求停止 MaiBot 核心");
                }
                Some("core-console") if action_idx == 0 => {
                    self.handle_menu_result(self.start_maibot_core(true))?;
                    self.invalidate_dashboard_cache();
                    state.set_status_message("已打开 MaiBot 交互终端");
                }
                Some("core-logs") if action_idx == 0 => {
                    self.handle_menu_result(self.print_maibot_core_logs(200, true))?;
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
                    Some("napcat-note") if action_idx == 0 => {
                        state.popup = Some(macos_protocol_popup("NapCatQQ"));
                    }
                    Some("llbot-note") if action_idx == 0 => {
                        state.popup = Some(macos_protocol_popup("LuckyLilliaBot"));
                    }
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
                state.set_status_message("协议端状态已保持明确限制");
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
                Some("access-note") if action_idx == 0 => {
                    state.popup = Some(macos_access_note_popup());
                }
                Some("init") if action_idx == 0 => {
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
                _ if action_idx == 0 => {
                    self.handle_menu_result(self.manage_config_access_menu())?;
                }
                _ => {}
            },
            DashboardTab::About | DashboardTab::Deploy => {}
        }
        Ok(true)
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
                    _ => "󰋼",
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
        let pid_path = PathBuf::from(&cfg.mai_path).join("logs").join("maibot.pid");
        let maibot_pid = pid_running(&pid_path).unwrap_or(None);
        let mut cards = Vec::new();
        cards.push(if maibot_pid.is_some() {
            StatusCard::running("MaiBot", "后台运行中 · 日志写入 logs/maibot.log")
        } else {
            StatusCard::stopped("MaiBot", "核心后台进程未运行")
        });
        cards.push(StatusCard::warning(
            "协议端服务",
            "说明",
            "macOS 当前只管理 MaiBot 核心与插件；协议端保留说明入口",
        ));
        cards.push(StatusCard::neutral(
            "插件中心",
            "可用",
            "插件安装、更新和卸载与核心工作区共用",
        ));
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

fn macos_protocol_popup(name: &str) -> DashboardPopup {
    DashboardPopup {
        title: name.to_string(),
        subtitle: "macOS 当前的协议端能力说明".to_string(),
        lines: vec![
            "当前版本先集中管理 MaiBot 核心、访问配置和插件。".to_string(),
            format!("{name} 的安装、启停和日志入口暂未接入 macOS。"),
            "界面会保留说明入口，并隐藏不可执行的启停操作。".to_string(),
        ],
        actions: vec!["取消".to_string()],
        selected: 0,
        scroll: 0,
    }
}

fn macos_protocol_overview_popup() -> DashboardPopup {
    DashboardPopup {
        title: "协议端服务".to_string(),
        subtitle: "macOS 当前的协议端能力说明".to_string(),
        lines: vec![
            "当前版本先集中管理 MaiBot 核心、访问配置和插件。".to_string(),
            "NapCatQQ 与 LuckyLilliaBot 的安装、启停和日志入口暂未接入 macOS。".to_string(),
            "协议端页面会保留说明入口，并隐藏不可执行的启停操作。".to_string(),
        ],
        actions: vec!["取消".to_string()],
        selected: 0,
        scroll: 0,
    }
}

fn macos_access_note_popup() -> DashboardPopup {
    DashboardPopup {
        title: "访问策略说明".to_string(),
        subtitle: "macOS 当前仅管理 MaiBot WebUI".to_string(),
        lines: vec![
            "访问汇总会展示 MaiBot WebUI 地址与访问密钥。".to_string(),
            "NapCat 与 LuckyLilliaBot 的访问配置会随对应管理能力一起开放。".to_string(),
            "这样可以避免把尚不可执行的协议端操作混入当前面板。".to_string(),
        ],
        actions: vec!["取消".to_string()],
        selected: 0,
        scroll: 0,
    }
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
    ]
}

fn planner_field_detail(field: PlanField) -> &'static str {
    match field {
        PlanField::InstallPath => "MaiBot 工作区路径；首次会自动创建目录。",
        PlanField::MaiBotBranch => "在稳定版 main 和预览版 dev 之间切换。",
        PlanField::InstallMode => "决定是修复更新还是清空目录后全新安装。",
        PlanField::PythonEnv => "选择本机 python3 或由 uv 管理的独立环境。",
        PlanField::VenvMode => "控制是否保留现有虚拟环境，Clean 模式会强制重建。",
        PlanField::GithubProxy => "切换 GitHub 直连、自动测速或镜像源。",
        PlanField::PipSource => "设置 pip / uv 使用的 PyPI 镜像源。",
        PlanField::BotProtocols => "macOS 当前仅提供协议端说明入口，不安装附加协议端。",
        PlanField::DockerMirror => "macOS 当前不使用 Docker 管理协议端。",
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
                "使用系统 Python 解释器。".to_string()
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
        PlanField::BotProtocols => "macOS 当前明确不安装协议端。".to_string(),
        PlanField::DockerMirror => "macOS 当前不使用 Docker 部署协议端。".to_string(),
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
