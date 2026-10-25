//! 运行时清单 `project.tblmanifest` 的解析 / 序列化。
//!
//! 清单只负责「项目身份 + 节点索引 + 后端 + 规则 / display」；字段结构来自
//! `.tbl` 表头或 workbook 表头，数据在数据文件里。模板 `.tblschema` 仍归
//! [`crate::tblschema`] 管。
//!
//! 两种文件共享 section 语法，但清单不允许字段行和 `# @preset`；这条限制在
//! 共享解析器里按 [`SchemaKind::Manifest`] 执行。

use anyhow::Result;

use crate::model::Severity;
use crate::tblschema::{
    parse_schema, write_section_header, write_sep_diff, SchemaKind, TblSchema,
};

pub const MANIFEST_HEADER: &str = "#!tblmanifest v1";

/// 解析运行时清单。
pub fn parse_tblmanifest(content: &str) -> Result<TblSchema> {
    parse_schema(content, SchemaKind::Manifest)
}

/// 序列化运行时清单：不写字段行、不写 preset。
pub fn serialize_tblmanifest(schema: &TblSchema) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "{}", MANIFEST_HEADER).unwrap();

    if !schema.meta.id.is_empty() {
        writeln!(s, "# @meta id: {}", schema.meta.id).unwrap();
    }
    if !schema.meta.name.is_empty() && schema.meta.name != schema.meta.id {
        writeln!(s, "# @meta name: {}", schema.meta.name).unwrap();
    }
    if !schema.meta.category.is_empty() {
        writeln!(s, "# @meta category: {}", schema.meta.category).unwrap();
    }
    if !schema.meta.version.is_empty() {
        writeln!(s, "# @meta version: {}", schema.meta.version).unwrap();
    }
    if !schema.meta.created_at.is_empty() {
        writeln!(s, "# @meta created_at: {}", schema.meta.created_at).unwrap();
    }
    if !schema.meta.source_template.is_empty() {
        writeln!(s, "# @meta source_template: {}", schema.meta.source_template).unwrap();
    }
    if !schema.meta.source_template_version.is_empty() {
        writeln!(s, "# @meta source_template_version: {}", schema.meta.source_template_version).unwrap();
    }

    write_sep_diff(&mut s, &schema.separators);

    for sec in &schema.sections {
        writeln!(s).unwrap();
        write_section_header(&mut s, sec);
        if let Some(d) = &sec.display {
            writeln!(s, "# @display: {}", d).unwrap();
        }
        for rule in &sec.rules {
            let level = match rule.level {
                Severity::Error => "",
                Severity::Warning => " [warn]",
                Severity::Info => " [info]",
            };
            writeln!(s, "# @rule: {} = {}{}", rule.column, rule.expr_string(), level).unwrap();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GroupStorage;

    #[test]
    fn manifest_round_trips_xlsx_and_semantics() {
        let src = r#"#!tblmanifest v1
# @meta id: demo
# @meta name: 演示

[hero/HeroBase] table
# @display: name
# @rule: id = range(1, 10)

[building/Building] table xlsx
"#;
        let s = parse_tblmanifest(src).expect("parse manifest");
        assert_eq!(s.sections.len(), 2);
        assert_eq!(s.sections[0].storage, GroupStorage::Tbl);
        assert_eq!(s.sections[1].storage, GroupStorage::Xlsx);
        assert!(s.sections[0].fields.is_empty(), "manifest 不承载字段");
        assert!(s.sections[0].preset.is_empty(), "manifest 不承载数据");
        assert_eq!(s.sections[0].display.as_deref(), Some("name"));
        assert_eq!(s.sections[0].rules.len(), 1);

        let out = serialize_tblmanifest(&s);
        assert!(out.starts_with("#!tblmanifest v1"));
        assert!(out.contains("[hero/HeroBase] table"));
        assert!(out.contains("[building/Building] table xlsx"));
        assert!(out.contains("# @display: name"));
        assert!(!out.contains("id | int"), "不能写字段行");

        let back = parse_tblmanifest(&out).expect("re-parse");
        assert_eq!(back, s);
    }

    #[test]
    fn manifest_rejects_field_rows_and_preset() {
        let field = "#!tblmanifest v1\n[g/T] table\nid | int | cs | ID\n";
        let err = parse_tblmanifest(field).unwrap_err().to_string();
        assert!(err.contains("不允许字段") && err.contains("line 3"), "{err}");

        let preset = "#!tblmanifest v1\n[g/T] table\n# @preset\n1\n";
        let err = parse_tblmanifest(preset).unwrap_err().to_string();
        assert!(err.contains("@preset"), "{err}");
    }

    #[test]
    fn manifest_requires_its_own_header() {
        let err = parse_tblmanifest("#!tblschema v1\n[g/T] table\n")
            .unwrap_err().to_string();
        assert!(err.contains("tblmanifest"), "{err}");
    }

    #[test]
    fn storage_must_be_consistent_within_a_group() {
        let mixed = "#!tblmanifest v1\n[g/A] table\n[g/B] table xlsx\n";
        let err = parse_tblmanifest(mixed).unwrap_err().to_string();
        assert!(err.contains("storage") && err.contains("g"), "{err}");
    }
}
