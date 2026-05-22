# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Build / run

The compiled binary only runs on Linux — `main.rs` calls `ensure_linux()` and bails on any other OS. Use WSL for development on Windows.

```bash
# Quick check
cargo check --target x86_64-unknown-linux-musl

# Release build, both architectures (what build-release.sh does)
cargo build --release --target x86_64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl

# Full pipeline producing output/maibot-manager-{x86_64,arm64}
./build-release.sh                # inside Linux / WSL
.\build-release.ps1               # Windows host, dispatches to WSL Ubuntu-24.04
```

Release builds use musl static targets so the binaries do not depend on the target server's GLIBC version. There are no tests.

## Architecture

This is a single-binary TUI that orchestrates `bash` to install/manage MaiBot on a Linux server. Almost every "action" — clone, venv, docker, screen, systemctl — is a shell string executed via `App::run_shell`, not native Rust. Treat shell command strings as the primary IR; the Rust code is a config builder + menu driver around them.

**Module shape.** Every domain module (`installer`, `services`, `access`, `lpmm`, `plugins`, `runtime`, `ui`) is `impl App for ...` — they share state via `App` (theme + config path) rather than holding their own types. `app.rs` is the entry shell that prints the main menu and dispatches; `main.rs` only wires terminal cleanup and the OS guard.

**Config.** `~/.maibot_config` is a shell-style `KEY="value"` file (NOT TOML), read/written by `runtime.rs::{load_config,save_config}`. `require_config()` is the gate every management menu uses to refuse to run before installation.

**Install planner.** `installer.rs::install_planner` is a hand-rolled TUI list (crossterm raw mode) — not a `dialoguer::Select`. Cursor position is tracked as `Option<PlannerEntry>` (a logical target), not as a row index, because expand/collapse reflows the list. Adding a new `PlanField` means touching: the field enum (`model.rs`), `build_planner_entries`, `planner_choices`, `planner_field_label`, `planner_field_value`, `planner_choice_active`, and `apply_planner_choice`.

**Terminal modes.** The planner runs under `TerminalUiGuard` (raw mode + hidden cursor, restored on Drop). `dialoguer` prompts conflict with raw mode — when you need an `Input`/`Confirm`/`Select` from inside the planner, wrap it in `App::with_prompt_mode(|| ...)` which temporarily disables raw mode. `terminal.rs` also installs a `ctrlc` handler so abnormal exits still restore the terminal.

**Screen-based background jobs.** Long-running processes are wrapped in `screen -dmS <name>` sessions. Hardcoded session names: `maibot`, `llbot`, `mai-lpmm-info`, `mai-lpmm-import`. Status detection is `utils::screen_exists` (or `docker ps` filter for NapCat). Main menu's "running" indicators read these.

**GitHub speedtest.** `installer.rs::run_github_speedtest` fans out one thread per mirror, each running `curl -sL -o /dev/null -w '%{time_total}'`. If every mirror fails, the function recurses with a retry/direct/cancel prompt — it never returns a silent fallback.

**UI alignment.** CJK characters occupy 2 terminal columns, ASCII 1. Don't use `{key:>10}` style formatting — it counts chars, not columns, and produces ragged tables. Use `utils::{display_width, pad_left}` and the helpers in `ui.rs` (`print_kv`, `print_status_dot`, `print_section`, `print_line`).

**Shell-string safety.** All paths interpolated into command strings go through `utils::shell_escape` / `shell_escape_raw` (single-quote escape). When extending shell commands, keep this contract — direct `format!("'{}'", path.display())` will break on apostrophes.

**Side-effect surface area.** A few operations write outside the install dir and deserve extra care when modified: Docker daemon `/etc/docker/daemon.json` (`configure_docker_daemon`), LinuxQQ apt/pacman install (`install_linuxqq_for_llbot`), and writing `pip.conf` — which intentionally lives at `<venv>/pip.conf` (NOT `~/.pip/pip.conf`) to avoid polluting the user's global pip config.
