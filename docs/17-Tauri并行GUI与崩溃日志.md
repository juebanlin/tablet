# Tauri 并行 GUI 与崩溃日志

> **状态：定稿（2026-11）。** 本文是「新增 Tauri GUI + 崩溃日志收敛」的实施依据，可直接交给接手 agent 落地。
> 与 @09（平台发布）、@05（Slint 实现）并列；涉及 Tauri 的部分与既有文档冲突时，**以本文为准**。

## 1. 结论摘要

| 决策点 | 定稿 | 明确不做 |
| --- | --- | --- |
| GUI 数量 | 两套并行：`app-slint`（现有）+ `app-tauri`（新增），各自独立构建与打包 | 不合并、不替换、不做运行时切换 |
| Tauri Windows target | `x86_64-pc-windows-msvc` | 不为复用 gnu 工具链而妥协 |
| Tauri 产物 | **单 exe**（`tauri build --no-bundle`），WebView2Loader 静态链接 | 不发 NSIS/MSI 安装器、不带 DLL、不带 resources |
| GUI/CLI 双模式 | 保留 @03.1 的 `classify()` 语义，Tauri 版同样双模式 | 不引入 `tauri-plugin-single-instance` |
| 崩溃日志 | `file:line:col` + 崩溃前 N 条日志，**不依赖任何符号文件** | 不做 pdb 内嵌、不做离线符号化工具、不做 minidump |
| 许可 | Slint 版 `GPL-3.0-only`（被 Slint 传染），Tauri 版 `Apache-2.0` | — |

三条最容易踩错的结论，先记住：

1. **Tauri 版必须是 msvc**：`windows-gnu` 下 `WebView2Loader.dll` 是动态链接，exe 旁边得跟一个 DLL，直接破坏"单文件"。
2. **不要照抄 Tauri 模板的 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`**：它会让 CLI 模式没有 stdout、cmd 也不等，直接废掉双模式（@4）。
3. **崩溃日志不需要符号**：`PanicHookInfo` 的 `Display` 自带 `panicked at <file>:<line>:<col>:`，是编译期常量，与 pdb / strip / LTO 全部无关（@5.2 有实测）。

## 2. 双 GUI：仓库布局

### 2.1 crate 与产物

```
crates/
  core/        不动（Apache-2.0）—— 锁、解析、校验、导出、崩溃日志
  app-cli/     不动（Apache-2.0）—— 加一个 dispatch()，见 @4.1
  app-slint/   现有 GUI（GPL-3.0-only），windows-gnu + crt-static，Win7 tier3 保留
  app-tauri/   新增 GUI（Apache-2.0），windows-msvc，单文件
