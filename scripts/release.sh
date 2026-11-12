#!/usr/bin/env bash
# tablet 平台发布脚本（Tauri 单 GUI + CLI）
#
# 用法:
#   scripts/release.sh <target>
#
# target:
#   windows          x86_64-pc-windows-msvc        (Tier 1, 需 Windows 宿主；Tauri 要求 MSVC)
#   linux            x86_64-unknown-linux-gnu      (Tier 1, 需 Linux 宿主 + WebKitGTK 4.1)
#   linux-cli-musl   x86_64-unknown-linux-musl     (Tier 2, 仅 tablet-cli, 静态链, 任意 Linux 可跑)
#   macos            x86_64-apple-darwin + aarch64-apple-darwin → universal (需 macOS 宿主, 最低 11.0)
#   host             自动按宿主跑所有可跑目标:
#                      Windows → windows
#                      Linux   → linux + linux-cli-musl
#                      macOS   → macos
#
# 产物:
#   dist/<target>/tablet(.exe)        ← Tauri GUI（零参数 / --gui 走 GUI、其它参数转 CLI）
#   dist/<target>/tablet-cli(.exe)    ← 纯 CLI（Jenkins / 自动化批处理）
#
# 设计:
#   不做跨编译；CI matrix 的每个 runner 各编各的目标。
#   Tauri 的 beforeBuildCommand 只在 `cargo tauri build` 下执行，这里用裸 cargo build，
#   所以必须显式先跑 Univer 前端构建（npm ci && npm run build）。
#
# GitHub Actions 复用:
#   matrix:
#     - { os: windows-latest, target: windows }
#     - { os: ubuntu-22.04,  target: linux }
#     - { os: ubuntu-22.04,  target: linux-cli-musl }
#     - { os: macos-latest,  target: macos }
#   再 run: bash scripts/release.sh ${{ matrix.target }}

set -euo pipefail

# 切到仓库根
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WS_DIR"

DIST_DIR="$WS_DIR/dist"
mkdir -p "$DIST_DIR"

FRONTEND_DIR="$WS_DIR/crates/app-tauri/univer-editor"

# Tauri 的 UI 资源在编译期内嵌（frontendDist: ui），ui/univer 是构建产物且被 gitignore。
# 裸 cargo build 不会触发 tauri 的 beforeBuildCommand，缺了它运行时会提示「资源未构建」。
build_frontend() {
    echo ">>> [frontend] npm ci && npm run build (Univer)"
    (cd "$FRONTEND_DIR" && npm ci --no-audit --no-fund && npm run build)
    test -f "$WS_DIR/crates/app-tauri/ui/univer/index.html" || {
        echo "frontend 构建后仍缺少 ui/univer/index.html" >&2
        exit 1
    }
}

# ------------------------------------------------------------------
# 各 target 实现
# ------------------------------------------------------------------

build_windows() {
    local triple="x86_64-pc-windows-msvc"
    local out="$DIST_DIR/windows"
    rm -rf "$out"
    mkdir -p "$out"

    build_frontend

    # Tauri 必须 MSVC：gnu 下 WebView2Loader 是外挂 DLL，破坏单 exe。
    # 本机 rustup 默认宿主可能是 gnu，这里优先显式指定 msvc toolchain。
    local tc="stable-x86_64-pc-windows-msvc"
    local -a cargo_cmd=(cargo)
    if rustup toolchain list 2>/dev/null | grep -q "^${tc}\b"; then
        cargo_cmd=(cargo "+${tc}")
    fi
    rustup target add "$triple" >/dev/null

    echo ">>> [windows] ${cargo_cmd[*]} build --release --target=$triple -p tablet -p tablet-cli"
    "${cargo_cmd[@]}" build --release --target "$triple" -p tablet -p tablet-cli

    cp "target/$triple/release/tablet.exe"     "$out/"
    cp "target/$triple/release/tablet-cli.exe" "$out/"
    echo ">>> [windows] OK -> $out"
    echo "    要求目标机器有 WebView2 运行时（Win11 自带；Win10 一般随 Edge 安装）"
}

