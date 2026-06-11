//! Excel 导出/导入 action。
//!
//! - 导出：把指定 group（或子集）写成多 sheet xlsx。
//! - 导入：把策划编辑过的 xlsx 回读到内存 group，然后保存。

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use tablet_core::excel::{export_group_book, import_xlsx_into_group, GroupPatch, NodePatch};
use tablet_core::ops::ProjectEngine;

/// Excel 导出目标：分组 + 可选子集。
#[derive(Debug, Clone, serde::Serialize)]
pub enum ExcelTarget {
    Group { name: String, include: Vec<String> },
}

#[derive(Debug, serde::Serialize)]
pub struct ExcelExportSummary {
    pub target: ExcelTarget,
    pub output: PathBuf,
    pub bytes: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct ExcelImportSummary {
    pub group: String,
    pub nodes_patched: usize,
}

/// 把 group（或子集）导成 xlsx，落盘到 `output`（None = 当前目录 / `<group>.xlsx`）。
pub fn run_excel_export(
    engine: &ProjectEngine,
    target: ExcelTarget,
    output: Option<&Path>,
) -> Result<ExcelExportSummary> {
    let project = engine.project();
    let (bytes, default_name) = match &target {
        ExcelTarget::Group { name, include } => export_group(project, name, include)?,
    };

    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("{}.xlsx", default_name)));

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&out_path, &bytes)?;

    Ok(ExcelExportSummary {
        target,
        output: out_path,
        bytes: bytes.len(),
    })
}

/// 把 xlsx 回读到内存 group 并保存。
pub fn run_excel_import(
    engine: &mut ProjectEngine,
    group_name: &str,
    file: &Path,
) -> Result<ExcelImportSummary> {
    let project = engine.project();
    let group = project.groups.iter()
        .find(|g| g.name == group_name)
        .ok_or_else(|| anyhow!("未找到分组: {}", group_name))?;

    let patch = import_xlsx_into_group(file, group)?;
    let nodes_patched = patch.patches.len();

    let project_mut = engine.project_mut();
    let group_mut = project_mut.groups.iter_mut()
        .find(|g| g.name == group_name)
        .ok_or_else(|| anyhow!("未找到分组: {}", group_name))?;
    apply_patch(group_mut, &patch);

    engine.save_all();

    Ok(ExcelImportSummary { group: group_name.to_string(), nodes_patched })
}

fn apply_patch(group: &mut tablet_core::model::Group, patch: &GroupPatch) {
    for np in &patch.patches {
        match np {
            NodePatch::Table { name, records } => {
                if let Some(t) = group.tables.iter_mut().find(|t| &t.name == name) {
                    t.records = records.clone();
                    t.update_dirty();
                }
            }
            NodePatch::Constant { name, entries } => {
                if let Some(c) = group.constants.iter_mut().find(|c| &c.name == name) {
                    c.entries = entries.clone();
                    c.update_dirty();
                }
            }
            NodePatch::Enum { name, entries } => {
                if let Some(e) = group.enums.iter_mut().find(|e| &e.name == name) {
                    e.entries = entries.clone();
                    e.update_dirty();
                }
            }
        }
    }
}

fn export_group(
    project: &tablet_core::model::Project,
    name: &str,
    include: &[String],
) -> Result<(Vec<u8>, String)> {
    let g = project
        .groups
        .iter()
        .find(|g| g.name == name)
        .ok_or_else(|| anyhow!("未找到分组: {}", name))?;

    let bytes = if include.is_empty() {
        export_group_book(g, None)?
    } else {
        let xs: Vec<&str> = include.iter().map(|s| s.as_str()).collect();
        export_group_book(g, Some(&xs))?
    };
    Ok((bytes, g.name.clone()))
}
