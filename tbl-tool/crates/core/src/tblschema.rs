use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;

#[derive(Debug, Clone, Default)]
pub struct SchemaMetadata {
    pub id: String,         // 路径标识：[a-z0-9_-]{1,32}；缺省时由调用方按文件名兜底
    pub name: String,       // 显示文本（含中文）；缺省时 = id
    pub category: String,   // 分类筛选（test / slg / rpg / ...）
    pub version: String,    // semver
}

#[derive(Debug, Clone, Default)]
pub struct TblSchema {
    pub meta: SchemaMetadata,
    pub sections: Vec<SchemaSection>,
}

#[derive(Debug, Clone)]
pub struct SchemaSection {
    pub group: String,
    pub name: String,
    pub mode: SchemaMode,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaMode {
    Table,
    Constant,
    Enum,
}

#[derive(Debug, Clone)]
pub struct SchemaField {
    pub name: String,
    pub tbl_type: String,
    pub export: String,
    pub desc: String,
}

impl SchemaField {
    pub fn export_display(&self) -> &str {
        match self.export.as_str() {
            "cs" | "" => "前后端",
            "c" => "客户端",
            "s" => "服务器",
            "-" => "不导出",
            _ => "前后端",
        }
    }

    pub fn is_server_export(&self) -> bool {
        matches!(self.export.as_str(), "cs" | "" | "s")
    }
}

/// 校验 metadata id 是否合法 ([a-z0-9_-]{1,32})。
pub fn is_valid_metadata_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub fn parse_tblschema(content: &str) -> Result<TblSchema> {
    let mut sections = Vec::new();
    let mut current: Option<SchemaSection> = None;
    let mut meta = SchemaMetadata::default();
    let mut seen_section = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('#') {
            // 仅在第一个 [group/Name] 之前的 `# @meta key: value` 行视作 metadata；
            // 之后的 `#` 行一律视作普通注释。
            if !seen_section {
                if let Some((key, value)) = parse_meta_line(trimmed) {
                    match key.as_str() {
                        "id" => meta.id = value,
                        "name" => meta.name = value,
                        "category" => meta.category = value,
                        "version" => meta.version = value,
                        _ => { /* 未知 key：忽略，前向兼容 */ }
                    }
                }
            }
            continue;
        }

        if trimmed.starts_with('[') {
            seen_section = true;
            if let Some(sec) = current.take() {
                validate_section(&sec, line_num)?;
                sections.push(sec);
            }
            current = Some(parse_section_header(trimmed, line_num)?);
        } else if let Some(ref mut sec) = current {
            let field = parse_field_line(trimmed, line_num, &sec.mode)?;
            sec.fields.push(field);
        } else {
            bail!("line {}: field outside section", line_num + 1);
        }
    }

    if let Some(sec) = current {
        validate_section(&sec, content.lines().count())?;
        sections.push(sec);
    }

    // name 缺省 = id；id 缺省由调用方按文件名 stem 兜底（这里不强制）
    if meta.name.is_empty() {
        meta.name = meta.id.clone();
    }

    Ok(TblSchema { meta, sections })
}

/// 解析 `# @meta key: value` 行；返回 (key, value)。
/// 接受形式：
///   `# @meta id: full`
///   `#@meta name: 完整测试模板`
/// key 大小写敏感；后者覆盖前者由调用方处理。
fn parse_meta_line(line: &str) -> Option<(String, String)> {
    // 去掉前导 '#'，再去空白
    let body = line.trim_start_matches('#').trim();
    let after = body.strip_prefix("@meta")?.trim_start();
    let (key, value) = after.split_once(':')?;
    Some((key.trim().to_string(), value.trim().to_string()))
}

