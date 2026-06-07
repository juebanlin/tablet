# CLI 工程

`tablet` 工程的"how"层：源码三层的抽象划分、仓库目录布局、`tablet.toml` 配置、Git 协作约定、`tablet-cli` 二进制的命令骨架与子命令清单。

核心层（`tablet-core`）暴露的能力（导出 / 导入 / 验证 / Project / 模板 / 测试数据生成）的语义在 @02；本文聚焦**工程怎么组织代码、仓库怎么放文件、命令行怎么调用**。GUI 共享同一份 `actions/` 业务编排层，行为完全一致，UI 设计见 @04，Slint 实现细节见 @05。

## 1. 工具源码三层

整体三层（自下而上）：

```
layer 3 (UX 顶层)     tablet.exe    零参数 / --gui → GUI；其它参数 → 转给 tablet-cli lib
                              │ (depends on)
layer 2 (脚本/Jenkins) tablet-cli.exe  +  tablet-cli (lib)
                              │ (depends on)
layer 1 (基础设施)     tablet-core (rlib)  ← 后期可加 cdylib + C ABI 给第三方集成
```

```
tablet/
├── Cargo.toml                  ← workspace
└── crates/
    ├── core/                   ← 核心库（无 UI 依赖）
    │   ├── src/
    │   │   ├── model.rs        ← Project / Group / Table / ...（@02.25）
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
    │   └── schemas/            ← 内置 .tblschema（include_str! 嵌入；@02 项目模板 / @08 测试驱动共用）
    │
    ├── app-cli/                ← CLI lib + bin 双输出（产物 tablet-cli + libtablet_cli）
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
    └── app-slint/              ← Slint GUI（产物 tablet，@05）
        └── src/main.rs         ← 头部分流：classify(args) → Route::Cli 走 tablet_cli::run_with_args
                                  零参数 / --gui [--workdir] 走 run_gui()
                                  Windows 下 GUI 分支 FreeConsole() 释放 console（双击启动后无黑窗驻留）
```

`core` 不依赖任何 UI 框架，所有前端共享同一份模型与验证逻辑。

**模块路径就是契约**——`tablet_cli::actions::*` = GUI 可复用；`tablet_cli::cli::*` 或后缀 `_cli` 的函数 = 仅 CLI 二进制内部用，GUI 不应引用。新加业务 → 放 `actions/`，签名干净返 `Result<某种 Summary>`；新加屏幕输出 → 放 `cli/output.rs`，函数名带 `_cli` 后缀。

`core/src/template/` 模块同时被项目模板库（@02）和 generate-test CLI（@02 测试数据生成）复用。

**未来扩展：core 可加 cdylib + C ABI** —— 当前 `tablet-core` 的 `pub` API 已按"可序列化 / 类型稳定"约束维护，后期若要把核心能力暴露给第三方程序（如 Unity 编辑器插件、定制 IDE 集成），只需追加 `[lib] crate-type = ["rlib", "cdylib"]` 与一份 `ffi.rs` + cbindgen 生成的 .h，无需改动现有调用方。

## 2. 仓库布局

```
game-config/
├── projects/                   ← 全部 Project，按 id 分目录
│   ├── slg-test/               ← 一个 Project
│   │   ├── project.toml        ← Project 元数据（[project]）+ 可选配置覆盖（[export]/[ui]）
│   │   ├── project.tblschema   ← 该 Project 的 schema（多表合并）
│   │   ├── config/             ← .tbl 文件，Group = 子目录
│   │   │   ├── hero/
│   │   │   │   ├── HeroBase.tbl       ← Table
│   │   │   │   ├── HeroSkill.tbl      ← Table
│   │   │   │   ├── HeroConst.tbl      ← Constant
│   │   │   │   └── HeroType.tbl       ← Enum
│   │   │   ├── item/
│   │   │   │   ├── ItemBase.tbl
│   │   │   │   └── ItemDrop.tbl
│   │   │   └── global/
│   │   │       └── GlobalConst.tbl    ← Constant
│   │   └── .tbl-cache/         ← 临时 xlsx + .tbl.tmp，gitignore（§6、@06）
│   └── slg-prod/...            ← 另一个 Project
├── tblschema/                  ← 本地模板库（可选）
│   ├── slg-base.tblschema
│   └── rpg-base.tblschema
├── gen/                        ← 生成产物，详见 @02.23
├── tablet.toml               ← 工具配置，详见 §4
└── .gitignore                  ← 详见 §6
```

