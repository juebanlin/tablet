# Plan: 分隔符配置内嵌 .tblschema + 项目设置对话框

## Context

目前分隔符配置只存在 workspace 级 `tbl-tool.toml [separators]`，所有项目共用一套。后果：

- A 公司用 `Map.entry=,` 配出来的 `.tbl` 数据，B 公司打开（默认 `;`）会把整列误切成单值，校验大面积红
- "导入其它项目数据"完全不可用——格式不一致就直接炸
- 用户无法在 GUI 里看见 / 改这套配置；目前唯一办法是手编 toml

用户决策（已通过 `AskUserQuestion`）：

| 维度 | 选择 |
|---|---|
| 分隔符配置位置 | **只进 `.tblschema`**（schema = 单一身份源；项目间复制 schema 即可迁移） |
| 入口 | 项目右键 **「项目设置...」一锁**（合并旧"重命名..."），含 id/name/category/version + 分隔符 |
| Workspace `[separators]` 角色 | 仅作"新建项目时的起点默认值"，已建项目一律忽略它 |
| 旧项目兼容 | 不考虑，beta 阶段；既有 `project.toml [separators]` 段 → load 时**直接忽略** |

目标：每个 `.tblschema` 自描述其分隔符；项目右键单一入口编辑全部 meta + 分隔符，schema 一次写盘。

## 设计

### 1. `.tblschema` 分隔符语法

复用现有 `# @meta key: value` 直行风格，加 `# @sep` 平行 directive。**只识别第一个 [section] 之前的 `# @sep`**（与 `# @meta` 一致），后续 `#` 行当注释。

格式（25 个叶子键，对齐 `SeparatorsSection` 结构）：

```
# @sep Tuple2 = ,
# @sep Tuple3 = ,
# @sep Tuple4 = ,
# @sep List = ;
# @sep Set = ;
# @sep Map.kv = :
# @sep Map.entry = ;
# @sep List_Tuple2.tuple = ,
# @sep List_Tuple2.list = ;
# @sep List_Tuple3.tuple = ,
# @sep List_Tuple3.list = ;
# @sep List_Tuple4.tuple = ,
# @sep List_Tuple4.list = ;
# @sep Map_Tuple2.kv = :
# @sep Map_Tuple2.tuple = ,
# @sep Map_Tuple2.entry = ;
# @sep Map_Tuple3.kv = :
# @sep Map_Tuple3.tuple = ,
# @sep Map_Tuple3.entry = ;
# @sep Map_Tuple4.kv = :
# @sep Map_Tuple4.tuple = ,
# @sep Map_Tuple4.entry = ;
# @sep Map_List.kv = :
# @sep Map_List.item = ,
# @sep Map_List.entry = ;
```

- 顶级键名直接对应 `SeparatorsSection` 字段（`Tuple2 / List / Map ...`）
- 嵌套用点号：`Map.kv`、`Map_Tuple2.entry`
- 等号两侧 trim，**值原样保留**（不 trim，保护 ` ` 这种边界情况）—— 保险起见序列化时不写空格分隔符
- 未识别的 key：忽略（前向兼容）
- 未列出的 key：保持代码默认（`SeparatorsSection::default()`）

### 2. 数据模型（`crates/core`）

**`crates/core/src/types.rs`** — 给 `SeparatorsSection` 及其嵌套 `MapSep` / `ListTupleSep` / `MapTupleSep` / `MapListSep` 加 `Default + PartialEq` derive，便于"是否等于默认"比较。

**`crates/core/src/tblschema.rs`** — `TblSchema` 加字段：

```rust
pub struct TblSchema {
    pub meta: SchemaMetadata,
    pub separators: SeparatorsSection,   // 新增；初始 = default()
    pub sections: Vec<SchemaSection>,
}
```

不用 `Option`：`SeparatorsSection` 自己已经是"全字段都有 default"的结构。schema 没写的字段从 default 拿，等价于"没分隔符段"。

### 3. parser 改动（`crates/core/src/tblschema.rs::parse_tblschema`）

`# @meta` 那个分支旁边加 `# @sep` 兄弟分支：

```rust
if !seen_section {
    if let Some((key, value)) = parse_meta_line(trimmed) { ... }
    else if let Some((key, value)) = parse_sep_line(trimmed) {
        apply_sep_kv(&mut separators, &key, &value);
    }
}
```

`parse_sep_line` 仿 `parse_meta_line`：strip `# @sep`，按第一个 `=` 切。

`apply_sep_kv` 是个 match 表（25 case），每个 case 写一个字段。模板：

