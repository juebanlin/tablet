//! project rename: 重命名 Project（id 和/或 name）。
//! CLI 采用 util 方式：直接解析和修改文件，不依赖 ProjectEngine 的内存状态。

use anyhow::{Context, Result};
use tablet_core::project::{PROJECT_SCHEMA_FILE, PROJECTS_DIR};
use tablet_core::tblschema::{parse_tblschema, serialize_tblschema};

pub fn run_project_rename(
    engine: &mut tablet_core::ops::ProjectEngine,
    id: &str,
    new_id: Option<&str>,
    new_name: Option<&str>,
) -> Result<()> {
    if new_id.is_none() && new_name.is_none() {
        anyhow::bail!("至少需要指定 --new-id 或 --new-name");
    }

    // 找到项目目录
    let project = engine.find_project(id)
        .with_context(|| format!("项目不存在: {}", id))?;
    let old_root = project.project_root.clone();
    let workdir = project.workdir.clone();

    // 读取 project.tblschema
    let schema_path = old_root.join(PROJECT_SCHEMA_FILE);
    let schema_txt = std::fs::read_to_string(&schema_path)
        .with_context(|| format!("读取 {} 失败", schema_path.display()))?;
    let mut schema = parse_tblschema(&schema_txt)
        .with_context(|| format!("解析 {} 失败", schema_path.display()))?;

    // 修改 id 和/或 name
    let effective_new_id = new_id.unwrap_or(id);
    let effective_new_name = new_name.unwrap_or(&schema.meta.name).to_string();

    let id_changed = id != effective_new_id;
    if id_changed {
        // 检查新 id 是否已存在
        let new_root = workdir.join(PROJECTS_DIR).join(effective_new_id);
        if new_root.exists() {
            anyhow::bail!("项目 id 已存在: {}", effective_new_id);
        }
        schema.meta.id = effective_new_id.to_string();
    }
    schema.meta.name = effective_new_name.clone();

    // 写回 tblschema
    let new_schema_txt = serialize_tblschema(&schema);
    std::fs::write(&schema_path, new_schema_txt)
        .with_context(|| format!("写入 {} 失败", schema_path.display()))?;

    // 如果改了 id，重命名目录
    if id_changed {
        let new_root = workdir.join(PROJECTS_DIR).join(effective_new_id);
        std::fs::rename(&old_root, &new_root)
            .with_context(|| format!("重命名目录失败: {} -> {}", old_root.display(), new_root.display()))?;
        println!("已重命名项目目录: {} -> {}", id, effective_new_id);
    }

    println!("已更新项目: id={}, name={}", effective_new_id, effective_new_name);
    Ok(())
}
