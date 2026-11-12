//! Project 加载、扫描、配置合并。
//!
//! 文档对应：@02 Project / @03.4 / @07 S15-D。
//!
//! 仓库级 `tablet.toml` 是全局**默认值**（不随 Project 切换）。Project 自己的
//! 身份元数据存在 `<workdir>/projects/<id>/project.tblmanifest` 的 `# @meta` 段里
//! （id / name / created_at / source_template / source_template_version / category / version）。
//! 项目自己的完整配置（`[export]` / `[validate]`）落在同目录下的 `project.toml`，
//! 首次保存即生成；`[ui]` / `[separators]` / `[project]` 写在项目级会被加载流程剥掉
//! （`[ui]` 是工作区级；`[separators]` 以 manifest `# @sep` 为准；
//! `[project]` 是仓库状态）。文件缺席时按全局 `tablet.toml` 继承。
//!
//! ```toml
//! [export]                     # 全量写出；手写的部分字段也会用全局值补齐
//! [export.server.java]
//! package = "com.foo.bar"
//!
//! [validate]                   # 校验策略（error / warning / info）
//! error = "block"
//! ```
//!
//! 加载顺序：项目 toml > 全局 toml > 内置默认。
//!
//! **历史兼容**：
//! - 配置文件 `tbl-tool.toml` 在 load 时一次性 rename 为 `tablet.toml`
//! - 不再支持单项目模式（`<workdir>/config/` 直接存放数据），
//!   旧仓库需要手动迁移到 `projects/default/` 结构

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::*;
use crate::tbl::{self, TblFile};
use crate::schema_model::TblSchema;

pub const PROJECTS_DIR: &str = "projects";
pub const PROJECT_TOML_FILE: &str = "project.toml";
pub const PROJECT_MANIFEST_FILE: &str = "project.tblmanifest";

/// tablet.toml 由内存 `GlobalConfig` 用 serde 整文件序列化而成（无 `#` 注释）。
/// 这也是**唯一的**整写入口——结构、表头、顺序都由 serde 保证规范且幂等。
///
/// 序列化整个工作区配置（[project]/[export]/[ui]/[separators]）为 TOML 文本。
pub fn serialize_global_config(cfg: &GlobalConfig) -> Result<String> {
    Ok(toml::to_string(cfg)?)
}

/// 整文件写回 `<workdir>/tablet.toml`。
pub fn write_global_config(workdir: &Path, cfg: &GlobalConfig) -> Result<()> {
    let path = workdir.join(crate::CONFIG_FILE);
    std::fs::write(&path, serialize_global_config(cfg)?)?;
    Ok(())
}

/// 首跑/配置缺失时用默认配置落一份 tablet.toml。
pub fn write_default_global_config(workdir: &Path) -> Result<()> {
    write_global_config(workdir, &GlobalConfig::default())
}

/// 把项目的 `[export]` 配置序列化成 `project.toml` 的内容（无注释、整写）。
///
/// 用单字段包装让 serde 把 `export` 输出为顶层 `[export]` 表（子段带规范前缀），
/// 可被项目配置解析器（`parse_project_config`）读取。
///
/// `project.toml` 是项目**自带的完整配置**：首次保存就落盘，内容取自项目**当前**的
/// `config`（不是默认值——写默认值会把从全局拷来的配置静默重置掉，@02.26.3）。
/// `[export]` 和 `[validate]` 两段都写全，文件本身就是项目配置的真相；
/// 只有文件缺席时才从工作区 `tablet.toml` 继承（见 `parse_project_config`）。
pub fn project_toml_text(export: &ExportConfig, validate: &ValidateConfig) -> Result<String> {
    #[derive(serde::Serialize)]
    struct ProjectRoot<'a> {
        export: &'a ExportConfig,
        validate: &'a ValidateConfig,
    }
    Ok(toml::to_string(&ProjectRoot { export, validate })?)
}

/// 加载 Project（默认走 last_project 或扫描第一个）。
///
/// 流程：
/// 1. 解析 `<workdir>/tablet.toml`（不存在则写默认）—— 全局配置
/// 2. 从 `<workdir>/projects/` 扫描，从 `last_project` 或第一个挑选
/// 3. 解析 project.tblmanifest 和 project.toml，合并配置
/// 4. 扫 config/ 加载 groups
pub fn load_project(workdir: &Path) -> Result<Project> {
    let config_path = workdir.join(crate::CONFIG_FILE);

    if !config_path.exists() {
        write_default_global_config(workdir)?;
        println!("已生成默认配置文件: {}", config_path.display());
    }

    let global_text = std::fs::read_to_string(&config_path)?;

    // 必须有 projects/ 目录
    let projects_dir = workdir.join(PROJECTS_DIR);
    if !projects_dir.is_dir() {
        anyhow::bail!(
            "projects/ 目录不存在。请使用 'tablet-cli project new' 创建项目，\
             或参考文档将旧仓库结构迁移到多项目模式。"
        );
    }

    let candidates = scan_projects_dir(&projects_dir);
    let global_config: GlobalConfig = toml::from_str(&global_text)?;
    let chosen = pick_project(&candidates, &global_config.project_management.last_project)
        .with_context(|| format!("projects/ 目录下没有可用 Project: {}", projects_dir.display()))?;

    load_specific_project_impl(workdir, &projects_dir.join(&chosen.id), &global_config)
}

/// 加载指定 id 的 Project（用于 Project 切换）。
pub fn load_specific_project(workdir: &Path, project_id: &str) -> Result<Project> {
    let config_path = workdir.join(crate::CONFIG_FILE);
    let global_text = std::fs::read_to_string(&config_path)?;
    let global_config: GlobalConfig = toml::from_str(&global_text)?;

    let project_root = workdir.join(PROJECTS_DIR).join(project_id);
    if !project_root.is_dir() {
        anyhow::bail!("Project 不存在: {}", project_root.display());
    }

    load_specific_project_impl(workdir, &project_root, &global_config)
}

/// 加载指定路径的 Project（内部实现）。
fn load_specific_project_impl(
    workdir: &Path,
    project_root: &Path,
    global_config: &GlobalConfig,
) -> Result<Project> {
    let proj_text = std::fs::read_to_string(project_root.join(PROJECT_TOML_FILE)).ok();
    let manifest = read_project_manifest(
        project_root,
        project_root.file_name().unwrap().to_str().unwrap(),
    )?;

    // 解析项目级配置（仅 [export] 段）
    let config = parse_project_config(proj_text.as_deref(), global_config)?;

    // 分隔符始终来自 manifest，不从 toml 读取
    // （project.toml 中的 [separators] 段会被忽略）

    let data_dir = project_root.join("config");
    let mut issues = LoadIssues::default();
    let mut groups = if data_dir.is_dir() {
        load_groups_in(&data_dir, &manifest, &mut issues)?
    } else {
        std::fs::create_dir_all(&data_dir)?;
        println!("已创建配置目录: {}", data_dir.display());
        Vec::new()
    };
    // 被引用展示列（@10）：.tbl 不含该信息，从 manifest 段回填到节点。
    crate::schema_projection::apply_display_to_groups(&mut groups, &manifest);
    // 节点级校验规则（@01.10.7）：同上，规则只住在 manifest。
    crate::schema_projection::apply_rules_to_groups(&mut groups, &manifest);

    // 刚从盘上读进来，盘上的就是它自己
    let original_export = config.export.clone();
    let original_validate = config.validate.clone();
    let mut project = Project {
        workdir: workdir.to_path_buf(),
        project_root: project_root.to_path_buf(),
        config,
        original_meta: manifest.meta.clone(),
        original_separators: manifest.separators.clone(),
        original_export,
        original_validate,
        schema: manifest,
        groups,
        state: crate::model::ProjectState::Loaded,
        load_warnings: Vec::new(),
    };
    // 运行时 schema 是 groups 的投影：manifest 只提供索引/语义，字段结构来自
    // `.tbl` 表头与 workbook 表头。这里补全内存投影，供导出 / 校验 / UI 使用。
    project.resync_schema();

    // 数据文件被删（移除旧段）或 workbook 新加 sheet（补新段）时，落盘 manifest
    // 已经和内存投影不一致；启动时顺手修正，避免下次启动仍然对不上。
    if issues.schema_changed() {
        if let Err(e) = persist_manifest(&project) {
            let path = project_root.join(PROJECT_MANIFEST_FILE);
            issues
                .warnings
                .push(format!("修正 {} 失败: {e}", path.display()));
        }
    }
    project.load_warnings = issues.warnings;
    Ok(project)
}

