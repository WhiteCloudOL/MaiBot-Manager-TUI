#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

if [ -t 1 ]; then
    CYAN=$'\033[0;36m'
    GREEN=$'\033[0;32m'
    YELLOW=$'\033[0;33m'
    RED=$'\033[0;31m'
    DIM=$'\033[2m'
    RESET=$'\033[0m'
else
    CYAN='' GREEN='' YELLOW='' RED='' DIM='' RESET=''
fi

step() { printf '%s==>%s %s\n' "$CYAN" "$RESET" "$*" >&2; }
ok() { printf '%s OK %s %s\n' "$GREEN" "$RESET" "$*" >&2; }
warn() { printf '%s !  %s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
fail() { printf '%sERR%s %s\n' "$RED" "$RESET" "$*" >&2; }
dim() { printf '%s%s%s\n' "$DIM" "$*" "$RESET" >&2; }

die() {
    fail "$*"
    exit 1
}

trap 'die "Build failed near line ${LINENO}: ${BASH_COMMAND}"' ERR

require_cmd() {
    local missing=()
    local cmd
    for cmd in "$@"; do
        command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
    done
    if [ "${#missing[@]}" -gt 0 ]; then
        die "Missing required command(s): ${missing[*]}"
    fi
}

if [ -f "${HOME}/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

require_cmd cargo rustup rustc uname sed cp chmod mkdir ls

CARGO_BIN="$(command -v cargo)"
RUSTC_BIN="$(command -v rustc)"
if RUSTUP_CARGO="$(rustup which cargo 2>/dev/null)"; then
    CARGO_BIN="${RUSTUP_CARGO}"
fi
if RUSTUP_RUSTC="$(rustup which rustc 2>/dev/null)"; then
    RUSTC_BIN="${RUSTUP_RUSTC}"
fi
export RUSTC="${RUSTC_BIN}"

HOST_OS="$(uname -s)"
HOST_TRIPLE="$("${RUSTC_BIN}" -vV | sed -n 's/^host: //p')"

case "${HOST_OS}" in
    Linux)
        RUST_LLD_DIR="$("${RUSTC_BIN}" --print sysroot)/lib/rustlib/${HOST_TRIPLE}/bin"
        if [ -d "${RUST_LLD_DIR}" ]; then
            export PATH="${RUST_LLD_DIR}:${PATH}"
        fi
        TARGETS=(
            x86_64-unknown-linux-musl
            aarch64-unknown-linux-musl
        )
        OUTPUT_NAMES=(
            maibot-manager-linux-x86_64
            maibot-manager-linux-arm64
        )
        ;;
    Darwin)
        TARGETS=(aarch64-apple-darwin)
        OUTPUT_NAMES=(maibot-manager-macos-arm64)
        ;;
    *)
        die "Unsupported build host: ${HOST_OS}. Use Linux/WSL for Linux artifacts or macOS for macOS artifacts."
        ;;
esac

mkdir -p output

step "MaiBot Manager release build"
dim "Project: ${SCRIPT_DIR}"
dim "Host:    ${HOST_OS} (${HOST_TRIPLE})"
dim "Cargo:   ${CARGO_BIN}"
dim "Rustc:   ${RUSTC_BIN}"

step "Ensuring Rust targets"
rustup target add "${TARGETS[@]}"

if [ "${MAIBOT_SKIP_CARGO_FETCH:-0}" = "1" ]; then
    warn "Skipping cargo fetch because MAIBOT_SKIP_CARGO_FETCH=1"
else
    step "Fetching dependencies with Cargo.lock"
    "${CARGO_BIN}" fetch --locked
fi

for index in "${!TARGETS[@]}"; do
    target="${TARGETS[$index]}"
    output_name="${OUTPUT_NAMES[$index]}"
    output_path="output/${output_name}"

    step "Building ${target}"
    "${CARGO_BIN}" build --release --locked --target "${target}"
    cp "target/${target}/release/maibot-manager-tui" "${output_path}"
    chmod +x "${output_path}"

    if [ ! -s "${output_path}" ]; then
        die "Expected artifact was not created: ${output_path}"
    fi
    ok "${output_name}"
done

step "Build complete"
ls -lh output
