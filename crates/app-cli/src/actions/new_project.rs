//! 用模板 / `.tblschema` 文件新建项目 → 可选写回 `tablet.toml` 的 `last_project`。
//!
//! 返回新项目根目录。CLI 会 println 路径，GUI 会刷新树并切到新项目。
//!
//! **两条数据源走同一条实现**：都归结成一个 `TblSchema`，再交给
//! `ProjectEngine::create_project_from_schema`（= 内存态建 + 验证闸门 + 立刻保存）。
//! 那和界面从模板 / 文件新建走的是同一条 L2，差别只在界面停在内存态等用户点保存。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tablet_core::project::PROJECTS_DIR;
use tablet_core::template::{BuiltinTemplates, LocalTemplates, TemplateSource};

#[derive(Debug)]
pub struct NewProjectOutcome {
    pub project_root: PathBuf,
}

/// 新项目的 schema 从哪来——这是两条路唯一的差别。
pub enum NewProjectSource<'a> {
    /// 内置 / 本地模板 id
    Template(&'a str),
    /// 一个 `.tblschema` 文件
    SchemaFile(&'a Path),
}

pub fn run_new_project(
    workdir: &Path,
    source: NewProjectSource<'_>,
    project_id: &str,
    name: Option<&str>,
    switch_after: bool,
) -> Result<NewProjectOutcome> {
    use tablet_core::tblschema::is_valid_metadata_id;
    if !is_valid_metadata_id(project_id) {
        anyhow::bail!("Project id 不合法（要求 [a-z0-9_-]{{1,32}}）：{}", project_id);
    }
    // 目录被别的东西占了就直接说，不要往里写
    let project_root = workdir.join(PROJECTS_DIR).join(project_id);
    if project_root.exists() {
        anyhow::bail!("Project 已存在: {}", project_root.display());
    }

    // `--from-file` 的 schema 若声明了显示名（`# @meta name:`），没给 `--name` 时就沿用它。
    // 以前这里一律 `name.unwrap_or(project_id)`，AI 写在 schema 里的项目名被直接丢掉、
    // 换成了 id —— 而 id 受 `[a-z0-9_-]` 限制，装不下中文名。
    // 模板则不一样：模板的 name 是**模板**的名字，不该变成项目名。
    let mut declared_name: Option<String> = None;
    let mut schema = match source {
        NewProjectSource::Template(template_id) => {
            let builtin = BuiltinTemplates::new();
            let local = LocalTemplates::new(tablet_core::template::default_local_dir());
            let content = builtin.load_by_id(template_id)
                .or_else(|| local.load_by_id(template_id))
                .with_context(|| format!("找不到模板: {}", template_id))?;
            let mut s = content.schema.clone();
            s.meta.source_template = content.meta.id.clone();
            s.meta.source_template_version = content.meta.version.clone();
            s
        }
        NewProjectSource::SchemaFile(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("读取 {} 失败", path.display()))?;
            let s = tablet_core::tblschema::parse_tblschema(&text)
                .with_context(|| format!("解析 {} 失败", path.display()))?;
            // 解析器在缺省时把 name 填成 id；"name == id" 视为没声明。
            if !s.meta.name.is_empty() && s.meta.name != s.meta.id {
                declared_name = Some(s.meta.name.clone());
            }
            s
        }
    };

    schema.meta.id = project_id.to_string();
    schema.meta.name = name
        .map(str::to_string)
        .or(declared_name)
        .unwrap_or_else(|| project_id.to_string());
    schema.meta.created_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // 走 engine：内存态建 → 验证闸门 → 立刻保存。
    // 闸门不过时 `create_project_from_schema` 会把项目整个撤回，这里把原因原样报给用户。
    let (mut engine, _config_fixed, _failed) = tablet_core::project::load_workspace(workdir)?;
    // 注意：这里**不**因为有别的项目打不开就拒绝建新项目 —— 「建一个新的」正是
    // 折腾坏了之后的出路之一，堵死它没有意义。打不开的项目会在其它命令上吵。
    let id = engine.create_project_from_schema(schema).map_err(|e| anyhow::anyhow!(e))?;
    let project_root = engine.find_project(&id)
        .map(|p| p.project_root.clone())
        .with_context(|| format!("创建后找不到项目: {}", id))?;

    if switch_after {
        // tablet.toml 已由 load_workspace 确保存在；只更新 last_project，
        // open 集 / 排序等段整写保留。
        let config_path = workdir.join(tablet_core::CONFIG_FILE);
        let (mut cfg, _fixed) = tablet_core::config_fix::load_and_fix_global_config(&config_path)?;
        cfg.project_management.last_project = id;
        tablet_core::project::write_global_config(workdir, &cfg)?;
    }

    Ok(NewProjectOutcome { project_root })
}
