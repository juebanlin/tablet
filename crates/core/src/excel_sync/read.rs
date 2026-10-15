//! 两侧的投影：`Group` / `Workbook` → [`SheetVo`]。
//!
//! 这一层只做「读出来、摆整齐」，不做任何合并决策（那是 `plan` 的事）。
//!
//! 读取用 umya-spreadsheet 的全量 `read`——它只用于**旁观**（算差异、给面板显示），
//! 永远不写回，所以把每张 sheet 都反序列化没有代价。真正要写回的走 `apply` 里的
//! `lazy_read`，两者不能混（混了就会让没被同步的 sheet 白白走一遍再序列化）。

use std::path::Path;

use anyhow::{Context, Result};
use umya_spreadsheet::Worksheet;

use crate::model::{Group, Table};

use super::vo::*;

// ── tbl 侧 ──

/// `Table` → VO。tbl 侧永远是「四样都有」，没有布局之说。
pub fn tbl_sheet_vo(table: &Table) -> SheetVo {
    let fields = &table.schema.fields;

    let columns: Vec<ColumnVo> = fields.iter().enumerate().map(|(i, f)| ColumnVo {
        index: i as u32 + 1,
        name: Some(f.name.clone()),
        desc: Some(f.desc.clone()),
        ty: Some(f.tbl_type.clone()),
        // 与 xlsx 侧的写法一致（中文展示值），否则两边比不齐
        export: Some(f.export.display().to_string()),
    }).collect();

    let rows: Vec<RowVo> = table.records.iter().enumerate().map(|(i, r)| {
        let mut cells = vec![String::new(); fields.len()];
        for (c, v) in r.iter().enumerate() {
            if c < cells.len() { cells[c] = v.clone(); }
        }
        RowVo { index: DATA_START + i as u32, cells }
    }).collect();

    let max_row = if rows.is_empty() { DATA_START - 1 } else { DATA_START + rows.len() as u32 - 1 };
    SheetVo {
        name: table.name.clone(),
        columns,
        rows,
        range: SheetRange { max_col: fields.len() as u32, max_row },
    }
}

// ── xlsx 侧 ──

/// 读整个 xlsx → 每个 sheet 一个 VO。
///
/// `group` 用来判定表头布局（列名行必然与 schema 的字段名一致，这是唯一可靠的锚点）。
/// 判不出来会出事：旧文件按新布局读会把 3 行数据当表头吃掉、整体错位。
pub fn read_xlsx_sheets(path: &Path, group: Option<&Group>) -> Result<Vec<SheetVo>> {
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    let mut sheets = Vec::new();
    for idx in 0..book.sheet_collection_no_check().len() {
        let ws = book.sheet_collection_no_check().get(idx)
            .with_context(|| format!("获取第 {} 个 sheet 失败", idx))?;
        sheets.push(sheet_vo(ws, group));
    }
    Ok(sheets)
}

fn sheet_vo(ws: &Worksheet, group: Option<&Group>) -> SheetVo {
    let name = ws.name().to_string();
    let expected: Option<&[crate::model::FieldDef]> = group
        .and_then(|g| g.tables.iter().find(|t| t.name == name))
        .map(|t| t.schema.fields.as_slice());
    let layout = detect_layout(ws, expected);

    let (max_col, max_row) = ws.highest_column_and_row();
    let range = SheetRange { max_col, max_row };

    // 列的区域由**列名行**界定：最后一个非空的格是第几列，就有几列。
    // 不能拿 `max_col` 当列数——策划在 Z 列随手写一条备注就会把表头补成一串空格子，
    // 面板会据此判成「xlsx 多列」而只给一个「强制→」。区域外的东西记在 range 里。
    let name_row = layout.name_row();
    let last_col = if max_row >= name_row {
        (1..=max_col).rev().find(|&c| !ws.value((c, name_row)).is_empty()).unwrap_or(0)
    } else {
        0
    };

    let modern = layout == Layout::Modern;
    let header_cell = |c: u32, row: u32| -> Option<String> {
        if modern && max_row >= row { Some(ws.value((c, row))) } else { None }
    };

    let columns: Vec<ColumnVo> = (1..=last_col).map(|c| ColumnVo {
        index: c,
        name: Some(ws.value((c, name_row))),
        desc: header_cell(c, HDR_DESC),
        ty: header_cell(c, HDR_TYPE),
        export: header_cell(c, HDR_EXPORT),
    }).collect();

    // 行只收有效行：尾部空行不进；中间的整行空白也不进（tablet 侧没有「空记录」
    // 这个概念），但 index 保留真实行号，所以位置不会丢。
    let mut rows: Vec<RowVo> = Vec::new();
    let first_data = layout.data_start();
    if max_row >= first_data {
        for r in first_data..=max_row {
            let cells: Vec<String> = (1..=last_col).map(|c| ws.value((c, r))).collect();
            if cells.iter().all(|v| v.is_empty()) { continue; }
            rows.push(RowVo { index: r, cells });
        }
    }

    SheetVo { name, columns, rows, range }
}