fn parse_section_header(line: &str, line_num: usize) -> Result<SchemaSection> {
    let end_bracket = line.find(']').unwrap_or(0);
    let path = &line[1..end_bracket];
    let rest = line[end_bracket + 1..].trim();

    let (group, name) = path.split_once('/')
        .ok_or_else(|| anyhow::anyhow!("line {}: section must be [group/Name]", line_num + 1))?;

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let mode_str = parts.first().copied().unwrap_or("");
    let mode = match mode_str {
        "table" => SchemaMode::Table,
        "constant" => SchemaMode::Constant,
        "enum" => SchemaMode::Enum,
        _ => bail!("line {}: mode must be 'table' / 'constant' / 'enum'", line_num + 1),
    };

    // ignore index= option (backward compat, index is always "id")
    Ok(SchemaSection {
        group: group.trim().to_string(),
        name: name.trim().to_string(),
        mode,
        fields: Vec::new(),
    })
}

fn parse_field_line(line: &str, line_num: usize, mode: &SchemaMode) -> Result<SchemaField> {
    let parts: Vec<&str> = line.split('|').collect();
    match mode {
        SchemaMode::Enum => {
            // enum 数据行：id | name | desc
            if parts.len() < 2 {
                bail!("line {}: enum row needs at least id|name", line_num + 1);
            }
            Ok(SchemaField {
                name: parts[1].trim().to_string(),
                tbl_type: parts[0].trim().to_string(), // 借用 tbl_type 字段存储 id
                export: String::new(),
                desc: parts.get(2).map(|s| s.trim().to_string()).unwrap_or_default(),
            })
        }
        _ => {
            if parts.len() < 3 {
                bail!("line {}: field needs at least name|type|export", line_num + 1);
            }
            Ok(SchemaField {
                name: parts[0].trim().to_string(),
                tbl_type: parts[1].trim().to_string(),
                export: parts[2].trim().to_string(),
                desc: parts.get(3).map(|s| s.trim().to_string()).unwrap_or_default(),
            })
        }
    }
}

fn validate_section(sec: &SchemaSection, _line_num: usize) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    for f in &sec.fields {
        if !names.insert(&f.name) {
            bail!("[{}/{}]: duplicate field '{}'", sec.group, sec.name, f.name);
        }
    }
    Ok(())
}

pub fn merge_schemas(schemas: &[TblSchema]) -> Result<TblSchema> {
    let mut all_sections = Vec::new();
    let mut keys = std::collections::HashSet::new();

    for schema in schemas {
        for sec in &schema.sections {
            let key = format!("{}/{}", sec.group, sec.name);
            if !keys.insert(key.clone()) {
                bail!("duplicate section: [{}]", key);
            }
            all_sections.push(sec.clone());
        }
    }

    // 多文件合并不挂任何 metadata（合并产物没有单一来源 id）。调用方如有需要自行赋 meta。
    Ok(TblSchema { meta: SchemaMetadata::default(), sections: all_sections })
}

pub fn serialize_tblschema(schema: &TblSchema) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "#!tblschema v1").unwrap();

    // metadata：仅非空字段输出
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

    for sec in &schema.sections {
        writeln!(s).unwrap();
        let mode = match sec.mode {
            SchemaMode::Table => "table",
            SchemaMode::Constant => "constant",
            SchemaMode::Enum => "enum",
        };
        writeln!(s, "[{}/{}] {}", sec.group, sec.name, mode).unwrap();
        match sec.mode {
            SchemaMode::Enum => {
                for f in &sec.fields {
                    // tbl_type 借位存 id
                    writeln!(s, "{} | {} | {}", f.tbl_type, f.name, f.desc).unwrap();
                }
            }
            _ => {
                for f in &sec.fields {
                    writeln!(s, "{} | {} | {} | {}", f.name, f.tbl_type, f.export, f.desc).unwrap();
                }
            }
        }
    }
    s
}

