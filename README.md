# MaiBot Manager TUI

面向 Linux 服务器的 MaiBot 一站式部署与运维终端面板。使用 Rust 编写，是具有MaiBot安装、更新、服务管理、协议端管理、配置查看、LPMM 与插件管理能力的单文件程序。  

> 声明：本项目使用`Claude Code`/`Codex` 进行开发  

## 功能概览  

- **单文件发布**：构建后输出 `x86_64` 与 `arm64` 两个 Linux 可执行文件。
- **主菜单概览**：进入主菜单即可看到 MaiBot、NapCat、LLBot 的运行状态。
- **安装向导**：单页式安装计划，方向键展开 / 折叠选项，所见即所得。
- **并行测速**：GitHub 官方线路与镜像源并行测速，自动选择最佳线路；全部失败时提供重试 / 直连 / 取消的回退选择。
- **MaiBot 管理**：启动、停止、进入 `screen` 控制台。
- **协议端管理**：NapCatQQ Docker 管理 + LuckyLilliaBot CLI 管理。
- **LLBot 辅助安装**：安装 LuckyLilliaBot 时自动按 PM 适配预装 LinuxQQ（`apt`/`dnf`/`yum`/`zypper`/`pacman+yay/paru`）。
- **依赖自检**：进入安装流程时自动检测包管理器，缺失的 `git/curl/screen/unzip/python3` 等基础工具按当前发行版自动补装。
- **配置访问**：集中查看 MaiBot、NapCat、LLBot WebUI 地址与密钥；初始化访问配置带二次确认。
- **LPMM 管理**：知识库目录初始化、前台 / 后台执行提取与导入任务。
- **插件管理**：安装、卸载插件并按需补装依赖。
- **原始脚本兼容模式**：内置 `maibot.sh`，必要时可回退原脚本流程。

## 目录结构

```text
.
├── src/
│   ├── main.rs       # 入口与模块声明
│   ├── app.rs        # App 状态、主菜单、运行状态汇总
│   ├── installer.rs  # 安装计划、向导、部署执行、测速
│   ├── services.rs   # MaiBot / NapCat / LLBot 服务管理
│   ├── access.rs     # WebUI 访问汇总与 Adapter 黑白名单
│   ├── lpmm.rs       # LPMM 目录、提取、导入流程
│   ├── plugins.rs    # 插件安装、卸载、依赖
│   ├── runtime.rs    # 配置 IO、命令执行、原脚本兼容入口
│   ├── ui.rs         # 页眉、列宽对齐、提示与 prompt/raw mode 切换
│   ├── model.rs      # 配置模型、安装计划、枚举与常量
│   ├── terminal.rs   # 终端 raw mode、光标恢复、Ctrl+C 清理
│   └── utils.rs      # 路径、shell、列宽对齐、插件工具
├── maibot.sh         # 原始脚本，构建时嵌入兼容模式
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
- target：`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu`
- ARM64 交叉编译器：`gcc-aarch64-linux-gnu`

WSL Ubuntu 安装示例：

```bash
sudo apt-get update
sudo apt-get install -y build-essential gcc-aarch64-linux-gnu pkg-config curl
curl https://sh.rustup.rs -sSf | sh -s -- -y
source ~/.cargo/env
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
```

仓库中的 `.cargo/config.toml` 已为 `aarch64-unknown-linux-gnu` 配置交叉链接器。

## 自定义配置

仓库根的 `app.toml` 是构建时配置（**不是运行时配置**），由 `build.rs` 在 `cargo build` 阶段读取并烘焙进二进制：

```toml
version          = "0.1.1"   # 标题栏显示的版本号
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

## 使用

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