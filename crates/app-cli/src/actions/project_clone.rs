//! project clone: 深拷贝一个已有 Project 为新 Project。

use anyhow::Result;
use tablet_core::ops::ProjectEngine;

pub fn run_project_clone(
    engine: &mut ProjectEngine,
    source_id: &str,
    new_id: &str,
    new_name: Option<&str>,
) -> Result<()> {
    let display_name = new_name.unwrap_or(new_id);
    let result = engine.clone_project_in_memory(source_id, new_id, display_name);
    if result.is_none() {
        anyhow::bail!("克隆失败（源项目不存在或新 id 不合法）");
    }
    engine.save_all_projects();
    Ok(())
}
