//! Excel 同步核心引擎（umya-spreadsheet 编辑保留格式化）。

use std::path::Path;

use anyhow::{Context, Result};
use calamine::{open_workbook, Data, Reader, Xlsx};

use crate::model::Group;

// ── types ──

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

#[derive(Clone, Copy, PartialEq)]
pub enum SyncMode { DataOnly, WithColumns, Full }

// ── cell helpers ──

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

// ── read ──

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

        let headers: Vec<String> = rows_data[0].iter().map(cell_to_string).collect();
        let mut data_rows: Vec<Vec<String>> = Vec::new();
        for row in rows_data.iter().skip(1) {
            let mut cells: Vec<String> = row.iter().map(cell_to_string).collect();
            if cells.iter().all(|c| c.is_empty()) { continue; }
            while cells.last().map_or(false, |c| c.is_empty()) { cells.pop(); }
            data_rows.push(cells);
        }
        sheets.push(XlsxSheet { name: name.clone(), headers, rows: data_rows });
    }
    Ok(sheets)
}

// ── header matching ──

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

fn map_columns(tablet_fields: &[crate::model::FieldDef], xlsx_headers: &[String]) -> Vec<Option<u32>> {
    tablet_fields.iter().map(|f| {
        xlsx_headers.iter().position(|h| h == &f.name).map(|i| i as u32 + 1)
    }).collect()
}

// ── diff ──

pub fn diff_rows(
    left_rows: usize, right_rows: usize,
    left: &[Vec<String>], right: &[Vec<String>],
) -> DiffCount {
    let mut d = DiffCount::default();
    let min = left_rows.min(right_rows);
    for i in 0..min {
        if left.get(i) != right.get(i) { d.modified += 1; }
    }
    if left_rows > right_rows { d.added = left_rows - right_rows; }
    if right_rows > left_rows { d.removed = right_rows - left_rows; }
    d
}

// ── sync: tablet → xlsx (with mode) ──

pub fn sync_group_to_xlsx(path: &Path, group: &Group, mode: SyncMode) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;

    if !path.exists() || mode == SyncMode::Full {
        return write_fresh_xlsx(path, group);
    }

    // Open existing xlsx with umya-spreadsheet to preserve formatting (3.0 API)
    let mut book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    for table in &group.tables {
        if table.deleted { continue; }
        let sheet_name = &table.name;

        // Get or create sheet — 3.0 sheet_by_name returns Result
        if book.sheet_by_name(sheet_name).is_err() {
            book.new_sheet(sheet_name)?;
        }
        let ws = book.sheet_by_name_mut(sheet_name)
            .map_err(|_| anyhow::anyhow!("sheet '{}' 不存在", sheet_name))?;

        let fields = &table.schema.fields;

        // Read existing xlsx headers (row 1) — 3.0 returns (u32, u32)
        let (max_col, _max_row) = ws.highest_column_and_row();

        let mut xlsx_headers: Vec<String> = Vec::new();
        for c in 1..=max_col {
            xlsx_headers.push(ws.value((c, 1u32)));
        }

        let col_map = map_columns(fields, &xlsx_headers);

        match mode {
            SyncMode::DataOnly => {
                for (ti, fi) in fields.iter().enumerate() {
                    if let Some(xcol) = col_map[ti] {
                        ws.cell_mut((xcol, 1u32)).set_value(fi.name.clone());
                    }
                }
            }
            SyncMode::WithColumns | SyncMode::Full => {
                let mut next_xcol = max_col + 1;
                for (ti, fi) in fields.iter().enumerate() {
                    if let Some(xcol) = col_map[ti] {
                        ws.cell_mut((xcol, 1u32)).set_value(fi.name.clone());
                    } else {
                        ws.cell_mut((next_xcol, 1u32)).set_value(fi.name.clone());
                        next_xcol += 1;
                    }
                }
            }
        }

        // Write data rows
        for (ri, row) in table.records.iter().enumerate() {
            let r = ri as u32 + 2;
            match mode {
                SyncMode::DataOnly => {
                    for (ci, val) in row.iter().enumerate() {
                        if let Some(xcol) = col_map[ci] {
                            ws.cell_mut((xcol, r)).set_value(val.clone());
                        }
                    }
                }
                SyncMode::WithColumns | SyncMode::Full => {
                    let mut next_xcol = max_col + 1;
                    for (ci, val) in row.iter().enumerate() {
                        if let Some(xcol) = col_map[ci] {
                            ws.cell_mut((xcol, r)).set_value(val.clone());
                        } else {
                            ws.cell_mut((next_xcol, r)).set_value(val.clone());
                            next_xcol += 1;
                        }
                    }
                }
            }
        }
    }

    umya_spreadsheet::writer::xlsx::write(&book, path)
        .with_context(|| format!("写入 xlsx 失败: {}", path.display()))?;
    Ok(())
}

fn write_fresh_xlsx(path: &Path, group: &Group) -> Result<()> {
    use rust_xlsxwriter::{Format, FormatBorder, Workbook};
    let mut wb = Workbook::new();
    let hdr = Format::new().set_bold().set_background_color("#E0E0E0").set_border(FormatBorder::Thin);
    let bdy = Format::new().set_border(FormatBorder::Thin);

    for table in &group.tables {
        if table.deleted { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(sanitize_sheet_name(&table.name))?;
        for (i, f) in table.schema.fields.iter().enumerate() {
            ws.write_string_with_format(0, i as u16, &f.name, &hdr)?;
        }
        for (r, row) in table.records.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                ws.write_string_with_format((r + 1) as u32, c as u16, val, &bdy)?;
            }
        }
    }
    let buf = wb.save_to_buffer()?;
    std::fs::write(path, buf)?;
    Ok(())
}

// ── sync: xlsx → tablet ──

pub struct GroupPatch {
    pub tables: Vec<(String, Vec<Vec<String>>)>,
}

pub fn read_group_from_xlsx(path: &Path, group: &Group, force: bool) -> Result<GroupPatch> {
    let sheets = read_xlsx_sheets(path)?;
    let mut tables = Vec::new();
    for sheet in &sheets {
        if let Some(table) = group.tables.iter().find(|t| t.name == sheet.name) {
            if !force {
                let matched = classify_header(&sheet.headers, &table.schema.fields);
                if matched == HeaderMatch::Invalid || matched == HeaderMatch::Mismatch { continue; }
            }
            let n = table.schema.fields.len().max(1);
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
