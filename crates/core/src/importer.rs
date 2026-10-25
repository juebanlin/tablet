//! 外部 Excel → `.tbl` 的模板驱动映射导入。
//!
//! 与原生 xlsx 组不同：这里的工作簿**不是** tablet 的 4 行规则表头，而是策划自己的
//! 表格。`.tblschema` 模板给出目标节点与字段，按字段名匹配列，生成 `.tbl` 文件。
//!
//! 支持：
//! - Table：4 行规则表头（desc/export/type/字段名）或单行列名；
//! - Constant：单行列名 `name / type / value / export / desc`；
//! - Enum：单行列名 `id / name / desc`。
//!
//! 列名匹配大小写不敏感；缺列直接报错，不做静默补空。

use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{ConstEntry, EnumEntry, Export, FieldDef, Table, TableSchema};
use crate::tblschema::{SchemaMode, TblSchema};

/// 把 `xlsx_path` 按 `schema` 的 section 映射成 `.tbl`，写到 `output_dir`。
/// 返回写出的节点数。
pub fn xlsx_to_tbl(schema: &TblSchema, xlsx_path: &Path, output_dir: &Path) -> Result<usize> {
    let mut book = umya_spreadsheet::reader::xlsx::read(xlsx_path)
        .with_context(|| format!("打开 {}", xlsx_path.display()))?;
    std::fs::create_dir_all(output_dir)?;
    let mut written = 0usize;

    for sec in &schema.sections {
        let actual = book.sheet_collection_no_check().iter()
            .find(|s| s.name().eq_ignore_ascii_case(&sec.name))
            .map(|s| s.name().to_string())
            .with_context(|| format!("{} 里没有 sheet {}", xlsx_path.display(), sec.name))?;
        let ws = book.sheet_by_name_mut(&actual)
            .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", actual, e))?;
        let out_path = output_dir.join(format!("{}.tbl", sec.name));

        match sec.mode {
            SchemaMode::Table => {
                let fields: Vec<FieldDef> = sec.fields.iter().map(|f| FieldDef {
                    name: f.name.clone(),
                    desc: f.desc.clone(),
                    tbl_type: f.tbl_type.clone(),
                    export: Export::from_input(&f.export),
                }).collect();
                if fields.is_empty() {
                    anyhow::bail!("[{}] 没有字段，无法映射 Excel", sec.name);
                }
                let (max_col, max_row) = ws.highest_column_and_row();
                let row_has = |row: u32| fields.iter().filter(|f| {
                    (1..=max_col).any(|c| ws.value((c, row)).trim().eq_ignore_ascii_case(&f.name))
                }).count();
                // 规则 4 行表头优先；否则把第 1 行当列名。
                let (header_row, data_start) = if row_has(4) > 0 { (4, 5) } else { (1, 2) };
                if row_has(header_row) != fields.len() {
                    anyhow::bail!("sheet {} 找不到全部字段（第 {} 行列名）", actual, header_row);
                }
                let mut cols = Vec::with_capacity(fields.len());
                for f in &fields {
                    let c = (1..=max_col)
                        .find(|&c| ws.value((c, header_row)).trim().eq_ignore_ascii_case(&f.name))
                        .ok_or_else(|| anyhow::anyhow!("sheet {} 缺少列 {}", actual, f.name))?;
                    cols.push(c);
                }
                let mut records = Vec::new();
                for r in data_start..=max_row {
                    let row: Vec<String> = cols.iter().map(|&c| ws.value((c, r))).collect();
                    if row.iter().all(|v| v.is_empty()) { continue; }
                    records.push(row);
                }
                let table = Table {
                    name: sec.name.clone(),
                    original_name: sec.name.clone(),
                    path: out_path.clone(),
                    schema: TableSchema { fields },
                    records,
                    dirty: false,
                    deleted: false,
                    original_records: Vec::new(),
                    original_fields: Vec::new(),
                    saved: false,
                    fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
                    display_field: None,
                    original_display_field: None,
                    rules: Vec::new(),
                    original_rules: Vec::new(),
                };
                std::fs::write(&out_path, crate::tbl::serialize_table(&table))?;
            }
            SchemaMode::Constant => {
                let headers = ["name", "type", "value", "export", "desc"];
                let mut cols = [0u32; 5];
                for (i, h) in headers.iter().enumerate() {
                    cols[i] = find_col(ws, 1, h)
                        .with_context(|| format!("sheet {} 缺少列 {}", actual, h))?;
                }
                let mut entries = Vec::new();
                for r in 2..=ws.highest_row() {
                    let name = ws.value((cols[0], r));
                    if name.trim().is_empty() { continue; }
                    entries.push(ConstEntry {
                        name: name.trim().to_string(),
                        tbl_type: ws.value((cols[1], r)).trim().to_string(),
                        value: ws.value((cols[2], r)),
                        export: Export::from_input(&ws.value((cols[3], r))),
                        desc: ws.value((cols[4], r)),
                    });
                }
                let constant = crate::model::Constant {
                    name: sec.name.clone(),
                    original_name: sec.name.clone(),
                    path: out_path.clone(),
                    original_entries: entries.clone(),
                    entries,
                    dirty: false,
                    deleted: false,
                    saved: false,
                };
                std::fs::write(&out_path, crate::tbl::serialize_constant(&constant))?;
            }
            SchemaMode::Enum => {
                let headers = ["id", "name", "desc"];
                let mut cols = [0u32; 3];
                for (i, h) in headers.iter().enumerate() {
                    cols[i] = find_col(ws, 1, h)
                        .with_context(|| format!("sheet {} 缺少列 {}", actual, h))?;
                }
                let mut entries = Vec::new();
                for r in 2..=ws.highest_row() {
                    let id = ws.value((cols[0], r));
                    let name = ws.value((cols[1], r));
                    if id.trim().is_empty() && name.trim().is_empty() { continue; }
                    entries.push(EnumEntry {
                        id: id.trim().to_string(),
                        name: name.trim().to_string(),
                        desc: ws.value((cols[2], r)),
                    });
                }
                let enum_def = crate::model::EnumDef {
                    name: sec.name.clone(),
                    original_name: sec.name.clone(),
                    path: out_path.clone(),
                    original_entries: entries.clone(),
                    entries,
                    dirty: false,
                    deleted: false,
                    saved: false,
                    display_field: None,
                    original_display_field: None,
                    rules: Vec::new(),
                    original_rules: Vec::new(),
                };
                std::fs::write(&out_path, crate::tbl::serialize_enum(&enum_def))?;
            }
        }
        written += 1;
    }
    Ok(written)
}

