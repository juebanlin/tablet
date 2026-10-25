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
    │   │   ├── excel.rs        ← xlsx 严格互转（CLI / GUI 编辑桥接共用）
    │   │   └── excel_sync.rs   ← Excel 同步引擎（umya-spreadsheet，仅 GUI 同步面板用）
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

`core/src/template/` 模块同时被项目模板库（@02）和 `util gen-test` CLI（测试代码生成）复用。

**未来扩展：core 可加 cdylib + C ABI** —— 当前 `tablet-core` 的 `pub` API 已按"可序列化 / 类型稳定"约束维护，后期若要把核心能力暴露给第三方程序（如 Unity 编辑器插件、定制 IDE 集成），只需追加 `[lib] crate-type = ["rlib", "cdylib"]` 与一份 `ffi.rs` + cbindgen 生成的 .h，无需改动现有调用方。

## 2. 仓库布局

```
game-config/
├── projects/                   ← 全部 Project，按 id 分目录
│   ├── slg-test/               ← 一个 Project
│   │   ├── project.toml        ← Project 可选配置覆盖（仅 [export] 段，字段级 deep merge）
│   │   ├── project.tblschema   ← 该 Project 的元数据（id/name/category/version）+ 结构骨架 + separators
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
├── .data/                    ← 本机用户数据（列宽等）+ 进程锁，不进 git
└── .gitignore                  ← 详见 §6
```

`projects/<id>/` 自包含一个 Project 全部所需（schema + 数据 + 元数据 + Excel 缓存）。同一仓库可并存多个 Project，启动时 UI 默认进入 `[project] last_project` 指定的那个；不存在则进入扫描结果中第一个。

老仓库的根目录 `config/` 在新版本启动时**自动迁移**到 `projects/default/`（@02 Project / 老结构迁移），不保留双结构。

## 3. Group 与 Excel 的对应关系

- 一个**文件夹** = 一个 **Group** = 一个 **Excel 文件**
- 文件夹内每个 `.tbl` 文件 = 一个 **Table / Constant / Enum** = Excel 中的一个 tab
- 用 Excel 编辑时整个 Group 生成一个 xlsx，每个配置项对应一个 tab（详见 @06）

Group 名仅在工具内部使用，不进生成代码路径，因此允许中文（@01.8.1、@02.1）。

## 4. tablet.toml 配置

工具配置文件（`GlobalConfig`），定义启动展开偏好、导出目标、UI 行为、类型分隔符默认值。所有字段都有默认值，新建项目可只写少量字段。**`tablet.toml` 是仓库全局配置，所有 Project 共享；Project 自身的元数据放在 `projects/<id>/project.tblschema`，可选的导出配置覆盖放在 `projects/<id>/project.toml`（@02 Project）**。

**配置分层与合并规则**：
- **export**：Project 的 `[export]` 段按 **field-level deep-merge** 覆盖全局；缺失字段回退到全局（例如：项目只写 `[export.server.cpp]`，其它语言用全局配置）
- **validate**：Project 的 `[validate]` 段是**项目级**的校验策略（@01.10.7），不参与工作区继承；没写 = 默认策略
  ```toml
  [validate]
  error   = "block"    # 阻断保存
  warning = "report"   # 仅提示
  info    = "ignore"   # 完全忽略
  ```
  可选值 `block` / `report` / `ignore`；拼错的键或非法值直接报错，不静默回退。
- **ui**：直接用全局，Project 级不允许覆盖（UI 偏好是用户级，不应按项目切换）
- **separators**：用 Project 自身 schema 的 `# @sep` 行（优先级最高）；仅在「新建空项目」时从全局拷贝初值（@01.7.4 / §4.9）

最终装配：`GlobalConfig` + `ProjectConfig.raw` (project.toml) → `ProjectConfig.config`（业务逻辑使用）；
**separators 不参与这层装配**，它只有 `schema.separators` 一个来源（见上一行）。

### 4.1 完整示例

```toml
[project]
last_project = "slg-test"     # 启动时默认展开的 Project id
opened_projects = ["slg-test", "slg-prod"]  # 启动时自动打开的项目列表

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

### 4.2 [project]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| last_project | 启动时**默认展开**的 Project id；不存在则展开扫描结果中第一个；其它 project 仍同时加载，仅折叠展示 | 空（首次进入）|
| opened_projects | 启动时自动打开的 Project id 列表；UI 层会自动维护（打开/关闭项目时更新） | [] |
| project_sort | Project 排序方式：`"name"` / `"created"` / `""` (字典序) | "" |
| project_order | 手动排序时的 id 顺序列表 | [] |

Project 列表由启动扫描 `projects/` 目录得出，**不需要**在 `tablet.toml` 里枚举。所有打开的 project 同时加载到内存，`last_project` 仅影响 TreeSection 首屏展开状态（无切换概念）；新建 project 后写入 last_project 让下次启动直接展开。

> 历史命名：旧版本是 `[app]`，S16 改名为 `[project]`（与 ProjectManagementConfig 类型对应），语义不变——仍是"全仓库级"项目管理配置。

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
| lombok | 用 Lombok 的 `@Getter` 代替逐字段手写 getter（只写注解，不打包 jar） | `true` |
| custom_regions | 产物带业务扩展保留区间，重新导出时留住区间内的手写代码（区间外一律覆盖） | `true` |

### 4.7 [export.server.go]

| 字段 | 说明 | 默认值 |
|------|------|--------|
| package | Go package | — |
| code_output | Go 代码输出目录 | — |
| custom_regions | 产物带业务扩展保留区间，重新导出时留住区间内的手写代码（区间外一律覆盖） | `true` |

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
| 项目设置对话框改分隔符 | 写回项目 `.tblschema`；**关闭对话框时**统一触发该项目 `revalidate_all`（不是每次按键，见 @04.6.7）；与 toml 无关 |

`[separators]` 共 25 项 leaf（list / set / tuple2/3/4 / map.kv / map.entry / list_tuple{2,3,4}.{tuple,list} / map_tuple{2,3,4}.{kv,tuple,entry} / map_list.{kv,item,entry}）— 这套清单是**唯一真值源**，定义在 `crates/core/src/export/sep_meta.rs::sep_kv_pairs`，JSON `_sep` / XML `sep_*` attr / Go `SepConfig` / Java `SepConfig` 全部由此派生：

- 数据文件（JSON `_sep` / XML `sep_*` attr）：**按需输出** —— `paradigm_sep_keys` 列出每个范式用到的 leaf，扫表/常量字段 union 得到该文件需要的 key 集合，集合外的 leaf 不出。全 base 表 / 全 base + Ref 表（如纯 int/str/bool）`_sep` 整个省略。
- 生成代码（Go `templates/go/sep.go` / Java `templates/java/SepConfig.java`）：**保持 25 字段全量** —— 项目级共享一份 SepConfig 类，不同表用不同范式，按字段范式查 `SepConfig.listTuple2List` 等成员，不能裁剪。
- 消费端 `SepConfig.fillDefaults()`（Go）/ `getAny(..., default)`（Java）给缺失 key 兜底，所以数据文件省略 leaf 安全。

加新分隔符时，需要同步：(1) `SeparatorsSection` + serde rename；(2) `types.rs::SepKey` 枚举（加 variant + 4 个 match：`as_directive_key` / `as_export_key` / `get` / `set`，加进 `SepKey::ALL`）；(3) `Paradigm::sep_keys` 把新 leaf 接到对应范式；(4) Go / Java 模板里的 SepConfig 字段及对应 fromMap / fromXMLAttrs 加载；(5) 项目设置对话框 `project_settings.slint` 的 `ps-sep-*` property（值 + `-modified` 成对）与 `SepRow`；后端 `push_sep()` 里的成对推送（@02.26.4）。`sep_meta.rs` 的 invariant 测试 + `match` 编译期穷尽性锁死 (1)(2)(3) 一致性——漏改会编译失败或测试失败。

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

# 本机用户数据（列宽 / 视图偏好）+ 进程锁，不进 git
.data/
```

