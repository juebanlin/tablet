//! 任意 Excel → tablet 的映射导入（Phase A）。
//!
//! 源表不要求 tablet 的 4 行表头：用户选表头行 / 数据起始行 / id 列，把源列映射到
//! 目标字段。输出默认是 Tbl 组（`.tbl`），源 Excel 不修改。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{ConstEntry, EnumEntry, Export, FieldDef};
use pinyin::ToPinyin;

/// 源表预览（只读）。
#[derive(Debug, Clone, PartialEq)]
pub struct SheetPreview {
    pub columns: Vec<String>,
    /// 从 `data_start` 开始的预览行；空行跳过。
    pub rows: Vec<Vec<String>>,
    pub max_row: u32,
}

/// 一个目标字段的映射。
#[derive(Debug, Clone, PartialEq)]
pub struct MappedField {
    pub target_name: String,
    /// 1-based 源列号。
    pub source_col: u32,
    pub tbl_type: String,
    pub export: Export,
    pub desc: String,
}

/// 一次映射导入的完整计划。
#[derive(Debug, Clone, PartialEq)]
pub struct MappingPlan {
    pub source: PathBuf,
    pub sheet: String,
    /// 1-based 表头行。
    pub header_row: u32,
    /// 1-based 数据起始行。
    pub data_start: u32,
    /// 可选的语义行；0 = 没有。
    pub desc_row: u32,
    pub type_row: u32,
    pub export_row: u32,
    pub group_name: String,
    pub node_name: String,
    /// 第一个必须是 id。
    pub fields: Vec<MappedField>,
}

pub fn list_sheets(path: &Path) -> Result<Vec<String>> {
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 {}", path.display()))?;
    Ok(book.sheet_collection_no_check().iter().map(|s| s.name().to_string()).collect())
}

/// 工作簿里每张 sheet 的名字与「已用范围」（用于映射界面左栏的概览）。
#[derive(Debug, Clone, PartialEq)]
pub struct SheetInfo {
    pub name: String,
    pub columns: u32,
    pub rows: u32,
}

pub fn sheet_infos(path: &Path) -> Result<Vec<SheetInfo>> {
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 {}", path.display()))?;
    Ok(book
        .sheet_collection_no_check()
        .iter()
        .map(|s| {
            let (columns, rows) = s.highest_column_and_row();
            SheetInfo { name: s.name().to_string(), columns, rows }
        })
        .collect())
}

/// 打开一张 sheet 的预览。`header_row` / `data_start` 都是 1-based。
pub fn read_sheet_preview(
    path: &Path,
    sheet: &str,
    header_row: u32,
    data_start: u32,
    max_preview_rows: usize,
) -> Result<SheetPreview> {
    let mut book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 {}", path.display()))?;
    let ws = book.sheet_by_name_mut(sheet)
        .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet, e))?;
    let (max_col, max_row) = ws.highest_column_and_row();
    if header_row == 0 || header_row > max_row {
        anyhow::bail!("表头行 {} 超出 sheet 范围（1..={}）", header_row, max_row);
    }

    let last_col = (1..=max_col).rev()
        .find(|&c| !ws.value((c, header_row)).trim().is_empty())
        .unwrap_or(0);
    if last_col == 0 {
        anyhow::bail!("第 {} 行没有任何列名", header_row);
    }
    let columns: Vec<String> = (1..=last_col)
        .map(|c| ws.value((c, header_row)).trim().to_string())
        .collect();

    let start = data_start.max(header_row + 1);
    let end = max_row.min(start.saturating_add(max_preview_rows as u32).saturating_sub(1));
    let mut rows = Vec::new();
    for r in start..=end {
        let cells: Vec<String> = (1..=last_col).map(|c| ws.value((c, r))).collect();
        if cells.iter().all(|v| v.is_empty()) { continue; }
        rows.push(cells);
    }
    Ok(SheetPreview { columns, rows, max_row })
}

/// 建议表头行：前 20 行里非空格最多的一行。
pub fn detect_header_row(path: &Path, sheet: &str) -> Result<u32> {
    let mut book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 {}", path.display()))?;
    let ws = book.sheet_by_name_mut(sheet)
        .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet, e))?;
    let (max_col, max_row) = ws.highest_column_and_row();

    // 标准 4 行表头：desc / export / type / field。前两行（相对字段行）分别是
    // 类型行与导出行时，下一行就是字段行——直接认它，避免 "cs|cs|s" 这类
    // 每个值都长得像合法字段名、把导出码行顶成表头。
    let semantic_kind = |r: u32| -> u8 {
        let mut ty = 0usize;
        let mut ex = 0usize;
        let mut non_empty = 0usize;
        for c in 1..=max_col {
            let t = ws.value((c, r));
            let t = t.trim();
            if t.is_empty() { continue; }
            non_empty += 1;
            if crate::types::TblType::parse(t).is_some() { ty += 1; }
            if parse_export_value(t).is_some() { ex += 1; }
        }
        if non_empty > 0 && ty * 2 >= non_empty { 1 }
        else if non_empty > 0 && ex * 2 >= non_empty { 2 }
        else { 0 }
    };
    for r in 3..=max_row.min(20) {
        let a = semantic_kind(r - 1);
        let b = semantic_kind(r - 2);
        if semantic_kind(r) == 0 && ((a == 1 && b == 2) || (a == 2 && b == 1)) {
            return Ok(r);
        }
    }

    let mut best = (0u32, 0i64);
    for r in 1..=max_row.min(20) {
        let mut score = 0i64;
        let mut non_empty = 0usize;
        let mut type_hits = 0usize;
        let mut export_hits = 0usize;
        for c in 1..=max_col {
            let v = ws.value((c, r));
            let t = v.trim();
            if t.is_empty() { continue; }
            non_empty += 1;
            score += t.chars().count() as i64;
            // 像字段名加分；纯数字/日期等数据行减分。
            if crate::validate::validate_field_name(t).is_none() {
                score += 5;
            } else if t.parse::<f64>().is_ok() {
                score -= 8;
            }
            if crate::types::TblType::parse(t).is_some() {
                type_hits += 1;
            }
            if parse_export_value(t).is_some() {
                export_hits += 1;
            }
        }
        // 整行几乎都是类型 / 导出码：这是标准 4 行表头里的语义行，不是字段行。
        // 不罚分的话 `cs|cs|s|s|c` 会因为每个值都是合法字段名而盖过真正的字段行。
        if non_empty > 0 && type_hits * 2 >= non_empty {
            score -= 50;
        }
        if non_empty > 0 && export_hits * 2 >= non_empty {
            score -= 50;
        }
        if non_empty > 0 && score > best.1 { best = (r, score); }
    }
    if best.0 == 0 { anyhow::bail!("sheet {} 是空的", sheet); }
    Ok(best.0)
}

