#!/bin/bash
#License: GNU GENERAL PUBLIC LICENSE Version 3
#Author: 清蒸云鸭
#with VibeCoding
#Update: 2026-05-20

# =========================================================
# 1. 全局配置与变量
# =========================================================

CONFIG_FILE="$HOME/.maibot_config"
INSTALLATION_ACTIVE=false

# --- 颜色定义 ---
RED='\033[1;31m'
GREEN='\033[1;32m'
YELLOW='\033[1;33m'
BLUE='\033[1;34m'
PURPLE='\033[1;35m'
CYAN='\033[1;36m'
WHITE='\033[1;37m'
GREY='\033[0;37m'
NC='\033[0m'

GITHUB_MIRRORS=(
    "https://gh-proxy.org"
    "https://hk.gh-proxy.org"
    "https://cdn.gh-proxy.org"
    "https://ghproxy.net"
    "https://ghfast.top"
    "https://github.moeyy.xyz"
)

# 测速目标文件
TEST_FILE_PATH="https://raw.githubusercontent.com/Mai-with-u/MaiBot/refs/heads/main/README.md"

# 临时存储用户选择的变量
USER_INSTALL_PATH=""
USER_INSTALL_MODE=""   # normal / clean
USER_PYTHON_ENV=""     # system / uv
USER_VENV_MODE=""      # keep / recreate
USER_GH_PROXY=""       # GitHub 镜像源配置
USER_PIP_DISPLAY=""    # UI显示用
USER_PIP_INDEX=""      # pip 实际配置
USER_PIP_HOST=""       # pip 实际配置
USER_UV_INDEX=""       # uv 实际配置
USER_BOT_PROTOCOL_MODE=""

# =========================================================
# 2. UI & 工具函数
# =========================================================

log_info()    { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warning() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error()   { echo -e "${RED}[ERROR]${NC} $1"; }

# --- 标题栏与状态栏绘制 ---
draw_header() {
    clear
    echo -e "${PURPLE}┌────────────────────────────────────────────────────────┐${NC}"
    echo -e "${PURPLE}│${NC}           ${WHITE}MaiBot 一键部署与管理脚本 ${CYAN}v1.12${NC}              ${PURPLE}│${NC}"
    echo -e "${PURPLE}│${NC}                 ${WHITE}Copyright@清蒸云鸭${NC}                     ${PURPLE}│${NC}"
    echo -e "${PURPLE}│${NC}                ${WHITE}现已支持 MaiBot v1.0${NC}                    ${PURPLE}│${NC}"
    echo -e "${PURPLE}└────────────────────────────────────────────────────────┘${NC}"
    
    # 状态栏
    if [[ "$INSTALLATION_ACTIVE" == "true" && -n "$USER_INSTALL_PATH" ]]; then
        echo -e "${WHITE} 配置预览:${NC}"
        echo -e " ${GREY}●${NC} 目录: ${CYAN}${USER_INSTALL_PATH}${NC}"
        
        # 安装模式
        if [[ -n "$USER_INSTALL_MODE" ]]; then
            local mode_str="保留数据更新"
            [[ "$USER_INSTALL_MODE" == "clean" ]] && mode_str="${RED}清空并全新安装${NC}"
            echo -e " ${GREY}●${NC} 模式: ${mode_str}"
        fi

        # Python 运行环境
        if [[ -n "$USER_PYTHON_ENV" ]]; then
            local py_env_str="本机 python3"
            [[ "$USER_PYTHON_ENV" == "uv" ]] && py_env_str="${CYAN}uv (Python 3.14)${NC}"
            echo -e " ${GREY}●${NC} Python: ${py_env_str}"
        fi

        # 虚拟环境处理
        if [[ -n "$USER_VENV_MODE" && "$USER_INSTALL_MODE" != "clean" ]]; then
            local venv_str=""
            if [[ "$USER_PYTHON_ENV" == "uv" ]]; then
                venv_str="保留现有 .venv"
                [[ "$USER_VENV_MODE" == "recreate" ]] && venv_str="${YELLOW}删除并重建 .venv${NC}"
            else
                venv_str="保留旧环境"
                [[ "$USER_VENV_MODE" == "recreate" ]] && venv_str="${YELLOW}强制重建环境${NC}"
            fi
            echo -e " ${GREY}●${NC} 环境: ${venv_str}"
        fi

        # GitHub
        if [[ -n "$USER_GH_PROXY" ]]; then
            local gh_display="自定义/自动"
            [[ "$USER_GH_PROXY" == "https://github.com" ]] && gh_display="官方直连"
            if [[ "$gh_display" == "官方直连" ]]; then
                echo -e " ${GREY}●${NC} Git : ${CYAN}${gh_display}${NC}"
            else
                echo -e " ${GREY}●${NC} Git : ${CYAN}${USER_GH_PROXY}${NC}"
            fi
        fi
        
        # PyPI
        if [[ -n "$USER_PIP_DISPLAY" ]]; then
            echo -e " ${GREY}●${NC} PyPI: ${CYAN}${USER_PIP_DISPLAY}${NC}"
        fi

        # Bot 协议端
        if [[ -n "$USER_BOT_PROTOCOL_MODE" ]]; then
            local protocol_str="暂不安装"
            [[ "$USER_BOT_PROTOCOL_MODE" == "1" ]] && protocol_str="NapCatQQ (Docker)"
            [[ "$USER_BOT_PROTOCOL_MODE" == "2" ]] && protocol_str="LuckyLilliaBot (LinuxCLI)"
            echo -e " ${GREY}●${NC} 协议端: ${CYAN}${protocol_str}${NC}"
        fi
        echo -e "${PURPLE}──────────────────────────────────────────────────────────${NC}"
    else
        echo -e ""
    fi
}

# --- 分隔线 ---
draw_line() {
    echo -e "${PURPLE}──────────────────────────────────────────────────────────${NC}"
}

confirm_and_enter_screen() {
    local screen_name="$1"
    local cancel_message="${2:-已取消进入 Screen 窗口。}"

    echo -e "${YELLOW}进入 Screen 前请注意：必须使用 ${WHITE}Ctrl+A 然后按下 D${YELLOW} 退出窗口。${NC}"
    echo -e "${RED}如果直接关闭终端或使用错误方式退出，可能导致后续管理出现问题。${NC}"
    read -p "确认已知晓并进入 Screen？请输入 y 确认: " enter_screen_confirm
    if [[ "$enter_screen_confirm" == "y" || "$enter_screen_confirm" == "Y" ]]; then
        screen -r "$screen_name"
    else
        log_info "$cancel_message"
    fi
}

run_github_speedtest() {
    echo -e "${YELLOW}正在并行测速，请稍候...${NC}"
    local temp_dir=$(mktemp -d)
    local mirrors=("https://github.com" "${GITHUB_MIRRORS[@]}")

    for mirror in "${mirrors[@]}"; do
        (
            local test_url
            [[ "$mirror" == "https://github.com" ]] && test_url="$TEST_FILE_PATH" || test_url="${mirror}/${TEST_FILE_PATH}"

            local time_cost
            time_cost=$(curl -sL -o /dev/null --max-time 3 -w "%{time_total}" "$test_url")
            local exit_code=$?

            if [[ $exit_code -eq 0 ]]; then
                local ms=$(awk -v t="$time_cost" 'BEGIN {printf "%.0f", t*1000}')
                echo "$ms $mirror" >> "$temp_dir/results"
            else
                echo "9999 $mirror" >> "$temp_dir/results"
            fi
        ) &
    done
    wait

    echo -e "\n   延迟(ms) | 线路地址"
    echo -e "------------|----------------------------------"

    local best_mirror=""
    local best_ms=9999

    if [[ -f "$temp_dir/results" ]]; then
        sort -n "$temp_dir/results" > "$temp_dir/sorted"

        while read -r line; do
            local ms=$(echo "$line" | awk '{print $1}')
            local url=$(echo "$line" | awk '{print $2}')

            if [[ "$ms" == "9999" ]]; then
                echo -e " ${RED}超时/失败${NC}| $url"
            else
                if [[ -z "$best_mirror" ]]; then
                    best_mirror=$url
                    best_ms=$ms
                fi

                local color=$GREEN
                if [ "$ms" -gt 800 ]; then color=$YELLOW; fi
                if [ "$ms" -gt 1500 ]; then color=$RED; fi
                echo -e " ${color}${ms}ms${NC}\t| $url"
            fi
        done < "$temp_dir/sorted"
    fi

    if [[ -n "$best_mirror" ]]; then
        USER_GH_PROXY="$best_mirror"
        echo -e "\n自动选择: ${CYAN}$best_mirror${NC} (延迟: ${best_ms}ms)"
    else
        USER_GH_PROXY="https://gh-proxy.org"
        echo -e "\n${RED}测速全失败，使用默认代理。${NC}"
    fi

    rm -rf "$temp_dir"
    sleep 2
}

choose_github_proxy() {
    echo -e "${GREEN}1.${NC} 自动测速选择最佳线路 ${YELLOW}(推荐)${NC}"
    echo -e "${GREEN}2.${NC} 手动选择线路"
    echo -e "${GREEN}3.${NC} 官方直连"
    echo -e "${GREEN}4.${NC} 自定义镜像源"
    read -p "选择 [1-4] (默认1): " gh_choice
    case ${gh_choice:-1} in
        2)
            select mirror in "${GITHUB_MIRRORS[@]}"; do
                USER_GH_PROXY="$mirror"
                break
            done
            ;;
        3)
            USER_GH_PROXY="https://github.com"
            ;;
        4)
            echo -e "${YELLOW}请输入自定义 GitHub 镜像源，格式类似: https://gh-proxy.org${NC}"
            echo -e "${YELLOW}注意：末尾不要加 / ${NC}"
            read -p "请输入: " custom_gh_proxy
            custom_gh_proxy="${custom_gh_proxy%/}"
            if [[ -n "$custom_gh_proxy" ]]; then
                USER_GH_PROXY="$custom_gh_proxy"
            else
                log_warning "未输入自定义镜像源，改为自动测速选择。"
                run_github_speedtest
            fi
            ;;
        *)
            run_github_speedtest
            ;;
    esac
}

convert_github_url() {
    local url="$1"

    url="${url%.git}"

    local repo=""
    if [[ "$url" =~ github\.com/([^/]+/[^/]+)$ ]]; then
        repo="${BASH_REMATCH[1]}"
    else
        echo "$url"
        return
    fi

    if [[ "$USER_GH_PROXY" == "https://github.com" ]]; then
        echo "https://github.com/$repo.git"
    else
        echo "${USER_GH_PROXY}/https://github.com/${repo}.git"
    fi
}

