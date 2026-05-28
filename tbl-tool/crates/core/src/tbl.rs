use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;

pub fn parse_tbl(path: &Path) -> Result<TblFile> {
    let content = std::fs::read_to_string(path)?;
    let mut lines = content.lines();

    let first = lines.next().unwrap_or("");
    if !first.starts_with("#!tbl") {
        bail!("not a valid .tbl file: missing #!tbl header");
    }

    let mut mode = String::new();
    let mut index = String::new();
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
        } else if let Some(val) = line.strip_prefix("#index ") {
            index = val.trim().to_string();
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
        "table" => parse_table(path, &index, &desc_line, &type_line, &export_line, &field_line, &data_lines),
        "constant" => parse_constant(path, &data_lines),
        _ => bail!("unknown mode: {}", mode),
    }
}

fn parse_table(
    path: &Path,
    index: &str,
    desc_line: &str,
    type_line: &str,
    export_line: &str,
    field_line: &str,
    data_lines: &[&str],
) -> Result<TblFile> {
    let descs: Vec<&str> = desc_line.split('|').collect();
    let types: Vec<&str> = type_line.split('|').collect();
    let exports: Vec<&str> = export_line.split('|').collect();
    let fields: Vec<&str> = field_line.split('|').collect();

    let field_count = fields.len();
    let mut field_defs = Vec::with_capacity(field_count);

    for i in 0..field_count {
        field_defs.push(FieldDef {
            name: fields[i].trim().to_string(),
            desc: descs.get(i).unwrap_or(&"").trim().to_string(),
            tbl_type: types.get(i).unwrap_or(&"str").trim().to_string(),
            export: Export::from_str(exports.get(i).unwrap_or(&"")),
        });
    }

    let records: Vec<Vec<String>> = data_lines
        .iter()
        .map(|line| line.split('|').map(|s| s.to_string()).collect())
        .collect();

    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();

    let mut table = Table {
        name,
        path: path.to_path_buf(),
        schema: TableSchema {
            fields: field_defs,
            index: index.to_string(),
        },
        records,
        dirty: false,
        deleted: false,
        original: String::new(),
    };
    table.original = serialize_table(&table);
    Ok(TblFile::Table(table))
}

fn parse_constant(path: &Path, data_lines: &[&str]) -> Result<TblFile> {
    let mut entries = Vec::new();

    for line in data_lines {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 3 {
            continue;
        }
        entries.push(ConstEntry {
            name: parts[0].trim().to_string(),
            tbl_type: parts[1].trim().to_string(),
            value: parts[2].trim().to_string(),
            export: Export::from_str(parts.get(3).unwrap_or(&"")),
            desc: parts.get(4).unwrap_or(&"").trim().to_string(),
        });
    }

    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();

    let mut constant = Constant {
        name,
        path: path.to_path_buf(),
        entries,
        dirty: false,
        deleted: false,
        original: String::new(),
    };
    constant.original = serialize_constant(&constant);
    Ok(TblFile::Constant(constant))
}

pub enum TblFile {
    Table(Table),
    Constant(Constant),
}

pub fn serialize_table(table: &Table) -> String {
    let fields = &table.schema.fields;
    let mut s = String::new();
    s.push_str("#!tbl v2\n");
    s.push_str("#mode table\n");
    s.push_str(&format!("#index {}\n", table.schema.index));
    s.push_str(&format!("#desc {}\n", fields.iter().map(|f| f.desc.as_str()).collect::<Vec<_>>().join("|")));
    s.push_str(&format!("#type {}\n", fields.iter().map(|f| f.tbl_type.as_str()).collect::<Vec<_>>().join("|")));
    s.push_str(&format!("#export {}\n", fields.iter().map(|f| f.export.to_tbl()).collect::<Vec<_>>().join("|")));
    s.push_str(&format!("#field {}\n", fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join("|")));
    s.push_str("---\n");
    for row in &table.records {
        s.push_str(&row.join("|"));
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
        s.push_str(&format!("{}|{}|{}|{}|{}\n", e.name, e.tbl_type, e.value, e.export.to_tbl(), e.desc));
    }
    s
}
