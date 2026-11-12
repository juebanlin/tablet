#!/bin/bash
# packaging/linux/build-deb.sh — 构建 Linux .deb 包
#
# 用法: bash packaging/linux/build-deb.sh
#
# 前置: cargo build -p tablet -p tablet-cli --release (Linux 上，需 WebKitGTK 4.1 开发包)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
VERSION=$(grep '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
ARCH=$(dpkg --print-architecture 2>/dev/null || echo "amd64")

PKG_NAME="tablet"
PKG_DIR="$ROOT_DIR/target/release/${PKG_NAME}_${VERSION}_${ARCH}"
BUILD_DIR="$ROOT_DIR/target/release"

echo "=== 构建 ${PKG_NAME}_${VERSION}_${ARCH}.deb ==="

# 检查二进制
if [ ! -f "$BUILD_DIR/tablet" ] || [ ! -f "$BUILD_DIR/tablet-cli" ]; then
    echo "未找到二进制，先编译..."
    cargo build -p tablet -p tablet-cli --release
fi

# 清理旧包
rm -rf "$PKG_DIR"

# 创建 deb 目录结构
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/share/applications"
mkdir -p "$PKG_DIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$PKG_DIR/usr/share/icons/hicolor/1024x1024/apps"

# 拷贝二进制
cp "$BUILD_DIR/tablet" "$PKG_DIR/usr/bin/tablet"
cp "$BUILD_DIR/tablet-cli" "$PKG_DIR/usr/bin/tablet-cli"
# 不 strip：release profile 的符号表用于崩溃日志。

# 拷贝图标
cp "$ROOT_DIR/res/icon-256.png" "$PKG_DIR/usr/share/icons/hicolor/256x256/apps/tablet.png"
cp "$ROOT_DIR/res/icon-1024.png" "$PKG_DIR/usr/share/icons/hicolor/1024x1024/apps/tablet.png"

# 拷贝 .desktop
cp "$SCRIPT_DIR/tablet.desktop" "$PKG_DIR/usr/share/applications/tablet.desktop"

# 计算安装大小
INSTALLED_SIZE=$(du -sk "$PKG_DIR" | cut -f1)

# 写入 control 文件
cat > "$PKG_DIR/DEBIAN/control" << CTRL
Package: $PKG_NAME
Version: $VERSION
Architecture: $ARCH
Maintainer: juebanlin <juebanlin@gmail.com>
Installed-Size: $INSTALLED_SIZE
Depends: libwebkit2gtk-4.1-0, libgtk-3-0, libsoup-3.0-0, libjavascriptcoregtk-4.1-0, libglib2.0-0
Section: devel
Priority: optional
Homepage: https://github.com/juebanlin/tablet
Description: TBL 配置管理工具
 游戏/应用配置表编辑工具，支持多 Project 管理、Univer 可视化编辑、
 Excel 导入/导出、12 种语言平台导出。
CTRL

# 构建 deb
dpkg-deb --build "$PKG_DIR"
echo "${PKG_DIR}.deb 构建完成"