/// 列名规范化：ASCII 字母数字保留下划线，驼峰转 snake_case；中文/符号作为分隔符。
/// 返回空串表示无法自动规范化，需要用户手填。
pub fn normalize_field_name(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let push_sep = |out: &mut String| {
        if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    };
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_alphanumeric() {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = chars.get(i + 1).copied();
            let prev_lower_or_digit = prev.map_or(false, |p| p.is_ascii_lowercase() || p.is_ascii_digit());
            let prev_upper = prev.map_or(false, |p| p.is_ascii_uppercase());
            let prev_other = prev.map_or(false, |p| !p.is_ascii_alphanumeric() && p != '_');
            let next_lower = next.map_or(false, |n| n.is_ascii_lowercase());
            // 驼峰 / 连续大写 / 中文后接英文都断开：
            // heroName → hero_name；AABb → aa_bb；URLPath → url_path；道具ID → dao_ju_id
            let boundary = !out.is_empty()
                && !out.ends_with('_')
                && ch.is_ascii_uppercase()
                && (prev_lower_or_digit || (prev_upper && next_lower) || prev_other);
            if boundary {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else if let Some(py) = ch.to_pinyin() {
            let plain = py.plain();
            if plain.is_empty() {
                push_sep(&mut out);
            } else {
                push_sep(&mut out);
                out.push_str(plain);
            }
        } else if ch == '_' || ch.is_whitespace() || ch.is_ascii_punctuation() {
            push_sep(&mut out);
        } else {
            // 其它 Unicode（emoji 等）当分隔符
            push_sep(&mut out);
        }
    }
    let mut out = out.trim_matches('_').to_string();
    if out.is_empty() {
        return out;
    }
    if out.chars().next().map_or(false, |c| c.is_ascii_digit()) {
        out.insert_str(0, "f_");
    }
    out
}

/// sheet 名 → PascalCase 节点名建议。
pub fn normalize_node_name(raw: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if upper {
                out.push(ch.to_ascii_uppercase());
                upper = false;
            } else {
                out.push(ch);
            }
        } else {
            upper = true;
        }
    }
    if out.is_empty() { return "Node".to_string(); }
    if out.chars().next().map_or(false, |c| c.is_ascii_digit()) {
        out.insert(0, 'N');
    }
    out
}

/// 按样本推断基础类型。
pub fn infer_type(values: &[String]) -> &'static str {
    let mut any = false;
    let mut int_ok = true;
    let mut float_ok = true;
    let mut bool_ok = true;
    for v in values {
        let t = v.trim();
        if t.is_empty() { continue; }
        any = true;
        if t.parse::<i64>().is_err() { int_ok = false; }
        if t.parse::<f64>().is_err() { float_ok = false; }
        if !t.eq_ignore_ascii_case("true") && !t.eq_ignore_ascii_case("false") { bool_ok = false; }
    }
    if !any { return "str"; }
    if int_ok { "int" }
    else if float_ok { "float" }
    else if bool_ok { "bool" }
    else { "str" }
}

/// 根据列名与预览数据给出初始映射；字段名冲突时加 `_2`。
pub fn suggest_fields(preview: &SheetPreview) -> Vec<MappedField> {
    let mut used: HashSet<String> = HashSet::new();
    let mut fields = Vec::new();
    for (i, raw) in preview.columns.iter().enumerate() {
        let mut name = normalize_field_name(raw);
        if name.is_empty() { name = format!("field_{}", i + 1); }
        let base = name.clone();
        let mut n = 2;
        while used.contains(&name) {
            name = format!("{}_{}", base, n);
            n += 1;
        }
        used.insert(name.clone());
        let samples: Vec<String> = preview.rows.iter()
            .filter_map(|r| r.get(i).cloned())
            .collect();
        let ty = infer_type(&samples);
        fields.push(MappedField {
            target_name: name,
            source_col: i as u32 + 1,
            tbl_type: ty.to_string(),
            export: Export::ClientServer,
            desc: raw.trim().to_string(),
        });
    }
    // id 必须是第一列；找到规范化名 == id 的列就提前。
    if let Some(pos) = fields.iter().position(|f| f.target_name == "id") {
        let id = fields.remove(pos);
        fields.insert(0, id);
    }
    fields
}

/// 常量 / 枚举的默认列映射：按固定目标列名字匹配源表头；匹配不上的源列不导入。
pub fn suggest_entry_fields(columns: &[String], kind: MappedNodeKind) -> Vec<MappedField> {
    let allowed = kind.allowed_targets();
    let mut used: HashSet<String> = HashSet::new();
    columns
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            let mut target = normalize_field_name(raw);
            if !allowed.contains(&target.as_str()) || !used.insert(target.clone()) {
                target = String::new();
            }
            MappedField {
                target_name: target,
                source_col: i as u32 + 1,
                tbl_type: "str".to_string(),
                export: Export::ClientServer,
                desc: raw.trim().to_string(),
            }
        })
        .collect()
}

