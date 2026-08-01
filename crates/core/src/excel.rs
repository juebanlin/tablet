//! Excel 桥接：把 .tbl 内容（Table / Constant / Enum）写成 xlsx。
//!
//! 设计要点：
//! - 单文件：一个 .tbl → 单 sheet xlsx（[`export_table_book`] 等）
//! - 整组：一个 Group → 多 sheet xlsx（[`export_group_book`]）
//! - 表头锁定：开 sheet protection，数据区用 `unprotect_range` 整段解锁；允许调整行高列宽、插入行
//! - 不带密码：防手滑而非防恶意
//! - 回读：[`import_xlsx_into_group`] 把策划改完的 xlsx 解析成 [`GroupPatch`]，调用方负责 apply。
//!   严格 header 校验：表头 / sheet 名 / 列数任一不对 → 整次拒绝（@docs/02-核心功能.md §20）。
//!
//! 行结构（与 .tbl 文本顺序一致，@01-tbl系统.md / tbl.rs::serialize_table）：
//! - Table：4 行表头（desc / export / type / field），数据从第 5 行开始
//! - Constant：1 行表头（name / type / value / export / desc），数据从第 2 行开始
//! - Enum：1 行表头（id / name / desc），数据从第 2 行开始
//!
//! Export 字段在 xlsx 里用 [`Export::display()`] 的中文形式（"前后端" / "客户端" / "服务器" /
//! "不导出"），便于策划阅读；回读用 [`Export::from_str`] 反解，同时兼容空串 / `cs` 短码。

use std::path::Path;

use anyhow::{Context, Result};
use calamine::{open_workbook, Data, Range, Reader, Xlsx};
use rust_xlsxwriter::{Format, FormatBorder, ProtectionOptions, Workbook, Worksheet};

use crate::model::{ConstEntry, Constant, EnumDef, EnumEntry, Export, Group, Table};

const HEADER_BG: &str = "#E0E0E0";
const EXCEL_MAX_ROW: u32 = 1_048_575;

fn header_format() -> Format {
    Format::new()
        .set_bold()
        .set_background_color(HEADER_BG)
        .set_border(FormatBorder::Thin)
}

fn body_format() -> Format {
    Format::new().set_border(FormatBorder::Thin)
}

fn sheet_protection() -> ProtectionOptions {
    ProtectionOptions {
        format_columns: true,
        format_rows: true,
        insert_rows: true,
        ..ProtectionOptions::default()
    }
}

/// Excel 工作表名约束：≤ 31 字符 + 禁止 `\ / ? * [ ] :`。
fn sanitize_sheet_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | '?' | '*' | '[' | ']' | ':' => '_',
            _ => c,
        })
        .collect();
    if out.chars().count() > 31 {
        out = out.chars().take(31).collect();
    }
    if out.is_empty() {
        out = "Sheet".to_string();
    }
    out
}

/// 把单个 Table 写到 worksheet。表头 4 行（desc / export / type / field）+ 数据。
///
/// 顺序与 .tbl 文本（@tbl.rs::serialize_table）一致。Export 列写入 [`Export::display()`]
/// 中文形式（"前后端" / "客户端" / "服务器" / "不导出"），策划读起来直观。
pub fn write_table(ws: &mut Worksheet, table: &Table) -> Result<()> {
    let header_fmt = header_format();
    let body_fmt = body_format();
    let fields = &table.schema.fields;
    let col_count = fields.len() as u16;

    for (i, f) in fields.iter().enumerate() {
        let c = i as u16;
        ws.write_string_with_format(0, c, &f.desc, &header_fmt)?;
        ws.write_string_with_format(1, c, f.export.display(), &header_fmt)?;
        ws.write_string_with_format(2, c, &f.tbl_type, &header_fmt)?;
        ws.write_string_with_format(3, c, &f.name, &header_fmt)?;
    }

    for (r, row) in table.records.iter().enumerate() {
        let row_idx = (r + 4) as u32;
        for (c, cell) in row.iter().enumerate() {
            ws.write_string_with_format(row_idx, c as u16, cell, &body_fmt)?;
        }
    }

    ws.set_freeze_panes(4, 0)?;
    ws.protect_with_options(&sheet_protection());
    if col_count > 0 {
        ws.unprotect_range(4, 0, EXCEL_MAX_ROW, col_count - 1)?;
    }
    Ok(())
}