get_plugin_names() {
    local plugins_dir="$1"
    [[ ! -d "$plugins_dir" ]] && return

    local dir
    for dir in "$plugins_dir"/*; do
        local dir_name
        dir_name="$(basename "$dir")"
        if [[ -d "$dir" && "$dir_name" != "__pycache__" && "$dir_name" != "data" ]]; then
            basename "$dir"
        fi
    done
}

list_plugins_display() {
    local plugins_dir="$1"
    local count=0
    local plugin_name

    while IFS= read -r plugin_name; do
        [[ -z "$plugin_name" ]] && continue
        count=$((count + 1))
        echo -e " ${CYAN}$plugin_name${NC}"
    done < <(get_plugin_names "$plugins_dir")

    if [[ $count -eq 0 ]]; then
        echo -e " ${GREY}(无插件)${NC}"
    fi
}

load_config() {
    if [[ -f "$CONFIG_FILE" ]]; then
        source "$CONFIG_FILE"
        if [[ -z "$MAI_PYTHON_ENV" ]]; then
            MAI_PYTHON_ENV="system"
        fi

        # 兼容旧配置: MAI_PATH <-> USER_INSTALL_PATH
        if [[ -z "$USER_INSTALL_PATH" && -n "$MAI_PATH" ]]; then
            USER_INSTALL_PATH="$MAI_PATH"
        fi
        if [[ -z "$MAI_PATH" && -n "$USER_INSTALL_PATH" ]]; then
            MAI_PATH="$USER_INSTALL_PATH"
        fi

        if [[ -n "$USER_INSTALL_PATH" || -n "$MAI_PATH" ]]; then return 0; fi
    fi
    return 1
}

save_config() {
    local path="$1"
    local python_env="$2"
    local llbot_path="$3"
    local llbot_path_provided="false"
    if [[ $# -ge 3 ]]; then
        llbot_path_provided="true"
    fi
    if [[ -z "$python_env" ]]; then
        python_env="${USER_PYTHON_ENV:-${MAI_PYTHON_ENV:-system}}"
    fi
    if [[ "$llbot_path_provided" != "true" && -z "$llbot_path" ]]; then
        if [[ -d "$path/LLBot" ]]; then
            llbot_path="$path/LLBot"
        else
            llbot_path="${MAI_LLBOT_PATH:-}"
        fi
    fi
    {
        echo "USER_INSTALL_PATH=\"$path\""
        echo "MAI_PATH=\"$path\""
        echo "MAI_PYTHON_ENV=\"$python_env\""
        echo "MAI_LLBOT_PATH=\"$llbot_path\""
    } > "$CONFIG_FILE"
}

ensure_uv_installed() {
    if command -v uv &> /dev/null; then
        return 0
    fi

    log_warning "未检测到 uv，开始自动安装..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    if [[ $? -ne 0 ]]; then
        log_error "uv 安装失败。"
        return 1
    fi

    # 刷新环境变量，确保当前 shell 可立即使用 uv
    if [[ -f "$HOME/.local/bin/env" ]]; then
        source "$HOME/.local/bin/env"
    fi
    hash -r

    if ! command -v uv &> /dev/null; then
        log_error "uv 已安装但当前会话仍不可用，请重新登录 shell 后重试。"
        return 1
    fi

    log_success "uv 安装并加载完成。"
    return 0
}

check_screen_installed() {
    if ! command -v screen &> /dev/null; then return 1; fi
    return 0
}

git_update_safe() {
    local repo_dir="$1"
    local branch="$2"
    local remote_url="$3"

    if [[ ! -d "$repo_dir/.git" ]]; then
        log_error "目录不是 Git 仓库: $repo_dir"
        return 2
    fi

    cd "$repo_dir" || return 1

    if [[ -n "$remote_url" ]]; then
        log_info "本次更新将使用当前选择的 Git 地址: $remote_url"
    fi

    if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then
        echo -e "${YELLOW}检测到仓库存在未提交改动:${NC} $repo_dir"
        echo -e "${GREEN}1.${NC} 临时保存改动后更新${NC}"
        echo -e "${RED}2.${NC} 丢弃本地改动并强制同步 ${YELLOW}(推荐)${NC}"
        echo -e "${WHITE}3.${NC} 取消更新"
        read -p "请选择 [1-3] (默认3): " dirty_choice
        case ${dirty_choice:-3} in
            1)
                git stash push -u -m "maibot-auto-update-$(date +%Y%m%d-%H%M%S)"
                if [[ $? -ne 0 ]]; then
                    log_error "保存本地改动失败，已取消更新。"
                    return 1
                fi
                ;;
            2)
                git reset --hard HEAD
                git clean -fd
                ;;
            *)
                log_warning "已取消更新。"
                return 3
                ;;
        esac
    fi

    if [[ -n "$branch" ]]; then
        if [[ -n "$remote_url" ]]; then
            git fetch --depth 1 "$remote_url" "$branch"
        else
            git fetch --depth 1 origin "$branch"
        fi
        if [[ $? -ne 0 ]]; then
            log_error "获取远程分支失败: $branch"
            return 1
        fi

        git checkout -B "$branch" FETCH_HEAD
        if [[ $? -ne 0 ]]; then
            log_error "切换分支失败: $branch"
            return 1
        fi

        git reset --hard FETCH_HEAD
        if [[ $? -ne 0 ]]; then
            log_error "同步远程分支失败: $branch"
            return 1
        fi
    else
        local current_branch
        current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)

        if [[ -z "$current_branch" || "$current_branch" == "HEAD" ]]; then
            log_error "无法识别当前分支，已取消更新。"
            return 1
        fi

        if [[ -n "$remote_url" ]]; then
            git fetch --depth 1 "$remote_url" "$current_branch"
        else
            git fetch --depth 1 origin "$current_branch"
        fi
        if [[ $? -ne 0 ]]; then
            log_error "获取远程分支失败: $current_branch"
            return 1
        fi

        git checkout -B "$current_branch" FETCH_HEAD
        if [[ $? -ne 0 ]]; then
            log_error "切换分支失败: $current_branch"
            return 1
        fi

        git reset --hard FETCH_HEAD
        if [[ $? -ne 0 ]]; then
            log_error "同步远程分支失败: $current_branch"
            return 1
        fi
    fi

    return 0
}

# Git Clone
git_clone_safe() {
    local url="$1"
    local dir="$2"
    local branch="$3"
    local install_path="${USER_INSTALL_PATH:-$MAI_PATH}"

    if [[ -z "$install_path" ]]; then
        load_config >/dev/null 2>&1
        install_path="${USER_INSTALL_PATH:-$MAI_PATH}"
    fi

    if [[ -z "$install_path" ]]; then
        log_error "未找到安装目录(USER_INSTALL_PATH/MAI_PATH)"
        return 1
    fi

    local target_dir="$dir"
    if [[ "$target_dir" != /* ]]; then
        target_dir="$install_path/$dir"
    fi

    local should_remove_uv_lock="false"
    if [[ "$USER_PYTHON_ENV" == "uv" && "$(basename "$target_dir")" == "MaiBot" ]]; then
        should_remove_uv_lock="true"
    fi
    
    # 目录存在处理逻辑
    if [[ -d "$target_dir" ]]; then
        if [[ "$USER_INSTALL_MODE" == "clean" ]]; then
            log_warning "清理旧目录: $target_dir"
            rm -rf "$target_dir"
        else
            log_info "检测到目录 ${CYAN}$target_dir${NC} 已存在，尝试更新..."

            if [[ "$should_remove_uv_lock" == "true" && -f "$target_dir/uv.lock" ]]; then
                log_warning "检测到 MaiBot 主程序目录下存在 uv.lock，更新前先移除以避免 git/uv 冲突..."
                rm -f "$target_dir/uv.lock"
            fi

            git_update_safe "$target_dir" "$branch" "$url"
            local update_result=$?
            if [[ $update_result -eq 0 ]]; then
                cd "$install_path" || return 1
                return 0
            elif [[ $update_result -eq 3 ]]; then
                cd "$install_path" || return 1
                return 1
            else
                log_error "更新失败。"
                cd "$install_path" || return 1
                echo -e "${YELLOW}是否删除旧文件夹并重新克隆？${NC}"
                read -p "请输入 (y/n): " re_choice
                if [[ "$re_choice" == "y" ]]; then rm -rf "$target_dir"; else return 1; fi
            fi
        fi
    fi

    while true; do
        log_info "正在克隆 ${CYAN}$dir${NC}..."
        if [[ -n "$branch" ]]; then
            git clone --depth 1 --progress -b "$branch" "$url" "$target_dir"
        else
            git clone --depth 1 --progress "$url" "$target_dir"
        fi

        if [ $? -eq 0 ]; then
            log_success "$dir 克隆成功"
            return 0
        else
            log_error "克隆失败！请检查网络或更换加速源。"
            rm -rf "$target_dir"
            echo -e "1. 重试  2. 跳过  3. 退出"
            read -p "请选择: " retry_choice
            case $retry_choice in
                2) return 1 ;;
                3) exit 1 ;;
                *) ;;
            esac
        fi
    done
}

# Docker Compose Up
docker_compose_safe() {
    local work_dir="$1"
    cd "$work_dir" || return 1
    while true; do
        log_info "正在启动 Docker 容器..."
        docker compose up -d
        if [ $? -eq 0 ]; then return 0; else
            log_error "容器启动失败！"
            echo -e "${YELLOW}尝试清理容器状态...${NC}"
            docker compose down 2>/dev/null
            read -p "1. 重试  2. 放弃 : " dc_choice
            if [[ "$dc_choice" == "2" ]]; then return 1; fi
        fi
    done
}

remove_existing_napcat_containers() {
    if ! command -v docker &> /dev/null; then
        log_error "未检测到 Docker，无法移除 NapCat 容器。"
        return 1
    fi

    local napcat_containers
    napcat_containers=$(docker ps -a --format '{{.Names}}' | grep '^napcat' || true)

    if [[ -z "$napcat_containers" ]]; then
        log_info "未找到名称以 napcat 开头的容器。"
        return 0
    fi

    echo -e "${YELLOW}将移除以下 NapCat 容器:${NC}"
    echo "$napcat_containers" | sed 's/^/ - /'

    while read -r container_name; do
        [[ -z "$container_name" ]] && continue
        docker rm -f "$container_name"
    done <<< "$napcat_containers"

    if [[ $? -eq 0 ]]; then
        log_success "NapCat 容器移除完成。"
        return 0
    else
        log_error "NapCat 容器移除失败。"
        return 1
    fi
}

# Docker 安装函数
install_docker_safe() {
    while true; do
        log_info "调用 LinuxMirrors 脚本安装 Docker..."
        bash <(curl -sSL https://linuxmirrors.cn/docker.sh)
        if command -v docker &> /dev/null; then log_success "Docker 安装成功！"; return 0; else
            log_error "Docker 安装检测失败。"
            read -p "1. 重试  2. 手动安装后继续  3. 退出 : " d_inst_choice
            case $d_inst_choice in 2) return 0 ;; 3) exit 1 ;; *) ;; esac
        fi
    done
}

# Docker 镜像源配置
configure_docker_mirror() {
    if ! command -v docker &> /dev/null; then return; fi
    echo -e "${BLUE}▶ Docker 镜像加速配置${NC}"
    echo -e "${GREEN}1.${NC} docker.1ms.run ${WHITE}(国内首选)${NC}"
    echo -e "${GREEN}2.${NC} docker.xuanyuan.me ${WHITE}(国内次选)${NC}"
    echo -e "${GREEN}3.${NC} 官方源 ${WHITE}(香港/海外首选)${NC}"
    echo -e "${GREEN}4.${NC} 保持不变"
    read -p "请选择 [1-4] (默认4): " mirror_choice
    mirror_choice=${mirror_choice:-4}
    if [[ "$mirror_choice" == "4" ]]; then return; fi

    local mirror_url=""
    case $mirror_choice in
        1) mirror_url="https://docker.1ms.run" ;;
        2) mirror_url="https://docker.xuanyuan.me" ;;
        3) mirror_url="OFFICIAL" ;;
        *) return ;;
    esac

    log_info "正在配置 Docker daemon..."
    local daemon_file="/etc/docker/daemon.json"
    mkdir -p /etc/docker
    if [[ "$mirror_url" == "OFFICIAL" ]]; then
        if [[ -f "$daemon_file" ]]; then echo "{}" > "$daemon_file"; fi
    else
        echo "{\"registry-mirrors\": [\"$mirror_url\"]}" > "$daemon_file"
    fi
    systemctl restart docker
}

# =========================================================
# 3. 配置流程模块
# =========================================================

configure_install_path() {
    draw_header
    echo -e "${BLUE}▶ 1/6 安装目录配置${NC}"
    local default_path="$HOME/maimai"
    load_config
    if [[ -n "$MAI_PATH" ]]; then default_path="$MAI_PATH"; fi
    echo -e "上次/默认安装位置: ${CYAN}$default_path${NC}"
    read -p "请输入安装路径 (回车使用默认): " user_path
    if [[ -z "$user_path" ]]; then
        USER_INSTALL_PATH="$default_path"
    else
        user_path="${user_path/#\~/$HOME}"
        if [[ "$user_path" != /* ]]; then
            USER_INSTALL_PATH="$HOME/$user_path"
        else
            USER_INSTALL_PATH="$user_path"
        fi
    fi
    mkdir -p "$USER_INSTALL_PATH"
}

step_install_mode() {
    draw_header
    echo -e "${BLUE}▶ 2/6 选择安装模式${NC}"
    if [[ ! -d "$USER_INSTALL_PATH/MaiBot" ]]; then
        USER_INSTALL_MODE="normal"; USER_VENV_MODE="recreate"; return
    fi
    echo -e "${GREEN}1.${NC} 正常更新/修复 ${WHITE}(保留配置文件与数据)${NC}"
    echo -e "${RED}2. 全新安装 ${NC}${YELLOW}(⚠️  删除目录下所有内容，配置不保留)${NC}"
    read -p "请选择 [1-2] (默认1): " mode_choice
    case ${mode_choice:-1} in
        2) 
            echo -e "${RED}严重警告：此操作将清空 $USER_INSTALL_PATH 下的所有数据！${NC}"
            read -p "确认执行? 输入 'YES' 继续: " confirm_clean
            if [[ "$confirm_clean" == "YES" ]]; then USER_INSTALL_MODE="clean"; USER_VENV_MODE="recreate"; else USER_INSTALL_MODE="normal"; fi
            ;;
        *) USER_INSTALL_MODE="normal" ;;
    esac
}

step_python_env_mode() {
    draw_header
    echo -e "${BLUE}▶ 3/6 Python 环境方式${NC}"
    local default_choice="2"

    if [[ -z "$USER_PYTHON_ENV" ]]; then
        if load_config && [[ "$MAI_PYTHON_ENV" == "uv" ]]; then
            USER_PYTHON_ENV="uv"
        else
            USER_PYTHON_ENV="system"
        fi
    fi

    [[ "$USER_PYTHON_ENV" == "uv" ]] && default_choice="2"

    echo -e "${GREEN}1.${NC} 本机 python3 环境"
    echo -e "${GREEN}2.${NC} uv 环境 ${WHITE}(Python 3.14)${NC}"
    read -p "请选择 [1-2] (默认${default_choice}): " py_choice
    case ${py_choice:-$default_choice} in
        2) USER_PYTHON_ENV="uv" ;;
        *) USER_PYTHON_ENV="system" ;;
    esac
}

step_venv_mode() {
    if [[ "$USER_INSTALL_MODE" == "clean" ]]; then
        USER_VENV_MODE="recreate"
        return
    fi

    draw_header
    if [[ "$USER_PYTHON_ENV" == "uv" ]]; then
        echo -e "${BLUE}▶ 4/6 uv 虚拟环境处理${NC}"
        echo -e "${GREEN}1.${NC} 保留现有 .venv ${WHITE}(首选)${NC}"
        echo -e "${YELLOW}2.${NC} 删除并重建 .venv ${WHITE}(如有依赖冲突选这个)${NC}"
    else
        echo -e "${BLUE}▶ 4/6 Python 虚拟环境处理${NC}"
        echo -e "${GREEN}1.${NC} 保留现有环境 ${WHITE}(首选)${NC}"
        echo -e "${YELLOW}2.${NC} 删除并重建环境 ${WHITE}(如有依赖冲突选这个)${NC}"
    fi

    read -p "请选择 [1-2] (默认1): " venv_choice
    case ${venv_choice:-1} in 2) USER_VENV_MODE="recreate" ;; *) USER_VENV_MODE="keep" ;; esac
}

configure_github() {
    draw_header
    echo -e "${BLUE}▶ 5/6 GitHub 线路配置${NC}"
    choose_github_proxy
}

configure_pip() {
    draw_header
    echo -e "${BLUE}▶ 6/6 Python 依赖源配置${NC}"
    echo -e "${GREY}该配置会同时用于 pip 与 uv。${NC}"

    echo -e "${GREEN}1.${NC} 保持现状/系统默认"
    echo -e "${GREEN}2.${NC} 阿里云${YELLOW}(国内首选，下载速度快)${NC}"
    echo -e "${GREEN}3.${NC} 腾讯云"
    echo -e "${GREEN}4.${NC} 清华大学"
    echo -e "${GREEN}5.${NC} 中国科学技术大学${NC}"
    echo -e "${GREEN}6.${NC} 官方源${YELLOW}(海外首选)${NC}"
    echo -e "${GREEN}7.${NC} 自定义镜像源"
    read -p "选择 [1-7] (默认1): " pip_choice
    case ${pip_choice:-1} in
        2)
            USER_PIP_DISPLAY="阿里云"
            USER_PIP_INDEX="https://mirrors.aliyun.com/pypi/simple/"
            USER_PIP_HOST="mirrors.aliyun.com"
            USER_UV_INDEX="$USER_PIP_INDEX"
            ;;
        3)
            USER_PIP_DISPLAY="腾讯云"
            USER_PIP_INDEX="http://mirrors.cloud.tencent.com/pypi/simple"
            USER_PIP_HOST="mirrors.cloud.tencent.com"
            USER_UV_INDEX="$USER_PIP_INDEX"
            ;;
        4)
            USER_PIP_DISPLAY="清华大学"
            USER_PIP_INDEX="https://pypi.tuna.tsinghua.edu.cn/simple"
            USER_PIP_HOST="pypi.tuna.tsinghua.edu.cn"
            USER_UV_INDEX="$USER_PIP_INDEX"
            ;;
        5)
            USER_PIP_DISPLAY="中国科学技术大学"
            USER_PIP_INDEX="https://pypi.mirrors.ustc.edu.cn/simple/"
            USER_PIP_HOST="pypi.mirrors.ustc.edu.cn"
            USER_UV_INDEX="$USER_PIP_INDEX"
            ;;
        6)
            USER_PIP_DISPLAY="官方源"
            USER_PIP_INDEX="https://pypi.org/simple"
            USER_PIP_HOST="pypi.org"
            USER_UV_INDEX="$USER_PIP_INDEX"
            ;;
        7)
            echo -e "${YELLOW}请输入自定义 PyPI 镜像源，格式类似: https://pypi.example.com/simple${NC}"
            read -p "请输入: " custom_pip_index
            if [[ -n "$custom_pip_index" ]]; then
                if [[ "$custom_pip_index" != http://* && "$custom_pip_index" != https://* ]]; then
                    custom_pip_index="https://$custom_pip_index"
                fi
                USER_PIP_DISPLAY="$custom_pip_index"
                USER_PIP_INDEX="$custom_pip_index"
                USER_PIP_HOST=$(echo "$custom_pip_index" | awk -F/ '{print $3}')
                USER_UV_INDEX="$USER_PIP_INDEX"
            else
                USER_PIP_DISPLAY="系统默认"
                USER_PIP_INDEX=""
                USER_PIP_HOST=""
                USER_UV_INDEX=""
            fi
            ;;
        *)
            USER_PIP_DISPLAY="系统默认"
            USER_PIP_INDEX=""
            USER_PIP_HOST=""
            USER_UV_INDEX=""
            ;;
    esac
}

configure_bot_protocol_selection() {
    draw_header
    echo -e "${BLUE}▶ 附加: Bot 协议端部署选项${NC}"
    echo -e "${GREEN}1.${NC} NapCatQQ ${WHITE}(使用 Docker 安装)${NC}"
    echo -e "${GREEN}2.${NC} LuckyLilliaBot ${WHITE}(LinuxCLI 安装，风控概率更小)${NC}"
    echo -e "${GREEN}3.${NC} 暂不安装"
    read -p "请选择 [1-3] (默认1): " nc_choice
    USER_BOT_PROTOCOL_MODE=${nc_choice:-1}
}

prepare_bot_protocol_environment() {
    if [[ "$USER_BOT_PROTOCOL_MODE" != "1" ]]; then return; fi
    draw_header
    echo -e "${BLUE}▶ Docker 环境准备${NC}"
    if ! command -v docker &> /dev/null; then
        log_warning "未检测到 Docker，准备安装..."
        install_docker_safe
    fi
    configure_docker_mirror
}

# =========================================================
# 4. 执行安装模块
# =========================================================

run_install() {
    draw_header
    echo -e "${YELLOW}请确认上方配置预览无误。${NC}"
    if [[ "$USER_INSTALL_MODE" == "clean" ]]; then
        echo -e "${RED}警告: 将执行全新安装，目标目录数据将被清除！${NC}"
    fi
    read -p "确认无误开始安装? (y/n): " confirm
    if [[ "$confirm" != "y" ]]; then return; fi

    echo -e "\n${BLUE}▶ 开始安装系统依赖...${NC}"
    if command -v apt &> /dev/null; then
        sudo DEBIAN_FRONTEND=noninteractive apt update -y -qq
        if [[ "$USER_PYTHON_ENV" == "uv" ]]; then
            sudo DEBIAN_FRONTEND=noninteractive apt install -y -qq build-essential git wget curl screen jq unzip
        else
            sudo DEBIAN_FRONTEND=noninteractive apt install -y -qq python3-dev python3-venv python3-pip build-essential git wget curl screen jq unzip
        fi
    elif command -v yum &> /dev/null; then
        if [[ "$USER_PYTHON_ENV" == "uv" ]]; then
            sudo yum install -y git wget curl screen jq unzip
        else
            sudo yum install -y python3-devel git wget curl screen jq unzip
        fi
    fi

    # 目录清理
    if [[ "$USER_INSTALL_MODE" == "clean" && -n "$USER_INSTALL_PATH" ]]; then
        if [[ "$USER_INSTALL_PATH" != "/" && "$USER_INSTALL_PATH" != "$HOME" ]]; then
            log_warning "执行清理..."
            rm -rf "${USER_INSTALL_PATH:?}/"*
        fi
    fi

    cd "$USER_INSTALL_PATH" || exit 1
    
    get_url() {
        local repo=$1
        if [[ "$USER_GH_PROXY" == "https://github.com" ]]; then echo "https://github.com/$repo.git"; else echo "${USER_GH_PROXY}/https://github.com/${repo}.git"; fi
    }

    echo -e "\n${BLUE}▶ 下载/更新组件...${NC}"
    git_clone_safe "$(get_url 'MaiM-with-u/MaiBot')" "MaiBot" "main"
    git_clone_safe "$(get_url 'MaiM-with-u/MaiBot-Napcat-Adapter')" "MaiBot/plugins/MaiBot-Napcat-Adapter" "main"
    
    echo -e "\n${BLUE}▶ 初始化配置文件...${NC}"
    
   

    echo -e "\n${BLUE}▶ 配置 Python 环境...${NC}"
    if [[ "$USER_PYTHON_ENV" == "uv" ]]; then
        ensure_uv_installed || return
        cd "$USER_INSTALL_PATH/MaiBot" || exit 1

        if [[ -f "uv.lock" ]]; then
            log_warning "检测到 MaiBot 主程序目录下存在 uv.lock，先移除以避免依赖冲突..."
            rm -f uv.lock
        fi

        if [[ -n "$USER_UV_INDEX" ]]; then
            export UV_INDEX_URL="$USER_UV_INDEX"
            export PIP_INDEX_URL="$USER_UV_INDEX"
            log_info "已为 uv/pip 设置镜像源: $USER_UV_INDEX"
        fi

        if [[ "$USER_VENV_MODE" == "recreate" && -d ".venv" ]]; then
            log_warning "移除旧 uv 虚拟环境..."
            rm -rf .venv
        fi

        if [[ ! -d ".venv" ]]; then
            echo " - 使用 uv 创建虚拟环境 (.venv, Python 3.14)..."
            uv venv --python 3.14
        fi

        echo " - 使用 uv sync 同步依赖..."
        uv sync
        cd "$USER_INSTALL_PATH" || exit 1
    else
        if [[ "$USER_VENV_MODE" == "recreate" && -d "venv" ]]; then
            log_warning "移除旧虚拟环境..."
            rm -rf venv
        fi
        if [[ ! -d "venv" ]]; then
            echo " - 创建虚拟环境 venv..."
            python3 -m venv venv
        fi
        source venv/bin/activate

        if [[ -n "$USER_PIP_INDEX" ]]; then
            mkdir -p ~/.pip
            echo -e "[global]\nindex-url = $USER_PIP_INDEX\ntrusted-host = $USER_PIP_HOST" > ~/.pip/pip.conf
        fi

        echo " - 更新 pip..."
        pip install --upgrade pip

        if [[ -f "MaiBot/requirements.txt" ]]; then pip install -r MaiBot/requirements.txt; fi
        if [[ -f "MaiBot/plugins/MaiBot-Napcat-Adapter/requirements.txt" ]]; then pip install -r MaiBot/plugins/MaiBot-Napcat-Adapter/requirements.txt; fi
    fi

    save_config "$USER_INSTALL_PATH" "$USER_PYTHON_ENV"
    log_success "MaiBot 本体部署完成！"
    execute_napcat_install
    
    draw_line
    echo -e "${GREEN}所有任务执行完毕！${NC}"
    echo -e "请使用主菜单的 [管理 MaiBot] 或 [管理Bot协议端服务] 来启动服务。"
    read -p "按回车返回主菜单..."
}

execute_napcat_install() {
    if [[ "$USER_BOT_PROTOCOL_MODE" == "3" ]]; then return; fi
    local NAPCAT_DIR="$USER_INSTALL_PATH/NapCat"
    local LLBOT_DIR="$USER_INSTALL_PATH/LLBot"

    get_github_asset_url() {
        local asset_url="$1"
        if [[ "$USER_GH_PROXY" == "https://github.com" || -z "$USER_GH_PROXY" ]]; then
            echo "$asset_url"
        else
            echo "${USER_GH_PROXY}/${asset_url}"
        fi
    }

    detect_llbot_arch() {
        local arch
        arch=$(uname -m)
        case "$arch" in
            x86_64|amd64) echo "x64" ;;
            aarch64|arm64) echo "arm64" ;;
            *)
                log_error "当前主机架构不受支持: $arch，仅支持 x64 / arm64。"
                return 1
                ;;
        esac
    }

    install_linuxqq_for_llbot() {
        if [[ -f "/opt/QQ/qq" ]]; then
            log_info "检测到系统已安装 LinuxQQ，跳过预安装。"
            return 0
        fi

        local machine arch_deb lib_snd qq_deb
        machine=$(uname -m)

        if command -v apt &>/dev/null; then
            case "$machine" in
                x86_64|amd64) arch_deb="amd64" ;;
                aarch64|arm64) arch_deb="arm64" ;;
                *)
                    log_error "当前主机架构不支持 LinuxQQ 预安装: $machine"
                    return 1
                    ;;
            esac

            echo -e "\n${BLUE}▶ 预安装 LinuxQQ...${NC}"
            sudo apt-get update -y || return 1
            sudo apt-get install -y wget || return 1
            qq_deb="/tmp/linuxqq_${arch_deb}.deb"
            if ! wget -O "$qq_deb" "https://dldir1v6.qq.com/qqfile/qq/QQNT/ab90fdfa/linuxqq_3.2.20-40768_${arch_deb}.deb"; then
                log_error "LinuxQQ 安装包下载失败。"
                return 1
            fi

            lib_snd="libasound2"
            apt-cache show libasound2t64 &>/dev/null && lib_snd="libasound2t64"
            if ! sudo apt install -y "$qq_deb" x11-utils libgtk-3-0 libxcb-xinerama0 libgl1-mesa-dri libnotify4 libnss3 xdg-utils libsecret-1-0 libappindicator3-1 libgbm1 "$lib_snd" fonts-noto-cjk libxss1; then
                rm -f "$qq_deb"
                log_error "LinuxQQ 预安装失败。"
                return 1
            fi
            rm -f "$qq_deb"
            log_success "LinuxQQ 预安装完成。"
            return 0
        fi

        if command -v pacman &>/dev/null; then
            echo -e "\n${BLUE}▶ 预安装 LinuxQQ...${NC}"
            if command -v yay &>/dev/null; then
                if ! yay -S --noconfirm linuxqq; then
                    log_error "LinuxQQ 预安装失败。"
                    return 1
                fi
                log_success "LinuxQQ 预安装完成。"
                return 0
            fi

            log_warning "检测到 Arch 系系统，但未安装 yay，暂时无法自动预装 LinuxQQ。"
            log_warning "后续首次启动 LLBot 时，可按官方脚本提示安装 QQ。"
            return 0
        fi

        log_warning "当前系统未识别为 apt/pacman 环境，跳过 LinuxQQ 预安装。"
        return 0
    }

    update_llbot_default_config() {
        local config_file="$LLBOT_DIR/bin/llbot/default_config.json"
        if [[ ! -f "$config_file" ]]; then
            log_warning "未找到 LLBot 默认配置文件: $config_file"
            return 0
        fi

        local current_webui_port="3080"

        if command -v jq &>/dev/null; then
            current_webui_port=$(jq -r '.webui.port // 3080' "$config_file" 2>/dev/null)
        fi

        python3 - <<PY
import json
from pathlib import Path

path = Path("$config_file")
data = json.loads(path.read_text(encoding="utf-8"))

data.setdefault("webui", {})
data["webui"]["host"] = "0.0.0.0"
data["webui"]["port"] = int("$current_webui_port")
data["webui"]["enable"] = True

path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY

        log_success "已将 LLBot 默认配置中的 WebUI 调整为对外监听。"
        echo -e "  WebUI: ${CYAN}0.0.0.0:${current_webui_port}${NC}"
    }

    install_llbot_cli() {
        echo -e "\n${BLUE}▶ 部署 LuckyLilliaBot...${NC}"
        local llbot_arch asset_name api_url asset_url download_url zip_path backup_data_dir
        llbot_arch=$(detect_llbot_arch) || return 1
        asset_name="LLBot-CLI-linux-${llbot_arch}.zip"
        api_url="https://api.github.com/repos/LLOneBot/LuckyLilliaBot/releases/latest"

        mkdir -p "$LLBOT_DIR"
        log_info "检测 LuckyLilliaBot 最新 Release..."
        if command -v jq &>/dev/null; then
            asset_url=$(curl -fsSL "$api_url" | jq -r --arg asset_name "$asset_name" '.assets[] | select(.name == $asset_name) | .browser_download_url' | head -n 1)
        else
            asset_url=$(curl -fsSL "$api_url" | python3 -c "import json,sys; data=json.load(sys.stdin); name='$asset_name'; print(next((a['browser_download_url'] for a in data.get('assets', []) if a.get('name') == name), ''))" 2>/dev/null)
        fi
        if [[ -z "$asset_url" ]]; then
            log_error "未找到对应架构的 LuckyLilliaBot 安装包: $asset_name"
            return 1
        fi

        download_url=$(get_github_asset_url "$asset_url")
        zip_path="$LLBOT_DIR/$asset_name"
        log_info "下载资产: $asset_name"
        if ! curl -fL --retry 3 --connect-timeout 10 -o "$zip_path" "$download_url"; then
            log_error "LuckyLilliaBot 下载失败。"
            return 1
        fi

        if [[ -d "$LLBOT_DIR/bin/llbot/data" ]]; then
            backup_data_dir="$LLBOT_DIR/.llbot_data_backup"
            rm -rf "$backup_data_dir"
            cp -a "$LLBOT_DIR/bin/llbot/data" "$backup_data_dir"
            log_info "已备份现有 LLBot 数据目录，更新后将自动恢复。"
        fi

        log_info "更新 / 解压 LuckyLilliaBot 到: $LLBOT_DIR"
        rm -rf "$LLBOT_DIR/bin" "$LLBOT_DIR/llbot" "$LLBOT_DIR/start.sh" "$LLBOT_DIR/使用说明.txt" "$LLBOT_DIR/更新日志.txt"
        if ! unzip -oq "$zip_path" -d "$LLBOT_DIR"; then
            log_error "LuckyLilliaBot 解压失败。"
            return 1
        fi

        if [[ -n "$backup_data_dir" && -d "$backup_data_dir" ]]; then
            rm -rf "$LLBOT_DIR/bin/llbot/data"
            mv "$backup_data_dir" "$LLBOT_DIR/bin/llbot/data"
            log_info "已恢复 LLBot 旧数据目录（保留原密码与运行数据）。"
        fi

        chmod +x "$LLBOT_DIR/start.sh" "$LLBOT_DIR/llbot" 2>/dev/null || true
        if [[ -d "$LLBOT_DIR/bin" ]]; then
            find "$LLBOT_DIR/bin" -type f -exec chmod +x {} \; 2>/dev/null || true
        fi

        install_linuxqq_for_llbot || return 1
        update_llbot_default_config || return 1
        save_config "$USER_INSTALL_PATH" "$USER_PYTHON_ENV" "$LLBOT_DIR"
        log_success "LuckyLilliaBot (LinuxCLI) 部署成功！"
        echo -e "安装目录: ${CYAN}${LLBOT_DIR}${NC}"
        echo -e "启动脚本: ${CYAN}${LLBOT_DIR}/start.sh${NC}"
        echo -e "默认 WebUI: ${CYAN}http://0.0.0.0:3080${NC}"
    }

    case $USER_BOT_PROTOCOL_MODE in
        1)
            echo -e "\n${BLUE}▶ 部署 NapCatQQ...${NC}"
            mkdir -p "$NAPCAT_DIR"
            log_info "生成 docker-compose.yml..."
            # 移除 WEBUI_TOKEN，修改 UID/GID
            cat > "$NAPCAT_DIR/docker-compose.yml" <<EOF
services:
  napcat:
    image: mlikiowa/napcat-docker:latest
    container_name: napcat
    restart: always
    environment:
      - NAPCAT_UID=\${NAPCAT_UID:-1000}
      - NAPCAT_GID=\${NAPCAT_GID:-1000}
    volumes:
      - ./config:/app/napcat/config
      - ./qq_config:/app/.config/QQ
    network_mode: "host"
EOF
            docker_compose_safe "$NAPCAT_DIR"
            if [ $? -eq 0 ]; then
                log_success "NapCat (Docker) 部署成功！"
            else
                log_error "NapCat (Docker) 部署失败。"
                echo -e "${YELLOW}如果报错与现有 napcat 容器冲突有关，可尝试移除旧容器。${NC}"
                read -p "是否移除现有名称以 napcat 开头的容器并重试？(y/n): " remove_retry
                if [[ "$remove_retry" == "y" || "$remove_retry" == "Y" ]]; then
                    if remove_existing_napcat_containers; then
                        docker_compose_safe "$NAPCAT_DIR"
                        if [ $? -eq 0 ]; then
                            log_success "NapCat (Docker) 部署成功！"
                        else
                            log_error "重试后 NapCat (Docker) 仍部署失败。"
                        fi
                    fi
                fi
            fi
            ;;
        2)
            install_llbot_cli
            ;;
    esac
}

# =========================================================
# 5. 配置与访问菜单
# =========================================================

get_ip() {
    local ip=$(curl -s4 ifconfig.me)
    if [[ -z "$ip" ]]; then ip="127.0.0.1"; fi
    echo "$ip"
}

manage_config_access_menu() {
    if ! load_config; then log_error "未找到配置"; return; fi
    local MAIBOT_DIR="$MAI_PATH/MaiBot"
    local ADAPTER_DIR="$MAI_PATH/MaiBot/plugins/MaiBot-Napcat-Adapter"
    local NAPCAT_DIR="$MAI_PATH/NapCat"
    local LLBOT_DIR="${MAI_LLBOT_PATH:-$MAI_PATH/LLBot}"
    local PUBLIC_IP=$(get_ip)

    while true; do
        draw_header
        echo -e "${BLUE}▶ 配置与访问${NC}"
        echo -e " 公网IP: ${CYAN}${PUBLIC_IP}${NC}"
        draw_line
        echo -e "${GREEN}1.${NC} 查看 WebUI 访问信息 (MaiBot / NapCat / LLBot)"
        echo -e "${GREEN}2.${NC} 初始化MaiBot访问配置"
        echo -e "${GREEN}3.${NC} 修改 Adapter 配置 (黑白名单管理)"
        draw_line
        echo -e "${WHITE}0.${NC} 返回上一级"
        echo -e ""
        read -p " 请选择: " opt
        
        case $opt in
            1) 
                draw_header
                echo -e "${BLUE}▶ WebUI 访问汇总${NC}"
                
                # --- Part A: MaiBot ---
                echo -e "\n${PURPLE}● MaiBot WebUI${NC}"
                local bot_cfg="$MAIBOT_DIR/config/bot_config.toml"
                local webui_json="$MAIBOT_DIR/data/webui.json"
                if [[ -f "$bot_cfg" ]]; then
                    local webui_host=""
                    local port=""
                    local token=""

                    webui_host=$(awk '
                        /^\[webui\]/ {in_webui=1; next}
                        /^\[/ {if (in_webui) exit; in_webui=0}
                        in_webui && $0 ~ /^[[:space:]]*host[[:space:]]*=/ {
                            line=$0
                            sub(/^[^=]*=[[:space:]]*/, "", line)
                            sub(/[[:space:]]*#.*/, "", line)
                            gsub(/["[:space:]]/, "", line)
                            print line
                            exit
                        }
                    ' "$bot_cfg")

                    port=$(awk '
                        /^\[webui\]/ {in_webui=1; next}
                        /^\[/ {if (in_webui) exit; in_webui=0}
                        in_webui && $0 ~ /^[[:space:]]*port[[:space:]]*=/ {
                            line=$0
                            sub(/^[^=]*=[[:space:]]*/, "", line)
                            sub(/[[:space:]]*#.*/, "", line)
                            gsub(/["[:space:]]/, "", line)
                            print line
                            exit
                        }
                    ' "$bot_cfg")

                    [[ -z "$webui_host" ]] && webui_host="0.0.0.0"
                    [[ -z "$port" ]] && port="8001"

                    if [[ -f "$webui_json" ]]; then
                        if command -v jq &>/dev/null; then
                            token=$(jq -r '.access_token' "$webui_json")
                        else
                            token=$(python3 -c "import json; print(json.load(open('$webui_json'))['access_token'])" 2>/dev/null)
                        fi
                    else
                        token="(未生成，请先启动 MaiBot 本体)"
                    fi

                    local display_host="$PUBLIC_IP"
                    if [[ "$webui_host" == "127.0.0.1" || "$webui_host" == "localhost" ]]; then
                        display_host="$webui_host"
                    fi

                    echo -e "  访问地址: ${CYAN}http://${display_host}:${port}${NC}"
                    echo -e "  访问密钥: ${YELLOW}${token}${NC}"

                    if [[ "$webui_host" == "127.0.0.1" || "$webui_host" == "localhost" ]]; then
                        echo -e "  ${YELLOW}当前 WebUI host=$webui_host，仅本机可访问，无法远程访问。${NC}"
                        echo -e "  ${YELLOW}请修改 $bot_cfg 中 [webui] 的 host 为 0.0.0.0 后重启 MaiBot。${NC}"
                    fi
                else
                    echo -e "  ${YELLOW}未找到 MaiBot 配置文件: $bot_cfg${NC}"
                fi
                
                draw_line 

                # --- Part B: NapCat ---
                echo -e "${PURPLE}● NapCat WebUI (Docker)${NC}"
                local nc_conf="$NAPCAT_DIR/config/webui.json"
                if [[ -f "$nc_conf" ]]; then
                    local nc_port=""
                    local nc_token=""
                    if command -v jq &>/dev/null; then
                        nc_port=$(jq -r '.port' "$nc_conf")
                        nc_token=$(jq -r '.token' "$nc_conf")
                    else
                        nc_port=$(python3 -c "import json; print(json.load(open('$nc_conf'))['port'])" 2>/dev/null)
                        nc_token=$(python3 -c "import json; print(json.load(open('$nc_conf'))['token'])" 2>/dev/null)
                    fi
                    
                    echo -e "  访问地址: ${CYAN}http://${PUBLIC_IP}:${nc_port}${NC}"
                    echo -e "  访问密钥: ${YELLOW}${nc_token}${NC}"
                else
                    echo -e "  ${YELLOW}未找到 NapCat 配置文件 (未安装或未启动)${NC}"
                fi

                draw_line

                # --- Part C: LLBot ---
                echo -e "${PURPLE}● LuckyLilliaBot WebUI (CLI)${NC}"
                local llbot_cfg_file="$LLBOT_DIR/bin/llbot/default_config.json"
                local llbot_token_file
                local llbot_token=""
                llbot_token_file=$(get_llbot_webui_token_file "$LLBOT_DIR")
                if [[ -f "$llbot_cfg_file" ]]; then
                    local llbot_webui_host=""
                    local llbot_webui_port="3080"
                    local llbot_webui_display_host=""

                    if command -v jq &>/dev/null; then
                        llbot_webui_host=$(jq -r '.webui.host // "0.0.0.0"' "$llbot_cfg_file" 2>/dev/null)
                        llbot_webui_port=$(jq -r '.webui.port // 3080' "$llbot_cfg_file" 2>/dev/null)
                    fi

                    [[ -z "$llbot_webui_host" ]] && llbot_webui_host="0.0.0.0"
                    llbot_webui_display_host="$llbot_webui_host"
                    [[ "$llbot_webui_display_host" == "0.0.0.0" ]] && llbot_webui_display_host="$PUBLIC_IP"
                    llbot_token=$(read_llbot_webui_token "$LLBOT_DIR")
                    [[ -z "$llbot_token" ]] && llbot_token="(未设置，可在 LLBot 管理菜单中创建)"

                    echo -e "  WebUI: ${CYAN}http://${llbot_webui_display_host}:${llbot_webui_port}${NC}"
                    echo -e "  WebUI 密码: ${YELLOW}${llbot_token}${NC}"
                elif [[ -d "$LLBOT_DIR" ]]; then
                    echo -e "  ${YELLOW}尚未找到 LLBot 默认配置文件，请先完成安装。${NC}"
                else
                    echo -e "  ${YELLOW}未检测到 LuckyLilliaBot 安装目录${NC}"
                fi
                
                echo ""
                read -p "按回车返回..."
                ;;
            2)
                initialize_maibot_access_config
                ;;
            3)
                modify_adapter_config
                ;;
            0) return ;;
        esac
    done
}

initialize_maibot_access_config() {
    if ! load_config; then
        log_error "未找到配置"
        sleep 2
        return
    fi

    local bot_cfg="$MAI_PATH/MaiBot/config/bot_config.toml"
    local adapter_dir="$MAI_PATH/MaiBot/plugins/MaiBot-Napcat-Adapter"
    local adapter_cfg="$adapter_dir/config.toml"

    draw_header
    echo -e "${BLUE}▶ 初始化MaiBot访问配置${NC}"
    draw_line

    # a. 修改 MaiBot WebUI host
    if [[ ! -f "$bot_cfg" ]]; then
        echo -e "${YELLOW}MaiBot暂未完成初始化，请启动一次MaiBot并同意用户许可协议(EULA)${NC}"
    else
        python3 - <<EOF
import re
path = "$bot_cfg"
with open(path, "r", encoding="utf-8") as f:
    lines = f.readlines()

webui_start = None
webui_end = len(lines)
for i, line in enumerate(lines):
    if re.match(r'^\s*\[webui\]\s*$', line):
        webui_start = i
        break

if webui_start is None:
    print("未找到 [webui] 段，已跳过 MaiBot WebUI host 初始化。")
else:
    for j in range(webui_start + 1, len(lines)):
        if re.match(r'^\s*\[.+\]\s*$', lines[j]):
            webui_end = j
            break

    host_found = False
    changed = False
    for k in range(webui_start + 1, webui_end):
        m = re.match(r'^(\s*host\s*=\s*)([^#\n]*)(\s*(#.*)?)$', lines[k])
        if m:
            host_found = True
            current = m.group(2).strip().strip('"').strip("'")
            if current != "0.0.0.0":
                lines[k] = f'{m.group(1)}"0.0.0.0"{m.group(3)}\n'
                changed = True
            break

    if not host_found:
        lines.insert(webui_start + 1, 'host = "0.0.0.0"\n')
        changed = True

    if changed:
        with open(path, "w", encoding="utf-8") as f:
            f.writelines(lines)
        print("已完成 MaiBot WebUI host 初始化: 0.0.0.0")
    else:
        print("MaiBot WebUI host 已是 0.0.0.0，无需修改")
EOF
    fi

    draw_line

    # b. 修改 Adapter 配置
    if [[ ! -d "$adapter_dir" ]]; then
        echo -e "${YELLOW}未找到MaiBot-Napcat-Adapter插件，请重新执行一次安装程序以安装${NC}"
    elif [[ ! -f "$adapter_cfg" ]]; then
        echo -e "${YELLOW}MaiBot-Napcat-Adapter插件暂未完成初始化，请启动一次MaiBot并同意用户许可协议(EULA)${NC}"
    else
        python3 - <<EOF
import re
path = "$adapter_cfg"
with open(path, "r", encoding="utf-8") as f:
    lines = f.readlines()

def find_section(name):
    start = None
    end = len(lines)
    for i, line in enumerate(lines):
        if re.match(r'^\s*\[' + re.escape(name) + r'\]\s*$', line):
            start = i
            break
    if start is None:
        return None, None
    for j in range(start + 1, len(lines)):
        if re.match(r'^\s*\[.+\]\s*$', lines[j]):
            end = j
            break
    return start, end

def set_key(section, key, value_text):
    s, e = find_section(section)
    if s is None:
        return False, False
    found = False
    changed = False
    for i in range(s + 1, e):
        m = re.match(r'^(\s*' + re.escape(key) + r'\s*=\s*)([^#\n]*)(\s*(#.*)?)$', lines[i])
        if m:
            found = True
            old = m.group(2).strip()
            if old != value_text:
                lines[i] = f'{m.group(1)}{value_text}{m.group(3)}\n'
                changed = True
            break
    if not found:
        lines.insert(s + 1, f'{key} = {value_text}\n')
        changed = True
    return True, changed

ok1, ch1 = set_key("plugin", "enabled", "true")

if not ok1:
    print("未找到 [plugin] 段，已跳过 enabled 初始化。")

if ch1:
    with open(path, "w", encoding="utf-8") as f:
        f.writelines(lines)
    print("已完成 Adapter 访问配置初始化（enabled=true）")
else:
    print("Adapter enabled 已是 true，无需修改")
EOF
    fi

    draw_line
    echo -e "${YELLOW}以上所有修改完成后，请手动前往主菜单 [2. 管理 MaiBot 核心] 重新启动一次MaiBot。${NC}"
    read -p "按回车返回..."
}

modify_adapter_config() {
    if ! load_config; then
        log_error "未找到配置"
        sleep 2
        return
    fi

    local adapter_dir="$MAI_PATH/MaiBot/plugins/MaiBot-Napcat-Adapter"
    local config_file="$adapter_dir/config.toml"

    if [[ ! -d "$adapter_dir" ]]; then
        log_error "未找到MaiBot-Napcat-Adapter插件，请重新执行一次安装程序以安装"
        sleep 2
        return
    fi
    if [[ ! -f "$config_file" ]]; then
        log_error "MaiBot-Napcat-Adapter插件暂未完成初始化，请启动一次MaiBot并同意用户许可协议(EULA)"
        sleep 2
        return
    fi

    while true; do
        draw_header
        echo -e "${BLUE}▶ Adapter 黑白名单管理${NC}"
        python3 - <<EOF
import re
try:
    with open("$config_file", "r", encoding="utf-8") as f:
        content = f.read()

    def find_val(key):
        m = re.search(r'^\\s*' + re.escape(key) + r'\\s*=\\s*"(.*?)"', content, re.MULTILINE)
        return m.group(1) if m else "Unknown"

    def find_list(key):
        m = re.search(r'^\\s*' + re.escape(key) + r'\\s*=\\s*\\[(.*?)\\]', content, re.MULTILINE | re.DOTALL)
        if not m:
            return "Not Found"
        return m.group(1).replace("\\n", "").strip()

    print(f" 群聊模式: \\033[1;36m{find_val('group_list_type')}\\033[0m")
    print(f" 群聊列表: \\033[1;33m[{find_list('group_list')}]\\033[0m")
    print(f" 私聊模式: \\033[1;36m{find_val('private_list_type')}\\033[0m")
    print(f" 私聊列表: \\033[1;33m[{find_list('private_list')}]\\033[0m")
    print(f" 封禁QQ : \\033[1;33m[{find_list('ban_user_id')}]\\033[0m")
except Exception as e:
    print(f"读取配置出错: {e}")
EOF
        draw_line
        echo -e "${GREEN}1.${NC} 切换群聊名单类型 (白名单/黑名单)"
        echo -e "${GREEN}2.${NC} 添加群号到 群聊列表"
        echo -e "${GREEN}3.${NC} 从 群聊列表 移除群号"
        echo -e "${GREEN}4.${NC} 切换私聊名单类型 (白名单/黑名单)"
        echo -e "${GREEN}5.${NC} 添加QQ到 私聊列表"
        echo -e "${GREEN}6.${NC} 从 私聊列表 移除QQ"
        echo -e "${GREEN}7.${NC} 添加QQ到 用户黑名单"
        echo -e "${GREEN}8.${NC} 从 私聊列表 移除QQ"
        echo -e "${WHITE}0.${NC} 返回"
        echo -e ""
        read -p " 请选择操作: " m_opt

        if [[ "$m_opt" == "0" ]]; then
            return
        fi

        local action=""
        local key=""
        local input_val=""

        case "$m_opt" in
            1) action="toggle"; key="group_list_type" ;;
            2) action="add"; key="group_list" ;;
            3) action="del"; key="group_list" ;;
            4) action="toggle"; key="private_list_type" ;;
            5) action="add"; key="private_list" ;;
            6) action="del"; key="private_list" ;;
            7) action="add"; key="ban_user_id" ;;
            8) action="del"; key="ban_user_id" ;;
            *) continue ;;
        esac

        if [[ "$action" == "add" || "$action" == "del" ]]; then
            read -p "请输入号码: " input_val
            [[ -z "$input_val" ]] && continue
            if [[ ! "$input_val" =~ ^[0-9]+$ ]]; then
                log_warning "号码必须为纯数字"
                sleep 1
                continue
            fi
        fi

        python3 - <<EOF
