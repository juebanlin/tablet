//! workspace 子命令：save / reload / clear。

use anyhow::Result;
use tablet_core::ops::ProjectEngine;

pub fn run_workspace_save(engine: &mut ProjectEngine) -> Result<()> {
    engine.save_all();
    Ok(())
}

pub fn run_workspace_reload(engine: &mut ProjectEngine) -> Result<()> {
    engine.reload();
    Ok(())
}

pub fn run_workspace_clear(engine: &mut ProjectEngine) -> Result<()> {
    engine.clear_all_config();
    Ok(())
}