`projects/<id>/` 自包含一个 Project 全部所需（schema + 数据 + 元数据 + Excel 缓存）。同一仓库可并存多个 Project，启动时 UI 默认进入 `[app] last_project` 指定的那个；不存在则进入扫描结果中第一个。

老仓库的根目录 `config/` 在新版本启动时**自动迁移**到 `projects/default/`（@02 Project / 老结构迁移），不保留双结构。

## 3. Group 与 Excel 的对应关系

- 一个**文件夹** = 一个 **Group** = 一个 **Excel 文件**
- 文件夹内每个 `.tbl` 文件 = 一个 **Table / Constant / Enum** = Excel 中的一个 tab
- 用 Excel 编辑时整个 Group 生成一个 xlsx，每个配置项对应一个 tab（详见 @06）

Group 名仅在工具内部使用，不进生成代码路径，因此允许中文（@01.8.1、@02.1）。

## 4. tablet.toml 配置

工具配置文件，定义启动展开偏好、导出目标、UI 行为、类型分隔符默认值。所有字段都有默认值，新建项目可只写少量字段。**`tablet.toml` 是仓库全局配置，所有 Project 共享；Project 自身的元数据 + 可选配置覆盖放在 `projects/<id>/project.toml`（@02 Project）**。Project 的 `[export]` / `[ui]` 段按 field-level deep-merge 覆盖全局；缺失字段回退到全局。

`[separators]` 是例外：**不参与 deep-merge**，仅在「新建空项目」时拷贝到新项目 `.tblschema` 作为初值；运行期分隔符以项目自身 schema 的 `# @sep` 行为单一来源（@01.7.4 / §4.9）。

### 4.1 完整示例

```toml
[app]
last_project = "slg-test"     # 启动时进入的 Project id；为空 = 扫到的第一个

[export]
encoding = "utf-8"
line_ending = "lf"

[export.json]
empty_as = "null"     # null / omit

[export.xml]
empty_as = "empty"    # empty / omit

[export.server]
data_output = "gen/server/data"

[export.server.java]
package = "com.game.config"
code_output = "gen/server/java"

[export.server.go]
package = "config"
code_output = "gen/server/go"

[export.client.lua]
output = "gen/client"

[ui]
auto_commit_on_blur = true
realtime_validate = false
log_level = "debug"
picker_trigger_header = "single"
picker_trigger_data = "double"
show_meta_id = false           # 模板/Project 列表展示：false→显示 name；true→显示 id

[separators]                   # 程序级默认分隔符；仅作新建空项目种子（详见 §4.9）
Tuple2 = ","
Tuple3 = ","
Tuple4 = ","
List = ";"
Set = ";"

[separators.Map]
kv = ":"
entry = ";"

[separators.List_Tuple2]
tuple = ","
list = ";"

[separators.Map_Tuple2]
kv = ":"
tuple = ","
entry = ";"

[separators.Map_List]
kv = ":"
item = ","
entry = ";"
```

### 4.2 [app]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| last_project | 启动时**默认展开**的 Project id；不存在则展开扫描结果中第一个；其它 project 仍同时加载，仅折叠展示 | 空（首次进入）|

Project 列表由启动扫描 `projects/` 目录得出，**不需要**在 `tablet.toml` 里枚举。所有 project 同时加载到内存，`last_project` 仅影响 TreeSection 首屏展开状态（无切换概念）；新建 project 后写入 last_project 让下次启动直接展开。

> 历史命名：旧版本是 `[project] name / config_dir / cache_dir`；S15-D 把 `[project]` 段彻底改名为 `[app]`，且字段语义变为"全仓库级"——name/config_dir/cache_dir 已被 Project 内 `project.toml` + `projects/<id>/` 目录约定取代。

### 4.3 [export]（全局）

| 字段 | 说明 | 默认值 |
|------|------|--------|
| encoding | 生成文件编码 | utf-8 |
| line_ending | 换行符（lf / crlf） | lf |