import re
import sys

file_path = "$config_file"
action = "$action"
key = "$key"
val = "$input_val"

try:
    with open(file_path, "r", encoding="utf-8") as f:
        content = f.read()

    if action == "toggle":
        pattern = r'(^\\s*' + re.escape(key) + r'\\s*=\\s*")(\\w+)(".*)$'
        m = re.search(pattern, content, re.MULTILINE)
        if not m:
            print("未找到配置项，操作中止")
            sys.exit(0)
        old = m.group(2)
        new = "blacklist" if old == "whitelist" else "whitelist"
        new_line = f'{m.group(1)}{new}{m.group(3)}'
        content = re.sub(pattern, new_line, content, count=1, flags=re.MULTILINE)
        with open(file_path, "w", encoding="utf-8") as f:
            f.write(content)
        print(f"已切换 {key}: {old} -> {new}")
    else:
        pattern = r'(^\\s*' + re.escape(key) + r'\\s*=\\s*\\[)(.*?)(\\]\\s*(#.*)?$)'
        m = re.search(pattern, content, re.MULTILINE | re.DOTALL)
        if not m:
            print("未找到配置项，操作中止")
            sys.exit(0)

        nums = re.findall(r'\\d+', m.group(2))
        target = str(val)

        if action == "add":
            if target in nums:
                print(f"号码 {target} 已存在")
            else:
                nums.append(target)
                print(f"已添加 {target}")
        elif action == "del":
            if target in nums:
                nums = [n for n in nums if n != target]
                print(f"已移除 {target}")
            else:
                print(f"号码 {target} 不在列表中")

        new_list = ", ".join(nums)
        new_block = f"{m.group(1)}{new_list}{m.group(3)}"
        content = content.replace(m.group(0), new_block, 1)

        with open(file_path, "w", encoding="utf-8") as f:
            f.write(content)

