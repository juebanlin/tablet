//! 列模板：返回内置 + 本地两组 `TemplateMeta`。
//!
//! 不打印；CLI / GUI 自行决定怎么展示。

use tbl_core::template::{BuiltinTemplates, LocalTemplates, TemplateMeta, TemplateSource};

#[derive(Debug, Default)]
pub struct TemplateList {
    pub builtin: Vec<TemplateMeta>,
    pub local: Vec<TemplateMeta>,
    /// 本地模板根目录（用于 UI 展示）。
    pub local_root: std::path::PathBuf,
}

pub fn list_templates() -> TemplateList {
    let builtin = BuiltinTemplates::new();
    let local_root = tbl_core::template::default_local_dir();
    let local = LocalTemplates::new(local_root.clone());
    TemplateList {
        builtin: builtin.list(),
        local: local.list(),
        local_root,
    }
}
