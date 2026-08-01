use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;
use crate::tbl_str::{encode, decode, classify, split_row, FieldKind};

const INDENT: &str = "  ";

/// 文件格式版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TblVersion { V2, V3 }

impl TblVersion {
    pub fn from_header(line: &str) -> Option<Self> {
        match line.trim() {
            "#!tbl v2" => Some(Self::V2),
            "#!tbl v3" => Some(Self::V3),
            _ => None,
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            TblVersion::V2 => "#!tbl v2",
            TblVersion::V3 => "#!tbl v3",
        }
    }
}

/// 新建表的默认格式版本。
pub const DEFAULT_TBL_VERSION: TblVersion = TblVersion::V3;

/// Table 格式编解码器：v2（CSV 行列）和 v3（逐字段 key:value）。
/// 业务层通过 Codec enum 选择具体实现。
pub trait TableCodec {
    fn parse(
        &self,
        path: &Path,
        desc_line: &str, type_line: &str, export_line: &str, field_line: &str,
        data_lines: &[&str],
    ) -> Result<Table>;

    fn serialize(&self, table: &Table) -> String;
}

// ===== V2 codec =====

pub struct V2Codec;

impl TableCodec for V2Codec {
    fn parse(
        &self, path: &Path,
        desc_line: &str, type_line: &str, export_line: &str, field_line: &str,
        data_lines: &[&str],
    ) -> Result<Table> {
        let descs: Vec<String> = split_row(desc_line).iter()
            .map(|s| decode(s, FieldKind::Text)).collect();
        let types: Vec<&str> = type_line.split('|').collect();
        let exports: Vec<&str> = export_line.split('|').collect();
        let field_names: Vec<&str> = field_line.split('|').collect();

        let n = field_names.len();
        let mut field_defs = Vec::with_capacity(n);
        for i in 0..n {
            field_defs.push(FieldDef {
                name: field_names[i].trim().to_string(),
                desc: descs.get(i).map(|s| s.trim().to_string()).unwrap_or_default(),
                tbl_type: types.get(i).unwrap_or(&"str").trim().to_string(),
                export: Export::from_str(exports.get(i).unwrap_or(&"")),
            });
        }

        let kinds: Vec<FieldKind> = field_defs.iter().map(|f| classify(&f.tbl_type)).collect();
        let records: Vec<Vec<String>> = data_lines.iter()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let raw = split_row(line);
                raw.iter().enumerate().map(|(i, s)| {
                    let kind = kinds.get(i).copied().unwrap_or(FieldKind::Text);
                    decode(s, kind)
                }).collect()
            }).collect();

        let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        Ok(Table {
            name, path: path.to_path_buf(),
            schema: TableSchema { fields: field_defs.clone() },
            original_records: records.clone(), original_fields: field_defs,
            records, dirty: false, deleted: false, saved: true, fmt_ver: TblVersion::V2,
        })
    }

    fn serialize(&self, table: &Table) -> String {
        let fields = &table.schema.fields;
        let mut s = String::new();
        s.push_str("#!tbl v2\n");
        s.push_str("#mode table\n");
        s.push_str(&format!("#desc {}\n", fields.iter()
            .map(|f| encode(&f.desc, FieldKind::Text)).collect::<Vec<_>>().join("|")));
        s.push_str(&format!("#export {}\n", fields.iter()
            .map(|f| f.export.to_tbl().to_string()).collect::<Vec<_>>().join("|")));
        s.push_str(&format!("#type {}\n", fields.iter()
            .map(|f| f.tbl_type.as_str()).collect::<Vec<_>>().join("|")));
        s.push_str(&format!("#field {}\n", fields.iter()
            .map(|f| f.name.as_str()).collect::<Vec<_>>().join("|")));
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
}

// ===== V3 codec =====

pub struct V3Codec;

