//! project info: 显示项目详情（groups/tables/constants/enums/行数/dirty）。

use anyhow::Result;
use serde::Serialize;
use tablet_core::ops::ProjectEngine;

#[derive(Debug, Serialize)]
pub struct ProjectInfoSummary {
    pub id: String,
    pub name: String,
    pub groups: usize,
    pub tables: usize,
    pub constants: usize,
    pub enums: usize,
    pub total_rows: usize,
    pub dirty: bool,
}

pub fn run_project_info(engine: &ProjectEngine, target_id: Option<&str>) -> Result<ProjectInfoSummary> {
    let project = match target_id {
        Some(id) => engine.find_project(id)
            .ok_or_else(|| anyhow::anyhow!("Project 不存在: {}", id))?,
        None => engine.active()
            .ok_or_else(|| anyhow::anyhow!("没有活跃的 Project"))?,
    };

    let mut tables = 0;
    let mut constants = 0;
    let mut enums = 0;
    let mut total_rows = 0;

    for g in &project.groups {
        tables += g.tables.len();
        constants += g.constants.len();
        enums += g.enums.len();
        for t in &g.tables {
            total_rows += t.records.len();
        }
        for c in &g.constants {
            total_rows += c.entries.len();
        }
        for e in &g.enums {
            total_rows += e.entries.len();
        }
    }

    Ok(ProjectInfoSummary {
        id: project.schema.meta.id.clone(),
        name: project.schema.meta.name.clone(),
        groups: project.groups.len(),
        tables,
        constants,
        enums,
        total_rows,
        dirty: engine.is_dirty(),
    })
}
