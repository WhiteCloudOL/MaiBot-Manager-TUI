#!/usr/bin/env bash
# MaiBot Manager TUI 一键安装脚本
# 用法:
#   curl -fsSL https://raw.githubusercontent.com/WhiteCloudOL/MaiBot-Manager-TUI/main/scripts/install.sh | bash
#   或下载后直接执行: bash install.sh
#
# 可选环境变量:
#   MAIBOT_INSTALL_DIR   安装目录，默认 $HOME/.local/bin
#   MAIBOT_FORCE_PROXY   强制使用的镜像（如 https://gh-proxy.org 或 direct）
#   MAIBOT_VERSION       指定版本 tag（如 v0.1.2），默认拉取 latest

set -euo pipefail

REPO="WhiteCloudOL/MaiBot-Manager-TUI"
INSTALL_DIR="${MAIBOT_INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="maibot"

GITHUB_MIRRORS=(
    "https://gh-proxy.org"
    "https://hk.gh-proxy.org"
    "https://cdn.gh-proxy.org"
    "https://ghproxy.net"
    "https://ghfast.top"
    "https://github.moeyy.xyz"
)

if [ -t 1 ]; then
    RED=$'\033[0;31m'
    GREEN=$'\033[0;32m'
    YELLOW=$'\033[0;33m'
    CYAN=$'\033[0;36m'
    BOLD=$'\033[1m'
    DIM=$'\033[2m'
    RESET=$'\033[0m'
else
    RED='' GREEN='' YELLOW='' CYAN='' BOLD='' DIM='' RESET=''
fi

info()  { printf '%s==>%s %s\n' "$CYAN"   "$RESET" "$*" >&2; }
ok()    { printf '%s ✓ %s %s\n' "$GREEN"  "$RESET" "$*" >&2; }
warn()  { printf '%s !  %s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
err()   { printf '%s ✗ %s %s\n' "$RED"    "$RESET" "$*" >&2; }
dim()   { printf '%s%s%s\n' "$DIM" "$*" "$RESET" >&2; }

require_cmd() {
    local missing=()
    for c in "$@"; do
        command -v "$c" >/dev/null 2>&1 || missing+=("$c")
    done
    if [ "${#missing[@]}" -gt 0 ]; then
        err "缺少必要命令: ${missing[*]}"
        err "请先安装上述工具后重试。"
        exit 1
    fi
}

detect_asset() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64)   echo "maibot-manager-x86_64" ;;
        aarch64|arm64)  echo "maibot-manager-arm64"  ;;
        *)
            err "当前架构暂不支持: $arch"
            exit 1
            ;;
    esac
}

# 透传 GITHUB_TOKEN（若存在），减少 API 限流概率
gh_curl() {
    local url="$1"
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        curl -fsSL --max-time 15 \
            -H "Authorization: Bearer ${GITHUB_TOKEN}" \
            -H "Accept: application/vnd.github+json" \
            "$url"
    else
        curl -fsSL --max-time 15 -H "Accept: application/vnd.github+json" "$url"
    fi
}

fetch_release_json() {
    local tag="${MAIBOT_VERSION:-}"
    local api_url
    if [ -n "$tag" ]; then
        api_url="https://api.github.com/repos/${REPO}/releases/tags/${tag}"
    else
        api_url="https://api.github.com/repos/${REPO}/releases/latest"
    fi
    local json
    if json="$(gh_curl "$api_url" 2>/dev/null)" && [ -n "$json" ]; then
        printf '%s' "$json"
        return 0
    fi
    warn "GitHub API 直连失败，尝试镜像..."
    for m in "${GITHUB_MIRRORS[@]}"; do
        if json="$(gh_curl "${m}/${api_url}" 2>/dev/null)" && [ -n "$json" ]; then
            printf '%s' "$json"
            return 0
        fi
    done
    err "无法获取 release 元数据（GitHub API 直连与所有镜像均失败）"
    exit 1
}

