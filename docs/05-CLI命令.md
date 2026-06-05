# CLI 命令

工具源码的层级划分、`tbl-cli` 二进制的命令骨架、子命令完整列表与覆盖键。所有"用户视角的核心能力"（导出 / 验证 / 测试数据生成 / 项目管理）的语义在 @04，本文聚焦**怎么从命令行驱动这些能力**。

GUI 复用同一份 `actions/`，行为完全一致；GUI 实现见 @06 / @07。

## 1. 工具源码三层

整体三层（自下而上）：

```
layer 3 (UX 顶层)     tbl-slint.exe    零参数 / --gui → GUI；其它参数 → 转给 tbl-cli lib
                              │ (depends on)
layer 2 (脚本/Jenkins) tbl-cli.exe  +  tbl-cli (lib)
                              │ (depends on)
layer 1 (基础设施)     tbl-core (rlib)  ← 后期可加 cdylib + C ABI 给第三方集成
```

```
tbl-tool/
├── Cargo.toml                  ← workspace
└── crates/
    ├── core/                   ← 核心库（无 UI 依赖）
    │   ├── src/
    │   │   ├── model.rs        ← Project / Group / Table / ...（@02.5）
    │   │   ├── project.rs      ← ProjectEngine（按 project root 加载）
    │   │   ├── types.rs        ← TblType + 分隔符 SepKey 枚举
    │   │   ├── ops.rs          ← 数据操作（增删改）
    │   │   ├── search.rs       ← name_matches: 字面 + 拼音首字母模糊匹配（@04.2.3）
    │   │   ├── parse/          ← .tbl / .tblschema 解析
    │   │   ├── serialize/      ← 序列化
    │   │   ├── validate/       ← cell / row / schema / project 四层验证
    │   │   ├── template/       ← 项目模板（TemplateSource / Builtin / Local / instantiate）
    │   │   ├── export/         ← JSON / XML / Java / Go / Lua
    │   │   └── excel.rs        ← xlsx 互转
    │   ├── templates/          ← Java/Go/Lua 模板源（生成代码用）
    │   └── schemas/            ← 内置 .tblschema（include_str! 嵌入；@04 项目模板 / @10 测试驱动共用）
    │
    ├── app-cli/                ← CLI lib + bin 双输出（产物 tbl-cli + libtbl_cli）
    │   └── src/
    │       ├── lib.rs          ← pub mod actions; pub mod cli; re-export run_with_args
    │       ├── actions/        ← 业务编排层：GUI / CLI 共用，无 println / 无 clap / 无 process::exit
    │       │   ├── export.rs        ← run_export → ExportSummary
    │       │   ├── validate.rs      ← run_validate → ValidationSummary
    │       │   ├── new_project.rs   ← run_new_project → NewProjectOutcome
    │       │   ├── generate_test.rs ← run_generate_test → GenerateTestSummary
    │       │   ├── migrate.rs       ← run_migrate
    │       │   ├── list_templates.rs / list_projects.rs
    │       │   └── overrides.rs     ← apply_overrides + ensure_* 私有 helper
    │       ├── cli/            ← 仅 CLI 二进制用：clap 派生 / stdout 打印 / exit code
    │       │   ├── args.rs          ← Cli + Commands（clap derive）
    │       │   ├── output.rs        ← print_*_cli（后缀 `_cli` = 仅 CLI）
    │       │   └── dispatcher.rs    ← run_with_args(&[String]) -> Result<i32>
    │       └── main.rs         ← ~10 行：转发到 dispatcher，把 Result<i32> → process::exit
    │
    └── app-slint/              ← Slint GUI（产物 tbl-slint，@07）
        └── src/main.rs         ← 头部分流：classify(args) → Route::Cli 走 tbl_cli::run_with_args
                                  零参数 / --gui [--workdir] 走 run_gui()
                                  Windows 下 GUI 分支 FreeConsole() 释放 console（双击启动后无黑窗驻留）
```

`core` 不依赖任何 UI 框架，所有前端共享同一份模型与验证逻辑。

**模块路径就是契约**——`tbl_cli::actions::*` = GUI 可复用；`tbl_cli::cli::*` 或后缀 `_cli` 的函数 = 仅 CLI 二进制内部用，GUI 不应引用。新加业务 → 放 `actions/`，签名干净返 `Result<某种 Summary>`；新加屏幕输出 → 放 `cli/output.rs`，函数名带 `_cli` 后缀。