子段（json/xml/server/client）可分别覆盖 `encoding` `line_ending`。

### 4.4 [export.json] / [export.xml]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| empty_as | json: `null` / `omit`； xml: `empty` / `omit` | json=null； xml=empty |

空值策略详见 @01.8.4。

### 4.5 [export.server]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| data_output | 后端数据文件目录（JSON/XML 共用） | — |

### 4.6 [export.server.java]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| package | Java 包名 | — |
| code_output | Java 模板类输出目录 | — |

### 4.7 [export.server.go]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| package | Go package | — |
| code_output | Go 代码输出目录 | — |

### 4.8 [export.client.lua]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| output | Lua 文件输出目录 | — |

### 4.9 [separators]

**程序级默认分隔符**（25 项 leaf 完整清单见 @01.7.4）。运行期数据来源是各 Project 的 `.tblschema` `# @sep` 行（@01.10.3），与本段解耦——本段**仅**在「新建空项目」时被读取一次，作为新项目 `schema.separators` 的初值；之后该项目运行期不再回查 toml。

| 场景 | 行为 |
|------|-----|
| 启动加载 | `tablet.toml [separators]` 反序列化为 `engine.default_separators`（toml 没写的字段走代码 default） |
| 新建空项目（GUI 对话框 / `tablet-cli new-project`） | 拷贝 `engine.default_separators` 给新项目 schema |
| 从模板/文件新建项目 | 继承 source schema 的 `separators`，与 toml 无关 |
| 已加载项目运行期校验/导出/示例值 | 用 `Project.config.separators`（来自该项目 schema） |
| 项目设置对话框改分隔符 | 写回项目 `.tblschema`，触发该项目 `revalidate_all`；与 toml 无关 |

`[separators]` 共 25 项 leaf（list / set / tuple2/3/4 / map.kv / map.entry / list_tuple{2,3,4}.{tuple,list} / map_tuple{2,3,4}.{kv,tuple,entry} / map_list.{kv,item,entry}）— 这套清单是**唯一真值源**，定义在 `crates/core/src/export/sep_meta.rs::sep_kv_pairs`，JSON `_sep` / XML `sep_*` attr / Go `SepConfig` / Java `SepConfig` 全部由此派生：

- 数据文件（JSON `_sep` / XML `sep_*` attr）：**按需输出** —— `paradigm_sep_keys` 列出每个范式用到的 leaf，扫表/常量字段 union 得到该文件需要的 key 集合，集合外的 leaf 不出。全 base 表 / 全 base + Ref 表（如纯 int/str/bool）`_sep` 整个省略。
- 生成代码（Go `templates/go/sep.go` / Java `templates/java/SepConfig.java`）：**保持 25 字段全量** —— 项目级共享一份 SepConfig 类，不同表用不同范式，按字段范式查 `SepConfig.listTuple2List` 等成员，不能裁剪。
- 消费端 `SepConfig.fillDefaults()`（Go）/ `getAny(..., default)`（Java）给缺失 key 兜底，所以数据文件省略 leaf 安全。

加新分隔符时，需要同步：(1) `SeparatorsSection` + serde rename；(2) `types.rs::SepKey` 枚举（加 variant + 4 个 match：`as_directive_key` / `as_export_key` / `get` / `set`，加进 `SepKey::ALL`）；(3) `Paradigm::sep_keys` 把新 leaf 接到对应范式；(4) Go / Java 模板里的 SepConfig 字段及对应 fromMap / fromXMLAttrs 加载；(5) 项目设置对话框 `project_settings.slint` 的 sep-\* property 与 SepRow。`sep_meta.rs` 的 invariant 测试 + `match` 编译期穷尽性锁死 (1)(2)(3) 一致性——漏改会编译失败或测试失败。

### 4.10 [ui]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| auto_commit_on_blur | 点击空白区域时自动保存当前编辑 | true |
| realtime_validate | 编辑单元格时实时验证（关闭时仅保存前验证） | false |
| log_level | 日志级别（debug / info / warn / error） | debug |
| picker_trigger_header | 表头 picker 单元格呼出方式：`single`（单击直出选择器）/ `double`（单击仅选中、双击呼出） | `"single"` |
| picker_trigger_data | 数据区 picker 单元格呼出方式：`single` / `double` | `"double"` |
| show_meta_id | 模板/Project 列表里显示 id（true）还是 name（false）；切换效果类似枚举显示 id/name | `false` |

