//! Excel 同步核心引擎（umya-spreadsheet 编辑保留格式化）。
//!
//! Table sheet 的表头是 **4 行**，与 [`crate::excel::write_table`] 逐格一致：
//! 第 1 行 desc / 第 2 行 export / 第 3 行 type / 第 4 行**列名**，数据从第 5 行起。
//! 策划在 Excel 里只该改 desc 与数据——列名是列匹配的依据（@06 §7），
//! 颜色 / 行高列宽随便调，同步时不碰。
//!
//! 推送（[`sync_group_to_xlsx`]）走 `lazy_read`：只有被写到的 sheet 会被反序列化，
//! 其余 sheet 在写回时是**原始字节直通**（umya `writer/xlsx.rs` 按 `is_deserialized()`
//! 分流）。所以策划在别的 sheet 上做的一切原样保留，代价只是被同步的那几张表会
//! 丢掉 umya 没建模的元素（行/列分组折叠、已应用的筛选条件等，@06 §7.4）。

use std::path::Path;

use anyhow::{Context, Result};
use umya_spreadsheet::Worksheet;

use crate::model::{FieldDef, Group};

// ── 表头布局 ──

/// Table sheet 的 4 行表头。顺序是**约定**：列名行必须在最后一行，列匹配靠它。
const HDR_DESC: u32 = 1;
const HDR_EXPORT: u32 = 2;
const HDR_TYPE: u32 = 3;
const HDR_NAME: u32 = 4;
/// 数据行起点
const DATA_START: u32 = 5;

/// 旧布局：本特性之前生成的文件只有 1 行列名，数据从第 2 行起。
const LEGACY_HEADER_ROWS: u32 = 1;
const LEGACY_DATA_START: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layout {
    /// desc / export / type / 列名 四行
    Modern,
    /// 只有列名一行
    Legacy,
}

impl Layout {
    fn name_row(self) -> u32 {
        match self {
            Layout::Modern => HDR_NAME,
            Layout::Legacy => LEGACY_HEADER_ROWS,
        }
    }

    fn data_start(self) -> u32 {
        match self {
            Layout::Modern => DATA_START,
            Layout::Legacy => LEGACY_DATA_START,
        }
    }
}

// ── types ──

pub struct XlsxSheet {
    pub name: String,
    /// 列名行。新布局 = 第 4 行；旧布局 = 第 1 行
    pub headers: Vec<String>,
    /// desc 行。新布局 = 第 1 行；旧布局 = `None`。
    ///
    /// 这里必须用 `Option` 而不是空 vec：空 vec 在回读时会把 tbl 里所有 desc
    /// 抹成空串，而"这份文件没有 desc 行、这一轮别动 desc"是**另一个**语义。
    pub descs: Option<Vec<String>>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, PartialEq, Debug)]
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
//
// 注意这条读法只用于**旁观**（算差异、给面板显示），永远配全量 `read`；
// 真正要写回的走 `sync_group_to_xlsx` 里的 `lazy_read`，两者不能混。

pub fn read_xlsx_sheets(path: &Path, group: Option<&Group>) -> Result<Vec<XlsxSheet>> {
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    let mut sheets = Vec::new();
    let sheet_count = book.sheet_collection_no_check().len();

    for idx in 0..sheet_count {
        let worksheet = book.sheet_collection_no_check().get(idx)
            .with_context(|| format!("获取第 {} 个 sheet 失败", idx))?;
        let name = worksheet.name().to_string();
        let (max_col, max_row) = worksheet.highest_column_and_row();
        let layout = detect_layout(worksheet, group, &name);

        let read_row = |r: u32| -> Vec<String> {
            (1..=max_col).map(|c| worksheet.value((c, r))).collect()
        };

        let name_row = layout.name_row();
        let headers = if max_row >= name_row { read_row(name_row) } else { Vec::new() };

        let descs = match layout {
            Layout::Modern if max_row >= HDR_DESC => Some(read_row(HDR_DESC)),
            _ => None,
        };

        let first_data = layout.data_start();
        let mut data_rows: Vec<Vec<String>> = Vec::new();
        if max_row >= first_data {
            for r in first_data..=max_row {
                let mut cells = Vec::new();
                let mut all_empty = true;
                for c in 1..=max_col {
                    let v = worksheet.value((c, r));
                    if !v.is_empty() { all_empty = false; }
                    cells.push(v);
                }
                if all_empty { continue; }
                while cells.last().map_or(false, |c| c.is_empty()) { cells.pop(); }
                data_rows.push(cells);
            }
        }
        sheets.push(XlsxSheet { name, headers, descs, rows: data_rows });
    }
    Ok(sheets)
}

