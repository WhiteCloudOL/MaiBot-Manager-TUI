# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Build / run

The binary supports Linux, Windows 10/11, and macOS. `main.rs` selects platform implementations at compile time and bails on other OSes. Use WSL for Linux builds on Windows hosts; use the GitHub Actions Windows job or a local Windows Rust toolchain for Windows release checks; use a local macOS Rust toolchain for macOS checks.

```bash
# Quick check
cargo check --target x86_64-unknown-linux-musl

# Linux release build, both architectures (what build-release.sh does on Linux/WSL)
cargo build --release --target x86_64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl

# Windows release build
cargo build --release --target x86_64-pc-windows-msvc

# macOS local release build, both artifacts (what build-release.sh does on macOS)
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

# Full local pipelines
./build-release.sh                # inside Linux / WSL for Linux artifacts, or macOS for macOS artifacts
.\build-release.ps1               # Windows host, builds Windows exe then Linux binaries via WSL
```

Linux release builds use musl static targets so the binaries do not depend on the target server's GLIBC version. Windows release builds target Windows 10/11 x86_64. macOS builds currently target the local host architecture. There are no integration tests; unit tests may exist for platform helpers.
`build-release.ps1` intentionally uses `target/build-release-windows` instead of Cargo's default target dir so a manually launched TUI exe cannot block release builds. `output/` artifact names are fixed release contract names; do not write timestamped or alternate release filenames. If `output/maibot-manager-windows-x86_64.exe` is running, the script should fail with a clear close-and-rerun message. WSL output is captured through `Start-Process` redirected files rather than `2>&1 | ...` because Windows PowerShell can treat native stderr as terminating errors under `$ErrorActionPreference = "Stop"`; keep filtering the known WSL localhost proxy warning so build logs do not get mojibake.

GitHub Actions publishes `main` pushes as the stable latest release tagged `v<version>`, and `dev` pushes as prereleases tagged `<version>-dev-<short-sha>` (for example `0.3.0-dev-abcdef1`). Release notes should compare the current build with the latest stable GitHub Release and group conventional commits under professional sections such as `Feature:` and `Fix:`.
The release workflow runs on `main` and `dev` pushes. Keep both branches in `.github/workflows/release.yml` when changing workflow triggers. It builds Linux x86_64/arm64, Windows x86_64, and macOS x86_64/arm64 artifacts. Build both macOS artifacts on the newer Apple Silicon `macos-15` GitHub-hosted runner; the x86_64 macOS artifact is cross-compiled there to avoid waiting on older Intel runner capacity.

Windows and macOS support are compiled as separate targets. Linux-only implementations live in `src/linux/`, Windows-only implementations live in `src/win/`, macOS-only implementations live in `src/macos/`, and `src/main.rs` selects them with `#[cfg(target_os = "...")]` plus `#[path = "..."]`. Do not import non-current-platform modules from shared code, because release binaries should not include dead platform code.

## Architecture

This is a single-binary TUI + CLI that orchestrates platform shell commands to install/manage MaiBot. Almost every "action" — clone, venv, docker/screen/process launch, system package setup — is a shell string executed via `App::run_shell`, not native Rust. Treat shell command strings as the primary IR; the Rust code is a config builder + menu/CLI driver around them.

**Entry behavior.** `main.rs` parses args before the Linux guard only for global help (`help`, `-h`, `--help`) so help can be printed anywhere. No args or `tui` enters the TUI. Any other first arg dispatches to `App::run_cli`. Keep this behavior when adding commands.

**Module shape.** Every domain module (`installer`, `services`, `access`, `plugins`, `runtime`, `ui`) is `impl App for ...` — they share state via `App` (theme + config path) rather than holding their own types. `app.rs` is the TUI shell that prints the main menu and dispatches; `src/cli/` is the CLI shell. Keep CLI parsing split by domain (`install`, `services`, `access`, `plugins`, `help`) instead of growing one giant command file.