/// 读取某一行在所有列上的值（1-based 行号）。
pub fn read_row_values(path: &Path, sheet: &str, row: u32) -> Result<Vec<String>> {
    let mut book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 {} 失败", path.display()))?;
    let ws = book.sheet_by_name_mut(sheet)
        .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet, e))?;
    let (max_col, max_row) = ws.highest_column_and_row();
    if row == 0 || row > max_row {
        return Ok(Vec::new());
    }
    Ok((1..=max_col).map(|c| ws.value((c, row))).collect())
}

/// 一次打开读取多行（1-based）；行号 0 / 越界返回空行。
pub fn read_rows(path: &Path, sheet: &str, rows: &[u32]) -> Result<Vec<Vec<String>>> {
    let mut book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 {} 失败", path.display()))?;
    let ws = book.sheet_by_name_mut(sheet)
        .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet, e))?;
    let (max_col, max_row) = ws.highest_column_and_row();
    Ok(rows
        .iter()
        .map(|&r| {
            if r == 0 || r > max_row {
                Vec::new()
            } else {
                (1..=max_col).map(|c| ws.value((c, r))).collect()
            }
        })
        .collect())
}

/// 尝试识别表头行上方的 desc / type / export 行。识别不到返回 0。
///
/// - type / export：在表头行之前找匹配值最多的一行；
/// - desc：默认取表头上一行（若它不是 type/export 行）。
pub fn detect_semantic_rows(path: &Path, sheet: &str, header_row: u32) -> Result<(u32, u32, u32)> {
    if header_row <= 1 {
        return Ok((0, 0, 0));
    }
    let mut book = umya_spreadsheet::reader::xlsx::read(path)
        .with_context(|| format!("打开 {} 失败", path.display()))?;
    let ws = book.sheet_by_name_mut(sheet)
        .map_err(|e| anyhow::anyhow!("打开 sheet {} 失败: {}", sheet, e))?;
    let (max_col, max_row) = ws.highest_column_and_row();
    let end = header_row.saturating_sub(1).min(max_row);
    let mut type_best = (0u32, 0usize);
    let mut export_best = (0u32, 0usize);
    for r in 1..=end {
        let mut tys = 0usize;
        let mut exs = 0usize;
        let mut non_empty = 0usize;
        for c in 1..=max_col {
            let v = ws.value((c, r));
            if v.trim().is_empty() { continue; }
            non_empty += 1;
            if crate::types::TblType::parse(&v).is_some() { tys += 1; }
            if parse_export_value(&v).is_some() { exs += 1; }
        }
        if non_empty > 0 && tys > type_best.1 { type_best = (r, tys); }
        if non_empty > 0 && exs > export_best.1 { export_best = (r, exs); }
    }
    let type_row = if type_best.1 >= 1 { type_best.0 } else { 0 };
    let mut export_row = if export_best.1 >= 1 { export_best.0 } else { 0 };
    // 同一行既像类型又像导出时，不硬猜导出。
    if type_row != 0 && type_row == export_row && export_best.1 < 2 {
        export_row = 0;
    }
    // desc 行：表头之前的行里，找第一行带非 ASCII 文本的行
    // （列字母 / 英文单词太容易误判；识别不到就是 0）。
    let mut desc_row = 0;
    for r in 1..header_row {
        if r == type_row || r == export_row { continue; }
        let has_text = (1..=max_col).any(|c| {
            let v = ws.value((c, r));
            !v.trim().is_empty() && v.chars().any(|ch| !ch.is_ascii())
        });
        if has_text { desc_row = r; break; }
    }
    Ok((desc_row, type_row, export_row))
}

/// 映射导入的目标节点类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedNodeKind {
    Table,
    Constant,
    Enum,
}

impl MappedNodeKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "table" | "表" => Some(Self::Table),
            "constant" | "常量" => Some(Self::Constant),
            "enum" | "枚举" => Some(Self::Enum),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Constant => "constant",
            Self::Enum => "enum",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Table => "表",
            Self::Constant => "常量",
            Self::Enum => "枚举",
        }
    }

    /// 常量 / 枚举的固定目标列；Table 返回空（字段由用户自由命名）。
    pub fn allowed_targets(self) -> &'static [&'static str] {
        match self {
            Self::Table => &[],
            Self::Constant => &["name", "type", "value", "export", "desc"],
            Self::Enum => &["id", "name", "desc"],
        }
    }

    pub fn required_targets(self) -> &'static [&'static str] {
        match self {
            Self::Table => &["id"],
            Self::Constant => &["name", "type", "value"],
            Self::Enum => &["id", "name"],
        }
    }
}

/// 自动识别出的整张表布局：节点类型 + 字段行 + 数据行 + 可选语义行。
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedLayout {
    pub kind: MappedNodeKind,
    pub header_row: u32,
    pub data_start: u32,
    pub desc_row: u32,
    pub type_row: u32,
    pub export_row: u32,
}

