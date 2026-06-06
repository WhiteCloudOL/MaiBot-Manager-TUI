# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Build / run

The binary supports Linux and Windows 10/11. `main.rs` selects platform implementations at compile time and bails on other OSes. Use WSL for Linux builds on Windows hosts; use the GitHub Actions Windows job or a local Windows Rust toolchain for Windows release checks.

```bash
# Quick check
cargo check --target x86_64-unknown-linux-musl

# Linux release build, both architectures (what build-release.sh does)
cargo build --release --target x86_64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl

# Windows release build
cargo build --release --target x86_64-pc-windows-msvc

# Full local pipelines
./build-release.sh                # inside Linux / WSL
.\build-release.ps1               # Windows host, builds Windows exe then Linux binaries via WSL
```

Linux release builds use musl static targets so the binaries do not depend on the target server's GLIBC version. Windows release builds target Windows 10/11 x86_64. There are no tests.

GitHub Actions publishes every automatic build as a prerelease only. Keep release tags in the form `v<version>-nextdev-<short-sha>` (for example `v0.3.0-nextdev-abcdef1`) and do not create stable `v<version>` releases from the automatic workflow. Release notes should compare the current build with the latest stable GitHub Release and group conventional commits under professional sections such as `Feature:` and `Fix:`.

Windows support is compiled as a separate target. Linux-only implementations live in `src/linux/`, Windows-only implementations live in `src/win/`, and `src/main.rs` selects them with `#[cfg(target_os = "...")]` plus `#[path = "..."]`. Do not import non-current-platform modules from shared code, because release binaries should not include dead platform code.

## Architecture

This is a single-binary TUI + CLI that orchestrates `bash` to install/manage MaiBot on a Linux server. Almost every "action" — clone, venv, docker, screen, systemctl — is a shell string executed via `App::run_shell`, not native Rust. Treat shell command strings as the primary IR; the Rust code is a config builder + menu/CLI driver around them.

**Entry behavior.** `main.rs` parses args before the Linux guard only for global help (`help`, `-h`, `--help`) so help can be printed anywhere. No args or `tui` enters the TUI. Any other first arg dispatches to `App::run_cli`. Keep this behavior when adding commands.

**Module shape.** Every domain module (`installer`, `services`, `access`, `plugins`, `runtime`, `ui`) is `impl App for ...` — they share state via `App` (theme + config path) rather than holding their own types. `app.rs` is the TUI shell that prints the main menu and dispatches; `src/cli/` is the CLI shell. Keep CLI parsing split by domain (`install`, `services`, `access`, `plugins`, `help`) instead of growing one giant command file.

**Windows implementation.** Windows commands should prefer BAT/cmd syntax executed through `App::run_shell`, which writes a temporary `.bat` and invokes `cmd.exe /C`. Use PowerShell only when cmd has no good primitive, currently UAC elevation via `Start-Process -Verb RunAs` and log tailing. Windows NapCat must use GitHub API to fetch the latest `NapCat.Shell.zip` from `NapNeko/NapCatQQ`, not Docker. Windows LLBot must use GitHub API to fetch the latest `LLBot-Desktop-win-x64.zip` from `LLOneBot/LuckyLilliaBot`, not the CLI zip. Apply the selected GitHub proxy to both API URLs and release asset URLs.

**CLI contract.** CLI commands should reuse the same `App` action methods used by TUI menus rather than duplicating shell strings. `maibot install` / `maibot update` build an `InstallPlan` from existing config plus command-line overrides and then call `run_install`. CLI may ask for confirmation at risk points, but every install/update prompt that blocks scripting must have an explicit CLI strategy flag that bypasses the prompt: `--github-fallback`, `--git-dirty`, `--napcat-conflict`, `--llbot-update`. `maibot access init` prompts by default and `--yes` bypasses it. `maibot core|napcat|llbot ...` should remain script-friendly: status/log commands print and exit, while interactive commands (`core exec`, `llbot exec`, `napcat exec`) intentionally inherit stdio.

**Plugin directory naming.** Plugin install/update must not assume the final directory name equals the repository name. After clone/update, resolve the plugin's `_manifest.json` and use its `id` as the canonical folder name under `MaiBot/plugins`. `src/plugins.rs` owns this logic for generic plugin installs, and the built-in NapCat adapter install in `installer.rs` must follow the same rule. Keep compatibility with the historical `MaiBot/plugins/MaiBot-Napcat-Adapter` path by migrating it to `maibot-team.napcat-adapter`; if the destination already exists, preserve the old directory as a backup instead of overwriting it.

