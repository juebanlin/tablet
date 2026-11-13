//! 分表二进制导出（v1）：给 Unity/小游戏等对启动速度敏感的运行时用。
//!
//! 源格式仍是 `.tbl` / `.tblschema`；这里只生成派生数据文件，不进 Git。
//!
//! 整个项目导出一个 `config.bytes`（bundle）：
//! ```text
//! magic "TBLB" | version u16 | kind u16=2 | node_count u16
//! 每个节点: name(u16+utf8) | offset u64 | len u64
//! 节点数据区（每个节点是下面这种 table payload）...
//! ```
//! 单节点 payload 布局（little-endian）：
//! ```text
//! magic "TBLB" | version u16 | kind u16 | col_count u16 | row_count u32
//! schema_hash u64 | sep_count u16 | sep 键值对...
//! 每列: name(u16+utf8) | type(u16+utf8) | tag u8 | offset u64 | len u64
//! 数据区（按列连续存放）：
//!   int32/int64/float32/float64: 定长数组
//!   bool: 每行 1 字节
//!   string/composite: (row_count+1) 个 u32 偏移 + UTF-8 blob
//! ```
//!
//! v1 的复合类型（Tuple / List / Set / Map / 嵌套）仍按原文本编码为字符串，
//! 由读取端用生成代码里的 `TblParse` 解析；标量列已经是零拷贝的定长数组。
//! 常量按 5 列字符串（name/type/value/export/desc）编码；枚举没有数据文件。

use std::path::Path;

use anyhow::Result;

use crate::model::{Constant, Project, Table};
use crate::types::{BaseType, Paradigm, SepKey, SeparatorsSection, TblType};

use super::{DataFilter, ExportFile, ExportResult, FileStatus};

pub const MAGIC: &[u8; 4] = b"TBLB";
pub const VERSION: u16 = 1;
pub const KIND_TABLE: u16 = 0;
pub const KIND_CONSTANT: u16 = 1;
pub const KIND_BUNDLE: u16 = 2;
/// 一个项目一个二进制包（Unity 的 TextAsset 用 .bytes 扩展名）。
pub const BUNDLE_FILE: &str = "config.bytes";

/// 列数据类型标签。读取端必须与这里保持一致。
pub const TAG_I32: u8 = 0;
pub const TAG_I64: u8 = 1;
pub const TAG_F32: u8 = 2;
pub const TAG_F64: u8 = 3;
pub const TAG_BOOL: u8 = 4;
pub const TAG_STR: u8 = 5;
/// 复合类型：v1 存原文本（分号/逗号等由文件头里的分隔符解释）。
pub const TAG_RAW: u8 = 6;

/// FNV-1a 64：只用于“schema 是否变了”的快速校验，不作为安全哈希。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_str(out: &mut Vec<u8>, s: &str) {
    push_u16(out, s.len().min(u16::MAX as usize) as u16);
    out.extend_from_slice(&s.as_bytes()[..s.len().min(u16::MAX as usize)]);
}

/// 列类型标签：标量走定长数组，其余（含所有复合类型）v1 存原文本。
pub fn tag_of(tbl_type: Option<&TblType>) -> u8 {
    let Some(t) = tbl_type else { return TAG_STR };
    match t.paradigm {
        Paradigm::Base => match t.params.first() {
            Some(crate::types::TypeParam::Base(BaseType::Int)) => TAG_I32,
            Some(crate::types::TypeParam::Base(BaseType::Long | BaseType::Date)) => TAG_I64,
            Some(crate::types::TypeParam::Base(BaseType::Float)) => TAG_F32,
            Some(crate::types::TypeParam::Base(BaseType::Double)) => TAG_F64,
            Some(crate::types::TypeParam::Base(BaseType::Bool)) => TAG_BOOL,
            Some(crate::types::TypeParam::Base(BaseType::Str | BaseType::Txt)) | None => TAG_STR,
            Some(crate::types::TypeParam::Ref(_)) => TAG_I32,
        },
        // 顶层引用在数据层就是 int id
        Paradigm::Ref => TAG_I32,
        _ => TAG_RAW,
    }
}

fn parse_i32(s: &str) -> i32 {
    s.trim().parse::<i32>().unwrap_or(0)
}
fn parse_i64(s: &str) -> i64 {
    s.trim().parse::<i64>().unwrap_or(0)
}
fn parse_f32(s: &str) -> f32 {
    s.trim().parse::<f32>().unwrap_or(0.0)
}
fn parse_f64(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}
fn parse_bool(s: &str) -> bool {
    matches!(s.trim(), "true" | "1")
}