except Exception as e:
    print(f"修改失败: {e}")
EOF
        read -p "按回车继续..."
    done
}

# =========================================================
# 6. 服务管理菜单
# =========================================================

manage_napcat_menu() {
    local NAPCAT_DIR="$MAI_PATH/NapCat"
    check_docker_status() {
        if command -v docker &> /dev/null; then
            if docker ps --format '{{.Names}}' | grep -q "napcat"; then
                echo -e " NapCat 状态:   ${GREEN}● 运行中 (Docker)${NC}"
            else
                echo -e " NapCat 状态:   ${RED}○ 未运行 / 停止${NC}"
            fi
        else
            echo -e " NapCat 状态:   ${YELLOW}未知 (未安装Docker)${NC}"
        fi
    }

    while true; do
        draw_header
        echo -e "${BLUE}▶ NapCat 管理面板${NC}"
        echo -e " 目录: ${CYAN}$NAPCAT_DIR${NC}"
        check_docker_status
        draw_line
        echo -e "${GREEN}1.${NC} 启动 NapCat"
        echo -e "${GREEN}2.${NC} 停止 NapCat"
        echo -e "${GREEN}3.${NC} 重启 NapCat"
        echo -e "${GREEN}4.${NC} 查看实时日志 ${WHITE}(Ctrl+C 退出)${NC}"
        draw_line
        echo -e "${YELLOW}5.${NC} 重建容器 ${WHITE}(更新/修复，保留配置)${NC}"
        echo -e "${RED}6. 移除容器 ${WHITE}(删除容器与配置，慎用！)${NC}"
        echo -e "${RED}7.${NC} 移除现有 NapCat 容器 ${WHITE}(删除所有名称以 napcat 开头的容器)${NC}"
        draw_line
        echo -e "${WHITE}0.${NC} 返回上一级"
        echo -e ""
        
        if [[ ! -d "$NAPCAT_DIR" ]]; then
            echo -e "${YELLOW}提示: 未检测到 NapCat 目录，此面板仅支持 Docker 版管理。${NC}"
        fi

        read -p " 请选择: " nc_opt
        if [[ "$nc_opt" =~ [1-5] ]]; then
            if [[ -d "$NAPCAT_DIR" ]]; then cd "$NAPCAT_DIR" || return; else log_error "找不到目录"; sleep 2; continue; fi
        fi

        case $nc_opt in
            1) docker compose up -d; sleep 1 ;;
            2) docker compose stop; sleep 1 ;;
            3) docker compose restart; sleep 1 ;;
            4) docker compose logs -f --tail=100 ;;
            5)
                echo -e "${YELLOW}警告: 即将停止并移除旧容器，拉取新镜像并重新启动。${NC}"
                read -p "确认执行? (y/n): " rebuild_confirm
                if [[ "$rebuild_confirm" == "y" ]]; then
                    docker compose down; docker compose pull; docker compose up -d
                    log_success "重建完成！"
                fi; sleep 2 ;;
            6)
                echo -e "${RED}严重警告: 此操作将删除 NapCat 容器及所有数据！${NC}"
                read -p "请输入 'DELETE' 确认删除: " del_confirm
                if [[ "$del_confirm" == "DELETE" ]]; then
                    if [[ -d "$NAPCAT_DIR" ]]; then cd "$NAPCAT_DIR"; docker compose down 2>/dev/null; cd ..; rm -rf "$NAPCAT_DIR"; log_success "已移除。"; fi
                fi; sleep 2 ;;
            7)
                echo -e "${RED}警告: 此操作将删除所有名称以 napcat 开头的容器！${NC}"
                read -p "确认执行? (y/n): " rm_nc_confirm
                if [[ "$rm_nc_confirm" == "y" || "$rm_nc_confirm" == "Y" ]]; then
                    remove_existing_napcat_containers
                fi
                sleep 2 ;;
            0) return ;;
        esac
        if [[ "$nc_opt" =~ [1-3] ]]; then read -p "操作完成，按回车继续..."; fi
    done
}

