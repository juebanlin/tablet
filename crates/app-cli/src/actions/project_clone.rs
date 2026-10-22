//! project clone: 深拷贝一个已有 Project 为新 Project。

use anyhow::{Result, bail};
use tablet_core::ops::ProjectEngine;

pub fn run_project_clone(
    engine: &mut ProjectEngine,
    source_id: &str,
    new_id: &str,
    new_name: Option<&str>,
) -> Result<()> {
    // 分开报，别把三种失败揉成一句"源项目不存在或新 id 不合法"——调用方是脚本。
    if !engine.available().iter().any(|a| a.id == source_id) {
        bail!("源 Project 不存在: {}", source_id);
    }
    if !tablet_core::tblschema::is_valid_metadata_id(new_id) {
        bail!("新 id 不合法（要求 [a-z0-9_-]{{1,32}}）：{}", new_id);
    }
    if engine.available().iter().any(|a| a.id == new_id) {
        bail!("Project id 已存在: {}", new_id);
    }

    let display_name = new_name.unwrap_or(new_id);
    let result = engine.clone_project_in_memory(source_id, new_id, display_name);
    if result.is_none() {
        bail!("克隆失败: {} → {}", source_id, new_id);
    }

    // `save_all_projects()` 只要**任何**一个 Project 有验证错误就整体不落盘（连日志都不打）。
    // 克隆结果是否真的写下去了，去盘上看。
    engine.save_all_projects();
    let root = engine.workdir.join(tablet_core::project::PROJECTS_DIR).join(new_id);
    if !root.join(tablet_core::project::PROJECT_SCHEMA_FILE).is_file() {
        bail!(
            "克隆未落盘：{} 不存在（可能有其它 Project 存在验证错误，保存被整体中止）",
            root.display()
        );
    }
    Ok(())
}