/// 这份 sheet 是 4 行表头还是旧的 1 行表头。
///
/// `expected` 为 `None`（group 里没有同名表）时按旧布局读：那种 sheet 在面板上只
/// 显示「(新 sheet)」，行列数是硬编码 0，读法不参与任何决策——按旧布局读可以避免
/// 把 desc 行错当成列名。
pub fn detect_layout(ws: &Worksheet, expected: Option<&[crate::model::FieldDef]>) -> Layout {
    let Some(fields) = expected else { return Layout::Legacy };
    if fields.is_empty() { return Layout::Legacy; }

    let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    if row_is(&ws, HDR_NAME, &names) {
        Layout::Modern
    } else if row_is(&ws, LEGACY_HEADER_ROW, &names) {
        Layout::Legacy
    } else {
        // 两种都对不上 = 列名被改坏了。按新布局读，交给面板照常报「表头不符合规范」，
        // 用户走「强制→」即可自愈。
        Layout::Modern
    }
}

/// 这一行是不是恰好是给定的名字（逐列比对，只看前 `names.len()` 列）。
fn row_is(ws: &Worksheet, row: u32, names: &[&str]) -> bool {
    let (max_col, max_row) = ws.highest_column_and_row();
    if max_row < row || max_col < names.len() as u32 { return false; }
    names.iter().enumerate().all(|(i, n)| ws.value((i as u32 + 1, row)) == *n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::excel_sync::testutil::*;

    /// 手造一份**新布局**（4 行表头）文件。`records` 里的空 vec 会写成整行空白，
    /// 用来验证「空行不进 rows，但行号不丢」。
    fn write_modern(path: &Path, records: &[Vec<&str>]) {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        for (i, (n, d, t)) in FIELD_SPEC.iter().enumerate() {
            let c = i as u16;
            ws.write_string(HDR_DESC - 1, c, *d).unwrap();
            ws.write_string(HDR_EXPORT - 1, c, "前后端").unwrap();
            ws.write_string(HDR_TYPE - 1, c, *t).unwrap();
            ws.write_string(HDR_NAME - 1, c, *n).unwrap();
        }
        for (r, row) in records.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32 + DATA_START - 1, c as u16, *v).unwrap();
            }
        }
        std::fs::write(path, wb.save_to_buffer().unwrap()).unwrap();
    }

    /// 手造一份**旧布局**（只有 1 行列名，数据从第 2 行起）文件。
    fn write_legacy(path: &Path, records: &[Vec<&str>]) {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        for (i, (n, _, _)) in FIELD_SPEC.iter().enumerate() {
            ws.write_string(0, i as u16, *n).unwrap();
        }
        for (r, row) in records.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32 + 1, c as u16, *v).unwrap();
            }
        }
        std::fs::write(path, wb.save_to_buffer().unwrap()).unwrap();
    }

    #[test]
    fn modern_layout_reads_header_and_rows() {
        let g = group(data());
        let tmp = TempXlsx::new();
        write_modern(tmp.path(), &data());

        let sheets = read_xlsx_sheets(tmp.path(), Some(&g)).unwrap();
        let s = &sheets[0];
        assert_eq!(s.name, "HeroBase");
        assert_eq!(s.columns.len(), 2);
        assert_eq!(s.columns[0].name.as_deref(), Some("id"));
        assert_eq!(s.columns[0].desc.as_deref(), Some("编号"));
        assert_eq!(s.columns[0].ty.as_deref(), Some("int"));
        assert_eq!(s.columns[0].export.as_deref(), Some("前后端"));
        assert!(s.has_desc_row());

        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.rows[0].index, DATA_START, "行号要带真实的物理行号");
        assert_eq!(s.rows[0].cells, vec!["1", "alice"]);
    }

    #[test]
    fn legacy_layout_has_no_desc_row() {
        let g = group(data());
        let tmp = TempXlsx::new();
        write_legacy(tmp.path(), &data());

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert_eq!(s.columns.len(), 2);
        assert_eq!(s.columns[0].name.as_deref(), Some("id"), "旧布局的列名在第 1 行");
        assert!(
            s.columns.iter().all(|c| c.desc.is_none()),
            "旧布局没有 desc 行——回读时这一格都不该动"
        );
        assert!(!s.has_desc_row());
        assert_eq!(s.rows[0].index, LEGACY_DATA_START);
        assert_eq!(s.rows.len(), 2, "数据不能被当成表头吃掉");
    }

    /// 整行空白不进 `rows`，但**行号不能丢**：以前 `rows[i]` 被当成 `data_start + i`，
    /// 中间夹一个空行，后面所有行都错位一格，而推送是密集写的——那个空行会被永久
    /// 压掉且看不出来。
    #[test]
    fn blank_row_keeps_real_row_index() {
        let g = group(data());
        let tmp = TempXlsx::new();
        // 第 5 行有数据、第 6 行整行空白、第 7 行有数据
        write_modern(tmp.path(), &[
            vec!["1", "alice"],
            vec![],
            vec!["2", "bob"],
        ]);

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert_eq!(s.rows.len(), 2, "空白行不进 VO");
        assert_eq!(s.rows[0].index, DATA_START);
        assert_eq!(s.rows[1].index, DATA_START + 2, "行号要跳过那个空行");
        assert_eq!(s.rows[1].cells, vec!["2", "bob"]);
    }

    /// 区域由**列名行**界定。以前拿的是全表最大列：策划在 Z 列写一条备注就会把表头
    /// 补成一串空格子、让面板判成「xlsx 多列」而只给一个「强制→」。
    #[test]
    fn content_outside_header_region_is_out_of_the_vo() {
        use rust_xlsxwriter::Workbook;
        let g = group(data());
        let tmp = TempXlsx::new();
        write_modern(tmp.path(), &data());

        // 在第 3 列（表头只有 2 列）塞一条备注
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((3, DATA_START)).set_value("随手记一笔");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }
        let _ = Workbook::new();

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert_eq!(s.columns.len(), 2, "列数由列名行界定，表头之外的格子不进 VO");
        assert_eq!(s.rows[0].cells, vec!["1", "alice"], "区域外的格子不混进来");
        assert_eq!(s.range.max_col, 3, "区域外的事实不能丢——记在 range 里");
        assert!(s.has_content_outside_region());
    }

    /// desc 行存在但内容全空，与「根本没有 desc 行」是两回事。
    #[test]
    fn empty_desc_cells_still_count_as_a_desc_row() {
        use rust_xlsxwriter::Workbook;
        let g = group(data());
        let tmp = TempXlsx::new();
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        for (i, (n, _, t)) in FIELD_SPEC.iter().enumerate() {
            let c = i as u16;
            ws.write_string(HDR_DESC - 1, c, "").unwrap(); // desc 行在，但内容是空的
            ws.write_string(HDR_EXPORT - 1, c, "前后端").unwrap();
            ws.write_string(HDR_TYPE - 1, c, *t).unwrap();
            ws.write_string(HDR_NAME - 1, c, *n).unwrap();
        }
        for (r, row) in data().iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32 + DATA_START - 1, c as u16, *v).unwrap();
            }
        }
        std::fs::write(tmp.path(), wb.save_to_buffer().unwrap()).unwrap();

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert!(s.has_desc_row(), "desc 行存在，只是内容为空");
        assert_eq!(s.columns[0].desc.as_deref(), Some(""));
    }
}
