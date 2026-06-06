#!/usr/bin/env bash
# tbl-tool 平台发布脚本
#
# 用法:
#   scripts/release.sh <target>
#
# target:
#   windows         x86_64-pc-windows-msvc          (Tier 1, 需 Windows 宿主)
#   windows-win7    x86_64-win7-windows-msvc        (Tier 3, 需 Windows 宿主 + nightly)
#   linux           x86_64-unknown-linux-gnu        (Tier 1, 需 Linux 宿主)
#   macos           x86_64-apple-darwin + aarch64-apple-darwin → universal (需 macOS 宿主)
#   host            自动按宿主跑所有可跑目标:
#                     Windows → windows + windows-win7
#                     Linux   → linux
#                     macOS   → macos
#
# 产物:
#   dist/<target>/tbl-slint(.exe)
#   dist/<target>/tbl-cli(.exe)
#
# 设计:
#   不做跨编译。Slint 渲染栈 (D3D/OpenGL/Metal/字体) 跨编出来的二进制
#   在用户机器上很容易炸；让 CI matrix 的每个 runner 各编各的目标即可。
#
# GitHub Actions 复用:
#   matrix:
#     - { os: windows-latest, target: windows }
#     - { os: windows-latest, target: windows-win7 }
#     - { os: ubuntu-latest,  target: linux }
#     - { os: macos-latest,   target: macos }
#   再 run: bash tbl-tool/scripts/release.sh ${{ matrix.target }}

set -euo pipefail

# 切到 tbl-tool/ workspace
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WS_DIR"

DIST_DIR="$WS_DIR/dist"
mkdir -p "$DIST_DIR"

# ------------------------------------------------------------------
# 各 target 实现
# ------------------------------------------------------------------

build_windows() {
    local triple="x86_64-pc-windows-msvc"
    local out="$DIST_DIR/windows"
    mkdir -p "$out"

    echo ">>> [windows] cargo build --release --target=$triple"
    # +crt-static: 静态链 VC++ runtime, 用户机器无需装 "VC++ 可再发行包"
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build --release --target "$triple" \
            -p tbl-slint -p tbl-cli

    cp "target/$triple/release/tbl-slint.exe" "$out/"
    cp "target/$triple/release/tbl-cli.exe"   "$out/"
    echo ">>> [windows] OK -> $out"
}

build_windows_win7() {
    local triple="x86_64-win7-windows-msvc"
    local out="$DIST_DIR/windows-win7"
    mkdir -p "$out"

    # Tier 3 target 需要 nightly + build-std (std 不预编译)
    echo ">>> [windows-win7] 需要 nightly + rust-src + build-std"

    if ! rustup toolchain list | grep -q '^nightly'; then
        echo ">>> 安装 nightly toolchain"
        rustup toolchain install nightly --component rust-src
    else
        rustup component add rust-src --toolchain nightly >/dev/null
    fi

    echo ">>> [windows-win7] cargo +nightly build --release --target=$triple"
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo +nightly build --release \
            -Z build-std=std,panic_abort \
            -Z build-std-features=panic_immediate_abort \
            --target "$triple" \
            -p tbl-slint -p tbl-cli

    cp "target/$triple/release/tbl-slint.exe" "$out/"
    cp "target/$triple/release/tbl-cli.exe"   "$out/"
    echo ">>> [windows-win7] OK -> $out"
    echo "    警告: Tier 3 target 无 CI 保证, 升级 nightly 可能突然炸; 发布前需 Win7 SP1 真机回归"
}

build_linux() {
    local triple="x86_64-unknown-linux-gnu"
    local out="$DIST_DIR/linux"
    mkdir -p "$out"

    echo ">>> [linux] cargo build --release --target=$triple"
    cargo build --release --target "$triple" -p tbl-slint -p tbl-cli

    cp "target/$triple/release/tbl-slint" "$out/"
    cp "target/$triple/release/tbl-cli"   "$out/"
    echo ">>> [linux] OK -> $out"
    echo "    用户机器可能需要: apt install libgl1 libfontconfig1 fonts-noto-cjk"
}

build_macos() {
    local out="$DIST_DIR/macos"
    mkdir -p "$out"

    rustup target add x86_64-apple-darwin aarch64-apple-darwin >/dev/null

    echo ">>> [macos] build x86_64-apple-darwin"
    cargo build --release --target x86_64-apple-darwin -p tbl-slint -p tbl-cli

    echo ">>> [macos] build aarch64-apple-darwin"
    cargo build --release --target aarch64-apple-darwin -p tbl-slint -p tbl-cli

    echo ">>> [macos] lipo -> universal binary"
    for bin in tbl-slint tbl-cli; do
        lipo -create -output "$out/$bin" \
            "target/x86_64-apple-darwin/release/$bin" \
            "target/aarch64-apple-darwin/release/$bin"
        chmod +x "$out/$bin"
    done
    echo ">>> [macos] OK -> $out (universal, intel + arm64)"
    echo "    正式发布需 Apple Developer ID 签名 + 公证, 否则 Gatekeeper 拦截"
}

# ------------------------------------------------------------------
# 入口
# ------------------------------------------------------------------

target="${1:-}"
if [[ -z "$target" ]]; then
    echo "usage: $0 <windows|windows-win7|linux|macos|host>" >&2
    exit 2
fi

if [[ "$target" == "host" ]]; then
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*|Windows_NT) target="windows-host" ;;
        Linux)   target="linux" ;;
        Darwin)  target="macos" ;;
        *) echo "unsupported host: $(uname -s)" >&2; exit 2 ;;
    esac
fi

case "$target" in
    windows)        build_windows ;;
    windows-win7)   build_windows_win7 ;;
    windows-host)   build_windows; build_windows_win7 ;;
    linux)          build_linux ;;
    macos)          build_macos ;;
    *) echo "unknown target: $target" >&2; exit 2 ;;
esac

echo ""
echo "=== 完成 ==="
ls -lh "$DIST_DIR"/*/* 2>/dev/null || true