pub fn detect_layout(path: &Path, sheet: &str) -> Result<DetectedLayout> {
    // 常量 / 枚举是固定单行表头：先按第 1 行精确匹配，避免数据行较宽时
    // detect_header_row 把某条数据行误当成表头。
    let first = read_row_values(path, sheet, 1)?;
    if let Some(kind) = fixed_sheet_kind(&first) {
        return Ok(DetectedLayout {
            kind,
            header_row: 1,
            data_start: 2,
            desc_row: 0,
            type_row: 0,
            export_row: 0,
        });
    }
    let header_row = detect_header_row(path, sheet)?;
    let columns = read_row_values(path, sheet, header_row)?;
    let mut kind = classify_columns(&columns);
    let mut semantic = (0, 0, 0);
    if header_row > 1 {
        semantic = detect_semantic_rows(path, sheet, header_row)?;
        // 上方有说明 / 类型 / 导出行的就是标准多行表头——即使字段恰好叫
        // id/name/desc，也按“表”处理（单行枚举表不会在字段行上方放语义行）。
        if semantic.0 > 0 || semantic.1 > 0 || semantic.2 > 0 {
            kind = MappedNodeKind::Table;
        }
    }
    let (desc_row, type_row, export_row) = semantic;
    Ok(DetectedLayout {
        kind,
        header_row,
        data_start: header_row + 1,
        desc_row,
        type_row,
        export_row,
    })
}

/// 固定单行表头：常量 5 列 / 枚举 3 列（大小写、前后空格不敏感）。
fn fixed_sheet_kind(columns: &[String]) -> Option<MappedNodeKind> {
    let norm: Vec<String> = columns
        .iter()
        .map(|c| c.trim().to_ascii_lowercase())
        .collect();
    let take = |n: usize| -> Vec<&str> { norm.iter().take(n).map(|s| s.as_str()).collect() };
    if take(5) == vec!["name", "type", "value", "export", "desc"] {
        return Some(MappedNodeKind::Constant);
    }
    if take(3) == vec!["id", "name", "desc"] {
        return Some(MappedNodeKind::Enum);
    }
    None
}

/// 标准布局判定：
/// - 常量：单行表头恰好是 name/type/value(/export/desc)；
/// - 枚举：单行表头恰好是 id/name(/desc)；
/// - 其余（含标准 4 行表头）按表处理。
fn classify_columns(columns: &[String]) -> MappedNodeKind {
    let set: HashSet<String> = columns
        .iter()
        .map(|c| normalize_field_name(c))
        .filter(|c| !c.is_empty())
        .collect();
    // 常量：有 name/type/value 就足够；多出来的源列不映射，不因此判成表。
    if set.contains("name") && set.contains("type") && set.contains("value") {
        return MappedNodeKind::Constant;
    }
    let enum_cols: HashSet<&str> = ["id", "name", "desc"].into_iter().collect();
    if set.contains("id")
        && set.contains("name")
        && set.iter().all(|c| enum_cols.contains(c.as_str()))
    {
        return MappedNodeKind::Enum;
    }
    MappedNodeKind::Table
}

/// 常量 / 枚举的列映射计划：每个目标列绑一个源列，数据行 → 条目。
#[derive(Debug, Clone, PartialEq)]
pub struct EntryMappingPlan {
    pub source: PathBuf,
    pub sheet: String,
    pub header_row: u32,
    pub data_start: u32,
    pub group_name: String,
    pub node_name: String,
    pub kind: MappedNodeKind,
    pub fields: Vec<MappedField>,
}

pub fn entry_field_issues(kind: MappedNodeKind, fields: &[MappedField]) -> Vec<FieldIssue> {
    let allowed = kind.allowed_targets();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut src = HashSet::new();
    for (i, f) in fields.iter().enumerate() {
        if !allowed.contains(&f.target_name.as_str()) {
            out.push(FieldIssue {
                index: i,
                message: format!("目标列 {} 不适用于{}", f.target_name, kind.label()),
            });
            continue;
        }
        if !seen.insert(f.target_name.clone()) {
            out.push(FieldIssue { index: i, message: format!("目标列重复: {}", f.target_name) });
        }
        if f.source_col == 0 {
            out.push(FieldIssue { index: i, message: format!("目标列 {} 没有选择源列", f.target_name) });
        } else if !src.insert(f.source_col) {
            out.push(FieldIssue {
                index: i,
                message: format!("源列 {} 被多个目标列使用", f.source_col),
            });
        }
    }
    for req in kind.required_targets() {
        if !seen.contains(*req) {
            out.push(FieldIssue {
                index: 0,
                message: format!("缺少目标列 {}：{}必须有这一列", req, kind.label()),
            });
        }
    }
    out
}

pub fn validate_entry_plan(plan: &EntryMappingPlan) -> Result<(), String> {
    if plan.kind == MappedNodeKind::Table {
        return Err("EntryMappingPlan 只用于常量 / 枚举".to_string());
    }
    if plan.sheet.trim().is_empty() {
        return Err("没有选择 sheet".to_string());
    }
    if !crate::validate::is_valid_group_name(&plan.group_name) {
        return Err(format!("组名不合法: {}", plan.group_name));
    }
    if !crate::validate::is_valid_node_name(&plan.node_name) {
        return Err(format!("节点名不合法: {}", plan.node_name));
    }
    if plan.data_start <= plan.header_row {
        return Err("数据起始行必须在表头行之后".to_string());
    }
    if plan.fields.is_empty() {
        return Err("至少映射一个字段".to_string());
    }
    if let Some(issue) = entry_field_issues(plan.kind, &plan.fields).into_iter().next() {
        return Err(issue.message);
    }
    Ok(())
}

pub enum MappedEntries {
    Constants(Vec<ConstEntry>),
    Enums(Vec<EnumEntry>),
}

