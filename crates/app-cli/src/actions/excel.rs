//! Excel 导出 action：把指定 group（或子集）写成多 sheet xlsx。
//!
//! GUI / CLI 共用：返 [`ExcelExportSummary`]，调用方负责打印 / 弹窗。
//!
//! 接口设计（@docs/02-核心功能.md §19）：
//! - 单变体 `Group { name, include }`；`include` 空 = 整组全部，非空 = 子集筛选
//! - 不再区分 Node / Group——单 sheet 编辑场景由 GUI 端构造 `include = vec![node_name]`

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use tablet_core::excel::export_group_book;
use tablet_core::ops::ProjectEngine;

/// Excel 导出目标：分组 + 可选子集。
///
/// `include` 空 = 整组全部（向 core 层传 `None`）；非空 = 仅这些节点（向 core 层传 `Some(&xs)`）。
#[derive(Debug, Clone)]
pub enum ExcelTarget {
    Group { name: String, include: Vec<String> },
}

#[derive(Debug)]
pub struct ExcelExportSummary {
    pub target: ExcelTarget,
    pub output: PathBuf,
    pub bytes: usize,
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