/// 把单个 Constant 写到 worksheet。表头：name / type / value / export / desc。
pub fn write_constant(ws: &mut Worksheet, constant: &Constant) -> Result<()> {
    let header_fmt = header_format();
    let body_fmt = body_format();
    const HEADERS: [&str; 5] = ["name", "type", "value", "export", "desc"];

    for (i, h) in HEADERS.iter().enumerate() {
        ws.write_string_with_format(0, i as u16, *h, &header_fmt)?;
    }

    for (r, e) in constant.entries.iter().enumerate() {
        let row = (r + 1) as u32;
        ws.write_string_with_format(row, 0, &e.name, &body_fmt)?;
        ws.write_string_with_format(row, 1, &e.tbl_type, &body_fmt)?;
        ws.write_string_with_format(row, 2, &e.value, &body_fmt)?;
        ws.write_string_with_format(row, 3, e.export.display(), &body_fmt)?;
        ws.write_string_with_format(row, 4, &e.desc, &body_fmt)?;
    }

    ws.set_freeze_panes(1, 0)?;
    ws.protect_with_options(&sheet_protection());
    ws.unprotect_range(1, 0, EXCEL_MAX_ROW, 4)?;
    Ok(())
}

/// 把单个 Enum 写到 worksheet。表头：id / name / desc。
pub fn write_enum(ws: &mut Worksheet, enum_def: &EnumDef) -> Result<()> {
    let header_fmt = header_format();
    let body_fmt = body_format();
    const HEADERS: [&str; 3] = ["id", "name", "desc"];

    for (i, h) in HEADERS.iter().enumerate() {
        ws.write_string_with_format(0, i as u16, *h, &header_fmt)?;
    }

    for (r, e) in enum_def.entries.iter().enumerate() {
        let row = (r + 1) as u32;
        ws.write_string_with_format(row, 0, &e.id, &body_fmt)?;
        ws.write_string_with_format(row, 1, &e.name, &body_fmt)?;
        ws.write_string_with_format(row, 2, &e.desc, &body_fmt)?;
    }

    ws.set_freeze_panes(1, 0)?;
    ws.protect_with_options(&sheet_protection());
    ws.unprotect_range(1, 0, EXCEL_MAX_ROW, 2)?;
    Ok(())
}

/// 单 Table → 单 sheet xlsx 字节。
pub fn export_table_book(table: &Table) -> Result<Vec<u8>> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(sanitize_sheet_name(&table.name))?;
    write_table(ws, table)?;
    Ok(wb.save_to_buffer()?)
}

/// 单 Constant → 单 sheet xlsx 字节。
pub fn export_constant_book(constant: &Constant) -> Result<Vec<u8>> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(sanitize_sheet_name(&constant.name))?;
    write_constant(ws, constant)?;
    Ok(wb.save_to_buffer()?)
}

/// 单 Enum → 单 sheet xlsx 字节。
pub fn export_enum_book(enum_def: &EnumDef) -> Result<Vec<u8>> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(sanitize_sheet_name(&enum_def.name))?;
    write_enum(ws, enum_def)?;
    Ok(wb.save_to_buffer()?)
}