# 用最朴素的 sed 在 JSON 里抓取字段，避免依赖 jq
json_field() {
    local json="$1" key="$2"
    printf '%s' "$json" | sed -n 's/.*"'"$key"'"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1
}

# 从 release JSON 中按资产名定位 browser_download_url
find_asset_url() {
    local json="$1" name="$2"
    printf '%s' "$json" \
        | tr '{},' '\n' \
        | grep -F '"browser_download_url"' \
        | grep -F "/${name}\"" \
        | head -n1 \
        | sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p'
}

# 把 https://github.com/.../foo 转成 {proxy}/https://github.com/.../foo
convert_github_url() {
    local url="$1" proxy="$2"
    if [ "$proxy" = "direct" ] || [ -z "$proxy" ]; then
        printf '%s' "$url"
    else
        printf '%s/%s' "${proxy%/}" "$url"
    fi
}

choose_github_proxy() {
    # 参数保留是为了调用点兼容；手动选择不再需要 test_url
    : "${1:-}"
    if [ -n "${MAIBOT_FORCE_PROXY:-}" ]; then
        info "使用强制指定镜像: ${MAIBOT_FORCE_PROXY}"
        printf '%s' "${MAIBOT_FORCE_PROXY}"
        return
    fi

    local candidates=("direct" "${GITHUB_MIRRORS[@]}")
    local total=${#candidates[@]}

    # curl ... | bash 时 stdin 已被占用，必须从 /dev/tty 读
    if [ ! -r /dev/tty ]; then
        warn "无可用 TTY，无法手动选择镜像，回退直连 github.com"
        warn "如需走镜像，请重跑并设置 MAIBOT_FORCE_PROXY=https://gh-proxy.org"
        printf 'direct'
        return
    fi

    info "请选择 GitHub 镜像源 (回车默认 1):"
    local i=1
    for m in "${candidates[@]}"; do
        local label
        if [ "$m" = "direct" ]; then
            label="直连 github.com"
        else
            label="$m"
        fi
        printf '  %s%2d)%s %s\n' "$CYAN" "$i" "$RESET" "$label" >&2
        i=$((i+1))
    done
    printf '  %s(下次想跳过这步，可设置环境变量 MAIBOT_FORCE_PROXY)%s\n' "$DIM" "$RESET" >&2

    local pick=""
    while :; do
        printf '%s请输入序号 [1-%d, 回车=1]:%s ' "$BOLD" "$total" "$RESET" >&2
        if ! IFS= read -r pick < /dev/tty; then
            warn "读取输入失败，回退直连"
            printf 'direct'
            return
        fi
        [ -z "$pick" ] && pick=1
        if [[ "$pick" =~ ^[0-9]+$ ]] && [ "$pick" -ge 1 ] && [ "$pick" -le "$total" ]; then
            break
        fi
        warn "输入无效，请输入 1 到 ${total} 之间的整数"
    done

    local choice="${candidates[$((pick-1))]}"
    if [ "$choice" = "direct" ]; then
        ok "选择: 直连 github.com"
    else
        ok "选择: $choice"
    fi
    printf '%s' "$choice"
}

download() {
    local url="$1" out="$2"
    if [ -t 1 ]; then
        curl -fL --progress-bar -o "$out" "$url"
    else
        curl -fsSL -o "$out" "$url"
    fi
}

# 把 PATH 写入 rc 文件（幂等，按整行匹配跳过）
add_line_idempotent() {
    local rcfile="$1" line="$2"
    mkdir -p "$(dirname "$rcfile")"
    [ -f "$rcfile" ] || : > "$rcfile"
    if grep -Fxq "$line" "$rcfile"; then
        dim "  $rcfile 已包含 PATH，跳过"
        return
    fi
    {
        printf '\n# Added by MaiBot Manager TUI installer\n'
        printf '%s\n' "$line"
    } >> "$rcfile"
    ok "更新 $rcfile"
}

setup_path() {
    local install_dir="$1"
    case ":${PATH:-}:" in
        *":${install_dir}:"*)
            info "当前会话已包含 $install_dir，仅同步 shell 配置"
            ;;
    esac

    local sh_line='export PATH="'"${install_dir}"':$PATH"'
    local touched=0

    if [ -f "$HOME/.bashrc" ] || command -v bash >/dev/null 2>&1; then
        add_line_idempotent "$HOME/.bashrc" "$sh_line"
        touched=1
    fi
    if [ -f "$HOME/.bash_profile" ]; then
        add_line_idempotent "$HOME/.bash_profile" "$sh_line"
        touched=1
    fi
    if [ -f "$HOME/.zshrc" ] || command -v zsh >/dev/null 2>&1; then
        add_line_idempotent "$HOME/.zshrc" "$sh_line"
        touched=1
    fi
    if command -v fish >/dev/null 2>&1 || [ -d "$HOME/.config/fish" ]; then
        local fish_conf="$HOME/.config/fish/config.fish"
        local fish_line='set -gx PATH '"${install_dir}"' $PATH'
        add_line_idempotent "$fish_conf" "$fish_line"
        touched=1
    fi

    if [ "$touched" -eq 0 ]; then
        warn "未识别到 bash/zsh/fish 配置文件，请手动将 $install_dir 加入 PATH"
    fi
}