**Windows implementation.** Windows commands should prefer BAT/cmd syntax executed through `App::run_shell`, which writes a temporary `.bat` and invokes `cmd.exe /C`. Use PowerShell only when cmd has no good primitive, currently UAC elevation via `Start-Process -Verb RunAs`, log tailing, process-window fallbacks, and launching the MaiBot BAT wrapper with `Start-Process -PassThru` so the manager can record a PID. Do not use `winget` or install required tooling globally from the manager. If Git / uv / Python are missing, Windows install should prepare portable tooling under `<install>/tools` (`tools/git`, `tools/uv`, `tools/python`, `tools/uv-cache`) and prepend those paths only for manager-owned commands. MaiBot core writes `<install>/logs/start-maibot.bat`, opens it in an independent cmd window through PowerShell `Start-Process`, writes the returned cmd PID to `<install>/logs/maibot.pid`, and stops by `taskkill /PID <pid> /T /F`; do not reintroduce `Tee-Object` or direct child stdout inheritance because they broke PID tracking and UTF-8/colorama output. Windows NapCat must use GitHub API to fetch the latest `NapCat.Shell.zip` from `NapNeko/NapCatQQ`, not Docker. Windows LLBot must use GitHub API to fetch the latest `LLBot-Desktop-win-x64.zip` from `LLOneBot/LuckyLilliaBot`, not the CLI zip. SnowLuma is Linux-only: keep Windows commands as clear capability errors and do not add Docker deployment for it. Apply the selected GitHub proxy to both API URLs and release asset URLs.

**macOS implementation.** macOS commands should use native shell commands through `/bin/zsh -lc` and Homebrew for dependency bootstrap. If Homebrew is missing, call the official Homebrew install script; if Git / uv / Python are missing, install them with `brew install` rather than portable Windows tooling or Linux package managers. Keep Homebrew path prefixes (`/opt/homebrew`, `/usr/local`) in manager-owned commands. macOS currently installs and manages MaiBot core only; NapCat, LLBot, and SnowLuma protocol entries should return clear, user-facing platform capability messages until native protocol management is available. macOS core start does not use `screen`: default start creates a background child process in its own process group, redirects stdout/stderr to `<install>/logs/maibot.log`, records `<install>/logs/maibot.pid` for status/stop, and returns immediately so MaiBot keeps running after the manager exits. TUI start must offer a timed launch-mode choice for first-run/EULA interaction; if the user does not choose within the timeout, default to background start. In TUI, the interactive option opens a Terminal.app launcher that writes the same pid/log files; in CLI, `core start --exec` attaches the current terminal for EULA. `core exec` follows the log file rather than attaching to a terminal multiplexer. Do not add Docker, LinuxQQ, BAT/cmd, PowerShell, winget, apt/dnf/yum/pacman/zypper/apk, or `screen` process management logic to macOS modules.

**CLI contract.** CLI commands should reuse the same `App` action methods used by TUI menus rather than duplicating shell strings. `maibot install` / `maibot update` build an `InstallPlan` from existing config plus command-line overrides and then call `run_install`. CLI may ask for confirmation at risk points, but every install/update prompt that blocks scripting must have an explicit CLI strategy flag that bypasses the prompt: `--github-fallback`, `--git-dirty`, `--napcat-conflict`, `--llbot-update`, and `--snowluma-swap`. `maibot access init` prompts by default and `--yes` bypasses it. `maibot core|napcat|llbot|snowluma ...` should remain script-friendly: status/log commands print and exit, while interactive commands (`core exec`, `llbot exec`, `napcat exec`, `snowluma exec`) intentionally inherit stdio.

