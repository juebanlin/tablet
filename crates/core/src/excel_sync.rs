//! Excel 同步核心引擎（umya-spreadsheet 编辑保留格式化）。

use std::path::Path;

use anyhow::{Context, Result};
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

// ── read ──
// 统一使用 umya-spreadsheet 读取，value() 直接返回字符串，避免 calamine 的 Float/Int/Data 类型推断。

pub fn read_xlsx_sheets(path: &Path) -> Result<Vec<XlsxSheet>> {
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    let mut sheets = Vec::new();
    let sheet_count = book.get_sheet_collection_no_check().len();

    for idx in 0..sheet_count {
        let worksheet = book.get_sheet_collection_no_check().get(idx)
            .with_context(|| format!("获取第 {} 个 sheet 失败", idx))?;
        let name = worksheet.get_name().to_string();

        let (max_col, max_row) = worksheet.highest_column_and_row();

        let mut headers = Vec::new();
        let mut data_rows: Vec<Vec<String>> = Vec::new();

        for c in 1..=max_col {
            headers.push(worksheet.value((c, 1u32)));
        }

        for r in 2..=max_row {
            let mut cells = Vec::new();
            let mut all_empty = true;
            for c in 1..=max_col {
                let v = worksheet.value((c as u32, r as u32));
                if !v.is_empty() { all_empty = false; }
                cells.push(v);
            }
            if all_empty { continue; }
            while cells.last().map_or(false, |c| c.is_empty()) { cells.pop(); }
            data_rows.push(cells);
        }
        sheets.push(XlsxSheet { name, headers, rows: data_rows });
    }
    Ok(sheets)
}

// ── header matching ──

/// 列匹配结果。
pub struct ColumnMatch {
    /// 按顺序匹配的前缀列数（同名同序）。
    pub matched_prefix: usize,
    /// tablet 有但 xlsx 没有的列数（在 matched_prefix 之后）。
    pub tablet_only: usize,
    /// xlsx 有但 tablet 没有的列数（在 matched_prefix 之后）。
    pub xlsx_only: usize,
    /// tablet 每列在 xlsx 中的列号 (1-based)；None 表示不匹配。
    pub col_map: Vec<Option<u32>>,
}

/// 计算两边的列匹配关系。
pub fn compute_column_match(tablet_fields: &[crate::model::FieldDef], xlsx_headers: &[String]) -> ColumnMatch {
    let n = tablet_fields.len();
    let m = xlsx_headers.len();

    // 按顺序找匹配前缀
    let mut matched = 0;
    while matched < n && matched < m && tablet_fields[matched].name == xlsx_headers[matched] {
        matched += 1;
    }

    // 匹配前缀之后的列
    let tablet_only = n.saturating_sub(matched);
    let xlsx_only = m.saturating_sub(matched);

    // map: 只在 matched_prefix 内查找
    let col_map: Vec<Option<u32>> = tablet_fields.iter().enumerate().map(|(i, f)| {
        if i < matched {
            Some(i as u32 + 1) // 匹配列直接按位置
        } else {
            // 只在 xlsx 的 matched.. 范围内查找同名列
            xlsx_headers[matched..].iter().position(|h| h == &f.name)
                .map(|j| (matched + j) as u32 + 1)
        }
    }).collect();

    ColumnMatch { matched_prefix: matched, tablet_only, xlsx_only, col_map }
}

pub fn classify_header(xlsx_headers: &[String], tablet_fields: &[crate::model::FieldDef]) -> HeaderMatch {
    if xlsx_headers.is_empty() { return HeaderMatch::Invalid; }
    if xlsx_headers.first().map_or(true, |h| h != "id") { return HeaderMatch::Invalid; }
    let cm = compute_column_match(tablet_fields, xlsx_headers);
    if cm.tablet_only == 0 && cm.xlsx_only == 0 { HeaderMatch::Identical }
    else if cm.matched_prefix > 0 { HeaderMatch::TailDiff }
    else { HeaderMatch::Mismatch }
}

fn map_columns_for_sync(tablet_fields: &[crate::model::FieldDef], xlsx_headers: &[String]) -> Vec<Option<u32>> {
    compute_column_match(tablet_fields, xlsx_headers).col_map
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

        // Read existing xlsx headers
        let (max_col, _max_row) = ws.highest_column_and_row();
        let mut xlsx_headers: Vec<String> = Vec::new();
        for c in 1..=max_col { xlsx_headers.push(ws.value((c, 1u32))); }

        let cm = compute_column_match(fields, &xlsx_headers);
        let insert_at = cm.matched_prefix as u32 + 1; // Insert new cols after matched prefix

        match mode {
            SyncMode::DataOnly => {
                // Only update headers for matched prefix columns
                for i in 0..cm.matched_prefix {
                    ws.cell_mut((i as u32 + 1, 1u32)).set_value(fields[i].name.clone());
                }
            }
            SyncMode::WithColumns | SyncMode::Full => {
                // Update matched prefix
                for i in 0..cm.matched_prefix {
                    ws.cell_mut((i as u32 + 1, 1u32)).set_value(fields[i].name.clone());
                }
                // Insert tablet-only columns after matched prefix
                for i in cm.matched_prefix..fields.len() {
                    ws.cell_mut((insert_at + (i - cm.matched_prefix) as u32, 1u32))
                        .set_value(fields[i].name.clone());
                }
            }
        }

        // Write data rows
        for (ri, row) in table.records.iter().enumerate() {
            let r = ri as u32 + 2;
            match mode {
                SyncMode::DataOnly => {
                    for i in 0..cm.matched_prefix {
                        if i < row.len() { ws.cell_mut((i as u32 + 1, r)).set_value(row[i].clone()); }
                    }
                }
                SyncMode::WithColumns | SyncMode::Full => {
                    // matched prefix data
                    for i in 0..cm.matched_prefix {
                        if i < row.len() { ws.cell_mut((i as u32 + 1, r)).set_value(row[i].clone()); }
                    }
                    // tablet-only columns inserted after matched prefix
                    for i in cm.matched_prefix..row.len() {
                        ws.cell_mut((insert_at + (i - cm.matched_prefix) as u32, r))
                            .set_value(row[i].clone());
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
            // Map xlsx columns to tablet columns by name, only matched prefix
            let cm = compute_column_match(&table.schema.fields, &sheet.headers);
            let n = table.schema.fields.len();
            let records: Vec<Vec<String>> = sheet.rows.iter().map(|xlsx_row| {
                let mut tablet_row = vec![String::new(); n];
                for i in 0..cm.matched_prefix {
                    if i < xlsx_row.len() { tablet_row[i] = xlsx_row[i].clone(); }
                }
                tablet_row
            }).collect();
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