impl TableCodec for V3Codec {
    fn parse(
        &self, path: &Path,
        _desc_line: &str, _type_line: &str, _export_line: &str, _field_line: &str,
        data_lines: &[&str],
    ) -> Result<Table> {
        // v3 puts field definitions in @field blocks, records in [id] blocks,
        // both after ---. Ignore the 4 legacy header lines.

        // first pass: collect @field blocks to build schema
        let mut field_defs: Vec<FieldDef> = Vec::new();
        let mut records: Vec<Vec<String>> = Vec::new();
        let mut cur_field: Option<FieldDef> = None;
        let mut cur_record: Option<Vec<String>> = None;

        for line in data_lines {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            if trimmed.starts_with("#@color:") { continue; }

            // @field block start
            if let Some(fname) = trimmed.strip_prefix("@field ") {
                if let Some(f) = cur_field.take() { field_defs.push(f); }
                if let Some(r) = cur_record.take() { records.push(r); }
                cur_field = Some(FieldDef {
                    name: fname.trim().to_string(),
                    desc: String::new(),
                    tbl_type: String::from("str"),
                    export: Export::ClientServer,
                });
                continue;
            }

            // [id] record block start
            if let Some(id_val) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                if let Some(f) = cur_field.take() { field_defs.push(f); }
                if let Some(r) = cur_record.take() { records.push(r); }
                cur_record = Some(vec![id_val.to_string()]);
                continue;
            }

            // indented field property / record value
            let unindented = line.strip_prefix(INDENT).unwrap_or(line);
            if let Some((key, value)) = unindented.split_once(':') {
                let key = key.trim();
                let val = value.trim();
                if let Some(ref mut f) = cur_field {
                    match key {
                        "desc" => f.desc = val.to_string(),
                        "export" => f.export = Export::from_str(val),
                        "type" => f.tbl_type = val.to_string(),
                        _ => {}
                    }
                } else if let Some(ref mut r) = cur_record {
                    // record field: key=field name, value=cell
                    if let Some(idx) = field_defs.iter().position(|fd| fd.name == key) {
                        let kind = classify(&field_defs[idx].tbl_type);
                        while r.len() <= idx { r.push(String::new()); }
                        r[idx] = decode(val, kind);
                    }
                }
            }
        }

        if let Some(f) = cur_field { field_defs.push(f); }
        if let Some(r) = cur_record { records.push(r); }

        // id column defaults: if user didn't define @field id, prepend it
        if field_defs.first().map_or(true, |f| f.name != "id") {
            field_defs.insert(0, FieldDef {
                name: "id".into(), desc: "ID".into(),
                tbl_type: "int".into(), export: Export::ClientServer,
            });
        }

        // normalize records to field_defs width
        let n = field_defs.len();
        for r in &mut records {
            r.resize(n, String::new());
        }

        let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        Ok(Table {
            name, path: path.to_path_buf(),
            schema: TableSchema { fields: field_defs.clone() },
            original_records: records.clone(), original_fields: field_defs,
            records, dirty: false, deleted: false, saved: true, fmt_ver: TblVersion::V3,
        })
    }

    fn serialize(&self, table: &Table) -> String {
        let fields = &table.schema.fields;
        let mut s = String::new();
        s.push_str("#!tbl v3\n");
        s.push_str("#mode table\n");
        s.push_str("---\n");

        // @field blocks
        for f in fields {
            s.push('\n');
            s.push_str(&format!("@field {}\n", f.name));
            if !f.desc.is_empty() {
                s.push_str(&format!("  desc:{}\n", encode(&f.desc, FieldKind::Text)));
            }
            s.push_str(&format!("  export:{}\n", f.export.to_tbl()));
            s.push_str(&format!("  type:{}\n", f.tbl_type));
        }

        // [id] records
        let kinds: Vec<FieldKind> = fields.iter().map(|f| classify(&f.tbl_type)).collect();
        for row in &table.records {
            let id_val = row.first().map(String::as_str).unwrap_or("");
            s.push('\n');
            s.push_str(&format!("[{}]\n", id_val));
            for (i, cell) in row.iter().enumerate().skip(1) {
                if cell.is_empty() { continue; }
                let kind = kinds.get(i).copied().unwrap_or(FieldKind::Text);
                s.push_str(&format!("  {}:{}\n", fields[i].name, encode(cell, kind)));
            }
        }
        s
    }
}

// ===== Codec dispatch enum =====

const V2: V2Codec = V2Codec;
const V3: V3Codec = V3Codec;

pub enum Codec { V2, V3 }

impl Codec {
    pub fn from_version(v: TblVersion) -> Self {
        match v { TblVersion::V2 => Codec::V2, TblVersion::V3 => Codec::V3 }
    }

    fn detect(first_line: &str) -> Result<Self> {
        TblVersion::from_header(first_line)
            .map(Self::from_version)
            .ok_or_else(|| anyhow::anyhow!("unsupported tbl version: '{}', expected #!tbl v2 or v3", first_line))
    }

    fn parse_table(
        &self, path: &Path,
        desc_line: &str, type_line: &str, export_line: &str, field_line: &str,
        data_lines: &[&str],
    ) -> Result<Table> {
        match self {
            Codec::V2 => V2.parse(path, desc_line, type_line, export_line, field_line, data_lines),
            Codec::V3 => V3.parse(path, desc_line, type_line, export_line, field_line, data_lines),
        }
    }