`core/src/template/` 模块同时被项目模板库（@04）和 generate-test CLI（@04 测试数据生成）复用。

**未来扩展：core 可加 cdylib + C ABI** —— 当前 `tbl-core` 的 `pub` API 已按"可序列化 / 类型稳定"约束维护，后期若要把核心能力暴露给第三方程序（如 Unity 编辑器插件、定制 IDE 集成），只需追加 `[lib] crate-type = ["rlib", "cdylib"]` 与一份 `ffi.rs` + cbindgen 生成的 .h，无需改动现有调用方。

## 2. 命令骨架

```
tbl-cli [-w <workdir>] [--project <id>] [-s key=value]... <subcommand>
```

| 参数 | 作用 | 默认值 |
|------|------|--------|
| `-w, --workdir <path>` | 仓库根目录（`projects/` 的父） | `.`（当前目录） |
| `--project <id>` | 显式指定 Project id；覆盖 `[app] last_project` | 不指定 = 跟随 `[app] last_project`，再不存在则取扫描结果第一个 |
| `-s, --set KEY=VALUE` | 覆盖 `tbl-tool.toml` 任意配置项；可重复 | — |

`-s` 覆盖键名与 toml 路径一一对应，列表见 §4。未知 / 废弃 / 格式错误的 key 会以 warning 形式打到 stderr，但不会让命令失败。

## 3. 子命令

按是否需要先加载 Project 分两组。

### 3.1 不需要加载 Project

| 子命令 | 作用 | 输出 |
|--------|------|------|
| `list-templates` | 列出可用模板（内置 + `<workdir>/tblschema/` 本地模板库） | id / name / version / source 表 |
| `list-projects` | 列出 `<workdir>/projects/` 下所有 Project（id / name / created_at / source_template） | 表 |
| `migrate-legacy` | 把根目录 `config/` + `project.tblschema` 迁移到 `projects/default/` | 迁移结果摘要；@04 老结构迁移 |
| `new-project --template <id> --id <pid> [--name <n>] [--switch-after <bool>]` | 用模板新建 Project | 新建后的 `project_root` 路径 |

`new-project` 参数：

| 参数 | 说明 | 默认 |
|------|------|------|
| `--template <id>` | 模板 id（来自 `list-templates`），必填 | — |
| `--id <pid>` | Project id，约束 `[a-z0-9_-]{1,32}` | — |
| `--name <n>` | Project 显示名 | = id |
| `--switch-after <bool>` | 创建后写入 `[app] last_project` | true |

### 3.2 需要加载 Project

> 这些子命令先按 `--project` / `[app] last_project` / 扫描第一个的优先级选 Project，加载后才执行。

#### `export [--json] [--xml] [--java] [--go] [--lua]`

不带任何 flag = 全格式导出；指定其中一项或多项 = 仅导出这些格式。每项产物的具体路径派生规则见 @02.4 与 @04 各导出章节。

退出码恒为 0；单格式失败仅 eprintln 错误，不影响其它格式继续，便于 CI 收集。

#### `validate`

全项目离线校验：cell / row / schema / project 四层（@01 附录 A）。

- 通过 → 退出码 0
- 任一节点失败 → 退出码 1，错误明细打到 stderr

#### `generate-test [...]`

灌测试数据，详细规则见 @04 测试数据生成。参数：

| 参数 | 说明 | 默认 |
|------|------|------|
| `--empty` | 在测试数据里穿插空值列样本，覆盖 schema 的可空场景 | false |
| `--schema <path>` | 用外部 `.tblschema` 文件代替 Project 内的 schema | — |
| `--rows <n>` | 数据行数；`0` 表示走内置固定数据 | 0 |
| `--seed <u64>` | 随机种子；`0` 表示固定数据，非 0 启用伪随机 | 0 |
| `--format json\|xml` | TestMain 的初始化方式 | json |
| `--lang java\|go\|none` | 生成 TestMain 的语言；`none` 表示只生成数据不生成 TestMain | java |

退出码 0 = 成功；I/O / 解析失败 → 抛错由 main 翻成 1。

