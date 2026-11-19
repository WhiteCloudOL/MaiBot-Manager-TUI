# 本地同步下载站

`sync_release.py` 在本机运行，不依赖 GitHub Actions。需要 Python 3.10+、OpenSSH 的 `ssh` / `scp`，以及远端 Linux 的 Python 3.10+。

首次配置：将项目根目录的 `.env.example` 复制为 `.env.deploy`，填写 SSH 别名、远端目录、下载目录 URL 和 EdgeOne 站点 ID。脚本复用本机 SSH config 的账号、端口与私钥；远端目录必须已经存在，服务器公钥需要预先登记在本机 `known_hosts`。

腾讯云密钥可以通过 `TENCENTCLOUD_SECRET_ID` / `TENCENTCLOUD_SECRET_KEY` 环境变量提供，或者用 `EDGEONE_CREDENTIALS_CSV` 指向仓库外已有的 SecretId/SecretKey CSV 文件。临时凭证另填 `TENCENTCLOUD_TOKEN`。本机进程环境变量优先于 `.env.deploy`；环境文件不会执行命令或展开变量，路径相对于该文件所在目录解析。

```powershell
Copy-Item .env.example .env.deploy
python scripts/sync_release.py --dry-run
python scripts/sync_release.py
```

Windows 也可以双击 `scripts/sync_release.cmd`。从其他目录调用时，默认仍使用项目根目录的 `.env.deploy`，也可以通过 `--env-file` 指定其他本机配置文件。

每次执行会读取 GitHub 最新**正式版**，下载四个平台安装包并校验发布记录中的 SHA-256 和大小；`install.sh`、`install.ps1` 从该版本对应的 Git 提交获取并校验 Git blob，避免混入本机未发布改动。所有六个文件准备成功后才上传。

远端只替换这六个固定文件，其他文件保留；上传先进入目录旁的临时包，校验成功后逐个原子替换，替换或复核失败时恢复原文件。并发同步由远端文件锁阻止。上传包与正常结束后的临时备份会清理；恢复异常时保留备份并报告位置。远端替换期间如果 SSH 意外断开，请先核实源站文件再重试。

同步及远端摘要校验全部成功后，自动调用 [EdgeOne CreatePurgeTask](https://cloud.tencent.com/document/product/1552/80703)，固定使用 `Type=purge_prefix`、`Method=delete`，仅刷新 `MMTUI_CACHE_URL` 指定的目录缓存。URL 必须包含协议和非根目录，并以 `/` 结尾。不会提交 Hostname 或整个站点刷新。脚本会等待任务实际成功；失败或等待超时均以非零状态退出，源站文件不会因为缓存刷新失败而回滚。

如果文件同步已成功，缓存刷新明确失败，可以单独重新刷新目录：

```powershell
python scripts/sync_release.py --purge-only
```

网络错误导致提交结果不明确时，请先用输出的任务 ID 或 EdgeOne 控制台确认原任务状态，再决定是否重试，避免重复提交。密钥需要 `teo:CreatePurgeTask` 和 `teo:DescribePurgeTasks` 权限。

`.env.deploy` 已被项目的 `.gitignore` 排除。只提交无真实凭据的 `.env.example`；不要将凭据 CSV、SSH 私钥、真实主机/IP、远端目录或站点 ID 写入脚本和公开文档。

## TUI 冒烟检查

`verify_tui_capture.py` 在 Linux / WSL 的独立临时 HOME 中启动真实 PTY，检查首次启动、宽屏、窄屏、切换页面、部署选项、访问弹窗，以及 Ctrl+C 退出后的终端恢复。仅需在独立开发环境中安装 `pyte` 和 `wcwidth`，运行结果保存在忽略的 `target/` 下：

```bash
python scripts/verify_tui_capture.py --exe target/debug/maibot-manager-tui --output target/tui-capture
```