注意：`project.toml` / `project.tblschema` / `config/**/*.tbl` 都需要进版本控制——它们是 Project 的本体。

#### `.data/`：本机用户数据

每个 checkout 自带一个 `.data/` 隐藏目录（**不进 Git / SVN**）：

```text
.data/
  .gitignore        # 内容 *，应用首次写数据时创建
  user-data.json    # 根结构 UserData：ui.grid（列宽）…；预留 history / session
  tablet.lock       # GUI / CLI 互斥锁（原 .tablet.lock）
  tmp/              # 原子写临时文件
```

- **不参与项目保存**：不走 `save_all` / dirty / 校验；用户改变视图时由独立通道写。
- 同一工作区多个项目共用一个文件，条目带 `project/group/kind/node`。
- 换机器 / 重新 clone 不会带过去；这是本机习惯数据，不是业务配置。
- 后续「取消保存 / 操作回退 / 启动 UI 锁」复用这个目录，不再新增位置。

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
├── .data/                        # 本机用户数据 + 进程锁（.gitignore）
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
tablet-cli [全局选项] <命令>
```

### 7.1 项目上下文选项

以下选项仅对需要项目上下文的命令生效（project / schema / export / validate / excel / workspace / sep）。`util` 子命令不接受这些选项（传了也会忽略）。

| 参数 | 作用 | 默认值 |
|------|------|--------|
| `-w, --workdir <path>` | 仓库根目录（`projects/` 的父） | `.`（当前目录） |
| `--project <id>` | 显式指定 Project id；覆盖 `[project] last_project` | 不指定 = 跟随 `[project] last_project`，再不存在则取扫描结果第一个 |
| `-s, --set KEY=VALUE` | 覆盖 `tablet.toml` 任意配置项；可重复 | — |

### 7.2 通用选项

| 参数 | 作用 | 适用范围 |
|------|------|---------|
| `--fmt <FORMAT>` | 输出格式。`json` = JSON 结构化输出（默认人类可读文本） | 查询类命令：`project list`/`info`、`schema show`、`validate`、`export data`/`code`/`all`、`sep show`、`util parse-*`/`stat` |
| `--help` | 显示帮助信息 | 所有命令 |
| `--version` | 显示版本号 | 顶层 |

### 7.3 util 专属配置选项

`util` 子命令中需要分隔符的命令（`validate-tbl` / `validate-type`）有独立的配置体系：

| 参数 | 作用 |
|------|------|
| `--config <tablet.toml>` | 从**指定的** tablet.toml 读取 `[separators]` |
| `--schema <.tblschema>` | 从**指定的** schema 文件读取 `# @sep`（优先级高于 config） |
| `--sep <KEY=VALUE>` | 手动指定单个分隔符（最高优先级，可多次） |

优先级（从低到高）：内置默认值 → `--config` → `--schema` → `--sep`。
这是**显式参数之间**的优先级，与项目上下文无关（`util` 子命令不接受 `-w`/`--project`）。

### 7.4 退出码与失败契约

CLI 主要由 AI / 脚本驱动：**没有反馈等于没有发生**（@02.26.7）。

| 退出码 | 含义 |
|---|---|
| `0` | 命令完成且结果可信（导出文件已写、改动已落盘） |
| `1` | 验证不通过 / 参数指向的目标不存在 / 落盘被验证闸门拦下 / 解析失败 / I/O 失败 |
| `2` | clap 用法错误（缺必填参数、未知选项） |

三条硬约定：

1. **报错走 stderr，机器可读的信息走 stdout**（`--fmt json` 时 stdout 只有 JSON）。
2. **解析不了的文件直接失败**，不做"跳过这一条继续"：`.tblschema` 坏 → 项目打不开；
   `.tbl` 坏 → 整个项目打不开（以前只 warn 一行，然后那张表就从项目里消失了）。
   扫描类命令（`project list`）仍按目录名兜底，但加载闸门会拦下后续所有需要项目上下文的命令。
3. **凡是对外声称"已添加 / 已重命名 / 已保存"的命令，都必须先看 `save_all()` 的返回值**：
   项目有验证错误时它一个字节都不写。

### 7.5 GUI 与 CLI 互斥

同一个工作区同一时刻只允许一个 tablet 进程操作：GUI 和 CLI 共享同一份
`projects/<id>/config/`，同时写就是丢更新（临时文件名还会撞车）。

做法是 git `index.lock` 那一套：

- 工作区 `.data/tablet.lock`，内容 `<pid> <kind>`（kind = `gui` / `cli`）；
- `create_new` 独占创建，谁先谁得；
- 拿不到锁时读 PID：**活着 → 拒绝**并说明是谁占着；已经死了 → 当残留锁清掉重试；
- 持有者退出（含 panic 之外的正常路径）时 drop 释放，且只删 PID 对得上的那一把。

适用范围：`util` 与 `list-templates` 不碰工作区状态，不参与互斥；其余命令（含
只读的 `project list`）都会先拿锁。因此 **GUI 开着时 CLI 会直接失败**，
报 `工作区正被另一个 GUI 使用（PID xxx）`。

