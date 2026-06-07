#!/usr/bin/env bash
# tablet 平台发布脚本
#
# 用法:
#   scripts/release.sh <target>
#
# target:
#   windows          x86_64-pc-windows-msvc          (Tier 1, 需 Windows 宿主)
#   windows-win7     x86_64-win7-windows-msvc        (Tier 3, 需 Windows 宿主 + nightly)
#   linux            x86_64-unknown-linux-gnu        (Tier 1, 需 Linux 宿主)
#   linux-cli-musl   x86_64-unknown-linux-musl       (Tier 2, 仅 tablet-cli, 静态链, 任意 Linux 可跑)
#   macos            x86_64-apple-darwin + aarch64-apple-darwin → universal (需 macOS 宿主, 最低 10.13)
#   host             自动按宿主跑所有可跑目标:
#                      Windows → windows + windows-win7
#                      Linux   → linux + linux-cli-musl
#                      macOS   → macos
#
# 产物:
#   dist/<target>/tablet(.exe)        ← GUI（顶层入口，零参数 / --gui 走 GUI、其它参数转 CLI）
#   dist/<target>/tablet-cli(.exe)    ← 纯 CLI（Jenkins / 自动化批处理）
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
#     - { os: ubuntu-latest,  target: linux-cli-musl }
#     - { os: macos-latest,   target: macos }
#   再 run: bash scripts/release.sh ${{ matrix.target }}

set -euo pipefail

# 切到仓库根
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
            -p tablet-slint -p tablet-cli

    cp "target/$triple/release/tablet.exe"     "$out/"
    cp "target/$triple/release/tablet-cli.exe" "$out/"
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
    # 新版 nightly 把 panic_immediate_abort 从 -Z build-std-features 提升为正式 panic 策略,
    # 旧的 -Z build-std-features=panic_immediate_abort 已被拒绝, 现在通过 -Cpanic=immediate-abort 启用
    # (仍然需要 -Z build-std 重编 core/std).
    RUSTFLAGS="-C target-feature=+crt-static -Z unstable-options -C panic=immediate-abort" \
        cargo +nightly build --release \
            -Z build-std=std,panic_abort \
            --target "$triple" \
            -p tablet-slint -p tablet-cli

    cp "target/$triple/release/tablet.exe"     "$out/"
    cp "target/$triple/release/tablet-cli.exe" "$out/"
    echo ">>> [windows-win7] OK -> $out"
    echo "    警告: Tier 3 target 无 CI 保证, 升级 nightly 可能突然炸; 发布前需 Win7 SP1 真机回归"
}

build_linux() {
    local triple="x86_64-unknown-linux-gnu"
    local out="$DIST_DIR/linux"
    mkdir -p "$out"

    echo ">>> [linux] cargo build --release --target=$triple"
    cargo build --release --target "$triple" -p tablet-slint -p tablet-cli

    cp "target/$triple/release/tablet"     "$out/"
    cp "target/$triple/release/tablet-cli" "$out/"
    echo ">>> [linux] OK -> $out"
    echo "    用户机器可能需要: apt install libgl1 libfontconfig1 fonts-noto-cjk"
}

build_linux_cli_musl() {
    local triple="x86_64-unknown-linux-musl"
    local out="$DIST_DIR/linux-cli-musl"
    mkdir -p "$out"

    # 仅 tablet-cli: tablet-slint 依赖 X11/Wayland/fontconfig 等动态库, musl 静态链路死路一条.
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
    mkdir -p "$out"

    rustup target add x86_64-apple-darwin aarch64-apple-darwin >/dev/null

    # 最低部署目标 10.13 (High Sierra, 2017): 覆盖 ~99% 仍在用的 mac.
    # 不设的话默认跟宿主走, runner 是 13.x → 10.13 用户启动时 dyld 报错.
    export MACOSX_DEPLOYMENT_TARGET=10.13

    echo ">>> [macos] build x86_64-apple-darwin (min 10.13)"
    cargo build --release --target x86_64-apple-darwin -p tablet-slint -p tablet-cli

    echo ">>> [macos] build aarch64-apple-darwin (min 10.13)"
    cargo build --release --target aarch64-apple-darwin -p tablet-slint -p tablet-cli

    echo ">>> [macos] lipo -> universal binary"
    for bin in tablet tablet-cli; do
        lipo -create -output "$out/$bin" \
            "target/x86_64-apple-darwin/release/$bin" \
            "target/aarch64-apple-darwin/release/$bin"
        chmod +x "$out/$bin"
    done
    echo ">>> [macos] OK -> $out (universal, intel + arm64, min 10.13)"
    echo "    正式发布需 Apple Developer ID 签名 + 公证, 否则 Gatekeeper 拦截"
}

# ------------------------------------------------------------------
# 入口
# ------------------------------------------------------------------

target="${1:-}"
if [[ -z "$target" ]]; then
    echo "usage: $0 <windows|windows-win7|linux|linux-cli-musl|macos|host>" >&2
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
    windows-win7)   build_windows_win7 ;;
    windows-host)   build_windows; build_windows_win7 ;;
    linux)          build_linux ;;
    linux-cli-musl) build_linux_cli_musl ;;
    linux-host)     build_linux; build_linux_cli_musl ;;
    macos)          build_macos ;;
    *) echo "unknown target: $target" >&2; exit 2 ;;
esac

echo ""
echo "=== 完成 ==="
ls -lh "$DIST_DIR"/*/* 2>/dev/null || true