/// 解析项目级配置（仅 [export] 段）。
///
/// 项目级 project.toml 不允许覆盖工作区级段：
/// - [project]    仓库状态（last_project / opened_projects），不属于单项目
/// - [ui]         UI 偏好仅工作区级；多 Project 同时打开时按项目切 UI 策略会反复变
/// - [separators] 分隔符以 project.tblmanifest 的 # @sep 为准
///
/// 项目级配置加载策略：
/// - project.toml 缺失 / 无 `[export]` 段 → 从全局配置拷贝完整初始值
/// - project.toml 存在但**字段缺失** → 用全局配置对应字段补齐（这是设计内的，不是错误）
/// - project.toml 里写了**不认识或不合法的键** → 直接报错
///
/// 最后一条是刻意的：老版本删除过的配置键（如 TypeScript 的 `module_kind`）留下来时
/// 必须吵，不能静默回退。这个函数以前是 `.unwrap_or(global_export)`——那不只是忽略
/// 一个坏键，而是**整个项目配置被换成工作区默认值**（连 output 目录都跟着变），
/// 排查起来毫无线索。
fn parse_project_config(project_text: Option<&str>, global: &GlobalConfig) -> Result<ProjectConfig> {
    let global_export = global.export.clone().unwrap_or_default();

    let Some(text) = project_text else {
        // 无 project.toml，从全局拷贝
        return Ok(ProjectConfig {
            export: global_export,
            validate: ValidateConfig::default(),
        });
    };

    // [export] / [validate] 两段；其它段忽略
    let proj_val: toml::Value = toml::from_str(text)
        .map_err(|e| anyhow::anyhow!("{} 不是合法 TOML: {}", PROJECT_TOML_FILE, e))?;

    // 校验策略：缺省 = 默认策略；写了就必须合法（拼错的键同样报错）
    let validate = match proj_val.get("validate") {
        Some(v) => v.clone().try_into::<ValidateConfig>().map_err(|e| anyhow::anyhow!(
            "{} 的 [validate] 段不合法: {}（只接受 error / warning / info = block | report | ignore）",
            PROJECT_TOML_FILE, e
        ))?,
        None => ValidateConfig::default(),
    };

    let proj_export_val = match proj_val.get("export") {
        Some(v) => v.clone(),
        None => return Ok(ProjectConfig {
            export: global_export,
            validate,
        }),
    };

    // 将全局 export 序列化为 toml::Value，作为补齐基准
    let global_export_val = toml::Value::try_from(&global_export)
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));

    // 深度合并：用全局配置补充项目配置中缺失的字段
    let mut merged = global_export_val;
    deep_merge_toml(&mut merged, proj_export_val);

    // 缺失字段上面已经补齐了；合并后仍然解析不出来，就只能是项目里写了
    // 不认识的键或非法值——报错，不静默回退
    let export = merged.try_into::<crate::model::ExportConfig>()
        .map_err(|e| anyhow::anyhow!(
            "{} 的 [export] 段不合法: {}（字段缺失会用工作区配置补齐，\
             但拼错的键和已废弃的键不会被忽略）",
            PROJECT_TOML_FILE, e
        ))?;

    Ok(ProjectConfig { export, validate })
}

/// 深度合并 toml::Value（overlay 覆盖 base）。
/// 用于配置修复：base 是完整的全局配置，overlay 是可能不完整的项目配置。
/// 对于 Table：递归合并字段；对于非 Table：overlay 直接覆盖 base。
fn deep_merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    use toml::Value;

    match (base, overlay) {
        (Value::Table(base_table), Value::Table(overlay_table)) => {
            for (key, val) in overlay_table {
                if let Some(base_val) = base_table.get_mut(&key) {
                    deep_merge_toml(base_val, val);
                } else {
                    base_table.insert(key, val);
                }
            }
        }
        (base_val, overlay_val) => {
            *base_val = overlay_val;
        }
    }
}

