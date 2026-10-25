//! 落盘：把一次 [`SyncOp`] 作用到两个**真相对象**上（内存 `Group` / umya `Workbook`）。
//!
//! # 硬约束：xlsx 侧的最后一跳是「补丁应用器」，不是「序列化器」
//!
//! 保真来自**不重新序列化**：`lazy_read` 让没被碰过的 sheet 在写回时走 zip 层的
//! **原始压缩字节直通**，被碰过的也只改我们点名要改的格子。如果这里写成「把
//! [`SheetVo`] 序列化成 xlsx」，等于从零重建那张表，umya 没有建模的东西（行/列分组
//! 折叠、已应用的筛选条件、`protectedRanges`……见 @06 §7.4）当场全丢。
//!
//! VO 是**投影**，负责「决定」；真正被改的是原始 workbook 对象。这条由
//! `untouched_sheet_is_copied_byte_for_byte` 这条单测守着——它直接比对 zip 里那张
//! 没被同步的 sheet 的原始字节，末段一旦变成序列化，它必红。
//!
//! # 另一条：这个模块不做列匹配
//!
//! 列对齐是 `plan` 的黑盒，结果放在 [`PairPlan::col_match`] 里。这里只**消费**
//! （[`ColumnMatch::xlsx_col`]），不重新算——否则又会冒出第二份规则。

use std::path::Path;

use anyhow::{Context, Result};
use umya_spreadsheet::Worksheet;

use crate::model::{FieldDef, Group};

use super::plan::{ColumnMatch, PairPlan, PushScope, SyncOp};
use super::read;
use super::vo::{Layout, DATA_START, HDR_DESC, HDR_EXPORT, HDR_NAME, HDR_TYPE};

/// 执行一次同步操作。
///
/// **点下去之前先重算一遍计划**：面板上的那份是渲染时的快照，渲染到点击之间文件
/// 可能被别处改过。发现这个操作已经不该做了就**拒绝**，不硬做——这条流的数据丢失
/// 已经出过一次事，宁可让用户重看一遍，也不要拿旧快照去写新文件。
pub fn execute(path: &Path, group: &mut Group, node: &str, op: &SyncOp) -> Result<String> {
    let file_exists = path.exists();
    let sheets = if file_exists {
        read::read_xlsx_sheets(path, Some(group))?
    } else {
        Vec::new()
    };
    let plan = super::plan::plan_group(group, file_exists.then_some(sheets.as_slice()));

    let pair = plan.pairs.iter().find(|p| p.node == node)
        .with_context(|| format!("同步面板里没有 '{}' 这一行", node))?;
    if !pair.ops.contains(op) {
        anyhow::bail!(
            "同步面板已过期：'{}' 现在不该做「{}」。请刷新后重试",
            node, op.label()
        );
    }

    let right = sheets.iter().find(|s| s.name == node);
    match op {
        SyncOp::CreateFile => create(path, group, pair),
        SyncOp::Push { scope } => push(path, group, pair, *scope),
        SyncOp::Pull => {
            let right = right.with_context(|| format!("xlsx 里没有 '{}' 这张 sheet", node))?;
            pull(group, pair, right)
        }
        // 今天它和 ←数据 共用一条路径，点了会静默地做一次拉数据。如实报未实现，
        // 不再假装成功。
        SyncOp::ImportSheet => {
            anyhow::bail!("「←导入」尚未实现：还不能从 xlsx 独有的 sheet 建表")
        }
    }
}

// ── 推送：tablet → xlsx ──

