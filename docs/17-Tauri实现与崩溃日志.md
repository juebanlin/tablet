# Tauri 实现与崩溃日志

> **状态：3.0 定稿（2026-11）。** Tauri 是唯一 GUI；Slint 版与 `app-slint` 已删除。
> 与 @09（平台发布）配套；打包与崩溃日志冲突时以本文为准。

## 1. 结论摘要

| 决策点 | 定稿 | 明确不做 |
| --- | --- | --- |
| GUI 数量 | **仅 Tauri**（`crates/app-tauri`，包名/产物 `tablet`） | 不恢复 Slint、不做双 GUI 并存 |
| Windows target | `x86_64-pc-windows-msvc` | 不为复用 gnu 工具链而妥协 |
| 产物 | **单 exe**（`bundle.active = false`），前端资源内嵌、WebView2Loader 静态链接 | 不发 NSIS/MSI 安装器、不带 DLL、不带 resources |
| GUI/CLI 双模式 | 保留 `shell.rs` 的 `classify()`：零参数/`--gui` → GUI，其它 → CLI | 不引入 `tauri-plugin-single-instance` |
| 崩溃日志 | `file:line:col` + 崩溃前 N 条日志，不依赖符号文件 | 不做 pdb 内嵌 / 离线符号化 / minidump |
| 许可 | 全仓 Apache-2.0 | 不再有 GPL 组件 |

三条容易踩错的结论：

1. **必须 msvc**：`windows-gnu` 下 `WebView2Loader.dll` 是外挂 DLL，破坏单文件目标。
2. **不要加 `windows_subsystem = "windows"`**：CLI 模式会没有 stdout、cmd 也不等，直接废掉双模式。
3. **崩溃日志不需要符号**：`PanicHookInfo` 的 `Display` 自带 `panicked at <file>:<line>:<col>:`，与 pdb / strip / LTO 无关。

## 2. 仓库布局

```
crates/
  core/        tablet-core（Apache-2.0）—— 锁、解析、校验、导出、崩溃日志
  app-cli/     tablet-cli（Apache-2.0）—— CLI 命令实现 + run_with_args
  app-tauri/   tablet（Apache-2.0）—— 唯一 GUI；[[bin]] name = "tablet"
```

| 产物 | 来源 | 目标系统 | 说明 |
| --- | --- | --- | --- |
| `tablet.exe` / `tablet` | `app-tauri` | Win10 1803+ / Linux / macOS 11+ | 单文件，依赖系统 WebView2（Windows）或 WebKitGTK（Linux） |
| `tablet-cli.exe` / `tablet-cli` | `app-cli` | 同 @09 | 纯 CLI，Jenkins / 自动化用 |

- 工作区锁（`<workdir>/.data/tablet.lock`）由 core 独占；同一项目互斥、不同项目可并行。
- `tauri.conf.json` 的 identifier 固定为 `com.juebanlin.tablet.tauri`，决定 WebView2 user data 目录，**不要改**。

## 3. 单文件打包

一个 exe 双击即用：前端资源编译期内嵌、`WebView2Loader` 静态链接、无旁挂文件。

```bash
# 先构建 Univer 前端（ui/univer 被 gitignore；裸 cargo build 不会触发 beforeBuildCommand）
npm --prefix crates/app-tauri/univer-editor ci
npm --prefix crates/app-tauri/univer-editor run build

cargo build --release -p tablet --target x86_64-pc-windows-msvc
# 产物：target/x86_64-pc-windows-msvc/release/tablet.exe
```

也可以用 `cargo tauri build --no-bundle`（会执行 `beforeBuildCommand`）。CI/release 脚本走裸 cargo + 显式 npm。

约束：

| 项 | 规定 |
| --- | --- |
| target | `x86_64-pc-windows-msvc` |
| `strip` | 保持 `"debuginfo"`；pdb 不归档、不随包发 |
| `bundle.resources` | 必须为空 |
| `externalBin` | 禁用 |
| `webviewInstallMode` | `skip`；不要用 `fixedRuntime`（会多出 ~180MB 目录） |
| 前端构建 | `npm ci && npm run build`（或 tauri CLI 的 `beforeBuildCommand`） |

