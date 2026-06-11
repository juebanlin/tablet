#!/bin/bash
# packaging/macos/build-app.sh — 构建 macOS .app 包和 .dmg
#
# 用法: bash packaging/macos/build-app.sh [--dmg]
#
# 前置: cargo build -p tablet-slint --release (macOS 上)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
APP_NAME="Tablet"
BINARY="tablet"
VERSION=$(grep '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')

BUILD_DIR="$ROOT_DIR/target/release"
APP_DIR="$BUILD_DIR/$APP_NAME.app"

echo "=== 构建 $APP_NAME.app v$VERSION ==="

# 检查二进制
if [ ! -f "$BUILD_DIR/$BINARY" ]; then
    echo "未找到 $BUILD_DIR/$BINARY，先编译..."
    cargo build -p tablet-slint --release
fi

# 清理旧包
rm -rf "$APP_DIR"

# 创建 .app 目录结构
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# 拷贝文件
cp "$BUILD_DIR/$BINARY" "$APP_DIR/Contents/MacOS/$BINARY"
cp "$ROOT_DIR/res/icon.icns" "$APP_DIR/Contents/Resources/icon.icns"

# 写入 Info.plist（替换版本号）
sed "s/1.2.1/$VERSION/g" "$SCRIPT_DIR/Info.plist" > "$APP_DIR/Contents/Info.plist"

echo "$APP_DIR 构建完成"

# 可选：打包 .dmg
if [[ "${1:-}" == "--dmg" ]]; then
    DMG_PATH="$BUILD_DIR/$APP_NAME-$VERSION.dmg"
    rm -f "$DMG_PATH"

    if command -v create-dmg &>/dev/null; then
        create-dmg \
            --volname "$APP_NAME" \
            --window-size 600 400 \
            --icon "$APP_NAME.app" 150 200 \
            --app-drop-link 450 200 \
            "$DMG_PATH" "$APP_DIR"
    else
        hdiutil create -volname "$APP_NAME" -srcfolder "$APP_DIR" -ov -format UDZO "$DMG_PATH"
    fi
    echo "$DMG_PATH 构建完成"
fi