get_llbot_webui_token_file() {
    local llbot_dir="$1"
    echo "$llbot_dir/bin/llbot/data/webui_token.txt"
}

read_llbot_webui_token() {
    local llbot_dir="$1"
    local token_file
    token_file=$(get_llbot_webui_token_file "$llbot_dir")
    if [[ -f "$token_file" ]]; then
        sed -n '1p' "$token_file"
    fi
}

manage_llbot_menu() {
    if ! load_config; then
        log_error "未找到安装配置，请先执行安装。"
        return
    fi
    if ! check_screen_installed; then
        log_error "请先安装 screen"
        return
    fi

    local LLBOT_DIR="${MAI_LLBOT_PATH:-$MAI_PATH/LLBot}"
    local SCREEN_NAME="llbot"
    local LLBOT_TOKEN_FILE
    LLBOT_TOKEN_FILE=$(get_llbot_webui_token_file "$LLBOT_DIR")

    check_llbot_status() {
        if screen -list | grep -q "\\.${SCREEN_NAME}[[:space:]]"; then
            echo -e " LuckyLilliaBot: ${GREEN}● 运行中 (Screen)${NC}"
        else
            echo -e " LuckyLilliaBot: ${RED}○ 未运行${NC}"
        fi
    }

    update_llbot_password() {
        local new_password=""
        echo -e "${BLUE}▶ 修改 LuckyLilliaBot WebUI 密码${NC}"
        echo -e " 密码文件: ${CYAN}${LLBOT_TOKEN_FILE}${NC}"
        read -p "请输入新的 WebUI 密码: " new_password
        if [[ -z "$new_password" ]]; then
            log_warning "密码不能为空，已取消。"
            return
        fi

        mkdir -p "$(dirname "$LLBOT_TOKEN_FILE")"
        printf '%s\n' "$new_password" > "$LLBOT_TOKEN_FILE"
        chmod 600 "$LLBOT_TOKEN_FILE" 2>/dev/null || true
        log_success "LuckyLilliaBot WebUI 密码已更新。"
        echo -e "${YELLOW}如 LLBot 正在运行，建议重启一次以确保新密码立即生效。${NC}"
    }

    start_llbot() {
        if [[ ! -d "$LLBOT_DIR" ]]; then
            log_error "未找到 LuckyLilliaBot 安装目录: $LLBOT_DIR"
            return
        fi
        if [[ ! -f "$LLBOT_DIR/start.sh" ]]; then
            log_error "未找到启动脚本: $LLBOT_DIR/start.sh"
            return
        fi

        cd "$LLBOT_DIR" || return
        screen -list | grep -q "\\.${SCREEN_NAME}[[:space:]]" && screen -S "$SCREEN_NAME" -X quit
        screen -dmS "$SCREEN_NAME" bash -c "cd '$LLBOT_DIR'; chmod +x '$LLBOT_DIR/start.sh' '$LLBOT_DIR/llbot' 2>/dev/null || true; echo -e '${GREEN}LuckyLilliaBot 启动中...${NC}'; '$LLBOT_DIR/start.sh'; echo -e '${RED}LuckyLilliaBot 已停止/崩溃。${NC}'; exec bash"
        sleep 1
        log_success "LuckyLilliaBot 已在后台启动"
        echo -e "如需扫码或查看输出，请进入 Screen 会话。"
    }

    stop_llbot() {
        if screen -list | grep -q "\\.${SCREEN_NAME}[[:space:]]"; then
            screen -S "$SCREEN_NAME" -X quit
            log_success "已停止 LuckyLilliaBot"
        else
            echo -e "LuckyLilliaBot 未运行"
        fi
    }

    while true; do
        draw_header
        echo -e "${BLUE}▶ LuckyLilliaBot 管理面板${NC}"
        echo -e "  目录: ${CYAN}$LLBOT_DIR${NC}"
        check_llbot_status
        draw_line
        echo -e "${GREEN}1.${NC} 启动 LuckyLilliaBot"
        echo -e "${GREEN}2.${NC} 停止 LuckyLilliaBot"
        echo -e "${GREEN}3.${NC} 重启 LuckyLilliaBot"
        echo -e "${YELLOW}4.${NC} 进入 Screen 控制台 ${WHITE}(扫码 / 查看日志 / 调试)${NC}"
        echo -e "${GREEN}5.${NC} 修改 WebUI 密码"
        draw_line
        echo -e "${RED}6.${NC} 删除 LuckyLilliaBot 安装目录 ${WHITE}(慎用！)${NC}"
        draw_line
        echo -e "${WHITE}0.${NC} 返回上一级"
        echo -e ""
        read -p " 请选择: " ll_opt

        case $ll_opt in
            1) start_llbot; read -p "操作完成，按回车继续..." ;;
            2) stop_llbot; read -p "操作完成，按回车继续..." ;;
            3)
                stop_llbot
                start_llbot
                read -p "操作完成，按回车继续..."
                ;;
            4)
                if screen -list | grep -q "\\.${SCREEN_NAME}[[:space:]]"; then
                    confirm_and_enter_screen "$SCREEN_NAME" "已取消进入 Screen 窗口。LuckyLilliaBot 仍在后台运行。"
                else
                    log_warning "LuckyLilliaBot 当前未运行，请先启动。"
                    sleep 2
                fi
                ;;
            5)
                update_llbot_password
                read -p "操作完成，按回车继续..."
                ;;
            6)
                echo -e "${RED}严重警告: 此操作将删除 LuckyLilliaBot 安装目录及其数据！${NC}"
                read -p "请输入 'DELETE' 确认删除: " ll_del_confirm
                if [[ "$ll_del_confirm" == "DELETE" ]]; then
                    stop_llbot
                    rm -rf "$LLBOT_DIR"
                    save_config "$MAI_PATH" "${MAI_PYTHON_ENV:-system}" ""
                    log_success "LuckyLilliaBot 已移除。"
                fi
                read -p "操作完成，按回车继续..."
                ;;
            0) return ;;
        esac
    done
}

