use crate::model::APP_VERSION;

pub(crate) fn print_help() {
    let core_notes = if cfg!(target_os = "windows") {
        r#"  core start         在独立 Windows 控制台中启动 MaiBot，并记录 logs/maibot.pid
  core start --exec  Windows 下等同于 start；控制台会独立打开
  core logs          读取安装目录 logs/maibot.log，若上游未写入则提示不存在
  core logs -f       每 2 秒刷新日志文件尾部
  core exec          Windows 下无法附着已打开控制台；请查看独立窗口或日志"#
    } else if cfg!(target_os = "macos") {
        r#"  core start         在后台子进程中启动 MaiBot，并记录 logs/maibot.pid
  core start --exec  附加当前终端启动，适合首次启动时确认 EULA
  core logs          读取安装目录 logs/maibot.log
  core logs -f       每 2 秒刷新日志文件尾部
  core exec          跟随 logs/maibot.log；macOS 不使用 screen 附着"#
    } else {
        r#"  core start         在 screen 会话 maibot 中后台启动 MaiBot
  core start --exec  启动后立刻进入 screen 控制台，底部显示快捷退出方式
  core logs          通过 screen hardcopy 读取日志缓冲，不会进入或抢占 screen
  core logs -f       每 2 秒刷新一次 hardcopy 输出
  core exec          执行 screen -r maibot，适合需要交互控制台时使用"#
    };
    let protocol_notes = if cfg!(target_os = "windows") {
        r#"  napcat             管理 NapCat Shell；start/exec 会请求管理员权限启动 launcher.bat
  napcat logs        读取 NapCat Shell 日志文件
  napcat rebuild     Windows 不使用 Docker；会重新下载并解压最新 NapCat Shell 包
  llbot              管理 LuckyLilliaBot Desktop 程序 llbot.exe
  llbot logs         读取 LLBot 目录下的 llbot.log
  llbot exec         Windows 下无法附着 Desktop 窗口；请查看已打开窗口
  protocol           也可作为聚合入口，例如 maibot protocol napcat restart"#
    } else if cfg!(target_os = "macos") {
        r#"  napcat             macOS 版目前仅提供协议端说明入口
  llbot              macOS 版目前仅提供协议端说明入口
  protocol           聚合入口保留，后续接入 macOS 原生协议端管理"#
    } else {
        r#"  napcat             管理 NapCat Docker Compose
  napcat logs        使用 docker compose logs
  napcat exec        进入 napcat 容器 shell
  napcat rebuild     执行 down + pull + up -d
  snowluma           管理 SnowLuma Docker Compose
  snowluma recreate-data  删除数据目录后全新启动，生成新的临时密码
  llbot              管理 LuckyLilliaBot screen 会话
  llbot logs         通过 screen hardcopy 读取日志缓冲，不影响 screen 会话
  llbot exec         执行 screen -r llbot，进入前会提示退出方式
  protocol           也可作为聚合入口，例如 maibot protocol napcat restart"#
    };
    let console_tip = if cfg!(target_os = "windows") {
        r#"Windows 控制台提示:
  MaiBot 会在独立控制台窗口中运行，停止命令会优先读取 logs/maibot.pid 并结束完整进程树。
  安装时若缺少 Git / uv / Python，会优先在安装目录 tools 下准备便携工具链；
  uv 的缓存和托管 Python 也会固定在安装目录内，避免写到随机用户目录。"#
    } else if cfg!(target_os = "macos") {
        r#"macOS 提示:
  安装时若缺少 Homebrew，会调用 Homebrew 官方脚本安装；缺少 Git / uv / Python 时通过 Homebrew 补齐。
  当前 macOS 版只部署 MaiBot 核心与插件管理，协议端入口会清晰说明当前平台能力。
  MaiBot 核心默认由管理器创建后台子进程运行；首次启动/EULA 可用 --exec 附加当前终端。"#
    } else {
        r#"Screen 退出提示:
  进入 core exec 或 llbot exec 后，如需退出控制台但保持进程运行，请按 Ctrl+A，再按 D。"#
    };
    let install_examples = if cfg!(target_os = "macos") {
        r#"  maibot install --path ~/maimai --python uv --protocol none
  maibot update --branch main --github auto --protocol none
  maibot update --git-dirty stash --protocol none"#
    } else {
        r#"  maibot install --path ~/maimai --python uv --protocol napcat
  maibot update --branch main --github auto
  maibot update --protocol llbot --llbot-update update
  maibot update --git-dirty stash --napcat-conflict recreate"#
    };
    let default_note = if cfg!(target_os = "macos") {
        "推荐默认值为当前用户 HOME 下的 maimai 目录、uv Python 环境，并暂不安装 NapCat / LLBot 协议端。"
    } else {
        "推荐默认值为当前用户 HOME 下的 maimai 目录、uv Python 环境和 NapCatQQ 协议端。"
    };
    let access_init_note = if cfg!(target_os = "macos") {
        "将 MaiBot WebUI 绑定到所有 IPv4/IPv6 地址；默认会询问确认"
    } else {
        "将 MaiBot WebUI 绑定到所有 IPv4/IPv6 地址并启用 Napcat Adapter；默认会询问确认"
    };
    let access_show_note = if cfg!(target_os = "macos") {
        "显示 MaiBot WebUI 地址和密钥"
    } else {
        "显示 MaiBot、NapCat、LLBot 的 WebUI 地址和密钥/密码"
    };
    println!(
        r#"MaiBot Manager {APP_VERSION}

这是 MaiBot 的 Linux / Windows / macOS 部署与运维工具。
不加参数时进入交互式 TUI；带参数时直接执行对应 CLI 命令，适合脚本、SSH 快速操作和日常维护。

用法:
  maibot                         进入 TUI
  maibot tui                     进入 TUI
  maibot help | -h | --help      查看帮助

常用示例:
{install_examples}
  maibot core restart
  maibot core logs --tail 200
  maibot core exec
  maibot napcat restart
  maibot snowluma restart
  maibot llbot password my-new-password
  maibot access show
  maibot plugin install username/repo

安装 / 更新:
  maibot install [选项]
  maibot update [选项]

说明:
  install 和 update 使用同一套安装计划。未指定的选项会优先读取 ~/.maibot_config，
  没有配置时使用推荐默认值。CLI 允许必要的交互确认；如果用于脚本或 Agent，
  可通过下面的策略参数跳过对应询问并直接选择处理方式。
  {default_note}

安装选项:
  --path <目录>                  安装目录，默认读取配置或 ~/maimai
  --branch <main|dev>            MaiBot 分支
  --mode <normal|clean>          更新/修复或清空目录全新安装
  --python <system|uv>           Python 环境
  --venv <keep|recreate>         保留或重建虚拟环境
  --github <auto|direct|URL>     GitHub 线路
  --pip <system|aliyun|tencent|tsinghua|ustc|official|URL>
  --protocol <napcat|llbot|snowluma|none> 协议端（SnowLuma 仅 Linux）
  --docker <one-ms|xuanyuan|official|keep>  Linux NapCat Docker 部署使用；Windows/macOS 会忽略
  --github-fallback <direct|cancel>
  --git-dirty <stash|discard|cancel>
  --napcat-conflict <recreate|cancel>
  --llbot-update <update|skip>
  --snowluma-swap <enable|skip>

安装选项说明:
  --mode normal      保留目标目录，更新或修复已有安装
  --mode clean       清空目标目录后全新安装，会自动重建虚拟环境
  --github auto      并行测速 GitHub 官方线路和镜像源，自动选择最快线路
  --github direct    使用 https://github.com 直连
  --github URL       使用自定义 GitHub 代理前缀
  --pip system       不写 pip.conf，使用系统默认 PyPI 配置
  --pip URL          使用自定义 PyPI 镜像；仅写入当前 venv，不污染全局 pip 配置
  --protocol none    只部署 MaiBot 核心，不安装额外协议端
  --docker keep      Linux 下不修改 /etc/docker/daemon.json；Windows/macOS 下 NapCat 不使用 Docker
  --github-fallback direct
                    GitHub auto 测速全部失败时不再询问，改用官方直连继续
  --github-fallback cancel
                    GitHub auto 测速全部失败时不再询问，直接取消安装
  --git-dirty stash  目标 Git 仓库有本地改动时不再询问，自动 git stash -u 后继续
  --git-dirty discard
                    目标 Git 仓库有本地改动时不再询问，自动丢弃后继续；仅在确认不需要保留改动时使用
  --git-dirty cancel 目标 Git 仓库有本地改动时不再询问，直接取消更新
  --napcat-conflict recreate
                    检测到同名 napcat 容器时不再询问，删除旧容器并继续部署
  --napcat-conflict cancel
                    检测到同名 napcat 容器时不再询问，直接取消部署
  --llbot-update update
                    已安装 LuckyLilliaBot 且有新 release 时不再询问，执行更新并保留 data/default_config.json
  --llbot-update skip
                    已安装 LuckyLilliaBot 且有新 release 时不再询问，跳过更新
  --snowluma-swap enable
                    Linux 内存不超过 4 GB 且尚未启用 Swap 时，自动创建并启用 2 GB Swap
  --snowluma-swap skip
                    跳过 SnowLuma 的 2 GB Swap 提示

CLI 交互策略:
  install/update     默认允许交互确认。单独的 MaiBot uv.lock 改动会自动丢弃；其他 Git 改动、
                    NapCat 容器冲突、LLBot 已安装更新等情况可用上述参数跳过询问。
  TUI                仍会在这些分支展示提示，让用户现场选择。

MaiBot 核心:
  maibot core start [--exec]
  maibot core stop
  maibot core restart
  maibot core status
  maibot core logs [--tail 100] [-f|--follow]
  maibot core exec

说明:
{core_notes}

协议端:
  maibot napcat start|stop|restart|status
  maibot napcat logs [--tail 100] [-f|--follow]
  maibot napcat rebuild
  maibot napcat remove-container
  maibot napcat exec

  maibot snowluma start|stop|restart|status
  maibot snowluma logs [--tail 100] [-f|--follow]
  maibot snowluma rebuild
  maibot snowluma recreate-data
  maibot snowluma remove-container
  maibot snowluma exec

  maibot llbot start|stop|restart|status
  maibot llbot logs [--tail 100] [-f|--follow]
  maibot llbot exec
  maibot llbot password <新密码>

说明:
{protocol_notes}

配置与访问:
  maibot access show
  maibot access init
  maibot access init --yes
  maibot access clear-data
  maibot access clear-data --yes
  maibot access adapter show
  maibot access adapter group-mode <whitelist|blacklist>
  maibot access adapter group-add <群号>
  maibot access adapter group-remove <群号>
  maibot access adapter private-mode <whitelist|blacklist>
  maibot access adapter private-add <QQ>
  maibot access adapter private-remove <QQ>
  maibot access adapter ban-add <QQ>
  maibot access adapter ban-remove <QQ>

说明:
  access show        {access_show_note}
  access init        {access_init_note}
  access init --yes  跳过确认，直接应用访问配置，适合脚本中使用
  access clear-data  清空 MaiBot/data 下除 webui.json 外的所有内容；默认会询问确认
  access clear-data --yes
                    跳过确认直接清理数据文件，适合已确认目标目录的脚本环境
  adapter show       查看 Adapter 群聊、私聊、封禁 QQ 配置
  group-mode         设置群聊名单模式，取值 whitelist 或 blacklist
  private-mode       设置私聊名单模式，取值 whitelist 或 blacklist
  *-add/*-remove     增删对应列表中的纯数字号码

插件:
  maibot plugin list
  maibot plugin install <GitHub地址或username/repo>
  maibot plugin update <插件目录名>
  maibot plugin remove <插件目录名>

说明:
  plugin install     克隆或同步插件仓库；会读取 _manifest.json 中的 id 作为最终目录名
  plugin update      拉取已安装插件仓库的最新提交
  plugin remove      删除 MaiBot/plugins 下对应插件目录

配置文件:
  ~/.maibot_config   记录安装目录、Python 环境、LLBot 路径、安装偏好等

{console_tip}
"#
    );
}