这层锁只管**本工具的进程之间**：Excel 占用由 `~$` 检测（@02.26.8），
外部改动由基线核对兜底。

## 8. 子命令

### 8.1 project — 项目管理

需要工作区上下文（`-w` 指定 workdir，扫描 `projects/` 目录）。写操作执行后自动保存。

#### `project list`

列出工作区内所有 Project。

```bash
# 基本用法
tablet-cli project list

# 指定工作目录
tablet-cli -w D:/work/game-config project list

# JSON 输出（脚本消费）
tablet-cli -w D:/work/game-config --fmt json project list
```

输出示例：
```
slg-test                 SLG 测试项目
slg-prod                 SLG 正式项目
rpg-demo                 RPG Demo
```

#### `project info`

显示项目详情。

| 参数 | 说明 | 默认 |
|------|------|------|
| `--id <id>` | 目标项目 id | 当前活跃项目 |

```bash
# 当前活跃项目
tablet-cli project info

# 指定项目
tablet-cli project info --id slg-test

# JSON 格式
tablet-cli --fmt json project info --id slg-test
```

输出示例：
```
Project: slg-test (SLG 测试项目)
  Groups:    3
  Tables:    8
  Constants: 3
  Enums:     2
  总数据行:  1,245
  Dirty:     否
```

#### `project new`

从模板或一个 `.tblschema` 文件创建新 Project。

| 参数 | 说明 | 默认 |
|------|------|------|
| `--template <id>` | 模板 id（来自 `list-templates`）；与 `--from-file` 二选一 | — |
| `--from-file <path>` | 直接从一个 `.tblschema` 建项目（`# @preset` 会落成 `.tbl` 数据） | — |
| `--id <pid>` | Project id，约束 `[a-z0-9_-]{1,32}` | — |
| `--name <n>` | Project 显示名 | `--from-file` 时取 schema 的 `# @meta name`，否则 = id |
| `--switch-after <bool>` | 创建后写入 `[project] last_project` | `true` |

```bash
# 最小用法
tablet-cli project new --template slg-base --id slg-2026

# 完整参数
tablet-cli -w D:/work/game-config project new \
  --template slg-base --id slg-2026 --name "SLG 2026" --switch-after true

# 从 AI 生成的 schema 文件建项目（含预设数据）
tablet-cli project new --from-file demo.tblschema --id demo

# 不切换活跃项目
tablet-cli project new --from-file demo.tblschema --id temp-test --switch-after false
```

> 校验闸门不过（字段名非法、引用不存在、枚举为空……）时**整个项目撤回**，逐条打印原因并 exit 1，不留半成品。

#### `project rename`

重命名 Project（id 和/或显示名）。自动保存。

| 参数 | 说明 | 默认 |
|------|------|------|
| `--id <id>` | 目标项目 id，必填 | — |
| `--new-id <id>` | 新 id（会迁移目录）。必须满足 `[a-z0-9_-]{1,32}`，否则报错且**原项目不动** | 不改 |

读取 `project.tblschema` 失败（含 `# @meta id` 声明了非法值）时，命令**非零退出**并逐条打印
`错误: Project <id> 打不开: <文件>: line N: ...`；见 @02.26.7。
例外：`project new` 不因为有别的项目打不开就拒绝（建新的正是折腾坏之后的出路之一）。
| `--new-name <name>` | 新显示名 | 不改 |

```bash
# 改显示名
tablet-cli project rename --id slg-test --new-name "SLG 测试环境"

# 改 id（迁移 projects/slg-test/ → projects/slg-staging/）
tablet-cli project rename --id slg-test --new-id slg-staging

# 同时改
tablet-cli project rename --id slg-test --new-id slg-v2 --new-name "SLG V2"
```

#### `project delete`

删除 Project（不可逆）。

| 参数 | 说明 |
|------|------|
| `--id <id>` | 目标项目 id，必填 |
| `--confirm` | 确认删除，必填 |

```bash
# 安全删除
tablet-cli project delete --id temp-test --confirm

# 不带 --confirm 拒绝执行（exit 1）
tablet-cli project delete --id temp-test
# → 错误: 删除操作不可逆，请添加 --confirm 确认
```

#### `project clone`

深拷贝已有 Project 为新 Project。自动保存。

| 参数 | 说明 | 默认 |
|------|------|------|
| `--source <id>` | 源项目 id，必填 | — |
| `--id <new_id>` | 新项目 id，必填 | — |
| `--name <name>` | 新项目显示名 | = id |

```bash
tablet-cli project clone --source slg-test --id slg-test-backup

tablet-cli project clone --source slg-prod --id slg-hotfix --name "SLG 热修复"
```

---

### 8.2 schema — 结构操作

需要指定项目上下文（通过 `--project` 或当前活跃项目）。所有写操作执行后自动保存。

#### `schema show`

显示当前项目的 schema 结构树。

```bash
tablet-cli schema show
tablet-cli --project slg-test schema show
tablet-cli schema show --rules          # 同时列出每个节点的有效校验规则
tablet-cli --fmt json schema show
```

`--rules` 会在每个 table / enum 节点下追加**有效规则**（系统内置在前、用户规则在后）：

```
hero/
  └── HeroBase      (table, 5 fields, 120 rows)
      · id = unique() [系统]
      · id = non_empty() [系统]
      · id = positive_int() [系统]
      · name = matches(^[A-Z][A-Z0-9_]*$) [警告]
```

内置规则不落盘、不可编辑（@01.8.11）；`--fmt json` 时节点对象多一个 `rules` 数组。

输出示例：
```
hero/
  ├── HeroBase      (table, 5 fields, 120 rows)
  ├── HeroSkill     (table, 3 fields, 45 rows)
  ├── HeroConst     (constant, 8 entries)
  └── HeroType      (enum, 6 entries)
item/
  ├── ItemBase      (table, 6 fields, 200 rows)
  └── ItemDrop      (table, 4 fields, 80 rows)
global/
  └── GlobalConst   (constant, 15 entries)
```

#### `schema add-group`

```bash
tablet-cli schema add-group --name skill
tablet-cli --project rpg-demo schema add-group --name quest

# 工作簿形态的组（节点 = sheet，数据落在 <group>.xlsx）
tablet-cli schema add-group --name xg --storage xlsx
```

