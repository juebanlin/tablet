//! Project 加载、扫描、迁移。
//!
//! 文档对应：@02 Project / @03.4 / @07 S15-D。
//!
//! 仓库级 `tbl-tool.toml` 是全局**默认值**（不随 Project 切换）。Project 自己的
//! 身份元数据存在 `<workdir>/projects/<id>/project.tblschema` 的 `# @meta` 段里
//! （id / name / created_at / source_template / source_template_version / category / version）。
//! 可选的配置覆盖（[export] / [ui] / [separators]）落在同目录下的 `project.toml`：
//!
//! ```toml
//! [export]                     # 可选：覆盖全局 [export]（field-level deep merge）
//! [export.server.java]
//! package = "com.foo.bar"
//! ```
//!
//! 加载顺序：项目 toml > 全局 toml > 内置默认。
//!
//! 老仓库的根 `config/` 在 `load_project` 时**自动迁移**到 `projects/default/config/`，
//! 不保留双结构。
//!
//! 历史文件名 `schema.tblschema` 在 load 时一次性 rename 为 `project.tblschema`，
//! 保持代码路径单一。
//!
//! 测试 fixtures（`tests/<scenario>/`）保持兼容：当 `<workdir>/projects/` 不存在
//! 但 `<workdir>/<config_dir>/` 存在时，把 workdir 自身当 project_root（"老布局模式"），
//! 这样 `tbl-cli generate-test` 在 fixtures 里仍可写 `config/` 平铺。
//! 新建项目 / GUI 启动一律走 projects/<id>/ 结构。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::model::*;
use crate::tbl::{self, TblFile};

pub const PROJECTS_DIR: &str = "projects";
pub const PROJECT_TOML_FILE: &str = "project.toml";
pub const PROJECT_SCHEMA_FILE: &str = "project.tblschema";

/// 极简 const-time string concat：`concat!` 仅吃字面量，无法直接拼 `&'static str` 常量。
/// 这里手工实现一份避免新增 crate 依赖。所有参数必须是 `&'static str`。
macro_rules! const_concat {
    ($($s:expr),+ $(,)?) => {{
        const PARTS: &[&str] = &[$($s),+];
        const TOTAL: usize = {
            let mut n = 0usize;
            let mut i = 0;
            while i < PARTS.len() { n += PARTS[i].len(); i += 1; }
            n
        };
        const BUF: [u8; TOTAL] = {
            let mut buf = [0u8; TOTAL];
            let mut pos = 0usize;
            let mut i = 0;
            while i < PARTS.len() {
                let bytes = PARTS[i].as_bytes();
                let mut j = 0;
                while j < bytes.len() {
                    buf[pos] = bytes[j];
                    pos += 1;
                    j += 1;
                }
                i += 1;
            }
            buf
        };
        // SAFETY: PARTS 全是合法 UTF-8 &str，按字节顺序拼接后仍为合法 UTF-8。
        unsafe { std::str::from_utf8_unchecked(&BUF) }
    }};
}

const LEGACY_SCHEMA_FILE: &str = "schema.tblschema";

/// 工作区根 `tbl-tool.toml` banner：说明三类段的作用域差异，避免用户混淆
/// `[project]`（仓库级）/ `[export][ui]`（项目默认值）/ `[separators]`（仅新建空项目时拷贝）。
const BANNER_GLOBAL: &str = r#"# ============================================================
# tbl-tool 工作区配置（仓库共享）
# ============================================================
#
# 本文件分三类段，作用域和覆盖关系不同：
#
#   [project]      ← 工作区状态（last_project / opened_projects 等）
#                     仓库级；不属于任何具体 Project，不会被 Project 覆盖
#
#   [export][ui]   ← Project 默认值
#                     新建 Project 继承本段；Project 用自己的
#                     projects/<id>/project.toml 按字段 deep-merge 覆盖
#                     （在这里改 = 影响所有未显式覆盖的 Project）
#
#   [separators]   ← 新建空 Project 时的分隔符初值
#                     拷一次到 project.tblschema 后即与本段无关；
#                     已有 Project 改分隔符走 GUI「项目右键 → 项目设置 → 分隔符」
# ============================================================

"#;

/// 项目级 `project.toml` banner：说明覆盖语义、并明确禁止的两段（[project]/[separators]）。
const BANNER_PROJECT: &str = r#"# ============================================================
# 项目级配置覆盖（projects/<id>/project.toml）
# ============================================================
#
# 与 <workdir>/tbl-tool.toml 按字段 deep-merge，项目优先；
# 缺失字段自动回退到全局值。所有键均已声明，照需修改即可。
#
# 注意：
#   - 不要在这里写 [project] 段：那是工作区状态，不属于单个项目
#   - 不要在这里写 [separators] 段：分隔符以 project.tblschema 的
#     # @sep 行为准（项目右键 → 项目设置 → 分隔符即编辑那里）
# ============================================================

"#;

/// 仓库级 `[project]` 段：last_project / opened_projects / 排序状态。
/// 仅出现在工作区根 toml；项目 toml 不写。
const PROJECT_SECTION_BLOCK: &str = r#"[project]
# 启动时进入的 Project id；为空 = 扫到的第一个
last_project = ""
# 启动时自动打开的 Project id 列表；为空 = 仅打开 last_project
opened_projects = []
# 项目排序方式: "id" (默认) / "name" / "open" / "created" / "manual"
project_sort = "id"
# project_sort = "manual" 时使用：用户拖拽得到的 id 序列
project_order = []

"#;

/// `[export]` 全段共享模板：覆盖 12 种导出 target（json/xml + 5 server + 5 client）。
/// DEFAULT_CONFIG 与 [`default_project_toml_template`] 共用，避免双份漂移。
/// 模板里的值与各 export 模块代码里 `unwrap_or(...)` 的 fallback 严格一致——
/// 改值前请同步 `crates/core/src/export/<lang>.rs`，否则会出现"模板说 X 实际是 Y"的混淆。
const EXPORT_SECTION_BLOCK: &str = r#"[export]
# 生成文件编码: utf-8 (默认)
encoding = "utf-8"
# 生成文件换行符: lf (默认), crlf
line_ending = "lf"

[export.json]
# 导出 JSON 时空值表达方式:
#   "null" - tbl 空值输出为 JSON null（默认）
#   "omit" - tbl 空值不写入 JSON，省略该字段
empty_as = "null"

[export.xml]
# 导出 XML 时空值表达方式:
#   "empty" - 输出空标签 <field></field>（默认）
#   "omit"  - 不写入空字段
empty_as = "empty"