picker_trigger 的两个旋钮独立。默认 header=single / data=double 是按使用场景挑的：
- 表头每列 type/export 各一格、几乎不批量改，单击直接弹更顺手
- 数据区 ref / enum cell 是 Ctrl+C/V 批量复制粘贴的主战场，保留单击 = "瞄准选中"才不踩脚
切到 single 模式后单击即弹，但选中态会被弹窗遮住，要批量复制只能先 Esc 关掉弹窗再点其它 cell；按这条体感建议保留默认。

show_meta_id 影响的位置：模板库列表（@04.6.6）、新建项目对话框选项（@04.6.7）、TreeSection 各 Project 根节点（@04.2.0）。Project 实际目录始终用 id；该开关只决定**显示文本**。

#### 4.10.1 [ui.ref_picker]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| default_strategy | Table 引用列展示默认策略：`auto`（id + 最多 2 辅助列） / `full`（除 export=- 外全部字段） | `"auto"` |

弹窗内可临时切换策略；每次重新打开都会回退到配置默认。Enum 引用永远固定 id/name/desc 三列，不受此配置影响。详见 @04.6.3。

#### 4.10.2 realtime_validate 行为

| 取值 | 单元格编辑时 | 保存时 |
|------|--------|--------|
| `false`（默认） | 不验证；红框只在保存校验失败后才出现 | 强制全量验证 |
| `true` | 每次单元格提交都立刻 revalidate 当前节点 | 同上 |

打开实时验证可以更早发现错误，但项目大时（>10 万条记录）每次按键提交都会触发 cell+row+schema 三层调用，可能感知到卡顿。校验逻辑详见 @01.8 与 @01 附录 A。

**与项目结构操作的关系**：新建 / 重命名 / 拷贝 / 删除节点 / 删除组等操作属于"项目结构变化"，**无论 `realtime_validate` 开关如何，都必然触发 `validation_errors` 索引的同步**（删除→清除残留 entries；重命名→平移 key；新建/拷贝→建立新索引）。这是为了保证 TreeSection 的 `!` 聚合标记不会因为索引滞留而错算。`realtime_validate` 只控制**单元格内容编辑**这条路径上的复算行为。

UI 上的视觉反馈（红框/树节点 `!` /日志框）见 @04.5.4。

## 5. 输出目录结构

`gen/` 在仓库根，与 Project 解耦——多 Project 共享同一份导出目录。各语言完整路径派生规则见 @02.23（产物**扁平化**，不保留 group 子目录）。

## 6. Git 协作

工具不做本地并发锁，完全依赖 Git 解决冲突。文本化 + 一行一记录的格式让大多数情况下能自动 merge。

### 6.1 冲突最小化策略

| 策略 | 效果 |
|------|------|
| 一行一记录 | 不同策划改不同行，Git 自动 merge |
| 按 Group 分文件夹 | 不同策划改不同表，无冲突 |
| 主键排序 | 行顺序稳定，不因排序产生 diff |
| schema 与数据同文件 | 减少文件数，简化管理 |
| 多 Project 物理隔离 | 不同 Project 互不影响，跨 Project 改动**永远**不会冲突 |

### 6.2 .gitignore

```gitignore
# Excel 桥接缓存：每个 Project 自带一份，全部忽略
projects/*/.tbl-cache/

# Excel 临时文件
*.xlsx~
~$*.xlsx

# 生成产物
gen/

# 进程锁
.tablet.lock
```

注意：`project.toml` / `project.tblschema` / `config/**/*.tbl` 都需要进版本控制——它们是 Project 的本体。

### 6.3 仓库结构（多 Project）

```
<repo-root>/
├── tablet.toml                 # 全局配置（进 git）
├── projects/                     # 全部 Project（进 git，除 .tbl-cache/）
│   ├── slg-test/
│   │   ├── project.toml
│   │   ├── project.tblschema
│   │   ├── config/
│   │   └── .tbl-cache/           # ← .gitignore
│   └── slg-prod/...
├── tblschema/                    # 本地模板库（可选进 git，按团队约定）
└── gen/                          # 生成产物（.gitignore）
```

