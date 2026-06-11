//! project rename: 重命名 Project（id 和/或 name）。

use anyhow::Result;
use tablet_core::ops::ProjectEngine;

pub fn run_project_rename(
    engine: &mut ProjectEngine,
    id: &str,
    new_id: Option<&str>,
    new_name: Option<&str>,
) -> Result<()> {
    if new_id.is_none() && new_name.is_none() {
        anyhow::bail!("至少需要指定 --new-id 或 --new-name");
    }
    let effective_new_id = new_id.unwrap_or(id);
    let effective_new_name = new_name.unwrap_or_else(|| {
        engine.find_project(id)
            .map(|p| p.schema.meta.name.as_str())
            .unwrap_or(effective_new_id)
    }).to_string();

    let action = tablet_core::ops::ProjectAction::RenameProject {
        old_id: id.to_string(),
        new_id: effective_new_id.to_string(),
        new_name: effective_new_name,
    };
    engine.execute_action(&action);
    engine.save_all_projects();
    Ok(())
}