[export.server]
# 后端数据文件（json / xml）输出目录；具体格式落到 <data_output>/json / <data_output>/xml
data_output = "gen/server/data"

[export.server.java]
# Java 包名
package = "com.game.config"
# Java 模板类输出根目录（包路径会自动拼到末尾，如 gen/server/java/com/game/config/...）
code_output = "gen/server/java"

[export.server.go]
# Go 包名
package = "config"
# Go 输出根目录（实际落 <code_output>/<package>/）
code_output = "gen/server/go"

[export.server.cpp]
# C++ 命名空间（支持嵌套，如 game::config::tpl）
namespace = "game::config"
# C++ 头文件输出目录（每张表 / 常量 / 枚举一个 .h）
code_output = "gen/server/cpp"

[export.server.csharp_dotnet]
# .NET 服务端命名空间
namespace = "Game.Config.Server"
# .NET 服务端代码输出目录
code_output = "gen/server/csharp"

[export.server.typescript]
# TypeScript Node.js 服务端代码输出目录
output = "gen/server/typescript"

[export.client.lua]
# Lua 文件输出目录（每张表 / 常量 / 枚举一个 .lua）
output = "gen/client"

[export.client.gdscript]
# GDScript 文件输出目录（每张表 / 常量 / 枚举一个 .gd）
output = "gen/client/gdscript"

[export.client.typescript]
# TypeScript 前端代码输出目录
output = "gen/client/typescript"

[export.client.csharp_unity]
# Unity 客户端命名空间
namespace = "Game.Config.Client"
# Unity 客户端代码输出目录
code_output = "gen/client/csharp_unity"

[export.client.csharp_godot]
# Godot 客户端命名空间
namespace = "Game.Config.Client"
# Godot 客户端代码输出目录
code_output = "gen/client/csharp_godot"

"#;

/// `[ui]` 全段共享模板：UiConfig + RefPickerConfig 全部字段。
/// DEFAULT_CONFIG 与 [`default_project_toml_template`] 共用。
const UI_SECTION_BLOCK: &str = r#"[ui]
# 点击空白区域时自动保存当前编辑 (false 则只有回车才保存)
auto_commit_on_blur = true
# 编辑单元格时实时验证 (true 则每次修改立即检查)
realtime_validate = false
# 日志文件级别: debug, info, warn, error
log_level = "debug"
# 表头 picker 单元格（Table type/export 行）的呼出方式：
#   "single" - 单击直接弹选择器（默认；表头每列一格，几乎不批量改）
#   "double" - 单击仅选中、双击才弹
picker_trigger_header = "single"
# 数据区 picker 单元格（Ref 列 / Constant type / export 列）的呼出方式：
#   "double" - 单击仅选中、双击才弹（默认；保留单击作为"瞄准选中"，
#              是 Ctrl+C/V 批量复制 ref id / enum 值的前提）
#   "single" - 单击直接弹选择器
picker_trigger_data = "double"
# 模板 / Project 列表展示 id 还是 name
#   false - 显示 name（默认）
#   true  - 显示 id（路径标识）
show_meta_id = false
# 是否允许 Constant 表使用 @Xxx 引用类型
#   true  - 允许（默认；常量值经常指向 table/enum 某项，如 default_hero = @HeroType:1）
#   false - 禁用（恢复早期行为，constant 段里的 @Xxx 会校验失败）
constant_ref_allowed = true

[ui.ref_picker]
# 引用选择弹窗对 Table 的列展示策略：
#   "auto" - id + 最多 2 个 export=cs 且类型为字符串的辅助列（默认）
#   "full" - schema 全部字段（除了 export=- 不导出列）
default_strategy = "auto"

"#;

/// `[separators]` 段：仅工作区根 toml 写。项目级在 .tblschema 的 `# @sep` 行。
const SEPARATORS_BLOCK: &str = r#"[separators]
# 程序级默认分隔符：仅在「新建空项目」时拷贝到 schema.separators 作为初值。
# 已加载项目走各自 .tblschema 的 # @sep 行（GUI「项目右键 → 项目设置 → 分隔符」编辑），
# 与本段无关。从模板/文件新建则继承 source schema 的 separators。
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

[separators.List_Tuple3]
tuple = ","
list = ";"

[separators.List_Tuple4]
tuple = ","
list = ";"

[separators.Map_Tuple2]
kv = ":"
tuple = ","
entry = ";"

[separators.Map_Tuple3]
kv = ":"
tuple = ","
entry = ";"

[separators.Map_Tuple4]
kv = ":"
tuple = ","
entry = ";"

[separators.Map_List]
kv = ":"
item = ","
entry = ";"
"#;

/// 工作区根 `tbl-tool.toml` 默认模板：首次启动自动落盘到 `<workdir>/tbl-tool.toml`。
/// banner + [project] + [export] + [ui] + [separators]。
const DEFAULT_CONFIG: &str = const_concat!(
    BANNER_GLOBAL,
    PROJECT_SECTION_BLOCK,
    EXPORT_SECTION_BLOCK,
    UI_SECTION_BLOCK,
    SEPARATORS_BLOCK,
);

/// 项目级 `project.toml` 默认模板：首次保存时写到 `<project_root>/project.toml`。
/// 已存在则不覆盖。包含 `[export]` / `[ui]` 两段全字段，不含 `[project]` / `[separators]`。
const PROJECT_TOML_TEMPLATE: &str = const_concat!(
    BANNER_PROJECT,
    EXPORT_SECTION_BLOCK,
    UI_SECTION_BLOCK,
);

pub fn default_project_toml_template() -> &'static str {
    PROJECT_TOML_TEMPLATE
}