pub fn schema_from_project(groups: &[Group]) -> TblSchema {
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
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: table.name.clone(),
                mode: SchemaMode::Table,
                fields,
            });
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            let fields = constant.entries.iter()
                .filter(|e| !e.name.is_empty())
                .map(|e| SchemaField {
                    name: e.name.clone(),
                    tbl_type: e.tbl_type.clone(),
                    export: export_to_code(&e.export),
                    desc: e.desc.clone(),
                }).collect();
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: constant.name.clone(),
                mode: SchemaMode::Constant,
                fields,
            });
        }
        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            // tbl_type 借位存 id
            let fields = enum_def.entries.iter()
                .filter(|e| !e.id.is_empty() || !e.name.is_empty())
                .map(|e| SchemaField {
                    name: e.name.clone(),
                    tbl_type: e.id.clone(),
                    export: String::new(),
                    desc: e.desc.clone(),
                }).collect();
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: enum_def.name.clone(),
                mode: SchemaMode::Enum,
                fields,
            });
        }
    }
    TblSchema { meta: SchemaMetadata::default(), sections }
}

fn export_to_code(e: &Export) -> String {
    match e {
        Export::ClientServer | Export::Unselected => "cs".to_string(),
        Export::ClientOnly => "c".to_string(),
        Export::ServerOnly => "s".to_string(),
        Export::None => "-".to_string(),
    }
}