struct ColSpec {
    name: String,
    type_str: String,
    tag: u8,
    values: Vec<String>,
}

impl ColSpec {
    fn block(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self.tag {
            TAG_I32 => {
                for v in &self.values {
                    out.extend_from_slice(&parse_i32(v).to_le_bytes());
                }
            }
            TAG_I64 => {
                for v in &self.values {
                    out.extend_from_slice(&parse_i64(v).to_le_bytes());
                }
            }
            TAG_F32 => {
                for v in &self.values {
                    out.extend_from_slice(&parse_f32(v).to_le_bytes());
                }
            }
            TAG_F64 => {
                for v in &self.values {
                    out.extend_from_slice(&parse_f64(v).to_le_bytes());
                }
            }
            TAG_BOOL => {
                for v in &self.values {
                    out.push(if parse_bool(v) { 1 } else { 0 });
                }
            }
            _ => {
                // 字符串 / 复合：偏移表 + blob
                let mut offsets: Vec<u32> = Vec::with_capacity(self.values.len() + 1);
                let mut blob: Vec<u8> = Vec::new();
                offsets.push(0);
                for v in &self.values {
                    blob.extend_from_slice(v.as_bytes());
                    offsets.push(blob.len() as u32);
                }
                for off in offsets {
                    out.extend_from_slice(&off.to_le_bytes());
                }
                out.extend_from_slice(&blob);
            }
        }
        out
    }
}

/// 计算一个 Table 的 schema 指纹：节点名 + 列名 + 列类型 + 列标签。
pub fn schema_hash_table(table: &Table) -> u64 {
    let mut buf = Vec::new();
    push_str(&mut buf, &table.name);
    push_u16(&mut buf, KIND_TABLE);
    for f in &table.schema.fields {
        if f.is_aux() { continue; }
        push_str(&mut buf, &f.name);
        push_str(&mut buf, &f.tbl_type);
        buf.push(tag_of(TblType::parse(&f.tbl_type).as_ref()));
    }
    fnv1a64(&buf)
}

fn sep_pairs(sep: &SeparatorsSection) -> Vec<(String, String)> {
    SepKey::ALL
        .iter()
        .map(|k| (k.as_directive_key().to_string(), k.get(sep).to_string()))
        .collect()
}

/// 编码一张 Table（单节点 payload）。
pub fn encode_table(table: &Table, sep: &SeparatorsSection) -> Vec<u8> {
    let cols: Vec<ColSpec> = table
        .schema
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_aux())
        .map(|(ci, f)| ColSpec {
            name: f.name.clone(),
            type_str: f.tbl_type.clone(),
            tag: tag_of(TblType::parse(&f.tbl_type).as_ref()),
            values: table
                .records
                .iter()
                .map(|r| r.get(ci).cloned().unwrap_or_default())
                .collect(),
        })
        .collect();
    encode_payload(
        &table.name,
        KIND_TABLE,
        schema_hash_table(table),
        &cols,
        &sep_pairs(sep),
        table.records.len() as u32,
    )
}

fn encode_payload(
    _node: &str,
    kind: u16,
    schema_hash: u64,
    cols: &[ColSpec],
    sep_pairs: &[(String, String)],
    row_count: u32,
) -> Vec<u8> {
    let header = |offsets: &[(u64, u64)]| -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(MAGIC);
        push_u16(&mut h, VERSION);
        push_u16(&mut h, kind);
        push_u16(&mut h, cols.len() as u16);
        push_u32(&mut h, row_count);
        push_u64(&mut h, schema_hash);
        push_u16(&mut h, sep_pairs.len() as u16);
        for (k, v) in sep_pairs {
            push_str(&mut h, k);
            push_str(&mut h, v);
        }
        for (i, c) in cols.iter().enumerate() {
            push_str(&mut h, &c.name);
            push_str(&mut h, &c.type_str);
            h.push(c.tag);
            push_u64(&mut h, offsets[i].0);
            push_u64(&mut h, offsets[i].1);
        }
        h
    };
    let blocks: Vec<Vec<u8>> = cols.iter().map(ColSpec::block).collect();
    let zeros: Vec<(u64, u64)> = vec![(0, 0); cols.len()];
    let header_len = header(&zeros).len() as u64;
    let mut offsets = Vec::with_capacity(cols.len());
    let mut cur = header_len;
    for b in &blocks {
        offsets.push((cur, b.len() as u64));
        cur += b.len() as u64;
    }
    let mut out = header(&offsets);
    for b in blocks {
        out.extend_from_slice(&b);
    }
    out
}

/// 常量表的固定列：name / type / value / export / desc（全部按字符串存）。
pub const CONST_COLS: [&str; 5] = ["name", "type", "value", "export", "desc"];