/// 加载 Project（默认走 last_project 或扫描第一个）。
///
/// 流程：
/// 1. 解析 `<workdir>/tbl-tool.toml`（不存在则写默认）—— 全局 fallback
/// 2. 老布局迁移：若 `<workdir>/projects/` 不存在但 `<workdir>/<config_dir>/` 是非空目录，
///    把它视作 default project（不动盘上文件，仅在内存里把 project_root=workdir）。
///    GUI 端可在确认后调 [`migrate_legacy_to_default`] 真正搬目录。
/// 3. 多 Project 模式：从 `last_project` / 扫描结果挑一个，project_root = `<workdir>/projects/<id>/`
/// 4. 加载文件名迁移（schema.tblschema → project.tblschema）
/// 5. 解析 project.tblschema：meta → 项目身份；其它段（[export]/[ui]/...）由 project.toml deep-merge 到全局 config 上
/// 6. 扫 config/ 加载 groups
pub fn load_project(workdir: &Path) -> Result<Project> {
    let config_path = workdir.join(crate::CONFIG_FILE);

    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        println!("已生成默认配置文件: {}", config_path.display());
    }

    let global_text = std::fs::read_to_string(&config_path)?;

    // 决定 project_root
    let projects_dir = workdir.join(PROJECTS_DIR);
    let (project_root, schema, project_text) = if projects_dir.is_dir() {
        // 多 Project 模式：从 last_project 或扫描结果挑一个
        let candidates = scan_projects_dir(&projects_dir);
        // 先解析全局，仅为读 last_project；project.toml 的 overlay 一会再做
        let temp_cfg: WorkspaceConfig = toml::from_str(&global_text)?;
        let chosen = pick_project(&candidates, &temp_cfg.project.last_project)
            .with_context(|| format!("projects/ 目录下没有可用 Project: {}", projects_dir.display()))?;
        let root = projects_dir.join(&chosen.id);
        migrate_legacy_files(&root);
        let proj_text = std::fs::read_to_string(root.join(PROJECT_TOML_FILE)).ok();
        let schema = read_project_schema_with_fallback(&root, &chosen.id);
        (root, schema, proj_text)
    } else {
        // 老布局：workdir 自身就是 project_root，<config_dir> 即数据目录
        let mut schema = crate::tblschema::TblSchema::default();
        schema.meta.id = "default".to_string();
        schema.meta.name = "默认项目".to_string();
        (workdir.to_path_buf(), schema, None)
    };

    let mut config = merge_project_config(&global_text, project_text.as_deref())?;
    // 分隔符以 schema.separators 为单一来源（@docs/plans 方案）：
    // workspace tbl-tool.toml [separators] 与 project.toml [separators] 都被 schema 覆盖。
    config.separators = schema.separators.clone();

    // 决定 config 数据目录：多 Project 模式恒为 `<project_root>/config/`；
    // 老布局走 `<project_root>/<config_dir>`（即 `<workdir>/<config_dir>`）
    let data_dir = if projects_dir.is_dir() {
        project_root.join("config")
    } else {
        project_root.join(&config.project.config_dir)
    };

    let groups = if data_dir.is_dir() {
        load_groups_in(&data_dir)?
    } else {
        std::fs::create_dir_all(&data_dir)?;
        println!("已创建配置目录: {}", data_dir.display());
        Vec::new()
    };

    Ok(Project {
        workdir: workdir.to_path_buf(),
        project_root,
        config,
        schema,
        groups,
        schema_dirty: false,
        root_pending_create: false,
    })
}

/// 加载指定 id 的 Project（用于 Project 切换）。
pub fn load_specific_project(workdir: &Path, project_id: &str) -> Result<Project> {
    let config_path = workdir.join(crate::CONFIG_FILE);
    let global_text = std::fs::read_to_string(&config_path)?;

    let project_root = workdir.join(PROJECTS_DIR).join(project_id);
    if !project_root.is_dir() {
        anyhow::bail!("Project 不存在: {}", project_root.display());
    }
    migrate_legacy_files(&project_root);

    let proj_text = std::fs::read_to_string(project_root.join(PROJECT_TOML_FILE)).ok();
    let schema = read_project_schema_with_fallback(&project_root, project_id);

    let mut config = merge_project_config(&global_text, proj_text.as_deref())?;
    config.project.last_project = project_id.to_string();
    // 分隔符以 schema.separators 为唯一来源
    config.separators = schema.separators.clone();

    let data_dir = project_root.join("config");
    let groups = if data_dir.is_dir() { load_groups_in(&data_dir)? } else { Vec::new() };

    Ok(Project {
        workdir: workdir.to_path_buf(),
        project_root,
        config,
        schema,
        groups,
        schema_dirty: false,
        root_pending_create: false,
    })
}

/// 加载 workdir 下**全部** Project 到内存（多 Project 同时管理模型，@04.2.0）。
///
/// - 多 Project 模式：扫 `<workdir>/projects/` 列出全部 id；每个走 `load_specific_project` 全量加载
/// - 老布局：复用 `load_project`，得到单个 default Project 包成 Vec
/// - 没有 projects/ 也没有老 config/ 时 → load_project 会创建空 default
///
/// 返回的 Vec 顺序：按 project id 字典序（与 `list_projects` 一致）。
pub fn load_all_projects(workdir: &Path) -> Result<Vec<Project>> {
    let projects_dir = workdir.join(PROJECTS_DIR);
    if !projects_dir.is_dir() {
        let p = load_project(workdir)?;
        return Ok(vec![p]);
    }

    let candidates = scan_projects_dir(&projects_dir);
    if candidates.is_empty() {
        let p = load_project(workdir)?;
        return Ok(vec![p]);
    }

    let config_path = workdir.join(crate::CONFIG_FILE);
    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
    }

    let mut out = Vec::with_capacity(candidates.len());
    for entry in &candidates {
        match load_specific_project(workdir, &entry.id) {
            Ok(p) => out.push(p),
            Err(e) => eprintln!("warn: 加载 Project {} 失败: {}", entry.id, e),
        }
    }
    if out.is_empty() {
        anyhow::bail!("projects/ 下没有可加载的 Project");
    }
    Ok(out)
}

