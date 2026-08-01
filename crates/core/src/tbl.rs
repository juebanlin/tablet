use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;
use crate::tbl_str::{encode, decode, classify, split_row, FieldKind};

pub fn parse_tbl(path: &Path) -> Result<TblFile> {
    let content = std::fs::read_to_string(path)?;
    let mut lines = content.lines();

    let first = lines.next().unwrap_or("");
    if !first.starts_with("#!tbl") {
        bail!("not a valid .tbl file: missing #!tbl header");
    }

    let mut mode = String::new();
    let mut desc_line = String::new();
    let mut type_line = String::new();
    let mut export_line = String::new();
    let mut field_line = String::new();
    let mut data_lines: Vec<&str> = Vec::new();
    let mut in_data = false;

    for line in lines {
        if in_data {
            if !line.is_empty() {
                data_lines.push(line);
            }
            continue;
        }
        if line == "---" {
            in_data = true;
            continue;
        }
        if let Some(val) = line.strip_prefix("#mode ") {
            mode = val.trim().to_string();
        } else if line.starts_with("#index ") {
            // ignored: index is always "id"
        } else if let Some(val) = line.strip_prefix("#desc ") {
            desc_line = val.to_string();
        } else if let Some(val) = line.strip_prefix("#type ") {
            type_line = val.to_string();
        } else if let Some(val) = line.strip_prefix("#export ") {
            export_line = val.to_string();
        } else if let Some(val) = line.strip_prefix("#field ") {
            field_line = val.to_string();
        }
    }

    match mode.as_str() {
        "table" => parse_table(path, &desc_line, &type_line, &export_line, &field_line, &data_lines),
        "constant" => parse_constant(path, &data_lines),
        "enum" => parse_enum(path, &data_lines),
        _ => bail!("unknown mode: {}", mode),
    }
}

fn parse_table(
    path: &Path,
    desc_line: &str,
    type_line: &str,
    export_line: &str,
    field_line: &str,
    data_lines: &[&str],
) -> Result<TblFile> {
    // 表头行的字段名/类型/export 不允许含 `|` 或换行（结构性字段），使用简单 split。
    // desc 允许含特殊字符（人类描述），走 Str 类型解码。
    let descs: Vec<String> = split_row(desc_line).iter().map(|s| decode(s, FieldKind::Text)).collect();
    let types: Vec<&str> = type_line.split('|').collect();
    let exports: Vec<&str> = export_line.split('|').collect();
    let fields: Vec<&str> = field_line.split('|').collect();

    let field_count = fields.len();
    let mut field_defs = Vec::with_capacity(field_count);

    for i in 0..field_count {
        field_defs.push(FieldDef {
            name: fields[i].trim().to_string(),
            desc: descs.get(i).map(|s| s.trim().to_string()).unwrap_or_default(),
            tbl_type: types.get(i).unwrap_or(&"str").trim().to_string(),
            export: Export::from_str(exports.get(i).unwrap_or(&"")),
        });
    }

    // 按列类型逐列 decode
    let kinds: Vec<FieldKind> = field_defs.iter().map(|f| classify(&f.tbl_type)).collect();
    let records: Vec<Vec<String>> = data_lines
        .iter()
        .map(|line| {
            let raw = split_row(line);
            raw.iter().enumerate().map(|(i, s)| {
                let kind = kinds.get(i).copied().unwrap_or(FieldKind::Text);
                decode(s, kind)
            }).collect()
        })
        .collect();

    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();

    let table = Table {
        name,
        path: path.to_path_buf(),
        schema: TableSchema { fields: field_defs.clone() },
        original_records: records.clone(),
        original_fields: field_defs,
        records,
        dirty: false,
        deleted: false,
        saved: true,
    };
    Ok(TblFile::Table(table))
}