pub fn schema_hash_constant(constant: &Constant) -> u64 {
    let mut buf = Vec::new();
    push_str(&mut buf, &constant.name);
    push_u16(&mut buf, KIND_CONSTANT);
    for c in CONST_COLS {
        push_str(&mut buf, c);
        buf.push(TAG_STR);
    }
    fnv1a64(&buf)
}

/// 编码一张 Constant（5 列字符串，行 = 条目）。
pub fn encode_constant(constant: &Constant, sep: &SeparatorsSection) -> Vec<u8> {
    let entries: Vec<&crate::model::ConstEntry> = constant
        .entries
        .iter()
        .filter(|e| !e.name.is_empty())
        .collect();
    let cols: Vec<ColSpec> = vec![
        ColSpec { name: "name".into(), type_str: "str".into(), tag: TAG_STR, values: entries.iter().map(|e| e.name.clone()).collect() },
        ColSpec { name: "type".into(), type_str: "str".into(), tag: TAG_STR, values: entries.iter().map(|e| e.tbl_type.clone()).collect() },
        ColSpec { name: "value".into(), type_str: "str".into(), tag: TAG_STR, values: entries.iter().map(|e| e.value.clone()).collect() },
        ColSpec { name: "export".into(), type_str: "str".into(), tag: TAG_STR, values: entries.iter().map(|e| e.export.to_tbl().to_string()).collect() },
        ColSpec { name: "desc".into(), type_str: "str".into(), tag: TAG_STR, values: entries.iter().map(|e| e.desc.clone()).collect() },
    ];
    let sep_pairs = sep_pairs(sep);
    let row_count = entries.len() as u32;
    encode_payload(&constant.name, KIND_CONSTANT, schema_hash_constant(constant), &cols, &sep_pairs, row_count)
}

/// 编码整个项目的 bundle（节点名排序，保证字节稳定）。
pub fn encode_bundle(mut entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let header = |offsets: &[(u64, u64)]| -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(MAGIC);
        push_u16(&mut h, VERSION);
        push_u16(&mut h, KIND_BUNDLE);
        push_u16(&mut h, entries.len() as u16);
        for (i, (name, _)) in entries.iter().enumerate() {
            push_str(&mut h, name);
            push_u64(&mut h, offsets[i].0);
            push_u64(&mut h, offsets[i].1);
        }
        h
    };
    let zeros: Vec<(u64, u64)> = vec![(0, 0); entries.len()];
    let mut cur = header(&zeros).len() as u64;
    let mut offsets = Vec::with_capacity(entries.len());
    for (_, payload) in &entries {
        offsets.push((cur, payload.len() as u64));
        cur += payload.len() as u64;
    }
    let mut out = header(&offsets);
    for (_, payload) in entries {
        out.extend_from_slice(&payload);
    }
    out
}

fn collect_table_payloads(
    project: &Project,
    filter: &DataFilter,
) -> Vec<(String, Vec<u8>)> {
    let sep = &project.schema.separators;
    let mut entries = Vec::new();
    for group in &project.groups {
        if !filter.matches_group(&group.name) {
            continue;
        }
        for table in &group.tables {
            if table.deleted || !filter.matches_node(&table.name) {
                continue;
            }
            entries.push((table.name.clone(), encode_table(table, sep)));
        }
        for constant in &group.constants {
            if constant.deleted || !filter.matches_node(&constant.name) {
                continue;
            }
            entries.push((constant.name.clone(), encode_constant(constant, sep)));
        }
    }
    entries
}

fn bin_parts_dir(project: &Project) -> std::path::PathBuf {
    bin_output_dir(project).join("parts")
}

fn bin_output_dir(project: &Project) -> std::path::PathBuf {
    let data_output = project
        .config
        .export
        .server
        .as_ref()
        .and_then(|s| s.data_output.as_deref())
        .unwrap_or("gen/server/data");
    project.export_root().join(data_output).join("bin")
}

/// 单文件模式：整项目一个 `<data_output>/bin/config.bytes`。
pub fn export_all_bin(project: &Project, filter: &DataFilter) -> Result<ExportResult> {
    let output_dir = bin_output_dir(project);
    let bundle = encode_bundle(collect_table_payloads(project, filter));
    write_bin_dir(&output_dir, vec![(output_dir.join(BUNDLE_FILE), bundle)])
}