是否把本地模板（`tblschema/`）进 git 由团队决定：
- 如果想让团队成员共享自定义模板 → 进 git
- 如果只是个人临时模板 → .gitignore

### 6.4 冲突场景

| 场景 | 概率 | 解决方式 |
|------|------|----------|
| 不同策划改不同 Project | 最常见 | 完全无冲突 |
| 同 Project 不同策划改不同表 | 常见 | 无冲突 |
| 同 Project 不同策划改同表不同行 | 常见 | Git 自动 merge |
| 同 Project 不同策划改同一行 | 极少 | Git 标记冲突，手动解决（文本格式易读） |
| schema 变更 | 极少 | 需协调，一人改 schema 后通知其他人 |
| 同时新建同名 Project | 极少 | 目录冲突，约定 project id 命名规则规避 |

## 7. 命令骨架

```
tablet-cli [-w <workdir>] [--project <id>] [-s key=value]... <subcommand>
```

| 参数 | 作用 | 默认值 |
|------|------|--------|
| `-w, --workdir <path>` | 仓库根目录（`projects/` 的父） | `.`（当前目录） |
| `--project <id>` | 显式指定 Project id；覆盖 `[app] last_project` | 不指定 = 跟随 `[app] last_project`，再不存在则取扫描结果第一个 |
| `-s, --set KEY=VALUE` | 覆盖 `tablet.toml` 任意配置项；可重复 | — |

`-s` 覆盖键名与 toml 路径一一对应，列表见 §9。未知 / 废弃 / 格式错误的 key 会以 warning 形式打到 stderr，但不会让命令失败。

## 8. 子命令

按是否需要先加载 Project 分两组。

### 8.1 不需要加载 Project

| 子命令 | 作用 | 输出 |
|--------|------|------|
| `list-templates` | 列出可用模板（内置 + `<workdir>/tblschema/` 本地模板库） | id / name / version / source 表 |
| `list-projects` | 列出 `<workdir>/projects/` 下所有 Project（id / name / created_at / source_template） | 表 |
| `migrate-legacy` | 把根目录 `config/` + `project.tblschema` 迁移到 `projects/default/` | 迁移结果摘要；@02 老结构迁移 |
| `new-project --template <id> --id <pid> [--name <n>] [--switch-after <bool>]` | 用模板新建 Project | 新建后的 `project_root` 路径 |

`new-project` 参数：

| 参数 | 说明 | 默认 |
|------|------|------|
| `--template <id>` | 模板 id（来自 `list-templates`），必填 | — |
| `--id <pid>` | Project id，约束 `[a-z0-9_-]{1,32}` | — |
| `--name <n>` | Project 显示名 | = id |
| `--switch-after <bool>` | 创建后写入 `[app] last_project` | true |

### 8.2 需要加载 Project

> 这些子命令先按 `--project` / `[app] last_project` / 扫描第一个的优先级选 Project，加载后才执行。

#### `export [--json] [--xml] [--java] [--go] [--lua] [--gdscript] [--typescript] [--cpp] [--csharp]`

不带任何 flag = 全格式导出；指定其中一项或多项 = 仅导出这些格式。每项产物的具体路径派生规则见 @02.23 与 @02 各导出章节。

`--csharp` 一次同时生成 dotnet / unity / godot 三套 Loader（schema 类共享，配置以 `csharp_dotnet` / `csharp_unity` / `csharp_godot` 三个独立 key 区分）。

退出码恒为 0；单格式失败仅 eprintln 错误，不影响其它格式继续，便于 CI 收集。

#### `validate`

全项目离线校验：cell / row / schema / project 四层（@01 附录 A）。

- 通过 → 退出码 0
- 任一节点失败 → 退出码 1，错误明细打到 stderr

#### `generate-test [...]`

灌测试数据，详细规则见 @02 测试数据生成。参数：