**Plugin directory naming.** Plugin install/update must not assume the final directory name equals the repository name. After clone/update, resolve the plugin's `_manifest.json` and use its `id` as the canonical folder name under `MaiBot/plugins`. `src/plugins.rs` owns this logic for generic plugin installs, and the built-in NapCat adapter install in `installer.rs` must follow the same rule. Keep compatibility with the historical `MaiBot/plugins/MaiBot-Napcat-Adapter` path by migrating it to `maibot-team.napcat-adapter`; if the destination already exists, preserve the old directory as a backup instead of overwriting it.

**Plugin dependency and update behavior.** Generic plugin install/update does not manage plugin Python dependencies anymore. Do not add TUI actions that install `requirements.txt` for plugins. Plugin management should expose install/sync, update, and uninstall. Dashboard plugin cards may detect update status after the plugin page is loaded by comparing the local Git HEAD with the upstream HEAD; keep that work in the plugin-card cache and do not run network probes while merely moving the content cursor. Selecting a plugin and pressing Enter must open a centered confirmation popup; only the popup's update action should run the update.

**Install defaults.** TUI and CLI share `installer.rs::build_default_install_plan` and `build_recommended_defaults`. The recommended default install path is the current user's HOME joined with `maimai` (displayed as `~/maimai` in docs, but built via `dirs::home_dir()`), and the default Python environment is `uv`. Linux/Windows default protocol is NapCatQQ; macOS default protocol is `none` until NapCat / LLBot are adapted. Keep README/help/AGENTS in sync when changing defaults.

**Install risk strategy fields.** `InstallPlan` carries prompt-bypass strategy fields that are not persisted to `~/.maibot_config`: `github_fallback`, `git_dirty_mode`, `napcat_conflict_mode`, `llbot_update_mode`, and `snowluma_swap_mode`. Defaults are interactive for TUI and CLI unless a CLI flag overrides them. MaiBot main-repo updates are special-cased so a single local `uv.lock` change is automatically discarded before fetch; all other dirty Git states use the selected strategy or prompt.

**Help text.** `src/cli/help.rs` must use `model::APP_VERSION` instead of hardcoding versions. `APP_VERSION` comes from `app.toml` through `build.rs`.

**Build-time branding.** TUI header text is build-time branding from `app.toml` through `build.rs`: `header_title`, `header_subtitle`, `header_credit`, and `header_docs`. Keep `build_label` only as the legacy fallback for an empty `header_subtitle`; do not hardcode these header lines in `ui.rs`.

**Config.** `~/.maibot_config` is a shell-style `KEY="value"` file (NOT TOML), read/written by `runtime.rs::{load_config,save_config}`. `require_config()` is the gate every management menu uses to refuse to run before installation.

**MaiBot WebUI access schema.** MaiBot `bot_config.toml` uses `[webui] host` as a list, for example `host = ["0.0.0.0", "::"]`. Access initialization must write that array form to bind all IPv4 and IPv6 addresses; do not write the old scalar string `host = "0.0.0.0"`.

**MaiBot data clearing.** Settings / Access may expose a "清空数据文件" action, but it must only delete direct children of `<install>/MaiBot/data` while preserving `webui.json`. TUI entry points must show a centered confirmation popup before deletion; CLI must prompt by default and only skip confirmation with `--yes` / `-y` via `maibot access clear-data --yes`.

**Install planner.** `installer.rs::install_planner` is a hand-rolled TUI list (crossterm raw mode) — not a `dialoguer::Select`. Cursor position is tracked as `Option<PlannerEntry>` (a logical target), not as a row index, because expand/collapse reflows the list. The planner supports Up/Down navigation, Left/Right collapse/expand, and Enter to confirm the current field/choice/action. Space may move or expand the cursor but must not apply a choice. Adding a new `PlanField` means touching: the field enum (`model.rs`), `build_planner_entries`, `planner_choices`, `planner_field_label`, `planner_field_value`, `planner_choice_active`, and `apply_planner_choice`.