main() {
    require_cmd curl uname grep sed mkdir mv chmod mktemp

    if [ "$(uname -s)" != "Linux" ]; then
        err "仅支持 Linux（当前: $(uname -s)）。Windows 请在 WSL 中运行。"
        exit 1
    fi

    local asset
    asset="$(detect_asset)"
    info "目标资产: $asset"

    info "拉取 release 元数据..."
    local rel_json
    rel_json="$(fetch_release_json)"

    local tag
    tag="$(json_field "$rel_json" tag_name)"
    if [ -z "$tag" ]; then
        err "解析 release tag 失败，请检查网络或仓库状态"
        exit 1
    fi
    info "目标版本: $tag"

    local raw_url
    raw_url="$(find_asset_url "$rel_json" "$asset")"
    if [ -z "$raw_url" ]; then
        err "在 $tag 中未找到资产 $asset"
        exit 1
    fi

    local proxy dl_url
    proxy="$(choose_github_proxy "$raw_url")"
    dl_url="$(convert_github_url "$raw_url" "$proxy")"

    local tmpfile
    tmpfile="$(mktemp -t maibot-dl.XXXXXX)"
    trap 'rm -f "$tmpfile"' EXIT

    info "下载: $dl_url"
    if ! download "$dl_url" "$tmpfile"; then
        if [ "$proxy" != "direct" ]; then
            warn "通过镜像下载失败，尝试直连..."
            if ! download "$raw_url" "$tmpfile"; then
                err "下载失败"
                exit 1
            fi
        else
            err "下载失败"
            exit 1
        fi
    fi

    if ! head -c 4 "$tmpfile" | grep -q $'\x7fELF'; then
        err "下载文件不是有效的 Linux 可执行文件（可能是 HTML 错误页）"
        head -c 200 "$tmpfile" >&2 || true
        exit 1
    fi

    mkdir -p "$INSTALL_DIR"
    local dst="${INSTALL_DIR}/${BINARY_NAME}"
    mv -f "$tmpfile" "$dst"
    chmod +x "$dst"
    trap - EXIT
    ok "已安装: $dst"

    setup_path "$INSTALL_DIR"

    echo
    printf '%s%s%s\n' "$BOLD" "安装完成 ($tag)" "$RESET"
    printf '  二进制路径: %s\n' "$dst"
    printf '  调用方式:   %s\n' "$BINARY_NAME"
    echo
    if ! command -v "$BINARY_NAME" >/dev/null 2>&1; then
        warn "当前 shell 仍未识别 maibot 命令。"
        printf '    请重启终端，或执行其中之一:\n'
        printf '      bash/zsh: source ~/.bashrc 或 source ~/.zshrc\n'
        printf '      fish:     source ~/.config/fish/config.fish\n'
        printf '      临时:     export PATH="%s:$PATH"\n' "$INSTALL_DIR"
    fi
}

main "$@"