```rust
fn apply_sep_kv(sep: &mut SeparatorsSection, key: &str, value: &str) {
    match key {
        "Tuple2" => sep.tuple2 = value.into(),
        "Tuple3" => sep.tuple3 = value.into(),
        "Tuple4" => sep.tuple4 = value.into(),
        "List" => sep.list = value.into(),
        "Set" => sep.set = value.into(),
        "Map.kv" => sep.map.kv = value.into(),
        "Map.entry" => sep.map.entry = value.into(),
        "List_Tuple2.tuple" => sep.list_tuple2.tuple = value.into(),
        // ... 25 行表
        _ => { /* 未知 key 忽略 */ }
    }
}
```

构造 `Ok(TblSchema { meta, separators, sections })` 时把它带回去。

### 4. serializer 改动（`crates/core/src/tblschema.rs::serialize_tblschema`）

在 metadata 之后、第一个 section 之前输出 separators。**只输出与默认不同的字段**避免 noise：

```rust
let default_sep = SeparatorsSection::default();
write_sep_if_diff(&mut s, "Tuple2", &schema.separators.tuple2, &default_sep.tuple2);
write_sep_if_diff(&mut s, "Tuple3", &schema.separators.tuple3, &default_sep.tuple3);
// ... 25 行
```

新建项目时 `schema.separators = SeparatorsSection::default()` → 完全 diff = 空，schema 文件干净。用户在 GUI 改了某个值才会落字段。

### 5. 加载链路（`crates/core/src/project.rs`）

只改一处：`load_project` 流程在 `merge_project_config` 之后、构造 `Project` 之前，**用 `schema.separators` 直接覆盖 `cfg.separators`**：

```rust
let schema = parse_tblschema(&schema_text)?;
let mut cfg = merge_project_config(&global_text, project_text.as_deref())?;
cfg.separators = schema.separators.clone();   // schema 是单一来源
```

效果：
- 新建空项目 → schema 默认值 = workspace toml 默认值 = 代码常量，三者一致，零行为差
- 既有项目 `project.toml` 里写的 `[separators]` 永远不再读（即便存在）
- workspace `tbl-tool.toml [separators]` 仅在「新建项目」时被复制到 schema 里（见 §6）

**注**：`merge_project_config` 内部仍然读取 workspace toml `[separators]`，但结果立即被 schema 覆盖。保留它就是为了在「新建项目时把当前 workspace 默认值复制进 schema」这一步有完整的 25 键来源。

### 6. 新建项目链路（`crates/core/src/ops.rs::create_project_in_memory_with`）

把 workspace 当前的 `separators` 复制到新项目的 `schema.separators`。这样 workspace toml 仅作为"用户全局偏好的初始值"。

具体：找到内存项目构造点（engine 上的 `create_project_in_memory_with`），构造 `Project { schema, ... }` 时把 `schema.separators = self.workspace_default_separators.clone()` 或类似来源。

调研待确认：engine 是否有"全局 separators"挂在 self 上？看现有结构，似乎是每个 Project 自带 `config.separators`。可以约定：**新项目 schema.separators 用 `SeparatorsSection::default()`**。如果用户想要全局默认值不同，编辑 workspace toml 没意义——他们应当在「项目设置」里改。这条路更干净。

最终决策：**新项目 `schema.separators` 直接用 `SeparatorsSection::default()`**，根本不复制 workspace toml。workspace `tbl-tool.toml [separators]` 段从此变成历史遗留（保留不删，但任何代码路径都不再依赖它）。

### 7. 项目设置对话框（slint）

#### 7.1 入口收敛

`crates/app-slint/src/dialogs/context_menu.rs` 项目右键菜单：

```diff
  item("保存", "tree.proj-save", false),
  item("导出...", "tree.proj-export", false),
  item("导出 Schema...", "tree.proj-export-schema", false),
  item("合并 Schema...", "tree.proj-merge-schema", false),
  sep(),
  item("新建 Group", "tree.proj-new-group", false),
  item("复制(克隆)...", "tree.proj-clone", false),
  item_owned(paste_group_label.clone(), "tree.paste-group", !has_group_cb),
- item("重命名...", "tree.proj-rename", false),
+ item("项目设置...", "tree.proj-settings", false),
  item("删除", "tree.proj-delete", false),
```

废弃 `RenameProjectStage::EnterId / EnterName` 这套两段式 InputDialog 流程，从 `pending.rs` 删除分支。

#### 7.2 对话框结构

新文件 `crates/app-slint/ui/dialogs/project_settings.slint` —— 单对话框，顶部 2 tab：

