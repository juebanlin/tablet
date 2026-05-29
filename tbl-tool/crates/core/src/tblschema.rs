use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;

#[derive(Debug, Clone)]
pub struct TblSchema {
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

pub fn parse_tblschema(content: &str) -> Result<TblSchema> {
    let mut sections = Vec::new();
    let mut current: Option<SchemaSection> = None;

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            if let Some(sec) = current.take() {
                validate_section(&sec, line_num)?;
                sections.push(sec);
            }
            current = Some(parse_section_header(line, line_num)?);
        } else if let Some(ref mut sec) = current {
            let field = parse_field_line(line, line_num, &sec.mode)?;
            sec.fields.push(field);
        } else {
            bail!("line {}: field outside section", line_num + 1);
        }
    }

    if let Some(sec) = current {
        validate_section(&sec, content.lines().count())?;
        sections.push(sec);
    }

    Ok(TblSchema { sections })
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

    Ok(TblSchema { sections: all_sections })
}

pub fn serialize_tblschema(schema: &TblSchema) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "#!tblschema v1").unwrap();

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
    TblSchema { sections }
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
