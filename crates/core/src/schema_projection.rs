//! groups ↔ schema model 的投影 / 应用：与文件格式无关的领域逻辑。
//!
//! 模板导出、运行时 manifest 投影、Excel 导入都走这里；格式模块只负责文本读写。

use std::path::Path;

use anyhow::{bail, Result};

use crate::model::*;
use crate::schema_grammar::validate_storage_consistency;
use crate::schema_model::*;

pub fn schema_from_project(groups: &[Group], with_preset: bool) -> TblSchema {
    let mut sections = Vec::new();
    for group in groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let fields = table.schema.fields.iter().map(|f| SchemaField {
                name: f.name.clone(),
                tbl_type: f.tbl_type.clone(),
                export: export_to_code(&f.export),
                desc: f.desc.clone(),
            }).collect();
            let preset = if with_preset {
                table.records.iter().map(|r| r.clone()).collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: table.name.clone(),
                mode: SchemaMode::Table,
                storage: group.storage,
                fields,
                preset,
                display: table.display_field.clone(),
                rules: table.rules.clone(),
            });
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            // 新范式：constant section 的 schema fields 永远空（结构信息全在项目 .tbl）
            let preset = if with_preset {
                constant.entries.iter()
                    .filter(|e| !e.name.is_empty())
                    .map(|e| vec![
                        e.name.clone(),
                        e.tbl_type.clone(),
                        e.value.clone(),
                        export_to_code(&e.export),
                        e.desc.clone(),
                    ])
                    .collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: constant.name.clone(),
                mode: SchemaMode::Constant,
                storage: group.storage,
                fields: Vec::new(),
                preset,
                display: None,
                rules: Vec::new(),
            });
        }
        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let preset = if with_preset {
                enum_def.entries.iter()
                    .filter(|e| !e.id.is_empty() || !e.name.is_empty())
                    .map(|e| vec![e.id.clone(), e.name.clone(), e.desc.clone()])
                    .collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: enum_def.name.clone(),
                mode: SchemaMode::Enum,
                storage: group.storage,
                fields: Vec::new(),
                preset,
                display: enum_def.display_field.clone(),
                rules: enum_def.rules.clone(),
            });
        }
    }
    TblSchema { meta: SchemaMetadata::default(), separators: Default::default(), sections }
}

fn export_to_code(e: &Export) -> String {
    e.code().to_string()
}