`--storage` 只接受 `tbl`（默认）/ `xlsx`（@02.26.8）。一组一种后端：xlsx 组加节点
（`add-table` / `add-constant` / `add-enum`）时不会再写 `.tbl`，而是落成工作簿里的 sheet。

#### `schema add-table` / `add-constant` / `add-enum`

```bash
tablet-cli schema add-table --group hero --name HeroLevel
# 新建表可选文件格式：v3（默认，块式）/ v2（稀疏行）
tablet-cli schema add-table --group hero --name HeroLegacy --tbl-fmt v2
tablet-cli schema add-constant --group global --name ServerConfig

# 枚举条目必须同一条命令给全：空枚举过不了校验，也就落不了盘
tablet-cli schema add-enum --group hero --name HeroRarity \
  --entry 1:COMMON:普通 --entry 2:RARE:稀有
```

`--tbl-fmt` 只接受 `v2` / `v3`，缺省 `v3`；xlsx 组不落 `.tbl`，该参数无效果。
`--entry` 格式 `ID:NAME[:DESC]`，可重复；id 必须是正整数（0 保留给"未设置"），name 必须 UPPER_SNAKE_CASE。

> **schema 写命令一律「先校验、再动手、最后看落盘结果」**：
> 名字不合规（组名 / 节点名）→ 报错退出；目标 Group 不存在 → 报错退出；
> 节点撞名 → 报错退出（`NewTable` 是无条件 push，撞名会覆盖同名 `.tbl`）；
> 改名 / 删除的目标不存在 → 报错退出（不再静默 no-op 后宣布成功）；
> 保存时项目有验证错误 → 报错退出并逐条打印，盘上一个字节都没动。
>
> 一个组里最后一个节点被删掉时，组目录一并清理；**新建的空组**则保留，
> 好让 `add-group` → `add-table` 能跨进程走通。

#### `schema export`

把项目结构导出成 `.tblschema`（对应 GUI 的「导出 Schema...」/「另存为模板...」）。

```bash
# 全部节点（只导结构；加 --with-preset 才带数据）
tablet-cli --project slg-test schema export --out slg.tblschema

# 只导一个分组 / 一个节点（可重复，取并集）
tablet-cli --project slg-test schema export --out hero.tblschema --group hero
tablet-cli --project slg-test schema export --out one.tblschema --node hero/HeroBase

# 覆盖元数据
tablet-cli --project slg-test schema export --out tpl.tblschema   --meta-id slg_core --meta-name "SLG 核心结构" --meta-category slg --meta-version 1.2.0

# 保存到本地模板目录（等价于 GUI 的「另存为模板...」）
tablet-cli --project slg-test schema export --as-template --meta-id slg_core
```

| 参数 | 说明 |
|------|------|
| `--out <file>` | 输出文件；与 `--as-template` 二选一 |
| `--as-template` | 写到本地模板目录 `<local>/<meta-id>.tblschema`，需要 `--meta-id` |
| `--group <G>` | 只导出该分组（可重复） |
| `--node <G/N>` | 只导出该节点（可重复） |
| `--with-preset` | 带上 `# @preset` 数据 |
| `--meta-id` / `--meta-name` / `--meta-category` / `--meta-version` | 覆盖 `# @meta` |

- 不传 `--group` / `--node` = 全部节点；两者同时给时取并集。
- 元数据缺省继承当前项目；`--meta-name` 为空时用 id 兜底。
- `--fmt json` 输出 `{"path": "...", "template": false, "sections": N}`。
- 项目级「合并 Schema」是**导入**，CLI 目前只有文件级的 `util merge-schema`。

#### `schema rename-group`

```bash
tablet-cli schema rename-group --old hero --new character
```

#### `schema rename-node`

```bash
tablet-cli schema rename-node --group hero --old HeroBase --new CharacterBase
```

> `rename-group` / `rename-node` 的改名动作本身只写内存，**落盘由随后的自动保存完成**（见 @02.26.1）。
> 项目存在验证错误时自动保存会中止，此时命令**报错退出**、盘上仍是原名——不会对外声称"已重命名"。

#### `schema delete-group`

```bash
tablet-cli schema delete-group --name temp
```

#### `schema delete-node`

```bash
tablet-cli schema delete-node --group hero --name HeroOld
```

---

### 8.3 export — 数据导出

拆分为三个子命令，职责分离：

| 子命令 | 职责 | 粒度控制 |
|--------|------|---------|
| `export data` | 导出数据文件（JSON/XML） | 支持 --group/--node 过滤 |
| `export code` | 导出代码文件（Java/Go/Lua/...） | 全项目，选语言 |
| `export all` | 全量导出（data + code） | CI 一把梭 |

单格式失败**不打断**其它格式（逐项继续），但**只要有一个格式失败，退出码就是 1**——
`export all || exit 1` 这种 CI 写法全靠它。（以前这里恒为 0。）

#### `export data`

导出数据文件（JSON/XML），支持按组/节点过滤。

| 参数 | 说明 | 默认 |
|------|------|------|
| `--json` | 导出 JSON 数据 | — |
| `--xml` | 导出 XML 数据 | — |
| `--group <g>` | 仅导出指定组 | 全部 |
| `--node <n>` | 仅导出指定节点（依赖 --group） | 全部 |
| `-o, --output <path>` | 覆盖数据输出目录 | config 中的 data_output |

不指定 `--json`/`--xml` 则两者都导出。

```bash
# 全项目 JSON 数据
tablet-cli --project slg-test export data --json

# 仅 hero 组的 JSON 数据
tablet-cli --project slg-test export data --json --group hero

# 仅一张表的 JSON 数据
tablet-cli --project slg-test export data --json --group hero --node HeroBase

# XML 数据 + 自定义输出目录
tablet-cli --project slg-test export data --xml --group item -o ./tmp/data/

# JSON + XML 全部（不指定格式 = 全部）
tablet-cli --project slg-test export data
```

#### `export code`

导出代码文件，必须全项目（代码引用关系需要完整）。

| 参数 | 说明 |
|------|------|
| `--java` / `--go` / `--lua` / `--gdscript` / `--cpp` | 选择语言 |
| `--typescript` | TypeScript 两端（= `--typescript-client --typescript-server`） |
| `--typescript-client` / `--typescript-server` | 只导前端 / 只导 Node.js 端 |
| `--csharp` | C# 三套（dotnet + unity + godot） |
| `--csharp-dotnet` / `--csharp-unity` / `--csharp-godot` | 只导一个 C# runtime |
| `--all` | 全部语言 |
| `--package <pkg>` | 覆盖 Java/Go 的 package 名 |
| `--namespace <ns>` | 覆盖 C++/C# 的 namespace |
| `-o, --output <path>` | 覆盖代码输出目录（语义见下） |