build_linux() {
    local triple="x86_64-unknown-linux-gnu"
    local out="$DIST_DIR/linux"
    rm -rf "$out"
    mkdir -p "$out"

    build_frontend

    echo ">>> [linux] cargo build --release --target=$triple -p tablet -p tablet-cli"
    cargo build --release --target "$triple" -p tablet -p tablet-cli

    cp "target/$triple/release/tablet"     "$out/"
    cp "target/$triple/release/tablet-cli" "$out/"

    # .deb 打包
    echo ">>> [linux] 构建 .deb 包"
    local version
    version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
    local arch="amd64"
    local pkg_dir="$out/tablet_${version}_${arch}"
    rm -rf "$pkg_dir"
    mkdir -p "$pkg_dir/DEBIAN"
    mkdir -p "$pkg_dir/usr/bin"
    mkdir -p "$pkg_dir/usr/share/applications"
    mkdir -p "$pkg_dir/usr/share/icons/hicolor/256x256/apps"

    cp "$out/tablet"     "$pkg_dir/usr/bin/"
    cp "$out/tablet-cli" "$pkg_dir/usr/bin/"
    # 不 strip：release profile 的符号表用于崩溃日志，且 --strip-debug 之外的 strip 会连符号一起删。
    cp "$WS_DIR/res/icon-256.png" "$pkg_dir/usr/share/icons/hicolor/256x256/apps/tablet.png"
    cp "$WS_DIR/packaging/linux/tablet.desktop" "$pkg_dir/usr/share/applications/"

    local installed_size
    installed_size=$(du -sk "$pkg_dir" | cut -f1)
    cat > "$pkg_dir/DEBIAN/control" << CTRL
Package: tablet
Version: $version
Architecture: $arch
Maintainer: juebanlin <juebanlin@gmail.com>
Installed-Size: $installed_size
Depends: libwebkit2gtk-4.1-0, libgtk-3-0, libsoup-3.0-0, libjavascriptcoregtk-4.1-0, libglib2.0-0
Recommends: fonts-noto-cjk
Section: devel
Priority: optional
Homepage: https://github.com/juebanlin/tablet
Description: TBL 配置管理工具
 游戏/应用配置表编辑工具，支持多 Project 管理、Univer 可视化编辑、
 Excel 导入/导出、12 种语言平台导出。
CTRL

    dpkg-deb --build "$pkg_dir" 2>/dev/null && \
        mv "${pkg_dir}.deb" "$out/" && \
        echo ">>> [linux] .deb -> $out/tablet_${version}_${arch}.deb" || \
        echo ">>> [linux] dpkg-deb 不可用，跳过 .deb 打包"
    rm -rf "$pkg_dir"

    echo ">>> [linux] OK -> $out"
    echo "    用户机器需要 WebKitGTK 4.1 / GTK3；建议: apt install libwebkit2gtk-4.1-0 fonts-noto-cjk"
}

build_linux_cli_musl() {
    local triple="x86_64-unknown-linux-musl"
    local out="$DIST_DIR/linux-cli-musl"
    rm -rf "$out"
    mkdir -p "$out"

    # 仅 tablet-cli: Tauri GUI 依赖 WebKitGTK/GTK 动态库, musl 静态链路不可行.
    # CLI 全静态 → 任意 x86_64 Linux (老 glibc / 容器 scratch / Alpine) 直接跑.
    echo ">>> [linux-cli-musl] 安装 musl target"
    rustup target add "$triple" >/dev/null

    echo ">>> [linux-cli-musl] cargo build --release --target=$triple -p tablet-cli"
    cargo build --release --target "$triple" -p tablet-cli

    cp "target/$triple/release/tablet-cli" "$out/"
    echo ">>> [linux-cli-musl] OK -> $out (CLI only, 全静态)"
    echo "    需要 musl-tools: apt install musl-tools"
}

