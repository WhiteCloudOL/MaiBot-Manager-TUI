# MaiBot Manager

面向 Linux 服务器的 MaiBot 一站式部署与运维工具。使用 Rust 编写，是具有 MaiBot 安装、更新、服务管理、协议端管理、配置查看与插件管理能力的单文件程序；支持 TUI 面板，也支持直接通过 CLI 命令执行常用操作。

> 食用文档： https://docs.meowyun.cn/qqbot/maibot/install.html  
> 声明：本项目使用`Claude Code`/`Codex` 协助开发  

## 功能概览  

- **支持CLI/TUI**：支持使用`maibot`或`maibot tui`进入TUI界面；也支持附加参数执行CLI命令，便于AGENT使用MaiBot管理程序。
- **主菜单概览**：进入主菜单即可看到 MaiBot、NapCat、LLBot 的运行状态。
- **安装向导**：单页式安装计划，方向键展开 / 折叠选项，所见即所得。
- **Github优选**：GitHub 官方线路与镜像源并行测速，自动选择最佳线路；全部失败时提供重试 / 直连 / 取消的回退选择。
- **MaiBot管理**：启动、停止、进入 `screen` 控制台。
- **LLBot/Napcat安装**：支持安装MaiBot的同时同步安装LLBot与NapcatQQ与常用命令执行。
- **依赖自检**：进入安装流程时自动检测包管理器，缺失的 `git/curl/screen/unzip/python3` 等基础工具按当前发行版自动补装。
- **配置访问**：集中查看 MaiBot、NapCat、LLBot WebUI 地址与密钥；初始化访问配置带二次确认。
- **插件管理**：安装、卸载插件并按需补装依赖。

## 目录结构

```text
.
├── src/
│   ├── main.rs       # 入口与模块声明
│   ├── app.rs        # App 状态、主菜单、运行状态汇总
│   ├── cli/          # CLI 参数解析与命令分发
│   ├── installer.rs  # 安装计划、向导、部署执行、测速
│   ├── services.rs   # MaiBot / NapCat / LLBot 服务管理
│   ├── access.rs     # WebUI 访问汇总与 Adapter 黑白名单
│   ├── plugins.rs    # 插件安装、卸载、依赖
│   ├── runtime.rs    # 配置 IO、命令执行
│   ├── ui.rs         # 页眉、列宽对齐、提示与 prompt/raw mode 切换
│   ├── model.rs      # 配置模型、安装计划、枚举与常量
│   ├── terminal.rs   # 终端 raw mode、光标恢复、Ctrl+C 清理
│   └── utils.rs      # 路径、shell、列宽对齐、插件工具
├── build-release.sh  # Linux / WSL 构建脚本
├── build-release.ps1 # Windows 调用 WSL 构建脚本
└── output/           # 构建产物，默认被 .gitignore 忽略
```

## 运行要求

目标运行环境：

- Linux x86_64 或 Linux arm64
- 已识别的包管理器之一：`apt` / `dnf` / `yum` / `pacman` / `zypper` / `apk`
- `bash`（其余基础工具 `git` / `curl` / `screen` / `unzip` / `python3` 缺失时会自动通过当前发行版的包管理器补装）
- `python3` 或 `uv`
- 使用 NapCatQQ 时需要 Docker；未安装 Docker 时会按发行版尝试安装：`apt`/`dnf`/`yum` 走 `linuxmirrors.cn/docker.sh` 镜像脚本，Arch / openSUSE / Alpine 走各自原生包
- 使用 LuckyLilliaBot 时会按下述策略自动预装 LinuxQQ：
  - `apt`：官方 deb + 依赖（`libasound2t64` 自动回退到 `libasound2`）
  - `dnf` / `yum` / `zypper`：官方 rpm
  - `pacman`：`yay` 或 `paru` 装 AUR 包 `linuxqq`
  - `apk`：跳过（musl 不支持）

构建环境：

- Rust toolchain
- target：`x86_64-unknown-linux-musl`、`aarch64-unknown-linux-musl`

