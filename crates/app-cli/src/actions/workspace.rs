//! workspace 子命令：save / reload / clear。

use anyhow::Result;
use tablet_core::ops::ProjectEngine;

/// 保存当前活跃 Project。
///
/// `save_all()` 在项目有验证错误时**一个字节都不写**、只返回 false —— 对外说"已保存"
/// 的命令必须看返回值，否则校验失败在脚本眼里就是成功。
pub fn run_workspace_save(engine: &mut ProjectEngine) -> Result<()> {
    if engine.save_all() {
        return Ok(());
    }
    let errs = engine.validate();
    if errs.is_empty() {
        anyhow::bail!("保存失败：盘上未写入");
    }
    anyhow::bail!(
        "保存失败：项目存在 {} 个验证错误，盘上没有任何改动\n{}",
        errs.len(), errs.join("\n")
    )
}

pub fn run_workspace_reload(engine: &mut ProjectEngine) -> Result<()> {
    engine.reload();
    Ok(())
}

pub fn run_workspace_clear(engine: &mut ProjectEngine) -> Result<()> {
    engine.clear_all_config();
    Ok(())
}