build_macos() {
    local out="$DIST_DIR/macos"
    rm -rf "$out"
    mkdir -p "$out"

    build_frontend

    rustup target add x86_64-apple-darwin aarch64-apple-darwin >/dev/null

    # Tauri 2 的实际下限是 10.15；这里与 packaging/macos/Info.plist 对齐到 11.0。
    export MACOSX_DEPLOYMENT_TARGET=11.0

    echo ">>> [macos] build x86_64-apple-darwin (min 11.0)"
    cargo build --release --target x86_64-apple-darwin -p tablet -p tablet-cli

    echo ">>> [macos] build aarch64-apple-darwin (min 11.0)"
    cargo build --release --target aarch64-apple-darwin -p tablet -p tablet-cli

    echo ">>> [macos] lipo -> universal binary"
    for bin in tablet tablet-cli; do
        lipo -create -output "$out/$bin" \
            "target/x86_64-apple-darwin/release/$bin" \
            "target/aarch64-apple-darwin/release/$bin"
        chmod +x "$out/$bin"
    done

    # .app 包
    echo ">>> [macos] 构建 Tablet.app"
    local version
    version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
    local app_dir="$out/Tablet.app"
    rm -rf "$app_dir"
    mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
    cp "$out/tablet" "$app_dir/Contents/MacOS/tablet"
    cp "$WS_DIR/res/icon.icns" "$app_dir/Contents/Resources/icon.icns"
    sed "s/1.2.1/$version/g" "$WS_DIR/packaging/macos/Info.plist" > "$app_dir/Contents/Info.plist"

    # .app → zip（直接解压即可运行）
    echo ">>> [macos] 压缩 Tablet.app.zip"
    (cd "$out" && zip -r "Tablet-${version}.app.zip" Tablet.app)

    # .dmg
    echo ">>> [macos] 构建 .dmg"
    local dmg_path="$out/Tablet-${version}.dmg"
    rm -f "$dmg_path"
    if command -v create-dmg &>/dev/null; then
        create-dmg --volname "Tablet" --window-size 600 400 \
            --icon "Tablet.app" 150 200 --app-drop-link 450 200 \
            "$dmg_path" "$app_dir"
    else
        hdiutil create -volname "Tablet" -srcfolder "$app_dir" -ov -format UDZO "$dmg_path" 2>/dev/null || \
            echo ">>> [macos] hdiutil 不可用，跳过 .dmg"
    fi

    # 清理 .app 目录（zip 和 dmg 已包含）
    rm -rf "$app_dir"

    echo ">>> [macos] OK -> $out (universal, intel + arm64, min 11.0)"
    echo "    正式发布需 Apple Developer ID 签名 + 公证, 否则 Gatekeeper 拦截"
}

# ------------------------------------------------------------------
# 入口
# ------------------------------------------------------------------

target="${1:-}"
if [[ -z "$target" ]]; then
    echo "usage: $0 <windows|linux|linux-cli-musl|macos|host>" >&2
    exit 2
fi

if [[ "$target" == "host" ]]; then
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*|Windows_NT) target="windows-host" ;;
        Linux)   target="linux-host" ;;
        Darwin)  target="macos" ;;
        *) echo "unsupported host: $(uname -s)" >&2; exit 2 ;;
    esac
fi

case "$target" in
    windows)        build_windows ;;
    windows-host)   build_windows ;;
    linux)          build_linux ;;
    linux-cli-musl) build_linux_cli_musl ;;
    linux-host)     build_linux; build_linux_cli_musl ;;
    macos)          build_macos ;;
    *) echo "unknown target: $target" >&2; exit 2 ;;
esac

echo ""
echo "=== 完成 ==="
ls -lh "$DIST_DIR"/*/* 2>/dev/null || true
