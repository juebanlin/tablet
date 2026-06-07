//! 列项目：扫 `<workdir>/projects/`，返回 `ProjectListEntry` 列表。
//!
//! 直接复用 [`tablet_core::project::list_projects`]，本模块只是放在 actions 树下
//! 让 GUI / CLI 通过 `tablet_cli::actions::list_projects::list_projects` 调，
//! 跟其它 action 命名一致。

use std::path::Path;
use tablet_core::project::ProjectListEntry;

pub fn list_projects(workdir: &Path) -> Vec<ProjectListEntry> {
    tablet_core::project::list_projects(workdir)
}
