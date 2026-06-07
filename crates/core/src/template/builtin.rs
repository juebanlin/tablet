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

    /// 内置模板 preset 数据必须按 §8.3.1（中文标点拦截规则）合法。
    /// str 类型可含中文标点；其它类型（int / 复合 / @Ref）不允许。
    /// 这里逐 section 把 preset 翻译成 Table / Constant / EnumDef 跑 validate_*。
    #[test]
    fn builtin_presets_pass_validation() {
        use std::collections::HashMap;

        use crate::model::{Constant, ConstEntry, EnumDef, EnumEntry, Export, FieldDef, Table, TableSchema};
        use crate::tblschema::SchemaMode;
        use crate::types::SeparatorsSection;
        use crate::validate::{validate_constant, validate_enum, validate_table, RefIndex};

        let src = BuiltinTemplates::new();
        for tpl in src.list() {
            let content = src.load_by_id(&tpl.id).expect("load builtin");
            let schema = &content.schema;
            let sep = SeparatorsSection::default();

            // 先聚合一份 RefIndex：每个 table/enum 注册其 preset 的全部 id。
            // ref 校验依赖此索引存在，否则 @Hero/@Faction 等会假报 TypeRefMissing。
            use crate::model::Group;
            let mut groups_map: HashMap<String, Group> = HashMap::new();
            for sec in &schema.sections {
                let g = groups_map.entry(sec.group.clone()).or_insert_with(|| Group {
                    name: sec.group.clone(),
                    dir: std::path::PathBuf::new(),
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: false,
                });
                match sec.mode {
                    SchemaMode::Table => {
                        let fields: Vec<FieldDef> = sec.fields.iter().map(|f| FieldDef {
                            name: f.name.clone(),
                            desc: f.desc.clone(),
                            tbl_type: f.tbl_type.clone(),
                            export: Export::from_str(&f.export),
                        }).collect();
                        g.tables.push(Table {
                            name: sec.name.clone(),
                            path: std::path::PathBuf::new(),
                            schema: TableSchema { fields },
                            records: sec.preset.clone(),
                            dirty: false,
                            deleted: false,
                            original: String::new(),
                        });
                    }
                    SchemaMode::Constant => {
                        let entries: Vec<ConstEntry> = sec.preset.iter().map(|row| {
                            let g = |i: usize| row.get(i).cloned().unwrap_or_default();
                            ConstEntry {
                                name: g(0),
                                tbl_type: g(1),
                                value: g(2),
                                export: Export::from_str(&g(3)),
                                desc: g(4),
                            }
                        }).collect();
                        g.constants.push(Constant {
                            name: sec.name.clone(),
                            path: std::path::PathBuf::new(),
                            entries,
                            dirty: false,
                            deleted: false,
                            original: String::new(),
                        });
                    }
                    SchemaMode::Enum => {
                        let entries: Vec<EnumEntry> = sec.preset.iter().map(|row| {
                            let g = |i: usize| row.get(i).cloned().unwrap_or_default();
                            EnumEntry { id: g(0), name: g(1), desc: g(2) }
                        }).collect();
                        g.enums.push(EnumDef {
                            name: sec.name.clone(),
                            path: std::path::PathBuf::new(),
                            entries,
                            dirty: false,
                            deleted: false,
                            original: String::new(),
                        });
                    }
                }
            }
            let groups: Vec<Group> = groups_map.into_values().collect();
            let refs = RefIndex::build(&groups);

            // 跑 validate_*；任何错误都连同模板 id 一起 panic 出来。
            for g in &groups {
                for t in &g.tables {
                    let errs = validate_table(t, &sep, Some(&refs));
                    assert!(errs.is_empty(),
                        "[{}] table {}/{} 校验失败: {:?}", tpl.id, g.name, t.name, errs);
                }
                for c in &g.constants {
                    let errs = validate_constant(c, &sep, /*allow_ref=*/true, Some(&refs));
                    assert!(errs.is_empty(),
                        "[{}] constant {}/{} 校验失败: {:?}", tpl.id, g.name, c.name, errs);
                }
                for e in &g.enums {
                    let errs = validate_enum(e);
                    assert!(errs.is_empty(),
                        "[{}] enum {}/{} 校验失败: {:?}", tpl.id, g.name, e.name, errs);
                }
            }
        }
    }
}
