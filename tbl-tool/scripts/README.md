# tbl-tool/scripts

## release.sh

一次性平台发布脚本。`bash scripts/release.sh <target>`：

| target | 宿主要求 | 产物 |
|--------|--------|------|
| `windows` | Windows | `dist/windows/{tbl-slint,tbl-cli}.exe`（静态链 VC++ runtime） |
| `windows-win7` | Windows + nightly toolchain | `dist/windows-win7/...`（Tier 3，自动装 nightly + rust-src + build-std） |
| `linux` | Linux | `dist/linux/{tbl-slint,tbl-cli}` |
| `macos` | macOS | `dist/macos/{tbl-slint,tbl-cli}`（lipo 合并 intel + arm64 universal） |
| `host` | 任意 | 自动按宿主跑所有可跑目标（Windows 跑 `windows + windows-win7`） |

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
          - { os: macos-latest,   target: macos }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      # Linux 需要的系统依赖（slint 渲染栈 + 字体）
      - if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt update
          sudo apt install -y libgl1-mesa-dev libfontconfig1-dev \
                              libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
      - name: build
        shell: bash
        run: bash tbl-tool/scripts/release.sh ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: tbl-tool-${{ matrix.target }}
          path: tbl-tool/dist/${{ matrix.target }}/
```

注意点：
- **Win7 那行**自动装 nightly。本地没装 nightly 也能跑——脚本检测后调 `rustup toolchain install`
- **macOS 签名/公证**脚本不做。GitHub Actions 上签名要 `apple-actions/import-codesign-certs` + 团队证书
- **artifact 体积**：Linux/Win release 二进制各约 25-40MB（含 slint），上传到 GitHub Releases 没问题