## 4. `-s` 覆盖键

`apply_overrides` 当前认识的键（实现见 `crates/app-cli/src/actions/overrides.rs`）：

| 键 | 覆盖到 |
|----|--------|
| `app.last_project` / `project.last_project` | `[app] last_project` |
| `app.config_dir` / `project.config_dir` | `[project] config_dir`（兼容历史命名） |
| `app.cache_dir` / `project.cache_dir` | `[project] cache_dir`（兼容历史命名） |
| `export.encoding` | `[export] encoding` |
| `export.line_ending` | `[export] line_ending` |
| `export.json.empty_as` | `[export.json] empty_as` |
| `export.xml.empty_as` | `[export.xml] empty_as` |
| `export.server.data_output` | `[export.server] data_output` |
| `export.server.package` / `export.server.java.package` | `[export.server.java] package`（兼容历史命名） |
| `export.server.code_output` / `export.server.java.code_output` | `[export.server.java] code_output`（兼容历史命名） |
| `export.server.go.package` | `[export.server.go] package` |
| `export.server.go.code_output` | `[export.server.go] code_output` |
| `export.client.lua.output` / `export.client.output` | `[export.client.lua] output`（兼容历史命名） |

废弃键：`export.server.lang`（@02.3 拆分了 java/go 后不再使用），命中后只 warning 不写入。

`-s` 覆盖**仅作用于本次进程内存中的 `Project.config`**，不写回 toml；适合 Jenkins 临时切输出目录、CI 切编码等。toml 完整字段说明见 @02.3。

## 5. 退出码

| 码 | 含义 |
|----|------|
| 0 | 成功（包括 `validate` 全过 / `export` 单格式失败但整体收尾） |
| 1 | `validate` 不通过 / 解析 schema 失败 / 加载 Project 失败 / 其它 I/O |

Jenkins 脚本的失败判断完全依赖此码。

## 6. 依赖与体积

`tbl-cli` 仅依赖 `tbl-core` + `clap`，不引任何 UI 渲染依赖；release 体积约 1 MB（不开 LTO）。lib 形态（`libtbl_cli`）由 `tbl-slint` 在 CLI 分支直接调用，等价于跑了一次 `tbl-cli` 子进程，省掉一次 fork。

## 7. tbl-slint 的 CLI 分流

`tbl-slint.exe` 启动时先做 argv 分类：

```rust
fn classify(args: &[String]) -> Route {
    if args.len() <= 1 { return Route::Gui { workdir: None }; }
    if !args.iter().skip(1).any(|a| a == "--gui") { return Route::Cli; }
    // --gui 模式下顺便解析 --workdir=foo / --workdir foo
    ...
    Route::Gui { workdir }
}
```

| 启动方式 | 行为 |
|---------|------|
| 零参数（双击 / `tbl-slint.exe`） | GUI，workdir = exe 所在目录（即 cwd） |
| `tbl-slint --gui [--workdir=path]` | GUI，workdir 显式指定（开发期 `cargo run -p tbl-slint -- --gui --workdir=...`） |
| 其它任何参数（含 `--help` / 子命令 / `-w` / `-s`） | CLI：剔除 `--gui` 后转发给 `tbl_cli::run_with_args` |

CLI 分支和直接跑 `tbl-cli.exe` 行为完全一致；GUI 分支在 Windows 下额外调 `FreeConsole()` 释放 console，避免双击启动后黑窗驻留。

## 8. 用法示例

```bash
# 在指定仓库下导出全部格式
tbl-cli -w D:/work/game-config export

# 仅导出 Java 模板类，临时切输出目录
tbl-cli -w D:/work/game-config \
  -s export.server.java.code_output=tmp/java \
  export --java

# 离线校验某个 Project
tbl-cli -w D:/work/game-config --project slg-prod validate

# 用模板新建 Project
tbl-cli -w D:/work/game-config new-project \
  --template slg-base --id slg-2026 --name "SLG 2026"

# 灌 100 行带空值的测试数据，go 后端
tbl-cli -w D:/work/game-config generate-test \
  --rows 100 --empty --seed 42 --lang go

# Jenkins 风格：通过 tbl-slint.exe 跑 CLI（同一个 exe 兼任）
tbl-slint.exe -w D:/work/game-config validate
```