至少选一种语言或 `--all`。`--csharp` / `--typescript` 是简写；要单套（例如 Unity 客户端）
用 `--csharp-unity`，不会顺带生成 dotnet / godot。

**`-o` 的语义**：

- 只选一种格式：`DIR` 就是该格式的输出目录（与旧行为一致）。
- 选多种格式：`DIR` 是**输出根**，按 `export all -o` 的标准布局展开：
  `DIR/server/java`、`DIR/server/go`、`DIR/server/cpp`、`DIR/server/csharp`、
  `DIR/server/typescript`、`DIR/client/lua`、`DIR/client/gdscript`、
  `DIR/client/typescript`、`DIR/client/csharp_unity`、`DIR/client/csharp_godot`。
  这样多个 runtime 的 `Loader.cs` / 同名文件不会互相覆盖。
- 相对路径按 `projects/<id>/` 解析（不是当前目录）；拿不准就写绝对路径。

**UI 导出对话框的 4 个预设 = CLI 组合**：

| UI 预设 | CLI |
|---|---|
| 全部 | `export all` |
| 数据 | `export data` |
| 后端 | `export data` + `export code --java --go --cpp --csharp-dotnet --typescript-server` |
| 前端 | `export code --lua --gdscript --typescript-client --csharp-unity --csharp-godot` |

```bash
# 仅 Java
tablet-cli --project slg-test export code --java

# Java + 临时覆盖 package
tablet-cli --project slg-test export code --java --package com.test.config

# C++ + 覆盖 namespace
tablet-cli --project slg-test export code --cpp --namespace test::config

# C# + 覆盖 namespace
tablet-cli --project slg-test export code --csharp --namespace Test.Config

# 多语言同时导出
tablet-cli --project slg-test export code --java --go

# 多语言 + 统一输出根（实际写到 tmp/server/java、tmp/server/go）
tablet-cli --project slg-test export code --java --go -o ./tmp/

# 只导 Unity 客户端（不会生成 dotnet / godot）
tablet-cli --project slg-test export code --csharp-unity

# 全部语言
tablet-cli --project slg-test export code --all

# 全部语言 + 覆盖输出
tablet-cli --project slg-test export code --all -o ./release/code/
```

#### `export all`

全量导出（= data 全部 + code 全部），CI 流水线一把梭。

| 参数 | 说明 |
|------|------|
| `-o, --output <path>` | 覆盖公共输出根目录（各子目录结构不变） |

```bash
# 全量导出
tablet-cli --project slg-test export all

# 全量 + 自定义根目录
tablet-cli --project slg-prod export all -o ./release/

# Jenkins 典型用法
tablet-cli --project slg-prod validate && \
tablet-cli --project slg-prod export all
```

---

### 8.4 validate — 验证（五级粒度）

全项目离线校验，支持通过参数逐级缩小验证范围。

| 参数 | 说明 | 依赖 |
|------|------|------|
| `--group <g>` | 验证指定组 | — |
| `--node <n>` | 验证指定节点 | 依赖 --group |
| `--col <c>` | 验证指定列（从 0 开始） | 依赖 --node |
| `--row <r>` | 验证指定行（从 0 开始） | 依赖 --col |
| `--strict` | 把**警告**也算作失败（有警告时 exit 1）。CI 用 | — |

```bash
# 全项目验证
tablet-cli validate

# 指定项目
tablet-cli --project slg-prod validate

# 验证单个 group
tablet-cli validate --group hero

# 验证单个节点
tablet-cli validate --group hero --node HeroBase

# 验证指定列
tablet-cli validate --group hero --node HeroBase --col 2

# 验证单个单元格
tablet-cli validate --group hero --node HeroBase --row 5 --col 2

# JSON 输出错误列表
tablet-cli --fmt json validate --group hero
```

输出示例（失败时 exit 1）：
```
发现 3 个验证错误:
  [验证] hero/HeroBase C6:[abc] -> 不是合法的 int 值
  [验证] hero/HeroBase D12:[] -> 必填字段不能为空
  [验证] hero/HeroSkill B3:[99999] -> 引用 id 不存在于 HeroBase
```

每行就是 `tablet_core::ops::format_validation_log` 的输出——**与 GUI 日志同一套渲染**，含定位（数据格是 `C6` 这种列字母+行号，表头层是「表头第 4 行 G 列」）、单元格原值与消息。表头层错误不要看 `row` 字段，那是 `SCHEMA_ROW` 哨兵值。

`--fmt json` 输出的是对象，含 `errors` 与 `warnings` 两个数组，每项字段为 `project_id` / `group` / `node` / `row` / `col` / `message` / `display` / `severity`（`display` 即上面那行文本）：

```json
{
  "errors": [
    {
      "project_id": "col", "group": "item", "node": "Item",
      "row": 0, "col": 5,
      "message": "不是合法int, 示例: 1",
      "display": "[验证] item/Item F1:[abc] -> 不是合法int, 示例: 1",
      "severity": "error"
    }
  ],
  "warnings": [
    {
      "project_id": "col", "group": "error", "node": "ErrorCode",
      "row": 18446744073709551615, "col": 1,
      "message": "ref_exists 的目标节点 \"Lang\" 不存在",
      "display": "[警告] error/ErrorCode 表头B列:[name] -> ref_exists 的目标节点 \"Lang\" 不存在",
      "severity": "warning"
    }
  ]
}
```

输出分两段：先「发现 N 个验证错误」，再「M 条警告」（`[警告]` 前缀）。

退出码：通过（无错误）→ 0；任一错误 → 1。警告默认不影响退出码；`--strict` 时有警告也返回 1。

**`--group` / `--node` 指到不存在的东西也是 1**（"Group 不存在: xxx" / "节点不存在: g/n"）。
空集恒真：以前 `--group typo` 会返回"验证通过"+0，脚本无从分辨"这个组没问题"和"根本没有这个组"。

---

### 8.5 excel — Excel 桥接

#### `excel export`

导出分组为 xlsx（表头锁定，数据区可编辑）。

| 参数 | 说明 | 默认 |
|------|------|------|
| `--group <g>` | 分组名，必填 | — |
| `--include <a,b>` | 仅导出指定节点（逗号分隔） | 整组全部 |
| `-o, --output <path>` | 输出路径 | `./{group}.xlsx` |