```
┌─ DialogChrome：项目设置 ──────────────────────┐
│ [身份] [分隔符]                              │
├──────────────────────────────────────────┤
│ tab=身份：                                    │
│   项目 ID    [____]   ← 改 id 会 rename 目录 │
│   名称       [____]                          │
│   分类       [____]                          │
│   版本       [____]                          │
│   错误提示行                                  │
│                                            │
│ tab=分隔符（分组紧凑布局）：                    │
│   基础类型：Tuple2 [_] Tuple3 [_] Tuple4 [_] │
│             List [_] Set [_]                │
│   Map：     kv [_]  entry [_]                │
│   List_Tuple2：tuple [_] list [_]            │
│   List_Tuple3：...                           │
│   ...（25 个 LineEdit 总计）                  │
│   [恢复默认值]                               │
│                                  [取消] [确定] │
└──────────────────────────────────────────┘
```

每个 LineEdit 宽度固定 60–80px，紧凑塞满。

#### 7.3 数据模型（`crates/app-slint/src/state.rs`）

新增：

```rust
pub struct ProjectSettingsState {
    pub open: bool,
    pub project_id: String,        // 当前编辑哪个 project
    pub tab: i32,                  // 0=身份 1=分隔符
    // 身份字段（编辑缓冲）
    pub id_buf: String,
    pub name_buf: String,
    pub category_buf: String,
    pub version_buf: String,
    pub id_error: String,          // id 实时校验
    // 分隔符（编辑缓冲；25 个 String）
    pub sep: SeparatorsSection,
}
```

删除旧 `RenameProjectStage` 枚举 + `PendingAction::RenameProject` variant。

#### 7.4 写盘 + 业务逻辑（`crates/app-slint/src/dialogs/project_settings.rs` 新文件）

- `open_for(state, project_id)`：从 `engine.find_project` 拷出当前 meta + separators 作为 buf
- `push(ui, state)`：把 buf 投到 slint 端 properties
- `wire(ui, state)`：tab 切换 / 各字段 edited / 恢复默认 / 取消 / 确定 callback
- `run(state)`（确定）：
  1. 如果 id 改了 → 跑 `engine.execute_action(RenameProject { old_id, new_id, new_name })`（现有逻辑，含目录 rename + schema 写盘）
  2. 如果 name/category/version 改了 → 直接改 `project.schema.meta.*`
  3. 如果 separators 改了 → 改 `project.schema.separators` **和** `project.config.separators`（engine consumer 用的是 config 那个）
  4. `project.schema_dirty = true`
  5. 写 `project.tblschema`（`serialize_tblschema` 已自动 include separators）
  6. `state.engine.revalidate_all()` —— 分隔符变了要全表重校验
  7. 关对话框 + log

需要在 `crates/core/src/ops.rs` 加一个 `ProjectAction::EditProjectMeta { project_id, name, category, version, separators: SeparatorsSection }`，用以承载非 id 类改动。或者直接在 slint 端调一组 engine 上现有的 mutator API（`schema.meta.name = ...; schema_dirty = true; revalidate()`）。**选后者**，避免新加 ProjectAction —— 这不是命令式的"动作"，是聚合保存。

### 8. workspace toml 文档更新

`crates/core/src/project.rs::DEFAULT_CONFIG` 给 `[separators]` 段头加注释说明它已是死字段，仅保留以兼容老的 toml 解析：

```toml
[separators]
# ⚠ 已废弃：分隔符配置现在嵌入项目 .tblschema（# @sep 行），
# 通过 GUI「项目右键 → 项目设置 → 分隔符」编辑。这一段保留仅为了向前兼容，
# 实际加载时会被 .tblschema 内的 separators 完全覆盖。
Tuple2 = ","
...
```

不删除 `WorkspaceConfig.separators` 字段，避免破坏 `merge_project_config` —— 它仍参与解析（虽然结果被覆盖）。

### 9. 测试 / fixtures

- `tbl-tool/tests/lua/tbl-tool.toml` 等 7 份 fixtures：保留 `[separators]` 段不动；其 `project.tblschema`（如有）也不写 `# @sep` —— 加载得到 `SeparatorsSection::default()`，跟现状字节级一致，所有 expected_output.txt 不变
- 新增 core 单元测试 `tbl-tool/crates/core/src/tblschema.rs` 测试模块：
  - `parse_tblschema_with_sep_lines`：验证 `# @sep Map.entry = ,` 能正确覆写
  - `serialize_tblschema_omits_default_sep`：默认值不写到 schema 文件
  - `serialize_tblschema_writes_modified_sep`：改了 `Map.entry` 后 round-trip 出来还是逗号
- 现有 65/65 core tests 应继续 pass

### 10. 落地步骤（顺序，按依赖）

