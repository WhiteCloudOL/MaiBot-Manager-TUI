$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$wslProjectRoot = "/mnt/" + $projectRoot.Substring(0,1).ToLower() + $projectRoot.Substring(2).Replace("\", "/")

$bashScript = @"
set -euo pipefail
source ~/.cargo/env
cd '$wslProjectRoot'
mkdir -p output
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl
cargo build --release --locked --target aarch64-unknown-linux-musl
cp target/x86_64-unknown-linux-musl/release/maibot-manager-tui output/maibot-manager-x86_64
cp target/aarch64-unknown-linux-musl/release/maibot-manager-tui output/maibot-manager-arm64
echo 'Build complete:'
ls -lh output
"@

wsl -d Ubuntu-24.04 -- bash -lc $bashScript
