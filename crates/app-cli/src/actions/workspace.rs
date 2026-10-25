//! workspace 子命令：info / save / reload / clear。

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use tablet_core::model::GlobalConfig;
use tablet_core::ops::ProjectEngine;

/// `workspace info --fmt json` 的输出。字段全部只读，不加载任何 `.tbl`。
#[derive(Debug, Serialize)]
pub struct WorkspaceInfoSummary {
    pub workdir: String,
    /// "git" / "svn" / null
    pub vcs: Option<&'static str>,
    pub branch: Option<String>,
    pub revision: Option<String>,
    pub detached: bool,
    pub vcs_error: Option<String>,
    pub last_project: Option<String>,
    pub projects: Vec<WorkspaceProjectEntry>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceProjectEntry {
    pub id: String,
    pub name: String,
}

/// 工作区 + 版本库信息。**不加载项目、不占工作区锁**：Unity 插件关掉项目也能显示分支。
pub fn run_workspace_info(workdir: &Path) -> Result<WorkspaceInfoSummary> {
    let cfg = load_global_config_readonly(workdir)?;
    let vcs = tablet_core::vcs::probe(workdir);
    let projects = tablet_core::project::list_projects(workdir)
        .into_iter()
        .map(|p| WorkspaceProjectEntry { id: p.id, name: p.name })
        .collect();
    let last = cfg.project_management.last_project;
    Ok(WorkspaceInfoSummary {
        workdir: workdir.to_string_lossy().replace('\\', "/"),
        vcs: vcs.kind.map(|k| k.as_str()),
        branch: vcs.branch,
        revision: vcs.revision,
        detached: vcs.detached,
        vcs_error: vcs.error,
        last_project: if last.is_empty() { None } else { Some(last) },
        projects,
    })
}

/// 只读解析 `tablet.toml`；不做 `config_fix` 的自动改写（info 命令不写盘）。
fn load_global_config_readonly(workdir: &Path) -> Result<GlobalConfig> {
    let path = workdir.join(tablet_core::CONFIG_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text)
            .with_context(|| format!("解析 {} 失败", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GlobalConfig::default()),
        Err(e) => Err(e).with_context(|| format!("读取 {} 失败", path.display())),
    }
}

/// 保存当前活跃 Project。
///
/// `save_all()` 在项目有验证错误时**一个字节都不写**、只返回 false —— 对外说"已保存"
/// 的命令必须看返回值，否则校验失败在脚本眼里就是成功。
pub fn run_workspace_save(engine: &mut ProjectEngine) -> Result<()> {
    if engine.save_all() {
        return Ok(());
    }
    let errs = engine.validate_error_logs();
    if errs.is_empty() {
        let reason = engine.last_save_failure.clone().unwrap_or_else(|| "盘上未写入".to_string());
        anyhow::bail!("保存失败：{}", reason);
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