/// 这份 sheet 是 4 行表头还是旧的 1 行表头。
///
/// 判不出来会出事：旧文件按新布局读会把 3 行数据当表头吃掉，数据整体错位。
/// 判定靠 schema 的字段名——列名行必然与之一致，这是唯一可靠的锚点。
fn detect_layout(ws: &Worksheet, group: Option<&Group>, sheet_name: &str) -> Layout {
    let Some(fields) = group
        .and_then(|g| g.tables.iter().find(|t| t.name == sheet_name))
        .map(|t| &t.schema.fields)
    else {
        // group 里没有同名表的 xlsx 独有 sheet：面板只用它显示「(新 sheet)」，
        // right_rows / right_cols 是硬编码 0，读法不参与任何决策 → 按旧布局读，
        // 免得把 desc 行错当成列名。
        return Layout::Legacy;
    };

    if row_is_field_names(ws, HDR_NAME, fields) {
        Layout::Modern
    } else if row_is_field_names(ws, LEGACY_HEADER_ROWS, fields) {
        Layout::Legacy
    } else {
        // 两种都对不上 = 列名被改坏了。按新布局读，交给面板照常报「表头不符合规范」，
        // 用户走「强制→」即可自愈。
        Layout::Modern
    }
}

/// 这一行是不是恰好是 schema 的字段名（逐列比对前 `fields.len()` 列）。
fn row_is_field_names(ws: &Worksheet, row: u32, fields: &[FieldDef]) -> bool {
    if fields.is_empty() { return false; }
    let (max_col, max_row) = ws.highest_column_and_row();
    if max_row < row || max_col < fields.len() as u32 { return false; }
    fields.iter().enumerate().all(|(i, f)| ws.value((i as u32 + 1, row)) == f.name)
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

    // 文件还不存在 → 没有可保留的东西，交给只写的 rust_xlsxwriter 造一份标准的。
    // 注意 `Full`（强制→）**不**走这里：文件已存在时它就地重写，否则策划在这个
    // 文件上刷的颜色、调的高宽、自建的 sheet 会被整个新 workbook 顶掉。
    if !path.exists() {
        return write_fresh_xlsx(path, group);
    }

    // lazy_read：只有下面 `sheet_by_name_mut` 碰过的 sheet 会被反序列化，其余 sheet
    // 写回时走 raw 字节直通。所以这个函数里**不要**为了"看一眼"去调 sheet()/sheet_mut()，
    // 那会把那张 sheet 拖下水、连带丢掉 umya 没建模的元素。
    let mut book = umya_spreadsheet::reader::xlsx::lazy_read(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    for table in &group.tables {
        if table.deleted { continue; }
        let sheet_name = table.name.clone();

        // 存在性必须问 sheet_collection_no_check()：`sheet_by_name()` 走的是
        // `Workbook::sheet()`，它对**没反序列化**的 sheet 一律返回 NotDeserialized，
        // 在 lazy book 上会骗我们「这张表不存在」，然后 new_sheet 撞 NameDuplicate。
        // 名字在读取时就填好了，不查内容不算「碰」。
        let exists = book.sheet_collection_no_check().iter().any(|s| s.name() == sheet_name);
        if !exists {
            book.new_sheet(&sheet_name)?;
        }
        let ws = book.sheet_by_name_mut(&sheet_name)
            .map_err(|_| anyhow::anyhow!("sheet '{}' 不存在", sheet_name))?;

        let fields = &table.schema.fields;

        // 旧布局就地升级成 4 行表头（不重建整表，策划的颜色/列宽/行高要留着）
        if detect_layout(ws, Some(group), &sheet_name) == Layout::Legacy {
            upgrade_legacy_layout(ws);
        }

        let (max_col, _) = ws.highest_column_and_row();
        let xlsx_headers: Vec<String> = (1..=max_col).map(|c| ws.value((c, HDR_NAME))).collect();
        let cm = compute_column_match(fields, &xlsx_headers);

        // tablet 第 i 列落到 xlsx 第几列。匹配前缀内按位置；匹配前缀之外的（tablet
        // 独有列）只在补列 / 强制模式下落笔，落在匹配前缀正后面（追加，不插入——
        // 这两种模式下列名已经决定了 xlsx 侧没有多余列）。
        let insert_at = cm.matched_prefix as u32 + 1;
        let col_of = |i: usize| -> Option<u32> {
            if i < cm.matched_prefix {
                Some(i as u32 + 1)
            } else if matches!(mode, SyncMode::WithColumns | SyncMode::Full) {
                Some(insert_at + (i - cm.matched_prefix) as u32)
            } else {
                None
            }
        };

        // 表头 4 行
        for (i, f) in fields.iter().enumerate() {
            if let Some(c) = col_of(i) { write_header(ws, c, f); }
        }

        // 数据行
        for (ri, row) in table.records.iter().enumerate() {
            let r = ri as u32 + DATA_START;
            for (i, val) in row.iter().enumerate() {
                if let Some(c) = col_of(i) {
                    ws.cell_mut((c, r)).set_value(val.clone());
                }
            }
        }

        if mode == SyncMode::Full {
            trim_to_schema(ws, fields.len(), table.records.len());
        }
    }

    umya_spreadsheet::writer::xlsx::write(&book, path)
        .with_context(|| format!("写入 xlsx 失败: {}", path.display()))?;
    Ok(())
}

/// 往一个格子里写表头的四行。
fn write_header(ws: &mut Worksheet, col: u32, f: &FieldDef) {
    ws.cell_mut((col, HDR_DESC)).set_value(f.desc.clone());
    ws.cell_mut((col, HDR_EXPORT)).set_value(f.export.display().to_string());
    ws.cell_mut((col, HDR_TYPE)).set_value(f.tbl_type.clone());
    ws.cell_mut((col, HDR_NAME)).set_value(f.name.clone());
}

/// 旧布局原地升级：顶部插 3 行，原来的列名行（第 1 行）顺移到第 4 行，正好落在新
/// 布局的列名位；再补上 desc / export / type 三行。
///
/// `insert_new_row` 只挪坐标、不搬样式，新插的三行是裸的 —— 必须从已就位的列名行
/// 显式复制样式，否则升级完表头会突然掉样式。
fn upgrade_legacy_layout(ws: &mut Worksheet) {
    ws.insert_new_row(1, 3);
    let end_col = ws.highest_column().max(1);
    for target in [HDR_DESC, HDR_EXPORT, HDR_TYPE] {
        ws.copy_row_styling(HDR_NAME, target, Some(1), Some(end_col));
    }
}

/// `Full`（强制→）的就地清理：把 `keep_cols` 之外的列、`DATA_START + keep_rows` 之后
/// 的数据行删掉，让 xlsx 的结构与 tablet 严格对齐。
///
/// 用 `remove_cell` 而不是往格子里写空串：写空串会留下一个"存在"的格，
/// `highest_column_and_row()` 照算，同步面板的列数会一直虚高。
fn trim_to_schema(ws: &mut Worksheet, keep_cols: usize, keep_rows: usize) {
    let (max_col, max_row) = ws.highest_column_and_row();
    for c in (keep_cols as u32 + 1)..=max_col {
        for r in 1..=max_row {
            ws.remove_cell((c, r));
        }
    }
    let first_extra = DATA_START + keep_rows as u32;
    let (_, max_row) = ws.highest_column_and_row();
    if max_row >= first_extra {
        ws.remove_row(first_extra, max_row - first_extra + 1);
    }
}

/// 从零造一份：4 行表头（desc / export / type / 列名，与 `excel.rs::write_table` 同款
/// 样式）+ 数据，冻结前 4 行。
///
/// 不加 sheet protection：列名靠约定而非锁死（@06 §7），而这条流后续用 umya 原地
/// 编辑，umya 不认识 `<protectedRanges>`、会把它静默丢掉，只剩保护开关 —— 那就成了
/// 整张只读，策划连数据都改不了。
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
            let c = i as u16;
            ws.write_string_with_format(HDR_DESC - 1, c, &f.desc, &hdr)?;
            ws.write_string_with_format(HDR_EXPORT - 1, c, f.export.display(), &hdr)?;
            ws.write_string_with_format(HDR_TYPE - 1, c, &f.tbl_type, &hdr)?;
            ws.write_string_with_format(HDR_NAME - 1, c, &f.name, &hdr)?;
        }
        for (r, row) in table.records.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                ws.write_string_with_format(r as u32 + DATA_START - 1, c as u16, val, &bdy)?;
            }
        }
        ws.set_freeze_panes(DATA_START, 0)?;
    }
    let buf = wb.save_to_buffer()?;
    std::fs::write(path, buf)?;
    Ok(())
}