fn find_col(ws: &umya_spreadsheet::Worksheet, row: u32, name: &str) -> Option<u32> {
    (1..=ws.highest_column()).find(|&c| ws.value((c, row)).trim().eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tblschema::{SchemaField, SchemaMode, SchemaSection};

    #[test]
    fn maps_one_row_header_excel_to_tbl() {
        let dir = std::env::temp_dir().join(format!(
            "tablet_importer_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let xlsx = dir.join("foreign.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        ws.write_string(0, 0, "id").unwrap();
        ws.write_string(0, 1, "name").unwrap();
        ws.write_string(1, 0, "1").unwrap();
        ws.write_string(1, 1, "战士").unwrap();
        std::fs::write(&xlsx, wb.save_to_buffer().unwrap()).unwrap();

        let schema = TblSchema {
            meta: Default::default(),
            separators: Default::default(),
            sections: vec![SchemaSection {
                group: "hero".into(),
                name: "HeroBase".into(),
                mode: SchemaMode::Table,
                storage: crate::model::GroupStorage::Tbl,
                fields: vec![
                    SchemaField { name: "id".into(), tbl_type: "int".into(), export: "cs".into(), desc: "ID".into() },
                    SchemaField { name: "name".into(), tbl_type: "str".into(), export: "cs".into(), desc: "名称".into() },
                ],
                preset: Vec::new(),
                display: None,
                rules: Vec::new(),
            }],
        };
        let out = dir.join("out");
        assert_eq!(xlsx_to_tbl(&schema, &xlsx, &out).unwrap(), 1);

        let parsed = crate::tbl::parse_tbl(&out.join("HeroBase.tbl")).expect("parse tbl");
        let crate::tbl::TblFile::Table(t) = parsed else { panic!("expected table") };
        assert_eq!(t.schema.fields.len(), 2);
        assert_eq!(t.schema.fields[1].name, "name");
        assert_eq!(t.records, vec![vec!["1".to_string(), "战士".to_string()]]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
