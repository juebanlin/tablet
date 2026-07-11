//! 用模板新建项目：找模板 → 改写身份元数据 → 实例化到 `projects/<id>/`
//! → 可选写回 `tablet.toml` 的 `last_project`。
//!
//! 返回新项目根目录。CLI 会 println 路径，GUI 会刷新树并切到新项目。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tablet_core::project::PROJECTS_DIR;
use tablet_core::template::{instantiate_template, BuiltinTemplates, LocalTemplates, TemplateSource};

#[derive(Debug)]
pub struct NewProjectOutcome {
    pub project_root: PathBuf,
}

pub fn run_new_project(
    workdir: &Path,
    template_id: &str,
    project_id: &str,
    name: Option<&str>,
    switch_after: bool,
) -> Result<NewProjectOutcome> {
    use tablet_core::tblschema::is_valid_metadata_id;
    if !is_valid_metadata_id(project_id) {
        anyhow::bail!("Project id 不合法（要求 [a-z0-9_-]{{1,32}}）：{}", project_id);
    }

    let builtin = BuiltinTemplates::new();
    let local = LocalTemplates::new(tablet_core::template::default_local_dir());
    let content = builtin.load_by_id(template_id)
        .or_else(|| local.load_by_id(template_id))
        .with_context(|| format!("找不到模板: {}", template_id))?;

    let projects_dir = workdir.join(PROJECTS_DIR);
    std::fs::create_dir_all(&projects_dir)?;
    let project_root = projects_dir.join(project_id);
    if project_root.exists() {
        anyhow::bail!("Project 已存在: {}", project_root.display());
    }

    let display_name = name.unwrap_or(project_id).to_string();
    let mut schema = content.schema.clone();
    schema.meta.id = project_id.to_string();
    schema.meta.name = display_name;
    schema.meta.created_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    schema.meta.source_template = content.meta.id.clone();
    schema.meta.source_template_version = content.meta.version.clone();
    instantiate_template(&schema, &project_root)?;

    if switch_after {
        let config_path = workdir.join(tablet_core::CONFIG_FILE);
        if !config_path.exists() {
            // 用 load_project 顺手把 toml 默认值落盘
            let _ = tablet_core::project::load_project(workdir);
        }
        let original = std::fs::read_to_string(&config_path)?;
        let project_cfg = tablet_core::model::ProjectConfig {
            last_project: project_id.to_string(),
            opened_projects: Vec::new(),
            project_sort: String::new(),
            project_order: Vec::new(),
        };
        let updated = tablet_core::project::upsert_project_config_section(&original, &project_cfg);
        std::fs::write(&config_path, updated)?;
    }

    Ok(NewProjectOutcome { project_root })
}