```bash
# 整组导出
tablet-cli excel export --group hero

# 部分节点
tablet-cli excel export --group hero --include HeroBase,HeroSkill

# 自定义路径
tablet-cli excel export --group hero -o output/hero_config.xlsx
```

#### `excel import`

将策划编辑过的 xlsx 回读到 .tbl（严格 header 校验）。

| 参数 | 说明 |
|------|------|
| `--group <g>` | 目标分组名，必填 |
| `--file <path>` | xlsx 文件路径，必填 |

```bash
tablet-cli excel import --group hero --file hero.xlsx

tablet-cli excel import --group item --file D:/shared/item_update.xlsx
```

---

### 8.6 workspace — 工作区操作

#### `workspace info`

显示工作区路径、版本库分支 / 修订、项目列表。**不加载 `.tbl`、不占工作区锁**：
GUI 开着、甚至某张表坏了，也能读到分支（Unity 插件 / CI 用它显示"当前在哪个分支"）。

```bash
tablet-cli workspace info
tablet-cli --fmt json workspace info
```

```
Workspace: D:/game/config
  VCS:    git
  Branch: main @ 8c47a4f
  Projects: game (SLG), tools (工具配置)
  Last:   game
```

```json
{
  "workdir": "D:/game/config",
  "vcs": "git",
  "branch": "main",
  "revision": "8c47a4f",
  "detached": false,
  "vcs_error": null,
  "last_project": "game",
  "projects": [{"id": "game", "name": "SLG"}]
}
```

- Git：直接读 `.git`（含 submodule / worktree），不要求 PATH 里有 git。
- SVN：`.svn` + `svn info --show-item`（SVN 1.9+）；没装命令行工具时 `branch/revision` 为空、`vcs_error` 说明原因。
- 无版本库：`vcs = null`。

#### `workspace save`

保存所有 dirty 节点到磁盘，并顺手刷新 `project.tblschema` 投影
（手改过 `.tbl` 也能把结构同步进 schema 文件）。

```bash
tablet-cli workspace save
tablet-cli --project slg-test workspace save
```

项目存在验证错误时**一个字节都不写**，命令逐条打印错误并 exit 1——不会对外声称"已保存"。

#### `workspace reload`

从磁盘重新加载（丢弃内存中未保存的修改）。

```bash
tablet-cli workspace reload
```

#### `workspace clear`

清空所有 .tbl 数据文件（危险操作，需 `--confirm`）。

```bash
tablet-cli workspace clear --confirm

# 不带 --confirm 拒绝执行
tablet-cli workspace clear
# → 错误: 此操作将删除所有数据文件，请添加 --confirm 确认
```

---

### 8.7 util gen-test — 生成测试运行代码

从 schema 文件生成语言对应的测试运行代码（TestMain.java / main.go）。纯文件操作，无需项目上下文。

| 参数 | 说明 | 默认 |
|------|------|------|
| `--lang <lang>` | 测试语言（java / go），必填 | — |
| `--format <fmt>` | 数据格式（决定 init 方式） | json |
| `--schema <path>` | schema 文件路径，必填 | — |
| `-o, --output <dir>` | 输出目录，必填 | — |
| `--package <pkg>` | Java package / Go package 名 | com.game.config |
| `--code-output <path>` | Go 的 code_output 路径（构造 import path） | gen/server/go |

```bash
# 生成 Java 测试代码
tablet-cli util gen-test --lang java --format json \
  --schema project.tblschema -o ./test/java/

# 生成 Go 测试代码
tablet-cli util gen-test --lang go --format xml \
  --schema project.tblschema -o ./test/go/

# 自定义 package
tablet-cli util gen-test --lang java --format json \
  --schema project.tblschema --package com.test.config -o ./test/
```

---

### 8.8 sep — 分隔符查询

展示全部 25 个分隔符键的生效值及来源。

| 参数 | 说明 |
|------|------|
| （无） | 展示**当前项目**的生效值，来源是该项目的 `project.tblschema`（@01.7.4） |
| `--defaults` | 仅展示内置默认值（不读任何文件、不牵扯项目） |
| `--config <path>` | 读**指定** tablet.toml 的 `[separators]`（是「指定文件」，不是全局配置） |
| `--schema <path>` | 读**指定** .tblschema 的 `# @sep` |

给了 `--defaults` / `--config` / `--schema` 中任意一个就与项目无关；只有裸 `sep show` 需要项目上下文。
「当前项目」跟其它命令同一套解析：`--project <id>` 优先，否则 last_project。

**取值规则：每个 leaf 独立回退**——25 个键各自从高优先级往低找，取第一个「写过」该键的来源；都没写就回退到内置默认。
优先级（高 → 低）：`--schema` 文件 > `--config` 文件 > 当前项目的 schema > 内置默认。

`SeparatorsSection` 的 25 个字段**每个都带 serde 默认值**，所以「没写」在类型层面等价于「等于内置默认」——
这也是 `# @sep` / `[separators]` 都只写偏离默认值的 leaf、`serialize_tblschema` 只输出非默认 leaf 的原因（@01.7.4）。
于是 `--config` / `--schema` 只写关心的键即可，未写的自动回退。

每行行尾的 `[来源]` 标出该键的值最终来自哪一层：`默认` / `项目` / `config` / `schema`。

```bash
# 展示当前项目的生效分隔符（表头会写明是哪个项目）
tablet-cli sep show

# 多项目工作区里指定项目
tablet-cli --project slg-test sep show

# 仅展示内置默认值
tablet-cli sep show --defaults

# 读取指定 config 文件（只写关心的键即可，未写的回退默认）
tablet-cli sep show --config D:/work/game-config/tablet.toml

# 读取指定 schema 文件
tablet-cli sep show --schema D:/work/game-config/projects/slg-test/project.tblschema

# 同时指定：同一 leaf 两边都写了的话 schema 赢，只写一边的按那一边
tablet-cli sep show --config tablet.toml --schema project.tblschema

# JSON 输出
tablet-cli --fmt json sep show
```

输出示例（多项目工作区里展示项目 `pa` 的生效值；`pa` 的 schema 里写了 `# @sep List = ~`）：
```
分隔符配置（项目 pa）:
  List                  = ~         [项目]
  Set                   = ;         [默认]
  Tuple2                = ,         [默认]
  Tuple3                = ,         [默认]
  Tuple4                = ,         [默认]
  Map.kv                = :         [默认]
  Map.entry             = ;         [默认]
  List_Tuple2.tuple     = ,         [默认]
  List_Tuple2.list      = ;         [默认]
  Map_Tuple2.kv         = :         [默认]
  Map_List.item         = ,         [默认]
  ...（全 25 键）
```