pub fn apply_schema_to_project(
    groups: &mut Vec<Group>,
    sections: &[SchemaSection],
    config_dir: &Path,
    with_preset: bool,
) -> Result<(usize, usize)> {
    validate_storage_consistency(sections)?;
    // 先只读校验一遍：后端不一致必须在改动任何节点之前失败，避免留下半截导入。
    for sec in sections {
        if let Some(g) = groups.iter().find(|g| g.name == sec.group) {
            if g.storage != sec.storage {
                bail!(
                    "组 {} 已存在且后端是 {}，模板要求 {}；不自动转换后端",
                    sec.group, g.storage.as_str(), sec.storage.as_str()
                );
            }
        }
    }

    let mut added = 0usize;
    let mut overwritten = 0usize;

    for sec in sections {
        let group = match groups.iter_mut().find(|g| g.name == sec.group) {
            Some(g) => {
                if g.storage != sec.storage {
                    bail!(
                        "组 {} 已存在且后端是 {}，模板要求 {}；不自动转换后端",
                        sec.group, g.storage.as_str(), sec.storage.as_str()
                    );
                }
                g
            }
            None => {
                let dir = if sec.storage == GroupStorage::Xlsx {
                    config_dir.to_path_buf()
                } else {
                    config_dir.join(&sec.group)
                };
                groups.push(Group {
                    name: sec.group.clone(),
                    original_name: sec.group.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: true,
                    storage: sec.storage,
                    book_stamp: None,
                });
                groups.last_mut().unwrap()
            }
        };
        // 节点路径按组后端推导：xlsx 组所有节点都指向同一个工作簿。
        let node_path = |group: &Group, name: &str| -> std::path::PathBuf {
            if group.is_xlsx() {
                group.book_path()
            } else {
                group.dir.join(format!("{}.tbl", name))
            }
        };

        match sec.mode {
            SchemaMode::Table => {
                let fields: Vec<FieldDef> = sec.fields.iter().map(|f| FieldDef {
                    name: f.name.clone(),
                    desc: f.desc.clone(),
                    tbl_type: f.tbl_type.clone(),
                    // .tblschema 的字段行在 `parse_field_line` 已严格校验；这里拿到的是已校验数据
                    export: Export::from_input(&f.export),
                }).collect();
                let new_len = fields.len();

                let records: Vec<Vec<String>> = if with_preset {
                    sec.preset.iter().map(|row| {
                        let mut r = row.clone();
                        r.resize(new_len, String::new()); // 多余/缺失列对齐
                        r
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(table) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    let old_len = table.schema.fields.len();
                    table.schema.fields = fields;
                    if with_preset {
                        // preset 覆盖现有 records（与 overwritten 语义一致）
                        table.records = records;
                    } else {
                        // 仅同步列结构，保留现有 records 但按新列数 resize
                        for row in &mut table.records {
                            row.resize(new_len, String::new());
                            if new_len < old_len { row.truncate(new_len); }
                        }
                    }
                    table.display_field = sec.display.clone();
                    table.rules = sec.rules.clone();
                    table.dirty = true;
                    overwritten += 1;
                } else {
                    let path = node_path(group, &sec.name);
                    group.tables.push(Table {
                        name: sec.name.clone(),
                        original_name: sec.name.clone(),
                        path,
                        schema: TableSchema { fields: fields.clone() },
                        records,
                        dirty: true,
                        deleted: false,
                        original_records: Vec::new(),
                        original_fields: fields,
                        saved: false,
                        fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
                        display_field: sec.display.clone(),
                        original_display_field: None,
                        rules: sec.rules.clone(),
                        original_rules: Vec::new(),
                    });
                    added += 1;
                }
            }
            SchemaMode::Constant => {
                // preset 行：name | type | value | export | desc
                let entries: Vec<ConstEntry> = if with_preset {
                    sec.preset.iter().map(|row| ConstEntry {
                        name:     row.first()  .cloned().unwrap_or_default(),
                        tbl_type: row.get(1)   .cloned().unwrap_or_default(),
                        value:    row.get(2)   .cloned().unwrap_or_default(),
                        // 常量预设行同样在 parse_tblschema 里校验过
                        export:   Export::from_input(row.get(3).map(String::as_str).unwrap_or("cs")),
                        desc:     row.get(4)   .cloned().unwrap_or_default(),
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(constant) = group.constants.iter_mut().find(|c| c.name == sec.name) {
                    if with_preset {
                        constant.entries = entries;
                    } // 不带预设：保留现有 entries，仅记一次 overwritten
                    constant.dirty = true;
                    overwritten += 1;
                } else {
                    let path = node_path(group, &sec.name);
                    let snap = entries.clone();
                    group.constants.push(Constant {
                        name: sec.name.clone(),
                        original_name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original_entries: snap, saved: false,
                    });
                    added += 1;
                }
            }
            SchemaMode::Enum => {
                // preset 行：id | name | desc
                let entries: Vec<EnumEntry> = if with_preset {
                    sec.preset.iter().map(|row| EnumEntry {
                        id:   row.first().cloned().unwrap_or_default(),
                        name: row.get(1) .cloned().unwrap_or_default(),
                        desc: row.get(2) .cloned().unwrap_or_default(),
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(en) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    if with_preset {
                        en.entries = entries;
                    }
                    en.display_field = sec.display.clone();
                    en.rules = sec.rules.clone();
                    en.dirty = true;
                    overwritten += 1;
                } else {
                    let path = node_path(group, &sec.name);
                    let snap = entries.clone();
                    group.enums.push(EnumDef {
                        name: sec.name.clone(),
                        original_name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original_entries: snap, saved: false,
                        display_field: sec.display.clone(),
                        original_display_field: None,
                        rules: sec.rules.clone(),
                        original_rules: Vec::new(),
                    });
                    added += 1;
                }
            }
        }
    }

    Ok((added, overwritten))
}

pub fn apply_display_to_groups(groups: &mut [Group], schema: &TblSchema) {
    for sec in &schema.sections {
        let Some(display) = &sec.display else { continue };
        let Some(group) = groups.iter_mut().find(|g| g.name == sec.group) else { continue };
        match sec.mode {
            SchemaMode::Table => {
                if let Some(t) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    t.display_field = Some(display.clone());
                    // 这是**从盘上读回来的**值（`.tbl` 里没有 `# @display`，靠 schema 回填），
                    // 所以快照要一起填——否则一打开就显示"改过"，项目永远亮 🟡。
                    t.original_display_field = Some(display.clone());
                }
            }
            SchemaMode::Enum => {
                if let Some(e) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    e.display_field = Some(display.clone());
                    e.original_display_field = Some(display.clone());
                }
            }
            SchemaMode::Constant => {}
        }
    }
}

pub fn apply_rules_to_groups(groups: &mut [Group], schema: &TblSchema) {
    for sec in &schema.sections {
        let Some(group) = groups.iter_mut().find(|g| g.name == sec.group) else { continue };
        match sec.mode {
            SchemaMode::Table => {
                if let Some(t) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    t.rules = sec.rules.clone();
                    t.original_rules = sec.rules.clone();
                }
            }
            // 常量不支持规则（@01.8.8）：schema 里写了也在解析阶段报错。
            SchemaMode::Constant => {}
            SchemaMode::Enum => {
                if let Some(e) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    e.rules = sec.rules.clone();
                    e.original_rules = sec.rules.clone();
                }
            }
        }
    }
}