**Install defaults.** TUI and CLI share `installer.rs::build_default_install_plan` and `build_recommended_defaults`. The recommended default install path is the current user's HOME joined with `maimai` (displayed as `~/maimai` in docs, but built via `dirs::home_dir()`), the default Python environment is `uv`, and the default protocol is NapCatQQ. Keep README/help/AGENTS in sync when changing defaults.

**Install risk strategy fields.** `InstallPlan` carries prompt-bypass strategy fields that are not persisted to `~/.maibot_config`: `github_fallback`, `git_dirty_mode`, `napcat_conflict_mode`, and `llbot_update_mode`. Defaults are interactive for TUI and CLI unless a CLI flag overrides them. MaiBot main-repo updates are special-cased so a single local `uv.lock` change is automatically discarded before fetch; all other dirty Git states use the selected strategy or prompt.

**Help text.** `src/cli/help.rs` must use `model::APP_VERSION` instead of hardcoding versions. `APP_VERSION` comes from `app.toml` through `build.rs`.

**Config.** `~/.maibot_config` is a shell-style `KEY="value"` file (NOT TOML), read/written by `runtime.rs::{load_config,save_config}`. `require_config()` is the gate every management menu uses to refuse to run before installation.

**Install planner.** `installer.rs::install_planner` is a hand-rolled TUI list (crossterm raw mode) — not a `dialoguer::Select`. Cursor position is tracked as `Option<PlannerEntry>` (a logical target), not as a row index, because expand/collapse reflows the list. Adding a new `PlanField` means touching: the field enum (`model.rs`), `build_planner_entries`, `planner_choices`, `planner_field_label`, `planner_field_value`, `planner_choice_active`, and `apply_planner_choice`.

**Terminal modes.** The planner runs under `TerminalUiGuard` (raw mode + hidden cursor, restored on Drop). `dialoguer` prompts conflict with raw mode — when you need an `Input`/`Confirm`/`Select` from inside the planner, wrap it in `App::with_prompt_mode(|| ...)` which temporarily disables raw mode. `terminal.rs` also installs a `ctrlc` handler so abnormal exits still restore the terminal.

**Screen-based background jobs.** Long-running processes are wrapped in `screen -dmS <name>` sessions. Hardcoded session names: `maibot`, `llbot`, `mai-lpmm-info`, `mai-lpmm-import`. Status detection is `utils::screen_exists` (or `docker ps` filter for NapCat). Main menu's "running" indicators read these. CLI logs for screen-backed services should use `screen -X hardcopy` (snapshot or follow loop) rather than `screen -r`, so log viewing does not attach to or disturb the running session. `exec` commands may attach, but must keep the warning prompt.

**LLBot updates.** `install_llbot` reads the latest LuckyLilliaBot release, stores the installed tag in `<install>/LLBot/.maibot-llbot-release`, and preserves absolute-path `LLBot/bin/llbot/data` plus `LLBot/bin/llbot/default_config.json` across updates. If an installed LLBot is not current, TUI/CLI prompt by default; `--llbot-update update` updates without prompting and `--llbot-update skip` keeps the existing install.

**GitHub speedtest.** `installer.rs::run_github_speedtest` fans out one thread per mirror, each running `curl -sL -o /dev/null -w '%{time_total}'`. If every mirror fails, interactive mode offers retry/direct/cancel; CLI can bypass that prompt with `--github-fallback direct` or `--github-fallback cancel`.

**UI alignment.** CJK characters occupy 2 terminal columns, ASCII 1. Don't use `{key:>10}` style formatting — it counts chars, not columns, and produces ragged tables. Use `utils::{display_width, pad_left}` and the helpers in `ui.rs` (`print_kv`, `print_status_dot`, `print_section`, `print_line`).

**Shell-string safety.** All paths interpolated into command strings go through `utils::shell_escape` / `shell_escape_raw` (single-quote escape). When extending shell commands, keep this contract — direct `format!("'{}'", path.display())` will break on apostrophes.

**Side-effect surface area.** A few operations write outside the install dir and deserve extra care when modified: Docker daemon `/etc/docker/daemon.json` (`configure_docker_daemon`), LinuxQQ apt/pacman install (`install_linuxqq_for_llbot`), and writing `pip.conf` — which intentionally lives at `<venv>/pip.conf` (NOT `~/.pip/pip.conf`) to avoid polluting the user's global pip config.
