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

    Ok(TblFile::Table(Table {
        name,
        path: path.to_path_buf(),
        schema: TableSchema {
            fields: field_defs,
            index: index.to_string(),
        },
        records,
    }))
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

    Ok(TblFile::Constant(Constant {
        name,
        path: path.to_path_buf(),
        entries,
    }))
}

pub enum TblFile {
    Table(Table),
    Constant(Constant),
}