**Deploy option confirmation.** In the modern dashboard Deploy tab, Up/Down only moves the highlighted option cursor. It must not mutate `InstallPlan`, save config, or cause the highlight to snap back to the already-active value. Enter is the only key that confirms the highlighted option. Render selected/highlighted state separately from the active/checked value: `DashboardChoice::selected` is the cursor, `DashboardChoice::active` is the committed value. This is especially important for the GitHub mirror field, where the visible option order must match `apply_planner_choice`: auto, official direct, configured mirrors, then custom input.

**Terminal modes.** The planner runs under `TerminalUiGuard` (raw mode + hidden cursor, restored on Drop). `dialoguer` prompts conflict with raw mode — when you need an `Input`/`Confirm`/`Select` from inside the planner, wrap it in `App::with_prompt_mode(|| ...)` which temporarily disables raw mode. `terminal.rs` also installs a `ctrlc` handler so abnormal exits still restore the terminal.

**Linux screen-based background jobs.** Linux long-running processes are wrapped in `screen -dmS <name>` sessions. Hardcoded Linux session names: `maibot`, `llbot`, `mai-lpmm-info`, `mai-lpmm-import`. Linux status detection is `utils::screen_exists`; dashboard pages should use `utils::screen_sessions_exist` when checking more than one session so one `screen -list` result can feed multiple cards. NapCat and SnowLuma status use direct `docker ps` probes with a short Rust-side timeout. Main menu's "running" indicators read these platform-specific backends through the dashboard runtime cache, not on every content-row move. CLI logs for Linux screen-backed services should use `screen -X hardcopy` (snapshot or follow loop) rather than `screen -r`, so log viewing does not attach to or disturb the running session. Linux `exec` commands may attach, but must keep the warning prompt.

**LLBot updates.** `install_llbot` reads the latest LuckyLilliaBot release, stores the installed tag in `<install>/LLBot/.maibot-llbot-release`, and preserves absolute-path `LLBot/bin/llbot/data` plus `LLBot/bin/llbot/default_config.json` across updates. If an installed LLBot is not current, TUI/CLI prompt by default; `--llbot-update update` updates without prompting and `--llbot-update skip` keeps the existing install.

**SnowLuma (Linux only).** `install_snowluma` creates `<install>/SnowLuma/docker-compose.yml` from the managed template only when absent, creates `.env` with a cryptographically random 16-character `VNC_PASSWD` containing uppercase, lowercase, digits, and `%@+-`, and bind-mounts `<install>/MaiBot` at the same absolute path in the container as read-only. Preserve user-edited Compose and `.env` files on later installs. Lifecycle methods must use that directory's `docker compose` project: start, stop, restart, logs, rebuild (`down + pull + up -d`), remove container, and recreate data. Data recreation removes only `snowluma-data`, `snowluma-qq-config`, and `snowluma-qq-data`, then starts the container so upstream emits a new one-time WebUI password. Access summary shows WebUI `http://<ip>:5099/`, VNC `http://<ip>:6081/`, VNC password from `.env`, and only the initial password parsed from `docker logs snowluma`; never imply a permanent WebUI password can be recovered. If total memory is at most 4 GiB and Swap is absent, prompt to create persistent 2 GiB `/swapfile`; skip this check when Swap is already active and expose `--snowluma-swap enable|skip` for scripts.

**GitHub speedtest.** `installer.rs::run_github_speedtest` fans out one thread per mirror, each running `curl -sL -o /dev/null -w '%{time_total}'`. If every mirror fails, interactive mode offers retry/direct/cancel; CLI can bypass that prompt with `--github-fallback direct` or `--github-fallback cancel`.

**UI alignment.** CJK characters occupy 2 terminal columns, ASCII 1. Don't use `{key:>10}` style formatting — it counts chars, not columns, and produces ragged tables. Use `utils::{display_width, pad_left}` and the helpers in `ui.rs` (`print_kv`, `print_status_cards`, `select_action`, `select_action_timeout`, `print_section`, `print_line`). New top-level TUI menus should use `ActionItem` with short labels plus concrete descriptions instead of plain string arrays. Service overview pages should use `StatusCard` so Linux, Windows, and macOS keep the same dashboard language while preserving platform-specific details. Interactive terminal attach flows must show an obvious shortcut hint near the bottom of the terminal view; Linux screen attach also sets a persistent hardstatus line with `Ctrl+A` then `D`.

