#!/usr/bin/env bash
set -euo pipefail

source "${HOME}/.cargo/env"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

mkdir -p output

cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu

cp target/x86_64-unknown-linux-gnu/release/maibot-manager-tui output/maibot-manager-x86_64
cp target/aarch64-unknown-linux-gnu/release/maibot-manager-tui output/maibot-manager-arm64

echo "Build complete:"
ls -lh output
