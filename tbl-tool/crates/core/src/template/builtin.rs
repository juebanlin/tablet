//! 内置模板源：通过 `include_str!` 嵌入 `crates/core/schemas/*.tblschema`。
//!
//! 增加内置模板：把新文件放进 `crates/core/schemas/`，再在 `BUILTIN_FILES` 末尾添加一行
//! `(include_str!("../../schemas/X.tblschema"), "stem-fallback")`。

use super::{TemplateContent, TemplateMeta, TemplateSource};
use crate::tblschema::{fill_metadata_defaults, parse_tblschema};

/// 内置模板列表。每条 = (raw 文本, 文件名兜底 stem)。
///
/// 解析失败 = 编译期就坏的内置 schema，单元测试会先失败把它顶出来。
const BUILTIN_FILES: &[(&str, &str)] = &[
    (include_str!("../../schemas/standard.tblschema"), "standard"),
    (include_str!("../../schemas/sanguo.tblschema"), "sanguo"),
];

#[derive(Debug, Default)]
pub struct BuiltinTemplates;

impl BuiltinTemplates {
    pub fn new() -> Self {
        Self
    }

    fn all_contents(&self) -> Vec<TemplateContent> {
        BUILTIN_FILES
            .iter()
            .map(|(raw, stem)| {
                let mut schema =
                    parse_tblschema(raw).expect("内置 schema 解析失败，crates/core/schemas 出错");
                fill_metadata_defaults(&mut schema, stem);
                TemplateContent {
                    meta: TemplateMeta::from_schema(&schema, "builtin"),
                    raw: (*raw).to_string(),
                    schema,
                }
            })
            .collect()
    }
}

impl TemplateSource for BuiltinTemplates {
    fn list(&self) -> Vec<TemplateMeta> {
        self.all_contents().into_iter().map(|c| c.meta).collect()
    }

    fn load_by_id(&self, id: &str) -> Option<TemplateContent> {
        self.all_contents().into_iter().find(|c| c.meta.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_full_and_empty() {
        let src = BuiltinTemplates::new();
        let list = src.list();
        let ids: Vec<_> = list.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"standard"));
        for m in &list {
            assert_eq!(m.source, "builtin");
        }
    }

    #[test]
    fn load_full_returns_schema_with_sections() {
        let src = BuiltinTemplates::new();
        let c = src.load_by_id("standard").expect("standard template");
        assert_eq!(c.meta.id, "standard");
        assert!(!c.schema.sections.is_empty());
        // standard 包含 enum + table + constant
        let modes: Vec<_> = c
            .schema
            .sections
            .iter()
            .map(|s| format!("{:?}", s.mode))
            .collect();
        assert!(modes.iter().any(|m| m == "Table"));
        assert!(modes.iter().any(|m| m == "Enum"));
        assert!(modes.iter().any(|m| m == "Constant"));
    }

    #[test]
    fn unknown_id_returns_none() {
        let src = BuiltinTemplates::new();
        assert!(src.load_by_id("nonexistent").is_none());
    }
}