**Shell-string safety.** All paths interpolated into command strings go through `utils::shell_escape` / `shell_escape_raw` (single-quote escape). When extending shell commands, keep this contract — direct `format!("'{}'", path.display())` will break on apostrophes.

**Side-effect surface area.** A few operations write outside the install dir and deserve extra care when modified: Docker daemon `/etc/docker/daemon.json` (`configure_docker_daemon`), LinuxQQ apt/pacman install (`install_linuxqq_for_llbot`), and writing `pip.conf` — which intentionally lives at `<venv>/pip.conf` (NOT `~/.pip/pip.conf`) to avoid polluting the user's global pip config.

## Modern TUI roadmap

The desktop experience has moved from the legacy vertical menu to a modern ratatui dashboard while keeping the existing CLI contract, platform shell-command behavior, and install/service logic intact. Treat this section as the maintenance contract for future TUI work.

**Target experience.**

- Use the current holy-grail layout: a height-3 build-time header, a left sidebar, a main content area, and a single-line footer.
- Keep the sidebar as the only global navigation surface. `Tab` moves focus between sidebar and content; `Ctrl+1` returns to the sidebar quickly.
- Keep all global and contextual key hints in the footer. Do not scatter "Enter/Tab" hints across panels.
- Use the Nord minimalist cool palette globally: bg `#2E3440`, text `#D8DEE9`, focus/accent `#88C0D0`, selected-row accent `#81A1C1`, success `#A3BE8C`, warning `#EBCB8B`, error `#BF616A`, and muted borders `#4C566A`.
- Introduce Nerd Font friendly icons for tabs, status chips, actions, and cards. Keep graceful text fallback behavior when icons render poorly.
- Shorten copy throughout the TUI so titles, subtitles, labels, and body text form a clear visual hierarchy.

**Information architecture.**

- Sidebar entries: `概览`, `部署与更新`, `核心服务管理`, `协议端服务`, `插件中心`, `设置`, `关于`.
- `概览`: service and workspace rows in a full-width table with a compact detail panel below.
- `部署与更新`: horizontal stepper at the top, current field options in the main panel, read-only config summary on the side, and description below. `Left/Right` changes the selected field; `Up/Down` moves the highlighted option inside that field without changing the plan; `Enter` confirms the highlighted option, except the path field where it opens the path editor. `F5` runs install/update, and `Ctrl+R` resets the in-memory plan to recommended defaults.
- `核心服务管理`: full-width table with `名称 / 状态 / 版本 / 快捷操作` columns plus centered modal actions.
- `协议端服务`: keep platform-specific behavior, but present NapCat / LLBot / SnowLuma panels with the same shared layout language; SnowLuma is Linux Docker only, and unavailable Windows/macOS entries must stay explicit and written in natural product language.
- `访问配置`: compact info cards plus direct actions for init/copy/view style tasks where supported.
- `插件中心`: full-width plugin table with the same shortcut-action column language plus centered modal actions.
- `关于`: build metadata, documentation links, runtime environment, and troubleshooting hints.

**Current implementation.**

