<div align="center">

# MaiBot Manager TUI

![MaiBot 1.0.0+](https://img.shields.io/badge/MaiBot-1.0.0+-success.svg)
![TUI + CLI](https://img.shields.io/badge/Interface-TUI%20%2B%20CLI-blue.svg)
![Linux + Windows + macOS](https://img.shields.io/badge/Target-Linux%20%2B%20Windows%20%2B%20macOS-informational.svg)
![x86_64 + ARM64](https://img.shields.io/badge/Arch-x86__64%20%2B%20ARM64%20%2B%20Win64%20%2B%20macOS-orange.svg)
![License](https://img.shields.io/badge/License-AGPL%203.0-lightgrey.svg)

面向 Linux 服务器、Windows 10/11 与 macOS 的 MaiBot 一站式部署与运维工具。使用 Rust 编写，是具有 MaiBot 安装、更新、服务管理、协议端管理、配置查看与插件管理能力的单文件程序；支持 TUI 面板，也支持直接通过 CLI 命令执行常用操作。

</div>

> **食用文档**：[https://docs.meowyun.cn/qqbot/maibot/install.html](https://docs.meowyun.cn/qqbot/maibot/install.html)  
> **声明**：本项目使用 `Claude Code` / `Codex` 协助维护

---

## 🌟 功能概览

* **支持 CLI / TUI**：支持使用 `maibot` 或 `maibot tui` 进入 TUI 界面；也支持附加参数执行 CLI 命令，便于 AGENT 使用 MaiBot 管理程序。
* **现代化 TUI**：Header / Sidebar / Content / Footer 的清爽布局、圆角面板、服务/插件表格、统一底部快捷键和 Nord 极简冷色调，让常用操作不用在脚本式菜单里猜含义。
* **安装向导**：横向步骤条 + 当前项配置面板；左右切换安装项，上下切换当前项选项，安装/更新与恢复默认使用底部快捷键。
* **Github 优选**：GitHub 官方线路与镜像源并行测速，自动选择最佳线路；全部失败时提供重试 / 直连 / 取消的回退选择。
* **MaiBot 管理**：Linux 使用 `screen` 后台会话；Windows 使用独立进程 / 窗口启动；macOS 使用后台子进程，退出管理器后核心仍继续运行。首次启动需要确认 EULA 时，TUI 会提供交互终端选项，10 秒未选择则默认后台启动。
* **LLBot / Napcat 安装**：Linux 使用 LLBot CLI + NapCat Docker；Windows 使用 LLBot Desktop + NapCat Shell，并在启动时请求管理员权限；macOS 当前保留清晰的平台能力说明入口。
* **依赖自检**：Linux 自动检测包管理器并补装基础工具；Windows 缺少 Git / uv / Python 时会优先下载便携工具到 MaiBot 安装目录；macOS 缺少 Homebrew 时会调用官方脚本安装，并通过 Homebrew 补齐 Git / uv / Python。
* **配置访问**：集中查看当前平台已支持的 WebUI 地址与密钥；TUI 内使用居中弹窗展示汇总，CLI 直接输出文本；初始化访问配置带二次确认。
* **插件管理**：安装、卸载插件并按需补装依赖。


---

## 🚀 一键安装（推荐）

### Linux

在 Linux 服务器执行下述命令，会自动识别架构、并行测速 GitHub 镜像、下载最新 release 到 `~/.local/bin/maibot` 并写入 `bash` / `zsh` / `fish` 的 PATH：

```bash
# 国内安装
curl -fsSL https://dl.meowyun.cn/bot/mmtui/install.sh | bash

# 海外安装
curl -fsSL https://raw.githubusercontent.com/WhiteCloudOL/MaiBot-Manager-TUI/main/scripts/install.sh | bash

```

**可选环境变量：**

* `MAIBOT_INSTALL_DIR`：安装目录，默认 `~/.local/bin`
* `MAIBOT_FORCE_PROXY`：跳过测速，强制使用的镜像（或 `direct`）
* `MAIBOT_VERSION`：指定版本 tag（如 `v0.3.0`），默认 `latest`

> 安装完成后重启终端或 `source` 对应的 rc 文件，即可在任意位置执行 `maibot`。

### Windows 10/11

在 PowerShell 中执行下述命令，会下载最新包含 Windows 资产的 release / prerelease 到用户目录，并写入用户 PATH：

```powershell
# 国内安装
irm https://dl.meowyun.cn/bot/mmtui/install.ps1 | iex

# 海外安装
irm https://raw.githubusercontent.com/WhiteCloudOL/MaiBot-Manager-TUI/main/scripts/install.ps1 | iex
```

**可选环境变量 / 参数：**

* `MAIBOT_INSTALL_DIR` / `-InstallDir`：安装目录，默认 `%LOCALAPPDATA%\Programs\MaiBotManager`
* `MAIBOT_FORCE_PROXY` / `-ForceProxy`：强制使用 GitHub 镜像（或 `direct`）
* `MAIBOT_VERSION` / `-Version`：指定版本 tag；不指定时会从 release 列表选择最新含 Windows 资产的版本

> Windows 安装脚本本身不需要管理员权限；启动 NapCat Shell 的 `launcher.bat` 和 LLBot Desktop 的 `llbot.exe` 时，程序会通过 UAC 请求管理员权限。
> Windows 版管理 MaiBot 时不会强制安装全局 Git / Python / uv；缺失依赖会放在你选择的 MaiBot 安装目录下，例如 `D:\Apps\maimai\tools`。

### macOS

macOS 版目前支持 MaiBot 核心安装 / 更新、后台子进程运行、访问配置与插件管理；NapCat / LLBot 协议端在当前平台保留说明入口，安装计划默认不安装协议端。

```bash
# 国内安装
curl -fsSL https://dl.meowyun.cn/bot/mmtui/install.sh | bash

# 海外安装
curl -fsSL https://raw.githubusercontent.com/WhiteCloudOL/MaiBot-Manager-TUI/main/scripts/install.sh | bash

maibot install --protocol none
```

> macOS 安装 MaiBot 时会优先使用本机原生命令；缺少 Homebrew 时会调用 Homebrew 官方安装脚本，缺少 Git / uv / Python 时会通过 Homebrew 补齐。启动核心时，管理器默认创建后台子进程，退出管理器后 MaiBot 仍会继续运行；首次启动 / EULA 可选择打开交互 Terminal，输出写入 `logs/maibot.log`。

---

## 📂 目录结构

```text
.
├── src/
│   ├── main.rs       # 入口与平台模块选择
│   ├── cli/          # 共享 CLI 参数解析与命令分发
│   ├── linux/        # Linux 专属安装、服务、访问、插件与命令执行
│   ├── macos/        # macOS 专属安装、服务、访问、插件与命令执行
│   ├── win/          # Windows 专属安装、服务、访问、插件与 BAT 执行
│   ├── ui.rs         # 共享现代 TUI Header/Sidebar/Content/Footer、表格、模态框与 raw mode 切换
│   ├── model.rs      # 共享配置模型、安装计划、状态机、枚举与常量
│   └── terminal.rs   # 终端 raw mode、光标恢复、Ctrl+C 清理
├── scripts/
│   ├── install.sh    # Linux / macOS 一键安装脚本
│   └── install.ps1   # Windows 一键安装脚本
├── build-release.sh  # Linux / WSL / macOS 构建脚本
├── build-release.ps1 # Windows 构建 exe，并调用 WSL 构建 Linux 产物
└── output/           # 构建产物，默认被 .gitignore 忽略

```

---

## 💻 运行与构建要求

### 目标运行环境

**Linux：**

* **系统架构**：Linux x86_64 或 Linux arm64
* **包管理器**：已识别的包管理器之一：`apt` / `dnf` / `yum` / `pacman` / `zypper` / `apk`
* **基础工具**：`bash`（其余基础工具 `git/curl/screen/unzip/python3` 缺失时会自动通过当前发行版的包管理器补装）
* **Python 环境**：`python3` 或 `uv`
* **NapCatQQ 依赖 (Docker)**：使用 NapCatQQ 时需要 Docker；未安装 Docker 时会按发行版尝试安装：`apt`/`dnf`/`yum` 走 `linuxmirrors.cn/docker.sh` 镜像脚本，Arch / openSUSE / Alpine 走各自原生包。
* **LuckyLilliaBot 依赖 (LinuxQQ)**：使用 LLBot 时会按下述策略自动预装 LinuxQQ：
* `apt`：官方 deb + 依赖（`libasound2t64` 自动回退到 `libasound2`）
* `dnf` / `yum` / `zypper`：官方 rpm
* `pacman`：`yay` 或 `paru` 装 AUR 包 `linuxqq`
* `apk`：跳过（musl 不支持）

**Windows：**

* **系统架构**：Windows 10/11 x86_64
* **基础工具**：Windows 自带 `curl.exe` / `tar.exe`；缺少 Git / uv 时会自动下载到 `<MaiBot安装目录>\tools\git` 与 `<MaiBot安装目录>\tools\uv`
* **Python 环境**：推荐 `uv`；缺少系统 Python 时会通过安装目录内的 uv 创建本地虚拟环境，并把 uv 缓存与托管 Python 固定到 `<MaiBot安装目录>\tools`
* **NapCatQQ**：通过 GitHub API 获取最新 `NapCat.Shell.zip`，启动 `launcher.bat` 时请求管理员权限，不使用 Docker
* **LuckyLilliaBot**：通过 GitHub API 获取最新 `LLBot-Desktop-win-x64.zip`，启动 `llbot.exe` 时请求管理员权限

**macOS：**

* **系统架构**：macOS x86_64 或 Apple Silicon
* **基础工具**：优先使用 macOS 自带 `curl` / `unzip`；缺少 Homebrew 时自动调用官方脚本安装，缺少 Git / uv / Python 时通过 Homebrew 补齐
* **Python 环境**：推荐 `uv`；uv 缓存与托管 Python 目录固定在 `<MaiBot安装目录>/tools`
* **协议端**：NapCat / LLBot 在当前平台保留说明入口，macOS 默认只部署 MaiBot 核心


### 构建环境

* Rust toolchain
* Linux target：`x86_64-unknown-linux-musl`、`aarch64-unknown-linux-musl`
* Windows target：`x86_64-pc-windows-msvc`
* macOS target：当前宿主架构（`cargo build --release`）

**WSL Ubuntu 安装依赖示例：**

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config curl
curl https://sh.rustup.rs -sSf | sh -s -- -y
source ~/.cargo/env
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl

```

> 发布脚本默认使用 musl 静态目标，产物不依赖目标服务器的 GLIBC 版本。
> GitHub Actions 自动构建在 `main` 与 `dev` 分支触发：`main` 发布 latest 稳定 release，`dev` 发布 `<version>-dev-<SHA>` prerelease。

---

## ⚙️ 自定义配置

仓库根的 `app.toml` 是构建时配置（**非运行时配置**），由 `build.rs` 在 `cargo build` 阶段读取并烘焙进二进制：

```toml
version          = "0.3.0"   # 标题栏显示的版本号
header_title     = "..."     # 标头第一行标题
header_subtitle  = "..."     # 标头第二行副标题
header_credit    = "..."     # 作者 / License 行
header_docs      = "..."     # 文档站行
build_label      = "..."     # 兼容旧字段：header_subtitle 为空时使用
github_test_path = "..."     # 测速参考文件
github_mirrors   = [...]     # 镜像源候选清单
docker_oneliner  = "..."     # 装 Docker 的一键脚本

```

修改 `app.toml` 后重新执行 `build-release.sh` / `.ps1`，新值即生效；运行时二进制不需要外部配置文件。

---

## 🔨 构建指南

**Windows 主机完整构建：**

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\build-release.ps1

```

默认会先在 Windows 本机尝试构建 `x86_64-pc-windows-msvc`，再调用 WSL Ubuntu 构建 Linux musl 双架构。可用 `-SkipWindows` 或 `-SkipLinux` 跳过其中一类产物；macOS 产物需在 macOS 本机或 GitHub Actions 中构建。

Windows 发布脚本默认把 Cargo target 放在 `target/build-release-windows`，避免覆盖正在手动运行的默认 `target` exe。`output/` 产物命名固定；若 `output/maibot-manager-windows-x86_64.exe` 正被占用，脚本会明确失败，请关闭占用进程后重跑。可选参数：`-WindowsTargetDir <path>`、`-OutputDir <path>`、`-WslDistro <name>`、`-PauseAtEnd`。

**在 Linux / WSL 或 macOS 本机：**

```bash
chmod +x ./build-release.sh
./build-release.sh

```

构建完成后会在 `output/` 目录生成：

```text
# Linux / WSL
output/maibot-manager-linux-x86_64
output/maibot-manager-linux-arm64

# Windows build-release.ps1
output/maibot-manager-windows-x86_64.exe

# macOS
output/maibot-manager-macos-x86_64
output/maibot-manager-macos-arm64

```

---

## 🖥️ 界面使用说明

### 使用 TUI 面板

把对应平台/架构的文件放到目标机器后执行：

```bash
# x86_64 服务器
chmod +x ./maibot-manager-linux-x86_64
./maibot-manager-linux-x86_64

# ARM64 服务器
chmod +x ./maibot-manager-linux-arm64
./maibot-manager-linux-arm64

# Windows 10/11
.\maibot-manager-windows-x86_64.exe

# macOS 本机构建产物
./target/release/maibot-manager-tui

```

现代 TUI 使用固定 Header、左侧 Sidebar、主 Content Area 和底部 Footer。侧边栏包含 `概览`、`部署与更新`、`核心服务管理`、`协议端服务`、`插件中心`、`设置`、`关于`；中间区域按当前功能显示概览详情、部署向导、服务表格、插件表格或关于信息。

底部状态栏会统一显示当前可用按键，界面其他区域不会重复散落快捷键提示。部署页使用横向步骤条：`←/→` 切换安装配置项，`↑/↓` 调整当前配置项的候选值，`F5` 开始安装/更新，`Ctrl+R` 将当前表单恢复为推荐默认值。`Ctrl+1` 可从内容区或弹窗快速回到侧边栏。

核心服务、协议端和插件中心使用全宽表格呈现，列为名称、状态、版本与快捷操作；按 `Enter` 后会弹出居中的操作对话框，底层表格保持干净。访问汇总、平台能力说明等纯信息内容会在当前 ratatui 会话内直接打开弹窗，不会短暂跳回空白页或旧式回车返回页面；弹窗按内容收缩，保持与 Nord 面板一致。界面使用 Nerd Font 友好的图标和 Nord 柔和状态色显示状态；没有 Nerd Font 时文字标签仍可读。

TUI 全局使用 Nord 冷色调：背景 `#2E3440`、常规文本 `#D8DEE9`、焦点边框 `#88C0D0`、选中行 `#81A1C1`、运行/警告/错误分别为 `#A3BE8C`、`#EBCB8B`、`#BF616A`。

安装到 PATH 后，也可以直接执行 `maibot` 或 `maibot tui`。

**响应性能说明：**

TUI 的内容区移动和普通操作弹窗打开走缓存重绘路径，不会在每次按键时重新探测服务状态。访问汇总这类信息弹窗只在用户明确打开时读取对应报告，并在当前界面内重绘；公网 IP 只在可见入口确实需要外部地址时查询。Linux 状态页会批量读取 `screen -list`，NapCat 使用带短超时的 `docker ps` 探测；服务日志摘要只读取文件尾部，避免大日志拖慢切换。

**按键操作说明：**

| 按键 | 功能 |
| --- | --- |
| `↑` / `↓` | 在当前所在的侧边栏、表格或部署选项中移动 |
| `←` / `→` | 部署页切换安装配置项；弹窗中切换操作按钮 |
| `Tab` / `Shift+Tab` | 在侧边栏与内容区之间切换焦点 |
| `Enter` | 进入内容区、打开操作弹窗，或编辑部署路径 |
| `F5` | 在部署页执行安装 / 更新 |
| `Ctrl+R` | 在部署页恢复推荐默认配置 |
| `Ctrl+1` | 快速回到侧边栏 |
| `Backspace` | 清空当前筛选 |
| `Esc` | 返回侧边栏或关闭弹窗 |
| `Ctrl+Q` / `Ctrl+C` | 退出 TUI 并恢复终端 |

### 主菜单运行状态识别

进入主菜单时会自动检测并显示（● 运行中 或 ○ 未运行）：

* **Linux MaiBot / LLBot**：基于 `screen` 会话
* **Linux NapCat**：基于 Docker 容器状态
* **Windows MaiBot / LLBot**：基于 Windows 进程 / 窗口状态
* **Windows NapCat**：基于 NapCat Shell 进程状态
* **macOS MaiBot**：基于 `logs/maibot.pid` 记录的子进程状态；NapCat / LLBot 显示当前平台能力说明

未检测到安装时会引导先进入「安装 / 更新 MaiBot」。

---

## ⌨️ 使用 CLI 命令

CLI 适合在 SSH、脚本或 Agent 工作流中直接执行管理动作。除 `exec` 类命令外，CLI 默认执行完即退出。
查看帮助：`maibot help` / `maibot --help` / `maibot -h`

### 1. 安装与更新

```bash
# 默认配置安装/更新
maibot install
maibot update

# 指定安装目录、分支、Python 环境与协议端
maibot install --path ~/maimai --branch main --python uv --protocol napcat

# macOS 当前只部署 MaiBot 核心
maibot install --path ~/maimai --branch main --python uv --protocol none

# 全新安装，重建环境，GitHub 直连，使用清华 PyPI
maibot install --mode clean --venv recreate --github direct --pip tsinghua

# 脚本中自动化更新：发现 LLBot 新 release 不询问；处理风险分支；解决 NapCat 容器冲突
maibot update --protocol llbot --llbot-update update \
              --git-dirty stash --napcat-conflict recreate

```

**CLI 安装核心参数说明：**

| 参数与语法 | 可选值/说明 |
| --- | --- |
| `--path <目录>` | 安装目录，默认读取配置或 `~/maimai` |
| `--branch <main|dev>` | 部署的 MaiBot 分支 |
| `--mode <normal|clean>` | `normal`: 保留目录更新修复；`clean`: 清空目录全新安装并强制重建虚拟环境 |
| `--python <system|uv>` | `system`: 系统 Python + venv；`uv`: 使用 uv 创建并同步环境。Windows 缺少系统 Python 时会自动使用安装目录内的 uv 创建本地 venv；macOS 缺少 uv/Python 时通过 Homebrew 补齐 |
| `--venv <keep|recreate>` | 保留或强制重建虚拟环境 |
| `--github <auto|direct|URL>` | `auto`: 并行测速；`direct`: 强制官方直连；`URL`: 自定义代理前缀 |
| `--pip <system|aliyun...|URL>` | 系统源/内置国内源/自定义源（仅写入当前虚拟环境配置，不污染全局） |
| `--protocol <napcat|llbot|none>` | 选择绑定安装的底层协议端；macOS 目前仅支持 `none` |
| `--docker <one-ms|official...|keep>` | Linux Docker 换源/官方脚本，或 `keep` 不修改 Docker daemon 配置；Windows/macOS 会忽略 |

**自动化静默/回退策略参数：**

| 参数与语法 | 行为说明 |
| --- | --- |
| `--github-fallback direct` | GitHub auto 测速全部失败时跳过询问，改用官方直连继续 |
| `--github-fallback cancel` | GitHub auto 测速全部失败时跳过询问，直接取消安装 |
| `--git-dirty stash` | 目标 Git 仓库有本地改动时跳过询问，自动 `git stash -u` 后继续 |
| `--git-dirty discard` | 跳过询问，自动丢弃本地改动后继续；只应在确认不需要保留改动时使用 |
| `--git-dirty cancel` | 跳过询问，直接取消更新 |
| `--napcat-conflict recreate` | 检测到同名 napcat 容器时跳过询问，删除旧容器并继续部署 |
| `--napcat-conflict cancel` | 检测到同名 napcat 容器时跳过询问，直接取消部署 |
| `--llbot-update update` | 有新 release 时跳过询问，执行更新并保留 `data/default_config.json` |
| `--llbot-update skip` | 有新 release 时跳过询问，保留当前 LLBot 不更新 |

> 未指定的安装参数优先从 `~/.maibot_config` 读取，无历史记录则采用推荐默认值（Linux/Windows 为 `~/maimai` 目录、`uv` 环境、`NapCatQQ` 协议端；macOS 为 `~/maimai` 目录、`uv` 环境、无协议端）。
> *特殊逻辑*：如果 MaiBot 主仓库只有 `uv.lock` 一个文件被修改，程序会自动丢弃该锁文件改动继续同步上游；其他本地改动则按 `--git-dirty` 策略处理。

### 2. MaiBot 核心管理

```bash
maibot core start        # Linux: 后台 screen；Windows: 独立窗口；macOS: 后台子进程
maibot core start --exec # Linux: 启动后进入 screen；Windows: 等同于 start；macOS: 附加当前终端用于 EULA
maibot core stop         # 停止 MaiBot 核心
maibot core restart      # 重启 MaiBot 核心
maibot core status       # 输出 running / stopped，适合脚本判断
maibot core logs         # 查看最近 100 行日志
maibot core logs --tail 200     # 指定输出行数
maibot core logs --follow       # 每 2 秒刷新日志输出
maibot core exec         # Linux: 进入 screen；Windows: 提示独立窗口；macOS: 跟随 logs/maibot.log

```

### 3. 协议端服务管理

**NapCatQQ：**

```bash
maibot napcat start             # Linux: docker compose up -d；Windows: 管理员启动 launcher.bat；macOS: 显示当前平台能力说明
maibot napcat stop              # Linux: docker compose stop；Windows: 停止 NapCat Shell 进程
maibot napcat restart           # 重启 NapCat
maibot napcat status            # 输出 running / stopped
maibot napcat logs              # 查看最近 100 行日志
maibot napcat logs --tail 200 --follow # 实时跟随日志
maibot napcat rebuild           # Linux: down + pull + up -d；Windows: 重新下载最新 NapCat Shell 包；macOS: 显示当前平台能力说明
maibot napcat remove-container  # Linux: 删除 napcat 容器；Windows/macOS 不使用 Docker
maibot napcat exec              # Linux: 进入容器 shell；Windows: 管理员启动 launcher.bat；macOS: 显示当前平台能力说明

```

**LuckyLilliaBot：**

```bash
maibot llbot start              # Linux: 后台 screen；Windows: 管理员启动 llbot.exe；macOS: 显示当前平台能力说明
maibot llbot stop               # 停止
maibot llbot restart            # 重启
maibot llbot status             # 输出 running / stopped
maibot llbot logs               # 缓冲日志
maibot llbot logs --tail 200 --follow
maibot llbot exec               # Linux: 进入 screen 控制台；Windows: 提示 Desktop 程序窗口；macOS: 显示当前平台能力说明
maibot llbot password <新密码>   # 写入 LLBot WebUI 密码文件

```

*(注：也可以使用聚合入口，例如 `maibot protocol napcat restart`)*

### 4. 配置与访问 (Access)

```bash
maibot access show              # 直接输出配置地址与密钥
maibot access init              # 绑定 WebUI 到 0.0.0.0（Linux/Windows 同时启用 Napcat Adapter，macOS 保留核心配置能力）
maibot access init --yes        # 跳过交互确认直接应用（适合确信防火墙安全的脚本环境）

# Adapter 黑白名单设置
maibot access adapter show                      # 查看群聊、私聊和封禁 QQ 配置
maibot access adapter group-mode whitelist      # 设置群聊名单模式：whitelist 或 blacklist
maibot access adapter group-add 123456          # 添加群号到群聊列表
maibot access adapter group-remove 123456       # 从群聊列表移除群号
maibot access adapter private-mode blacklist    # 设置私聊名单模式：whitelist 或 blacklist
maibot access adapter private-add 10001         # 添加 QQ 到私聊列表
maibot access adapter private-remove 10001      # 从私聊列表移除 QQ
maibot access adapter ban-add 10001             # 添加 QQ 到封禁列表
maibot access adapter ban-remove 10001          # 从封禁列表移除 QQ

```

### 5. 插件管理

```bash
maibot plugin list                             # 列出 MaiBot/plugins 下的插件目录
maibot plugin install username/repo            # 安装/更新插件 (支持完整 URL 或 username/repo，以 _manifest.json 的 id 命名目录)
maibot plugin deps <插件目录名>                  # 为已安装插件重新安装 requirements.txt
maibot plugin remove <插件目录名>                # 删除对应插件目录

```

---

## 📝 系统文件与配置文件

**安装计划加载逻辑**：
默认计划会根据已有配置自动填充：

* **安装目录**：优先读取 `~/.maibot_config`，否则默认 `~/maimai`
* **安装模式**：默认正常更新 / 修复
* **Python 环境**：沿用历史配置，否则默认 `uv`（Python 3.14）
* **GitHub**：默认执行时并行测速；全部失败提供重试 / 直连 / 取消
* **PyPI**：默认系统源；选自定义源时**只在 venv 目录写 `pip.conf`**，不污染用户全局 `~/.pip/`
* **协议端**：Linux/Windows 检测已有 NapCat / LLBot，未检测到时默认安装 NapCatQQ；macOS 默认不安装协议端，并在协议端入口说明当前平台能力
*(可修改模块：安装目录 / 安装模式 / Python 环境 / 虚拟环境处理 / GitHub 线路 / PyPI 源 / Bot 协议端 / Docker 镜像；macOS 默认隐藏协议端 / Docker 安装项)*

**配置文件记录**：
`~/.maibot_config` 用于记录安装路径、Python 环境和 LLBot 路径，便于管理菜单自动定位。

---

## ⚠️ 注意事项

* 请下载与平台匹配的产物：Linux 使用 `maibot-manager-linux-x86_64` / `maibot-manager-linux-arm64`，Windows 使用 `maibot-manager-windows-x86_64.exe`，macOS 使用 `maibot-manager-macos-x86_64` / `maibot-manager-macos-arm64`。
* Linux LLBot 安装时会尝试自动安装 LinuxQQ，`apt` 环境下可能需要输入 `sudo` 密码。
* Windows NapCat Shell 与 LLBot Desktop 启动时会请求管理员权限，这是上游程序运行需要。
* Windows 缺失 Git / uv / Python 时会在 MaiBot 安装目录的 `tools` 子目录准备便携工具链，不会写入系统安装目录。
* macOS 缺少 Homebrew 时会调用 Homebrew 官方安装脚本；NapCat / LLBot 在当前平台保留说明入口。
* Docker、GitHub、PyPI、NapCat / LLBot Release 下载都依赖目标机器的网络。
* `初始化 MaiBot 访问配置` 会把 WebUI 绑定到 `0.0.0.0`，相当于把端口暴露给外网，请确认已设置 token 或防火墙策略。
* NapCat 的 `docker-compose.yml` 仅在首次安装时写入，更新时不会覆盖你的自定义修改；如需重置请手动删除该文件再运行安装。

---

## 🛠️ 维护指南

**本地快速检查：**

```bash
cargo check

```

**TUI 改动建议检查：**

```powershell
cargo fmt
C:\Users\white\.cargo\bin\cargo.exe check
C:\Users\white\.cargo\bin\cargo.exe clippy --all-targets -- -D warnings
C:\Users\white\.cargo\bin\cargo.exe build --release --target x86_64-pc-windows-msvc --target-dir target\windows-verify
target\windows-verify\x86_64-pc-windows-msvc\release\maibot-manager-tui.exe --help
target\windows-verify\x86_64-pc-windows-msvc\release\maibot-manager-tui.exe tui

```

```bash
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI && cargo build"
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI && cargo check --target x86_64-unknown-linux-musl"
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI && python3 scripts/verify_tui_capture.py --cwd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI --exe ./target/debug/maibot-manager-tui --cols 132 --rows 42 --mode wide"
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI && python3 scripts/verify_tui_capture.py --cwd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI --exe ./target/debug/maibot-manager-tui --cols 72 --rows 28 --mode narrow"
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI && python3 scripts/verify_tui_capture.py --cwd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI --exe ./target/debug/maibot-manager-tui --cols 132 --rows 42 --mode tabs"
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI && python3 scripts/verify_tui_capture.py --cwd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI --exe ./target/debug/maibot-manager-tui --cols 100 --rows 34 --mode deploy"
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI && python3 scripts/verify_tui_capture.py --cwd /mnt/d/Coding/GithubProject/MaiBot-Manager-TUI --exe ./target/debug/maibot-manager-tui --cols 132 --rows 30 --mode access"

```

**发布构建：**

```bash
./build-release.sh

```

Windows 主机请使用 `.\build-release.ps1`；它会先构建 Windows x86_64，再通过 WSL 调用 `build-release.sh` 构建 Linux x86_64/arm64，并过滤 WSL 本身可能输出的 localhost 代理乱码警告。