/// 整个 Group（或子集）→ 多 sheet xlsx 字节。
///
/// 顺序：tables → constants → enums，每个域内按 group 内**原顺序**。Sheet 名同节点名（经 [`sanitize_sheet_name`]）。
///
/// `include` 语义：
/// - `None`              = 写出 group 内所有 Table / Constant / Enum
/// - `Some(&[name, ...])`= 仅写名字在列表中的节点（按 group 内**原顺序**排列，与 include 列表顺序无关）
/// - `Some(&[])`         = `Err`（语义不明确，强制调用方用 `None` 表示"全部"）
///
/// `include` 列表中存在 group 内不存在的节点名 → `Err`，错误信息列出全部 miss 的名字。
pub fn export_group_book(group: &Group, include: Option<&[&str]>) -> Result<Vec<u8>> {
    // include = Some(&[]) 不允许
    if let Some(xs) = include {
        if xs.is_empty() {
            anyhow::bail!("excel: include 列表不能为空 Some(&[])，要表示\"全部\"请用 None");
        }
    }

    // include = Some(xs) 时先校验全部命中（构 set 一次性检查，错误一次报全）
    let include_set: Option<std::collections::HashSet<&str>> = include.map(|xs| xs.iter().copied().collect());

    if let Some(set) = &include_set {
        let mut existing: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for t in &group.tables { existing.insert(t.name.as_str()); }
        for c in &group.constants { existing.insert(c.name.as_str()); }
        for e in &group.enums { existing.insert(e.name.as_str()); }

        let miss: Vec<&str> = set.iter().copied().filter(|n| !existing.contains(n)).collect();
        if !miss.is_empty() {
            let mut sorted = miss;
            sorted.sort_unstable();
            anyhow::bail!(
                "excel: include 包含不属于分组 '{}' 的节点: {}",
                group.name,
                sorted.join(", ")
            );
        }
    }

    let want = |name: &str| -> bool {
        include_set.as_ref().is_none_or(|s| s.contains(name))
    };

    let mut wb = Workbook::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();

    for t in &group.tables {
        if !want(&t.name) { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(unique_sheet_name(&t.name, &mut used))?;
        write_table(ws, t)?;
    }
    for c in &group.constants {
        if !want(&c.name) { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(unique_sheet_name(&c.name, &mut used))?;
        write_constant(ws, c)?;
    }
    for e in &group.enums {
        if !want(&e.name) { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(unique_sheet_name(&e.name, &mut used))?;
        write_enum(ws, e)?;
    }

    Ok(wb.save_to_buffer()?)
}

fn unique_sheet_name(raw: &str, used: &mut std::collections::HashSet<String>) -> String {
    let base = sanitize_sheet_name(raw);
    if used.insert(base.clone()) {
        return base;
    }
    for n in 2..1000 {
        let suffix = format!("_{}", n);
        let take = 31usize.saturating_sub(suffix.chars().count());
        let trimmed: String = base.chars().take(take).collect();
        let candidate = format!("{}{}", trimmed, suffix);
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    base
}

// ============================================================================
// 回读：xlsx → GroupPatch（@docs/02-核心功能.md §20 / @docs/06-Excel桥接.md §3）
// ============================================================================

/// 单个节点的回写补丁。
#[derive(Debug, Clone)]
pub enum NodePatch {
    Table { name: String, records: Vec<Vec<String>> },
    Constant { name: String, entries: Vec<ConstEntry> },
    Enum { name: String, entries: Vec<EnumEntry> },
}

/// 整组回写补丁。GUI / CLI 拿到后逐项 apply 到内存 group。
#[derive(Debug, Clone, Default)]
pub struct GroupPatch {
    pub patches: Vec<NodePatch>,
}

/// 解析 xlsx → GroupPatch。
///
/// 严格结构校验失败 → `Err`，**不返回部分 patch**（整次回写要么全成要么全拒）。
///
/// 校验项（@docs/02-核心功能.md §20）：
/// - sheet 名集合必须 = group 内节点名集合（多 / 缺均拒）
/// - Table sheet：列数 = schema 字段数；4 行表头每格 = (desc / export / type / field) 与 schema 一致
/// - Constant sheet：列数 = 5；表头 = ["name","type","value","export","desc"]
/// - Enum sheet：列数 = 3；表头 = ["id","name","desc"]
///
/// 数据宽容度：
/// - 尾部全空行自动忽略
/// - 单元格空 → 保留空字符串
/// - 单元格 Float / Int / Bool → 全部转字符串（保持 .tbl 文本格式）
pub fn import_xlsx_into_group(path: &Path, group: &Group) -> Result<GroupPatch> {
    let mut wb: Xlsx<_> = open_workbook(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    validate_sheets_match_group(&wb, group)?;

    let mut patches: Vec<NodePatch> = Vec::new();
    for t in &group.tables {
        let range = wb
            .worksheet_range(&t.name)
            .with_context(|| format!("读取 sheet '{}' 失败", t.name))?;
        let records = parse_table_sheet(&range, t)?;
        patches.push(NodePatch::Table { name: t.name.clone(), records });
    }
    for c in &group.constants {
        let range = wb
            .worksheet_range(&c.name)
            .with_context(|| format!("读取 sheet '{}' 失败", c.name))?;
        let entries = parse_constant_sheet(&range, c)?;
        patches.push(NodePatch::Constant { name: c.name.clone(), entries });
    }
    for e in &group.enums {
        let range = wb
            .worksheet_range(&e.name)
            .with_context(|| format!("读取 sheet '{}' 失败", e.name))?;
        let entries = parse_enum_sheet(&range, e)?;
        patches.push(NodePatch::Enum { name: e.name.clone(), entries });
    }

    Ok(GroupPatch { patches })
}

fn validate_sheets_match_group<R: std::io::Read + std::io::Seek>(
    wb: &Xlsx<R>,
    group: &Group,
) -> Result<()> {
    use std::collections::HashSet;

    let mut node_names: HashSet<&str> = HashSet::new();
    for t in &group.tables { node_names.insert(t.name.as_str()); }
    for c in &group.constants { node_names.insert(c.name.as_str()); }
    for e in &group.enums { node_names.insert(e.name.as_str()); }

    let sheet_names: Vec<String> = wb.sheet_names();
    let sheet_set: HashSet<&str> = sheet_names.iter().map(|s| s.as_str()).collect();

    let mut unknown: Vec<&str> = sheet_set
        .iter()
        .filter(|n| !node_names.contains(*n))
        .copied()
        .collect();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        anyhow::bail!(
            "xlsx 含未匹配 sheet（不属于分组 '{}' 的任何节点）: {}",
            group.name,
            unknown.join(", "),
        );
    }

    let mut missing: Vec<&str> = node_names
        .iter()
        .filter(|n| !sheet_set.contains(*n))
        .copied()
        .collect();
    if !missing.is_empty() {
        missing.sort_unstable();
        anyhow::bail!(
            "xlsx 缺少分组 '{}' 内节点对应的 sheet: {}",
            group.name,
            missing.join(", "),
        );
    }

    Ok(())
}

/// xlsx 单元格 → 字符串。Float / Int / Bool / 日期统一转字符串，保持 .tbl 文本格式。
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            // 整数化处理：1.0 → "1"，避免 calamine 把整数读成 float 后污染 .tbl
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#ERR:{:?}", e),
    }
}

fn is_empty_row(row: &[Data]) -> bool {
    row.iter().all(|c| matches!(c, Data::Empty) || cell_to_string(c).is_empty())
}

fn parse_table_sheet(range: &Range<Data>, table: &Table) -> Result<Vec<Vec<String>>> {
    let expected_cols = table.schema.fields.len();
    let total_rows = range.height();
    let total_cols = range.width();

    if total_rows < 4 {
        anyhow::bail!(
            "table sheet '{}' 行数 {} < 4，缺表头",
            table.name, total_rows,
        );
    }
    if total_cols != expected_cols {
        anyhow::bail!(
            "table sheet '{}' 列数 {} ≠ schema 字段数 {}",
            table.name, total_cols, expected_cols,
        );
    }

    let rows: Vec<&[Data]> = range.rows().collect();

    // 严格 header 校验：4 行 × N 列，逐格比对（顺序：desc / export / type / field）。
    // Export 与写出端一致使用 [`Export::display()`] 中文形式。
    for (col_idx, field) in table.schema.fields.iter().enumerate() {
        let actual_desc = cell_to_string(&rows[0][col_idx]);
        let actual_export = cell_to_string(&rows[1][col_idx]);
        let actual_type = cell_to_string(&rows[2][col_idx]);
        let actual_name = cell_to_string(&rows[3][col_idx]);

        if actual_desc != field.desc {
            anyhow::bail!(
                "table sheet '{}' 列 {} 表头 desc '{}' ≠ schema '{}'",
                table.name, col_idx, actual_desc, field.desc,
            );
        }
        if actual_export != field.export.display() {
            anyhow::bail!(
                "table sheet '{}' 列 {} 表头 export '{}' ≠ schema '{}'",
                table.name, col_idx, actual_export, field.export.display(),
            );
        }
        if actual_type != field.tbl_type {
            anyhow::bail!(
                "table sheet '{}' 列 {} 表头 type '{}' ≠ schema '{}'",
                table.name, col_idx, actual_type, field.tbl_type,
            );
        }
        if actual_name != field.name {
            anyhow::bail!(
                "table sheet '{}' 列 {} 表头 name '{}' ≠ schema '{}'",
                table.name, col_idx, actual_name, field.name,
            );
        }
    }

    let mut records: Vec<Vec<String>> = Vec::new();
    for row in rows.iter().skip(4) {
        if is_empty_row(row) {
            continue;
        }
        let cells: Vec<String> = (0..expected_cols)
            .map(|c| cell_to_string(&row[c]))
            .collect();
        records.push(cells);
    }
    Ok(records)
}

fn parse_constant_sheet(range: &Range<Data>, constant: &Constant) -> Result<Vec<ConstEntry>> {
    let total_rows = range.height();
    let total_cols = range.width();

    if total_rows < 1 {
        anyhow::bail!("constant sheet '{}' 缺表头行", constant.name);
    }
    if total_cols != 5 {
        anyhow::bail!(
            "constant sheet '{}' 列数 {} ≠ 5（name/type/value/export/desc）",
            constant.name, total_cols,
        );
    }

    let rows: Vec<&[Data]> = range.rows().collect();
    const EXPECTED: [&str; 5] = ["name", "type", "value", "export", "desc"];
    for (i, exp) in EXPECTED.iter().enumerate() {
        let actual = cell_to_string(&rows[0][i]);
        if actual != *exp {
            anyhow::bail!(
                "constant sheet '{}' 表头第 {} 列 '{}' ≠ '{}'",
                constant.name, i, actual, exp,
            );
        }
    }

    let mut entries: Vec<ConstEntry> = Vec::new();
    for row in rows.iter().skip(1) {
        if is_empty_row(row) {
            continue;
        }
        entries.push(ConstEntry {
            name: cell_to_string(&row[0]),
            tbl_type: cell_to_string(&row[1]),
            value: cell_to_string(&row[2]),
            export: Export::from_str(&cell_to_string(&row[3])),
            desc: cell_to_string(&row[4]),
        });
    }
    Ok(entries)
}

fn parse_enum_sheet(range: &Range<Data>, enum_def: &EnumDef) -> Result<Vec<EnumEntry>> {
    let total_rows = range.height();
    let total_cols = range.width();

    if total_rows < 1 {
        anyhow::bail!("enum sheet '{}' 缺表头行", enum_def.name);
    }
    if total_cols != 3 {
        anyhow::bail!(
            "enum sheet '{}' 列数 {} ≠ 3（id/name/desc）",
            enum_def.name, total_cols,
        );
    }

    let rows: Vec<&[Data]> = range.rows().collect();
    const EXPECTED: [&str; 3] = ["id", "name", "desc"];
    for (i, exp) in EXPECTED.iter().enumerate() {
        let actual = cell_to_string(&rows[0][i]);
        if actual != *exp {
            anyhow::bail!(
                "enum sheet '{}' 表头第 {} 列 '{}' ≠ '{}'",
                enum_def.name, i, actual, exp,
            );
        }
    }

    let mut entries: Vec<EnumEntry> = Vec::new();
    for row in rows.iter().skip(1) {
        if is_empty_row(row) {
            continue;
        }
        entries.push(EnumEntry {
            id: cell_to_string(&row[0]),
            name: cell_to_string(&row[1]),
            desc: cell_to_string(&row[2]),
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FieldDef, TableSchema};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn t(name: &str) -> Table {
        Table {
            name: name.to_string(),
            path: PathBuf::from(format!("{}.tbl", name)),
            schema: TableSchema { fields: vec![
                FieldDef { name: "id".into(), desc: "编号".into(), tbl_type: "int".into(), export: Export::ClientServer },
                FieldDef { name: "name".into(), desc: "名字".into(), tbl_type: "str".into(), export: Export::ClientServer },
            ]},
            records: vec![vec!["1".into(), "alice".into()]],
            dirty: false, deleted: false, original_records: vec![vec!["1".into(), "alice".into()]], saved: false,
        }
    }

    fn c(name: &str) -> Constant {
        let snap = vec![ConstEntry {
            name: "FOO".into(), tbl_type: "int".into(), value: "42".into(),
            export: Export::ClientServer, desc: "示例常量".into(),
        }];
        Constant {
            name: name.into(),
            path: PathBuf::from(format!("{}.tbl", name)),
            entries: snap.clone(),
            dirty: false, deleted: false, original_entries: snap, saved: false,
        }
    }

    fn e(name: &str) -> EnumDef {
        EnumDef {
            name: name.into(),
        path: PathBuf::from(format!("{}.tbl", name)),
            entries: vec![
                EnumEntry { id: "1".into(), name: "RED".into(), desc: "红".into() },
            ],
            dirty: false, deleted: false, original_entries: vec![EnumEntry { id: "1".into(), name: "RED".into(), desc: "红".into() }], saved: false,
        }
    }

    fn g(name: &str, table_names: &[&str]) -> Group {
        Group {
            name: name.into(),
            dir: PathBuf::from(name),
            tables: table_names.iter().map(|n| t(n)).collect(),
            constants: vec![],
            enums: vec![],
            is_new: false,
        }
    }

    fn g_mixed() -> Group {
        Group {
            name: "hero".into(),
            dir: PathBuf::from("hero"),
            tables: vec![t("t1")],
            constants: vec![c("c1")],
            enums: vec![e("e1")],
            is_new: false,
        }
    }

    /// 临时 xlsx 文件包装：drop 时自动删除。
    struct TempXlsx(PathBuf);
    impl TempXlsx {
        fn new(bytes: &[u8]) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir()
                .join(format!("tablet_excel_test_{}_{}.xlsx", std::process::id(), n));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }
    impl Drop for TempXlsx {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn xlsx_magic_bytes() {
        let bytes = export_table_book(&t("T")).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
    }

    #[test]
    fn sheet_name_sanitize() {
        assert_eq!(sanitize_sheet_name("a/b:c"), "a_b_c");
        assert_eq!(sanitize_sheet_name(""), "Sheet");
        let long = "x".repeat(50);
        assert_eq!(sanitize_sheet_name(&long).chars().count(), 31);
    }

    // ----- export_group_book: filter mode -----

    #[test]
    fn group_book_full_default_ok() {
        let group = g("hero", &["t1", "t2", "t3"]);
        let bytes = export_group_book(&group, None).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
    }

    #[test]
    fn group_book_include_subset_smaller_than_full() {
        let group = g("hero", &["t1", "t2", "t3"]);
        let full = export_group_book(&group, None).unwrap();
        let subset = export_group_book(&group, Some(&["t1", "t2"])).unwrap();
        // 子集少一个 sheet，xlsx 字节应严格小于全集
        assert!(
            subset.len() < full.len(),
            "subset bytes {} should be < full bytes {}",
            subset.len(), full.len()
        );
    }

    #[test]
    fn group_book_include_empty_slice_errs() {
        let group = g("hero", &["t1"]);
        let err = export_group_book(&group, Some(&[])).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("不能为空"), "got: {}", msg);
    }

    #[test]
    fn group_book_include_unknown_errs_with_name() {
        let group = g("hero", &["t1"]);
        let err = export_group_book(&group, Some(&["t1", "ghost"])).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("ghost"), "错误信息应包含未匹配的名字 'ghost'，got: {}", msg);
        assert!(msg.contains("hero"), "错误信息应包含 group 名 'hero'，got: {}", msg);
    }

    #[test]
    fn group_book_include_order_independent_of_argument_order() {
        // include 列表顺序与 group 内顺序不同时，输出仍按 group 内原顺序，字节应一致
        let group = g("hero", &["alpha", "beta", "gamma"]);
        let a = export_group_book(&group, Some(&["alpha", "beta"])).unwrap();
        let b = export_group_book(&group, Some(&["beta", "alpha"])).unwrap();
        assert_eq!(a.len(), b.len(),
            "include 列表顺序变化不应影响输出，{} vs {}", a.len(), b.len());
    }

    // ----- import_xlsx_into_group: round-trip + 严格校验 -----

    #[test]
    fn import_round_trip_table_only() {
        let group = g("hero", &["t1"]);
        let bytes = export_group_book(&group, None).unwrap();
        let tmp = TempXlsx::new(&bytes);

        let patch = import_xlsx_into_group(&tmp.0, &group).unwrap();
        assert_eq!(patch.patches.len(), 1);
        match &patch.patches[0] {
            NodePatch::Table { name, records } => {
                assert_eq!(name, "t1");
                assert_eq!(records, &group.tables[0].records);
            }
            _ => panic!("expected Table patch, got {:?}", patch.patches[0]),
        }
    }

    #[test]
    fn import_round_trip_mixed_group() {
        let group = g_mixed();
        let bytes = export_group_book(&group, None).unwrap();
        let tmp = TempXlsx::new(&bytes);

        let patch = import_xlsx_into_group(&tmp.0, &group).unwrap();
        assert_eq!(patch.patches.len(), 3);
        // 顺序：tables → constants → enums
        match &patch.patches[0] {
            NodePatch::Table { name, records } => {
                assert_eq!(name, "t1");
                assert_eq!(records, &group.tables[0].records);
            }
            _ => panic!("[0] expected Table"),
        }
        match &patch.patches[1] {
            NodePatch::Constant { name, entries } => {
                assert_eq!(name, "c1");
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "FOO");
                assert_eq!(entries[0].tbl_type, "int");
                assert_eq!(entries[0].value, "42");
                assert_eq!(entries[0].export, Export::ClientServer);
                assert_eq!(entries[0].desc, "示例常量");
            }
            _ => panic!("[1] expected Constant"),
        }
        match &patch.patches[2] {
            NodePatch::Enum { name, entries } => {
                assert_eq!(name, "e1");
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].id, "1");
                assert_eq!(entries[0].name, "RED");
                assert_eq!(entries[0].desc, "红");
            }
            _ => panic!("[2] expected Enum"),
        }
    }

    #[test]
    fn import_unknown_sheet_errs() {
        // 写出 group_a (含 t1)；用 group_b (含 other) 去 import → t1 是未匹配 sheet
        let group_a = g("hero", &["t1"]);
        let bytes = export_group_book(&group_a, None).unwrap();
        let tmp = TempXlsx::new(&bytes);

        let group_b = g("hero", &["other"]);
        let err = import_xlsx_into_group(&tmp.0, &group_b).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("t1"), "应提及未匹配的 sheet 't1'，got: {}", msg);
    }

    #[test]
    fn import_missing_sheet_errs() {
        // 写出 group (含 t1+t2) 仅 include t1；用完整 group 去 import → t2 缺失
        let group = g("hero", &["t1", "t2"]);
        let bytes = export_group_book(&group, Some(&["t1"])).unwrap();
        let tmp = TempXlsx::new(&bytes);

        let err = import_xlsx_into_group(&tmp.0, &group).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("t2"), "应提及缺失的 sheet 't2'，got: {}", msg);
    }

    #[test]
    fn import_header_tampering_errs() {
        // 写出 group_a；用 schema 字段名不同的 group_b 去 import → 表头 name 列不一致
        let group_a = g("hero", &["t1"]);
        let bytes = export_group_book(&group_a, None).unwrap();
        let tmp = TempXlsx::new(&bytes);

        let mut group_b = g("hero", &["t1"]);
        group_b.tables[0].schema.fields[1].name = "alias".into(); // 原是 "name"

        let err = import_xlsx_into_group(&tmp.0, &group_b).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("name") || msg.contains("alias"),
            "应提及表头不一致，got: {}", msg);
    }

    // ----- cell_to_string -----

    #[test]
    fn cell_to_string_float_integer_unwrap() {
        // calamine 把整数读成 Float(1.0) 时不应保留 ".0"
        assert_eq!(cell_to_string(&Data::Float(1.0)), "1");
        assert_eq!(cell_to_string(&Data::Float(42.0)), "42");
        assert_eq!(cell_to_string(&Data::Float(1.5)), "1.5");
        assert_eq!(cell_to_string(&Data::Int(42)), "42");
        assert_eq!(cell_to_string(&Data::Bool(true)), "true");
        assert_eq!(cell_to_string(&Data::Empty), "");
        assert_eq!(cell_to_string(&Data::String("hi".into())), "hi");
    }
}