- `src/ui.rs` owns the shared modern TUI renderer: header/sidebar/content/footer layout, rounded blocks, responsive deployment form, table views, centered modals, contextual footer, action menu drawing, and raw-mode prompt switching.
- `src/model.rs` owns dashboard state and view models: sidebar tabs, focus zones, selected rows, filters, detail choices, contextual status messages, deployment events, and popup state.
- Each platform `app.rs` drives the tab shell with platform-specific cards/details/actions, but must still call the same underlying install/service/access/plugin methods used by CLI and legacy submenus.
- Wide terminals render a two-column workspace; narrow terminals must degrade to stacked panels without overflowing. Readability bugs in either layout are regressions.
- The dashboard can show "running but config/path not recorded" as a warning state. Do not display contradictory copy such as "running" plus "not installed"; prefer "配置待同步" / "configuration pending sync".
- The `About` tab is read-only: Enter should not exit the manager. Top-level exit is `Ctrl+C`.
- Pure information pages opened from the ratatui dashboard, such as access summaries or platform capability notes, must stay inside centered `DashboardPopup` modals. They should open through the inline information-popup callback while the ratatui alternate screen remains active, not by returning `DashboardEvent::Activate` to the platform loop. CLI commands such as `maibot access show` should continue to print plain text and exit without any popup behavior.
- Content-row movement and ordinary popup opening must use cached redraw paths and must not trigger platform probes, filesystem scans, or shell commands. Pure information popup construction may read the specific files or bounded network data needed for the requested report, but it must redraw in-place and avoid dropping back to legacy "press Enter to return" pages. Access summaries should query the public IP lazily only when at least one visible endpoint needs an external address. Rebuilds should happen when changing sidebar sections or after actions that can alter runtime state. Plugin update checks must never block tab switching or first paint of the plugin page; render local plugin metadata immediately, then refresh update badges from a bounded background check.
- Platform dashboard caches should keep short-lived runtime snapshots and plugin card snapshots. Linux status probes must stay bounded by Rust-side timeouts and should batch `screen` session checks when possible.
- `scripts/verify_tui_capture.py` is the PTY smoke-test helper for final screen snapshots. Keep it able to allocate a controlling terminal, set rows/cols, drive key timelines, and report `overflow`.

**Non-negotiable constraints.**

- Do not duplicate business logic: new TUI pages must call the same `App` action methods already used by CLI / current menus.
- Keep platform isolation rules intact; shared code must not pull in non-current-platform modules.
- Preserve `TerminalUiGuard`, `with_prompt_mode`, and prompt safety when mixing raw-mode panels with `dialoguer`.
- Keep CJK-aware alignment via `display_width` and related helpers; new layout code must not assume ASCII-only widths.
- Keep unavailable macOS NapCat / LLBot / SnowLuma flows explicit instead of faking parity, and avoid raw implementation-status labels in user-facing UI copy.
- When Nerd Font icons are added, surrounding labels must remain understandable even if the terminal lacks glyph support.
- Do not render internal UI metadata such as current focus, layout mode, or item counters. If a value exists only to explain the implementation, keep it out of the interface.

**Validation expectations.**

- At minimum, run `cargo fmt`, host `cargo check`, and `cargo clippy --all-targets -- -D warnings` after meaningful TUI refactors.
- On Windows hosts, build and directly run a Windows release EXE before claiming TUI work is done:
  `cargo build --release --target x86_64-pc-windows-msvc --target-dir target\windows-verify`,
  then run `target\windows-verify\x86_64-pc-windows-msvc\release\maibot-manager-tui.exe --help`
  and `target\windows-verify\x86_64-pc-windows-msvc\release\maibot-manager-tui.exe tui`.
- Use WSL for Linux reality checks from Windows: `cargo build`, `cargo check --target x86_64-unknown-linux-musl`, and `scripts/verify_tui_capture.py` in `wide`, `narrow`, `tabs`, `deploy`, and `access` modes.
- Inspect PTY capture output for `overflow: false`, visible rounded blocks, expected sidebar/content/footer structure, deployment footer shortcuts, no internal metadata text, and no contradictory running/installed state text.
- Manually sanity-check raw-mode navigation paths for sidebar/content focus, deployment left-right/up-down behavior, centered modals, install planner, service actions, and `Ctrl+C` terminal restoration.
- Treat readability regressions in narrow terminals as bugs even though the optimized target is a modern wide terminal.
