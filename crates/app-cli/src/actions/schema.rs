//! schema 子命令：show / add-* / rename-* / delete-*。
//!
//! 所有写操作执行后自动 save_all()。

use anyhow::Result;
use tablet_core::ops::{ProjectAction, ProjectEngine};

#[derive(serde::Serialize)]
pub struct SchemaShowEntry {
    pub group: String,
    pub nodes: Vec<SchemaNodeInfo>,
}

#[derive(serde::Serialize)]
pub struct SchemaNodeInfo {
    pub name: String,
    pub kind: &'static str,
    pub detail: String,
}

pub fn run_schema_show(engine: &ProjectEngine) -> Result<Vec<SchemaShowEntry>> {
    let project = engine.active()
        .ok_or_else(|| anyhow::anyhow!("没有活跃的 Project"))?;

    let mut entries = Vec::new();
    for g in &project.groups {
        let mut nodes = Vec::new();
        for t in &g.tables {
            nodes.push(SchemaNodeInfo {
                name: t.name.clone(),
                kind: "table",
                detail: format!("{} fields, {} rows", t.schema.fields.len(), t.records.len()),
            });
        }
        for c in &g.constants {
            nodes.push(SchemaNodeInfo {
                name: c.name.clone(),
                kind: "constant",
                detail: format!("{} entries", c.entries.len()),
            });
        }
        for e in &g.enums {
            nodes.push(SchemaNodeInfo {
                name: e.name.clone(),
                kind: "enum",
                detail: format!("{} entries", e.entries.len()),
            });
        }
        entries.push(SchemaShowEntry { group: g.name.clone(), nodes });
    }
    Ok(entries)
}

pub fn run_schema_add_group(engine: &mut ProjectEngine, name: &str) -> Result<()> {
    let pid = active_id(engine)?;
    engine.execute_action(&ProjectAction::NewGroup {
        project_id: pid.clone(),
        name: name.to_string(),
    });
    engine.save_all();
    Ok(())
}

pub fn run_schema_add_table(engine: &mut ProjectEngine, group: &str, name: &str) -> Result<()> {
    let pid = active_id(engine)?;
    engine.execute_action(&ProjectAction::NewTable {
        project_id: pid.clone(),
        group: group.to_string(),
        name: name.to_string(),
    });
    engine.save_all();
    Ok(())
}

pub fn run_schema_add_constant(engine: &mut ProjectEngine, group: &str, name: &str) -> Result<()> {
    let pid = active_id(engine)?;
    engine.execute_action(&ProjectAction::NewConstant {
        project_id: pid.clone(),
        group: group.to_string(),
        name: name.to_string(),
    });
    engine.save_all();
    Ok(())
}

pub fn run_schema_add_enum(engine: &mut ProjectEngine, group: &str, name: &str) -> Result<()> {
    let pid = active_id(engine)?;
    engine.execute_action(&ProjectAction::NewEnum {
        project_id: pid.clone(),
        group: group.to_string(),
        name: name.to_string(),
    });
    engine.save_all();
    Ok(())
}

pub fn run_schema_rename_group(engine: &mut ProjectEngine, old: &str, new: &str) -> Result<()> {
    let pid = active_id(engine)?;
    engine.execute_action(&ProjectAction::RenameGroup {
        project_id: pid.clone(),
        old_name: old.to_string(),
        new_name: new.to_string(),
    });
    engine.save_all();
    Ok(())
}

pub fn run_schema_rename_node(
    engine: &mut ProjectEngine,
    group: &str,
    old: &str,
    new: &str,
) -> Result<()> {
    let pid = active_id(engine)?;
    engine.execute_action(&ProjectAction::RenameNode {
        project_id: pid.clone(),
        group: group.to_string(),
        old_name: old.to_string(),
        new_name: new.to_string(),
    });
    engine.save_all();
    Ok(())
}

pub fn run_schema_delete_group(engine: &mut ProjectEngine, name: &str) -> Result<()> {
    engine.delete_group(name);
    engine.save_all();
    Ok(())
}

pub fn run_schema_delete_node(engine: &mut ProjectEngine, group: &str, name: &str) -> Result<()> {
    engine.delete_node(group, name);
    engine.save_all();
    Ok(())
}

fn active_id(engine: &ProjectEngine) -> Result<String> {
    engine.active()
        .map(|p| p.schema.meta.id.clone())
        .ok_or_else(|| anyhow::anyhow!("没有活跃的 Project"))
}