/// xlsx 还不存在时新建。整组一次写完（文件是按 group 一个）。
pub fn create(path: &Path, group: &Group, pair: &PairPlan) -> Result<String> {
    if path.exists() {
        // 文件在、只是缺这张 sheet：**只补这一张**，不要把整份推倒重来——
        // 别的 sheet 上策划刷的颜色、调的高宽都得留着。
        let cm = pair.col_match.clone().unwrap_or(ColumnMatch {
            matched_prefix: 0,
            tablet_only: pair.left.as_ref().map_or(0, |s| s.cols),
            xlsx_only: 0,
        });
        let pair = PairPlan { col_match: Some(cm), ..pair.clone() };
        return push(path, group, &pair, PushScope::WholeTable);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_fresh_xlsx(path, group)?;
    Ok(format!("已创建 {}（{}）", path.display(), pair.node))
}

pub fn push(path: &Path, group: &Group, pair: &PairPlan, scope: PushScope) -> Result<String> {
    let table = group.tables.iter().find(|t| t.name == pair.node)
        .with_context(|| format!("找不到表 {}", pair.node))?;
    let cm = pair.col_match.as_ref()
        .with_context(|| format!("'{}' 没有列对齐结果，不该做推送", pair.node))?;
    let fields = &table.schema.fields;

    // lazy_read：只有下面 sheet_by_name_mut 碰过的 sheet 会被反序列化，其余 sheet
    // 写回时走 raw 字节直通。所以这里**不要**为了「看一眼」去调 sheet()/sheet_mut()。
    let mut book = umya_spreadsheet::reader::xlsx::lazy_read(path)
        .with_context(|| format!("打开 xlsx 失败: {}", path.display()))?;

    // 存在性必须问 sheet_collection_no_check()：`sheet_by_name()` 走的是
    // `Workbook::sheet()`，它对**没反序列化**的 sheet 一律返回 NotDeserialized，
    // 在 lazy book 上会骗我们「这张表不存在」，然后 new_sheet 撞 NameDuplicate。
    let exists = book.sheet_collection_no_check().iter().any(|s| s.name() == pair.node);
    if !exists {
        book.new_sheet(&pair.node)?;
    }
    let ws = book.sheet_by_name_mut(&pair.node)
        .map_err(|_| anyhow::anyhow!("sheet '{}' 不存在", pair.node))?;

    // 旧布局（本特性之前生成的文件）就地升级成 4 行表头。不重建整表——策划已有的
    // 颜色 / 列宽 / 行高要留着。
    if exists && read::detect_layout(ws, Some(fields)) == Layout::Legacy {
        upgrade_legacy_layout(ws);
    }

    // 补列是**插入**，不是往那几个位置上写：策划经常在 schema 列后面放自己的测试列 /
    // 公式列，直接写会盖掉它们。先把位置腾出来（1-based，在该列之前插），后面的列
    // 连同各自的样式一起往右推。
    if scope == PushScope::WithTabletColumns && cm.tablet_only > 0 {
        ws.insert_new_column_by_index(cm.matched_prefix as u32 + 1, cm.tablet_only as u32);
    }

    for (i, f) in fields.iter().enumerate() {
        if let Some(c) = cm.xlsx_col(i, scope) {
            write_header(ws, c, f);
        }
    }

    for (ri, row) in table.records.iter().enumerate() {
        let r = ri as u32 + DATA_START;
        for (i, v) in row.iter().enumerate() {
            if let Some(c) = cm.xlsx_col(i, scope) {
                ws.cell_mut((c, r)).set_value(v.clone());
            }
        }
    }

    if scope == PushScope::WholeTable {
        trim_to_schema(ws, fields.len(), table.records.len());
    }

    umya_spreadsheet::writer::xlsx::write(&book, path)
        .with_context(|| format!("写入 xlsx 失败: {}", path.display()))?;

    let what = match scope {
        PushScope::MatchedOnly => "数据",
        PushScope::WithTabletColumns => "数据 + 补列",
        PushScope::WholeTable => "整表",
    };
    Ok(format!("已同步 {} → {}（{}）", pair.node, path.display(), what))
}

/// 从零造一份：**纯数据文本**——4 行表头 + 数据，此外什么也不加。
///
/// 不加样式、不加冻结、不加保护。理由：**样式是 Excel 侧的东西**（@06 §7）。初次生成
/// 不预设任何格式，之后策划在这个文件上做的一切（底色 / 边框 / 列宽 / 行高）由 `push`
/// 的定点写入原样留存——写数据不碰样式。
///
/// 工作表保护尤其不能加：这条流用 umya 原地编辑，而 umya 不认识 `<protectedRanges>`
/// （「放开某片区域可编辑」那个元素）、会把它静默丢掉，却保留整表保护开关——结果是把
/// 文件变成整张只读，策划连数据都改不了。
/// 从零写一整组工作簿（table / constant / enum 都写，默认样式）。
///
/// 模板导入 / `util scaffold` 用：没有原工作簿可保留样式，只要求「结构 + 数据」
/// 与内存模型一致。运行时保存仍走 `ops::save_xlsx_group` 的定点写入。
pub(crate) fn write_fresh_group(path: &Path, group: &Group) -> Result<()> {
    use rust_xlsxwriter::Workbook;
    let mut wb = Workbook::new();

    let mut n = 0usize;
    for table in &group.tables {
        if table.deleted { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(sanitize_sheet_name(&table.name))?;
        for (i, f) in table.schema.fields.iter().enumerate() {
            let c = i as u16;
            ws.write_string(HDR_DESC - 1, c, &f.desc)?;
            ws.write_string(HDR_EXPORT - 1, c, f.export.display())?;
            ws.write_string(HDR_TYPE - 1, c, &f.tbl_type)?;
            ws.write_string(HDR_NAME - 1, c, &f.name)?;
        }
        for (r, row) in table.records.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                ws.write_string(r as u32 + DATA_START - 1, c as u16, val)?;
            }
        }
        n += 1;
    }
    for constant in &group.constants {
        if constant.deleted { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(sanitize_sheet_name(&constant.name))?;
        for (i, h) in ["name", "type", "value", "export", "desc"].iter().enumerate() {
            ws.write_string(0, i as u16, *h)?;
        }
        for (r, e) in constant.entries.iter().enumerate() {
            ws.write_string(r as u32 + 1, 0, &e.name)?;
            ws.write_string(r as u32 + 1, 1, &e.tbl_type)?;
            ws.write_string(r as u32 + 1, 2, &e.value)?;
            ws.write_string(r as u32 + 1, 3, e.export.display())?;
            ws.write_string(r as u32 + 1, 4, &e.desc)?;
        }
        n += 1;
    }
    for enum_def in &group.enums {
        if enum_def.deleted { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(sanitize_sheet_name(&enum_def.name))?;
        for (i, h) in ["id", "name", "desc"].iter().enumerate() {
            ws.write_string(0, i as u16, *h)?;
        }
        for (r, e) in enum_def.entries.iter().enumerate() {
            ws.write_string(r as u32 + 1, 0, &e.id)?;
            ws.write_string(r as u32 + 1, 1, &e.name)?;
            ws.write_string(r as u32 + 1, 2, &e.desc)?;
        }
        n += 1;
    }
    if n == 0 {
        // xlsx 至少要有一个 sheet；运行时保存也用同一个占位名（ops::save_xlsx_group）。
        let ws = wb.add_worksheet();
        ws.set_name("_empty")?;
    }
    std::fs::write(path, wb.save_to_buffer()?)?;
    Ok(())
}

fn write_fresh_xlsx(path: &Path, group: &Group) -> Result<()> {
    use rust_xlsxwriter::Workbook;
    let mut wb = Workbook::new();
    for table in &group.tables {
        if table.deleted { continue; }
        let ws = wb.add_worksheet();
        ws.set_name(sanitize_sheet_name(&table.name))?;
        for (i, f) in table.schema.fields.iter().enumerate() {
            let c = i as u16;
            ws.write_string(HDR_DESC - 1, c, &f.desc)?;
            ws.write_string(HDR_EXPORT - 1, c, f.export.display())?;
            ws.write_string(HDR_TYPE - 1, c, &f.tbl_type)?;
            ws.write_string(HDR_NAME - 1, c, &f.name)?;
        }
        for (r, row) in table.records.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                ws.write_string(r as u32 + DATA_START - 1, c as u16, val)?;
            }
        }
    }
    std::fs::write(path, wb.save_to_buffer()?)?;
    Ok(())
}