/// 常量 / 枚举：从数据起始行往下逐行读成条目。全空的映射单元格跳过。
pub fn build_entries(plan: &EntryMappingPlan) -> Result<MappedEntries, String> {
    validate_entry_plan(plan)?;
    let mut book = umya_spreadsheet::reader::xlsx::read(&plan.source)
        .map_err(|e| format!("打开 {} 失败: {}", plan.source.display(), e))?;
    let ws = book.sheet_by_name_mut(&plan.sheet)
        .map_err(|e| format!("打开 sheet {} 失败: {}", plan.sheet, e))?;
    let max_row = ws.highest_row();
    let find = |target: &str| {
        plan.fields.iter().find(|f| f.target_name == target).map(|f| f.source_col)
    };
    let get = |row: u32, target: &str| {
        find(target)
            .map(|c| ws.value((c, row)).trim().to_string())
            .unwrap_or_default()
    };
    let mut constants = Vec::new();
    let mut enums = Vec::new();
    for r in plan.data_start..=max_row {
        let any = plan
            .fields
            .iter()
            .any(|f| !ws.value((f.source_col, r)).trim().is_empty());
        if !any {
            continue;
        }
        match plan.kind {
            MappedNodeKind::Constant => {
                let name = get(r, "name");
                if name.is_empty() {
                    return Err(format!("第 {} 行：常量名不能为空", r));
                }
                let ty = get(r, "type");
                if ty.is_empty() {
                    return Err(format!("第 {} 行：类型不能为空", r));
                }
                if crate::types::TblType::parse(&ty).is_none() {
                    return Err(format!("第 {} 行：类型不合法: {:?}", r, ty));
                }
                let export_raw = get(r, "export");
                let export = if export_raw.is_empty() {
                    Export::ClientServer
                } else {
                    parse_export_value(&export_raw)
                        .ok_or_else(|| format!("第 {} 行：export 值不合法: {:?}", r, export_raw))?
                };
                constants.push(ConstEntry {
                    name,
                    tbl_type: ty,
                    value: get(r, "value"),
                    export,
                    desc: get(r, "desc"),
                });
            }
            MappedNodeKind::Enum => {
                let id = get(r, "id");
                let name = get(r, "name");
                if id.is_empty() {
                    return Err(format!("第 {} 行：枚举 id 不能为空", r));
                }
                if name.is_empty() {
                    return Err(format!("第 {} 行：枚举名不能为空", r));
                }
                enums.push(EnumEntry { id, name, desc: get(r, "desc") });
            }
            MappedNodeKind::Table => unreachable!(),
        }
    }
    match plan.kind {
        MappedNodeKind::Constant => {
            if constants.is_empty() {
                return Err("没有可导入的数据行".to_string());
            }
            Ok(MappedEntries::Constants(constants))
        }
        MappedNodeKind::Enum => {
            if enums.is_empty() {
                return Err("没有可导入的数据行".to_string());
            }
            Ok(MappedEntries::Enums(enums))
        }
        MappedNodeKind::Table => unreachable!(),
    }
}

/// 宽松解析 Excel 里的 export 值（短码或中文展示值）。
pub fn parse_export_value(raw: &str) -> Option<Export> {
    if let Some(e) = Export::parse_code(raw) { return Some(e); }
    match raw.trim() {
        "前后端" => Some(Export::ClientServer),
        "客户端" => Some(Export::ClientOnly),
        "服务器" => Some(Export::ServerOnly),
        "不导出" => Some(Export::None),
        _ => None,
    }
}

/// 生成最终字段与数据行。错误都带 sheet / 行号。
pub fn build_table_parts(plan: &MappingPlan) -> Result<(Vec<FieldDef>, Vec<Vec<String>>), String> {
    validate_plan(plan)?;
    let mut book = umya_spreadsheet::reader::xlsx::read(&plan.source)
        .map_err(|e| format!("打开 {} 失败: {}", plan.source.display(), e))?;
    let ws = book.sheet_by_name_mut(&plan.sheet)
        .map_err(|e| format!("打开 sheet {} 失败: {}", plan.sheet, e))?;
    let max_row = ws.highest_row();
    for (label, row) in [("描述行", plan.desc_row), ("类型行", plan.type_row), ("导出行", plan.export_row)] {
        if row > max_row {
            return Err(format!("{} {} 超出 sheet 范围（1..={}）", label, row, max_row));
        }
    }

    let mut fields: Vec<FieldDef> = plan.fields.iter().map(|f| FieldDef {
        name: f.target_name.clone(),
        desc: f.desc.clone(),
        tbl_type: f.tbl_type.clone(),
        export: f.export.clone(),
     aux: None, source_col: None,}).collect();
    for (i, mf) in plan.fields.iter().enumerate() {
        if plan.desc_row > 0 {
            let v = ws.value((mf.source_col, plan.desc_row));
            if !v.trim().is_empty() { fields[i].desc = v; }
        }
        if plan.type_row > 0 {
            let v = ws.value((mf.source_col, plan.type_row));
            if !v.trim().is_empty() {
                if crate::types::TblType::parse(&v).is_none() {
                    return Err(format!("sheet {} 第 {} 行字段 {} 的类型不合法: {:?}",
                        plan.sheet, plan.type_row, mf.target_name, v));
                }
                fields[i].tbl_type = v.trim().to_string();
            }
        }
        if plan.export_row > 0 {
            let v = ws.value((mf.source_col, plan.export_row));
            if !v.trim().is_empty() {
                let ex = parse_export_value(&v).ok_or_else(|| format!(
                    "sheet {} 第 {} 行字段 {} 的 export 值不合法: {:?}",
                    plan.sheet, plan.export_row, mf.target_name, v))?;
                fields[i].export = ex;
            }
        }
    }
    if fields[0].tbl_type != "int" {
        return Err("id 的类型必须是 int（类型行里 id 列不是 int）".to_string());
    }

    let mut records = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    for r in plan.data_start..=max_row {
        let mut cells = Vec::with_capacity(plan.fields.len());
        for f in &plan.fields {
            cells.push(ws.value((f.source_col, r)));
        }
        if cells.iter().all(|v| v.is_empty()) { continue; }
        let id = cells[0].trim();
        if id.is_empty() {
            return Err(format!("sheet {} 第 {} 行 id 为空", plan.sheet, r));
        }
        if id.parse::<i64>().map_or(true, |v| v <= 0) {
            return Err(format!("sheet {} 第 {} 行 id 必须是正整数: {:?}", plan.sheet, r, id));
        }
        if !seen_ids.insert(id.to_string()) {
            return Err(format!("sheet {} 第 {} 行 id 重复: {}", plan.sheet, r, id));
        }
        records.push(cells);
    }
    if records.is_empty() {
        return Err(format!("sheet {} 没有数据行", plan.sheet));
    }

    Ok((fields, records))
}

