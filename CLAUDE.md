# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build / run

The binary targets Linux, Windows, and macOS. Cross-compiling from Windows uses WSL.

```bash
# Quick check (use the explicit target so .cargo/config.toml's cross linker is picked up)
cargo check --target x86_64-unknown-linux-gnu

# Release build, both architectures (what build-release.sh does)
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu

# Full pipeline producing output/maibot-manager-{x86_64,arm64}
./build-release.sh                # inside Linux / WSL
.\build-release.ps1               # Windows host, dispatches to WSL Ubuntu-24.04
```

ARM64 cross requires `gcc-aarch64-linux-gnu`. The linker is configured in `.cargo/config.toml`. There are no tests.

## Compile-time constants

App metadata (version, mirrors, header strings, etc.) comes from `app.toml`, not `Cargo.toml`. `build.rs` reads `app.toml` at build time and emits `cargo:rustc-env=APP_*` lines. `model.rs` exposes them as `pub const` via `env!()`. Changing `app.toml` triggers a rebuild; changing only `Cargo.toml` version has no effect on the running binary's displayed version.

## Platform module layout

`main.rs` uses `#[cfg(target_os)] #[path = "linux/…"] mod …` to select the correct platform implementation at compile time. The modules `app`, `installer`, `services`, `access`, `plugins`, `runtime`, and `utils` each have a separate implementation under `src/linux/`, `src/win/`, and `src/macos/`. The shared modules `model`, `terminal`, `theme`, `ui`, and `cli` are always compiled. When adding platform-specific behavior, add it to all three platform directories and keep shared types in `model.rs`.

## Architecture

This is a single-binary TUI that orchestrates shell commands to install/manage MaiBot. Almost every "action" — clone, venv, docker, screen, systemctl — is a shell string executed via `App::run_shell`, not native Rust. Treat shell command strings as the primary IR; the Rust code is a config builder + menu driver around them.

**Two UI modes.** Running without arguments enters the **ratatui dashboard** (`app.rs::run`). Running with arguments enters **CLI mode** (`app.rs::run_cli` → `cli/` subcommands). CLI mode calls the same underlying methods; it just skips the event loop and runs a single action.

**Module shape.** Every domain module (`installer`, `services`, `access`, `plugins`, `runtime`) is `impl App for …` — they share state via `App` (theme + config path) rather than holding their own types. `app.rs` is the entry shell that drives the dashboard event loop and dispatches card activations; `main.rs` only wires terminal cleanup and the platform guard.

**Config.** `~/.maibot_config` is a shell-style `KEY="value"` file (NOT TOML), read/written by `runtime.rs::{load_config,save_config}`. Keys: `USER_INSTALL_PATH`, `MAI_PATH`, `MAI_PYTHON_ENV`, `MAI_LLBOT_PATH`, `MAI_INSTALL_MODE`, `MAI_VENV_MODE`, `MAIBOT_BRANCH`, `PIP_DISPLAY`, `PIP_INDEX`, `PIP_HOST`, `BOT_PROTOCOLS`. `require_config()` is the gate every management menu uses to refuse to run before installation. All install preferences (except GitHub mirror and Docker mirror) are persisted to config and restored on next launch.

**Dashboard state model.** `AppState` (`model.rs`) holds the live state: active tab, sidebar/content focus, selection per tab, current deploy plan, and optional popup. `DashboardView` is the read-only snapshot passed to the renderer. `app.rs::build_dashboard_view` derives a `DashboardView` from `AppState` on every frame. Status reads are cached in `DashboardRuntimeCache` with a 10-second TTL to avoid shelling out on every render tick.

**Install planner (dashboard Deploy tab).** The Deploy tab is driven by `deploy_cards_from_plan` which produces `DashboardCard` entries for each `PlanField`. Selecting a card and pressing Enter opens a popup or input box to change that field's value on the in-memory `InstallPlan`. Pressing "执行安装" calls `run_install`. Adding a new `PlanField` requires touching: the field enum (`model.rs`), `deploy_cards_from_plan`, `planner_choices`, `planner_field_label`, `planner_field_value`, `planner_choice_active`, `apply_planner_choice`, `deploy_card_field`, and `deploy_choice_detail`. Also update `model.rs::{AppConfig, InstallPlan}` if persisted, `runtime.rs::{load_config, save_config}`, and `installer.rs::{build_default_install_plan, plan_to_config}`.

**Terminal modes.** The ratatui dashboard runs under `TerminalUiGuard` (alternate screen + raw mode + hidden cursor, restored on Drop). `dialoguer` prompts conflict with raw mode — when you need an `Input`/`Confirm`/`Select` inside an action handler, wrap it in `App::with_prompt_mode(|| …)` which temporarily leaves the alternate screen. `terminal.rs` also installs a `ctrlc` handler so abnormal exits still restore the terminal.

**Screen-based background jobs (Linux).** Long-running processes are wrapped in `screen -dmS <name>` sessions. Hardcoded session names: `maibot`, `llbot`, `mai-lpmm-info`, `mai-lpmm-import`. Status detection is `utils::screen_exists` (or `docker ps` filter for NapCat). The dashboard polls these on a 10-second TTL via `dashboard_snapshot`.

**GitHub speedtest.** `installer.rs::run_github_speedtest` fans out one thread per mirror, each running `curl -sL -o /dev/null -w '%{time_total}'`. If every mirror fails, the function recurses with a retry/direct/cancel prompt — it never returns a silent fallback.

**UI alignment.** CJK characters occupy 2 terminal columns, ASCII 1. Don't use `{key:>10}` style formatting — it counts chars, not columns. Use `utils::{display_width, pad_left}` and the helpers in `ui.rs` (`print_kv`, `print_status_dot`, `print_section`, `print_line`). The ratatui dashboard uses `unicode-width` directly for column math.

**Shell-string safety.** All paths interpolated into command strings go through `utils::shell_escape` / `shell_escape_raw` (single-quote escape). When extending shell commands, keep this contract — direct `format!("'{}'", path.display())` will break on apostrophes.

**Side-effect surface area.** A few operations write outside the install dir: Docker daemon `/etc/docker/daemon.json` (`configure_docker_daemon`), LinuxQQ apt/pacman install (`install_linuxqq_for_llbot`), and `pip.conf` — which intentionally lives at `<venv>/pip.conf` (NOT `~/.pip/pip.conf`) to avoid polluting the user's global pip config.