WSL Ubuntu 安装示例：

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config curl
curl https://sh.rustup.rs -sSf | sh -s -- -y
source ~/.cargo/env
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
```

发布脚本默认使用 musl 静态目标，产物不依赖目标服务器的 GLIBC 版本。

## 自定义配置

仓库根的 `app.toml` 是构建时配置（**非运行时配置**），由 `build.rs` 在 `cargo build` 阶段读取并烘焙进二进制：

```toml
version          = "0.2.2"   # 标题栏显示的版本号
build_label      = "..."     # 标题栏副标题
github_test_path = "..."     # 测速参考文件
github_mirrors   = [...]     # 镜像源候选清单
docker_oneliner  = "..."     # 装 Docker 的一键脚本
```

修改 `app.toml` 后重新执行 `build-release.sh` / `.ps1`，新值即生效；运行时二进制不需要外部配置文件。

## 构建

Windows 通过 WSL 构建：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\build-release.ps1
```

在 Linux 或 WSL 内：

```bash
chmod +x ./build-release.sh
./build-release.sh
```

构建完成后会生成：

```text
output/maibot-manager-x86_64
output/maibot-manager-arm64
```

## 一键安装（推荐）

在 Linux 服务器执行下述命令，会自动识别架构、并行测速 GitHub 镜像、下载最新 release 到 `~/.local/bin/maibot` 并写入 `bash` / `zsh` / `fish` 的 PATH：

```bash
# 国内安装
curl -fsSL https://dl.meowyun.cn/bot/mmtui/install.sh | bash

# 海外安装  
curl -fsSL https://raw.githubusercontent.com/WhiteCloudOL/MaiBot-Manager-TUI/main/scripts/install.sh | bash
```

可选环境变量：

```text
MAIBOT_INSTALL_DIR  安装目录，默认 ~/.local/bin
MAIBOT_FORCE_PROXY  跳过测速，强制使用的镜像（或 direct）
MAIBOT_VERSION      指定版本 tag（如 v0.2.2），默认 latest
```

安装完成后重启终端或 `source` 对应的 rc 文件，即可在任意位置执行 `maibot`。

## 使用 TUI

把对应架构的文件上传到 Linux 服务器后执行：

```bash
chmod +x ./maibot-manager-x86_64
./maibot-manager-x86_64
```

ARM64 服务器：

```bash
chmod +x ./maibot-manager-arm64
./maibot-manager-arm64
```

主菜单进入「安装 / 更新 MaiBot」后会进入单页安装计划：把光标停在配置项上按 `Enter` 展开选项，再次 `Enter` 应用所选。光标会留在你刚操作的位置，跨字段移动也不会跳。

安装到 PATH 后，也可以直接执行：

```bash
maibot
maibot tui
```

## 使用 CLI

查看帮助：

```bash
maibot help
maibot --help
maibot -h
```

CLI 适合在 SSH、脚本或 Agent 工作流中直接执行管理动作。除 `exec` 类命令外，CLI 默认执行完即退出；涉及清空目录、暴露 WebUI、删除冲突容器、处理 Git 本地改动等高风险操作时，程序仍会保留二次确认提示。

### 安装 / 更新

```bash
# 使用当前配置或推荐默认值安装 / 更新
maibot install
maibot update

# 指定安装目录、分支、Python 环境与协议端
maibot install --path ~/maimai --branch main --python uv --protocol napcat

# 全新安装，重建环境，GitHub 直连，使用清华 PyPI
maibot install --mode clean --venv recreate --github direct --pip tsinghua
```

安装参数：

```text
--path <目录>                  安装目录，默认读取配置或 ~/maimai
--branch <main|dev>            MaiBot 分支
--mode <normal|clean>          更新/修复或清空目录全新安装
--python <system|uv>           Python 环境
--venv <keep|recreate>         保留或重建虚拟环境
--github <auto|direct|URL>     GitHub 线路
--pip <system|aliyun|tencent|tsinghua|ustc|official|URL>
--protocol <napcat|llbot|none> 协议端
--docker <one-ms|xuanyuan|official|keep>
```

