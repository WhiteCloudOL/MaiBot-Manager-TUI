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

HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
RUST_LLD_DIR="$(rustc --print sysroot)/lib/rustlib/${HOST_TRIPLE}/bin"
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

mkdir -p output

rustup target add "${TARGETS[@]}"

echo "Synchronizing Cargo.lock without network..."
cargo generate-lockfile --offline

for index in "${!TARGETS[@]}"; do
    target="${TARGETS[$index]}"
    output_name="${OUTPUT_NAMES[$index]}"

    cargo build --release --locked --target "${target}"
    cp "target/${target}/release/maibot-manager-tui" "output/${output_name}"
    chmod +x "output/${output_name}"
done

echo "Build complete:"
ls -lh output