/// 加载 workdir 下**全部** Project 到内存（多 Project 同时管理模型，@04.2.0）。
///
/// - 扫描 `<workdir>/projects/` 列出全部 id；每个走 `load_specific_project` 全量加载
/// - 返回的 Vec 顺序：按 project id 字典序（与 `list_projects` 一致）
pub fn load_all_projects(workdir: &Path) -> Result<Vec<Project>> {
    let projects_dir = workdir.join(PROJECTS_DIR);
    if !projects_dir.is_dir() {
        anyhow::bail!(
            "projects/ 目录不存在。请使用 'tablet-cli project new' 创建项目，\
             或参考文档将旧仓库结构迁移到多项目模式。"
        );
    }

    let candidates = scan_projects_dir(&projects_dir);
    if candidates.is_empty() {
        anyhow::bail!("projects/ 下没有可加载的 Project");
    }

    let config_path = workdir.join(crate::CONFIG_FILE);
    if !config_path.exists() {
        write_default_global_config(workdir)?;
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
/// 1. 扫描 `<workdir>/projects/` 得到 `Vec<AvailableProject>`，按 id 字典序
/// 2. 决定 `to_open`：
///    - `opened_projects` 非空：用它过滤掉不存在的 id（保持顺序）
///    - 否则若 `last_project` 非空且存在：仅打开它
///    - 否则打开 available 第一个（保持兼容，确保启动有内容）
/// 3. active = `last_project`（不在 opened 里则 fallback opened[0]）
pub fn load_workspace(workdir: &Path) -> Result<(crate::ops::ProjectEngine, bool, Vec<ProjectOpenFailure>)> {
    use crate::ops::{AvailableProject, ProjectEngine};

    let projects_dir = workdir.join(PROJECTS_DIR);
    // 确保 tablet.toml 存在（不存在则写默认）；
    // 老仓库可能仍是 `tbl-tool.toml`，一次性 rename 迁移。
    let config_path = workdir.join(crate::CONFIG_FILE);
    if !config_path.exists() {
        let legacy = workdir.join(crate::LEGACY_CONFIG_FILE);
        if legacy.exists() {
            std::fs::rename(&legacy, &config_path)?;
            println!("已迁移配置文件: {} → {}", legacy.display(), config_path.display());
        } else {
            write_default_global_config(workdir)?;
            println!("已生成默认配置文件: {}", config_path.display());
        }
    }
    let (global_config, config_fixed) = crate::config_fix::load_and_fix_global_config(&config_path)?;

    if !projects_dir.is_dir() {
        // projects/ 不存在 = 空仓库
        return Ok((ProjectEngine::new_workspace(
            workdir.to_path_buf(),
            global_config,
            Vec::new(),
            Vec::new(),
            None,
        ), config_fixed, Vec::new()));
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
        return Ok((ProjectEngine::new_workspace(
            workdir.to_path_buf(),
            global_config,
            Vec::new(),
            Vec::new(),
            None,
        ), config_fixed, Vec::new()));
    }

    // 计算 to_open
    let last_project = global_config.project_management.last_project.clone();
    let opened_pref = &global_config.project_management.opened_projects;
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

    // 打不开的项目**不在这里吞掉**：收集起来交给调用方决定怎么出声
    // （CLI = 打消息 + 非零退出；GUI = 日志 + 弹框 + 不打开它）。
    let mut opened: Vec<Project> = Vec::with_capacity(to_open.len());
    let mut failed: Vec<ProjectOpenFailure> = Vec::new();
    for id in &to_open {
        match load_specific_project(workdir, id) {
            Ok(p) => opened.push(p),
            Err(e) => failed.push(ProjectOpenFailure { id: id.clone(), message: e.to_string() }),
        }
    }

    let active_id = if !last_project.is_empty() && opened.iter().any(|p| p.schema.meta.id == last_project) {
        Some(last_project)
    } else {
        opened.first().map(|p| p.schema.meta.id.clone())
    };
    let mut engine = ProjectEngine::new_workspace(
        workdir.to_path_buf(),
        global_config.clone(),
        available,
        opened,
        active_id.as_deref(),
    );
    // workspace tablet.toml [separators] 作为「新建空项目」时 schema.separators 的初值；
    // toml 里没写的字段走 SeparatorsSection 自身 default。
    engine.set_default_separators(global_config.separators.clone());
    Ok((engine, config_fixed, failed))
}

/// 一个打不开的 Project：id 与原因（带文件路径与行号）。
///
/// 由 [`load_workspace`] 带回，**不在这里决定怎么出声** —— CLI 要的是消息 + 非零退出，
/// GUI 要的是日志 + 弹框，两边呈现方式不同。
#[derive(Debug, Clone)]
pub struct ProjectOpenFailure {
    pub id: String,
    pub message: String,
}

/// 项目布局变化后的落盘由 `ProjectEngine::persist_global_config` 承担（见 ops.rs），
/// 统一走 `write_global_config` 整写，不再有分片文本写入器。
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

/// 扫 `<workdir>/projects/` 找出合法的 Project 目录。
///
/// **合法性判定**：目录里必须有 `project.tblmanifest` 文件——否则跳过。这能过滤掉
/// "Excel 编辑创建的孤儿 .tbl-cache 目录"（用户新建项目未保存就关程序，cache 目录留下
/// 但 schema 没落盘 → 下次启动会被误识别为空 Project）。
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
        // 仅当目录含 project.tblmanifest 时才视为合法 Project。
        // 跳过仅有 .tbl-cache（Excel 编辑残留）/ 空 / 其它垃圾目录。
        if !path.join(PROJECT_MANIFEST_FILE).is_file() {
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

/// 读 `<project_root>/project.tblmanifest` 解析出运行时清单。
///
/// - 文件缺失 → `Err`
/// - 文件在但**解析失败** → `Err`，原因里带文件路径与行号
///
fn read_project_manifest(
    project_root: &Path,
    project_id_fallback: &str,
) -> Result<crate::schema_model::TblSchema> {
    let path = project_root.join(PROJECT_MANIFEST_FILE);
    let txt = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => anyhow::bail!("缺少运行时清单: {}", path.display()),
    };
    let mut s = crate::manifest::parse_tblmanifest(&txt)
        .map_err(|e| anyhow::anyhow!("{} 解析失败: {}", path.display(), e))?;
    if s.meta.id.is_empty() {
        s.meta.id = project_id_fallback.to_string();
    }
    if s.meta.name.is_empty() {
        s.meta.name = s.meta.id.clone();
    }
    Ok(s)
}

/// 扫描 `projects/` 时用的宽松版：**一律兜底**。
///
/// 列表要能列出坏项目（按目录名），一个坏项目不该让整张列表打不开；
/// 真要动它的时候走 [`read_project_schema`]，那时会硬失败并给出位置。
fn read_project_manifest_lenient(
    project_root: &Path,
    project_id_fallback: &str,
) -> crate::schema_model::TblSchema {
    read_project_manifest(project_root, project_id_fallback)
        .unwrap_or_else(|_| fallback_schema(project_id_fallback))
}

fn fallback_schema(id: &str) -> crate::schema_model::TblSchema {
    let mut s = crate::schema_model::TblSchema::default();
    s.meta.id = id.to_string();
    s.meta.name = id.to_string();
    s
}

/// 仅读取 schema.meta 的 (id, name)，用于 scan_projects_dir 列表展示与 created_at 取值。
/// 失败时返回 None；调用方按目录名兜底。
fn read_project_meta_short(project_root: &Path) -> Option<(String, String)> {
    let s = read_project_manifest_lenient(project_root, "");
    if s.meta.id.is_empty() {
        return None;
    }
    Some((s.meta.id, s.meta.name))
}

/// 读取 schema.meta.created_at（可空）。供 load_workspace 填 AvailableProject。
fn read_project_created_at(project_root: &Path) -> String {
    read_project_manifest_lenient(project_root, "").meta.created_at
}


/// 启动加载时发现的「可以继续启动」的问题。
///
/// 目前只覆盖 xlsx 组：策划在 Excel 里删掉整本工作簿或其中若干 sheet 后，
/// manifest 仍留着旧节点。这类情况不能拖垮整个 Project 的加载：
/// - `warnings` 交给 GUI / CLI 打日志；
/// - `dropped` 是已经不存在的 (group, node)，加载后从 `project.tblmanifest` 移除。
#[derive(Default)]
pub(crate) struct LoadIssues {
    pub warnings: Vec<String>,
    pub dropped: Vec<(String, String)>,
    /// 自动登记的新 sheet / 节点数（workbook 里新加、manifest 没有）。
    pub added: usize,
}

impl LoadIssues {
    /// manifest 是否需要按加载结果重写（删掉了失效段，或补上了新节点）。
    pub(crate) fn schema_changed(&self) -> bool {
        !self.dropped.is_empty() || self.added > 0
    }
}

fn load_groups_in(
    data_dir: &Path,
    schema: &TblSchema,
    issues: &mut LoadIssues,
) -> Result<Vec<Group>> {
    // config/ 下现在混放两种组：
    //   config/<group>/        Tbl 组（目录）
    //   config/<group>.xlsx    Xlsx 组（单个工作簿；sheet = 节点）
    // 旧布局 config/<group>/<group>.xlsx 不再兼容，读到直接在下面报错。
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut books: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(data_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if path
            .extension()
            .map_or(false, |ext| ext.eq_ignore_ascii_case("xlsx"))
        {
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if !stem.starts_with("~$") {
                books.push(path);
            }
        }
    }
    dirs.sort();
    books.sort();

    // 同名目录 + 工作簿 = 一组两种后端，属于脏状态，不猜。
    let names = |paths: &[PathBuf]| -> Vec<String> {
        paths
            .iter()
            .map(|p| {
                if p.is_dir() {
                    p.file_name().unwrap_or_default().to_string_lossy().to_string()
                } else {
                    p.file_stem().unwrap_or_default().to_string_lossy().to_string()
                }
            })
            .collect()
    };
    let dir_names = names(&dirs);
    let book_names = names(&books);
    if let Some(dup) = dir_names.iter().find(|n| book_names.iter().any(|b| b == *n)) {
        anyhow::bail!(
            "组 {} 同时存在目录和 {}.xlsx：一组一种后端，请先清理其中一边",
            dup, dup
        );
    }

    // Tbl 组的 .tbl 文件（或整个组目录）被删：manifest 里的段还留着，但盘上没有数据源。
    // 跳过缺失节点并记警告；加载完成后由 `load_specific_project_impl` 修 manifest。
    // 注意只处理「文件不存在」；文件存在但内容读不懂（parse_tbl 报错）仍然让加载失败。
    let mut tbl_groups: Vec<&str> = schema
        .sections
        .iter()
        .filter(|s| s.storage == crate::model::GroupStorage::Tbl)
        .map(|s| s.group.as_str())
        .collect();
    tbl_groups.sort();
    tbl_groups.dedup();
    for name in tbl_groups {
        let secs: Vec<&crate::schema_model::SchemaSection> = schema
            .sections
            .iter()
            .filter(|s| s.group == name && s.storage == crate::model::GroupStorage::Tbl)
            .collect();
        match dir_names.iter().position(|n| n == name) {
            None => {
                issues.warnings.push(format!(
                    "组 {name} 的目录 config/{name}/ 不存在，已跳过该组（{} 个节点：{}），并已从 project.tblmanifest 移除",
                    secs.len(),
                    secs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("、")
                ));
                for sec in secs {
                    issues.dropped.push((name.to_string(), sec.name.clone()));
                }
            }
            Some(idx) => {
                let dir = &dirs[idx];
                for sec in secs {
                    if !dir.join(format!("{}.tbl", sec.name)).is_file() {
                        issues.warnings.push(format!(
                            "组 {name} 缺少文件 config/{name}/{}.tbl，已跳过该节点，并已从 project.tblmanifest 移除",
                            sec.name
                        ));
                        issues.dropped.push((name.to_string(), sec.name.clone()));
                    }
                }
            }
        }
    }

    // 整本工作簿被删掉的 xlsx 组：manifest 里的段还留着，但盘上没有任何数据源。
    let mut xlsx_groups: Vec<&str> = schema
        .sections
        .iter()
        .filter(|s| s.storage == crate::model::GroupStorage::Xlsx)
        .map(|s| s.group.as_str())
        .collect();
    xlsx_groups.sort();
    xlsx_groups.dedup();
    for name in xlsx_groups {
        if book_names.iter().any(|n| n == name) {
            continue;
        }
        let nodes: Vec<String> = schema
            .sections
            .iter()
            .filter(|s| s.group == name && s.storage == crate::model::GroupStorage::Xlsx)
            .map(|s| s.name.clone())
            .collect();
        issues.warnings.push(format!(
            "Excel 组 {name} 的工作簿 config/{name}.xlsx 不存在，已跳过该组（{} 个节点：{}），并已从 project.tblmanifest 移除",
            nodes.len(),
            nodes.join("、")
        ));
        for node in nodes {
            issues.dropped.push((name.to_string(), node));
        }
    }

    let mut groups = Vec::new();
    for (name, dir) in dir_names.into_iter().zip(dirs) {
        groups.push(load_tbl_group(&name, &dir)?);
    }
    for (name, book) in book_names.into_iter().zip(books) {
        groups.push(load_xlsx_group(&name, data_dir, schema, &book, Some(issues))?);
    }
    Ok(groups)
}

/// Tbl 组目录 → `Group`。xlsx 组已经由根级 `<group>.xlsx` 直接加载。
fn load_tbl_group(name: &str, dir: &Path) -> Result<Group> {
    let old_book = dir.join(format!("{}.xlsx", name));
    if old_book.is_file() {
        anyhow::bail!(
            "组 {} 使用了旧布局 {}；xlsx 组现在必须是 config/{}.xlsx（不兼容旧目录布局）",
            name, old_book.display(), name
        );
    }

    let mut tbl_files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "tbl"))
        .collect();

    let mut tables = Vec::new();
    let mut constants = Vec::new();
    let mut enums = Vec::new();

    tbl_files.sort();

    for path in tbl_files {
        // **读不懂就报错，不许静默跳过。**
        match tbl::parse_tbl(&path) {
            Ok(TblFile::Table(t)) => tables.push(t),
            Ok(TblFile::Constant(c)) => constants.push(c),
            Ok(TblFile::Enum(e)) => enums.push(e),
            // 解析器已经自带文件路径，这里原样往上抛，别把路径再包一遍
            Err(e) => return Err(e),
        }
    }

    Ok(Group {
        name: name.to_string(),
        original_name: name.to_string(),
        dir: dir.to_path_buf(),
        tables,
        constants,
        enums,
        is_new: false,
        storage: GroupStorage::Tbl,
        book_stamp: None,
    })
}

