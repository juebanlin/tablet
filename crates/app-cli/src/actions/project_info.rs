//! project info: 显示项目详情（groups/tables/constants/enums/行数/dirty）。

use anyhow::Result;
use serde::Serialize;
use tablet_core::model::Project;
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
    Ok(summarize(project, engine.is_dirty()))
}

/// 直接从一份（刚加载的）Project 出摘要。
///
/// `project info --id` / `--project` 要能查**没打开**的项目 —— 它们不在 engine 的
/// opened 集合里（`load_workspace` 只打开 `opened_projects` / `last_project` 那几个）。
/// 以前这里走 `find_project`，于是 `--id` 对一个盘上存在的项目报"不存在"，
/// 而 `--project` 被 `set_active_by_id` 静默忽略、顺手报告了另一个项目。
pub fn summarize(project: &Project, dirty: bool) -> ProjectInfoSummary {
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

    ProjectInfoSummary {
        id: project.schema.meta.id.clone(),
        name: project.schema.meta.name.clone(),
        groups: project.groups.len(),
        tables,
        constants,
        enums,
        total_rows,
        dirty,
    }
}
