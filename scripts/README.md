# scripts

## release.sh

一次性平台发布脚本。`bash scripts/release.sh <target>`：

| target | 宿主要求 | 产物 |
|--------|--------|------|
| `windows` | Windows | `dist/windows/{tablet,tablet-cli}.exe`（静态链 VC++ runtime） |
| `windows-win7` | Windows + nightly toolchain | `dist/windows-win7/...`（Tier 3，自动装 nightly + rust-src + build-std） |
| `linux` | Linux | `dist/linux/{tablet,tablet-cli}` |
| `linux-cli-musl` | Linux + `musl-tools` | `dist/linux-cli-musl/tablet-cli`（仅 CLI，全静态，老 glibc / Alpine / 容器 scratch 直接跑） |
| `macos` | macOS | `dist/macos/{tablet,tablet-cli}`（lipo 合并 intel + arm64 universal，最低 macOS 10.13） |
| `host` | 任意 | 自动按宿主跑所有可跑目标（Windows 跑 `windows + windows-win7`；Linux 跑 `linux + linux-cli-musl`） |

二进制名约定：
- `tablet[.exe]` —— GUI 顶层入口（基于 slint），零参数 / `--gui` 走 GUI、其它参数转 CLI fallback
- `tablet-cli[.exe]` —— 纯 CLI，无 slint 依赖，体积小，Jenkins / 自动化批处理用

**不做跨编译**：Slint 渲染栈/字体/系统 API 的跨编译副作用（Windows 上缺 D3D 路径、macOS 上 Apple SDK 链接、Linux 上 fontconfig 找不到）远比并行 3 台 runner 麻烦。每个 runner 各编自家目标即可。

## GitHub Actions 复用

脚本就是按这个用法设计的。参考 workflow（仓库推到 GitHub 后再启用即可）：

```yaml
# .github/workflows/release.yml
name: release
on:
  push:
    tags: ['v*']
  workflow_dispatch:

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: windows-latest, target: windows }
          - { os: windows-latest, target: windows-win7 }
          - { os: ubuntu-latest,  target: linux }
          - { os: ubuntu-latest,  target: linux-cli-musl }
          - { os: macos-latest,   target: macos }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      # Linux 需要的系统依赖（slint 渲染栈 + 字体）
      - if: matrix.os == 'ubuntu-latest' && matrix.target == 'linux'
        run: |
          sudo apt update
          sudo apt install -y libgl1-mesa-dev libfontconfig1-dev \
                              libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
      # musl 静态链需要 musl-tools
      - if: matrix.target == 'linux-cli-musl'
        run: sudo apt update && sudo apt install -y musl-tools
      - name: build
        shell: bash
        run: bash scripts/release.sh ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: tablet-${{ matrix.target }}
          path: dist/${{ matrix.target }}/
```

注意点：
- **Win7 那行**自动装 nightly。本地没装 nightly 也能跑——脚本检测后调 `rustup toolchain install`
- **`linux-cli-musl`** 仅产 `tablet-cli` 静态二进制。`tablet`（GUI）依赖 X11/Wayland/fontconfig 等动态库，musl 全静态没法走通；需要 GUI 仍用 `linux` 目标。需要 `musl-tools` (`apt install musl-tools`)
- **macOS 最低 10.13**（High Sierra, 2017）由 `MACOSX_DEPLOYMENT_TARGET=10.13` 限定；不设的话默认跟 runner 一致（13.x），10.13 用户启动会被 dyld 拒
- **macOS 签名/公证**脚本不做。GitHub Actions 上签名要 `apple-actions/import-codesign-certs` + 团队证书
- **artifact 体积**：Linux/Win release 二进制各约 25-40MB（含 slint），上传到 GitHub Releases 没问题；`linux-cli-musl/tablet-cli` 静态链后大约 8-12MB