/// DBeaver-style 启动加载：扫描 available + 仅按 `[project] opened_projects` 加载若干个。
///
/// 决策逻辑：
/// 1. 若 `<workdir>/projects/` 不存在 → 走老 `load_project` 单 project 路径，available = 1 条 default。
/// 2. 否则扫 `<workdir>/projects/` 得到 `Vec<AvailableProject>`，按 id 字典序。
/// 3. 决定 `to_open`：
///    - `opened_projects` 非空：用它过滤掉不存在的 id（保持顺序）
///    - 否则若 `last_project` 非空且存在：仅打开它
///    - 否则打开 available 第一个（保持兼容，确保启动有内容）
/// 4. active = `last_project`（不在 opened 里则 fallback opened[0]）
pub fn load_workspace(workdir: &Path) -> Result<crate::ops::ProjectEngine> {
    use crate::ops::{AvailableProject, ProjectEngine};

    let projects_dir = workdir.join(PROJECTS_DIR);
    // 确保 tbl-tool.toml 存在（不存在则写默认）
    let config_path = workdir.join(crate::CONFIG_FILE);
    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        println!("已生成默认配置文件: {}", config_path.display());
    }
    let global_text = std::fs::read_to_string(&config_path)?;
    let workspace_cfg: WorkspaceConfig = toml::from_str(&global_text)?;

    if !projects_dir.is_dir() {
        // projects/ 不存在 = 空仓库；不再隐式落地 default project / 老布局兜底。
        return Ok(ProjectEngine::new_workspace(
            workdir.to_path_buf(),
            Vec::new(),
            Vec::new(),
            None,
        ));
    }

    let entries = scan_projects_dir(&projects_dir);
    let available: Vec<AvailableProject> = entries.iter()
        .map(|e| {
            // 顺手读 created_at（rescan_available 直接走 list_entry，缺 created_at；这里走完整读）
            let created_at = read_project_created_at(&e.root);
            AvailableProject {
                id: e.id.clone(),
                name: if e.name.is_empty() { e.id.clone() } else { e.name.clone() },
                root: e.root.clone(),
                created_at,
            }
        })
        .collect();

    if available.is_empty() {
        // projects/ 存在但为空（用户删光了所有 project）：保留空 workspace，
        // 不再隐式创建 default。否则启动时会冒出一个磁盘上不存在的"幽灵项目"。
        return Ok(ProjectEngine::new_workspace(
            workdir.to_path_buf(),
            Vec::new(),
            Vec::new(),
            None,
        ));
    }

    // 计算 to_open
    let last_project = workspace_cfg.project.last_project.clone();
    let opened_pref = &workspace_cfg.project.opened_projects;
    let avail_ids: std::collections::HashSet<&str> = available.iter().map(|a| a.id.as_str()).collect();
    let to_open: Vec<String> = if !opened_pref.is_empty() {
        opened_pref.iter()
            .filter(|id| avail_ids.contains(id.as_str()))
            .cloned().collect()
    } else if !last_project.is_empty() && avail_ids.contains(last_project.as_str()) {
        vec![last_project.clone()]
    } else {
        vec![available[0].id.clone()]
    };

    let mut opened: Vec<Project> = Vec::with_capacity(to_open.len());
    for id in &to_open {
        match load_specific_project(workdir, id) {
            Ok(p) => opened.push(p),
            Err(e) => eprintln!("warn: 加载 Project {} 失败: {}", id, e),
        }
    }

    let active_id = if !last_project.is_empty() && opened.iter().any(|p| p.schema.meta.id == last_project) {
        Some(last_project)
    } else {
        opened.first().map(|p| p.schema.meta.id.clone())
    };
    let mut engine = ProjectEngine::new_workspace(
        workdir.to_path_buf(),
        available,
        opened,
        active_id.as_deref(),
    );
    // workspace tbl-tool.toml [separators] 作为「新建空项目」时 schema.separators 的初值；
    // toml 里没写的字段走 SeparatorsSection 自身 default。
    engine.set_default_separators(workspace_cfg.separators.clone());
    Ok(engine)
}

/// 把当前 engine 的 opened/active/sort/order 落盘到 `<workdir>/tbl-tool.toml`。
/// UI 在 open/close/sort 改变时调一次；失败由调用方决定如何处理（best-effort）。
pub fn persist_workspace_state(
    engine: &crate::ops::ProjectEngine,
    project_sort: &str,
    project_order: &[String],
) -> Result<()> {
    let path = engine.workdir.join(crate::CONFIG_FILE);
    let original = std::fs::read_to_string(&path).unwrap_or_default();
    // 取 config_dir / cache_dir：优先从已加载 project 的 config 拿，全关时反序列化原 toml
    let (config_dir, cache_dir) = if let Some(p) = engine.active() {
        (
            p.config.project.config_dir.clone(),
            p.config.project.cache_dir.clone(),
        )
    } else if let Some(p) = engine.projects.first() {
        (
            p.config.project.config_dir.clone(),
            p.config.project.cache_dir.clone(),
        )
    } else {
        match toml::from_str::<WorkspaceConfig>(&original) {
            Ok(c) => (c.project.config_dir, c.project.cache_dir),
            Err(_) => ("config".to_string(), ".tbl-cache".to_string()),
        }
    };
    let project_cfg = ProjectConfig {
        last_project: engine.active_project_id().unwrap_or("").to_string(),
        opened_projects: engine.opened_ids(),
        project_sort: project_sort.to_string(),
        project_order: project_order.to_vec(),
        config_dir,
        cache_dir,
    };
    let updated = upsert_project_config_section(&original, &project_cfg);
    std::fs::write(&path, updated)?;
    Ok(())
}

/// 把全局 tbl-tool.toml 与 project.toml deep-merge 后反序列化成 WorkspaceConfig。
/// project.toml 字段优先；缺失字段从全局取。
fn merge_project_config(global_text: &str, project_text: Option<&str>) -> Result<WorkspaceConfig> {
    let mut global_val: toml::Value = toml::from_str(global_text)?;
    if let Some(pt) = project_text {
        let mut proj_val: toml::Value = toml::from_str(pt).unwrap_or(toml::Value::Table(Default::default()));
        // [project] 段在两边语义不同：全局是仓库级 ProjectConfig；
        // project.toml 里则是 ProjectInstanceMeta（不参与 WorkspaceConfig 反序列化），剥掉
        if let toml::Value::Table(ref mut tbl) = proj_val {
            tbl.remove("project");
        }
        deep_merge_toml(&mut global_val, proj_val);
    }
    Ok(global_val.try_into::<WorkspaceConfig>()?)
}

/// 深度合并：override 的 table 字段递归合 base；非 table 值直接覆盖；override 缺失字段保留 base。
fn deep_merge_toml(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(existing) => deep_merge_toml(existing, v),
                    None => { b.insert(k, v); }
                }
            }
        }
        (slot, v) => { *slot = v; }
    }
}

