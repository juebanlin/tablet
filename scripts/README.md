# scripts

## release.sh

一次性平台发布脚本。`bash scripts/release.sh <target>`：

| target | 宿主要求 | 产物 |
|--------|--------|------|
| `windows` | Windows | `dist/windows/{tablet,tablet-cli}.exe`（`x86_64-pc-windows-msvc`；Tauri 单文件，需 WebView2 运行时） |
| `linux` | Linux + WebKitGTK 4.1 开发包 | `dist/linux/{tablet,tablet-cli}` + `.deb` |
| `linux-cli-musl` | Linux + `musl-tools` | `dist/linux-cli-musl/tablet-cli`（仅 CLI，全静态，老 glibc / Alpine / 容器 scratch 直接跑） |
| `macos` | macOS | `dist/macos/{tablet,tablet-cli}`（lipo 合并 intel + arm64 universal，最低 macOS 10.15） |
| `host` | 任意 | 自动按宿主跑所有可跑目标（Windows → `windows`；Linux → `linux + linux-cli-musl`） |

老平台兼容包**单独脚本**产出，不混进日常构建：

```bash
bash scripts/build-windows-legacy.sh
# -> dist/windows-legacy/tablet-<ver>-windows-legacy-win7-win10lt1803-x64.zip
#    适用 Win7 SP1 / 8.1 / Win10 1803 之前；nightly win7 target + WebView2 109
```

可用 `WEBVIEW2_FIXED_RUNTIME_DIR=<目录>` 把 WebView2 109 固定版运行时打进 zip。

二进制名约定：
- `tablet[.exe]` —— Tauri GUI 顶层入口，零参数 / `--gui` 走 GUI、其它参数转 CLI fallback
- `tablet-cli[.exe]` —— 纯 CLI，体积小，Jenkins / 自动化批处理用

发布前脚本会先执行 `npm ci && npm run build`（`crates/app-tauri/univer-editor`），因为 Tauri 的 UI 资源在编译期内嵌，而裸 `cargo build` 不会触发 `beforeBuildCommand`。

**不做跨编译**：每个 runner 各编自家目标即可。

## GitHub Actions

见 `.github/workflows/release.yml`（tag 触发）。要点：
- Windows 主线用 msvc（Win10 1803+ / Win11）；Win7/8.1/老 Win10 走独立 `windows-legacy` 包（nightly win7 target + WebView2 109，实验性）
- Linux 需要 `libwebkit2gtk-4.1-dev` / `libgtk-3-dev` 等；release 在 `ubuntu-22.04` 上跑以降低 glibc 下限
- `linux-cli-musl` 仅产 `tablet-cli` 静态二进制；GUI 依赖 WebKitGTK 动态库，musl 走不通
- macOS 最低 10.15（`MACOSX_DEPLOYMENT_TARGET=10.15`）；签名/公证脚本不做
- artifact 体积：Windows Tauri 单 exe 约 25MB，Linux 类似；`tablet-cli` 更小

## GUI 自动化

Tauri 的 CDP 脚本在 `crates/app-tauri/scripts/`（`start-debug.ps1` + `verify-*.mjs`）；旧的 Slint 坐标 harness 已随 Slint 版一起删除。
