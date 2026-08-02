//! Excel 同步核心引擎。
//!
//! 读取 .xlsx 使用 calamine，写出新文件使用 rust_xlsxwriter。
//! 编辑现有 xlsx 保留格式化需要 umya-spreadsheet，Phase 4 补上。

use std::path::Path;

use anyhow::{Context, Result};
use calamine::{open_workbook, Data, Reader, Xlsx};
use rust_xlsxwriter::{Format, FormatBorder, Workbook};

use crate::model::Group;

pub struct XlsxSheet {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, PartialEq)]
pub enum HeaderMatch {
    Identical,
    TailDiff,
    Mismatch,
    Invalid,
}

#[derive(Default, Clone)]
pub struct DiffCount {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", *f as i64) }
            else { f.to_string() }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        _ => cell.to_string(),
    }
}

/// Read all sheets from an xlsx file using calamine.
pub fn read_xlsx_sheets(path: &Path) -> Result<Vec<XlsxSheet>> {
    let mut wb: Xlsx<_> = open_workbook(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    let sheet_names: Vec<String> = wb.sheet_names();
    let mut sheets = Vec::new();

    for name in &sheet_names {
        let range = wb.worksheet_range(name)
            .with_context(|| format!("读取 sheet '{}' 失败", name))?;
        let rows_data: Vec<&[Data]> = range.rows().collect();
        if rows_data.is_empty() { continue; }

        // First row is header
        let headers: Vec<String> = rows_data[0].iter().map(cell_to_string).collect();
        let mut data_rows: Vec<Vec<String>> = Vec::new();

        for row in rows_data.iter().skip(1) {
            let cells: Vec<String> = row.iter().map(cell_to_string).collect();
            if cells.iter().all(|c| c.is_empty()) { continue; }
            // Trim trailing empty cells
            let mut cells = cells;
            while cells.last().map_or(false, |c| c.is_empty()) { cells.pop(); }
            data_rows.push(cells);
        }

        sheets.push(XlsxSheet { name: name.clone(), headers, rows: data_rows });
    }
    Ok(sheets)
}

pub fn classify_header(xlsx_headers: &[String], tablet_fields: &[crate::model::FieldDef]) -> HeaderMatch {
    if xlsx_headers.is_empty() { return HeaderMatch::Invalid; }
    if xlsx_headers.first().map_or(true, |h| h != "id") { return HeaderMatch::Invalid; }

    let n = xlsx_headers.len();
    let m = tablet_fields.len();
    let min_len = n.min(m);

    for i in 0..min_len {
        if xlsx_headers[i] != tablet_fields[i].name { return HeaderMatch::Mismatch; }
    }

    if n == m { HeaderMatch::Identical } else { HeaderMatch::TailDiff }
}

pub fn diff_rows(left_rows: usize, right_rows: usize, left: &[Vec<String>], right: &[Vec<String>]) -> DiffCount {
    let mut d = DiffCount::default();
    let min = left_rows.min(right_rows);
    for i in 0..min {
        if left.get(i) != right.get(i) { d.modified += 1; }
    }
    if left_rows > right_rows { d.added = left_rows - right_rows; }
    if right_rows > left_rows { d.removed = right_rows - left_rows; }
    d
}

/// Write group data to xlsx (creates new file, does not preserve existing formatting).
pub fn sync_group_to_xlsx(path: &Path, group: &Group) -> Result<()> {
    let mut wb = Workbook::new();
    let header_fmt = Format::new().set_bold().set_background_color("#E0E0E0").set_border(FormatBorder::Thin);
    let body_fmt = Format::new().set_border(FormatBorder::Thin);

    for table in &group.tables {
        if table.deleted { continue; }
        let ws = wb.add_worksheet();
        let name = sanitize_sheet_name(&table.name);
        ws.set_name(name)?;

        for (i, f) in table.schema.fields.iter().enumerate() {
            ws.write_string_with_format(0, i as u16, &f.name, &header_fmt)?;
        }
        for (r, row) in table.records.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                ws.write_string_with_format((r + 1) as u32, c as u16, val, &body_fmt)?;
            }
        }
    }

    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let buf = wb.save_to_buffer()?;
    std::fs::write(path, buf)?;
    Ok(())
}

pub struct GroupPatch {
    pub tables: Vec<(String, Vec<Vec<String>>)>,
}

pub fn read_group_from_xlsx(path: &Path, group: &Group) -> Result<GroupPatch> {
    let sheets = read_xlsx_sheets(path)?;
    let mut tables = Vec::new();

    for sheet in &sheets {
        if let Some(table) = group.tables.iter().find(|t| t.name == sheet.name) {
            let matched = classify_header(&sheet.headers, &table.schema.fields);
            if matched == HeaderMatch::Invalid || matched == HeaderMatch::Mismatch { continue; }
            let n = table.schema.fields.len();
            let records: Vec<Vec<String>> = sheet.rows.iter()
                .map(|r| { let mut c = r.clone(); c.resize(n, String::new()); c })
                .collect();
            tables.push((sheet.name.clone(), records));
        }
    }
    Ok(GroupPatch { tables })
}

fn sanitize_sheet_name(name: &str) -> String {
    let mut out: String = name.chars().map(|c| match c {
        '\\' | '/' | '?' | '*' | '[' | ']' | ':' => '_', _ => c,
    }).collect();
    if out.chars().count() > 31 { out = out.chars().take(31).collect(); }
    if out.is_empty() { out = "Sheet".to_string(); }
    out
}
