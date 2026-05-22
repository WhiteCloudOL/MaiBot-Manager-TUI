#!/usr/bin/env bash
set -euo pipefail

source "${HOME}/.cargo/env"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

mkdir -p output

rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl

cargo build --release --locked --target x86_64-unknown-linux-musl
cargo build --release --locked --target aarch64-unknown-linux-musl

cp target/x86_64-unknown-linux-musl/release/maibot-manager-tui output/maibot-manager-x86_64
cp target/aarch64-unknown-linux-musl/release/maibot-manager-tui output/maibot-manager-arm64

echo "Build complete:"
ls -lh output