    pub fn serialize_table(&self, table: &Table) -> String {
        match self {
            Codec::V2 => V2.serialize(table),
            Codec::V3 => V3.serialize(table),
        }
    }
}

// ===== Top-level parse / serialize =====

pub fn parse_tbl(path: &Path) -> Result<TblFile> {
    let content = std::fs::read_to_string(path)?;
    let mut lines = content.lines();

    let first = lines.next().unwrap_or("");
    if !first.starts_with("#!tbl") {
        bail!("not a valid .tbl file: missing #!tbl header");
    }
    let codec = Codec::detect(first)?;

    let mut mode = String::new();
    let mut desc_line = String::new();
    let mut type_line = String::new();
    let mut export_line = String::new();
    let mut field_line = String::new();
    let mut data_lines: Vec<&str> = Vec::new();
    let mut in_data = false;

    for line in lines {
        if in_data { data_lines.push(line); continue; }
        if line == "---" { in_data = true; continue; }
        if let Some(val) = line.strip_prefix("#mode ") { mode = val.trim().to_string(); }
        else if let Some(val) = line.strip_prefix("#desc ") { desc_line = val.to_string(); }
        else if let Some(val) = line.strip_prefix("#type ") { type_line = val.to_string(); }
        else if let Some(val) = line.strip_prefix("#export ") { export_line = val.to_string(); }
        else if let Some(val) = line.strip_prefix("#field ") { field_line = val.to_string(); }
    }

    match mode.as_str() {
        "table" => codec.parse_table(path, &desc_line, &type_line, &export_line, &field_line, &data_lines)
            .map(TblFile::Table),
        "constant" => parse_constant(path, &data_lines),
        "enum" => parse_enum(path, &data_lines),
        _ => bail!("unknown mode: {}", mode),
    }
}

pub fn serialize_table(table: &Table) -> String {
    Codec::from_version(table.fmt_ver).serialize_table(table)
}

// ===== Constant & enum (v2 only) =====

fn parse_constant(path: &Path, data_lines: &[&str]) -> Result<TblFile> {
    let mut entries = Vec::new();
    for line in data_lines {
        let raw = split_row(line);
        if raw.len() < 3 { continue; }
        let name = decode(&raw[0], FieldKind::Atom);
        let tbl_type = decode(&raw[1], FieldKind::Atom);
        let value = decode(&raw[2], classify(&tbl_type));
        let export_raw = raw.get(3).map(|s| decode(s, FieldKind::Atom)).unwrap_or_default();
        let desc = raw.get(4).map(|s| decode(s, FieldKind::Text)).unwrap_or_default();
        entries.push(ConstEntry {
            name: name.trim().to_string(), tbl_type: tbl_type.trim().to_string(),
            value: value.trim().to_string(), export: Export::from_str(&export_raw),
            desc: desc.trim().to_string(),
        });
    }
    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    Ok(TblFile::Constant(Constant {
        name, path: path.to_path_buf(),
        original_entries: entries.clone(), entries,
        dirty: false, deleted: false, saved: true,
    }))
}

fn parse_enum(path: &Path, data_lines: &[&str]) -> Result<TblFile> {
    let mut entries = Vec::new();
    for line in data_lines {
        let raw = split_row(line);
        if raw.is_empty() { continue; }
        entries.push(EnumEntry {
            id: decode(&raw[0], FieldKind::Atom).trim().to_string(),
            name: raw.get(1).map(|s| decode(s, FieldKind::Atom).trim().to_string()).unwrap_or_default(),
            desc: raw.get(2).map(|s| decode(s, FieldKind::Text).trim().to_string()).unwrap_or_default(),
        });
    }
    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    Ok(TblFile::Enum(EnumDef {
        name, path: path.to_path_buf(),
        original_entries: entries.clone(), entries,
        dirty: false, deleted: false, saved: true,
    }))
}

pub enum TblFile {
    Table(Table),
    Constant(Constant),
    Enum(EnumDef),
}

pub fn serialize_constant(constant: &Constant) -> String {
    let mut s = String::new();
    s.push_str("#!tbl v2\n");
    s.push_str("#mode constant\n");
    s.push_str("---\n");
    for e in &constant.entries {
        let value_kind = classify(&e.tbl_type);
        s.push_str(&format!("{}|{}|{}|{}|{}\n",
            encode(&e.name, FieldKind::Atom), e.tbl_type,
            encode(&e.value, value_kind), e.export.to_tbl(),
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
        s.push_str(&format!("{}|{}|{}\n",
            encode(&e.id, FieldKind::Atom),
            encode(&e.name, FieldKind::Atom),
            encode(&e.desc, FieldKind::Text),
        ));
    }
    s
}
