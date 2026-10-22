//! project delete: 删除 Project（rm 目录 + 从内存移除）。

use anyhow::{Result, bail};
use tablet_core::ops::ProjectEngine;

pub fn run_project_delete(engine: &mut ProjectEngine, id: &str) -> Result<()> {
    // `DeleteProject` 对不存在的 id 只在引擎日志里留一句、然后正常返回；
    // CLI 接着会 println"已删除 Project: X"。先确认它真的存在。
    let known = engine.find_project(id).is_some()
        || engine.available().iter().any(|a| a.id == id);
    if !known {
        bail!("Project 不存在: {}", id);
    }

    let action = tablet_core::ops::ProjectAction::DeleteProject {
        project_id: id.to_string(),
    };
    engine.execute_action(&action);

    // 目录删不掉时 `execute_action` 也只打日志（权限 / 文件占用），别替它宣布成功。
    let root = engine.workdir.join(tablet_core::project::PROJECTS_DIR).join(id);
    if root.exists() {
        bail!("删除失败：{} 仍然存在（详见日志）", root.display());
    }
    Ok(())
}