参数说明：

```text
--mode normal      保留目标目录，更新或修复现有 MaiBot
--mode clean       清空目标目录后全新安装，并强制重建虚拟环境
--python system    使用系统 python3 + venv
--python uv        使用 uv 创建并同步 Python 环境
--github auto      并行测速 GitHub 官方线路和镜像源，自动选择最快线路
--github direct    强制使用 https://github.com 直连
--github URL       使用自定义 GitHub 代理前缀
--pip system       使用系统默认 PyPI 配置
--pip URL          使用自定义 PyPI 镜像，仅写入当前虚拟环境配置
--protocol napcat  安装/更新 NapCatQQ Docker 协议端
--protocol llbot   安装/更新 LuckyLilliaBot Linux CLI 协议端
--protocol none    不安装附加协议端
--docker keep      不修改 Docker daemon 配置
```

未指定的安装参数会优先从 `~/.maibot_config` 读取；没有历史配置时使用推荐默认值。`install` 与 `update` 目前使用同一套安装计划，区别主要是语义表达，便于脚本里写得更清楚。
推荐默认值为安装到当前用户 HOME 下的 `maimai` 目录（等价于 `~/maimai`）、使用 `uv` Python 环境，并安装 NapCatQQ 协议端。

### MaiBot 核心

```bash
maibot core start
maibot core start --exec
maibot core stop
maibot core restart
maibot core status
maibot core logs
maibot core logs --tail 200
maibot core logs --follow
maibot core exec
```

`core logs` 通过 `screen hardcopy` 读取会话缓冲，不会进入或抢占 screen；`core exec` 会进入 `screen -r maibot`，进入前会保留退出提示。

命令说明：

```text
start          后台启动 MaiBot，screen 会话名为 maibot
start --exec   启动后立即进入 screen 控制台，首次运行或排错时推荐
stop           停止 maibot screen 会话
restart        重启 MaiBot 核心
status         输出 running / stopped，适合脚本判断
logs           查看最近 100 行 screen 缓冲日志
logs --tail N  指定输出日志行数
logs --follow  每 2 秒刷新一次 hardcopy 输出，不附着 screen
exec           进入 screen 控制台；退出请按 Ctrl+A 再按 D
```

### 协议端服务

NapCat：

```bash
maibot napcat start
maibot napcat stop
maibot napcat restart
maibot napcat status
maibot napcat logs
maibot napcat logs --tail 200 --follow
maibot napcat rebuild
maibot napcat remove-container
maibot napcat exec
```

NapCat 命令说明：

```text
start             docker compose up -d
stop              docker compose stop
restart           docker compose restart
status            基于 docker ps 判断运行状态
logs              docker compose logs --tail=100
logs --follow     docker compose logs -f
rebuild           docker compose down && pull && up -d
remove-container  删除现有 napcat 容器，不删除镜像和挂载目录
exec              docker exec -it napcat /bin/sh
```

LuckyLilliaBot：

```bash
maibot llbot start
maibot llbot stop
maibot llbot restart
maibot llbot status
maibot llbot logs
maibot llbot logs --tail 200 --follow
maibot llbot exec
maibot llbot password <新密码>
```

LuckyLilliaBot 命令说明：

```text
start          后台启动 LLBot，screen 会话名为 llbot
stop           停止 llbot screen 会话
restart        重启 LLBot
status         输出 running / stopped
logs           查看最近 100 行 screen 缓冲日志
logs --tail N  指定输出日志行数
logs --follow  每 2 秒刷新一次 hardcopy 输出，不附着 screen
exec           进入 LLBot screen 控制台；退出请按 Ctrl+A 再按 D
password       写入 LLBot WebUI 密码文件
```

也可以使用协议端聚合入口：

```bash
maibot protocol napcat restart
maibot protocol llbot logs
```

### 配置与访问

