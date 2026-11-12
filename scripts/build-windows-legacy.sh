#!/usr/bin/env bash
# Windows 老平台兼容包（Win7 SP1 / 8.1 / Win10 1803 之前）
#
# 用法:
#   bash scripts/build-windows-legacy.sh
#
# 环境变量:
#   WEBVIEW2_FIXED_RUNTIME_DIR=/path/to/runtime
#       可选。WebView2 109 固定版运行时目录（把微软 Fixed Version 的 .cab 解压后得到的目录）。
#       提供时会把整个目录塞进 zip 的 WebView2Runtime/；不提供时 zip 内附下载/放置说明。
#
# 产物:
#   dist/windows-legacy/tablet-<ver>-windows-legacy-win7-win10lt1803-x64.zip
#
# 前置:
#   - Windows 宿主；Node/npm（Univer 前端）
#   - nightly + rust-src（脚本会自动安装/补齐）
#
# 说明:
#   - 用 Tier 3 `x86_64-win7-windows-msvc` + `-Z build-std` 编译（Rust 1.78+ 的 Tier1
#     Windows 目标最低 Win10，无法直接产出 Win7 可用的 exe）。
#   - WebView2 109 是最后一个支持 Win7/8.1 的版本，2023 年后无安全更新；此包仅实验性。
#   - Linux/macOS 宿主请勿执行。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WS_DIR"

DIST_DIR="$WS_DIR/dist"
PKG_DIR="$DIST_DIR/windows-legacy"
OUT="$PKG_DIR/build"
FRONTEND_DIR="$WS_DIR/crates/app-tauri/univer-editor"
TRIPLE="x86_64-win7-windows-msvc"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
ZIP_NAME="tablet-${VERSION}-windows-legacy-win7-win10lt1803-x64.zip"

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*|Windows_NT) ;;
    *) echo "此脚本只能在 Windows 宿主运行（当前: $(uname -s)）" >&2; exit 2 ;;
esac

echo ">>> [legacy] 构建 Univer 前端"
(cd "$FRONTEND_DIR" && npm ci --no-audit --no-fund && npm run build)
test -f "$WS_DIR/crates/app-tauri/ui/univer/index.html" || {
    echo "frontend 构建后仍缺少 ui/univer/index.html" >&2
    exit 1
}

echo ">>> [legacy] 准备 nightly + rust-src"
NIGHTLY_TC="${NIGHTLY_TOOLCHAIN:-nightly}"
if ! rustup toolchain list 2>/dev/null | grep -q "^${NIGHTLY_TC}"; then
    # dtolnay/rust-toolchain 安装的是带日期的名字（如 nightly-2026-11-10），取第一个。
    NIGHTLY_TC=$(rustup toolchain list 2>/dev/null | sed -n 's/^\(nightly[^ ]*\).*/\1/p' | head -1)
fi
if [ -z "$NIGHTLY_TC" ]; then
    NIGHTLY_TC="nightly"
    rustup toolchain install nightly --profile minimal --component rust-src
else
    rustup component add rust-src --toolchain "$NIGHTLY_TC" >/dev/null 2>&1 || true
fi

echo ">>> [legacy] cargo build --target=$TRIPLE ($NIGHTLY_TC + build-std)"
RUSTFLAGS="-C target-feature=+crt-static -C panic=abort" \
    cargo +"$NIGHTLY_TC" build --release \
        -Z build-std=std,panic_abort \
        --target "$TRIPLE" \
        -p tablet -p tablet-cli

rm -rf "$PKG_DIR"
mkdir -p "$OUT"
cp "target/$TRIPLE/release/tablet.exe" "$OUT/"
cp "target/$TRIPLE/release/tablet-cli.exe" "$OUT/"
cp "$WS_DIR/LICENSE-APACHE" "$WS_DIR/NOTICE" "$OUT/"

# 启动器：有固定版运行时就用它（WebView2Loader 认这个环境变量），没有就按系统安装的跑。
cat > "$OUT/start-tablet.bat" <<'BAT'
@echo off
setlocal
if exist "%~dp0WebView2Runtime\msedgewebview2.exe" (
  set "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=%~dp0WebView2Runtime"
)
start "" "%~dp0tablet.exe" %*
BAT

cat > "$OUT/README.txt" <<'TXT'
tablet — Windows 老平台兼容包（实验性）
=====================================

适用系统
  Windows 7 SP1 / Windows 8.1 / Windows 10 1803 之前（x64）。
  Windows 10 1803+ / Windows 11 请使用主线 tablet-<版本>-windows10-1803-x64.exe。

运行
  1. 如果本目录没有 WebView2Runtime\，请先准备 WebView2 固定版 109：
     - 微软介绍: https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution
     - 固定版下载页: https://developer.microsoft.com/microsoft-edge/webview2/
     - 也可在 Microsoft Update Catalog 搜索 "WebView2 Runtime 109.0.1518.140"。
     把固定版 .cab 解压后的目录改名为 WebView2Runtime，放在本文件同级。
  2. 双击 start-tablet.bat（它会设置 WEBVIEW2_BROWSER_EXECUTABLE_FOLDER）。
  3. CLI 用法不变：tablet-cli.exe --help

限制
  - WebView2 109 是最后一个支持 Win7/8.1 的版本，2023 年后不再有安全更新。
  - 该包未做 Win7 真机 CI 验收；遇到启动失败请附 tablet-crash.log / tablet.log。
  - 与主线共用同一套数据格式，但请避免在不可信网络环境使用老浏览器内核。

许可证
  Apache-2.0（见 LICENSE-APACHE / NOTICE）。
TXT

if [ -n "${WEBVIEW2_FIXED_RUNTIME_DIR:-}" ]; then
    if [ ! -d "$WEBVIEW2_FIXED_RUNTIME_DIR" ]; then
        echo "WEBVIEW2_FIXED_RUNTIME_DIR 不存在: $WEBVIEW2_FIXED_RUNTIME_DIR" >&2
        exit 1
    fi
    echo ">>> [legacy] 打包 WebView2 固定版运行时"
    cp -r "$WEBVIEW2_FIXED_RUNTIME_DIR" "$OUT/WebView2Runtime"
else
    cat > "$OUT/WEBVIEW2-RUNTIME.txt" <<'TXT'
本 zip 未内置 WebView2 固定版运行时（CI 无法从微软稳定自动下载 109）。

请按 README.txt 获取 WebView2 109 固定版，解压为 WebView2Runtime\ 后
再双击 start-tablet.bat。
TXT
fi

echo ">>> [legacy] 打包 zip"
rm -f "$PKG_DIR/$ZIP_NAME"
powershell -NoProfile -Command \
    "Compress-Archive -Path '$(cygpath -w "$OUT")\\*' -DestinationPath '$(cygpath -w "$PKG_DIR/$ZIP_NAME")' -Force"
rm -rf "$OUT"

echo ">>> [legacy] OK -> $PKG_DIR/$ZIP_NAME"
ls -lh "$PKG_DIR/$ZIP_NAME"
