$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$wslProjectRoot = "/mnt/" + $projectRoot.Substring(0,1).ToLower() + $projectRoot.Substring(2).Replace("\", "/")

$bashScript = @"
set -euo pipefail
source ~/.cargo/env
cd '$wslProjectRoot'
mkdir -p output
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cp target/x86_64-unknown-linux-gnu/release/maibot-manager-tui output/maibot-manager-x86_64
cp target/aarch64-unknown-linux-gnu/release/maibot-manager-tui output/maibot-manager-arm64
echo 'Build complete:'
ls -lh output
"@

wsl -d Ubuntu-24.04 -- bash -lc $bashScript