manage_bot_protocol_menu() {
    if ! load_config; then
        log_error "未找到安装配置，请先执行安装。"
        return
    fi

    while true; do
        draw_header
        echo -e "${BLUE}▶ Bot 协议端服务管理${NC}"
        draw_line
        echo -e "${GREEN}1.${NC} 管理 NapCatQQ"
        echo -e "${GREEN}2.${NC} 管理 LuckyLilliaBot"
        draw_line
        echo -e "${WHITE}0.${NC} 返回主菜单"
        echo -e ""
        read -p " 请选择协议端: " protocol_choice
        case $protocol_choice in
            1) manage_napcat_menu ;;
            2) manage_llbot_menu ;;
            0) return ;;
        esac
    done
}

manage_maibot_menu() {
    if ! load_config; then
        echo -e "${RED}未找到安装记录${NC}"; echo "请输入 MaiBot 的安装目录:"; read -p "> " manual_path
        if [[ -d "$manual_path" ]]; then save_config "$manual_path"; load_config; else log_error "目录不存在"; return; fi
    fi
    if ! check_screen_installed; then log_error "请先安装 screen"; return; fi

    local MAIBOT_DIR="$MAI_PATH/MaiBot"
    local VENV_PATH="$MAI_PATH/venv/bin/activate"
    local PY_ENV_MODE="${MAI_PYTHON_ENV:-system}"
    local SCREEN_NAME="maibot"

    start_maibot() {
        if [[ ! -d "$MAIBOT_DIR" ]]; then log_warning "MaiBot 目录不存在"; return; fi

        echo -e "${YELLOW}⚠️  启动提示 ⚠️${NC}"
        echo -e "如果是首次启动 MaiBot，你需要同意 ${CYAN}用户协议(EULA)${NC}。"
        echo -e "1. 正常后台启动"
        echo -e "2. 启动并进入控制台 (首次运行选这个)"
        read -p "请选择 [1/2]: " run_mode

        cd "$MAIBOT_DIR" || return
        screen -list | grep -q "$SCREEN_NAME" && screen -S "$SCREEN_NAME" -X quit

        if [[ "$PY_ENV_MODE" == "uv" ]]; then
            ensure_uv_installed || return
            screen -dmS "$SCREEN_NAME" bash -c "cd '$MAIBOT_DIR'; echo -e '${GREEN}MaiBot 启动中 (uv)...${NC}'; uv run bot.py; echo -e '${RED}MaiBot 已停止/崩溃。${NC}'; exec bash"
        else
            screen -dmS "$SCREEN_NAME" bash -c "source '$VENV_PATH'; echo -e '${GREEN}MaiBot 启动中...${NC}'; python3 bot.py; echo -e '${RED}MaiBot 已停止/崩溃。${NC}'; exec bash"
        fi
        sleep 1

        if [[ "$run_mode" == "2" ]]; then
            confirm_and_enter_screen "$SCREEN_NAME" "已取消进入 Screen 窗口。MaiBot 仍在后台运行。"
        else
            log_success "MaiBot 已在后台启动"
        fi
    }

    stop_maibot() {
        if screen -list | grep -q "$SCREEN_NAME"; then
            screen -S "$SCREEN_NAME" -X quit
            log_success "已停止 MaiBot"
        else
            echo -e "MaiBot 未运行"
        fi
    }

    check_maibot_status() {
        if screen -list | grep -q "$SCREEN_NAME"; then
            echo -e " MaiBot(本体):\t${GREEN}● 运行中${NC}"
        else
            echo -e " MaiBot(本体):\t${RED}○ 未运行${NC}"
        fi
    }

    while true; do
        draw_header
        echo -e "${BLUE}▶ MaiBot 核心管理${NC}"
        check_maibot_status
        draw_line
        echo -e "${GREEN}1.${NC} 启动 MaiBot 本体"
        echo -e "${GREEN}2.${NC} 停止 MaiBot 本体"
        echo -e "${YELLOW}3.${NC} 进入 Screen 控制台 ${WHITE}(查看报错)${NC}"
        draw_line
        echo -e "${WHITE}0.${NC} 返回主菜单"
        echo -e ""
        read -p " 请选择: " m_choice
        case $m_choice in
            1) start_maibot ;;
            2) stop_maibot ;;
            3)
                confirm_and_enter_screen "$SCREEN_NAME"
                ;;
            0) return ;;
        esac
        if [[ "$m_choice" != "3" && "$m_choice" != "0" ]]; then read -p "操作已执行，按回车继续..."; fi
    done
}




