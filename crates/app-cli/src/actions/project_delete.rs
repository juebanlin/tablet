//! project delete: 删除 Project（rm 目录 + 从内存移除）。

use anyhow::Result;
use tablet_core::ops::ProjectEngine;

pub fn run_project_delete(engine: &mut ProjectEngine, id: &str) -> Result<()> {
    let action = tablet_core::ops::ProjectAction::DeleteProject {
        project_id: id.to_string(),
    };
    engine.execute_action(&action);
    Ok(())
}