fn write_header(ws: &mut Worksheet, col: u32, f: &FieldDef) {
    ws.cell_mut((col, HDR_DESC)).set_value(f.desc.clone());
    ws.cell_mut((col, HDR_EXPORT)).set_value(f.export.display().to_string());
    ws.cell_mut((col, HDR_TYPE)).set_value(f.tbl_type.clone());
    ws.cell_mut((col, HDR_NAME)).set_value(f.name.clone());
}

/// 旧布局原地升级：顶部插 3 行，原来的列名行（第 1 行）顺移到第 4 行，正好落在新
/// 布局的列名位；再补上 desc / export / type 三行。
///
/// `insert_new_row` 只挪坐标、不搬样式，新插的三行是裸的——必须从已就位的列名行
/// 显式复制样式，否则升级完表头会突然掉样式。
fn upgrade_legacy_layout(ws: &mut Worksheet) {
    ws.insert_new_row(1, 3);
    let end_col = ws.highest_column().max(1);
    for target in [HDR_DESC, HDR_EXPORT, HDR_TYPE] {
        ws.copy_row_styling(HDR_NAME, target, Some(1), Some(end_col));
    }
}

/// `WholeTable`（强制→）的就地清理：把 `keep_cols` 之外的列、`DATA_START + keep_rows`
/// 之后的数据行删掉，让 xlsx 的结构与 tablet 严格对齐。
///
/// 用 `remove_cell` 而不是往格子里写空串：写空串会留下一个「存在」的格，
/// `highest_column_and_row()` 照算，面板的列数会一直虚高。
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