```

| 产物 | 来源 | 目标系统 | 说明 |
| --- | --- | --- | --- |
| `tablet.exe` | `app-slint` | Win7 及以上 | 现状不变，零依赖，符号内嵌 |
| `tablet-tauri.exe` | `app-tauri` | Win10 1803+ | 单文件，依赖系统 WebView2 运行时 |
| `tablet-cli.exe` | `app-cli` | 同 @09 | 两份 GUI 共用，CI / Jenkins 用 |

要求：

- 两个 GUI 的 exe **文件名必须不同**，允许同机共存；`dist/` 分目录：`dist/windows-slint/`、`dist/windows-tauri/`。
- 工作区锁（`<workdir>/.data/tablet.lock`，`tablet_core::workspace_lock`）由 core 独占，两个 GUI 走同一把锁：同项目互斥、不同项目可并行。**不要加 `tauri-plugin-single-instance`**，那会把"多项目并行"锁死。
- 两套 GUI 写出的 `.tbl` / `.xlsx` 必须完全一致——它们共用 core，这点天然成立。

### 2.2 唯一值得共享的是 view model（待拍板）

`app-slint` 的表格已经是"Rust 算好数据、.slint 只画"的形态（`HeaderCell` / `DataCell` / `DataRow`，见 `ui/components/grid_area.slint`），构造逻辑在 `app-slint/src/convert/grid.rs`。

建议把这三个结构 + 选择集 / 撤销栈中**与 UI 无关的部分**提到共享层（`core` 或新 `crates/ui-model`）并加 `serde::Serialize`，让 Tauri 版直接复用同一份数据模型。**否则 `ui/grid.rs`(≈890 行) + `convert/grid.rs`(≈690 行) 这块最硬的逻辑要养两份，半年内必然分叉。**

> 状态：**未拍板，不阻塞 @3–@5**。若接手时该决定仍为空，先按"两套各自实现"推进 P0–P2，等表格功能对齐后再评估是否上提。

### 2.3 许可

`app-slint` 因静态链接 Slint 的 GPLv3 分支而整体 GPL-3.0-only（@09 末节）。`app-tauri` 只依赖 core（Apache-2.0）+ MIT/Apache 生态，**必须标 `Apache-2.0`**，并在 `LICENSE.md` 的 crate 授权表加一行。这是双 GUI 最实际的收益之一（对外可分发、不传染）。

## 3. Tauri 版单文件打包

### 3.1 目标形态

一个 exe 双击即用：前端资源编译期内嵌、`WebView2Loader` 静态链接、无旁挂文件。

```
cargo tauri build --no-bundle        # 或 tauri.conf.json 里 "bundle": { "active": false }
# 产物：target/x86_64-pc-windows-msvc/release/tablet-tauri.exe
```

要求与约束：

| 项 | 规定 |
| --- | --- |
| target | `x86_64-pc-windows-msvc`（官方只支持 MSVC；gnu 会多一个 DLL） |
| `strip` | 保持 `"debuginfo"`；pdb 不归档、不随包发 |
| `bundle.resources` | 必须为空 |
| `externalBin`（sidecar） | 禁用 |
| `webviewInstallMode` | `skip`（不装安装器，也就谈不上写这个字段；**不要用 `fixedRuntime`**，那会多出 ~180MB 目录） |
| `identifier` | 定一次不再改（决定 WebView2 user data 目录） |
| 前端构建 | 由 `beforeBuildCommand` 负责；CI 需装 Node |

### 3.2 允许落盘的位置（不影响"单文件"）

- WebView2 user data：`%LOCALAPPDATA%\<identifier>`（Tauri 默认）。**不要**改成 exe 同级目录——那样 exe 放只读目录/U 盘就跑不起来。
- 业务日志与崩溃日志：仍在 `workdir`（`tablet.log` / `tablet-crash.log`），与 exe 位置无关。

### 3.3 平台地板

| 系统 | Tauri 版 | 说明 |
| --- | --- | --- |
| Win11 / Win10 1803+ | ✅ | WebView2 随 OS 分发 |
| Win10 早期 / Server 2016–2019 | ⚠️ | 未预装 WebView2，需人工装运行时（本项目不发安装器，只能文档提示） |
| Win7 / 8.1 | ❌ 走 Slint 版 | WebView2 已冻结在 109，且与单文件目标冲突 |

对外口径：**Tauri 版地板 Win10 1803+，Win7 用户用 `tablet.exe`。**

## 4. GUI + CLI 双模式

### 4.1 启动分流

`app-slint/src/main.rs` 的 `classify()`（零参数 → GUI；`--gui [--workdir=]` → GUI；其它 → CLI）**语义一字不改**，Tauri 版复用。

建议下沉到 `app-cli`：

```rust
// crates/app-cli/src/dispatch.rs
pub fn dispatch(launch_gui: fn(Option<PathBuf>) -> anyhow::Result<()>) -> ! { /* classify + 分流 + 退出码 */ }
```

两个 GUI 的 `main.rs` 各剩 4~5 行。CLI 分支必须 `std::process::exit(code)`，且**在 `tauri::Builder` 之前**。

```rust
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match classify(&args) {
        Route::Gui { workdir } => { detach_console_for_gui(); run_gui(workdir) }
        Route::Cli => { /* tablet_cli::run_with_args → process::exit(code) */ }
    }
}
```

### 4.2 控制台语义（必须保持）

现状：**console 子系统**（三端统一），GUI 分支 `FreeConsole()` 释放控制台，代价是 <50ms 黑闪。这套方案保留，Tauri 版照搬。

- ❌ **删除** Tauri 模板的 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`。带上它，`tablet-tauri.exe --help` / `validate` 在 cmd 里既不等也没输出。
- 不用 `AttachConsole` 方案（那是"GUI 子系统 + 事后挂控制台"的反向做法，本项目已选定 console 子系统）。
- `detach_console_for_gui()` 在 Windows 上 `FreeConsole`，其它平台空实现。
- GUI 分支释放控制台后 stderr 无人接收 —— 这正是崩溃日志必须落文件的原因（@5）。

### 4.3 退出码

CLI 分支的退出码必须原样透传（Jenkins 依赖）。clap 错误走 `clap_error.exit_code()`，其它错误 `1`。**不允许**在 `tauri::Builder` 的回调里 `exit`。

## 5. 崩溃日志规格

### 5.1 目标

**能定位到出问题的代码位置即可**，不要求完整堆栈、不要求符号文件。

据此，`file:line:col` 是主证据，堆栈是可选增强，符号化整条链路不做（@6 说明何时才需要回头做）。

### 5.2 为什么不需要符号（实测）

`rustc -O -C strip=symbols`（模拟"无符号单文件"）跑同样的 hook：