// ── sync: xlsx → tablet ──

/// 单张表的回读结果。
pub struct TablePatch {
    pub name: String,
    /// 与 tablet schema 的列一一对应（长度 = `schema.fields.len()`）。
    pub records: Vec<Vec<String>>,
    /// 新的 desc，**长度 = 匹配前缀列数**（回读只认匹配上的那几列，与 records 的
    /// 取值规则一致）。
    ///
    /// `None` = 这份 sheet 没有 desc 行（旧布局），这一轮别动 desc。
    /// 新布局下空串是**有效值**：策划看得见第 1 行，清空就是他的意思。
    pub descs: Option<Vec<String>>,
}

pub struct GroupPatch {
    pub tables: Vec<TablePatch>,
}

pub fn read_group_from_xlsx(path: &Path, group: &Group, force: bool) -> Result<GroupPatch> {
    let sheets = read_xlsx_sheets(path, Some(group))?;
    let mut tables = Vec::new();
    for sheet in &sheets {
        let Some(table) = group.tables.iter().find(|t| t.name == sheet.name) else { continue };
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
        // desc 同样只取匹配前缀：匹配列按位置一一对应，前缀之外的列本来就不参与同步
        let descs = sheet.descs.as_ref().map(|d| {
            (0..cm.matched_prefix)
                .map(|i| d.get(i).cloned().unwrap_or_default())
                .collect()
        });
        tables.push(TablePatch { name: sheet.name.clone(), records, descs });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Export, Table, TableSchema};
    use crate::tbl::DEFAULT_TBL_VERSION;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    const FIELDS: [(&str, &str, &str, Export); 2] = [
        ("id", "编号", "int", Export::ClientServer),
        ("name", "名字", "str", Export::ClientServer),
    ];

    fn fields() -> Vec<FieldDef> {
        FIELDS.iter().map(|(n, d, t, e)| FieldDef {
            name: n.to_string(), desc: d.to_string(), tbl_type: t.to_string(), export: e.clone(),
        }).collect()
    }

    fn table(name: &str, records: Vec<Vec<&str>>) -> Table {
        let f = fields();
        Table {
            name: name.to_string(),
            path: PathBuf::from(format!("{}.tbl", name)),
            schema: TableSchema { fields: f.clone() },
            records: records.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect(),
            dirty: false, deleted: false,
            original_records: vec![],
            original_fields: f,
            saved: false,
            fmt_ver: DEFAULT_TBL_VERSION,
            display_field: None,
        }
    }

    fn group(records: Vec<Vec<&str>>) -> Group {
        Group {
            name: "hero".into(),
            dir: PathBuf::from("hero"),
            tables: vec![table("HeroBase", records)],
            constants: vec![], enums: vec![], is_new: false,
        }
    }

    /// 临时 xlsx：drop 时自动删除。
    struct TempXlsx(PathBuf);
    impl TempXlsx {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir()
                .join(format!("tablet_excel_sync_{}_{}.xlsx", std::process::id(), n));
            Self(path)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for TempXlsx {
        fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
    }

    fn data() -> Vec<Vec<&'static str>> {
        vec![vec!["1", "alice"], vec!["2", "bob"]]
    }

    /// 直接用手造的旧布局文件（只有 1 行列名，数据从第 2 行起）。
    fn write_legacy(path: &Path, records: &[Vec<&str>]) {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        for (i, (n, _, _, _)) in FIELDS.iter().enumerate() {
            ws.write_string(0, i as u16, *n).unwrap();
        }
        for (r, row) in records.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32 + 1, c as u16, *v).unwrap();
            }
        }
        std::fs::write(path, wb.save_to_buffer().unwrap()).unwrap();
    }

    fn open(path: &Path) -> umya_spreadsheet::Workbook {
        umya_spreadsheet::reader::xlsx::read(path).unwrap()
    }

    /// 直接取 xlsx（zip）里某个条目的原始字节。
    fn zip_entry(path: &Path, name: &str) -> Vec<u8> {
        use std::io::Read;
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut entry = archive.by_name(name).unwrap_or_else(|_| panic!("zip 里没有 {name}"));
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).unwrap();
        buf
    }

    /// 用 **rust_xlsxwriter** 造一份文件（表 1 = group 里的表，表 2 = 策划自建的
    /// 额外 sheet）。
    ///
    /// 关键在"谁写的"：umya 重新序列化**自己的**输出是幂等的、字节不变，所以拿
    /// umya 造的 sheet 去测「有没有被重写」永远测不出来（这条踩过一次）。必须用
    /// 另一个库写，umya 的再序列化才会产生不同的字节。
    fn write_two_sheet_book(path: &Path, records: &[Vec<&str>]) {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        {
            let ws = wb.add_worksheet();
            ws.set_name("HeroBase").unwrap();
            for (i, (n, d, t, _)) in FIELDS.iter().enumerate() {
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
        }
        {
            let ws = wb.add_worksheet();
            ws.set_name("策划备注").unwrap();
            ws.write_string(0, 0, "别删我").unwrap();
            ws.write_string(1, 0, "第二行").unwrap();
        }
        std::fs::write(path, wb.save_to_buffer().unwrap()).unwrap();
    }

    /// 这条才是 `lazy_read` 的真正断言，别的保真用例在全量 `read` 下也会通过。
    ///
    /// 机制：写回时 umya 按 `is_deserialized()` 分流，没被碰过的 sheet 走
    /// `raw_copy_file_rename`——zip 层的**原始压缩字节直通**，所以 `sheet2.xml`
    /// 应当逐字节不变。把 `sync_group_to_xlsx` 里的 `lazy_read` 换回 `read`，
    /// 这条立刻红（已实测）。
    #[test]
    fn untouched_sheet_is_copied_byte_for_byte() {
        let g = group(data());
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &data());

        let before = zip_entry(tmp.path(), "xl/worksheets/sheet2.xml");
        let hero_before = zip_entry(tmp.path(), "xl/worksheets/sheet1.xml");
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();
        let after = zip_entry(tmp.path(), "xl/worksheets/sheet2.xml");

        // 对照组先断：同一次推送里，group 那张表确实被重写了。没有它，上面那条
        // 可能因为「整个文件根本没动」而假绿。
        assert_ne!(
            zip_entry(tmp.path(), "xl/worksheets/sheet1.xml"),
            hero_before,
            "被同步的 sheet 应该被重写"
        );
        assert_eq!(before, after, "没被同步的 sheet 必须是原始字节直通，不该被重新序列化");
    }

    #[test]
    fn fresh_book_writes_four_row_header() {
        let g = group(data());
        let tmp = TempXlsx::new();
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();

        let sheets = read_xlsx_sheets(tmp.path(), Some(&g)).unwrap();
        assert_eq!(sheets.len(), 1);
        let s = &sheets[0];
        assert_eq!(s.name, "HeroBase");
        assert_eq!(s.headers, vec!["id", "name"]);
        assert_eq!(s.descs.as_deref(), Some(&["编号".to_string(), "名字".to_string()][..]));
        assert_eq!(s.rows, vec![vec!["1".to_string(), "alice".to_string()],
                                vec!["2".to_string(), "bob".to_string()]]);

        // 表头四行逐格核对（export 走中文展示值）
        let book = open(tmp.path());
        let ws = book.sheet_by_name("HeroBase").unwrap();
        assert_eq!(ws.value((1, HDR_DESC)), "编号");
        assert_eq!(ws.value((1, HDR_EXPORT)), "前后端");
        assert_eq!(ws.value((1, HDR_TYPE)), "int");
        assert_eq!(ws.value((1, HDR_NAME)), "id");
    }

    #[test]
    fn legacy_layout_is_recognised() {
        let g = group(data());
        let tmp = TempXlsx::new();
        write_legacy(tmp.path(), &data());

        let sheets = read_xlsx_sheets(tmp.path(), Some(&g)).unwrap();
        let s = &sheets[0];
        assert_eq!(s.headers, vec!["id", "name"], "旧布局的列名在第 1 行");
        assert_eq!(s.descs, None, "旧布局没有 desc 行，必须报 None 而不是空 vec");
        assert_eq!(s.rows.len(), 2, "数据不能被当成表头吃掉");
        assert_eq!(s.rows[0], vec!["1".to_string(), "alice".to_string()]);
    }

    #[test]
    fn push_upgrades_legacy_layout_in_place() {
        let g = group(data());
        let tmp = TempXlsx::new();
        write_legacy(tmp.path(), &data());

        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();

        let sheets = read_xlsx_sheets(tmp.path(), Some(&g)).unwrap();
        let s = &sheets[0];
        assert_eq!(s.descs.as_deref(), Some(&["编号".to_string(), "名字".to_string()][..]),
                   "升级后应该有 desc 行");
        assert_eq!(s.headers, vec!["id", "name"]);
        assert_eq!(s.rows, vec![vec!["1".to_string(), "alice".to_string()],
                                vec!["2".to_string(), "bob".to_string()]],
                   "升级不能吃掉原有数据");
    }

    #[test]
    fn round_trip_carries_desc_and_data() {
        let g = group(data());
        let tmp = TempXlsx::new();
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();

        // 策划改：desc 第 2 列、数据第 1 行第 2 列
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((2, HDR_DESC)).set_value("角色名");
            ws.cell_mut((2, DATA_START)).set_value("carol");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        let patch = read_group_from_xlsx(tmp.path(), &g, false).unwrap();
        assert_eq!(patch.tables.len(), 1);
        let t = &patch.tables[0];
        assert_eq!(t.descs.as_deref(), Some(&["编号".to_string(), "角色名".to_string()][..]));
        assert_eq!(t.records[0], vec!["1".to_string(), "carol".to_string()]);
    }

    #[test]
    fn push_refreshes_desc_from_tablet() {
        let g = group(data());
        let tmp = TempXlsx::new();
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((1, HDR_DESC)).set_value("策划改的");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        // 推送是双向的：desc 也按 tablet 刷新回去
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();
        let book = open(tmp.path());
        assert_eq!(book.sheet_by_name("HeroBase").unwrap().value((1, HDR_DESC)), "编号");
    }

    /// lazy_read 的核心断言：没被同步的 sheet、以及被同步 sheet 上的样式，都要活下来。
    #[test]
    fn push_preserves_other_sheets_and_cell_styles() {
        let g = group(data());
        let tmp = TempXlsx::new();
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();

        // 加一个策划自建的 sheet，并给数据格刷个底色
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            book.new_sheet("策划备注").unwrap();
            {
                let ws = book.sheet_by_name_mut("策划备注").unwrap();
                ws.cell_mut((1, 1)).set_value("别删我");
            }
            {
                let ws = book.sheet_by_name_mut("HeroBase").unwrap();
                ws.style_mut((1, DATA_START)).set_background_color("#FF0000");
            }
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();

        let book = open(tmp.path());
        let extra = book.sheet_by_name("策划备注").expect("自建 sheet 不该被删");
        assert_eq!(extra.value((1, 1)), "别删我");
        let ws = book.sheet_by_name("HeroBase").unwrap();
        assert_eq!(ws.value((1, HDR_NAME)), "id");
        assert!(
            ws.style((1, DATA_START)).background_color().is_some(),
            "数据格的底色被推送抹掉了"
        );
    }

    #[test]
    fn full_mode_trims_extra_columns_and_rows_in_place() {
        let g = group(data());
        let tmp = TempXlsx::new();
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();

        // 装备：多一列、多两行，外加一个自建 sheet（验证 Full 不是重建工作簿）
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            book.new_sheet("策划备注").unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((3, HDR_NAME)).set_value("策划加的列");
            for r in 0..2u32 {
                ws.cell_mut((1, DATA_START + 2 + r)).set_value("残留行");
            }
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        sync_group_to_xlsx(tmp.path(), &g, SyncMode::Full).unwrap();

        let sheets = read_xlsx_sheets(tmp.path(), Some(&g)).unwrap();
        let s = &sheets[0];
        assert_eq!(s.headers, vec!["id", "name"], "多余列应被清掉");
        assert_eq!(s.rows, vec![vec!["1".to_string(), "alice".to_string()],
                                vec!["2".to_string(), "bob".to_string()]],
                   "多余数据行应被清掉");

        let book = open(tmp.path());
        assert!(book.sheet_by_name("策划备注").is_ok(), "Full 不该重建工作簿");
    }

    /// 改掉尾列的列名：虽然还没到「表头不符合规范」，但匹配前缀退化成 1 列，
    /// 回读只能认到那一列 —— 这正是「不改列名」这条约定被破坏后的表现。
    #[test]
    fn tampered_tail_column_degrades_to_matched_prefix() {
        let g = group(data());
        let tmp = TempXlsx::new();
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((2, HDR_NAME)).set_value("策划改坏的列名");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        let sheets = read_xlsx_sheets(tmp.path(), Some(&g)).unwrap();
        assert_eq!(classify_header(&sheets[0].headers, &g.tables[0].schema.fields),
                   HeaderMatch::TailDiff);

        let patch = read_group_from_xlsx(tmp.path(), &g, false).unwrap();
        let t = &patch.tables[0];
        assert_eq!(t.descs.as_deref(), Some(&["编号".to_string()][..]), "只认匹配前缀那一列");
        assert_eq!(t.records[0], vec!["1".to_string(), String::new()]);
    }

    /// 改掉首列（id）：表头直接判为 Invalid，force=false 时整张表不回读，
    /// 面板会显示「表头不符合规范」并只给「强制→」。
    #[test]
    fn tampered_id_column_blocks_read() {
        let g = group(data());
        let tmp = TempXlsx::new();
        sync_group_to_xlsx(tmp.path(), &g, SyncMode::DataOnly).unwrap();
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((1, HDR_NAME)).set_value("编号");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        let sheets = read_xlsx_sheets(tmp.path(), Some(&g)).unwrap();
        assert_eq!(classify_header(&sheets[0].headers, &g.tables[0].schema.fields),
                   HeaderMatch::Invalid);

        let patch = read_group_from_xlsx(tmp.path(), &g, false).unwrap();
        assert!(patch.tables.is_empty(), "表头不符合规范时不该回读");
    }
}