| 参数 | 说明 | 默认 |
|------|------|------|
| `--empty` | 在测试数据里穿插空值列样本，覆盖 schema 的可空场景 | false |
| `--schema <path>` | 用外部 `.tblschema` 文件代替 Project 内的 schema | — |
| `--rows <n>` | 数据行数；`0` 表示走内置固定数据 | 0 |
| `--seed <u64>` | 随机种子；`0` 表示固定数据，非 0 启用伪随机 | 0 |
| `--format json\|xml` | TestMain 的初始化方式 | json |
| `--lang java\|go\|none` | 生成 TestMain 的语言；`none` 表示只生成数据不生成 TestMain | java |

退出码 0 = 成功；I/O / 解析失败 → 抛错由 main 翻成 1。

## 9. `-s` 覆盖键

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
| `export.server.cpp.namespace` | `[export.server.cpp] namespace` |
| `export.server.cpp.code_output` | `[export.server.cpp] code_output` |
| `export.server.cpp.json_lib` | `[export.server.cpp] json_lib`（`nlohmann` / `rapidjson`） |
| `export.server.csharp_dotnet.namespace` | `[export.server.csharp_dotnet] namespace` |
| `export.server.csharp_dotnet.code_output` | `[export.server.csharp_dotnet] code_output` |
| `export.client.csharp_unity.namespace` | `[export.client.csharp_unity] namespace` |
| `export.client.csharp_unity.code_output` | `[export.client.csharp_unity] code_output` |
| `export.client.csharp_godot.namespace` | `[export.client.csharp_godot] namespace` |
| `export.client.csharp_godot.code_output` | `[export.client.csharp_godot] code_output` |
| `export.client.lua.output` / `export.client.output` | `[export.client.lua] output`（兼容历史命名） |

废弃键：`export.server.lang`（§4.6 拆分了 java/go 后不再使用），命中后只 warning 不写入。

`-s` 覆盖**仅作用于本次进程内存中的 `Project.config`**，不写回 toml；适合 Jenkins 临时切输出目录、CI 切编码等。toml 完整字段说明见 §4。

## 10. 退出码

| 码 | 含义 |
|----|------|
| 0 | 成功（包括 `validate` 全过 / `export` 单格式失败但整体收尾） |
| 1 | `validate` 不通过 / 解析 schema 失败 / 加载 Project 失败 / 其它 I/O |

Jenkins 脚本的失败判断完全依赖此码。

## 11. 依赖与体积

`tablet-cli` 仅依赖 `tablet-core` + `clap`，不引任何 UI 渲染依赖；release 体积约 1 MB（不开 LTO）。lib 形态（`libtablet_cli`）由 `tablet` 在 CLI 分支直接调用，等价于跑了一次 `tablet-cli` 子进程，省掉一次 fork。

## 12. tablet 的 CLI 分流

`tablet.exe` 启动时先做 argv 分类：

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
| 零参数（双击 / `tablet.exe`） | GUI，workdir = exe 所在目录（即 cwd） |
| `tablet --gui [--workdir=path]` | GUI，workdir 显式指定（开发期 `cargo run -p tablet-slint -- --gui --workdir=...`） |
| 其它任何参数（含 `--help` / 子命令 / `-w` / `-s`） | CLI：剔除 `--gui` 后转发给 `tablet_cli::run_with_args` |

CLI 分支和直接跑 `tablet-cli.exe` 行为完全一致；GUI 分支在 Windows 下额外调 `FreeConsole()` 释放 console，避免双击启动后黑窗驻留。

## 13. 用法示例

```bash
# 在指定仓库下导出全部格式
tablet-cli -w D:/work/game-config export

# 仅导出 Java 模板类，临时切输出目录
tablet-cli -w D:/work/game-config \
  -s export.server.java.code_output=tmp/java \
  export --java

# 离线校验某个 Project
tablet-cli -w D:/work/game-config --project slg-prod validate

# 用模板新建 Project
tablet-cli -w D:/work/game-config new-project \
  --template slg-base --id slg-2026 --name "SLG 2026"

# 灌 100 行带空值的测试数据，go 后端
tablet-cli -w D:/work/game-config generate-test \
  --rows 100 --empty --seed 42 --lang go

# Jenkins 风格：通过 tablet.exe 跑 CLI（同一个 exe 兼任）
tablet.exe -w D:/work/game-config validate
```