键名就是 schema 里 `# @sep` 的写法（`List` / `Map.kv` / `List_Tuple2.tuple` …），全套键名见 @01.7.4。

---

### 8.9 util — 底层工具

纯文件操作工具，不接受 `-w`/`--project`/`-s`。直接操作指定文件，无需加载完整项目。

#### `util parse-tbl`

解析 .tbl 文件，输出结构化 JSON。

```bash
tablet-cli util parse-tbl config/hero/HeroBase.tbl

tablet-cli util parse-tbl HeroBase.tbl > hero_base.json
```

输出示例：
```json
{
  "type": "table",
  "name": "HeroBase",
  "fields": [
    {"name": "id", "type": "int", "export": "cs", "desc": "英雄ID"},
    {"name": "name", "type": "str", "export": "cs", "desc": "英雄名"}
  ],
  "records": [
    ["1001", "亚瑟"],
    ["1002", "兰斯洛特"]
  ]
}
```

#### `util parse-schema`

解析 .tblschema 文件，输出结构信息。

```bash
tablet-cli util parse-schema project.tblschema
tablet-cli util parse-schema custom.tblschema > schema.json
```

#### `util merge-schema`

合并多个 .tblschema 文件（sections 合并，meta 取第一个）。

```bash
tablet-cli util merge-schema base.tblschema addon.tblschema

tablet-cli util merge-schema a.tblschema b.tblschema c.tblschema > merged.tblschema
```

#### `util validate-tbl`

验证单个 .tbl 文件（类型校验 + 命名规则）。

```bash
# 使用内置默认分隔符
tablet-cli util validate-tbl HeroBase.tbl

# 指定 schema 获取分隔符
tablet-cli util validate-tbl HeroBase.tbl --schema project.tblschema

# 同时指定 config + schema
tablet-cli util validate-tbl HeroBase.tbl \
  --config tablet.toml --schema project.tblschema

# 手动覆盖特定分隔符
tablet-cli util validate-tbl HeroBase.tbl \
  --schema project.tblschema --sep list="|" --sep map.kv="="

# JSON 输出
tablet-cli --fmt json util validate-tbl HeroBase.tbl
```

#### `util validate-type`

验证单个值是否匹配指定类型。通过 → exit 0 无输出；失败 → 输出错误 + exit 1。

```bash
# ── 基础类型 ──
tablet-cli util validate-type int 42
tablet-cli util validate-type str "hello world"
tablet-cli util validate-type bool true
tablet-cli util validate-type float 3.14

# ── 列表类型（默认分隔符 ;）──
tablet-cli util validate-type "List<int>" "1;2;3"
tablet-cli util validate-type "List<str>" "苹果;香蕉;橘子"
tablet-cli util validate-type "Set<int>" "1;2;3"

# ── Map 类型（默认 kv=: entry=;）──
tablet-cli util validate-type "Map<str,int>" "name:10;age:20"
tablet-cli util validate-type "Map<int,str>" "1:苹果;2:香蕉"

# ── Tuple 类型（默认分隔符 ,）──
tablet-cli util validate-type "Tuple2<int,str>" "1001,亚瑟"
tablet-cli util validate-type "Tuple3<int,int,str>" "1,2,hello"

# ── 复合类型 ──
tablet-cli util validate-type "List<Tuple2<int,str>>" "1,a;2,b;3,c"
tablet-cli util validate-type "Map<str,List<int>>" "skills:1,2,3;items:4,5"

# ── 自定义分隔符 ──
tablet-cli util validate-type "List<int>" "1|2|3" --sep list="|"

tablet-cli util validate-type "Map<str,int>" "name=10;age=20" \
  --sep map.kv="="

# ── 从文件读分隔符 ──
tablet-cli util validate-type "List<int>" "1|2|3" \
  --schema project.tblschema

tablet-cli util validate-type "Map<str,int>" "name=10;age=20" \
  --config tablet.toml --schema project.tblschema

# ── 三级合并：config + schema + 手动覆盖 ──
tablet-cli util validate-type "List<Tuple2<int,str>>" "1:a|2:b" \
  --config tablet.toml --schema project.tblschema \
  --sep list="|" --sep list_tuple2.tuple=":"

# ── 失败示例 ──
tablet-cli util validate-type "List<int>" "1;abc;3"
# → 错误: 第2项 "abc" 不是合法的 int 值
# exit 1

tablet-cli util validate-type "Map<str,int>" "name:abc"
# → 错误: key "name" 的 value "abc" 不是合法的 int 值
# exit 1
```

#### `util tbl-to-xlsx`

单个 .tbl 文件转换为 xlsx。

```bash
tablet-cli util tbl-to-xlsx HeroBase.tbl -o HeroBase.xlsx
tablet-cli util tbl-to-xlsx config/hero/HeroBase.tbl -o output/hero.xlsx
```

#### `util xlsx-to-tbl`

xlsx 转换回 .tbl（需要 schema 做 header 校验）。

```bash
tablet-cli util xlsx-to-tbl hero.xlsx --schema project.tblschema -o config/hero/
```

#### `util scaffold`

从 .tblschema 生成项目骨架（空 .tbl 文件，含表头无数据）。

```bash
tablet-cli util scaffold project.tblschema -o new_project/config/
tablet-cli util scaffold custom.tblschema -o /tmp/skeleton/
```

#### `util diff`

对比两个 .tbl 文件的结构和数据差异。

```bash
tablet-cli util diff old/HeroBase.tbl new/HeroBase.tbl
tablet-cli util diff v1/GlobalConst.tbl v2/GlobalConst.tbl
```

输出示例：
```
结构差异:
  + col 5: List<int> "skills" (新增列)
  ~ col 2: type int → float (类型变更)

数据差异 (3 行):
  行 3:  [1003, "兰斯洛特", ...] → [1003, "兰斯", ...]
  行 8:  (新增) [1008, "莫德雷德", ...]
  行 12: (删除) [1012, "临时测试", ...]
```

#### `util fmt`

格式化 .tbl 文件（解析后重新序列化，统一风格）。

```bash
# 输出到 stdout（预览）
tablet-cli util fmt HeroBase.tbl

# 原地修改
tablet-cli util fmt HeroBase.tbl -i

# 批量格式化目录下所有 .tbl
tablet-cli util fmt config/ -i
```

#### `util stat`

统计 .tbl 文件或目录的信息。