/// 多文件模式：每节点一个 `<Node>.bytes`，内容与 bundle 里的单个 payload 完全一致。
pub fn export_all_bin_parts(project: &Project, filter: &DataFilter) -> Result<ExportResult> {
    let output_dir = bin_parts_dir(project);
    let files = collect_table_payloads(project, filter)
        .into_iter()
        .map(|(name, payload)| (output_dir.join(format!("{}.bytes", name)), payload))
        .collect();
    write_bin_dir(&output_dir, files)
}

/// 二进制目录同步：写 bundle 并清理不再生成的 `.bytes`（不碰其它扩展名）。
fn write_bin_dir(
    output_dir: &Path,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
) -> Result<ExportResult> {
    use std::collections::HashSet;

    let mut existing: HashSet<std::path::PathBuf> = HashSet::new();
    if output_dir.exists() {
        for e in std::fs::read_dir(output_dir)? {
            let p = e?.path();
            if p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("bytes")) {
                existing.insert(p);
            }
        }
    }

    let mut result = ExportResult::default();
    for (path, bytes) in files {
        let existed = path.exists();
        std::fs::create_dir_all(path.parent().unwrap_or(output_dir))?;
        existing.remove(&path);
        let status = if existed && std::fs::read(&path).map(|old| old == bytes).unwrap_or(false) {
            FileStatus::Unchanged
        } else {
            std::fs::write(&path, &bytes)?;
            if existed { FileStatus::Modified } else { FileStatus::Added }
        };
        result.files.push(ExportFile { path: super::normalize_path(&path), status });
    }
    for stale in existing {
        if std::fs::remove_file(&stale).is_ok() {
            result.files.push(ExportFile {
                path: super::normalize_path(&stale),
                status: FileStatus::Deleted,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Export, FieldDef, TableSchema};

    fn table() -> Table {
        let fields = vec![
            FieldDef { name: "id".into(), desc: "ID".into(), tbl_type: "int".into(), export: Export::ClientServer , aux: None, source_col: None},
            FieldDef { name: "name".into(), desc: "名字".into(), tbl_type: "str".into(), export: Export::ClientServer , aux: None, source_col: None},
            FieldDef { name: "hp".into(), desc: "血量".into(), tbl_type: "long".into(), export: Export::ClientServer , aux: None, source_col: None},
            FieldDef { name: "ok".into(), desc: "".into(), tbl_type: "bool".into(), export: Export::ClientServer , aux: None, source_col: None},
            FieldDef { name: "tags".into(), desc: "".into(), tbl_type: "List<int>".into(), export: Export::ClientServer , aux: None, source_col: None},
        ];
        let records = vec![
            vec!["1".into(), "亚瑟".into(), "100".into(), "true".into(), "1;2".into()],
            vec!["2".into(), "".into(), "0".into(), "false".into(), "".into()],
        ];
        Table {
            name: "Hero".into(),
            original_name: "Hero".into(),
            path: Default::default(),
            schema: TableSchema { fields },
            records,
            dirty: false,
            deleted: false,
            original_records: Vec::new(),
            original_fields: Vec::new(),
            saved: true,
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
            display_field: None,
            original_display_field: None,
            rules: Vec::new(),
            original_rules: Vec::new(),
        }
    }

    fn u16_at(b: &[u8], p: &mut usize) -> u16 {
        let v = u16::from_le_bytes([b[*p], b[*p + 1]]);
        *p += 2;
        v
    }
    fn u32_at(b: &[u8], p: &mut usize) -> u32 {
        let v = u32::from_le_bytes(b[*p..*p + 4].try_into().unwrap());
        *p += 4;
        v
    }
    fn u64_at(b: &[u8], p: &mut usize) -> u64 {
        let v = u64::from_le_bytes(b[*p..*p + 8].try_into().unwrap());
        *p += 8;
        v
    }
    fn str_at(b: &[u8], p: &mut usize) -> String {
        let n = u16_at(b, p) as usize;
        let s = String::from_utf8(b[*p..*p + n].to_vec()).unwrap();
        *p += n;
        s
    }

    /// 解析单节点 payload，返回 (kind, hash, 列名, 原始值)。
    fn decode(payload: &[u8]) -> (u16, u64, Vec<String>, Vec<Vec<String>>) {
        assert_eq!(&payload[0..4], MAGIC);
        let mut p = 4;
        let _ver = u16_at(payload, &mut p);
        let kind = u16_at(payload, &mut p);
        let cols = u16_at(payload, &mut p) as usize;
        let rows = u32_at(payload, &mut p) as usize;
        let hash = u64_at(payload, &mut p);
        let sep_n = u16_at(payload, &mut p) as usize;
        for _ in 0..sep_n {
            let _ = str_at(payload, &mut p);
            let _ = str_at(payload, &mut p);
        }
        let mut names = Vec::new();
        let mut meta = Vec::new();
        for _ in 0..cols {
            let name = str_at(payload, &mut p);
            let _ty = str_at(payload, &mut p);
            let tag = payload[p];
            p += 1;
            let off = u64_at(payload, &mut p) as usize;
            let _len = u64_at(payload, &mut p) as usize;
            names.push(name);
            meta.push((tag, off));
        }
        let mut out = vec![vec![String::new(); cols]; rows];
        for (ci, (tag, off)) in meta.iter().enumerate() {
            for r in 0..rows {
                out[r][ci] = match tag {
                    &TAG_I32 => i32::from_le_bytes(payload[off + r * 4..off + r * 4 + 4].try_into().unwrap()).to_string(),
                    &TAG_I64 => i64::from_le_bytes(payload[off + r * 8..off + r * 8 + 8].try_into().unwrap()).to_string(),
                    &TAG_BOOL => (payload[off + r] != 0).to_string(),
                    _ => {
                        let blob = off + (rows + 1) * 4;
                        let s = u32::from_le_bytes(payload[off + r * 4..off + r * 4 + 4].try_into().unwrap()) as usize;
                        let e = u32::from_le_bytes(payload[off + (r + 1) * 4..off + (r + 1) * 4 + 4].try_into().unwrap()) as usize;
                        String::from_utf8(payload[blob + s..blob + e].to_vec()).unwrap()
                    }
                };
            }
        }
        (kind, hash, names, out)
    }

    #[test]
    fn date_uses_i64_tag() {
        assert_eq!(tag_of(TblType::parse("date").as_ref()), TAG_I64);
    }

    #[test]
    fn encodes_table_payload() {
        let t = table();
        let sep = SeparatorsSection::default();
        let bytes = encode_table(&t, &sep);
        let (kind, hash, names, rows) = decode(&bytes);
        assert_eq!(kind, KIND_TABLE);
        assert_eq!(hash, schema_hash_table(&t));
        assert_eq!(names, vec!["id", "name", "hp", "ok", "tags"]);
        assert_eq!(rows[0], vec!["1", "亚瑟", "100", "true", "1;2"]);
        assert_eq!(rows[1], vec!["2", "", "0", "false", ""]);
    }

    #[test]
    fn bundle_offsets_are_slices_of_payloads() {
        let t = table();
        let sep = SeparatorsSection::default();
        let a = encode_table(&t, &sep);
        let mut t2 = t.clone();
        t2.name = "Other".into();
        let b = encode_table(&t2, &sep);
        let bundle = encode_bundle(vec![("Hero".into(), a.clone()), ("Other".into(), b.clone())]);
        assert_eq!(&bundle[0..4], MAGIC);
        let mut p = 4;
        assert_eq!(u16_at(&bundle, &mut p), VERSION);
        assert_eq!(u16_at(&bundle, &mut p), KIND_BUNDLE);
        assert_eq!(u16_at(&bundle, &mut p), 2);
        let mut entries = Vec::new();
        for _ in 0..2 {
            let name = str_at(&bundle, &mut p);
            let off = u64_at(&bundle, &mut p) as usize;
            let len = u64_at(&bundle, &mut p) as usize;
            entries.push((name, off, len));
        }
        for (name, off, len) in entries {
            let payload = &bundle[off..off + len];
            let (_, _, _, rows) = decode(payload);
            assert_eq!(rows[0][0], "1");
            assert_eq!(rows[0][1], "亚瑟");
            let _ = name;
        }
    }

    #[test]
    fn encodes_constant_payload() {
        use crate::model::ConstEntry;
        let c = Constant {
            name: "AllTypes".into(),
            original_name: "AllTypes".into(),
            path: Default::default(),
            entries: vec![
                ConstEntry { name: "c_list".into(), tbl_type: "List<int>".into(), value: "1;2".into(), export: Export::ClientServer, desc: "列表".into() },
                ConstEntry { name: "c_map".into(), tbl_type: "Map<str,int>".into(), value: "hp:1".into(), export: Export::ClientServer, desc: "".into() },
            ],
            dirty: false,
            deleted: false,
            original_entries: Vec::new(),
            saved: true,
        };
        let sep = SeparatorsSection::default();
        let bytes = encode_constant(&c, &sep);
        let (kind, hash, names, rows) = decode(&bytes);
        assert_eq!(kind, KIND_CONSTANT);
        assert_eq!(hash, schema_hash_constant(&c));
        assert_eq!(names, CONST_COLS.to_vec());
        assert_eq!(rows[0], vec!["c_list", "List<int>", "1;2", "cs", "列表"]);
        assert_eq!(rows[1][2], "hp:1");
    }
}