# =========================================================
# 7. LPMM知识库菜单
# =========================================================

manage_lpmm_menu() {
    # 加载配置，获取安装路径
    if ! load_config; then
        log_error "未找到安装配置，请先执行安装（主菜单选项1）"
        read -p "按回车返回主菜单..."
        return
    fi

    local MAIBOT_DIR="$MAI_PATH/MaiBot"
    local DATA_DIR="$MAIBOT_DIR/data"
    local RAW_DIR="$DATA_DIR/lpmm_raw_data"
    local OPENIE_DIR="$DATA_DIR/openie"
    local VENV_PATH="$MAI_PATH/venv/bin/activate"
    local PY_ENV_MODE="${MAI_PYTHON_ENV:-system}"
    local SCRIPT_INFO="$MAIBOT_DIR/scripts/info_extraction.py"
    local SCRIPT_IMPORT="$MAIBOT_DIR/scripts/import_openie.py"
    local SCRIPT_INFO_REL="scripts/info_extraction.py"
    local SCRIPT_IMPORT_REL="scripts/import_openie.py"

    # 检查必要目录和脚本是否存在
    if [[ ! -d "$MAIBOT_DIR" ]]; then
        log_error "MaiBot 目录不存在: $MAIBOT_DIR"
        read -p "按回车返回..."
        return
    fi

    if [[ ! -f "$SCRIPT_INFO" ]]; then
        log_warning "未找到 info_extraction.py，可能版本不完整"
    fi
    if [[ ! -f "$SCRIPT_IMPORT" ]]; then
        log_warning "未找到 import_openie.py，可能版本不完整"
    fi

    # 检查 screen 是否安装
    local SCREEN_AVAILABLE=false
    if command -v screen &>/dev/null; then
        SCREEN_AVAILABLE=true
    else
        log_warning "未安装 screen，后台运行功能无法使用"
    fi

    while true; do
        draw_header
        echo -e "${BLUE}▶ LPMM 知识库管理${NC}"
        echo -e " 数据目录: ${CYAN}$DATA_DIR${NC}"
        echo -e "${YELLOW} 知识库管理暂不可用，请等待官方更新！${NC}"

        # 后台任务状态显示
        local info_status import_status
        if [[ "$SCREEN_AVAILABLE" == true ]]; then
            if screen -list | grep -q "mai-lpmm-info"; then
                info_status="${GREEN}● 运行中${NC}"
            else
                info_status="${RED}○ 未运行${NC}"
            fi
            if screen -list | grep -q "mai-lpmm-import"; then
                import_status="${GREEN}● 运行中${NC}"
            else
                import_status="${RED}○ 未运行${NC}"
            fi
        else
            info_status="${YELLOW}不可用${NC}"
            import_status="${YELLOW}不可用${NC}"
        fi
        echo -e " 文本分割与实体提取: ${info_status}"
        echo -e " 知识库导入: ${import_status}"

        draw_line
        echo -e "${GREEN}1.${NC} 初始化LPMM知识库 ${WHITE}(创建目录结构)${NC}"
        echo -e "${GREEN}2.${NC} 文本分割与实体提取 ${WHITE}(前台运行)${NC}"
        echo -e "${GREEN}3.${NC} 文本分割与实体提取 ${WHITE}(后台运行 - screen)${NC}"
        echo -e "${RED}4.${NC} 关闭后台文本分割与实体提取"
        draw_line
        echo -e "${GREEN}5.${NC} 导入LPMM知识库 ${WHITE}(前台运行)${NC}"
        echo -e "${GREEN}6.${NC} 导入LPMM知识库 ${WHITE}(后台运行 - screen)${NC}"
        echo -e "${RED}7.${NC} 关闭后台LPMM知识库导入"
        draw_line
        echo -e "${WHITE}0.${NC} 返回主菜单"
        echo -e ""
        read -p " 请选择: " lpmm_opt

        case $lpmm_opt in
            1)
                draw_header
                echo -e "${BLUE}▶ 初始化LPMM知识库目录${NC}"
                mkdir -p "$RAW_DIR" "$OPENIE_DIR"
                log_success "目录创建完成"
                echo -e "请将知识库 ${YELLOW}txt文件${NC} 放入: ${CYAN}$RAW_DIR${NC}"
                echo -e "将已经提取好的 ${YELLOW}openie文件${NC} 放入: ${CYAN}$OPENIE_DIR${NC}"
                echo -e "${WHITE}提示：${NC}需要先进行文本分割与实体提取（选项2/3），再导入LPMM知识库（选项5/6）"
                read -p "按回车继续..."
                ;;
            2)
                if [[ ! -f "$SCRIPT_INFO" ]]; then
                    log_error "脚本文件不存在: $SCRIPT_INFO"
                    read -p "按回车继续..."
                    continue
                fi
                draw_header
                echo -e "${BLUE}▶ 文本分割与实体提取 (前台)${NC}"
                echo -e "${YELLOW}注意：此过程可能耗时较长，请勿关闭终端窗口！${NC}"
                if [[ "$PY_ENV_MODE" == "uv" ]]; then
                    echo -e "正在使用 uv 运行脚本...\n"
                    cd "$MAIBOT_DIR" && uv run "$SCRIPT_INFO_REL"
                else
                    echo -e "正在激活虚拟环境并运行脚本...\n"
                    bash -c "source '$VENV_PATH' && python '$SCRIPT_INFO'"
                fi
                echo -e "\n${GREEN}脚本执行完毕。${NC}"
                read -p "按回车继续..."
                ;;
            3)
                if [[ "$SCREEN_AVAILABLE" == false ]]; then
                    log_error "screen 未安装，无法使用后台功能"
                    read -p "按回车继续..."
                    continue
                fi
                if [[ ! -f "$SCRIPT_INFO" ]]; then
                    log_error "脚本文件不存在: $SCRIPT_INFO"
                    read -p "按回车继续..."
                    continue
                fi
                draw_header
                echo -e "${BLUE}▶ 文本分割与实体提取 (后台)${NC}"
                if screen -list | grep -q "mai-lpmm-info"; then
                    echo -e "${YELLOW}后台任务已存在，会话名: mai-lpmm-info${NC}"
                    echo -e "1. 进入该会话查看 (screen -r)"
                    echo -e "2. 关闭现有并重新启动"
                    echo -e "3. 返回"
                    read -p "请选择 [1-3]: " exist_opt
                    case $exist_opt in
                        1)
                            echo -e "进入 screen 会话，退出请按 ${WHITE}Ctrl+A 然后按下 D${NC}"
                            sleep 1
                            screen -r mai-lpmm-info
                            ;;
                        2)
                            screen -S mai-lpmm-info -X quit 2>/dev/null
                            log_info "已关闭旧会话，重新启动..."
                            if [[ "$PY_ENV_MODE" == "uv" ]]; then
                                ensure_uv_installed || { read -p "按回车继续..."; continue; }
                                screen -dmS mai-lpmm-info bash -c "cd '$MAIBOT_DIR'; uv run '$SCRIPT_INFO_REL'"
                            else
                                screen -dmS mai-lpmm-info bash -c "source '$VENV_PATH'; python '$SCRIPT_INFO'"
                            fi
                            log_success "已在后台启动，会话名: mai-lpmm-info"
                            echo -e "查看进度: ${CYAN}screen -r mai-lpmm-info${NC}"
                            echo -e "退出 screen: ${WHITE}Ctrl+A 然后按下 D${NC}"
                            read -p "是否立即进入该 screen 会话？(y/n): " enter_now
                            if [[ "$enter_now" == "y" || "$enter_now" == "Y" ]]; then
                                screen -r mai-lpmm-info
                            fi
                            ;;
                        3) ;;
                    esac
                else
                    if [[ "$PY_ENV_MODE" == "uv" ]]; then
                        ensure_uv_installed || { read -p "按回车继续..."; continue; }
                        screen -dmS mai-lpmm-info bash -c "cd '$MAIBOT_DIR'; uv run '$SCRIPT_INFO_REL'"
                    else
                        screen -dmS mai-lpmm-info bash -c "source '$VENV_PATH'; python '$SCRIPT_INFO'"
                    fi
                    log_success "已在后台启动，会话名: mai-lpmm-info"
                    echo -e "查看进度: ${CYAN}screen -r mai-lpmm-info${NC}"
                    echo -e "退出 screen: ${WHITE}Ctrl+A 按下 D${NC}"
                    read -p "是否立即进入该 screen 会话？(y/n): " enter_now
                    if [[ "$enter_now" == "y" || "$enter_now" == "Y" ]]; then
                        screen -r mai-lpmm-info
                    fi
                fi
                read -p "按回车继续..."
                ;;
            4)
                draw_header
                echo -e "${BLUE}▶ 关闭后台文本分割与实体提取${NC}"
                if screen -list | grep -q "mai-lpmm-info"; then
                    screen -S mai-lpmm-info -X quit
                    log_success "已关闭会话 mai-lpmm-info"
                else
                    log_info "没有正在运行的后台任务 (mai-lpmm-info)"
                fi
                read -p "按回车继续..."
                ;;
            5)
                if [[ ! -f "$SCRIPT_IMPORT" ]]; then
                    log_error "脚本文件不存在: $SCRIPT_IMPORT"
                    read -p "按回车继续..."
                    continue
                fi
                draw_header
                echo -e "${BLUE}▶ 导入LPMM知识库 (前台)${NC}"
                echo -e "${YELLOW}注意：此过程可能耗时较长，请勿关闭终端窗口！${NC}"
                if [[ "$PY_ENV_MODE" == "uv" ]]; then
                    echo -e "正在使用 uv 运行脚本...\n"
                    cd "$MAIBOT_DIR" && uv run "$SCRIPT_IMPORT_REL"
                else
                    echo -e "正在激活虚拟环境并运行脚本...\n"
                    bash -c "source '$VENV_PATH' && python '$SCRIPT_IMPORT'"
                fi
                echo -e "\n${GREEN}脚本执行完毕。${NC}"
                read -p "按回车继续..."
                ;;
            6)
                if [[ "$SCREEN_AVAILABLE" == false ]]; then
                    log_error "screen 未安装，无法使用后台功能"
                    read -p "按回车继续..."
                    continue
                fi
                if [[ ! -f "$SCRIPT_IMPORT" ]]; then
                    log_error "脚本文件不存在: $SCRIPT_IMPORT"
                    read -p "按回车继续..."
                    continue
                fi
                draw_header
                echo -e "${BLUE}▶ 导入LPMM知识库 (后台)${NC}"
                if screen -list | grep -q "mai-lpmm-import"; then
                    echo -e "${YELLOW}后台任务已存在，会话名: mai-lpmm-import${NC}"
                    echo -e "1. 进入该会话查看"
                    echo -e "2. 关闭现有并重新启动"
                    echo -e "3. 返回"
                    read -p "请选择 [1-3]: " exist_opt
                    case $exist_opt in
                        1)
                            echo -e "进入 screen 会话，退出请按 ${WHITE}Ctrl+A 然后按下 D${NC}"
                            sleep 1
                            screen -r mai-lpmm-import
                            ;;
                        2)
                            screen -S mai-lpmm-import -X quit 2>/dev/null
                            log_info "已关闭旧会话，重新启动..."
                            if [[ "$PY_ENV_MODE" == "uv" ]]; then
                                ensure_uv_installed || { read -p "按回车继续..."; continue; }
                                screen -dmS mai-lpmm-import bash -c "cd '$MAIBOT_DIR'; uv run '$SCRIPT_IMPORT_REL'"
                            else
                                screen -dmS mai-lpmm-import bash -c "source '$VENV_PATH'; python '$SCRIPT_IMPORT'"
                            fi
                            log_success "已在后台启动，会话名: mai-lpmm-import"
                            echo -e "查看进度: ${CYAN}screen -r mai-lpmm-import${NC}"
                            echo -e "退出 screen: ${WHITE}Ctrl+A 按下 D${NC}"
                            read -p "是否立即进入该 screen 会话？(y/n): " enter_now
                            if [[ "$enter_now" == "y" || "$enter_now" == "Y" ]]; then
                                screen -r mai-lpmm-import
                            fi
                            ;;
                        3) ;;
                    esac
                else
                    if [[ "$PY_ENV_MODE" == "uv" ]]; then
                        ensure_uv_installed || { read -p "按回车继续..."; continue; }
                        screen -dmS mai-lpmm-import bash -c "cd '$MAIBOT_DIR'; uv run '$SCRIPT_IMPORT_REL'"
                    else
                        screen -dmS mai-lpmm-import bash -c "source '$VENV_PATH'; python '$SCRIPT_IMPORT'"
                    fi
                    log_success "已在后台启动，会话名: mai-lpmm-import"
                    echo -e "查看进度: ${CYAN}screen -r mai-lpmm-import${NC}"
                    echo -e "退出 screen: ${WHITE}Ctrl+A 然后按下 D${NC}"
                    read -p "是否立即进入该 screen 会话？(y/n): " enter_now
                    if [[ "$enter_now" == "y" || "$enter_now" == "Y" ]]; then
                        screen -r mai-lpmm-import
                    fi
                fi
                read -p "按回车继续..."
                ;;
            7)
                draw_header
                echo -e "${BLUE}▶ 关闭后台LPMM知识库导入${NC}"
                if screen -list | grep -q "mai-lpmm-import"; then
                    screen -S mai-lpmm-import -X quit
                    log_success "已关闭会话 mai-lpmm-import"
                else
                    log_info "没有正在运行的后台任务 (mai-lpmm-import)"
                fi
                read -p "按回车继续..."
                ;;
            0)
                return
                ;;
            *)
                log_warning "无效选项"
                sleep 1
                ;;
        esac
    done
}