/// 加载 Xlsx 组：**结构与数据都来自 workbook**；manifest 的 section 只决定节点清单
/// （@02.27）。
///
/// 规则：
/// - schema 有 section、workbook 没有同名 sheet：`issues = Some` 时跳过并记警告
///   （启动加载：策划删了 sheet，不能拖垮整个 Project）；`None` 时报错（手动重载，需人处理）。
/// - section 的类型决定 sheet 怎么读（table 4 行表头 / constant、enum 1 行表头）；
/// - workbook 里有、manifest 里没有的 sheet → 忽略（策划的私有 sheet，P3 才变成"托管节点"）；
/// - Table 的字段 / 类型 / export / desc 全部从 4 行表头读，manifest 不提供。
pub(crate) fn load_xlsx_group(
    name: &str,
    dir: &Path,
    schema: &TblSchema,
    book_path: &Path,
    mut issues: Option<&mut LoadIssues>,
) -> Result<Group> {
    use crate::model::GroupStorage;

    let mut book = umya_spreadsheet::reader::xlsx::lazy_read(book_path)
        .with_context(|| format!("打开 {}", book_path.display()))?;

    let sections: Vec<&crate::schema_model::SchemaSection> =
        schema.sections.iter().filter(|s| s.group == name).collect();

    let mut tables = Vec::new();
    let mut constants = Vec::new();
    let mut enums = Vec::new();

    for sec in &sections {
        // 存在性必须问 sheet_collection_no_check()：`sheet_by_name()` 对没反序列化的
        // sheet 一律返回 NotDeserialized，在 lazy book 上会骗我们「这张表不存在」。
        // 名字大小写：Excel 里 sheet 名唯一是大小写不敏感的，这里用同样口径找。
        let actual = book.sheet_collection_no_check().iter()
            .find(|s| s.name().eq_ignore_ascii_case(&sec.name))
            .map(|s| s.name().to_string());
        let Some(sheet_name) = actual else {
            if let Some(iss) = issues.as_deref_mut() {
                // 策划在 Excel 里删了 sheet：启动继续，警告进日志，manifest 稍后修正。
                let file = book_path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                iss.warnings.push(format!(
                    "Excel 组 {name} 缺少 sheet {}（config/{file}），已跳过该节点，并已从 project.tblmanifest 移除",
                    sec.name
                ));
                iss.dropped.push((name.to_string(), sec.name.clone()));
                continue;
            }
            anyhow::bail!(
                "组 {} 的 schema 里有 {}，但 {} 里没有同名 sheet",
                name, sec.name, book_path.display()
            );
        };
        let ws = book.sheet_by_name_mut(&sheet_name)
            .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet_name, e))?;

        match sec.mode {
            crate::schema_model::SchemaMode::Table => {
                tables.push(table_from_sheet(sec, ws, book_path)?);
            }
            crate::schema_model::SchemaMode::Constant => {
                constants.push(constant_from_sheet(sec, ws, book_path)?);
            }
            crate::schema_model::SchemaMode::Enum => {
                enums.push(enum_from_sheet(sec, ws, book_path)?);
            }
        }
    }

    // 反向同步：manifest 没有、workbook 里新加的 sheet 自动登记为节点。
    // 只认两种「明确结构」：
    // - Constant / Enum：首行固定表头；
    // - Table：4 行表头里存在合法的 id 列（id 名 + 类型可解析 + export 可解析）。
    // 其余 sheet 视为策划私有，忽略（不建节点、不入 manifest）。
    let known: Vec<String> = sections.iter().map(|s| s.name.to_lowercase()).collect();
    let extra_sheets: Vec<String> = book
        .sheet_collection_no_check()
        .iter()
        .map(|s| s.name().to_string())
        .filter(|n| !n.eq_ignore_ascii_case("_empty"))
        .filter(|n| !known.iter().any(|k| *k == n.to_lowercase()))
        .filter(|n| crate::validate::is_valid_node_name(n))
        .collect();
    for sheet_name in extra_sheets {
        let ws = book
            .sheet_by_name_mut(&sheet_name)
            .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet_name, e))?;
        let Some(mode) = detect_new_sheet_mode(ws) else {
            continue;
        };
        let sec = crate::schema_model::SchemaSection {
            group: name.to_string(),
            name: sheet_name.clone(),
            mode: mode.clone(),
            storage: crate::model::GroupStorage::Xlsx,
            fields: Vec::new(),
            preset: Vec::new(),
            display: None,
            rules: Vec::new(),
        };
        match mode {
            crate::schema_model::SchemaMode::Table => {
                tables.push(table_from_sheet(&sec, ws, book_path)?);
            }
            crate::schema_model::SchemaMode::Constant => {
                constants.push(constant_from_sheet(&sec, ws, book_path)?);
            }
            crate::schema_model::SchemaMode::Enum => {
                enums.push(enum_from_sheet(&sec, ws, book_path)?);
            }
        }
        if let Some(iss) = issues.as_deref_mut() {
            iss.warnings.push(format!(
                "Excel 组 {name} 新增 sheet {sheet_name}，已自动加入 project.tblmanifest"
            ));
            iss.added += 1;
        }
    }

    let stamp = file_stamp(book_path);
    Ok(Group {
        name: name.to_string(),
        original_name: name.to_string(),
        dir: dir.to_path_buf(),
        tables,
        constants,
        enums,
        is_new: false,
        storage: GroupStorage::Xlsx,
        book_stamp: stamp,
    })
}

/// 重新从磁盘读一个 xlsx 组（外部编辑后的刷新）。
///
/// 与启动加载同一套宽容规则：缺 sheet / 新加 sheet 都记进 `issues`，
/// 由调用方打日志并用 [`persist_manifest`] 修正清单。
pub(crate) fn reload_xlsx_group(
    group: &Group,
    schema: &TblSchema,
    issues: Option<&mut LoadIssues>,
) -> Result<Group> {
    load_xlsx_group(&group.name, &group.dir, schema, &group.book_path(), issues)
}

/// 把内存 schema 投影写回 `project.tblmanifest`（与盘上文本相同则跳过）。
///
/// 启动加载与 xlsx 组重载用它做「自动修正清单」：节点索引来自 groups 投影，
/// 不涉及数据文件本身，所以可以安全地直接写。
pub(crate) fn persist_manifest(project: &Project) -> std::io::Result<bool> {
    let txt = crate::manifest::serialize_tblmanifest(&project.schema);
    let path = project.project_root.join(PROJECT_MANIFEST_FILE);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(txt.as_str()) {
        return Ok(false);
    }
    std::fs::write(&path, txt)?;
    Ok(true)
}

/// `(mtime, size)`：保存前拿来发现「文件被外部改过」。
pub(crate) fn file_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Table sheet → `Table`。结构完全来自 workbook 的 4 行表头：
/// desc / export / type / 字段名。
///
/// 字段名非空的列 = 模型列；字段名留空的列视为策划私有列，不进入模型、保存时原样保留。
fn table_from_sheet(
    sec: &crate::schema_model::SchemaSection,
    ws: &umya_spreadsheet::Worksheet,
    book_path: &Path,
) -> Result<Table> {
    use crate::excel::{DATA_START, HDR_DESC, HDR_EXPORT, HDR_NAME, HDR_TYPE};
    use crate::model::{FieldDef, Table, TableSchema};

    let (max_col, max_row) = ws.highest_column_and_row();
    if max_row < HDR_NAME {
        anyhow::bail!(
            "{} 的 sheet {} 缺少 4 行表头（desc/export/type/字段名）",
            book_path.display(), sec.name
        );
    }
    let last_col = (1..=max_col).rev()
        .find(|&c| !ws.value((c, HDR_NAME)).trim().is_empty())
        .unwrap_or(0);
    if last_col == 0 {
        anyhow::bail!("{} 的 sheet {} 没有字段列", book_path.display(), sec.name);
    }

    let mut fields: Vec<FieldDef> = Vec::new();
    let mut col_of: Vec<u32> = Vec::new();
    let mut aux_seq = 0usize;
    let mut seen_names = std::collections::HashSet::new();
    for c in 1..=last_col {
        let label = ws.value((c, HDR_NAME)).trim().to_string();
        if label.is_empty() { continue; }
        let desc = ws.value((c, HDR_DESC));
        let ty = ws.value((c, HDR_TYPE)).trim().to_string();
        let export_raw = ws.value((c, HDR_EXPORT));
        let export = parse_xlsx_export(&export_raw);
        let name_ok = crate::validate::validate_field_name(&label).is_none();
        let type_ok = crate::types::TblType::parse(&ty).is_some();
        let export_ok = export.is_some();
        let dup = !seen_names.insert(label.to_lowercase());

        if name_ok && type_ok && export_ok && !dup {
            fields.push(FieldDef {
                name: label,
                desc,
                tbl_type: ty,
                export: export.unwrap(),
                aux: None,
                source_col: Some(c),
            });
        } else {
            // 非 tablet 规范列：留在模型里展示 / 编辑 / 原样保存，但不参与校验 / 引用 / 导出。
            aux_seq += 1;
            let mut key = format!("aux_{}", aux_seq);
            while fields.iter().any(|f| f.name == key) {
                aux_seq += 1;
                key = format!("aux_{}", aux_seq);
            }
            fields.push(FieldDef {
                name: key,
                desc,
                tbl_type: ty,
                export: export.unwrap_or(crate::model::Export::ClientServer),
                aux: Some(crate::model::AuxMeta {
                    label,
                    raw_export: export_raw,
                }),
                source_col: Some(c),
            });
        }
        col_of.push(c);
    }
    if fields.is_empty() {
        anyhow::bail!("{} 的 sheet {} 没有字段列", book_path.display(), sec.name);
    }
    // 有合法 id 列就把它提到最前面（Excel 列顺序不动，保存按字段名回写）；
    // 没有合法 id 时不在这里判死，交给校验 / 网格提示，项目仍能打开。
    if let Some(pos) = fields.iter().position(|f| !f.is_aux() && f.name == "id") {
        if pos > 0 {
            let f = fields.remove(pos);
            let c = col_of.remove(pos);
            fields.insert(0, f);
            col_of.insert(0, c);
        }
    }

    let mut records: Vec<Vec<String>> = Vec::new();
    for r in DATA_START..=max_row {
        let cells: Vec<String> = col_of.iter().map(|&c| ws.value((c, r))).collect();
        if cells.iter().all(|v| v.is_empty()) { continue; }
        records.push(cells);
    }

    Ok(Table {
        name: sec.name.clone(),
        original_name: sec.name.clone(),
        path: book_path.to_path_buf(),
        schema: TableSchema { fields: fields.clone() },
        original_records: records.clone(),
        original_fields: fields,
        records,
        dirty: false,
        deleted: false,
        saved: true,
        fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        display_field: sec.display.clone(),
        original_display_field: sec.display.clone(),
        rules: sec.rules.clone(),
        original_rules: sec.rules.clone(),
    })
}

/// Excel 表头的 export 单元格：短码与中文展示值都认，其它值报错。
fn parse_xlsx_export(raw: &str) -> Option<crate::model::Export> {
    if let Some(e) = crate::model::Export::parse_code(raw) {
        return Some(e);
    }
    match raw.trim() {
        "前后端" => Some(crate::model::Export::ClientServer),
        "客户端" => Some(crate::model::Export::ClientOnly),
        "服务器" => Some(crate::model::Export::ServerOnly),
        "不导出" => Some(crate::model::Export::None),
        _ => None,
    }
}

