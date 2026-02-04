#!/bin/sh
set -e

# Maboroshi 一键安装脚本

REPO="KayneWang/maboroshi"
INSTALL_DIR="/usr/local/bin"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() {
    printf "${GREEN}[INFO]${NC} %s\n" "$1"
}

warn() {
    printf "${YELLOW}[WARN]${NC} %s\n" "$1"
}

error() {
    printf "${RED}[ERROR]${NC} %s\n" "$1"
    exit 1
}

# 检测操作系统和架构
detect_platform() {
    OS=$(uname -s)
    ARCH=$(uname -m)
    
    case "$OS" in
        Darwin)
            case "$ARCH" in
                arm64|aarch64)
                    PLATFORM="macos-aarch64"
                    ;;
                x86_64)
                    PLATFORM="macos-x86_64"
                    ;;
                *)
                    error "不支持的 macOS 架构: $ARCH"
                    ;;
            esac
            ;;
        *)
            error "目前只支持 macOS 平台"
            ;;
    esac
    
    info "检测到平台: $PLATFORM"
}

# 检查依赖
check_dependencies() {
    info "检查依赖..."
    
    if ! command -v yt-dlp >/dev/null 2>&1; then
        warn "未找到 yt-dlp，请先安装："
        echo "  brew install yt-dlp"
    fi
    
    if ! command -v mpv >/dev/null 2>&1; then
        warn "未找到 mpv，请先安装："
        echo "  brew install mpv"
    fi
}

# 下载二进制文件
download_binary() {
    info "下载 maboroshi..." >&2
    
    BINARY_NAME="maboroshi-${PLATFORM}"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}"
    
    TMP_DIR=$(mktemp -d)
    TMP_FILE="${TMP_DIR}/maboroshi"
    
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$DOWNLOAD_URL" -o "$TMP_FILE" || error "下载失败"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$DOWNLOAD_URL" -O "$TMP_FILE" || error "下载失败"
    else
        error "需要 curl 或 wget 来下载文件"
    fi
    
    chmod +x "$TMP_FILE"
    
    echo "$TMP_FILE"
}

# 安装二进制文件
install_binary() {
    BINARY_PATH=$1
    
    # 检查二进制文件是否存在
    if [ ! -f "$BINARY_PATH" ]; then
        error "二进制文件不存在: $BINARY_PATH"
    fi
    
    info "安装 maboroshi 到 $INSTALL_DIR..."
    
    # 确保安装目录存在
    if [ ! -d "$INSTALL_DIR" ]; then
        info "创建目录 $INSTALL_DIR..."
        sudo mkdir -p "$INSTALL_DIR"
    fi
    
    # 移动文件
    if [ -w "$INSTALL_DIR" ]; then
        mv "$BINARY_PATH" "$INSTALL_DIR/maboroshi" || error "安装失败"
    else
        sudo mv "$BINARY_PATH" "$INSTALL_DIR/maboroshi" || error "安装失败"
    fi
    
    # 确保文件可执行
    sudo chmod +x "$INSTALL_DIR/maboroshi"
    
    info "安装成功！"
}

# 主函数
main() {
    echo ""
    echo "🌀 Maboroshi (幻) 安装脚本"
    echo ""
    
    detect_platform
    check_dependencies
    
    BINARY_PATH=$(download_binary)
    install_binary "$BINARY_PATH"
    
    echo ""
    info "现在可以运行: maboroshi"
    echo ""
}

main