```bash
maibot access show
maibot access init
maibot access adapter show
maibot access adapter group-mode whitelist
maibot access adapter group-add 123456
maibot access adapter group-remove 123456
maibot access adapter private-mode blacklist
maibot access adapter private-add 10001
maibot access adapter private-remove 10001
maibot access adapter ban-add 10001
maibot access adapter ban-remove 10001
```

`access init` 会把 MaiBot WebUI 绑定到 `0.0.0.0` 并启用 Napcat Adapter，执行前仍会二次确认。

Adapter 命令说明：

```text
adapter show                 查看群聊、私聊和封禁 QQ 配置
adapter group-mode MODE      设置群聊名单模式：whitelist 或 blacklist
adapter group-add ID         添加群号到群聊列表
adapter group-remove ID      从群聊列表移除群号
adapter private-mode MODE    设置私聊名单模式：whitelist 或 blacklist
adapter private-add QQ       添加 QQ 到私聊列表
adapter private-remove QQ    从私聊列表移除 QQ
adapter ban-add QQ           添加 QQ 到封禁列表
adapter ban-remove QQ        从封禁列表移除 QQ
```

### 插件管理

```bash
maibot plugin list
maibot plugin install username/repo
maibot plugin install https://github.com/username/repo
maibot plugin deps <插件目录名>
maibot plugin remove <插件目录名>
```

插件命令说明：

```text
list     列出 MaiBot/plugins 下的插件目录
install  支持完整 GitHub URL 或 username/repo；仓库存在时执行更新，并按 _manifest.json 的 id 作为最终插件目录名
deps     为已安装插件重新安装 requirements.txt
remove   删除对应插件目录
```

### 按键

```text
↑ / ↓       移动光标
Home / End  跳到首项 / 末项
Enter       展开或应用选项
Esc         返回上一级
Ctrl+C      退出当前输入并恢复终端
```

## 安装计划

默认计划会根据已有配置自动填充：

- **安装目录**：优先读取 `~/.maibot_config`，否则默认 `~/maimai`
- **安装模式**：默认正常更新 / 修复
- **Python 环境**：沿用历史配置，否则默认 `uv`（Python 3.14）
- **GitHub**：默认执行时并行测速；全部失败提供重试 / 直连 / 取消
- **PyPI**：默认系统源；选自定义源时**只在 venv 目录写 `pip.conf`**，不污染用户全局 `~/.pip/`
- **协议端**：检测已有 NapCat / LLBot；未检测到时默认安装 NapCatQQ

可修改模块：

```
安装目录 / 安装模式 / Python 环境 / 虚拟环境处理 /
GitHub 线路 / PyPI 源 / Bot 协议端 / Docker 镜像
```

## 主菜单运行状态

进入主菜单时会自动检测并显示：

```
● 运行中  或  ○ 未运行
MaiBot       基于 screen 会话 `maibot`
NapCat       基于 `docker ps --filter name=^napcat$`
LLBot        基于 screen 会话 `llbot`
```

未检测到安装时会引导先进入「安装 / 更新 MaiBot」。

## 配置文件

```text
~/.maibot_config
```

记录安装路径、Python 环境和 LLBot 路径，便于管理菜单自动定位。

## 注意事项

- 该程序面向 **Linux 服务器**环境，在 Windows 终端直接运行 Linux 产物不会工作。
- LLBot 安装时会尝试自动安装 LinuxQQ，`apt` 环境下可能需要输入 `sudo` 密码。
- Docker、GitHub、PyPI、LLBot Release 下载都依赖目标服务器的网络。
- `初始化 MaiBot 访问配置` 会把 WebUI 绑定到 `0.0.0.0`，相当于把端口暴露给外网，请确认已设置 token 或防火墙策略。
- NapCat 的 `docker-compose.yml` 仅在首次安装时写入，更新时不会覆盖你的自定义修改；如需重置请手动删除该文件再运行安装。

## 开发

本地快速检查：

```bash
cargo check
```

发布构建：

```bash
./build-release.sh
```