```bash
# 单文件
tablet-cli util stat HeroBase.tbl

# 目录递归
tablet-cli util stat config/

# JSON 格式
tablet-cli --fmt json util stat config/
```

单文件输出示例：
```
文件: HeroBase.tbl
类型: table
字段: 6 (int×2, str×3, List<int>×1)
数据行: 120
空值: 14 (1.9%)
```

目录输出示例：
```
目录: config/ (3 groups)
文件: 12 (table×8, constant×3, enum×1)
总字段: 45
总数据行: 890
总空值: 52 (1.3%)
```

---

### 8.10 模板与迁移

#### `list-templates`

列出可用模板（内置 + `<workdir>/tblschema/` 本地模板库）。

```bash
tablet-cli list-templates
tablet-cli -w D:/work/game-config list-templates
tablet-cli --fmt json list-templates
```

## 9. `-s` 覆盖键

`apply_overrides` 当前认识的键（实现见 `crates/app-cli/src/actions/overrides.rs`）：

| 键 | 覆盖到 |
|----|--------|
| `app.last_project` / `project.last_project` | `[project] last_project` |
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
| `export.server.go.custom_regions` | `[export.server.go] custom_regions` |
| `export.server.cpp.namespace` | `[export.server.cpp] namespace` |
| `export.server.cpp.code_output` | `[export.server.cpp] code_output` |
| `export.server.cpp.json_lib` | `[export.server.cpp] json_lib`（`nlohmann`；`rapidjson` 未实现，选了会导出报错） |
| `export.server.cpp.custom_regions` | `[export.server.cpp] custom_regions` |
| `export.server.java.lombok` | `[export.server.java] lombok`（只接受 `true` / `false`，默认 `true`） |
| `export.server.java.custom_regions` | `[export.server.java] custom_regions` |
| `export.server.csharp_dotnet.namespace` | `[export.server.csharp_dotnet] namespace` |
| `export.server.csharp_dotnet.code_output` | `[export.server.csharp_dotnet] code_output` |
| `export.server.csharp_dotnet.custom_regions` | `[export.server.csharp_dotnet] custom_regions` |
| `export.client.csharp_unity.namespace` | `[export.client.csharp_unity] namespace` |
| `export.client.csharp_unity.code_output` | `[export.client.csharp_unity] code_output` |
| `export.client.csharp_unity.custom_regions` | `[export.client.csharp_unity] custom_regions` |
| `export.client.csharp_godot.namespace` | `[export.client.csharp_godot] namespace` |
| `export.client.csharp_godot.code_output` | `[export.client.csharp_godot] code_output` |
| `export.client.csharp_godot.custom_regions` | `[export.client.csharp_godot] custom_regions` |
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

### 13.1 日常开发

```bash
# 查看当前项目结构
tablet-cli schema show

# 验证当前项目
tablet-cli validate

# 仅验证某个表
tablet-cli validate --group hero --node HeroBase

# 导出全部（数据 + 代码）
tablet-cli export all

# 仅导出 JSON 数据
tablet-cli export data --json

# 仅导出 hero 组的 JSON 数据
tablet-cli export data --json --group hero

# 仅导出 Java 代码
tablet-cli export code --java
```

### 13.2 项目管理

```bash
# 列出所有项目
tablet-cli project list

# 新建项目
tablet-cli project new --template slg-base --id slg-2026 --name "SLG 2026"

# 克隆项目做热修复
tablet-cli project clone --source slg-prod --id slg-hotfix --name "热修复"

# 项目详情
tablet-cli project info --id slg-test

# 重命名
tablet-cli project rename --id slg-test --new-name "SLG 测试服"

# 删除（需确认）
tablet-cli project delete --id temp-test --confirm
```

### 13.3 结构操作

```bash
# 添加分组和表
tablet-cli schema add-group --name skill
tablet-cli schema add-table --group skill --name SkillBase
tablet-cli schema add-enum --group skill --name SkillType
tablet-cli schema add-constant --group global --name BalanceConfig

# 重命名
tablet-cli schema rename-group --old hero --new character
tablet-cli schema rename-node --group hero --old HeroBase --new CharBase

# 删除
tablet-cli schema delete-node --group temp --name TestTable
tablet-cli schema delete-group --name temp
```

### 13.4 Excel 桥接

```bash
# 导出给策划编辑
tablet-cli excel export --group hero -o hero_edit.xlsx

# 策划改完后回读
tablet-cli excel import --group hero --file hero_edit.xlsx
```

### 13.5 分隔符查询

```bash
# 查看当前项目的生效分隔符
tablet-cli sep show

# 查看内置默认值
tablet-cli sep show --defaults

# 查看某 schema 覆盖后的值
tablet-cli sep show --schema projects/slg-test/project.tblschema
```

### 13.6 底层工具

```bash
# 解析 .tbl 文件
tablet-cli util parse-tbl config/hero/HeroBase.tbl

# 验证单个值
tablet-cli util validate-type "List<int>" "1;2;3"
tablet-cli util validate-type "Map<str,int>" "name:10;age:20" --sep map.kv="="

# 对比两个版本
tablet-cli util diff old/HeroBase.tbl new/HeroBase.tbl

# 格式化
tablet-cli util fmt config/ -i

# 统计
tablet-cli util stat config/

# 转换
tablet-cli util tbl-to-xlsx HeroBase.tbl -o HeroBase.xlsx
tablet-cli util scaffold new_schema.tblschema -o skeleton/
```

### 13.7 CI / Jenkins

```bash
# 标准 CI 流水线：验证 + 全量导出
tablet-cli -w D:/work/game-config --project slg-prod validate && \
tablet-cli -w D:/work/game-config --project slg-prod export all

# 全量导出到指定目录
tablet-cli --project slg-prod export all -o ./artifacts/

# 仅导出代码到临时目录（临时覆盖 package）
tablet-cli --project slg-prod export code --java --package com.release -o ./release/java/

# 仅导出数据
tablet-cli --project slg-prod export data --json -o ./release/data/

# 生成测试数据 + 验证
tablet-cli util gen-test --lang go --format json --schema project.tblschema -o ./test/

# JSON 输出供脚本解析
tablet-cli --fmt json validate
tablet-cli --fmt json project list

# 批量导出多项目（脚本遍历）
for p in slg-test slg-prod rpg-demo; do
  tablet-cli --project $p export all -o ./artifacts/$p/
done

# Jenkins 风格：通过 tablet.exe 跑 CLI（同一个 exe 兼任）
tablet.exe -w D:/work/game-config --project slg-prod export all
```
