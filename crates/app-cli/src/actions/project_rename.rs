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
        // 走 core 的校验，**别自己拼路径判存在**。只判 `new_root.exists()` 会放过两类：
        //   1. 字符集 —— `Bad-Id` / `foo bar` / 中文。新建与克隆都拦这些，只有这里放行，
        //      于是能造出一个"别的地方都不允许存在"的 id。
        //   2. **路径穿越** —— `--new-id ../escaped` 会把目录从 `projects/` 里挪出去
        //      （实测过，项目直接落到工作区根）。这是 `validate_project_id_rename`
        //      本来就要挡的事，它却一直没人调用。
        if let Some(err) = engine.validate_project_id_rename(effective_new_id, id) {
            anyhow::bail!("{}", err);
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