/// 单个字段的校验问题；`index` 指向传入 `fields` 切片里的下标。
#[derive(Debug, Clone, PartialEq)]
pub struct FieldIssue {
    pub index: usize,
    pub message: String,
}

/// 逐字段校验：字段名 / 关键字 / 重复 / 类型 / 源列 / id 类型。
///
/// 映射界面用它做输入时的实时提示；`validate_plan` 也走同一份规则，
/// 避免 UI 与真正导入时判得不一样。
pub fn field_issues(fields: &[MappedField], type_row: u32) -> Vec<FieldIssue> {
    let mut out = Vec::new();
    let mut names = HashSet::new();
    for (i, f) in fields.iter().enumerate() {
        if let Some((_, msg)) = crate::validate::validate_field_name(&f.target_name) {
            out.push(FieldIssue { index: i, message: format!("字段 {}: {}", f.target_name, msg) });
        }
        if !names.insert(f.target_name.to_lowercase()) {
            out.push(FieldIssue { index: i, message: format!("字段名重复: {}", f.target_name) });
        }
        if type_row == 0 && crate::types::TblType::parse(&f.tbl_type).is_none() {
            out.push(FieldIssue {
                index: i,
                message: format!("字段 {} 的类型不合法: {}", f.target_name, f.tbl_type),
            });
        }
        if f.source_col == 0 {
            out.push(FieldIssue { index: i, message: format!("字段 {} 没有选择源列", f.target_name) });
        }
        if f.target_name == "id" && type_row == 0 && f.tbl_type != "int" {
            out.push(FieldIssue { index: i, message: "id 的类型必须是 int".to_string() });
        }
    }
    out
}