pub fn apply_schema_to_project(groups: &mut Vec<Group>, sections: &[SchemaSection], config_dir: &Path) -> (usize, usize) {
    let mut added = 0usize;
    let mut overwritten = 0usize;

    for sec in sections {
        let group = match groups.iter_mut().find(|g| g.name == sec.group) {
            Some(g) => g,
            None => {
                let dir = config_dir.join(&sec.group);
                groups.push(Group {
                    name: sec.group.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: true,
                });
                groups.last_mut().unwrap()
            }
        };

        match sec.mode {
            SchemaMode::Table => {
                let fields: Vec<FieldDef> = sec.fields.iter().map(|f| FieldDef {
                    name: f.name.clone(),
                    desc: f.desc.clone(),
                    tbl_type: f.tbl_type.clone(),
                    export: Export::from_str(&f.export),
                }).collect();

                if let Some(table) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    let old_len = table.schema.fields.len();
                    let new_len = fields.len();
                    table.schema.fields = fields;
                    for row in &mut table.records {
                        row.resize(new_len, String::new());
                        if new_len < old_len { row.truncate(new_len); }
                    }
                    table.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    group.tables.push(Table {
                        name: sec.name.clone(),
                        path,
                        schema: TableSchema { fields },
                        records: Vec::new(),
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    added += 1;
                }
            }
            SchemaMode::Constant => {
                let entries: Vec<ConstEntry> = sec.fields.iter().map(|f| ConstEntry {
                    name: f.name.clone(),
                    tbl_type: f.tbl_type.clone(),
                    value: String::new(),
                    export: Export::from_str(&f.export),
                    desc: f.desc.clone(),
                }).collect();

                if let Some(constant) = group.constants.iter_mut().find(|c| c.name == sec.name) {
                    constant.entries = entries;
                    constant.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    group.constants.push(Constant {
                        name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    added += 1;
                }
            }
            SchemaMode::Enum => {
                // tbl_type 借位存 id
                let entries: Vec<EnumEntry> = sec.fields.iter().map(|f| EnumEntry {
                    id: f.tbl_type.clone(),
                    name: f.name.clone(),
                    desc: f.desc.clone(),
                }).collect();

                if let Some(en) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    en.entries = entries;
                    en.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    group.enums.push(EnumDef {
                        name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    added += 1;
                }
            }
        }
    }

    (added, overwritten)
}

/// 解析后给缺失的 metadata 兜底：id 走文件名 stem，name 等于 id（如未填）。
/// 调用方在已知文件路径时使用：例如 `LocalTemplates` 扫目录时把 `<stem>` 灌进去。
pub fn fill_metadata_defaults(schema: &mut TblSchema, file_stem: &str) {
    if schema.meta.id.is_empty() {
        schema.meta.id = file_stem.to_string();
    }
    if schema.meta.name.is_empty() {
        schema.meta.name = schema.meta.id.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meta_lines() {
        let src = r#"#!tblschema v1
# @meta id: full
# @meta name: 完整测试模板
# @meta category: test
# @meta version: 1.0.0

[hero/HeroBase] table
id   | int | cs | 英雄ID
name | str | cs | 名称
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "full");
        assert_eq!(s.meta.name, "完整测试模板");
        assert_eq!(s.meta.category, "test");
        assert_eq!(s.meta.version, "1.0.0");
        assert_eq!(s.sections.len(), 1);
    }

    #[test]
    fn meta_lines_only_before_first_section() {
        let src = r#"#!tblschema v1
# @meta id: a

[g/N] table
id | int | cs | x
# @meta name: should-be-ignored
y  | str | cs | y
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "a");
        assert_eq!(s.meta.name, "a", "name 缺省 = id，第二段后的 @meta 应被当注释忽略");
    }

    #[test]
    fn meta_later_key_wins() {
        let src = r#"#!tblschema v1
# @meta id: a
# @meta id: b

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "b");
    }

    #[test]
    fn meta_unknown_key_ignored() {
        let src = r#"#!tblschema v1
# @meta id: a
# @meta future_field: 42

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "a");
    }

    #[test]
    fn legacy_no_meta_parses() {
        let src = r#"#!tblschema v1

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "");
        assert_eq!(s.meta.name, "");
        assert_eq!(s.sections.len(), 1);
    }

    #[test]
    fn fill_metadata_defaults_uses_stem() {
        let mut s = TblSchema::default();
        fill_metadata_defaults(&mut s, "myfile");
        assert_eq!(s.meta.id, "myfile");
        assert_eq!(s.meta.name, "myfile");

        // id 已存在 → 不覆盖；name 仍按 id 兜底
        let mut s2 = TblSchema::default();
        s2.meta.id = "explicit".to_string();
        fill_metadata_defaults(&mut s2, "stem");
        assert_eq!(s2.meta.id, "explicit");
        assert_eq!(s2.meta.name, "explicit");
    }

    #[test]
    fn id_validation() {
        assert!(is_valid_metadata_id("full"));
        assert!(is_valid_metadata_id("slg-base"));
        assert!(is_valid_metadata_id("slg_base_2"));
        assert!(is_valid_metadata_id("a"));
        assert!(is_valid_metadata_id(&"a".repeat(32)));

        assert!(!is_valid_metadata_id(""));
        assert!(!is_valid_metadata_id(&"a".repeat(33)));
        assert!(!is_valid_metadata_id("Full"));            // 不接受大写
        assert!(!is_valid_metadata_id("foo bar"));          // 空格
        assert!(!is_valid_metadata_id("foo.bar"));          // 点
        assert!(!is_valid_metadata_id("中文"));
    }

    #[test]
    fn serialize_round_trips_meta() {
        let mut s = TblSchema::default();
        s.meta.id = "demo".to_string();
        s.meta.name = "演示".to_string();
        s.meta.category = "test".to_string();
        s.meta.version = "0.1.0".to_string();
        s.sections.push(SchemaSection {
            group: "g".to_string(),
            name: "N".to_string(),
            mode: SchemaMode::Table,
            fields: vec![SchemaField {
                name: "id".to_string(),
                tbl_type: "int".to_string(),
                export: "cs".to_string(),
                desc: "x".to_string(),
            }],
        });
        let txt = serialize_tblschema(&s);
        assert!(txt.contains("# @meta id: demo"));
        assert!(txt.contains("# @meta name: 演示"));
        assert!(txt.contains("# @meta category: test"));
        assert!(txt.contains("# @meta version: 0.1.0"));

        let back = parse_tblschema(&txt).expect("re-parse");
        assert_eq!(back.meta.id, "demo");
        assert_eq!(back.meta.name, "演示");
        assert_eq!(back.meta.category, "test");
        assert_eq!(back.meta.version, "0.1.0");
        assert_eq!(back.sections.len(), 1);
    }
}
