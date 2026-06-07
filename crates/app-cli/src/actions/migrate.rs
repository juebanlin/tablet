//! 把根目录 `config/` 迁移到 `projects/default/`。
//!
//! 返回 `bool`：true = 实际迁移；false = 无需迁移（projects/ 已存在或老 config/ 不存在）。

use std::path::Path;

pub fn run_migrate(workdir: &Path) -> anyhow::Result<bool> {
    tablet_core::project::migrate_legacy_to_default(workdir)
}