pub fn validate_plan(plan: &MappingPlan) -> Result<(), String> {
    if plan.sheet.trim().is_empty() {
        return Err("没有选择 sheet".to_string());
    }
    if !crate::validate::is_valid_group_name(&plan.group_name) {
        return Err(format!("组名不合法: {}", plan.group_name));
    }
    if !crate::validate::is_valid_node_name(&plan.node_name) {
        return Err(format!("节点名不合法: {}", plan.node_name));
    }
    if plan.data_start <= plan.header_row {
        return Err("数据起始行必须在表头行之后".to_string());
    }
    if plan.fields.is_empty() {
        return Err("至少映射一个字段".to_string());
    }
    if plan.fields[0].target_name != "id" {
        return Err("第一个字段必须是 id".to_string());
    }
    if plan.type_row == 0 && plan.fields[0].tbl_type != "int" {
        return Err("id 的类型必须是 int".to_string());
    }
    if let Some(issue) = field_issues(&plan.fields, plan.type_row).into_iter().next() {
        return Err(issue.message);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_book(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("foreign.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        ws.write_string(0, 0, "Name").unwrap();
        ws.write_string(0, 1, "ID").unwrap();
        ws.write_string(0, 2, "HP").unwrap();
        ws.write_string(1, 0, "亚瑟").unwrap();
        ws.write_string(1, 1, "1").unwrap();
        ws.write_string(1, 2, "100").unwrap();
        ws.write_string(2, 0, "兰斯").unwrap();
        ws.write_string(2, 1, "2").unwrap();
        ws.write_string(2, 2, "90").unwrap();
        std::fs::write(&path, wb.save_to_buffer().unwrap()).unwrap();
        path
    }

    #[test]
    fn normalizes_names_and_infers_types() {
        assert_eq!(normalize_field_name("ID"), "id");
        assert_eq!(normalize_field_name("Hero Name"), "hero_name");
        assert_eq!(normalize_field_name("heroName"), "hero_name");
        assert_eq!(normalize_field_name("HP"), "hp");
        assert_eq!(normalize_field_name("AABb"), "aa_bb");
        assert_eq!(normalize_field_name("URLPath"), "url_path");
        assert_eq!(normalize_field_name("IDCard"), "id_card");
        assert_eq!(normalize_field_name("attack_power"), "attack_power");
        assert_eq!(normalize_field_name("名称"), "ming_cheng");
        assert_eq!(normalize_field_name("攻击力"), "gong_ji_li");
        assert_eq!(normalize_field_name("道具ID"), "dao_ju_id");
        assert_eq!(normalize_node_name("hero base"), "HeroBase");
        assert_eq!(infer_type(&["1".into(), "2".into()]), "int");
        assert_eq!(infer_type(&["1.5".into()]), "float");
        assert_eq!(infer_type(&["true".into(), "FALSE".into()]), "bool");
        assert_eq!(infer_type(&["a".into()]), "str");
    }

    #[test]
    fn maps_one_row_header_with_id_not_first() {
        let dir = std::env::temp_dir().join(format!(
            "tablet_excel_mapping_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let path = write_book(&dir);
        assert_eq!(list_sheets(&path).unwrap(), vec!["HeroBase"]);
        assert_eq!(detect_header_row(&path, "HeroBase").unwrap(), 1);

        let preview = read_sheet_preview(&path, "HeroBase", 1, 2, 20).unwrap();
        assert_eq!(preview.columns, vec!["Name", "ID", "HP"]);
        let fields = suggest_fields(&preview);
        assert_eq!(fields[0].target_name, "id", "id 应被提到第一列");
        assert_eq!(fields[0].source_col, 2);
        assert_eq!(fields[1].target_name, "name");
        assert_eq!(fields[2].target_name, "hp");

        let plan = MappingPlan {
            source: path,
            sheet: "HeroBase".into(),
            header_row: 1,
            data_start: 2,
            desc_row: 0,
            type_row: 0,
            export_row: 0,
            group_name: "hero".into(),
            node_name: "HeroBase".into(),
            fields,
        };
        let (fields, records) = build_table_parts(&plan).unwrap();
        assert_eq!(fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["id", "name", "hp"]);
        assert_eq!(records[0], vec!["1".to_string(), "亚瑟".to_string(), "100".to_string()]);
        assert_eq!(records[1], vec!["2".to_string(), "兰斯".to_string(), "90".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_empty_or_duplicate_ids() {
        let dir = std::env::temp_dir().join(format!(
            "tablet_excel_mapping_bad_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("T").unwrap();
        ws.write_string(0, 0, "id").unwrap();
        ws.write_string(1, 0, "1").unwrap();
        ws.write_string(2, 0, "1").unwrap();
        std::fs::write(&path, wb.save_to_buffer().unwrap()).unwrap();

        let plan = MappingPlan {
            source: path,
            sheet: "T".into(),
            header_row: 1,
            data_start: 2,
            desc_row: 0,
            type_row: 0,
            export_row: 0,
            group_name: "g".into(),
            node_name: "T".into(),
            fields: vec![MappedField {
                target_name: "id".into(),
                source_col: 1,
                tbl_type: "int".into(),
                export: Export::ClientServer,
                desc: "ID".into(),
            }],
        };
        let err = build_table_parts(&plan).unwrap_err();
        assert!(err.contains("id 重复") && err.contains("第 3 行"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }
    fn tmp_book(label: &str, sheet: &str, rows: &[&[&str]]) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tablet_{}_{}_{}",
            label,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name(sheet).unwrap();
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32, c as u16, *v).unwrap();
            }
        }
        std::fs::write(&path, wb.save_to_buffer().unwrap()).unwrap();
        (dir, path)
    }

    #[test]
    fn detects_standard_four_row_table_layout() {
        let (dir, path) = tmp_book("layout4", "HeroBase", &[
            &["英雄ID", "名字", "血量"],
            &["cs", "cs", "s"],
            &["int", "str", "long"],
            &["ID", "Name", "Hp"],
            &["1001", "亚瑟", "1200"],
        ]);
        let d = detect_layout(&path, "HeroBase").unwrap();
        assert_eq!(d.kind, MappedNodeKind::Table);
        assert_eq!((d.header_row, d.data_start), (4, 5));
        assert_eq!((d.desc_row, d.type_row, d.export_row), (1, 3, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_constant_and_enum_layouts() {
        let (dir_c, path_c) = tmp_book("layout_const", "GameConst", &[
            &["name", "type", "value", "export", "desc"],
            &["max_level", "int", "100", "cs", "等级上限"],
        ]);
        let dc = detect_layout(&path_c, "GameConst").unwrap();
        assert_eq!(dc.kind, MappedNodeKind::Constant);
        assert_eq!((dc.header_row, dc.data_start), (1, 2));
        assert_eq!((dc.desc_row, dc.type_row, dc.export_row), (0, 0, 0));
        let _ = std::fs::remove_dir_all(&dir_c);

        let (dir_e, path_e) = tmp_book("layout_enum", "HeroType", &[
            &["id", "name", "desc"],
            &["1", "WARRIOR", "战士"],
        ]);
        let de = detect_layout(&path_e, "HeroType").unwrap();
        assert_eq!(de.kind, MappedNodeKind::Enum);
        assert_eq!((de.header_row, de.data_start), (1, 2));
        let _ = std::fs::remove_dir_all(&dir_e);
    }

    #[test]
    fn builds_constant_and_enum_entries() {
        let (dir_c, path_c) = tmp_book("build_const", "GameConst", &[
            &["name", "type", "value", "export", "desc"],
            &["max_level", "int", "100", "cs", "等级上限"],
            &["rate", "float", "1.5", "s", "倍率"],
        ]);
        let cf = |name: &str, col: u32| MappedField {
            target_name: name.into(), source_col: col, tbl_type: "str".into(),
            export: Export::ClientServer, desc: String::new(),
        };
        let plan = EntryMappingPlan {
            source: path_c.clone(), sheet: "GameConst".into(), header_row: 1, data_start: 2,
            group_name: "global".into(), node_name: "GameConst".into(),
            kind: MappedNodeKind::Constant,
            fields: vec![cf("name", 1), cf("type", 2), cf("value", 3), cf("export", 4), cf("desc", 5)],
        };
        match build_entries(&plan).unwrap() {
            MappedEntries::Constants(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].name, "max_level");
                assert_eq!(items[0].tbl_type, "int");
                assert_eq!(items[1].export, Export::ServerOnly);
                assert_eq!(items[1].desc, "倍率");
            }
            _ => panic!("应为常量"),
        }
        let _ = std::fs::remove_dir_all(&dir_c);

        let (dir_e, path_e) = tmp_book("build_enum", "HeroType", &[
            &["id", "name", "desc"],
            &["1", "WARRIOR", "战士"],
            &["2", "MAGE", "法师"],
        ]);
        let ef = |name: &str, col: u32| MappedField {
            target_name: name.into(), source_col: col, tbl_type: "str".into(),
            export: Export::ClientServer, desc: String::new(),
        };
        let plan = EntryMappingPlan {
            source: path_e.clone(), sheet: "HeroType".into(), header_row: 1, data_start: 2,
            group_name: "hero".into(), node_name: "HeroType".into(),
            kind: MappedNodeKind::Enum,
            fields: vec![ef("id", 1), ef("name", 2), ef("desc", 3)],
        };
        match build_entries(&plan).unwrap() {
            MappedEntries::Enums(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[1].name, "MAGE");
                assert_eq!(items[1].desc, "法师");
            }
            _ => panic!("应为枚举"),
        }
        let _ = std::fs::remove_dir_all(&dir_e);
    }

    #[test]
    fn sheet_infos_reads_used_range() {
        let dir = std::env::temp_dir().join(format!(
            "tablet_sheet_infos_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let path = write_book(&dir);
        let infos = sheet_infos(&path).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "HeroBase");
        assert_eq!((infos[0].columns, infos[0].rows), (3, 3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn field_issues_reports_each_rule_with_index() {
        let f = |name: &str, ty: &str, col: u32| MappedField {
            target_name: name.to_string(),
            source_col: col,
            tbl_type: ty.to_string(),
            export: Export::ClientServer,
            desc: String::new(),
        };
        let fields = vec![
            f("id", "str", 1),     // id 必须是 int
            f("type", "int", 2),   // Java/Go/Lua 关键字
            f("name", "str", 3),
            f("name", "str", 4),   // 重复
            f("bad-name", "str", 0), // 非法名 + 缺源列
        ];
        let issues = field_issues(&fields, 0);
        assert!(issues.iter().any(|i| i.index == 0 && i.message.contains("id 的类型必须是 int")));
        assert!(issues.iter().any(|i| i.index == 1 && i.message.contains("关键字")));
        assert!(issues.iter().any(|i| i.index == 3 && i.message.contains("字段名重复")));
        assert!(issues.iter().any(|i| i.index == 4 && i.message.contains("不是合法字段名")));
        assert!(issues.iter().any(|i| i.index == 4 && i.message.contains("没有选择源列")));
    }
}

#[cfg(test)]
mod semantic_row_tests {
    use super::*;

    #[test]
    fn detects_and_applies_desc_type_export_rows() {
        let dir = std::env::temp_dir().join(format!(
            "tablet_semantic_rows_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("semantic.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("HeroBase").unwrap();
        ws.write_string(0, 0, "英雄ID").unwrap();
        ws.write_string(0, 1, "名称").unwrap();
        ws.write_string(1, 0, "int").unwrap();
        ws.write_string(1, 1, "str").unwrap();
        ws.write_string(2, 0, "cs").unwrap();
        ws.write_string(2, 1, "s").unwrap();
        ws.write_string(3, 0, "id").unwrap();
        ws.write_string(3, 1, "name").unwrap();
        ws.write_string(4, 0, "1").unwrap();
        ws.write_string(4, 1, "亚瑟").unwrap();
        std::fs::write(&path, wb.save_to_buffer().unwrap()).unwrap();

        let (desc, ty, ex) = detect_semantic_rows(&path, "HeroBase", 4).unwrap();
        assert_eq!((desc, ty, ex), (1, 2, 3));

        let plan = MappingPlan {
            source: path.clone(),
            sheet: "HeroBase".into(),
            header_row: 4,
            data_start: 5,
            desc_row: desc,
            type_row: ty,
            export_row: ex,
            group_name: "hero".into(),
            node_name: "HeroBase".into(),
            fields: vec![
                MappedField { target_name: "id".into(), source_col: 1, tbl_type: "str".into(), export: Export::ClientServer, desc: String::new() },
                MappedField { target_name: "name".into(), source_col: 2, tbl_type: "str".into(), export: Export::ClientServer, desc: String::new() },
            ],
        };
        let (fields, records) = build_table_parts(&plan).unwrap();
        assert_eq!(fields[0].desc, "英雄ID");
        assert_eq!(fields[0].tbl_type, "int");
        assert_eq!(fields[1].export, Export::ServerOnly);
        assert_eq!(records[0], vec!["1".to_string(), "亚瑟".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_guess_ascii_letters_as_desc() {
        let dir = std::env::temp_dir().join(format!(
            "tablet_semantic_nod_esc_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("letters.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("S").unwrap();
        for (i, v) in ["a", "b"].iter().enumerate() { ws.write_string(0, i as u16, *v).unwrap(); }
        for (i, v) in ["id", "name"].iter().enumerate() { ws.write_string(1, i as u16, *v).unwrap(); }
        std::fs::write(&path, wb.save_to_buffer().unwrap()).unwrap();
        let (desc, _, _) = detect_semantic_rows(&path, "S", 2).unwrap();
        assert_eq!(desc, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

}

#[cfg(test)]
mod header_detect_tests {
    use super::*;

    #[test]
    fn prefers_real_field_names_over_excel_letters() {
        let dir = std::env::temp_dir().join(format!(
            "tablet_header_detect_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("h.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("S").unwrap();
        for (i, v) in ["a", "b", "v"].iter().enumerate() { ws.write_string(0, i as u16, *v).unwrap(); }
        for (i, v) in ["id", "name", "kind"].iter().enumerate() { ws.write_string(1, i as u16, *v).unwrap(); }
        for (i, v) in ["3001", "伐木场", "1"].iter().enumerate() { ws.write_string(2, i as u16, *v).unwrap(); }
        std::fs::write(&path, wb.save_to_buffer().unwrap()).unwrap();
        assert_eq!(detect_header_row(&path, "S").unwrap(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