# =========================================================
# 8. 插件管理菜单
# =========================================================

manage_plugins_menu() {
    if ! load_config; then
        log_error "未找到安装配置，请先执行安装"
        read -p "按回车返回主菜单..."
        return
    fi

    local MAIBOT_DIR="$MAI_PATH/MaiBot"
    local PLUGINS_DIR="$MAIBOT_DIR/plugins"
    local VENV_PATH="$MAI_PATH/venv/bin/activate"
    local PY_ENV_MODE="${MAI_PYTHON_ENV:-system}"

    if [[ ! -d "$MAIBOT_DIR" ]]; then
        log_error "MaiBot 目录不存在: $MAIBOT_DIR"
        read -p "按回车返回..."
        return
    fi

    mkdir -p "$PLUGINS_DIR"

    while true; do
        draw_header
        echo -e "${BLUE}▶ 插件管理${NC}"
        echo -e " 插件目录: ${CYAN}$PLUGINS_DIR${NC}"
        echo -e "\n 已安装插件:"
        list_plugins_display "$PLUGINS_DIR"
        draw_line
        echo -e "${GREEN}1.${NC} 安装插件"
        echo -e "${GREEN}2.${NC} 卸载插件"
        echo -e "${GREEN}3.${NC} 安装插件依赖"
        draw_line
        echo -e "${WHITE}0.${NC} 返回主菜单"
        echo -e ""
        read -p " 请选择: " plugin_opt

        case $plugin_opt in
            1)
                draw_header
                echo -e "${BLUE}▶ 安装插件${NC}"
                echo -e "请输入插件 GitHub 地址，支持以下格式："
                echo -e " • https://github.com/username/repo"
                echo -e " • https://github.com/username/repo.git"
                echo -e " • username/repo"
                read -p "请输入: " plugin_input

                if [[ -z "$plugin_input" ]]; then
                    log_warning "输入不能为空"
                    read -p "按回车继续..."
                    continue
                fi

                # 获取 GitHub 加速源
                if [[ -z "$USER_GH_PROXY" ]]; then
                    draw_header
                    echo -e "${BLUE}▶ GitHub 线路配置${NC}"
                    choose_github_proxy
                fi

                # 规范化 URL
                local git_url=""
                if [[ "$plugin_input" =~ ^https?:// ]]; then
                    git_url=$(convert_github_url "$plugin_input")
                else
                    git_url=$(convert_github_url "https://github.com/$plugin_input")
                fi

                # 提取插件名称
                local plugin_name=$(echo "$git_url" | sed 's|.*/||' | sed 's|\.git$||')
                local plugin_path="$PLUGINS_DIR/$plugin_name"

                echo -e "\n${BLUE}插件信息:${NC}"
                echo -e " 名称: ${CYAN}$plugin_name${NC}"
                echo -e " 地址: ${CYAN}$git_url${NC}"
                echo -e " 路径: ${CYAN}$plugin_path${NC}"

                if [[ -d "$plugin_path" ]]; then
                    log_info "检测到插件已存在，尝试更新..."
                    git_update_safe "$plugin_path" "" "$git_url"
                    if [ $? -eq 0 ]; then
                        log_success "插件更新成功"
                        cd "$MAI_PATH" || continue
                    else
                        log_error "更新失败"
                        cd "$MAI_PATH" || continue
                        echo -e "${YELLOW}是否删除旧插件并重新克隆？${NC}"
                        read -p "请输入 (y/n): " re_choice
                        if [[ "$re_choice" != "y" ]]; then
                            read -p "按回车继续..."
                            continue
                        fi
                        rm -rf "$plugin_path"
                    fi
                fi

                if [[ ! -d "$plugin_path" ]]; then
                    log_info "正在克隆插件 ${CYAN}$plugin_name${NC}..."
                    git clone --depth 1 --progress "$git_url" "$plugin_path"
                    if [ $? -ne 0 ]; then
                        log_error "克隆失败！"
                        rm -rf "$plugin_path"
                        read -p "按回车继续..."
                        continue
                    fi
                fi

                # 创建 .gitignore 屏蔽 __pycache__
                if [[ ! -f "$plugin_path/.gitignore" ]]; then
                    echo "__pycache__/" > "$plugin_path/.gitignore"
                    echo "*.pyc" >> "$plugin_path/.gitignore"
                    echo ".DS_Store" >> "$plugin_path/.gitignore"
                fi

                if [[ -f "$plugin_path/requirements.txt" ]]; then
                    echo -e "\n${BLUE}▶ 安装插件依赖${NC}"
                    if [[ "$PY_ENV_MODE" == "uv" ]]; then
                        ensure_uv_installed || { read -p "按回车继续..."; continue; }
                        cd "$MAIBOT_DIR" || continue
                        uv pip install -r "$plugin_path/requirements.txt"
                    else
                        source "$VENV_PATH"
                        pip install -r "$plugin_path/requirements.txt"
                    fi

                    if [ $? -eq 0 ]; then
                        log_success "依赖安装成功"
                    else
                        log_error "依赖安装失败"
                    fi
                else
                    log_info "未找到 requirements.txt，跳过依赖安装"
                fi

                log_success "插件 ${CYAN}$plugin_name${NC} 安装完成"
                read -p "按回车继续..."
                ;;
            2)
                draw_header
                echo -e "${BLUE}▶ 卸载插件${NC}"
                if [[ ! -d "$PLUGINS_DIR" ]] || [[ -z "$(ls -A "$PLUGINS_DIR" 2>/dev/null)" ]]; then
                    log_warning "没有已安装的插件"
                    read -p "按回车继续..."
                    continue
                fi

                echo -e "已安装的插件:"
                mapfile -t plugins < <(get_plugin_names "$PLUGINS_DIR")
                local idx=1
                local plugin_name
                for plugin_name in "${plugins[@]}"; do
                    echo -e " ${GREEN}$idx.${NC} $plugin_name"
                    idx=$((idx + 1))
                done

                read -p "请输入插件名称或序号: " plugin_input
                local target_plugin=""

                if [[ "$plugin_input" =~ ^[0-9]+$ ]]; then
                    if [[ $plugin_input -ge 1 && $plugin_input -le ${#plugins[@]} ]]; then
                        target_plugin="${plugins[$((plugin_input - 1))]}"
                    fi
                else
                    target_plugin="$plugin_input"
                fi

                if [[ -z "$target_plugin" ]]; then
                    log_error "无效的选择"
                    read -p "按回车继续..."
                    continue
                fi

                local target_path="$PLUGINS_DIR/$target_plugin"
                if [[ ! -d "$target_path" ]]; then
                    log_error "插件目录不存在: $target_plugin"
                    read -p "按回车继续..."
                    continue
                fi

                echo -e "\n${YELLOW}确认删除以下内容：${NC}"
                echo -e " ${RED}$target_path${NC}"
                echo -e "\n${RED}警告：此操作不可撤销！${NC}"
                read -p "请输入插件名称确认删除: " confirm_name

                if [[ "$confirm_name" == "$target_plugin" ]]; then
                    rm -rf "$target_path"
                    log_success "插件 ${CYAN}$target_plugin${NC} 已卸载"
                else
                    log_warning "确认失败，操作已取消"
                fi
                read -p "按回车继续..."
                ;;
            3)
                draw_header
                echo -e "${BLUE}▶ 安装插件依赖${NC}"
                if [[ ! -d "$PLUGINS_DIR" ]] || [[ -z "$(ls -A "$PLUGINS_DIR" 2>/dev/null)" ]]; then
                    log_warning "没有已安装的插件"
                    read -p "按回车继续..."
                    continue
                fi

                echo -e "已安装的插件:"
                mapfile -t plugins < <(get_plugin_names "$PLUGINS_DIR")
                local idx=1
                local plugin_name
                for plugin_name in "${plugins[@]}"; do
                    echo -e " ${GREEN}$idx.${NC} $plugin_name"
                    idx=$((idx + 1))
                done

                read -p "请输入插件名称或序号: " plugin_input
                local target_plugin=""

                if [[ "$plugin_input" =~ ^[0-9]+$ ]]; then
                    if [[ $plugin_input -ge 1 && $plugin_input -le ${#plugins[@]} ]]; then
                        target_plugin="${plugins[$((plugin_input - 1))]}"
                    fi
                else
                    target_plugin="$plugin_input"
                fi

                if [[ -z "$target_plugin" ]]; then
                    log_error "无效的选择"
                    read -p "按回车继续..."
                    continue
                fi

                local target_path="$PLUGINS_DIR/$target_plugin"
                if [[ ! -d "$target_path" ]]; then
                    log_error "插件目录不存在: $target_plugin"
                    read -p "按回车继续..."
                    continue
                fi

                if [[ ! -f "$target_path/requirements.txt" ]]; then
                    log_warning "未找到 requirements.txt，建议查看插件文档是否有依赖需求"
                    read -p "按回车继续..."
                    continue
                fi

                echo -e "\n${BLUE}正在安装 ${CYAN}$target_plugin${NC} 的依赖...${NC}"
                if [[ "$PY_ENV_MODE" == "uv" ]]; then
                    ensure_uv_installed || { read -p "按回车继续..."; continue; }
                    cd "$MAIBOT_DIR" || continue
                    uv pip install -r "$target_path/requirements.txt"
                else
                    source "$VENV_PATH"
                    pip install -r "$target_path/requirements.txt"
                fi
                if [ $? -eq 0 ]; then
                    log_success "依赖安装成功"
                else
                    log_error "依赖安装失败"
                fi
                read -p "按回车继续..."
                ;;
            0)
                return
                ;;
        esac
    done
}


# =========================================================
# 9. 入口
# =========================================================

main_menu() {
    while true; do
        draw_header
        echo -e "${GREEN}1.${NC} 安装 / 更新 MaiBot ${NC}"
        draw_line
        echo -e "${PURPLE}2.${NC} 管理 MaiBot 核心${NC}"
        echo -e "${CYAN}3.${NC} 管理Bot协议端服务${NC}"
        echo -e "${BLUE}4.${NC} 配置与访问${NC}"
        echo -e "${YELLOW}5.${NC} LPMM知识库${YELLOW}(等待官方更新)${NC}"
        echo -e "${GREEN}6.${NC} 插件管理${NC}"
        draw_line
        echo -e "${WHITE}0.${NC} 退出脚本"
        echo -e ""
        read -p " 请输入选项: " choice
        
        case $choice in
            1) 
                INSTALLATION_ACTIVE=true
                configure_install_path
                step_install_mode
                step_python_env_mode
                step_venv_mode
                configure_github
                configure_pip
                configure_bot_protocol_selection
                prepare_bot_protocol_environment
                run_install
                INSTALLATION_ACTIVE=false
                ;;
            2) manage_maibot_menu ;;
            3) 
                if load_config; then manage_bot_protocol_menu
                else log_error "未找到安装配置，请先执行安装。"; read -p "按回车继续..."
                fi ;;
            4) manage_config_access_menu ;;
            5) manage_lpmm_menu ;;
            6) manage_plugins_menu ;;
            0) exit 0 ;;
        esac
    done
}


main_menu