/// Constant sheet → `Constant`。1 行表头 `name|type|value|export|desc`，按位置读。
fn constant_from_sheet(
    sec: &crate::schema_model::SchemaSection,
    ws: &umya_spreadsheet::Worksheet,
    book_path: &Path,
) -> Result<Constant> {
    let (_, max_row) = ws.highest_column_and_row();
    let mut entries = Vec::new();
    for r in 2..=max_row {
        let name = ws.value((1, r));
        if name.trim().is_empty() { continue; }
        entries.push(ConstEntry {
            name: name.trim().to_string(),
            tbl_type: ws.value((2, r)).trim().to_string(),
            value: ws.value((3, r)),
            export: Export::from_input(&ws.value((4, r))),
            desc: ws.value((5, r)),
        });
    }
    Ok(Constant {
        name: sec.name.clone(),
        original_name: sec.name.clone(),
        path: book_path.to_path_buf(),
        original_entries: entries.clone(),
        entries,
        dirty: false,
        deleted: false,
        saved: true,
    })
}

/// Enum sheet → `EnumDef`。1 行表头 `id|name|desc`，按位置读。
fn enum_from_sheet(
    sec: &crate::schema_model::SchemaSection,
    ws: &umya_spreadsheet::Worksheet,
    book_path: &Path,
) -> Result<EnumDef> {
    let (_, max_row) = ws.highest_column_and_row();
    let mut entries = Vec::new();
    for r in 2..=max_row {
        let id = ws.value((1, r));
        let name = ws.value((2, r));
        if id.trim().is_empty() && name.trim().is_empty() { continue; }
        entries.push(EnumEntry {
            id: id.trim().to_string(),
            name: name.trim().to_string(),
            desc: ws.value((3, r)),
        });
    }
    Ok(EnumDef {
        name: sec.name.clone(),
        original_name: sec.name.clone(),
        path: book_path.to_path_buf(),
        entries: entries.clone(),
        original_entries: entries,
        dirty: false,
        deleted: false,
        saved: true,
        display_field: sec.display.clone(),
        original_display_field: sec.display.clone(),
        rules: sec.rules.clone(),
        original_rules: sec.rules.clone(),
    })
}

/// 原生 xlsx 导入用：不依赖 manifest section，直接把 workbook 的所有 sheet 读成节点。
///
/// - `_empty` 是空组占位 sheet，跳过；
/// - Table 必须是 4 行表头；Constant / Enum 按首行固定表头识别；
/// - 不做改名，sheet 名就是节点名；重名 / 非法名直接报错。
pub(crate) fn load_xlsx_group_all_sheets(
    name: &str,
    dir: &Path,
    book_path: &Path,
) -> Result<Group> {
    use crate::schema_model::{SchemaMode, SchemaSection};
    use crate::model::GroupStorage;

    let mut book = umya_spreadsheet::reader::xlsx::read(book_path)
        .with_context(|| format!("打开 {}", book_path.display()))?;
    let sheet_names: Vec<String> = book.sheet_collection_no_check().iter()
        .map(|s| s.name().to_string())
        .collect();

    let mut tables = Vec::new();
    let mut constants = Vec::new();
    let mut enums = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for sheet_name in sheet_names {
        if sheet_name.eq_ignore_ascii_case("_empty") { continue; }
        if !crate::validate::is_valid_node_name(&sheet_name) {
            anyhow::bail!(
                "{} 的 sheet {:?} 不是合法配置项名（大写字母开头，只含英文数字下划线）",
                book_path.display(), sheet_name
            );
        }
        if !seen.insert(sheet_name.to_lowercase()) {
            anyhow::bail!("{} 的 sheet 名大小写重复: {}", book_path.display(), sheet_name);
        }

        let ws = book.sheet_by_name_mut(&sheet_name)
            .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet_name, e))?;
        let mode = detect_sheet_mode(ws);
        let sec = SchemaSection {
            group: name.to_string(),
            name: sheet_name.clone(),
            mode: mode.clone(),
            storage: GroupStorage::Xlsx,
            fields: Vec::new(),
            preset: Vec::new(),
            display: None,
            rules: Vec::new(),
        };
        match mode {
            SchemaMode::Table => tables.push(table_from_sheet(&sec, ws, book_path)?),
            SchemaMode::Constant => constants.push(constant_from_sheet(&sec, ws, book_path)?),
            SchemaMode::Enum => enums.push(enum_from_sheet(&sec, ws, book_path)?),
        }
    }

    Ok(Group {
        name: name.to_string(),
        original_name: name.to_string(),
        dir: dir.to_path_buf(),
        tables,
        constants,
        enums,
        is_new: false,
        storage: GroupStorage::Xlsx,
        book_stamp: file_stamp(book_path),
    })
}

/// 认一张 sheet 是 table / constant / enum。
/// Constant / Enum 的 1 行表头是固定值；其它一律按 Table 的 4 行表头读（读不出来会报错）。
/// manifest 里没有的新 sheet → 节点类型；认不出来返回 None（视为策划私有 sheet）。
///
/// - Constant / Enum：首行固定表头精确匹配；
/// - Table：4 行表头里存在**合法的 id 列**（字段名 id、类型可解析、export 可解析）。
///   其余列不合规也不要紧，`table_from_sheet` 会按普通列（aux）收进来，不丢数据。
fn detect_new_sheet_mode(ws: &umya_spreadsheet::Worksheet) -> Option<crate::schema_model::SchemaMode> {
    use crate::excel::{HDR_EXPORT, HDR_NAME, HDR_TYPE};
    use crate::schema_model::SchemaMode;
    let cell = |c: u32, r: u32| ws.value((c, r)).trim().to_string();
    if cell(1, 1) == "name" && cell(2, 1) == "type" && cell(3, 1) == "value"
        && cell(4, 1) == "export" && cell(5, 1) == "desc"
    {
        return Some(SchemaMode::Constant);
    }
    if cell(1, 1) == "id" && cell(2, 1) == "name" && cell(3, 1) == "desc" {
        return Some(SchemaMode::Enum);
    }
    let (max_col, _) = ws.highest_column_and_row();
    let has_valid_id = (1..=max_col).any(|c| {
        cell(c, HDR_NAME) == "id"
            && crate::types::TblType::parse(&cell(c, HDR_TYPE)).is_some()
            && parse_xlsx_export(&cell(c, HDR_EXPORT)).is_some()
    });
    if has_valid_id { Some(SchemaMode::Table) } else { None }
}