fn parse_constant(path: &Path, data_lines: &[&str]) -> Result<TblFile> {
    let mut entries = Vec::new();

    for line in data_lines {
        let raw = split_row(line);
        if raw.len() < 3 {
            continue;
        }
        // 列语义：name(Atom)|type(Atom)|value(按 type 分类)|export(Atom)|desc(Str)
        let name = decode(&raw[0], FieldKind::Atom);
        let tbl_type = decode(&raw[1], FieldKind::Atom);
        let value = decode(&raw[2], classify(&tbl_type));
        let export_raw = raw.get(3).map(|s| decode(s, FieldKind::Atom)).unwrap_or_default();
        let desc = raw.get(4).map(|s| decode(s, FieldKind::Text)).unwrap_or_default();
        entries.push(ConstEntry {
            name: name.trim().to_string(),
            tbl_type: tbl_type.trim().to_string(),
            value: value.trim().to_string(),
            export: Export::from_str(&export_raw),
            desc: desc.trim().to_string(),
        });
    }

    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();

    let constant = Constant {
        name,
        path: path.to_path_buf(),
        original_entries: entries.clone(),
        entries,
        dirty: false,
        deleted: false,
        saved: true,
    };
    Ok(TblFile::Constant(constant))
}

fn parse_enum(path: &Path, data_lines: &[&str]) -> Result<TblFile> {
    let mut entries = Vec::new();

    for line in data_lines {
        let raw = split_row(line);
        if raw.is_empty() {
            continue;
        }
        // 列语义：id(Atom, 数字标识)|name(Atom, 标识符)|desc(Str)
        entries.push(EnumEntry {
            id: decode(&raw[0], FieldKind::Atom).trim().to_string(),
            name: raw.get(1).map(|s| decode(s, FieldKind::Atom).trim().to_string()).unwrap_or_default(),
            desc: raw.get(2).map(|s| decode(s, FieldKind::Text).trim().to_string()).unwrap_or_default(),
        });
    }

    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();

    let enum_def = EnumDef {
        name,
        path: path.to_path_buf(),
        original_entries: entries.clone(),
        entries,
        dirty: false,
        deleted: false,
        saved: true,
    };
    Ok(TblFile::Enum(enum_def))
}

pub enum TblFile {
    Table(Table),
    Constant(Constant),
    Enum(EnumDef),
}

pub fn serialize_table(table: &Table) -> String {
    let fields = &table.schema.fields;
    let mut s = String::new();
    s.push_str("#!tbl v2\n");
    s.push_str("#mode table\n");
    s.push_str(&format!("#desc {}\n", fields.iter().map(|f| encode(&f.desc, FieldKind::Text)).collect::<Vec<_>>().join("|")));
    s.push_str(&format!("#export {}\n", fields.iter().map(|f| f.export.to_tbl().to_string()).collect::<Vec<_>>().join("|")));
    s.push_str(&format!("#type {}\n", fields.iter().map(|f| f.tbl_type.as_str()).collect::<Vec<_>>().join("|")));
    s.push_str(&format!("#field {}\n", fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join("|")));
    s.push_str("---\n");
    let kinds: Vec<FieldKind> = fields.iter().map(|f| classify(&f.tbl_type)).collect();
    for row in &table.records {
        let encoded: Vec<String> = row.iter().enumerate()
            .map(|(i, c)| encode(c, kinds.get(i).copied().unwrap_or(FieldKind::Text)))
            .collect();
        s.push_str(&encoded.join("|"));
        s.push('\n');
    }
    s
}

pub fn serialize_constant(constant: &Constant) -> String {
    let mut s = String::new();
    s.push_str("#!tbl v2\n");
    s.push_str("#mode constant\n");
    s.push_str("---\n");
    for e in &constant.entries {
        // 列语义：name(Atom)|type(Atom)|value(按 type 分类)|export(Atom)|desc(Str)
        let value_kind = classify(&e.tbl_type);
        s.push_str(&format!("{}|{}|{}|{}|{}\n",
            encode(&e.name, FieldKind::Atom),
            e.tbl_type,
            encode(&e.value, value_kind),
            e.export.to_tbl(),
            encode(&e.desc, FieldKind::Text),
        ));
    }
    s
}

pub fn serialize_enum(enum_def: &EnumDef) -> String {
    let mut s = String::new();
    s.push_str("#!tbl v2\n");
    s.push_str("#mode enum\n");
    s.push_str("---\n");
    for e in &enum_def.entries {
        // 列语义：id(Atom)|name(Atom, 标识符)|desc(Str)
        s.push_str(&format!("{}|{}|{}\n",
            encode(&e.id, FieldKind::Atom),
            encode(&e.name, FieldKind::Atom),
            encode(&e.desc, FieldKind::Text),
        ));
    }
    s
}
