use anyhow::{Error, Result, anyhow, bail};
use dialoguer::console::style;
use std::{
    cell::RefCell,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use crate::model::{
    DashboardCard, DashboardChoice, DashboardEvent, DashboardFocus, DashboardPopup,
    DashboardPopupPurpose, DashboardState, DashboardTab, DashboardView, InstallPlan, PlanField,
    StatusKind, deploy_card_field,
};
use crate::theme::AppTheme;
use crate::ui::{ActionItem, StatusCard};
use crate::utils::{list_plugins, screen_exists, screen_sessions_exist};

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
    snapshot_refreshed_at: Option<Instant>,
    plugin_cards: Option<Vec<DashboardCard>>,
    plugin_cards_refreshed_at: Option<Instant>,
}

const DASHBOARD_STATUS_TTL: Duration = Duration::from_secs(10);
const LOG_SUMMARY_TAIL_BYTES: u64 = 64 * 1024;
const DOCKER_STATUS_TIMEOUT: Duration = Duration::from_millis(350);

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
        let napcat_installed = has_config && napcat_installed(&config.mai_path);
        let llbot_installed = has_config && llbot_installed(&config);
        let plugin_count = if has_config {
            list_plugins(&root.join("MaiBot").join("plugins"))
                .map(|items| items.len())
                .unwrap_or(0)
        } else {
            0
        };
        let (maibot_running, llbot_running, napcat_running) = if has_config {
            let docker_probe = std::thread::spawn(napcat_running);
            let screen_states = screen_sessions_exist(&["maibot", "llbot"]).unwrap_or_default();
            let napcat_running = docker_probe.join().unwrap_or(false);
            (
                screen_states.first().copied().unwrap_or(false),
                screen_states.get(1).copied().unwrap_or(false),
                napcat_running,
            )
        } else {
            (false, false, false)
        };
        let snapshot = DashboardRuntimeSnapshot {
            config,
            has_config,
            maibot_running,
            napcat_running,
            llbot_running,
            napcat_installed,
            llbot_installed,
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
                        break;
                    }
                }
                DashboardEvent::ResetDeployPlan => {
                    let reset = self.build_recommended_defaults();
                    dashboard.deploy_plan = Some(reset);
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
                DashboardEvent::Exit => break,
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
            empty_title: "没有匹配项".to_string(),
            empty_detail: "试试清空筛选，或先完成部署与安装。".to_string(),
        })
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
                    .unwrap_or("启动、停止和控制台进入都保留现有逻辑。"),
            ),
            DashboardTab::Protocol => (
                "协议端服务",
                "NapCatQQ 与 LuckyLilliaBot 统一收纳在协议端工作区。",
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
                    .unwrap_or("安装、卸载与依赖修复集中管理。"),
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
                    "screen: maibot 正在托管核心服务".to_string()
                } else {
                    "核心服务当前未运行".to_string()
                },
                badge: if maibot_running {
                    "运行中"
                } else {
                    "已停止"
                }
                .to_string(),
                detail: "支持启动、停止与进入控制台。".to_string(),
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
                    "容器运行中，配置未记录目录".to_string()
                } else if napcat_installed {
                    if napcat_running {
                        "Docker 容器已运行".to_string()
                    } else {
                        "已安装，容器未运行".to_string()
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
                detail: "协议端日志、重建和容器管理入口。".to_string(),
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
                    "screen 运行中，配置未记录目录".to_string()
                } else if llbot_installed {
                    if llbot_running {
                        "screen: llbot 正在运行".to_string()
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
                detail: "密码、日志和 CLI 协议端控制入口。".to_string(),
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
                detail: "安装、卸载和修复依赖都从这里进入。".to_string(),
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
                    "服务已运行，可转入控制台或日志视图".to_string()
                } else {
                    "创建 screen: maibot 后台会话".to_string()
                },
                badge: if running { "已运行" } else { "可启动" }.to_string(),
                detail: "首次启动/EULA 可再进入 screen 终端确认。".to_string(),
                kind: StatusKind::Running,
            },
            DashboardCard {
                id: "core-stop",
                icon: "󰓛",
                title: "停止 MaiBot".to_string(),
                subtitle: "结束 screen 会话和核心进程".to_string(),
                badge: if running { "可停止" } else { "未运行" }.to_string(),
                detail: "适合服务无响应或准备热重启时使用。".to_string(),
                kind: if running {
                    StatusKind::Warning
                } else {
                    StatusKind::Stopped
                },
            },
            DashboardCard {
                id: "core-console",
                icon: "󰆍",
                title: "进入控制台".to_string(),
                subtitle: "进入 screen 会话交互视图".to_string(),
                badge: "控制台".to_string(),
                detail: "附着到 maibot 会话并显示运行输出。".to_string(),
                kind: StatusKind::Neutral,
            },
            DashboardCard {
                id: "core-logs",
                icon: "󰘷",
                title: "查看实时日志".to_string(),
                subtitle: "hardcopy + tail 跟随输出".to_string(),
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
                    "容器运行中 · 配置待同步".to_string()
                } else if napcat_installed {
                    "Docker Compose 协议端".to_string()
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
                detail: "适合查看容器状态、日志和重建操作。".to_string(),
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
                    "screen 运行中 · 配置待同步".to_string()
                } else if llbot_installed {
                    "screen 托管的 CLI 协议端".to_string()
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
                detail: "密码、控制台和日志查看都在现有菜单中。".to_string(),
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
                id: "access-init",
                icon: "󰑮",
                title: "初始化远程访问".to_string(),
                subtitle: "绑定 0.0.0.0 并启用 Adapter".to_string(),
                badge: "初始化".to_string(),
                detail: "适合首次部署后快速开放 WebUI 与协议端访问。".to_string(),
                kind: StatusKind::Running,
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
        let mut cards = vec![DashboardCard {
            id: "plugin-center",
            icon: "󰏗",
            title: "插件管理".to_string(),
            subtitle: "安装、卸载与依赖修复".to_string(),
            badge: format!("{} 个插件", plugins.len()),
            detail: "集中安装、卸载与修复插件依赖。".to_string(),
            kind: StatusKind::Neutral,
        }];
        for plugin in plugins {
            let summary = self.read_plugin_summary(&plugins_dir.join(&plugin)).ok();
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
                badge: if summary
                    .as_ref()
                    .is_some_and(|summary| summary.has_requirements)
                {
                    "已安装 · 含依赖"
                } else {
                    "已安装"
                }
                .to_string(),
                detail: summary
                    .as_ref()
                    .map(|summary| summary.description.clone())
                    .unwrap_or_else(|| "支持卸载、requirements 修复和目录规范化。".to_string()),
                kind: StatusKind::Running,
            });
        }
        let mut cache = self.dashboard_cache.borrow_mut();
        cache.plugin_cards = Some(cards.clone());
        cache.plugin_cards_refreshed_at = Some(Instant::now());
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
                subtitle: "Linux / WSL".to_string(),
                badge: "平台".to_string(),
                detail: "支持后台服务、协议端和插件管理。".to_string(),
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
                    lines.push(format!(
                        "协议端: {}",
                        if cfg.bot_protocols.is_empty() {
                            "未记录".to_string()
                        } else {
                            cfg.bot_protocols
                        }
                    ));
                    if let Some(card) = selected {
                        match card.id {
                            "maibot" => {
                                lines.push("控制方式: screen 会话 maibot".to_string());
                                lines.push("日志查看: hardcopy + tail 跟随".to_string());
                                lines.push(
                                    "常用动作: 启动 / 停止 / 进入控制台 / 查看日志".to_string(),
                                );
                                if let Some(snippet) = read_log_summary(
                                    &PathBuf::from("/tmp/maibot-manager-maibot.log"),
                                    2,
                                ) {
                                    lines.push(format!("日志摘要: {snippet}"));
                                }
                            }
                            "napcat" => {
                                lines.push("协议端类型: Docker Compose 容器".to_string());
                                lines.push("容器名: napcat".to_string());
                                lines.push(
                                    "常用动作: 启动 / 停止 / 重建 / 查看容器日志".to_string(),
                                );
                                let log_path = PathBuf::from(&cfg.mai_path)
                                    .join("NapCat")
                                    .join("logs")
                                    .join("onebot.log");
                                if let Some(snippet) = read_log_summary(&log_path, 1) {
                                    lines.push(format!("日志摘要: {snippet}"));
                                }
                            }
                            "llbot" => {
                                lines.push("协议端类型: screen 会话 llbot".to_string());
                                lines.push("日志查看: screen hardcopy".to_string());
                                lines.push(
                                    "常用动作: 启动 / 停止 / 进入控制台 / 修改密码".to_string(),
                                );
                                if let Some(snippet) = read_log_summary(
                                    &PathBuf::from("/tmp/maibot-manager-llbot.log"),
                                    1,
                                ) {
                                    lines.push(format!("日志摘要: {snippet}"));
                                }
                            }
                            "plugins" => {
                                lines.push("插件目录会按 manifest id 自动规范化命名。".to_string());
                                lines.push("可在插件中心直接执行依赖修复或卸载。".to_string());
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
                let running = snapshot.maibot_running;
                lines.push(if running {
                    "󱄩 当前状态: 运行中".to_string()
                } else {
                    "󱄩 当前状态: 已停止".to_string()
                });
                lines.push("Linux 后台服务通过 screen 会话托管。".to_string());
                lines.push("控制台入口会附加到正在运行的会话。".to_string());
                if snapshot.has_config {
                    lines.push(format!("工作区: {}", snapshot.config.mai_path));
                    lines.push("screen 会话: maibot".to_string());
                    lines.push("日志来源: /tmp/maibot-manager-maibot.log 快照".to_string());
                }
                if let Some(card) = selected {
                    match card.id {
                        "core-start" => {
                            lines.push("动作说明: 创建 screen: maibot 后台会话。".to_string())
                        }
                        "core-stop" => {
                            lines.push("动作说明: 结束 screen 会话和关联核心进程。".to_string())
                        }
                        "core-console" => {
                            lines.push("动作说明: 进入 maibot 会话查看实时输出。".to_string())
                        }
                        "core-logs" => {
                            lines.push("动作说明: 持续 hardcopy 并 tail 最新日志。".to_string())
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
                                    "docker compose 服务正在运行"
                                } else {
                                    "docker compose 服务未运行"
                                }
                            ));
                            lines.push("容器名: napcat".to_string());
                            lines.push("日志: docker compose logs --tail=100".to_string());
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
                                    "screen 会话 llbot 正在运行"
                                } else {
                                    "screen 会话 llbot 未运行"
                                }
                            ));
                            lines.push("控制台: screen -r llbot".to_string());
                            lines.push("日志: screen hardcopy + tail".to_string());
                        }
                        _ => {}
                    }
                } else {
                    lines.push("NapCat 使用 Docker Compose。".to_string());
                    lines.push("LLBot 使用 screen 会话与 CLI 协议端。".to_string());
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
                        "access-init" => {
                            lines.push("会把 MaiBot WebUI host 改为 0.0.0.0。".to_string());
                            lines.push("会同时启用 NapCat Adapter 插件。".to_string());
                            lines.push("执行后需要重启 MaiBot 才会完全生效。".to_string());
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
                    lines.push("初始化会把 WebUI 绑定到 0.0.0.0，并启用 Adapter。".to_string());
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
                                lines.push(format!(
                                    "依赖: {}",
                                    if summary.has_requirements {
                                        "requirements.txt"
                                    } else {
                                        "无额外依赖"
                                    }
                                ));
                                lines.push(format!("目录名: {}", summary.dir_name));
                            }
                        }
                    }
                }
                lines.push("插件目录按 _manifest.json 中的 id 规范化命名。".to_string());
                lines.push("依赖修复会复用当前 Python/uv 安装策略。".to_string());
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
                lines.push("平台: Linux / WSL".to_string());
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
                        "core-start" => actions.push("将复用 screen 后台启动逻辑".to_string()),
                        "core-stop" => actions.push("将结束 maibot screen 会话".to_string()),
                        "core-console" => actions.push("将附着到 maibot 会话".to_string()),
                        "core-logs" => actions.push("将进入日志快照跟随视图".to_string()),
                        _ => {}
                    }
                }
                actions.push("启动 / 停止 / 控制台 / 日志 已原生接入当前面板".to_string());
            }
            DashboardTab::Protocol => {
                if let Some(card) = selected {
                    match card.id {
                        "napcat" => {
                            actions.push("NapCat 管理支持启动、停止与日志查看".to_string());
                            actions.push("支持启动 / 停止 / 重启 / 日志 / 重建".to_string());
                        }
                        "llbot" => {
                            actions.push("LLBot 管理支持控制台与密码维护".to_string());
                            actions.push("支持启动 / 停止 / 控制台 / 密码修改".to_string());
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
                            actions.push("可确认 MaiBot / NapCat / LLBot WebUI 地址".to_string());
                        }
                        "access-init" => {
                            actions.push("初始化远程访问会写入 WebUI/Adapter 配置".to_string());
                            actions.push("会提示确认后写入 WebUI/Adapter 配置".to_string());
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
                        actions.push("插件维护动作包含依赖修复与卸载".to_string());
                        actions.push("可执行卸载或修复依赖".to_string());
                    } else {
                        actions.push("完整插件中心包含安装、卸载与修复".to_string());
                    }
                } else {
                    actions.push("完整插件中心包含安装、卸载与修复".to_string());
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
        let choices = self.planner_choices(plan, field);
        Ok(choices
            .into_iter()
            .enumerate()
            .map(|(idx, label)| DashboardChoice {
                detail: deploy_choice_detail(field, idx, &label),
                active: self.planner_choice_active(plan, field, idx),
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
            return self.activate_dashboard_popup(state, popup);
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
            DashboardTab::Deploy => {}
            DashboardTab::Core => {
                let cards = self.dashboard_cards(&state.active_tab, &state.search_query)?;
                let selected = cards.get(state.selected_for_len(cards.len()));
                match selected.map(|card| card.id) {
                    Some("core-start") => {
                        state.popup = Some(linux_core_start_popup());
                    }
                    Some("core-stop") => {
                        self.handle_menu_result(self.stop_maibot_core())?;
                        self.invalidate_dashboard_cache();
                        state.set_status_message("已请求停止 MaiBot 核心");
                    }
                    Some("core-console") => {
                        self.handle_menu_result(self.attach_screen("maibot"))?;
                        state.set_status_message("已进入 MaiBot 控制台");
                    }
                    Some("core-logs") => {
                        self.handle_menu_result(self.print_maibot_core_logs(100, true))?;
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
                    Some("adapter") => {
                        self.handle_menu_result(self.modify_adapter_config())?;
                        self.invalidate_dashboard_cache();
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
        popup: DashboardPopup,
    ) -> Result<bool> {
        let action_idx = popup.selected;
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
                    state.set_status_message("已请求后台启动 MaiBot 核心");
                }
                Some("core-start") if action_idx == 1 => {
                    self.handle_menu_result(self.start_maibot_core(true))?;
                    self.invalidate_dashboard_cache();
                    state.set_status_message("已启动并进入 MaiBot 控制台");
                }
                Some("core-start") if action_idx == 2 => {
                    self.handle_menu_result(self.manage_maibot_menu())?;
                    self.invalidate_dashboard_cache();
                }
                Some("core-stop") if action_idx == 0 => {
                    self.handle_menu_result(self.stop_maibot_core())?;
                    self.invalidate_dashboard_cache();
                    state.set_status_message("已请求停止 MaiBot 核心");
                }
                Some("core-console") if action_idx == 0 => {
                    self.handle_menu_result(self.attach_screen("maibot"))?;
                    state.set_status_message("已进入 MaiBot 控制台");
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
                Some("adapter") if action_idx == 0 => {
                    self.handle_menu_result(self.modify_adapter_config())?;
                    self.invalidate_dashboard_cache();
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
                    7 => "󰘨",
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
            ActionItem::primary("修复依赖", "安装该插件的 requirements.txt"),
            ActionItem::destructive("卸载插件", "删除该插件目录"),
            ActionItem::back("返回", "回到插件中心"),
        ];
        let choice = self.select_action(&format!("管理插件：{}", summary.name), &actions)?;
        let result = match choice {
            0 => self.install_plugin_dependencies(&summary.dir_name),
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
            0 => self.install_plugin_dependencies(&summary.dir_name),
            1 => self.remove_plugin(&summary.dir_name),
            _ => Ok(()),
        };
        if self.handle_menu_result(result)? {
            state.set_status_message(format!("插件 {} 操作已执行", summary.name));
        }
        Ok(())
    }

    pub(crate) fn handle_menu_result(&self, result: Result<()>) -> Result<bool> {
        match result {
            Ok(()) => Ok(true),
            Err(error) => {
                self.print_menu_error(&error)?;
                Ok(false)
            }
        }
    }

    fn print_menu_error(&self, error: &Error) -> Result<()> {
        println!();
        println!(
            "  {} {}",
            style("!").red().bold(),
            style("操作失败").red().bold()
        );
        println!("  {}", style(error.to_string()).red());
        for cause in error.chain().skip(1) {
            println!("    {}", style(format!("原因: {cause}")).dim());
        }
        self.pause("按回车返回当前菜单")?;
        Ok(())
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
        let maibot_running = screen_exists("maibot").unwrap_or(false);
        let llbot_running = screen_exists("llbot").unwrap_or(false);
        let napcat_running = napcat_running();
        let mut cards = Vec::new();
        cards.push(if maibot_running {
            StatusCard::running("MaiBot", "screen: maibot · 可进入控制台或查看日志")
        } else {
            StatusCard::stopped("MaiBot", "核心服务未启动")
        });
        cards.push(if napcat_installed(&cfg.mai_path) {
            if napcat_running {
                StatusCard::running("NapCatQQ", "Docker 容器已纳入管理")
            } else {
                StatusCard::stopped("NapCatQQ", "已安装，Docker 容器未运行")
            }
        } else {
            StatusCard::neutral("NapCatQQ", "未安装", "可在部署计划中启用")
        });
        cards.push(if llbot_installed(&cfg) {
            if llbot_running {
                StatusCard::running("LuckyLilliaBot", "screen: llbot · CLI 协议端运行中")
            } else {
                StatusCard::stopped("LuckyLilliaBot", "已安装，screen 会话未运行")
            }
        } else {
            StatusCard::neutral("LuckyLilliaBot", "未安装", "可在部署计划中启用")
        });
        self.print_status_cards("服务概览", &cards);
    }
}

pub(crate) fn napcat_running() -> bool {
    let mut child = match Command::new("docker")
        .arg("ps")
        .arg("--filter")
        .arg("name=^napcat$")
        .arg("--filter")
        .arg("status=running")
        .arg("--format")
        .arg("{{.Names}}")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut output);
                }
                return status.success() && output.lines().any(|line| line.trim() == "napcat");
            }
            Ok(None) if started.elapsed() >= DOCKER_STATUS_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}

fn napcat_installed(mai_path: &str) -> bool {
    !mai_path.is_empty() && PathBuf::from(mai_path).join("NapCat").exists()
}

fn llbot_installed(cfg: &crate::model::AppConfig) -> bool {
    if !cfg.mai_llbot_path.is_empty() && PathBuf::from(&cfg.mai_llbot_path).exists() {
        return true;
    }
    !cfg.mai_path.is_empty() && PathBuf::from(&cfg.mai_path).join("LLBot").exists()
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
        PlanField::DockerMirror,
    ]
}

fn linux_core_start_popup() -> DashboardPopup {
    DashboardPopup {
        title: "启动 MaiBot".to_string(),
        subtitle: "选择启动方式".to_string(),
        lines: vec![
            "后台启动会创建 screen: maibot 并立即返回管理器。".to_string(),
            "启动并进入终端会在 screen 中显示前台输出，适合首次启动确认 EULA。".to_string(),
            "这里不会判断是否首次启动，请按当前需要选择。".to_string(),
        ],
        actions: vec![
            "后台启动".to_string(),
            "启动并进入终端".to_string(),
            "更多控制".to_string(),
            "取消".to_string(),
        ],
        selected: 0,
        purpose: DashboardPopupPurpose::CoreStartMode,
        ..DashboardPopup::default()
    }
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
        PlanField::BotProtocols => "选择 NapCatQQ、LuckyLilliaBot 或暂不安装协议端。",
        PlanField::DockerMirror => "为 Linux Docker 容器设置镜像加速策略。",
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
        PlanField::BotProtocols => match idx {
            0 => "默认推荐，启用 NapCatQQ。".to_string(),
            1 => "切换到 LuckyLilliaBot 协议端。".to_string(),
            _ => "暂不安装附加协议端。".to_string(),
        },
        PlanField::DockerMirror => match idx {
            0 => "优先使用 1MS Docker 镜像。".to_string(),
            1 => "切换到轩辕 Docker 镜像。".to_string(),
            2 => "回到 Docker 官方源。".to_string(),
            _ => "保持当前 Docker 镜像设置。".to_string(),
        },
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
