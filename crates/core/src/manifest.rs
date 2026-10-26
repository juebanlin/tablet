//! 运行时清单 `project.tblmanifest` 的解析 / 序列化。
//!
//! 清单只负责「项目身份 + 节点索引 + 后端 + 规则 / display」；字段结构来自
//! `.tbl` 表头或 workbook 表头，数据在数据文件里。模板 `.tblschema` 仍归
//! [`crate::tblschema`] 管。
//!
//! 两种文件共享 section / directive 词法（`schema_grammar`），但清单自己的解析器
//! 拒绝字段行和 `# @preset`；模板规则不进入本模块。

use anyhow::{Result, bail};

use crate::schema_grammar::{
    apply_sep_kv, parse_meta_line, parse_rule_body, parse_sep_line,
    parse_section_header, validate_storage_consistency, write_display, write_meta,
    write_rules, write_sep_diff, write_section_header,
};
use crate::schema_model::{
    is_valid_metadata_id, SchemaMetadata, TblSchema,
};
use crate::types::SeparatorsSection;

pub const MANIFEST_HEADER: &str = "#!tblmanifest v1";

/// 解析运行时清单。manifest 不允许字段行 / 数据行 / `# @preset`。
pub fn parse_tblmanifest(content: &str) -> Result<TblSchema> {
    let first = content.lines().map(str::trim).find(|l| !l.is_empty());
    if first != Some(MANIFEST_HEADER) {
        bail!("line 1: 缺少 `{}` 头部（运行时文件必须是 project.tblmanifest）", MANIFEST_HEADER);
    }

    let mut sections = Vec::new();
    let mut current: Option<crate::schema_model::SchemaSection> = None;
    let mut meta = SchemaMetadata::default();
    let mut meta_id_line: Option<usize> = None;
    let mut separators = SeparatorsSection::default();
    let mut seen_section = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.trim().is_empty() { continue; }

        if trimmed.starts_with('#') {
            if let Some(rest) = trimmed.trim_start_matches('#').trim_start().strip_prefix("@preset") {
                if rest.trim().is_empty() {
                    bail!("line {}: 运行时清单不允许 `# @preset`（数据必须放在 .tbl / xlsx 里）", line_num + 1);
                }
            }
            if let Some(rest) = trimmed.trim_start_matches('#').trim_start().strip_prefix("@display") {
                if let Some(v) = rest.trim_start().strip_prefix(':') {
                    if let Some(ref mut sec) = current {
                        let v = v.trim();
                        sec.display = if v.is_empty() { None } else { Some(v.to_string()) };
                    }
                    continue;
                }
            }
            if let Some(rest) = trimmed.trim_start_matches('#').trim_start().strip_prefix("@rule") {
                let ours = rest.is_empty() || rest.starts_with(':')
                    || rest.starts_with(|c: char| c.is_whitespace());
                if ours {
                    if current.is_none() {
                        bail!("line {}: `# @rule` 必须出现在某个 [section] 之后", line_num + 1);
                    }
                    let after = rest.trim_start();
                    let Some(body) = after.strip_prefix(':') else {
                        bail!("line {}: `# @rule` 只支持 `# @rule: <列> = <表达式> [级别]`", line_num + 1);
                    };
                    if let Some(ref sec) = current {
                        if matches!(sec.mode, crate::schema_model::SchemaMode::Constant) {
                            bail!("line {}: constant 段不支持 `# @rule`", line_num + 1);
                        }
                    }
                    let rule = parse_rule_body(body)
                        .map_err(|msg| anyhow::anyhow!("line {}: {}", line_num + 1, msg))?;
                    if let Some(ref mut sec) = current { sec.rules.push(rule); }
                    continue;
                }
            }
            if !seen_section {
                if let Some((key, value)) = parse_meta_line(trimmed) {
                    match key.as_str() {
                        "id" => { meta_id_line = Some(line_num + 1); meta.id = value; }
                        "name" => meta.name = value,
                        "category" => meta.category = value,
                        "version" => meta.version = value,
                        "created_at" => meta.created_at = value,
                        "source_template" => meta.source_template = value,
                        "source_template_version" => meta.source_template_version = value,
                        "has_preset" => { /* manifest 没有 preset；忽略 */ }
                        _ => {}
                    }
                } else if let Some((key, value)) = parse_sep_line(trimmed) {
                    apply_sep_kv(&mut separators, &key, &value);
                }
            }
            continue;
        }

        if trimmed.starts_with('[') {
            seen_section = true;
            if let Some(sec) = current.take() {
                sections.push(sec);
            }
            current = Some(parse_section_header(trimmed, line_num + 1)?);
        } else if current.is_some() {
            bail!("line {}: 运行时清单不允许字段/数据行（结构由 .tbl / xlsx 表头提供）", line_num + 1);
        } else {
            bail!("line {}: field outside section", line_num + 1);
        }
    }
    if let Some(sec) = current { sections.push(sec); }

    validate_storage_consistency(&sections)?;
    if meta.name.is_empty() { meta.name = meta.id.clone(); }
    meta.has_preset = false;
    if let Some(n) = meta_id_line {
        if !is_valid_metadata_id(&meta.id) {
            bail!("line {}: `# @meta id` 不合法: {:?}", n, meta.id);
        }
    }
    Ok(TblSchema { meta, separators, sections })
}

/// 序列化运行时清单：不写字段行、不写 preset。
pub fn serialize_tblmanifest(schema: &TblSchema) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "{}", MANIFEST_HEADER).unwrap();

    write_meta(&mut s, &schema.meta, false);

    write_sep_diff(&mut s, &schema.separators);

    for sec in &schema.sections {
        writeln!(s).unwrap();
        write_section_header(&mut s, sec);
        write_display(&mut s, sec);
        write_rules(&mut s, sec);
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