| # | 任务 | 主要文件 |
|---|---|---|
| A | `SeparatorsSection` 加 `Default + PartialEq` derive；嵌套子结构同步 | `crates/core/src/types.rs` |
| B | `TblSchema` 加 `separators` 字段 | `crates/core/src/tblschema.rs` |
| C | parser 加 `# @sep` 解析 + `apply_sep_kv` 25-case 表 | `crates/core/src/tblschema.rs::parse_tblschema` |
| D | serializer 加 separators diff 输出 | `crates/core/src/tblschema.rs::serialize_tblschema` |
| E | `load_project`：`cfg.separators = schema.separators` 覆盖 | `crates/core/src/project.rs::load_project` |
| F | core 单元测试（parse / serialize round-trip） | `crates/core/src/tblschema.rs` 测试模块 |
| G | slint state：`ProjectSettingsState` 加；删 `RenameProjectStage` + `PendingAction::RenameProject` | `crates/app-slint/src/state.rs` |
| H | 新对话框 slint 组件：`project_settings.slint`（2 tab + 表单 + 分隔符表格） | `crates/app-slint/ui/dialogs/project_settings.slint` |
| I | 对话框 wire / push / run | `crates/app-slint/src/dialogs/project_settings.rs` 新文件 |
| J | 项目右键 "重命名..." → "项目设置..."；pending.rs 删旧 RenameProject 分支 | `dialogs/context_menu.rs` / `dialogs/pending.rs` |
| K | app.slint 注册 ps-* properties / callbacks + dialog 实例 | `crates/app-slint/ui/app.slint` |
| L | main.rs 注册 wire；mod.rs 加新 module | `crates/app-slint/src/main.rs` / `dialogs/mod.rs` |
| M | refresh.rs `after_ctx_menu` 加 `project_settings::push` | `crates/app-slint/src/refresh.rs` |
| N | DEFAULT_CONFIG 注释 `[separators]` 段已废弃 | `crates/core/src/project.rs` |

### 11. 关键复用

- 现有 `engine.execute_action(ProjectAction::RenameProject { ... })` 处理 id 改名 + 目录 rename
- 现有 `serialize_tblschema(&schema)` 一次性写出全 meta + sections
- 现有 `engine.revalidate_all()` 或循环 `revalidate(group, name)` 跑全项目重校验
- 现有 `is_valid_metadata_id` 做 id 校验（在 `tblschema.rs`）

### 12. 验证

1. **构建**：`cargo build --release -p tbl-slint` 通过
2. **单元测试**：`cargo test --release -p tbl-core --lib` 65/65 + 3 个新 sep 测试
3. **CLI 回归**：`cargo test --release` 全包；fixtures 输出字节级不变
4. **手测** 5 条：
   - 老项目打开 → 分隔符显示成默认值；不操作时 schema 文件零变更
   - 「项目设置」打开 → 改 `Map.entry` 从 `;` 到 `,` → 确定 → schema 文件多出一行 `# @sep Map.entry = ,`；表格里 Map<str,int> 列重校验过
   - 「项目设置」改 id → 目录 rename + schema 写盘，行为同旧「重命名...」
   - 「项目设置」改 separator + name + category 同时 → 一次确定一次写盘
   - 把改过 separator 的项目目录复制成另一份（id 不同）→ 打开后分隔符值仍随 schema 来；不依赖原 workspace toml

### 13. 不在本轮范围

- 老项目自动迁移（把 `project.toml [separators]` → schema）—— beta 阶段不做
- workspace toml `[separators]` 字段彻底删除（保留以避免现有 toml 解析报错）
- "导入其它项目数据" 功能（这是分隔符可独立的**前置**改动，但功能本体后续再做）
- egui 后端：保持不变，已不再维护

## 关键文件汇总

**新建：**
- `crates/app-slint/src/dialogs/project_settings.rs`
- `crates/app-slint/ui/dialogs/project_settings.slint`

**修改：**
- `crates/core/src/types.rs` — `SeparatorsSection` 等加 derive
- `crates/core/src/tblschema.rs` — `TblSchema` 加字段；parser/serializer 加 sep
- `crates/core/src/project.rs` — `load_project` 用 schema 覆盖 cfg.separators；DEFAULT_CONFIG 注释
- `crates/app-slint/src/state.rs` — 加 `ProjectSettingsState`，删 `RenameProjectStage`
- `crates/app-slint/src/dialogs/context_menu.rs` — "重命名..." → "项目设置..."
- `crates/app-slint/src/dialogs/pending.rs` — 删 `RenameProject` 分支
- `crates/app-slint/src/dialogs/mod.rs` — 加 `project_settings`
- `crates/app-slint/src/main.rs` — 加 wire 调用
- `crates/app-slint/src/refresh.rs` — `after_ctx_menu` 加 project_settings push
- `crates/app-slint/ui/app.slint` — 注册 dialog 实例 + ps-* properties

**删除（废弃）：**
- 无文件级删除；仅枚举/字段层面：`RenameProjectStage` enum、`PendingAction::RenameProject`