// ── 回读：xlsx → tablet ──

/// 把 xlsx 的 desc 与数据合并进内存模型。
///
/// **空 = 清空**：匹配前缀内以 Excel 为准，空串一样覆盖。范围由 `matched_prefix`
/// 表达，所以不再需要「空值不覆盖」那道守卫——它以前同时干着划范围和不覆盖两件事，
/// 害得策划在 Excel 里清空一个格、tbl 的值却清不掉。
fn pull(group: &mut Group, pair: &PairPlan, right: &super::vo::SheetVo) -> Result<String> {
    let cm = pair.col_match.as_ref()
        .with_context(|| format!("'{}' 没有列对齐结果，不该做回读", pair.node))?;
    let n = cm.matched_prefix;

    let table = group.tables.iter_mut().find(|t| t.name == pair.node)
        .with_context(|| format!("找不到表 {}", pair.node))?;
    let field_count = table.schema.fields.len();

    // desc 只有新布局有；旧布局读不到 desc 格（全是 None），那就一格都不动。
    for i in 0..n.min(field_count) {
        if let Some(d) = right.columns.get(i).and_then(|c| c.desc.as_ref()) {
            table.schema.fields[i].desc = d.clone();
        }
    }

    // 数据：按位置合并。tablet 独有的行与列原样保留（回读只覆盖 xlsx 有的那几列）。
    for (ri, xrow) in right.rows.iter().enumerate() {
        while table.records.len() <= ri {
            table.records.push(vec![String::new(); field_count]);
        }
        for i in 0..n.min(field_count) {
            while table.records[ri].len() <= i {
                table.records[ri].push(String::new());
            }
            table.records[ri][i] = xrow.cells.get(i).cloned().unwrap_or_default();
        }
    }

    // 比对落盘快照重算 dirty。**不要**手写 `original_records = records; dirty = false`
    // ——那样 `save_project_files` 会判定这张表干净而跳过，回读的数据永远写不进
    // .tbl、重载即丢（上次的 bug）。
    table.update_dirty();

    Ok(format!("已同步 {} ← {}（desc + 数据）", pair.node, n))
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
    use crate::excel_sync::testutil::*;
    use crate::excel_sync::{plan_group, read_xlsx_sheets};

    /// 当前这一行允许做哪些操作（就是面板上会画出来的按钮）。
    fn ops_for(path: &Path, g: &Group, node: &str) -> Vec<SyncOp> {
        let exists = path.exists();
        let sheets = if exists { read_xlsx_sheets(path, Some(g)).unwrap() } else { vec![] };
        let plan = plan_group(g, exists.then_some(sheets.as_slice()));
        plan.pairs.iter().find(|p| p.node == node).map(|p| p.ops.clone()).unwrap_or_default()
    }

    fn op(path: &Path, g: &Group, node: &str, want: SyncOp) -> SyncOp {
        let ops = ops_for(path, g, node);
        assert!(ops.contains(&want), "期望 {:?} 在 {:?} 里", want, ops);
        want
    }

    /// 与 `data()` 只差一个格、形状相同：用来制造「有差异」。
    ///
    /// 数据与 tablet 完全一致时 `plan` 会撤掉 `数据→` / `←数据`（没东西可同步），
    /// 所以凡是要走 Push/Pull 的用例都得先造出差异。
    fn drifted() -> Vec<Vec<&'static str>> {
        vec![vec!["1", "carol"], vec!["2", "bob"]]
    }

    /// 手造一份**两个 sheet** 的文件，两个 sheet 都由 **rust_xlsxwriter** 写。
    ///
    /// 关键在「谁写的」：umya 重新序列化**自己的**输出是幂等的、字节不变，所以拿
    /// umya 造的 sheet 去测「有没有被重写」永远测不出来（这条踩过一次）。必须用另一个
    /// 库写，umya 的再序列化才会产生不同的字节。
    fn write_two_sheet_book(path: &Path, records: &[Vec<&str>]) {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        {
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
        }
        {
            let ws = wb.add_worksheet();
            ws.set_name("策划备注").unwrap();
            ws.write_string(0, 0, "别删我").unwrap();
            ws.write_string(1, 0, "第二行").unwrap();
        }
        std::fs::write(path, wb.save_to_buffer().unwrap()).unwrap();
    }

    /// **这次重构的红线。** 没被同步的 sheet 在写回时走 zip 层的原始压缩字节直通，
    /// 所以它的 `sheetN.xml` 应当逐字节不变。末段一旦有人顺手写成「把 VO 序列化成
    /// xlsx」，这条立刻红（上一版重构成「全量 read」时实测过）。
    #[test]
    fn untouched_sheet_is_copied_byte_for_byte() {
        let mut g = group(data());
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &drifted());

        let before = zip_entry(tmp.path(), "xl/worksheets/sheet2.xml");
        let hero_before = zip_entry(tmp.path(), "xl/worksheets/sheet1.xml");

        let op = op(tmp.path(), &g, "HeroBase", SyncOp::Push { scope: PushScope::MatchedOnly });
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        // 对照组先断：同一次推送里，group 那张表确实被重写了。没有它，下面那条可能
        // 因为「整个文件根本没动」而假绿。
        assert_ne!(zip_entry(tmp.path(), "xl/worksheets/sheet1.xml"), hero_before,
                   "被同步的 sheet 应该被重写");
        assert_eq!(before, zip_entry(tmp.path(), "xl/worksheets/sheet2.xml"),
                   "没被同步的 sheet 必须是原始字节直通，不该被重新序列化");
    }

    /// **初次生成必须是纯数据文本**：不加样式、不加冻结、不加保护。
    ///
    /// 样式是 Excel 侧的东西——不预设任何格式，策划之后做的一切由推送原样留存。
    #[test]
    fn fresh_file_carries_no_styling_at_all() {
        let mut g = group(data());
        let tmp = TempXlsx::new();
        execute(tmp.path(), &mut g, "HeroBase", &SyncOp::CreateFile).unwrap();

        let styles = String::from_utf8(zip_entry(tmp.path(), "xl/styles.xml")).unwrap();
        for (what, needle) in [
            ("加粗", "<b/>"),
            ("边框", "style=\"thin\""),
            ("填充", "patternType=\"solid\""),
        ] {
            assert!(!styles.contains(needle), "初次生成的产物不该带{what}：styles.xml 里有 {needle}");
        }

        let sheet = String::from_utf8(zip_entry(tmp.path(), "xl/worksheets/sheet1.xml")).unwrap();
        assert!(!sheet.contains("<pane"), "初次生成的产物不该冻结窗格");
        assert!(!sheet.contains("sheetProtection"), "初次生成的产物不该带工作表保护");
        assert!(!sheet.contains(" s=\""), "初次生成的产物不该有样式引用");

        // 但结构要在：4 行表头 + 数据
        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert_eq!(s.columns.len(), 2);
        assert!(s.has_desc_row());
        assert_eq!(s.rows.len(), 2);
    }

    /// **框架契约**：umya 的读 → 写往返必须原样保住单元格样式。
    ///
    /// 这条曾经是坏的：3.0.1 会把边框定义清空（`<left style="thin"><color auto="1"/></left>`
    /// 变成 `<left/>`），连带让「只有边框」的格子连 `s` 属性都掉光——策划看到的
    /// 现象就是「同步一次，边框全没了」。3.1.1 修好了。
    ///
    /// 钉在这里：以后升/降 umya 一旦回归，这条立刻红，而不是等策划撞上。
    /// 注意它**不经过我们的代码**，纯粹是库的往返行为。
    #[test]
    fn umya_round_trip_keeps_all_cell_styles() {
        use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook};
        let tmp = TempXlsx::new();
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("S").unwrap();
        let fmts = [
            Format::new().set_bold(),
            Format::new().set_font_color("#FF0000"),
            Format::new().set_background_color("#FFFF00"),
            Format::new().set_border(FormatBorder::Thin),
            Format::new().set_num_format("0.00"),
            Format::new().set_align(FormatAlign::Center),
            Format::new().set_text_wrap(),
        ];
        for (i, f) in fmts.iter().enumerate() {
            ws.write_string_with_format(0, i as u16, "x", f).unwrap();
        }
        std::fs::write(tmp.path(), wb.save_to_buffer().unwrap()).unwrap();

        let features = |path: &Path| -> Vec<(&'static str, bool)> {
            let x = String::from_utf8(zip_entry(path, "xl/styles.xml")).unwrap();
            vec![
                ("加粗", x.contains("<b/>")),
                ("字体颜色", x.contains("FFFF0000")),
                ("填充", x.contains("FFFF00")),
                ("边框", x.contains("style=\"thin\"")),
                ("数字格式", x.contains("0.00")),
                ("对齐", x.contains("horizontal=\"center\"")),
                ("自动换行", x.contains("wrapText")),
            ]
        };
        let before = features(tmp.path());

        let book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
        umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();

        let after = features(tmp.path());
        for ((name, was), (_, now)) in before.iter().zip(after.iter()) {
            assert!(was, "夹具本身没写进 {name}");
            assert!(now, "umya 往返丢掉了「{name}」——查 umya 版本（3.0.1 会丢边框，3.1.1 已修）");
        }
    }

    #[test]
    fn push_preserves_other_sheets_and_cell_styles() {
        let mut g = group(data());
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &drifted());

        // 策划刷底色 + 加边框——推送两样都不该碰
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.style_mut((1, DATA_START)).set_background_color("#FF0000");
            ws.style_mut((2, DATA_START)).borders_mut().bottom_mut().set_border_style("thin");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        let op = op(tmp.path(), &g, "HeroBase", SyncOp::Push { scope: PushScope::MatchedOnly });
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        let book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
        let extra = book.sheet_by_name("策划备注").expect("自建 sheet 不该被删");
        assert_eq!(extra.value((1, 1)), "别删我");
        let ws = book.sheet_by_name("HeroBase").unwrap();
        assert_eq!(ws.value((1, HDR_NAME)), "id");
        assert!(ws.style((1, DATA_START)).background_color().is_some(),
                "数据格的底色被推送抹掉了");

        // 边框直接查 XML：umya 3.0.1 会把边框**定义**清空（`<left style="thin">` → `<left/>`），
        // 连带让「只有边框」的格子连 `s` 引用都掉光。这两条一起断才算数。
        let styles = String::from_utf8(zip_entry(tmp.path(), "xl/styles.xml")).unwrap();
        assert!(styles.contains("style=\"thin\""), "边框定义被推送抹掉了");
        let sheet = String::from_utf8(zip_entry(tmp.path(), "xl/worksheets/sheet1.xml")).unwrap();
        let at = sheet.find(&format!("r=\"B{DATA_START}\"")).expect("找不到那个数据格");
        let tag = &sheet[at..sheet[at..].find('>').map_or(sheet.len(), |e| at + e)];
        assert!(tag.contains("s=\""), "数据格丢了样式引用（umya 清空边框后连 s 一起掉）: <c {tag}>");
    }

    /// 强制→ 是**就地**重写：清掉多余的列与数据行，但不动别的东西。
    ///
    /// 「强制→」只在**真冲突**时给。判据见 plan::classify——xlsx 单纯是超集时
    /// 多出来的列一律不碰（列只能从 tbl 侧加），tablet 新加列且不与策划的列重名时
    /// 走的是「补列→」（插入）。
    #[test]
    fn whole_table_push_trims_in_place() {
        // tablet: id,name,atk ／ xlsx: id,name2,atk —— 第 2 列对不上、tail 里 atk 重名
        let mut g = group(data());
        g.tables[0].schema.fields.push(crate::model::FieldDef {
            name: "atk".into(), desc: "攻击".into(),
            tbl_type: "int".into(), export: crate::model::Export::ClientServer,
        });
        for r in &mut g.tables[0].records { r.push("9".into()); }
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &drifted());

        // 装备：改名第 2 列、加第 3 列 atk、多两行残留
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((2, HDR_NAME)).set_value("name2");
            ws.cell_mut((3, HDR_DESC)).set_value("攻击");
            ws.cell_mut((3, HDR_NAME)).set_value("atk");
            ws.cell_mut((3, DATA_START)).set_value("7");
            for r in 0..2u32 {
                ws.cell_mut((1, DATA_START + 2 + r)).set_value("残留行");
            }
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        let op = op(tmp.path(), &g, "HeroBase", SyncOp::Push { scope: PushScope::WholeTable });
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert_eq!(s.columns.len(), 3, "多余列应被清掉");
        assert_eq!(s.columns[1].name.as_deref(), Some("name"), "留下的是 tablet 的列名");
        assert_eq!(s.columns[2].name.as_deref(), Some("atk"));
        assert_eq!(s.rows.len(), 2, "多余数据行应被清掉");
        assert_eq!(s.rows[1].cells, vec!["2", "bob", "9"]);

        let book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
        assert!(book.sheet_by_name("策划备注").is_ok(), "强制→ 不该重建整个工作簿");
    }

    /// **「补列→」是插入，不是覆盖。** 策划经常在 schema 列后面放自己的测试列 / 公式列，
    /// 直接往那个位置写会把它们盖掉——这条钉的就是「策划的列活下来」。
    #[test]
    fn add_columns_inserts_and_keeps_designer_columns() {
        let mut g = group(data());
        g.tables[0].schema.fields.push(crate::model::FieldDef {
            name: "atk".into(), desc: "攻击".into(),
            tbl_type: "int".into(), export: crate::model::Export::ClientServer,
        });
        for (i, r) in g.tables[0].records.iter_mut().enumerate() {
            r.push(if i == 0 { "7".into() } else { "8".into() });
        }
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &drifted());

        // 策划在自己的第 3 列放了个公式列
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((3, HDR_DESC)).set_value("策划的");
            ws.cell_mut((3, HDR_EXPORT)).set_value("不导出");
            ws.cell_mut((3, HDR_TYPE)).set_value("str");
            ws.cell_mut((3, HDR_NAME)).set_value("公式");
            ws.cell_mut((3, DATA_START)).set_value("=A5");
            ws.cell_mut((3, DATA_START + 1)).set_value("=A6");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        let op = op(tmp.path(), &g, "HeroBase",
                    SyncOp::Push { scope: PushScope::WithTabletColumns });
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert_eq!(s.columns.len(), 4, "tablet 的 atk 插进来，策划的列还在");
        assert_eq!(s.columns[2].name.as_deref(), Some("atk"), "新列插在匹配前缀之后");
        assert_eq!(s.columns[3].name.as_deref(), Some("公式"), "策划的列被推到右边，没被盖掉");
        assert_eq!(s.columns[3].desc.as_deref(), Some("策划的"));
        assert_eq!(s.rows[0].cells[3], "=A5", "策划的公式值也活着");
        assert_eq!(s.rows[0].cells[2], "7", "tablet 的值落在新列上");
        assert_eq!(s.rows[1].cells[2], "8");
    }

    #[test]
    fn legacy_layout_is_upgraded_on_push() {
        use rust_xlsxwriter::Workbook;
        let mut g = group(data());
        let tmp = TempXlsx::new();

        // 手造一份旧布局文件：只有 1 行列名，数据从第 2 行起
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        for (i, (n, _, _)) in FIELD_SPEC.iter().enumerate() {
            ws.write_string(0, i as u16, *n).unwrap();
        }
        for (r, row) in data().iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32 + 1, c as u16, *v).unwrap();
            }
        }
        std::fs::write(tmp.path(), wb.save_to_buffer().unwrap()).unwrap();

        let op = op(tmp.path(), &g, "HeroBase", SyncOp::Push { scope: PushScope::MatchedOnly });
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert!(s.has_desc_row(), "升级后应该有 desc 行");
        assert_eq!(s.columns[0].desc.as_deref(), Some("编号"));
        assert_eq!(s.columns.len(), 2);
        assert_eq!(s.rows.len(), 2, "升级不能吃掉原有数据");
        assert_eq!(s.rows[0].cells, vec!["1", "alice"]);
    }
    /// 回读同时带走 desc 与数据；**空 = 清空**。
    #[test]
    fn pull_takes_desc_and_clears_empty_cells() {
        let mut g = group(data());
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &data());

        // 策划改：第 2 列 desc、清掉第 1 行第 2 列的值
        {
            let mut book = umya_spreadsheet::reader::xlsx::read(tmp.path()).unwrap();
            let ws = book.sheet_by_name_mut("HeroBase").unwrap();
            ws.cell_mut((2, HDR_DESC)).set_value("角色名");
            ws.cell_mut((2, DATA_START)).set_value("");
            umya_spreadsheet::writer::xlsx::write(&book, tmp.path()).unwrap();
        }

        let op = op(tmp.path(), &g, "HeroBase", SyncOp::Pull);
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        let t = &g.tables[0];
        assert_eq!(t.schema.fields[1].desc, "角色名", "desc 要跟着回读");
        assert_eq!(t.records[0], vec!["1", ""], "清空一个格要真的清掉");
        assert_eq!(t.records[1], vec!["2", "bob"]);
        assert!(t.dirty, "回读后必须置上未保存标记，否则改动写不进 .tbl");
    }
    /// 回读只覆盖 xlsx 有的那几列，tablet 独有的列原样保留。
    #[test]
    fn pull_keeps_tablet_only_columns() {
        let mut g = group(data());
        g.tables[0].schema.fields.push(crate::model::FieldDef {
            name: "atk".into(), desc: "攻击".into(),
            tbl_type: "int".into(), export: crate::model::Export::ClientServer,
        });
        for r in &mut g.tables[0].records { r.push("99".into()); }

        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &drifted());

        let op = op(tmp.path(), &g, "HeroBase", SyncOp::Pull);
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        assert_eq!(g.tables[0].records[0], vec!["1", "carol", "99"],
                   "xlsx 没有的列不能被回读清掉");
    }

    /// 面板渲染到点击之间文件被别处改过 → plan 过期 → **拒绝**，不拿旧快照硬写。
    #[test]
    fn stale_op_is_rejected() {
        let mut g = group(data());
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &data());

        // 数据完全一致、结构也没差异 → 此时不该做 Pull
        let err = execute(tmp.path(), &mut g, "HeroBase", &SyncOp::Pull).unwrap_err();
        assert!(format!("{}", err).contains("已过期"), "got: {}", err);
    }

    #[test]
    fn import_sheet_reports_unimplemented() {
        let mut g = group(data());
        let tmp = TempXlsx::new();
        write_two_sheet_book(tmp.path(), &data());

        let err = execute(tmp.path(), &mut g, "策划备注", &SyncOp::ImportSheet).unwrap_err();
        assert!(format!("{}", err).contains("尚未实现"), "got: {}", err);
    }

    #[test]
    fn create_writes_four_row_header_and_then_syncs() {
        let mut g = group(data());
        let tmp = TempXlsx::new();

        let op = op(tmp.path(), &g, "HeroBase", SyncOp::CreateFile);
        execute(tmp.path(), &mut g, "HeroBase", &op).unwrap();

        let s = &read_xlsx_sheets(tmp.path(), Some(&g)).unwrap()[0];
        assert_eq!(s.columns[0].desc.as_deref(), Some("编号"));
        assert_eq!(s.columns[0].ty.as_deref(), Some("int"));
        assert_eq!(s.columns[0].export.as_deref(), Some("前后端"));
        assert!(s.has_desc_row());
        assert_eq!(s.rows.len(), 2);
    }


    /// **黑盒的封闭性。** 「怎么对齐列」只该发生在 `plan` 里；执行器一旦自己重算，
    /// 就又有了两份规则，改一份忘一份——那正是这次重构要拆掉的东西。
    ///
    /// 断言直接读自己的源码。查询串在运行期拼出来，否则这几个字面量会出现在
    /// `include_str!` 的结果里、断言永远失败。
    #[test]
    fn apply_never_does_column_matching() {
        let src = include_str!("apply.rs");
        for name in ["compute_column_match", "classify_header"] {
            let needle = format!("{}{}", name, "(");
            assert!(
                !src.contains(&needle),
                "apply.rs 里不该出现 {needle} —— 列匹配是 plan 的黑盒，这里只消费结果"
            );
        }
    }
}
