use crate::model::APP_VERSION;

pub(crate) fn print_help() {
    println!(
        r#"MaiBot Manager {APP_VERSION}

这是 MaiBot 的 Linux 服务器部署与运维工具。
不加参数时进入交互式 TUI；带参数时直接执行对应 CLI 命令，适合脚本、SSH 快速操作和日常维护。

用法:
  maibot                         进入 TUI
  maibot tui                     进入 TUI
  maibot help | -h | --help      查看帮助

常用示例:
  maibot install --path ~/maimai --python uv --protocol napcat
  maibot update --branch main --github auto
  maibot core restart
  maibot core logs --tail 200
  maibot core exec
  maibot napcat restart
  maibot llbot password my-new-password
  maibot access show
  maibot plugin install username/repo

安装 / 更新:
  maibot install [选项]
  maibot update [选项]

说明:
  install 和 update 使用同一套安装计划。未指定的选项会优先读取 ~/.maibot_config，
  没有配置时使用推荐默认值。执行过程中仍会保留必要的风险提示和确认，例如清空目录、
  处理 Git 本地改动、删除冲突的 NapCat 容器等。
  推荐默认值为当前用户 HOME 下的 maimai 目录、uv Python 环境和 NapCatQQ 协议端。

安装选项:
  --path <目录>                  安装目录，默认读取配置或 ~/maimai
  --branch <main|dev>            MaiBot 分支
  --mode <normal|clean>          更新/修复或清空目录全新安装
  --python <system|uv>           Python 环境
  --venv <keep|recreate>         保留或重建虚拟环境
  --github <auto|direct|URL>     GitHub 线路
  --pip <system|aliyun|tencent|tsinghua|ustc|official|URL>
  --protocol <napcat|llbot|none> 协议端
  --docker <one-ms|xuanyuan|official|keep>

安装选项说明:
  --mode normal      保留目标目录，更新或修复已有安装
  --mode clean       清空目标目录后全新安装，会自动重建虚拟环境
  --github auto      并行测速 GitHub 官方线路和镜像源，自动选择最快线路
  --github direct    使用 https://github.com 直连
  --github URL       使用自定义 GitHub 代理前缀
  --pip system       不写 pip.conf，使用系统默认 PyPI 配置
  --pip URL          使用自定义 PyPI 镜像；仅写入当前 venv，不污染全局 pip 配置
  --protocol none    只部署 MaiBot 核心和 Adapter，不安装额外协议端
  --docker keep      不修改 /etc/docker/daemon.json

MaiBot 核心:
  maibot core start [--exec]
  maibot core stop
  maibot core restart
  maibot core status
  maibot core logs [--tail 100] [-f|--follow]
  maibot core exec

说明:
  core start         在 screen 会话 maibot 中后台启动 MaiBot
  core start --exec  启动后立刻进入 screen 控制台，进入前会提示退出方式
  core logs          通过 screen hardcopy 读取日志缓冲，不会进入或抢占 screen
  core logs -f       每 2 秒刷新一次 hardcopy 输出
  core exec          执行 screen -r maibot，适合需要交互控制台时使用

协议端:
  maibot napcat start|stop|restart|status
  maibot napcat logs [--tail 100] [-f|--follow]
  maibot napcat rebuild
  maibot napcat remove-container
  maibot napcat exec

  maibot llbot start|stop|restart|status
  maibot llbot logs [--tail 100] [-f|--follow]
  maibot llbot exec
  maibot llbot password <新密码>

说明:
  napcat             管理 NapCat Docker Compose 服务
  napcat logs        使用 docker compose logs，不进入容器
  napcat exec        进入 napcat 容器 shell
  napcat rebuild     down + pull + up -d，适合重建容器
  llbot              管理 LuckyLilliaBot 的 screen 会话 llbot
  llbot logs         通过 screen hardcopy 读取日志缓冲，不影响 screen 会话
  llbot exec         执行 screen -r llbot，进入前会提示退出方式
  protocol           也可作为聚合入口，例如 maibot protocol napcat restart

配置与访问:
  maibot access show
  maibot access init
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
  access show        显示 MaiBot、NapCat、LLBot 的 WebUI 地址和密钥/密码
  access init        将 MaiBot WebUI 绑定到 0.0.0.0 并启用 Napcat Adapter，执行前会二次确认
  adapter show       查看 Adapter 群聊、私聊、封禁 QQ 配置
  group-mode         设置群聊名单模式，取值 whitelist 或 blacklist
  private-mode       设置私聊名单模式，取值 whitelist 或 blacklist
  *-add/*-remove     增删对应列表中的纯数字号码

插件:
  maibot plugin list
  maibot plugin install <GitHub地址或username/repo>
  maibot plugin remove <插件目录名>
  maibot plugin deps <插件目录名>

说明:
  plugin install     克隆或更新插件仓库；如果存在 requirements.txt 会自动安装依赖
  plugin remove      删除 MaiBot/plugins 下对应插件目录
  plugin deps        为已安装插件重新安装 requirements.txt

配置文件:
  ~/.maibot_config   记录安装目录、Python 环境、LLBot 路径、安装偏好等

Screen 退出提示:
  进入 core exec 或 llbot exec 后，如需退出控制台但保持进程运行，请按 Ctrl+A，再按 D。
"#
    );
}