```
--- Display {info} ---
panicked at panic_probe.rs:18:14:
index out of bounds: the len is 0 but the index is 3
--- location() ---
Some(Location { file: "panic_probe.rs", line: 18, column: 14 })
--- Backtrace::force_capture() ---
   0: <unknown>
   1: <unknown>
   ...（共 18 行全是 <unknown>）
```

结论：

- `PanicHookInfo` 的 `Display` 已包含 `panicked at <file>:<line>:<col>:`；`location()` 单独可取。**两者都是编译期常量**，不吃 pdb、不吃 strip、不吃 LTO。
- 现有 `install_crash_hook` 里那句 `writeln!(out, "{info}")` 其实已经在写位置了。现有测试 `assert!(text.contains("main.rs"))` 之所以过，**靠的是 location，不是 backtrace**。
- 无符号时 `Backtrace::force_capture()` 输出十几行 `<unknown>`，纯噪音，还要在崩溃路径上白跑一次符号化 → 见 @5.5 条件化。

### 5.3 日志格式（字段级）

落 `<workdir>/tablet-crash.log`（沿用 `tablet_core::CRASH_LOG_FILE`），**append 打开、不截断**（历史崩溃必须保留）。

```
===== [2026-11-01 22:30:11] panic =====
app      : tablet-tauri
version  : 2.0.2 (x86_64-pc-windows-msvc)
built    : 2026-11-01T14:02:33Z
os       : Windows 10.0.22631
thread   : main
位置     : crates/app-tauri/src/ui/grid.rs:412:17
消息     : 保存 group slg-test 时 schema 缺失: index out of bounds: the len is 0
--- 崩溃前最后 20 条日志 ---
[2026-11-01 22:30:08.412] INFO  加载 project slg-test
[2026-11-01 22:30:09.001] DEBUG 选中 node Weapon
...
--- backtrace（仅符号可用时输出） ---
   0: tablet_tauri::ui::grid::save
   ...
```

字段规定：

| 字段 | 来源 | 备注 |
| --- | --- | --- |
| 时间 | `time::OffsetDateTime::now_utc().to_offset(time_offset)` | 沿用现有格式 |
| app | 调用方传入（`"tablet"` / `"tablet-tauri"`） | 两份日志混发时能区分 |
| version | `env!("CARGO_PKG_VERSION")` | 与 `[workspace.package] version` 一致 |
| target | 构建期注入，见 @5.4 | 区分 gnu / msvc / win7 |
| built | 构建期注入 | 同一版本号多份构建时用于核对 |
| os | 运行时取 | 至少含版本号 |
| thread | `std::thread::current().name()` | 缺省 `<unnamed>` |
| 位置 | `info.location()` | **`file:line:col`，核心字段** |
| 消息 | `payload` downcast（`&str` / `String`） | 非字符串 payload 写 `<非字符串 payload>` |
| 前 N 条日志 | 读 `tablet.log` 尾部，默认 20 行 | @5.6 |

### 5.4 实现骨架

新增 `crates/core/src/crash.rs`（两套 GUI 共用，替代 `app-slint/src/main.rs` 里现有的 `install_crash_hook`）：

```rust
pub struct CrashLog {
    app: &'static str,
    workdir: PathBuf,
    log_path: PathBuf,   // tablet.log
    time_offset: time::UtcOffset,
}

impl CrashLog {
    pub fn new(app: &'static str, workdir: &Path, log_path: &Path, off: time::UtcOffset) -> Self;
    /// 设置 panic hook，append 写 `workdir/tablet-crash.log`
    pub fn install(self);
}
```

硬性约束：

- **hook 里禁止用 `log::` 宏**：可能正持有日志锁，重入即死锁。读日志文件用 `std::fs::read_to_string`。
- 打开崩溃日志失败时只 `log::warn!` 一次并返回，**不能让安装 hook 本身 panic**。
- 写完后 `flush`；不要提前 `return` 漏掉 `default_hook`（开发期仍需在终端看到）。
- 行数/字节双上限（默认 20 行 / 8KB），单行长度截断（例如 512 字符），避免把一整个 schema 灌进日志。

构建期注入（`build.rs`）：

```rust
println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());
println!("cargo:rustc-env=BUILD_TIME={}", /* ISO8601 UTC */);
```

### 5.5 backtrace 条件化

```rust
let bt = std::backtrace::Backtrace::force_capture().to_string();
if !bt.contains("<unknown>") {
    let _ = writeln!(out, "--- backtrace ---\n{bt}");
}
```

语义：本机 / QA（有符号或用 dev profile）能拿到栈；用户机（单文件无符号）不写噪音。

### 5.6 崩溃前 N 条日志

崩溃日志只回答"崩在哪一行"，不回答"用户为什么走到这"。补上最后 20 条操作日志，性价比最高的一步。

