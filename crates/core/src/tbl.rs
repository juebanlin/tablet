use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;
use crate::tbl_str::{encode, decode, split_row};

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

    /// CLI / UI 输入里的版本标签（`v2` / `v3`，也接受裸数字）。
    pub fn parse_label(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "v2" | "2" => Some(Self::V2),
            "v3" | "3" => Some(Self::V3),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            TblVersion::V2 => "v2",
            TblVersion::V3 => "v3",
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
            .map(|s| decode(s, "txt")).collect();
        let types: Vec<&str> = type_line.split('|').collect();
        let exports: Vec<&str> = export_line.split('|').collect();
        let field_names: Vec<&str> = field_line.split('|').collect();

        let n = field_names.len();
        let mut field_defs = Vec::with_capacity(n);
        for i in 0..n {
            let raw_export = exports.get(i).copied().unwrap_or("");
            let Some(export) = Export::parse_code(raw_export) else {
                bail!("{}: 第 {} 列（字段 {}）的 export 值 \"{}\" 不合法——只接受短码 cs / c / s / -，留空 = cs",
                    path.display(), i + 1, field_names[i].trim(), raw_export.trim());
            };
            field_defs.push(FieldDef {
                name: field_names[i].trim().to_string(),
                desc: descs.get(i).map(|s| s.trim().to_string()).unwrap_or_default(),
                tbl_type: types.get(i).unwrap_or(&"str").trim().to_string(),
                export,
            });
        }

        let records: Vec<Vec<String>> = data_lines.iter()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let raw = split_row(line);
                raw.iter().enumerate().map(|(i, s)| {
                    let tp = field_defs.get(i).map(|f| f.tbl_type.as_str()).unwrap_or("txt");
                    decode(s, tp)
                }).collect()
            }).collect();

        let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        Ok(Table {
            original_name: name.clone(), name, path: path.to_path_buf(),
            schema: TableSchema { fields: field_defs.clone() },
            original_records: records.clone(), original_fields: field_defs,
            records, dirty: false, deleted: false, saved: true, fmt_ver: TblVersion::V2,
            display_field: None,
            original_display_field: None,
            rules: Vec::new(),
            original_rules: Vec::new(),
        })
    }

    fn serialize(&self, table: &Table) -> String {
        let fields = &table.schema.fields;
        let mut s = String::new();
        s.push_str("#!tbl v2\n");
        s.push_str("#mode table\n");
        s.push_str(&format!("#desc {}\n", fields.iter()
            .map(|f| encode(&f.desc, "txt")).collect::<Vec<_>>().join("|")));
        s.push_str(&format!("#export {}\n", fields.iter()
            .map(|f| f.export.to_tbl().to_string()).collect::<Vec<_>>().join("|")));
        s.push_str(&format!("#type {}\n", fields.iter()
            .map(|f| f.tbl_type.as_str()).collect::<Vec<_>>().join("|")));
        s.push_str(&format!("#field {}\n", fields.iter()
            .map(|f| f.name.as_str()).collect::<Vec<_>>().join("|")));
        s.push_str("---\n");
        for row in &table.records {
            let encoded: Vec<String> = row.iter().enumerate()
                .map(|(i, c)| encode(c, &fields[i].tbl_type))
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
            // `#` 开头是注释（含 Excel 往返用的 `#@color:` 行标记）。
            // 必须在属性/字段解析**之前**跳过：注释里带个冒号否则会被当成未知属性。
            if trimmed.starts_with('#') { continue; }

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
                        // 写盘走 `encode(desc, "txt")`（`serialize_table`），这里必须 decode 回去。
                        // 漏了的话 `|` 会被写成 `\|` 并把反斜杠留在 desc 里（V2 那条路是解码的）。
                        "desc" => f.desc = decode(val, "txt"),
                        "export" => {
                            let Some(e) = Export::parse_code(val) else {
                                bail!("{}: @field {} 的 export 值 \"{}\" 不合法——只接受短码 cs / c / s / -",
                                    path.display(), f.name, val);
                            };
                            f.export = e;
                        }
                        "type" => f.tbl_type = val.to_string(),
                        // 拼错的属性名不能吞：`descr:` / `typo:` 会让这段描述/类型
                        // 悄悄变回默认值，而文件看着没毛病。
                        _ => bail!(
                            "{}: @field {} 有未知属性 \"{}\"（只认 desc / export / type）",
                            path.display(), f.name, key
                        ),
                    }
                } else if let Some(ref mut r) = cur_record {
                    // record field: key=field name, value=cell
                    //
                    // **拼错的字段名必须报错。** 以前这里找不到就 `if let Some` 落空，
                    // 整行值被静默丢弃：AI 把 `hp` 写成 `HP`，`validate` 依然全绿，
                    // 导出少一个字段，直到运行时才发现。
                    match field_defs.iter().position(|fd| fd.name == key) {
                        Some(idx) => {
                            let tp = field_defs[idx].tbl_type.clone();
                            while r.len() <= idx { r.push(String::new()); }
                            r[idx] = decode(val, &tp);
                        }
                        None => bail!(
                            "{}: [{}] 里出现未声明的字段 \"{}\"（字段名拼错，或 @field 块写在了记录之后）",
                            path.display(),
                            r.first().map(String::as_str).unwrap_or("?"),
                            key
                        ),
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
            original_name: name.clone(), name, path: path.to_path_buf(),
            schema: TableSchema { fields: field_defs.clone() },
            original_records: records.clone(), original_fields: field_defs,
            records, dirty: false, deleted: false, saved: true, fmt_ver: TblVersion::V3,
            display_field: None,
            original_display_field: None,
            rules: Vec::new(),
            original_rules: Vec::new(),
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
                s.push_str(&format!("  desc:{}\n", encode(&f.desc, "txt")));
            }
            s.push_str(&format!("  export:{}\n", f.export.to_tbl()));
            s.push_str(&format!("  type:{}\n", f.tbl_type));
        }

        // [id] records
        for row in &table.records {
            let id_val = row.first().map(String::as_str).unwrap_or("");
            s.push('\n');
            s.push_str(&format!("[{}]\n", id_val));
            for (i, cell) in row.iter().enumerate().skip(1) {
                if cell.is_empty() { continue; }
                s.push_str(&format!("  {}:{}\n", fields[i].name, encode(cell, &fields[i].tbl_type)));
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
    // 每个错误都自带文件路径：调用方（加载整个项目 / util parse-tbl）不必再包一层，
    // 否则 `A 失败: A: ...` 会把路径打两遍。
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("{}: 读取失败: {}", path.display(), e))?;
    let mut lines = content.lines();

    let first = lines.next().unwrap_or("");
    if !first.starts_with("#!tbl") {
        bail!("{}: 缺少 #!tbl 头（第一行应为 `#!tbl v2` 或 `#!tbl v3`）", path.display());
    }
    let codec = Codec::detect(first)
        .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;

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
        let name = decode(&raw[0], "str");
        let tbl_type = decode(&raw[1], "str");
        let value = decode(&raw[2], &tbl_type);
        let export_raw = raw.get(3).map(|s| decode(s, "str")).unwrap_or_default();
        let desc = raw.get(4).map(|s| decode(s, "txt")).unwrap_or_default();
        let Some(export) = Export::parse_code(&export_raw) else {
            bail!("{}: 常量 {} 的 export 值 \"{}\" 不合法——只接受短码 cs / c / s / -，留空 = cs",
                path.display(), name.trim(), export_raw.trim());
        };
        entries.push(ConstEntry {
            name: name.trim().to_string(), tbl_type: tbl_type.trim().to_string(),
            value: value.trim().to_string(), export,
            desc: desc.trim().to_string(),
        });
    }
    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    Ok(TblFile::Constant(Constant {
        original_name: name.clone(), name, path: path.to_path_buf(),
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
            id: decode(&raw[0], "int").trim().to_string(),
            name: raw.get(1).map(|s| decode(s, "str").trim().to_string()).unwrap_or_default(),
            desc: raw.get(2).map(|s| decode(s, "txt").trim().to_string()).unwrap_or_default(),
        });
    }
    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    Ok(TblFile::Enum(EnumDef {
        original_name: name.clone(), name, path: path.to_path_buf(),
        original_entries: entries.clone(), entries,
        dirty: false, deleted: false, saved: true,
        display_field: None,
        original_display_field: None,
        rules: Vec::new(),
        original_rules: Vec::new(),
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
        s.push_str(&format!("{}|{}|{}|{}|{}\n",
            encode(&e.name, "str"), e.tbl_type,
            encode(&e.value, &e.tbl_type), e.export.to_tbl(),
            encode(&e.desc, "txt"),
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
            encode(&e.id, "int"),
            encode(&e.name, "str"),
            encode(&e.desc, "txt"),
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Export, FieldDef, Table, TableSchema};

    fn tmp(label: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!("tbl_rt_{}_{}_{}.tbl", label, std::process::id(), n))
    }

    fn table_with_desc(desc: &str) -> Table {
        let f = vec![
            FieldDef { name: "id".into(), desc: "ID".into(), tbl_type: "int".into(), export: Export::ClientServer },
            FieldDef { name: "name".into(), desc: desc.into(), tbl_type: "str".into(), export: Export::ClientServer },
        ];
        Table {
            name: "T".into(), original_name: "T".into(), path: std::path::PathBuf::new(),
            schema: TableSchema { fields: f.clone() },
            records: vec![vec!["1".into(), "a".into()]],
            dirty: false, deleted: false,
            original_records: vec![], original_fields: f,
            saved: true, fmt_ver: TblVersion::V3,
            display_field: None, original_display_field: None,

            rules: Vec::new(),
            original_rules: Vec::new(),
        }
    }

    /// `desc` 写在 `@field` 块里是 `desc:<值>`，上面没有分隔符语义——但写盘时走的是
    /// `encode(desc, "txt")` 转义。读回来必须 `decode` 回去，否则 `|` 会变成 `\|`
    /// 原样留在 desc 里（写转义、读不解码 = 往返不保真）。
    #[test]
    fn v3_round_trip_preserves_desc_with_special_characters() {
        for desc in ["普通说明", "含 | 竖线", r"含 \ 反斜杠", "含\n换行"] {
            let t = table_with_desc(desc);
            let txt = serialize_table(&t);
            let p = tmp("desc");
            std::fs::write(&p, &txt).unwrap();
            let back = match parse_tbl(&p).unwrap() { TblFile::Table(t) => t, _ => panic!("应为 Table") };
            let _ = std::fs::remove_file(&p);
            assert_eq!(back.schema.fields[1].desc, desc, "desc 往返失真:\n{}", txt);
        }
    }
    /// 中文展示值不是磁盘格式的一部分：`.tbl` 里读到就报错，不静默兜底
    /// （CLAUDE.md「数据格式：不做旧数据兼容」）。
    #[test]
    fn v3_rejects_a_non_short_code_export() {
        let good = serialize_table(&table_with_desc("x"));
        assert!(good.contains("  export:cs"), "写盘应当是短码:
{}", good);

        // 只改 name 那一段——id 在前面，全量 replace 会先报 id
        let bad = good.replace("desc:x
  export:cs", "desc:x
  export:前后端");
        assert_ne!(bad, good, "夹具没改到，锚点过时了");
        let p = tmp("bad_export");
        std::fs::write(&p, &bad).unwrap();
        let err = match parse_tbl(&p) { Err(e) => e, Ok(_) => panic!("中文 export 应当被拒绝") };
        let _ = std::fs::remove_file(&p);
        let msg = err.to_string();
        assert!(msg.contains("前后端"), "错误里应当有那个值: {}", msg);
        assert!(msg.contains("cs / c / s / -"), "错误里应当说清合法集合: {}", msg);
        assert!(msg.contains("name"), "错误里应当点名是哪个字段: {}", msg);
    }

    /// 记录里拼错字段名 / `@field` 块里拼错属性名，都必须报错。
    ///
    /// 以前两处都是"找不到就跳过"：`hp` 写成 `HP` 整行值被丢弃，`validate` 依然全绿，
    /// 导出少一个字段——手改 `.tbl` 正是 AI / 脚本的主要工作方式。
    #[test]
    fn v3_rejects_unknown_field_and_unknown_property() {
        let good = serialize_table(&table_with_desc("x"));

        let bad_field = good.replace("  name:a", "  Name:a");
        assert_ne!(bad_field, good, "夹具没改到，锚点过时了");
        let p = tmp("bad_field");
        std::fs::write(&p, &bad_field).unwrap();
        let msg = match parse_tbl(&p) { Err(e) => e.to_string(), Ok(_) => panic!("未声明字段应当被拒绝") };
        let _ = std::fs::remove_file(&p);
        assert!(msg.contains("Name"), "错误里要点名: {}", msg);
        assert!(msg.contains("[1]"), "错误里要带记录 id: {}", msg);

        let bad_prop = good.replace("  desc:x", "  descr:x");
        assert_ne!(bad_prop, good, "夹具没改到，锚点过时了");
        let p = tmp("bad_prop");
        std::fs::write(&p, &bad_prop).unwrap();
        let msg = match parse_tbl(&p) { Err(e) => e.to_string(), Ok(_) => panic!("未知属性应当被拒绝") };
        let _ = std::fs::remove_file(&p);
        assert!(msg.contains("descr"), "错误里要点名: {}", msg);

        // 注释（哪怕带冒号）不算属性
        let with_comment = good.replace("  desc:x", "  # 备注: 这行是注释\n  desc:x");
        let p = tmp("comment_ok");
        std::fs::write(&p, &with_comment).unwrap();
        assert!(parse_tbl(&p).is_ok(), "带冒号的注释不该被当成属性");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn constant_tbl_rejects_a_non_short_code_export() {
        let text = "#!tbl v2
#mode constant
---
max_level|int|100|前后端|最大等级
";
        let p = tmp("bad_const_export");
        std::fs::write(&p, text).unwrap();
        let err = match parse_tbl(&p) { Err(e) => e, Ok(_) => panic!("中文 export 应当被拒绝") };
        let _ = std::fs::remove_file(&p);
        assert!(err.to_string().contains("max_level"), "应当点名是哪个常量: {}", err);
    }

}