允许落盘的位置（不影响"单文件"）：

- WebView2 user data：`%LOCALAPPDATA%\<identifier>`（Tauri 默认）。
- 业务日志 / 崩溃日志：仍在 `workdir`（`tablet.log` / `tablet-crash.log`）。

## 4. 平台地板

| 系统 | 支持 | 说明 |
| --- | --- | --- |
| Win11 / Win10 1803+ | ✅ | WebView2 随 OS / Edge 分发；未安装时需人工装 Evergreen 运行时 |
| Win7 / 8.1 | ❌ | WebView2 已冻结，Tauri 不支持 |
| Linux x86_64 | ✅ | 需 WebKitGTK 4.1 / GTK3；musl 只发 CLI |
| macOS 11+ | ✅ | universal（intel + arm64），未签名需右键打开 |

## 5. GUI + CLI 双模式

`crates/app-tauri/src/shell.rs`：

- 零参数 → GUI；含 `--gui` → GUI；其它参数 → 转发给 `tablet_cli::run_with_args`。
- CLI 分支必须在创建 `tauri::Builder` 之前完成，并原样透传退出码。
- Windows 保持 **console 子系统**；GUI 分支 `FreeConsole()` 释放控制台（<50ms 黑闪可接受）。
- GUI 分支释放控制台后 stderr 无人接收，因此崩溃日志必须落文件（@6）。

## 6. 崩溃日志规格

- 文件：`<workdir>/tablet-crash.log`，追加写。
- 必含字段：`app`（`tablet`）/ `version` / `target` / `位置`（`<file>:<line>:<col>`）。
- 附带「崩溃前最后 N 条日志」（来自 `tablet.log` 的内存环形缓冲），用于缩小范围。
- **不依赖符号文件**；release profile 保留符号表只为 Linux/macOS 的地址转函数名，Windows 用 msvc 时符号在 pdb。
- 隐藏参数 `--crash-selftest` 故意 panic，用于在任意机器（含 release 单 exe）验证链路。
- 已知盲区：原生崩溃 / webview 渲染进程崩溃没有 `location()`，只能靠崩溃前日志。

## 7. 明确不做

| 不做 | 理由 | 触发条件 |
| --- | --- | --- |
| pdb 内嵌进 exe | 两阶段构建 + PE overlay，成本远超收益 | 必须直接在用户机拿完整栈时 |
| 离线符号化工具 | `file:line:col` 已够定位 | 同上 |
| minidump / 未处理异常过滤 | 只覆盖非 panic 崩溃 | "无崩溃日志的闪退"占比变高时 |
| JS 侧错误回灌 | 只覆盖 webview 内错误 | 实际使用反馈需要时 |
| `tauri-plugin-single-instance` | 与多项目并行冲突 | 永不 |

## 8. 验收清单

1. `cargo test --workspace` 通过。
2. `npm ... build` 后 `cargo build --target x86_64-pc-windows-msvc -p tablet` → `target/x86_64-pc-windows-msvc/debug/tablet.exe`。
3. `tablet.exe --help` / `--version` / `validate` 在 cmd 中输出完整、退出码正确。
4. `tablet.exe --crash-selftest` 后 `<workdir>/tablet-crash.log` 含 `app: tablet`、`version: 3.0.0`、`target: x86_64-pc-windows-msvc`、`位置`。
5. 干净虚拟机双击 release exe 能起窗口（WebView2 运行时已安装）。
6. 同机两个实例、不同项目并行正常；同一项目第二个实例被锁拒绝。
7. 产物目录只有一个 exe（外加日志 / 工作区数据），无 DLL / resources。

## 9. 工程约束

- 验证走 dev profile（`cargo check` / `cargo build` / `cargo test`）；release 只用于交付。
- 不自动 `git commit`；改完报告改动与验证结果，等验收。
- 注释与文档中文，标识符英文；`.rs` 文件带 SPDX 头（Apache-2.0）。
- 文档改动同步 `docs/00-概述.md` §4 索引。