fn detect_sheet_mode(ws: &umya_spreadsheet::Worksheet) -> crate::schema_model::SchemaMode {
    use crate::schema_model::SchemaMode;
    let cell = |c: u32| ws.value((c, 1)).trim().to_string();
    if cell(1) == "name" && cell(2) == "type" && cell(3) == "value"
        && cell(4) == "export" && cell(5) == "desc"
    {
        SchemaMode::Constant
    } else if cell(1) == "id" && cell(2) == "name" && cell(3) == "desc" {
        SchemaMode::Enum
    } else {
        SchemaMode::Table
    }
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

    fn write_schema(root: &Path, id: &str, name: &str) {
        std::fs::write(
            root.join(PROJECT_MANIFEST_FILE),
            format!("#!tblmanifest v1\n# @meta id: {}\n# @meta name: {}\n", id, name),
        ).unwrap();
    }

    fn write_book_sheet(
        wb: &mut rust_xlsxwriter::Workbook,
        name: &str,
        rows: &[&[&str]],
    ) {
        let ws = wb.add_worksheet();
        ws.set_name(name).unwrap();
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32, c as u16, *v).unwrap();
            }
        }
    }

    #[test]
    fn projects_dir_picks_last_project() {
        let dir = unique_tmp("multi");
        std::fs::create_dir_all(dir.join("projects/p1/config")).unwrap();
        std::fs::create_dir_all(dir.join("projects/p2/config")).unwrap();
        write_schema(&dir.join("projects/p1"), "p1", "项目1");
        write_schema(&dir.join("projects/p2"), "p2", "项目2");
        std::fs::write(
            dir.join("tablet.toml"),
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"ghost\"\n",
        )
        .unwrap();
        let proj = load_project(&dir).expect("load");
        assert_eq!(proj.schema.meta.id, "aaa");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一个写坏的 `.tbl` 必须让整个项目**打不开**，不能悄悄少一张表。
    ///
    /// 以前这里是 `eprintln!("warn: failed to parse ...")` 然后跳过：导出少一张表、
    /// 退出码还是 0。`.tblschema` 那边刚补过同一个洞（@02.26.7），数据文件这条
    /// 对 AI / 脚本同样致命 —— 它们只看退出码。
    #[test]
    fn a_broken_tbl_fails_the_load_instead_of_vanishing() {
        let dir = unique_tmp("broken_tbl");
        let root = dir.join("projects/p1");
        std::fs::create_dir_all(root.join("config/hero")).unwrap();
        write_schema(&root, "p1", "项目1");
        std::fs::write(root.join("config/hero/Bad.tbl"), "#!tbl v99\n#mode table\n---\n").unwrap();
        std::fs::write(dir.join("tablet.toml"), "").unwrap();

        let err = load_project(&dir).expect_err("写坏的 .tbl 必须报错");
        let msg = format!("{:#}", err);
        assert!(msg.contains("Bad.tbl"), "错误里要有文件路径: {}", msg);
        assert!(msg.contains("v99") || msg.contains("版本"), "错误里要说清原因: {}", msg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_global_config_serde_roundtrip() {
        // 默认配置 serde 整写后应能原样解析回默认值（取代旧“注释模板 vs struct 一致”校验）。
        let def = GlobalConfig::default();
        let txt = serialize_global_config(&def).expect("serialize");
        let from_toml: GlobalConfig = toml::from_str(&txt).expect("parse");

        assert_eq!(from_toml.project_management, def.project_management, "[project] 段不一致");
        assert_eq!(from_toml.export, def.export, "[export] 段不一致");
        assert_eq!(from_toml.ui, def.ui, "[ui] 段不一致");
        assert_eq!(from_toml.separators, def.separators, "[separators] 段不一致");
    }

    /// 在默认配置基础上改多处值，模拟用户在「全局设置 / 项目布局」改过的状态。
    fn modified_global_config() -> GlobalConfig {
        let mut cfg = GlobalConfig::default();
        cfg.ui.as_mut().unwrap().show_meta_id = true;
        cfg.export
            .as_mut()
            .unwrap()
            .server
            .as_mut()
            .unwrap()
            .java
            .as_mut()
            .unwrap()
            .package = Some("com.x".to_string());
        cfg.separators.tuple2 = "|".to_string();
        // project 布局也放非空值，验证数组单行渲染与整写覆盖
        cfg.project_management.last_project = "p1".to_string();
        cfg.project_management.opened_projects = vec!["p1".to_string(), "p2".to_string()];
        cfg.project_management.project_sort = crate::enums::ProjectSort::Manual;
        cfg.project_management.project_order = vec!["p2".to_string(), "p1".to_string()];
        cfg
    }

    #[test]
    fn write_global_config_round_trip_and_idempotent() {
        // 整文件写入 = serde(GlobalConfig)：规范点分表头、单份、无注释、幂等。
        let cfg = modified_global_config();
        let txt = serialize_global_config(&cfg).expect("serialize");

        // 无任何 # 注释
        assert!(!txt.lines().any(|l| l.trim_start().starts_with('#')), "整写不应再输出注释");
        // 各规范表头恰好出现一次
        let body = format!("\n{}", txt);
        for h in ["[project]", "[export]", "[export.json]", "[export.server.java]",
                  "[ui]", "[ui.ref_picker]", "[separators]", "[separators.Map]"] {
            assert_eq!(body.matches(&format!("\n{}", h)).count(), 1, "表头 {} 出现次数 != 1", h);
        }
        // 不出现裸（无分组前缀）子段
        assert!(!txt.contains("\n[json]"), "出现裸 [json]");
        assert!(!txt.contains("\n[Map]"), "出现裸 [Map]");
        // 数组单行渲染
        assert!(txt.contains("opened_projects = [\"p1\", \"p2\"]"), "opened_projects 应单行: {}", txt);
        assert!(txt.contains("project_order = [\"p2\", \"p1\"]"), "project_order 应单行");
        assert!(txt.contains("project_sort = \"manual\""));

        // 往返解析一致
        let reparsed: GlobalConfig = toml::from_str(&txt).expect("整写文本应可解析");
        assert_eq!(reparsed.project_management, cfg.project_management);
        assert_eq!(reparsed.export, cfg.export);
        assert_eq!(reparsed.ui, cfg.ui);
        assert_eq!(reparsed.separators, cfg.separators);

        // 幂等：同 cfg 再次序列化文本不变
        assert_eq!(txt, serialize_global_config(&cfg).unwrap(), "整写应幂等");
    }

    #[test]
    fn write_global_config_overwrites_and_heals_old_file() {
        // 整写不依赖旧文本：把一份旧注释/脏文件直接覆盖成 canonical 无注释文件。
        let dir = unique_tmp("gs-heal");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tablet.toml"),
            "# 旧 banner\n[project]\nlast_project = \"old\"\n\n[json]\nempty_as = \"null\"\n\n[json]\nempty_as = \"null\"\n",
        ).unwrap();

        let mut cfg = GlobalConfig::default();
        cfg.project_management.last_project = "p9".to_string();
        write_global_config(&dir, &cfg).expect("write");

        let txt = std::fs::read_to_string(dir.join("tablet.toml")).unwrap();
        let reparsed: GlobalConfig = toml::from_str(&txt).expect("清理后应可解析");
        assert_eq!(reparsed.project_management.last_project, "p9");
        assert!(!txt.contains("# 旧 banner"), "注释应被清除");
        assert!(!txt.contains("\n[json]"), "裸/重复子段应被清除");
        assert_eq!(reparsed.export, cfg.export);
        assert_eq!(reparsed.ui, cfg.ui);
        assert_eq!(reparsed.separators, cfg.separators);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_projects_sorts_by_id() {
        let dir = unique_tmp("listorder");
        std::fs::create_dir_all(dir.join("projects/zeta")).unwrap();
        std::fs::create_dir_all(dir.join("projects/alpha")).unwrap();
        std::fs::create_dir_all(dir.join("projects/beta")).unwrap();
        write_schema(&dir.join("projects/zeta"), "zeta", "Z");
        write_schema(&dir.join("projects/alpha"), "alpha", "A");
        write_schema(&dir.join("projects/beta"), "beta", "B");
        let list = list_projects(&dir);
        let ids: Vec<_> = list.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "beta", "zeta"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 孤儿目录（缺 project.tblmanifest 但有 .tbl-cache 之类残留）应被 list_projects 跳过。
    /// 防止"新建项目未保存就关程序，cache 目录留下"导致下次启动加载空 Project 的 bug。
    #[test]
    fn list_projects_skips_dirs_without_schema() {
        let dir = unique_tmp("orphan");
        // 合法 project：有 project.tblmanifest
        std::fs::create_dir_all(dir.join("projects/real/config")).unwrap();
        write_schema(&dir.join("projects/real"), "real", "真项目");
        // 孤儿 project：仅有 .tbl-cache 残留，无 project.tblmanifest
        std::fs::create_dir_all(dir.join("projects/orphan/.tbl-cache")).unwrap();
        std::fs::write(
            dir.join("projects/orphan/.tbl-cache/hero-1234567890.xlsx"),
            b"PK\x03\x04 fake xlsx",
        ).unwrap();
        // 完全空目录（用户手动建的或系统删了 schema）
        std::fs::create_dir_all(dir.join("projects/empty")).unwrap();

        let list = list_projects(&dir);
        let ids: Vec<_> = list.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["real"], "应仅识别有 project.tblmanifest 的目录");
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"p\"\n\n[export.server]\ndata_output = \"gen/server/data\"\n[export.server.java]\npackage = \"com.global\"\ncode_output = \"gen/server/java\"\n",
        ).unwrap();
        let proj = load_project(&dir).expect("load");
        let java = proj.config.export.server.as_ref().unwrap().java.as_ref().unwrap();
        // overlay 覆盖
        assert_eq!(java.package.as_deref(), Some("com.over.ride"));
        // 全局保留（overlay 没动 code_output）
        assert_eq!(java.code_output.as_deref(), Some("gen/server/java"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_toml_ui_section_is_stripped() {
        // 项目级 [ui] / [separators] / [project] 都该被 strip：以全局值为准。
        let dir = unique_tmp("strip_ui");
        std::fs::create_dir_all(dir.join("projects/p/config")).unwrap();
        write_schema(&dir.join("projects/p"), "p", "P");
        // 项目 toml 里写一个 [ui]，期望被剥掉：全局值（realtime_validate=true）应胜出
        std::fs::write(
            dir.join("projects/p/project.toml"),
            "[ui]\nrealtime_validate = false\npicker_trigger_data = \"single\"\n",
        ).unwrap();
        std::fs::write(
            dir.join("tablet.toml"),
            "[project]\nlast_project = \"p\"\n\n[ui]\nrealtime_validate = true\npicker_trigger_data = \"double\"\n",
        ).unwrap();
        let (engine, _, _) = load_workspace(&dir).expect("load");
        let ui = engine.global_config.ui.as_ref().expect("[ui] present");
        // 项目 toml 的 [ui] 该被 strip → 全局值留下来
        assert!(ui.realtime_validate, "项目级 [ui] 应被 strip，全局 realtime_validate=true 留下");
        assert_eq!(ui.picker_trigger_data, crate::enums::PickerTrigger::Double, "项目级 picker_trigger_data 应被 strip");
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"p2\"\n",
        ).unwrap();

        let (engine, _, _) = load_workspace(&dir).expect("load_workspace");
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"p2\"\nopened_projects = [\"p2\", \"p1\"]\n",
        ).unwrap();

        let (engine, _, _) = load_workspace(&dir).expect("load_workspace");
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"alpha\"\n",
        ).unwrap();

        let (engine, _, _) = load_workspace(&dir).expect("load_workspace");
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"a\"\n",
        ).unwrap();

        let (mut engine, _, _) = load_workspace(&dir).expect("load_workspace");
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"a\"\nopened_projects = [\"a\", \"b\"]\n",
        ).unwrap();

        let (mut engine, _, _) = load_workspace(&dir).expect("load_workspace");
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
            dir.join("tablet.toml"),
            "[project]\nname = \"x\"\nlast_project = \"only\"\n",
        ).unwrap();
        let (mut engine, _, _) = load_workspace(&dir).expect("load_workspace");
        engine.close_project("only");
        assert!(engine.projects.is_empty());
        assert_eq!(engine.active_project_id(), None);
        assert_eq!(engine.available().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_persist_global_config_syncs_project() {
        let dir = unique_tmp("ws_persist");
        write_project(&dir, "p1", "1");
        write_project(&dir, "p2", "2");
        std::fs::write(
            dir.join("tablet.toml"),
            "[project]\nname = \"my-game\"\nlast_project = \"p1\"\n\n[ui]\nshow_meta_id = false\n",
        ).unwrap();

        let (mut engine, _, _) = load_workspace(&dir).expect("load");
        engine.open_project("p2").unwrap();
        engine.set_active_by_id("p2");
        // 模拟全局设置也改过一处 ui
        engine.global_config.ui.as_mut().unwrap().show_meta_id = true;
        engine
            .persist_global_config(crate::enums::ProjectSort::Name, &["p2".to_string(), "p1".to_string()])
            .expect("persist");

        // 内存 project_management 已回写（内存=盘）
        assert_eq!(engine.global_config.project_management.last_project, "p2");
        assert_eq!(engine.global_config.project_management.project_sort, crate::enums::ProjectSort::Name);

        // re-load：四段都在、值一致
        let (engine2, _, _) = load_workspace(&dir).expect("reload");
        let opened = engine2.opened_ids();
        assert!(opened.contains(&"p1".to_string()));
        assert!(opened.contains(&"p2".to_string()));
        assert_eq!(engine2.active_project_id(), Some("p2"));
        assert!(engine2.global_config.ui.as_ref().unwrap().show_meta_id, "ui 改动应被整写保留");

        // 文本层：无注释、sort/order/opened 单行
        let txt = std::fs::read_to_string(dir.join("tablet.toml")).unwrap();
        assert!(!txt.lines().any(|l| l.trim_start().starts_with('#')), "文件不应带注释");
        assert!(txt.contains("project_sort = \"name\""));
        assert!(txt.contains("project_order = [\"p2\", \"p1\"]"));
        assert!(txt.contains("opened_projects = [\"p1\", \"p2\"]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reload_adopts_global_config() {
        // reload() 应采纳磁盘上新解析的全局配置（含默认分隔符），避免 stale 副本。
        let dir = unique_tmp("reload_cfg");
        write_project(&dir, "a", "A");
        std::fs::write(
            dir.join("tablet.toml"),
            "[project]\nlast_project = \"a\"\n\n[ui]\nshow_meta_id = true\n",
        ).unwrap();

        let (mut engine, _, _) = load_workspace(&dir).expect("load");
        // 把内存改成与磁盘不同，模拟 stale
        engine.global_config.ui.as_mut().unwrap().show_meta_id = false;
        engine.reload();
        assert!(
            engine.global_config.ui.as_ref().unwrap().show_meta_id,
            "reload 后应采纳磁盘 ui 配置（show_meta_id=true）"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_config_parses_as_global_config() {
        // 默认 GlobalConfig 整写出的文本应能解析出全部默认 [export] 子段
        let txt = serialize_global_config(&GlobalConfig::default()).expect("serialize default");
        let cfg: GlobalConfig = toml::from_str(&txt)
            .expect("serialized default must parse as GlobalConfig");
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
    fn validate_section_parses_and_rejects_bad_values() {
        let global = GlobalConfig::default();
        let ok = parse_project_config(Some(
            "[validate]
error = \"report\"
warning = \"block\"
info = \"report\"
"
        ), &global).expect("合法策略");
        assert_eq!(ok.validate.error, SeverityAction::Report);
        assert_eq!(ok.validate.warning, SeverityAction::Block);
        assert_eq!(ok.validate.info, SeverityAction::Report);

        // 缺省的键按默认值补齐
        let partial = parse_project_config(Some("[validate]
error = \"ignore\"
"), &global)
            .expect("缺省键可补");
        assert_eq!(partial.validate.error, SeverityAction::Ignore);
        assert_eq!(partial.validate.warning, SeverityAction::Report);

        // 非法值 / 拼错的键必须报错
        let err = parse_project_config(Some("[validate]
error = \"nope\"
"), &global)
            .unwrap_err();
        assert!(format!("{:#}", err).contains("[validate]"));
        let err = parse_project_config(Some("[validate]
mistyped = \"block\"
"), &global)
            .unwrap_err();
        assert!(format!("{:#}", err).contains("[validate]"));
    }

    #[test]
    fn project_toml_text_is_comment_free_full_config() {
        // 项目级 toml 含 [export] / [validate] 全量段、无注释——
        // [ui]/[project]/[separators] 都是工作区级。
        let txt = project_toml_text(&ExportConfig::default(), &ValidateConfig::default())
            .expect("text");
        // 完整配置：默认校验策略也要写出来，项目不依赖"文件里没写 = 默认"
        assert!(txt.contains("[validate]"), "校验策略是项目配置的一部分，必须写全");
        assert!(txt.contains("error = \"block\""), "缺省策略也要显式落盘");
        let _: GlobalConfig = toml::from_str(&txt)
            .expect("project.toml default must parse as GlobalConfig");
        assert!(!txt.lines().any(|l| l.trim_start().starts_with('#')), "project.toml 不应带注释");
        assert!(!txt.lines().any(|l| l.trim() == "[project]"), "项目级 toml 不应有 [project] 段");
        assert!(!txt.lines().any(|l| l.trim() == "[ui]"), "项目级 toml 不应有 [ui] 段（工作区级）");
        assert!(!txt.lines().any(|l| l.trim() == "[separators]"), "项目级 toml 不应有 [separators] 段");
        // [export] 全量段都在
        for h in ["[export.json]", "[export.xml]", "[export.server]",
                  "[export.server.java]", "[export.server.go]", "[export.server.cpp]",
                  "[export.server.csharp_dotnet]", "[export.server.typescript]",
                  "[export.client.lua]", "[export.client.gdscript]", "[export.client.typescript]",
                  "[export.client.csharp_unity]", "[export.client.csharp_godot]"] {
            assert!(txt.contains(h), "缺少 {}", h);
        }
    }

    #[test]
    fn unknown_export_key_is_an_error_not_a_silent_fallback() {
        // 已废弃的键（module_kind）留在 project.toml 里必须报错。
        // 以前这里是 .unwrap_or(global_export)——整个项目配置被换成工作区默认值，
        // 连 output 目录都跟着变，且没有任何提示。
        let global = GlobalConfig::default();
        let text = "[export.client.typescript]\noutput = \"gen/x\"\nmodule_kind = \"commonjs\"\n";
        let err = parse_project_config(Some(text), &global)
            .expect_err("废弃键必须报错");
        let msg = format!("{:#}", err);
        assert!(msg.contains("module_kind"), "错误信息要点出键名，实际: {}", msg);
        assert!(msg.contains(PROJECT_TOML_FILE), "错误信息要点出文件，实际: {}", msg);
    }

    #[test]
    fn missing_export_fields_are_repaired_from_global_not_errored() {
        // 只写了部分字段是正常的（字段缺失由工作区配置补齐），不该报错
        let global = GlobalConfig::default();
        let text = "[export.client.typescript]\noutput = \"gen/x\"\n";
        let cfg = parse_project_config(Some(text), &global).expect("部分字段应能加载");
        let ts = cfg.export.client.as_ref().and_then(|c| c.typescript.as_ref()).expect("client.typescript");
        assert_eq!(ts.output.as_deref(), Some("gen/x"));
    }

    #[test]
    fn absent_export_section_copies_global() {
        let global = GlobalConfig::default();
        let cfg = parse_project_config(Some("[ui]\n"), &global).expect("无 export 段应能加载");
        assert_eq!(cfg.export, global.export.unwrap_or_default());
    }

    #[test]
    fn malformed_toml_is_an_error() {
        let global = GlobalConfig::default();
        let err = parse_project_config(Some("this is not = = toml"), &global)
            .expect_err("非法 TOML 必须报错");
        assert!(format!("{:#}", err).contains(PROJECT_TOML_FILE));
    }
    #[test]
    fn xlsx_extra_invalid_column_loads_as_aux_instead_of_failing() {
        let dir = unique_tmp("xlsx_aux");
        std::fs::create_dir_all(&dir).unwrap();
        let book = dir.join("g.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("T").unwrap();
        let rows = [
            ["ID", "名字", "策划备注"],
            ["cs", "cs", "随便"],
            ["int", "str", "任意类型"],
            ["id", "name", "备注"],
            ["1", "亚瑟", "保留 | 原样"],
        ];
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32, c as u16, *v).unwrap();
            }
        }
        std::fs::write(&book, wb.save_to_buffer().unwrap()).unwrap();

        let mut schema = crate::schema_model::TblSchema::default();
        schema.sections.push(crate::schema_model::SchemaSection {
            group: "g".into(),
            name: "T".into(),
            mode: crate::schema_model::SchemaMode::Table,
            storage: crate::model::GroupStorage::Xlsx,
            fields: Vec::new(),
            preset: Vec::new(),
            display: None,
            rules: Vec::new(),
        });
        let group = load_xlsx_group("g", &dir, &schema, &book, None).expect("非规范列不应拖垮加载");
        let t = &group.tables[0];
        assert_eq!(t.schema.fields.len(), 3);
        assert!(!t.schema.fields[0].is_aux());
        assert!(!t.schema.fields[1].is_aux());
        let aux = &t.schema.fields[2];
        assert!(aux.is_aux());
        assert_eq!(aux.label(), "备注");
        assert_eq!(aux.tbl_type, "任意类型");
        assert_eq!(aux.aux.as_ref().unwrap().raw_export, "随便");
        assert_eq!(t.records[0][2], "保留 | 原样");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 策划在 Excel 里删掉某个 sheet：项目要能正常打开，记警告，并修掉 manifest 里的旧段。
    #[test]
    fn missing_xlsx_sheet_loads_with_warning_and_prunes_manifest() {
        let dir = unique_tmp("xlsx_missing_sheet");
        let root = dir.join("projects/p");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(dir.join("tablet.toml"), "[project]\nlast_project = \"p\"\n").unwrap();

        // 工作簿里只有 T；manifest 还声明了被删掉的 Gone。
        let book = root.join("config/g.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("T").unwrap();
        let rows = [
            ["ID", "名字"],
            ["cs", "cs"],
            ["int", "str"],
            ["id", "name"],
            ["1", "亚瑟"],
        ];
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32, c as u16, *v).unwrap();
            }
        }
        std::fs::write(&book, wb.save_to_buffer().unwrap()).unwrap();

        let mut schema = crate::schema_model::TblSchema::default();
        schema.meta.id = "p".into();
        schema.meta.name = "P".into();
        for name in ["T", "Gone"] {
            schema.sections.push(crate::schema_model::SchemaSection {
                group: "g".into(),
                name: name.into(),
                mode: crate::schema_model::SchemaMode::Table,
                storage: crate::model::GroupStorage::Xlsx,
                fields: Vec::new(),
                preset: Vec::new(),
                display: None,
                rules: Vec::new(),
            });
        }
        std::fs::write(
            root.join(PROJECT_MANIFEST_FILE),
            crate::manifest::serialize_tblmanifest(&schema),
        )
        .unwrap();

        let proj = load_project(&dir).expect("缺 sheet 不应拖垮启动");
        let g = proj.groups.iter().find(|g| g.name == "g").expect("组还在");
        assert_eq!(g.tables.len(), 1);
        assert_eq!(g.tables[0].name, "T");
        assert!(
            proj.load_warnings.iter().any(|w| w.contains("Gone")),
            "应记录缺失 sheet 警告: {:?}",
            proj.load_warnings
        );
        let manifest = std::fs::read_to_string(root.join(PROJECT_MANIFEST_FILE)).unwrap();
        assert!(!manifest.contains("Gone"), "manifest 应移除缺失节点:\n{manifest}");
        assert!(manifest.contains("T"), "存在的节点应保留:\n{manifest}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tbl 组的某个 `.tbl` 被删：项目正常启动，警告 + manifest 移除该节点。
    #[test]
    fn missing_tbl_node_loads_with_warning_and_prunes_manifest() {
        let dir = unique_tmp("tbl_missing_node");
        let root = dir.join("projects/p");
        std::fs::create_dir_all(root.join("config/g")).unwrap();
        std::fs::write(dir.join("tablet.toml"), "[project]\nlast_project = \"p\"\n").unwrap();
        std::fs::write(
            root.join("config/g/T.tbl"),
            "#!tbl v3\n#mode table\n---\n\n@field id\n  desc:ID\n  export:cs\n  type:int\n\n[1]\n",
        )
        .unwrap();

        let mut schema = crate::schema_model::TblSchema::default();
        schema.meta.id = "p".into();
        schema.meta.name = "P".into();
        for name in ["T", "Gone"] {
            schema.sections.push(crate::schema_model::SchemaSection {
                group: "g".into(),
                name: name.into(),
                mode: crate::schema_model::SchemaMode::Table,
                storage: crate::model::GroupStorage::Tbl,
                fields: Vec::new(),
                preset: Vec::new(),
                display: None,
                rules: Vec::new(),
            });
        }
        std::fs::write(
            root.join(PROJECT_MANIFEST_FILE),
            crate::manifest::serialize_tblmanifest(&schema),
        )
        .unwrap();

        let proj = load_project(&dir).expect("缺 .tbl 不应拖垮启动");
        let g = proj.groups.iter().find(|g| g.name == "g").expect("组还在");
        assert_eq!(g.tables.len(), 1);
        assert_eq!(g.tables[0].name, "T");
        assert!(
            proj.load_warnings.iter().any(|w| w.contains("Gone.tbl")),
            "应记录缺失 .tbl 警告: {:?}",
            proj.load_warnings
        );
        let manifest = std::fs::read_to_string(root.join(PROJECT_MANIFEST_FILE)).unwrap();
        assert!(!manifest.contains("Gone"), "manifest 应移除缺失节点:\n{manifest}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tbl 组整个目录被删：也要能启动，警告 + manifest 移除该组全部段。
    #[test]
    fn missing_tbl_group_dir_loads_with_warning_and_prunes_manifest() {
        let dir = unique_tmp("tbl_missing_dir");
        let root = dir.join("projects/p");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(dir.join("tablet.toml"), "[project]\nlast_project = \"p\"\n").unwrap();

        let mut schema = crate::schema_model::TblSchema::default();
        schema.meta.id = "p".into();
        schema.meta.name = "P".into();
        schema.sections.push(crate::schema_model::SchemaSection {
            group: "ghost".into(),
            name: "N".into(),
            mode: crate::schema_model::SchemaMode::Table,
            storage: crate::model::GroupStorage::Tbl,
            fields: Vec::new(),
            preset: Vec::new(),
            display: None,
            rules: Vec::new(),
        });
        std::fs::write(
            root.join(PROJECT_MANIFEST_FILE),
            crate::manifest::serialize_tblmanifest(&schema),
        )
        .unwrap();

        let proj = load_project(&dir).expect("组目录丢失不应拖垮启动");
        assert!(proj.groups.is_empty());
        assert!(
            proj.load_warnings.iter().any(|w| w.contains("config/ghost/")),
            "应记录组目录缺失警告: {:?}",
            proj.load_warnings
        );
        let manifest = std::fs::read_to_string(root.join(PROJECT_MANIFEST_FILE)).unwrap();
        assert!(!manifest.contains("ghost"), "manifest 应移除该组全部节点:\n{manifest}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// workbook 新加 sheet：能识别的自动登记为节点，并补进 manifest；私有 sheet 忽略。
    #[test]
    fn new_xlsx_sheets_are_auto_registered_and_manifest_updated() {
        let dir = unique_tmp("xlsx_new_sheets");
        let root = dir.join("projects/p");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(dir.join("tablet.toml"), "[project]\nlast_project = \"p\"\n").unwrap();

        let book = root.join("config/g.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        // 已登记的表
        write_book_sheet(&mut wb, "T", &[
            &["ID", "名字"], &["cs", "cs"], &["int", "str"], &["id", "name"], &["1", "a"],
        ]);
        // 新表：合法 id + 一列不合规（中文列名 → 普通列）
        write_book_sheet(&mut wb, "NewTable", &[
            &["ID", "名字", "价格"], &["cs", "cs", "s"], &["int", "str", "int"],
            &["id", "name", "价格"], &["1", "a", "9"],
        ]);
        write_book_sheet(&mut wb, "NewConst", &[
            &["name", "type", "value", "export", "desc"], &["a", "int", "1", "cs", "x"],
        ]);
        write_book_sheet(&mut wb, "NewEnum", &[&["id", "name", "desc"], &["1", "A", "a"]]);
        // 私有 sheet：固定表头不匹配、也没有合法 id → 不引入
        write_book_sheet(&mut wb, "Private", &[&["备注"], &["x"]]);
        std::fs::write(&book, wb.save_to_buffer().unwrap()).unwrap();

        let mut schema = crate::schema_model::TblSchema::default();
        schema.meta.id = "p".into();
        schema.meta.name = "P".into();
        schema.sections.push(crate::schema_model::SchemaSection {
            group: "g".into(),
            name: "T".into(),
            mode: crate::schema_model::SchemaMode::Table,
            storage: crate::model::GroupStorage::Xlsx,
            fields: Vec::new(),
            preset: Vec::new(),
            display: None,
            rules: Vec::new(),
        });
        std::fs::write(
            root.join(PROJECT_MANIFEST_FILE),
            crate::manifest::serialize_tblmanifest(&schema),
        )
        .unwrap();

        let proj = load_project(&dir).expect("新增 sheet 不应拖垮加载");
        let g = proj.groups.iter().find(|g| g.name == "g").expect("组在");
        let nt = g.tables.iter().find(|t| t.name == "NewTable").expect("新表应登记");
        assert!(
            nt.schema.fields.iter().any(|f| f.is_aux() && f.label() == "价格"),
            "不合规列应走普通列: {:?}",
            nt.schema.fields.iter().map(|f| (f.name.clone(), f.label().to_string())).collect::<Vec<_>>()
        );
        assert_eq!(nt.records[0], vec!["1".to_string(), "a".to_string(), "9".to_string()]);
        assert!(g.constants.iter().any(|c| c.name == "NewConst"), "新常量应登记");
        assert!(g.enums.iter().any(|e| e.name == "NewEnum"), "新枚举应登记");
        assert!(
            !proj.groups.iter().any(|g| g.tables.iter().any(|t| t.name == "Private")),
            "私有 sheet 不应变成节点"
        );
        assert!(
            proj.load_warnings.iter().any(|w| w.contains("NewTable")),
            "应记录自动登记警告: {:?}",
            proj.load_warnings
        );
        let manifest = std::fs::read_to_string(root.join(PROJECT_MANIFEST_FILE)).unwrap();
        for n in ["NewTable", "NewConst", "NewEnum"] {
            assert!(manifest.contains(n), "manifest 应补上 {}:\n{manifest}", n);
        }
        assert!(!manifest.contains("Private"), "私有 sheet 不应入 manifest:\n{manifest}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 整本工作簿被删掉：组整体消失也要能启动，警告 + 从 manifest 移除该组全部段。
    #[test]
    fn missing_xlsx_workbook_loads_with_warning_and_prunes_manifest() {
        let dir = unique_tmp("xlsx_missing_book");
        let root = dir.join("projects/p");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(dir.join("tablet.toml"), "[project]\nlast_project = \"p\"\n").unwrap();

        let mut schema = crate::schema_model::TblSchema::default();
        schema.meta.id = "p".into();
        schema.meta.name = "P".into();
        schema.sections.push(crate::schema_model::SchemaSection {
            group: "hero".into(),
            name: "HeroBase".into(),
            mode: crate::schema_model::SchemaMode::Table,
            storage: crate::model::GroupStorage::Xlsx,
            fields: Vec::new(),
            preset: Vec::new(),
            display: None,
            rules: Vec::new(),
        });
        std::fs::write(
            root.join(PROJECT_MANIFEST_FILE),
            crate::manifest::serialize_tblmanifest(&schema),
        )
        .unwrap();

        let proj = load_project(&dir).expect("整本工作簿丢失不应拖垮启动");
        assert!(proj.groups.is_empty(), "没有工作簿就没有组");
        assert!(
            proj.load_warnings
                .iter()
                .any(|w| w.contains("hero.xlsx") && w.contains("HeroBase")),
            "应记录工作簿缺失警告: {:?}",
            proj.load_warnings
        );
        let manifest = std::fs::read_to_string(root.join(PROJECT_MANIFEST_FILE)).unwrap();
        assert!(!manifest.contains("HeroBase"), "manifest 应移除该组全部节点:\n{manifest}");
        let _ = std::fs::remove_dir_all(&dir);
    }

}