```rust
fn tail(path: &Path, max_lines: usize, max_bytes: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let slice = &lines[lines.len().saturating_sub(max_lines)..];
    // 逐行截断到 512 字符后拼接，总长截到 max_bytes
}
```

不读整个文件也行（按块从尾部读），但本项目日志量小，整读 + 截断即可。

### 5.7 `expect` 化与 lint

`unwrap()` 的 location 指向 unwrap 那一行、消息是 `called Option::unwrap() on a None value`——能定位但没语义。

- 关键路径（保存 / 加载 / 导出 / 表格编辑）把 `unwrap()` 换成 `expect("业务上下文")`，例如
  `expect("保存 group 时 schema 缺失")`。日志一句话就能复现。
- 在 `core` / `app-*` 的关键模块开 `clippy::unwrap_used = "warn"`，兜住新增。

## 6. 明确不做（及触发条件）

| 不做 | 理由 | 什么时候才做 |
| --- | --- | --- |
| pdb 内嵌进 exe | pdb 是链接后才生成的，要两阶段构建 + PE overlay + 调整签名顺序，成本远超收益 | 只有"用户机器上必须直接拿到完整栈"成为硬需求时 |
| 离线符号化工具（`pdb-addr2line` 等） | `file:line:col` 已经够定位 | 同上；或需要看第三方库内部帧时 |
| minidump / `SetUnhandledExceptionFilter` | 只覆盖非 panic 崩溃（AV / 栈溢出 / abort / OOM） | 线上反馈里"无崩溃日志的闪退"占比变高时 |
| JS 侧错误回灌（`window.onerror` / `unhandledrejection`） | 只覆盖 webview 内的错误 | Tauri 版进入实际使用后第一轮反馈 |
| `tauri-plugin-single-instance` | 与"多项目并行"冲突 | 永不 |

**已知盲区（接受）**：原生崩溃与 webview 渲染进程崩溃没有 `location()`，这类闪退只能靠"崩溃前最后 N 条日志"缩小范围。

## 7. 验收清单

按顺序执行，全绿才算完成。

1. `cargo test --workspace` 通过；`crash` 模块至少有两条测试：
   - append 语义（连续两次 panic，两条都在文件里）
   - 位置字段（日志包含 `<file>:<line>:<col>`）
2. 新增隐藏参数 `--crash-selftest`（两份 GUI 都支持）：**故意 panic**，用于在任何机器（含 release 单文件）上验证崩溃日志链路。执行后检查 `<workdir>/tablet-crash.log` 含：
   - `app` / `version` / `target` / `位置` 四行
   - `--- 崩溃前最后 20 条日志 ---`
   - 无符号包里**不出现** `--- backtrace ---` 段
3. Tauri 版：`cargo tauri build --no-bundle` → 产物目录里**只有一个 exe**。
4. 干净虚拟机（无 VS / 无 rust / 无 cargo）双击该 exe 能起窗口 → 证明 WebView2Loader 静态 + 前端资源内嵌。
5. `tablet-tauri.exe --help` 与 `tablet-tauri.exe validate` 在 cmd 中输出完整、退出码正确（`echo %ERRORLEVEL%`）。
6. 同机跑两个实例、开两个不同项目 → 均正常；同一项目开第二个 → 被锁拒绝（文案同 Slint 版）。
7. Slint 版行为零变化：`tablet.exe` 的 GUI / CLI 双模式、崩溃日志格式与 Tauri 版一致。

## 8. 移交计划

| 阶段 | 内容 | 交付 |
| --- | --- | --- |
| P0 | `core/crash.rs` + 测试 + `--crash-selftest` | 不改任何现有行为；`cargo test` 绿 |
| P1 | `app-slint` 切到新 API，删掉本地 `install_crash_hook` | Slint 版日志格式升级，行为等价 |
| P2 | `app-cli::dispatch` 下沉 | 两份 GUI 的 `main.rs` 各 ≤ 10 行 |
| P3 | `app-tauri` 骨架：窗口 + 空表格 + 双模式 + 单文件打包 | @7 的 3–5 条验完 |
| P4 | （视 @2.2 拍板）view model 上提 | 表格逻辑单份实现 |

P0–P3 之间没有硬依赖颠倒，可并行；但 **P3 的崩溃日志接入依赖 P0**。

## 9. 工程约束（继承 AGENTS.md）

- 验证走 dev profile（`cargo check` / `cargo build` / `cargo test`），**不要 `--release`**；release 只在最终交付。
- **不要自动 `git commit`**。改完报告「改了什么 + 验证结果 + 需要人工验收的界面/操作」，停下等验收。
- 注释与文档用中文，标识符英文；新增 `.rs` 文件带 SPDX 头（`app-tauri` 用 `Apache-2.0`）。
- 新增本文后，已在 `docs/00-概述.md` §4 索引表登记；后续若拆分文档，同步更新索引。