/// 老文件名迁移：rename 一次到位，让后续代码只面对新名。
/// 仅当老文件存在且新文件不存在时才动；幂等。
fn migrate_legacy_files(project_root: &Path) {
    let pairs = [
        (LEGACY_SCHEMA_FILE, PROJECT_SCHEMA_FILE),
    ];
    for (old, new) in pairs {
        let old_path = project_root.join(old);
        let new_path = project_root.join(new);
        if old_path.exists() && !new_path.exists() {
            if let Err(e) = std::fs::rename(&old_path, &new_path) {
                eprintln!("warn: 迁移 {} → {} 失败: {}", old_path.display(), new_path.display(), e);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectListEntry {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// 扫描 `<workdir>/projects/` 列出全部 Project。返回按 id 字典序。
pub fn list_projects(workdir: &Path) -> Vec<ProjectListEntry> {
    let projects_dir = workdir.join(PROJECTS_DIR);
    scan_projects_dir(&projects_dir)
}

fn scan_projects_dir(projects_dir: &Path) -> Vec<ProjectListEntry> {
    if !projects_dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(projects_dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if id.starts_with('.') {
            continue;
        }
        let meta = read_project_meta_short(&path);
        let name = meta.map(|(_, n)| if n.is_empty() { id.clone() } else { n }).unwrap_or_else(|| id.clone());
        out.push(ProjectListEntry { id, name, root: path });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn pick_project<'a>(candidates: &'a [ProjectListEntry], preferred: &str) -> Option<&'a ProjectListEntry> {
    if !preferred.is_empty() {
        if let Some(found) = candidates.iter().find(|c| c.id == preferred) {
            return Some(found);
        }
    }
    candidates.first()
}

/// 读 `<project_root>/project.tblschema` 解析出 schema；缺失或解析失败时返回带 id/name 兜底
/// 的空 schema（id = project_id_fallback、name = project_id_fallback）。
fn read_project_schema_with_fallback(
    project_root: &Path,
    project_id_fallback: &str,
) -> crate::tblschema::TblSchema {
    let path = project_root.join(PROJECT_SCHEMA_FILE);
    let txt = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return fallback_schema(project_id_fallback),
    };
    match crate::tblschema::parse_tblschema(&txt) {
        Ok(mut s) => {
            if s.meta.id.is_empty() {
                s.meta.id = project_id_fallback.to_string();
            }
            if s.meta.name.is_empty() {
                s.meta.name = s.meta.id.clone();
            }
            s
        }
        Err(_) => fallback_schema(project_id_fallback),
    }
}

fn fallback_schema(id: &str) -> crate::tblschema::TblSchema {
    let mut s = crate::tblschema::TblSchema::default();
    s.meta.id = id.to_string();
    s.meta.name = id.to_string();
    s
}

/// 仅读取 schema.meta 的 (id, name)，用于 scan_projects_dir 列表展示与 created_at 取值。
/// 失败时返回 None；调用方按目录名兜底。
fn read_project_meta_short(project_root: &Path) -> Option<(String, String)> {
    let s = read_project_schema_with_fallback(project_root, "");
    if s.meta.id.is_empty() {
        return None;
    }
    Some((s.meta.id, s.meta.name))
}

/// 读取 schema.meta.created_at（可空）。供 load_workspace 填 AvailableProject。
fn read_project_created_at(project_root: &Path) -> String {
    read_project_schema_with_fallback(project_root, "").meta.created_at
}

/// 把仓库根的老 `<workdir>/<config_dir>/` 迁移到 `<workdir>/projects/default/config/`。
///
/// - 仅当 `<workdir>/projects/` 不存在 **且** 老 config_dir 为非空目录时执行。
/// - 创建 `project.tblschema`（id=default / name="默认项目"）。
/// - 修改 `tbl-tool.toml` 的 `[project] last_project = "default"`。
///
/// 不会复制额外的 schema/缓存（不在迁移范围）。
pub fn migrate_legacy_to_default(workdir: &Path) -> Result<bool> {
    let projects_dir = workdir.join(PROJECTS_DIR);
    if projects_dir.is_dir() {
        return Ok(false); // 已有 projects/，不迁移
    }
    let config_path = workdir.join(crate::CONFIG_FILE);
    let config_str = std::fs::read_to_string(&config_path)?;
    let mut config: WorkspaceConfig = toml::from_str(&config_str)?;

    let legacy_data = workdir.join(&config.project.config_dir);
    if !legacy_data.is_dir() {
        return Ok(false);
    }

    // 移动目录
    let target_root = projects_dir.join("default");
    let target_data = target_root.join("config");
    std::fs::create_dir_all(&target_root)?;
    rename_dir(&legacy_data, &target_data)
        .with_context(|| format!("迁移失败: {} → {}", legacy_data.display(), target_data.display()))?;

    // 写元数据：直接写 project.tblschema（meta 段，sections 留空）
    let mut schema = crate::tblschema::TblSchema::default();
    schema.meta.id = "default".to_string();
    schema.meta.name = "默认项目".to_string();
    schema.meta.created_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let schema_path = target_root.join(PROJECT_SCHEMA_FILE);
    std::fs::write(&schema_path, crate::tblschema::serialize_tblschema(&schema))?;

    // 更新 last_project
    config.project.last_project = "default".to_string();
    write_tool_config(&config_path, &config)?;

    println!("已迁移老配置目录到 projects/default/");
    Ok(true)
}

fn rename_dir(from: &Path, to: &Path) -> Result<()> {
    // 同卷直接 rename，跨卷或失败时回退到逐文件复制 + 删除原。
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_all(from, to)?;
            std::fs::remove_dir_all(from)?;
            Ok(())
        }
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// 重写 tbl-tool.toml（保留所有段，仅刷新 [project]）。
/// 这里走简单方案：用 toml::to_string_pretty 整体序列化。
fn write_tool_config(path: &Path, config: &WorkspaceConfig) -> Result<()> {
    // serde::Serialize 不在 WorkspaceConfig 上：手工写 [project] + 透传其它内容会丢评论。
    // 简化方案：读原文件，按行 upsert [project] 段字段。既保住注释又改对字段。
    let original = std::fs::read_to_string(path)?;
    let updated = upsert_project_config_section(&original, &config.project);
    std::fs::write(path, updated)?;
    Ok(())
}

/// 把 toml 文本里的 `[project]` 段 upsert：
/// `last_project / opened_projects / project_sort / project_order` 字段写新值，其它保持原状。
/// 不再兼容历史 `[app]` 段——上层应当先 migrate。
pub fn upsert_project_config_section(original: &str, project: &ProjectConfig) -> String {
    let app_lines = build_project_field_lines(project);
    let field_keys = ["last_project", "opened_projects", "project_sort", "project_order"];
    let mut written: std::collections::HashSet<&str> = std::collections::HashSet::new();

    let mut out_lines: Vec<String> = Vec::new();
    let mut in_app_section = false;
    let mut found_app = false;
    let flush_missing = |written: &std::collections::HashSet<&str>, out: &mut Vec<String>| {
        for key in field_keys.iter() {
            if !written.contains(key) {
                if let Some(line) = app_lines.get(*key) {
                    out.push(line.clone());
                }
            }
        }
    };

    for line in original.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // 离开上一段：若是 [project] 段，把没写过的字段补上
            if in_app_section {
                flush_missing(&written, &mut out_lines);
                written.clear();
            }
            // 进入新段
            if trimmed == "[project]" {
                in_app_section = true;
                found_app = true;
                out_lines.push("[project]".to_string());
                continue;
            }
            in_app_section = false;
        }

        if in_app_section {
            let mut handled = false;
            for key in field_keys.iter() {
                if trimmed.starts_with(key) {
                    let after = trimmed[key.len()..].trim_start();
                    if after.starts_with('=') {
                        if let Some(line) = app_lines.get(*key) {
                            out_lines.push(line.clone());
                        }
                        written.insert(*key);
                        handled = true;
                        break;
                    }
                }
            }
            if handled { continue; }
        }

        out_lines.push(line.to_string());
    }

    // EOF 时仍在 [project] 段
    if in_app_section {
        flush_missing(&written, &mut out_lines);
    }

    if !found_app {
        // 完全没有 [project] 段：在头部插一段
        let mut head = vec![
            "[project]".to_string(),
        ];
        for key in field_keys.iter() {
            if let Some(line) = app_lines.get(*key) {
                head.push(line.clone());
            }
        }
        head.push(String::new());
        head.extend(out_lines);
        return head.join("\n") + "\n";
    }

    let mut joined = out_lines.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// 给 [project] 段每个字段拼出一行 toml 文本（用于 upsert）。
fn build_project_field_lines(project: &ProjectConfig) -> std::collections::HashMap<&'static str, String> {
    let mut m = std::collections::HashMap::new();
    m.insert("last_project", format!("last_project = \"{}\"", escape_toml_string(&project.last_project)));
    let opened = project.opened_projects.iter()
        .map(|s| format!("\"{}\"", escape_toml_string(s)))
        .collect::<Vec<_>>().join(", ");
    m.insert("opened_projects", format!("opened_projects = [{}]", opened));
    m.insert("project_sort", format!("project_sort = \"{}\"", escape_toml_string(&project.project_sort)));
    let order = project.project_order.iter()
        .map(|s| format!("\"{}\"", escape_toml_string(s)))
        .collect::<Vec<_>>().join(", ");
    m.insert("project_order", format!("project_order = [{}]", order));
    m
}

fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn load_groups_in(data_dir: &Path) -> Result<Vec<Group>> {
    let mut groups = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(data_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let group = load_group(&name, &dir)?;
        groups.push(group);
    }
    Ok(groups)
}

fn load_group(name: &str, dir: &Path) -> Result<Group> {
    let mut tables = Vec::new();
    let mut constants = Vec::new();
    let mut enums = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "tbl"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        match tbl::parse_tbl(&path) {
            Ok(TblFile::Table(t)) => tables.push(t),
            Ok(TblFile::Constant(c)) => constants.push(c),
            Ok(TblFile::Enum(e)) => enums.push(e),
            Err(e) => eprintln!("warn: failed to parse {}: {}", path.display(), e),
        }
    }

    Ok(Group {
        name: name.to_string(),
        dir: dir.to_path_buf(),
        tables,
        constants,
        enums,
        is_new: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_tmp(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("tblproj_{}_{}_{}", label, std::process::id(), n))
    }

    #[test]
    fn legacy_layout_loads_workdir_as_root() {
        let dir = unique_tmp("legacy");
        std::fs::create_dir_all(dir.join("config/hero")).unwrap();
        std::fs::write(dir.join("tbl-tool.toml"), "[project]\nname = \"x\"\nlast_project = \"\"\nconfig_dir = \"config\"\ncache_dir = \".tbl-cache\"\n").unwrap();
        let proj = load_project(&dir).expect("load");
        assert_eq!(proj.project_root, dir);
        assert_eq!(proj.schema.meta.id, "default");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_section_round_trip() {
        let dir = unique_tmp("alias");
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nconfig_dir = \"config\"\ncache_dir = \".tbl-cache\"\n",
        )
        .unwrap();
        let proj = load_project(&dir).expect("load");
        assert_eq!(proj.config.project.config_dir, "config");
        assert_eq!(proj.config.project.cache_dir, ".tbl-cache");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_schema(root: &Path, id: &str, name: &str) {
        std::fs::write(
            root.join(PROJECT_SCHEMA_FILE),
            format!("#!tblschema v1\n# @meta id: {}\n# @meta name: {}\n", id, name),
        ).unwrap();
    }

    #[test]
    fn projects_dir_picks_last_project() {
        let dir = unique_tmp("multi");
        std::fs::create_dir_all(dir.join("projects/p1/config")).unwrap();
        std::fs::create_dir_all(dir.join("projects/p2/config")).unwrap();
        write_schema(&dir.join("projects/p1"), "p1", "项目1");
        write_schema(&dir.join("projects/p2"), "p2", "项目2");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"p2\"\n",
        )
        .unwrap();
        let proj = load_project(&dir).expect("load");
        assert_eq!(proj.schema.meta.id, "p2");
        assert_eq!(proj.project_root, dir.join("projects/p2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn projects_dir_falls_back_to_first_when_last_invalid() {
        let dir = unique_tmp("fallback");
        std::fs::create_dir_all(dir.join("projects/aaa/config")).unwrap();
        std::fs::create_dir_all(dir.join("projects/bbb/config")).unwrap();
        write_schema(&dir.join("projects/aaa"), "aaa", "a");
        write_schema(&dir.join("projects/bbb"), "bbb", "b");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"ghost\"\n",
        )
        .unwrap();
        let proj = load_project(&dir).expect("load");
        assert_eq!(proj.schema.meta.id, "aaa");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_moves_legacy_config_to_projects_default() {
        let dir = unique_tmp("migrate");
        std::fs::create_dir_all(dir.join("config/hero")).unwrap();
        std::fs::write(dir.join("config/hero/HeroBase.tbl"), "#!tbl v2\n").unwrap();
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"\"\nconfig_dir = \"config\"\n",
        )
        .unwrap();

        let migrated = migrate_legacy_to_default(&dir).expect("migrate");
        assert!(migrated);

        // 新位置存在
        assert!(dir.join("projects/default/config/hero/HeroBase.tbl").exists());
        // 元数据写好（schema 文件而非 toml）
        assert!(dir.join("projects/default/project.tblschema").exists());
        // 老 config 已清掉
        assert!(!dir.join("config").exists());

        // tbl-tool.toml 的 last_project 被刷新
        let cfg = std::fs::read_to_string(dir.join("tbl-tool.toml")).unwrap();
        assert!(cfg.contains("last_project = \"default\""));

        // 后续 load 从 projects/default 加载
        let proj = load_project(&dir).expect("load");
        assert_eq!(proj.schema.meta.id, "default");
        assert_eq!(proj.project_root, dir.join("projects/default"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_noop_when_projects_dir_already_exists() {
        let dir = unique_tmp("nomigrate");
        std::fs::create_dir_all(dir.join("projects/x/config")).unwrap();
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"\"\nconfig_dir = \"config\"\n",
        )
        .unwrap();
        let result = migrate_legacy_to_default(&dir).expect("migrate");
        assert!(!result);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_project_config_keeps_section_header() {
        let original = "[project]\nname = \"x\"\n\n[ui]\nlog_level = \"debug\"\n";
        let project = ProjectConfig {
            last_project: "p1".to_string(),
            opened_projects: Vec::new(),
            project_sort: String::new(),
            project_order: Vec::new(),
            config_dir: "config".to_string(),
            cache_dir: ".tbl-cache".to_string(),
        };
        let result = upsert_project_config_section(original, &project);
        assert!(result.contains("[project]"));
        assert!(result.contains("last_project = \"p1\""));
        assert!(result.contains("[ui]"));
        assert!(result.contains("log_level"));
    }

    #[test]
    fn upsert_project_config_replaces_existing_last_project() {
        let original = "[project]\nname = \"x\"\nlast_project = \"old\"\n";
        let project = ProjectConfig {
            last_project: "new".to_string(),
            opened_projects: Vec::new(),
            project_sort: String::new(),
            project_order: Vec::new(),
            config_dir: "config".to_string(),
            cache_dir: ".tbl-cache".to_string(),
        };
        let result = upsert_project_config_section(original, &project);
        assert!(result.contains("last_project = \"new\""));
        assert!(!result.contains("last_project = \"old\""));
    }

    #[test]
    fn upsert_project_config_inserts_when_missing() {
        let original = "[ui]\nlog_level = \"debug\"\n";
        let project = ProjectConfig {
            last_project: "p1".to_string(),
            opened_projects: Vec::new(),
            project_sort: String::new(),
            project_order: Vec::new(),
            config_dir: "config".to_string(),
            cache_dir: ".tbl-cache".to_string(),
        };
        let result = upsert_project_config_section(original, &project);
        assert!(result.starts_with("[project]"));
        assert!(result.contains("last_project = \"p1\""));
        assert!(result.contains("[ui]"));
    }

    #[test]
    fn upsert_project_config_persists_opened_and_order() {
        let original = "[project]\nname = \"x\"\nlast_project = \"\"\n";
        let project = ProjectConfig {
            last_project: "p1".to_string(),
            opened_projects: vec!["p1".to_string(), "p2".to_string()],
            project_sort: "manual".to_string(),
            project_order: vec!["p2".to_string(), "p1".to_string()],
            config_dir: "config".to_string(),
            cache_dir: ".tbl-cache".to_string(),
        };
        let result = upsert_project_config_section(original, &project);
        assert!(result.contains("opened_projects = [\"p1\", \"p2\"]"));
        assert!(result.contains("project_sort = \"manual\""));
        assert!(result.contains("project_order = [\"p2\", \"p1\"]"));
    }

    #[test]
    fn list_projects_sorts_by_id() {
        let dir = unique_tmp("listorder");
        std::fs::create_dir_all(dir.join("projects/zeta")).unwrap();
        std::fs::create_dir_all(dir.join("projects/alpha")).unwrap();
        std::fs::create_dir_all(dir.join("projects/beta")).unwrap();
        let list = list_projects(&dir);
        let ids: Vec<_> = list.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "beta", "zeta"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_toml_overlay_overrides_global_export() {
        let dir = unique_tmp("overlay");
        std::fs::create_dir_all(dir.join("projects/p/config")).unwrap();
        write_schema(&dir.join("projects/p"), "p", "P");
        std::fs::write(
            dir.join("projects/p/project.toml"),
            "[export.server.java]\npackage = \"com.over.ride\"\n",
        ).unwrap();
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"p\"\n\n[export.server]\ndata_output = \"gen/server/data\"\n[export.server.java]\npackage = \"com.global\"\ncode_output = \"gen/server/java\"\n",
        ).unwrap();
        let proj = load_project(&dir).expect("load");
        let java = proj.config.export.as_ref().unwrap().server.as_ref().unwrap().java.as_ref().unwrap();
        // overlay 覆盖
        assert_eq!(java.package.as_deref(), Some("com.over.ride"));
        // 全局保留（overlay 没动 code_output）
        assert_eq!(java.code_output.as_deref(), Some("gen/server/java"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_project(dir: &Path, id: &str, name: &str) {
        let root = dir.join("projects").join(id);
        std::fs::create_dir_all(root.join("config")).unwrap();
        write_schema(&root, id, name);
    }

    #[test]
    fn load_workspace_only_opens_last_project_by_default() {
        let dir = unique_tmp("ws_last_only");
        write_project(&dir, "p1", "项目1");
        write_project(&dir, "p2", "项目2");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"p2\"\n",
        ).unwrap();

        let engine = load_workspace(&dir).expect("load_workspace");
        assert_eq!(engine.available().len(), 2);
        assert_eq!(engine.projects.len(), 1, "只打开 last_project");
        assert_eq!(engine.projects[0].schema.meta.id, "p2");
        assert_eq!(engine.active_project_id(), Some("p2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_workspace_respects_opened_projects_order() {
        let dir = unique_tmp("ws_opened_order");
        write_project(&dir, "p1", "项目1");
        write_project(&dir, "p2", "项目2");
        write_project(&dir, "p3", "项目3");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"p2\"\nopened_projects = [\"p2\", \"p1\"]\n",
        ).unwrap();

        let engine = load_workspace(&dir).expect("load_workspace");
        let ids = engine.opened_ids();
        assert_eq!(ids, vec!["p2".to_string(), "p1".to_string()]);
        assert_eq!(engine.active_project_id(), Some("p2"));
        // available 按 id 字典序
        assert_eq!(engine.available()[0].id, "p1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_workspace_includes_closed_in_available() {
        let dir = unique_tmp("ws_avail_closed");
        write_project(&dir, "alpha", "A");
        write_project(&dir, "beta", "B");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"alpha\"\n",
        ).unwrap();

        let engine = load_workspace(&dir).expect("load_workspace");
        assert_eq!(engine.available().len(), 2);
        assert_eq!(engine.projects.len(), 1);
        assert!(!engine.is_opened("beta"));
        assert!(engine.is_opened("alpha"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_project_appends_and_does_not_change_active() {
        let dir = unique_tmp("ws_open_append");
        write_project(&dir, "a", "A");
        write_project(&dir, "b", "B");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"a\"\n",
        ).unwrap();

        let mut engine = load_workspace(&dir).expect("load_workspace");
        assert_eq!(engine.active_project_id(), Some("a"));
        let opened = engine.open_project("b").expect("open b");
        assert!(opened);
        assert!(engine.is_opened("b"));
        // active 不动
        assert_eq!(engine.active_project_id(), Some("a"));
        // 二次 open 返回 false
        assert!(!engine.open_project("b").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn close_active_project_clears_active() {
        let dir = unique_tmp("ws_close_active");
        write_project(&dir, "a", "A");
        write_project(&dir, "b", "B");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"a\"\nopened_projects = [\"a\", \"b\"]\n",
        ).unwrap();

        let mut engine = load_workspace(&dir).expect("load_workspace");
        assert_eq!(engine.active_project_id(), Some("a"));
        let closed = engine.close_project("a");
        assert!(closed);
        assert!(!engine.is_opened("a"));
        assert!(engine.is_opened("b"), "b 仍在 opened");
        // 关 active → active 切 None（用户后续可手动 set_active）
        assert_eq!(engine.active_project_id(), None);
        // available 不动
        assert_eq!(engine.available().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn close_all_projects_yields_empty_active() {
        let dir = unique_tmp("ws_close_all");
        write_project(&dir, "only", "Only");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"x\"\nlast_project = \"only\"\n",
        ).unwrap();
        let mut engine = load_workspace(&dir).expect("load_workspace");
        engine.close_project("only");
        assert!(engine.projects.is_empty());
        assert_eq!(engine.active_project_id(), None);
        assert_eq!(engine.available().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persist_workspace_state_round_trip() {
        let dir = unique_tmp("ws_persist");
        write_project(&dir, "p1", "1");
        write_project(&dir, "p2", "2");
        std::fs::write(
            dir.join("tbl-tool.toml"),
            "[project]\nname = \"my-game\"\nlast_project = \"p1\"\n",
        ).unwrap();

        let mut engine = load_workspace(&dir).expect("load");
        engine.open_project("p2").unwrap();
        engine.set_active_by_id("p2");
        persist_workspace_state(&engine, "name", &["p2".to_string(), "p1".to_string()])
            .expect("persist");

        // re-load
        let engine2 = load_workspace(&dir).expect("reload");
        let opened = engine2.opened_ids();
        assert!(opened.contains(&"p1".to_string()));
        assert!(opened.contains(&"p2".to_string()));
        assert_eq!(engine2.active_project_id(), Some("p2"));

        // toml 里 sort/order 已写入
        let txt = std::fs::read_to_string(dir.join("tbl-tool.toml")).unwrap();
        assert!(txt.contains("project_sort = \"name\""));
        assert!(txt.contains("project_order = [\"p2\", \"p1\"]"));
        // opened_projects 顺序按打开顺序：先 p1（last_project），再 open p2
        assert!(txt.contains("opened_projects = [\"p1\", \"p2\"]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_config_parses_as_workspace_config() {
        let cfg: WorkspaceConfig = toml::from_str(DEFAULT_CONFIG)
            .expect("DEFAULT_CONFIG must parse as WorkspaceConfig");
        let export = cfg.export.expect("[export] present");
        let server = export.server.expect("[export.server] present");
        // 服务端全部 5 项可解析
        assert!(server.java.is_some());
        assert!(server.go.is_some());
        assert!(server.cpp.is_some());
        assert!(server.csharp_dotnet.is_some());
        assert!(server.typescript.is_some());
        let client = export.client.expect("[export.client] present");
        // 客户端全部 5 项可解析
        assert!(client.lua.is_some());
        assert!(client.gdscript.is_some());
        assert!(client.typescript.is_some());
        assert!(client.csharp_unity.is_some());
        assert!(client.csharp_godot.is_some());
        // 数据导出 2 项
        assert!(export.json.is_some());
        assert!(export.xml.is_some());
        // 默认值与 export 模块代码 fallback 严格一致——任何漂移都让用户复制注释行后的值反而改了行为。
        assert_eq!(
            server.java.as_ref().unwrap().package.as_deref(),
            Some("com.game.config")
        );
        assert_eq!(
            server.cpp.as_ref().unwrap().namespace.as_deref(),
            Some("game::config")
        );
        assert_eq!(
            client.csharp_unity.as_ref().unwrap().code_output.as_deref(),
            Some("gen/client/csharp_unity")
        );
    }

    #[test]
    fn default_project_toml_template_parses_as_workspace_config() {
        // 项目级模板不含 [project] / [separators] 段，但应能被同一个 WorkspaceConfig 反序列化
        // （deep-merge 路径用同一种类型）。
        let txt = default_project_toml_template();
        let _: WorkspaceConfig = toml::from_str(txt)
            .expect("project.toml template must parse as WorkspaceConfig");
        // 不应出现 [project] / [separators] 段（顶部 banner 注释里允许字面量出现，这里只看真段头）
        assert!(!txt.lines().any(|l| l.trim() == "[project]"), "项目级 toml 不应有 [project] 段");
        assert!(!txt.lines().any(|l| l.trim() == "[separators]"), "项目级 toml 不应有 [separators] 段");
        // 全量段都在
        assert!(txt.contains("[export.server.java]"));
        assert!(txt.contains("[export.server.go]"));
        assert!(txt.contains("[export.server.cpp]"));
        assert!(txt.contains("[export.server.csharp_dotnet]"));
        assert!(txt.contains("[export.server.typescript]"));
        assert!(txt.contains("[export.client.lua]"));
        assert!(txt.contains("[export.client.gdscript]"));
        assert!(txt.contains("[export.client.typescript]"));
        assert!(txt.contains("[export.client.csharp_unity]"));
        assert!(txt.contains("[export.client.csharp_godot]"));
        assert!(txt.contains("[ui]"));
        assert!(txt.contains("[ui.ref_picker]"));
    }
}
