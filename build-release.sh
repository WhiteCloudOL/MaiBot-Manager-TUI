#!/usr/bin/env bash
set -euo pipefail

if [ -f "${HOME}/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

command -v cargo >/dev/null 2>&1 || {
    echo "cargo is required. Install Rust first: https://rustup.rs/" >&2
    exit 1
}
command -v rustup >/dev/null 2>&1 || {
    echo "rustup is required to install release targets." >&2
    exit 1
}
command -v rustc >/dev/null 2>&1 || {
    echo "rustc is required." >&2
    exit 1
}

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
        TARGETS=(
            x86_64-apple-darwin
            aarch64-apple-darwin
        )
        OUTPUT_NAMES=(
            maibot-manager-macos-x86_64
            maibot-manager-macos-arm64
        )
        ;;
    *)
        echo "Unsupported build host: ${HOST_OS}. Use Linux/WSL for Linux artifacts or macOS for macOS artifacts." >&2
        exit 1
        ;;
esac

mkdir -p output

echo "Build host: ${HOST_OS} (${HOST_TRIPLE})"
echo "Rust toolchain: ${RUSTC_BIN}"
rustup target add "${TARGETS[@]}"

echo "Synchronizing Cargo.lock without network..."
"${CARGO_BIN}" generate-lockfile --offline

for index in "${!TARGETS[@]}"; do
    target="${TARGETS[$index]}"
    output_name="${OUTPUT_NAMES[$index]}"

    "${CARGO_BIN}" build --release --locked --target "${target}"
    cp "target/${target}/release/maibot-manager-tui" "output/${output_name}"
    chmod +x "output/${output_name}"
done

echo "Build complete:"
ls -lh output
