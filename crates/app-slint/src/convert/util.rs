// 通用工具：列字母编号、单元格 raw 读取、单格校验消息、TSV 转义/解析。
//
// 这些函数没有 UI 依赖，只关心「从 AppState 读出某格底层值」或纯字符串映射。

use crate::state::{AppState, SelectedNode};

/// grid 末尾追加的空白占位行数。复制粘贴 / 拖选可超过有效行落到占位行。
pub const EXTRA_ROWS: usize = 5;

/// 列序号 → Excel 列字母（0=A、25=Z、26=AA...）。
pub fn col_letter(idx: usize) -> String {
    let mut s = String::new();
    let mut n = idx;
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 { break; }
        n = n / 26 - 1;
    }
    s
}

/// 单行化：多行值只取**第一行**、末尾补 `…`，供「固定单行行高」的展示位置使用。
///
/// 单元格值**允许**含换行（txt 列合法、Excel 导入也会带 Alt+Enter，见 @04.3.2 / @01），
/// 但网格行高固定 22px、折叠公式栏也是单行盒子。原样喂给 slint 会让多行文本溢出盒外：
/// slint 的 `Text` 即使 `wrap: no-wrap` 也会在 `\n` 处强制断行，而盒子里没有任何
/// 纵向钳制手段（1.17 没有 `maximum-line-count`，`overflow: elide` 只在盒子有确定高度时
/// 才按行数截断）——结果是多行值盖住上下邻行，整块表格糊掉。
///
/// 之所以「取第一行 + …」而不是插一个 `⏎` 之类的换行标记：符号字形不一定在字体回退链里
/// （实测 `⏎` U+23CE、`▾` U+25BE 在本机渲染成豆腐块），而 `…` 是 slint `overflow: elide`
/// 自己就在用的字符，一定画得出来。
///
/// 所以凡是「单行展示位」一律先过这里。**只影响展示**：复制 / 保存 / 校验仍从
/// `AppState.engine` 读存储真值，换行不会被这个函数改掉；展开态公式栏显示真实换行。
pub fn one_line(s: &str) -> String {
    match s.find(['\n', '\r']) {
        // 绝大多数值没有换行 → 原样返回，省一次分配
        None => s.to_string(),
        Some(i) => {
            let mut out = s[..i].trim_end().to_string();
            out.push('…');
            out
        }
    }
}

// ── TSV 剪贴板 ──────────────────────────────────────────────
// RFC 4180 风格：单元格含 \t / \n / " → 用 "..." 包裹，内部 " → "" 转义。
// 与 Excel、WPS、Google Sheets 的纯文本剪贴板格式兼容。

/// 单格值 → TSV 安全表示。
///
/// `\r` 也必须进引号：`tsv_parse` 把「不跟在 `\n` 后面的 `\r`」当行分隔符（@见下），
/// 不引号的话含裸 `\r` 的单元格复制出去再粘回来会变成两行——精度丢失。
pub fn tsv_escape_cell(cell: &str) -> String {
    if cell.contains('\t') || cell.contains('\n') || cell.contains('\r') || cell.contains('"') {
        let escaped = cell.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        cell.to_string()
    }
}

/// 解析 TSV 文本 → 二维格矩阵（已反转义）。
pub fn tsv_parse(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match (ch, in_quoted) {
            ('"', false) => { in_quoted = true; }
            ('"', true) => {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next(); // skip doubled quote
                } else {
                    in_quoted = false;
                }
            }
            ('\t', false) => {
                row.push(std::mem::take(&mut cell));
            }
            ('\n', false) => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            ('\r', false) => {
                // skip CR in CRLF
                if chars.peek() == Some(&'\n') { continue; }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            _ => { cell.push(ch); }
        }
    }
    // 最后一条（无尾随 \n 时）
    row.push(cell);
    if !row.is_empty() && !(row.len() == 1 && row[0].is_empty() && rows.is_empty()) {
        rows.push(row);
    }
    while rows.last().map_or(false, |r| r.iter().all(|c| c.is_empty())) {
        rows.pop();
    }
    rows
}

/// Copy: 选区 → TSV 文本（cell 值经转义，与 Excel 剪贴板兼容）。
pub fn build_tsv(state: &AppState, r1: usize, c1: usize, r2: usize, c2: usize) -> String {
    (r1..=r2).map(|r| {
        (c1..=c2).map(|c| tsv_escape_cell(&raw_cell_for(state, r, c)))
            .collect::<Vec<_>>().join("\t")
    }).collect::<Vec<_>>().join("\r\n")
}

/// 读取真实存储值（不经过 display 翻译）。供 inline 编辑等取初始文本。
pub fn raw_cell_for(state: &AppState, r: usize, c: usize) -> String {
    match &state.selected {
        Some(SelectedNode::Table { group, name, .. }) => state.engine.find_table(group, name)
            .and_then(|t| t.records.get(r).and_then(|row| row.get(c)).cloned())
            .unwrap_or_default(),
        Some(SelectedNode::Constant { group, name, .. }) => state.engine.find_constant(group, name)
            .and_then(|cst| cst.entries.get(r))
            .map(|e| match c {
                0 => e.name.clone(),
                1 => e.tbl_type.clone(),
                2 => e.value.clone(),
                3 => e.export.code().to_string(),
                4 => e.desc.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        Some(SelectedNode::Enum { group, name, .. }) => state.engine.find_enum(group, name)
            .and_then(|en| en.entries.get(r))
            .map(|e| match c {
                0 => e.id.clone(),
                1 => e.name.clone(),
                2 => e.desc.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        Some(SelectedNode::Project { .. }) | Some(SelectedNode::Group { .. }) | None => String::new(),
    }
}

/// 取 (r,c) 单元格的具体验证错误信息；当前节点不存在 / 该格无错误时返回 None。
/// 用于 StatusBar 在选中单格时把"红框是因为什么"展示给用户，对齐文档 §5.4 实时验证 UX。
pub(super) fn cell_validation_message(state: &AppState, r: usize, c: usize) -> Option<String> {
    use tablet_core::validate::{validate_table, validate_constant, validate_enum, RefIndex};
    let sep = &state.engine.project().schema.separators;
    let errors = match &state.selected {
        Some(SelectedNode::Table { group, name, .. }) => {
            let table = state.engine.find_table(group, name)?;
            let refs = RefIndex::build(&state.engine.project().groups);
            validate_table(table, sep, Some(&refs))
        }
        Some(SelectedNode::Constant { group, name, .. }) => {
            let constant = state.engine.find_constant(group, name)?;
            let refs = RefIndex::build(&state.engine.project().groups);
            let allow_ref = state.engine.global_config.ui.as_ref()
                .map_or(true, |u| u.constant_ref_allowed);
            validate_constant(constant, sep, allow_ref, Some(&refs))
        }
        Some(SelectedNode::Enum { group, name, .. }) => {
            let enum_def = state.engine.find_enum(group, name)?;
            validate_enum(enum_def)
        }
        Some(SelectedNode::Project { .. }) | Some(SelectedNode::Group { .. }) | None => return None,
    };
    errors.into_iter()
        .find(|e| !e.is_schema() && e.row == r && e.col == c)
        .map(|e| e.message)
}

#[cfg(test)]
mod tests {
    use super::one_line;

    #[test]
    fn one_line_keeps_first_line_and_marks_truncation() {
        assert_eq!(one_line("战士"), "战士");
        assert_eq!(one_line("a\nb"), "a…");
        assert_eq!(one_line("a\r\nb"), "a…");
        // 裸 CR（老 Mac / 部分 Excel 粘贴）同样只取第一行
        assert_eq!(one_line("a\rb"), "a…");
        // 首行末尾的空白不带进展示串
        assert_eq!(one_line("a  \nb"), "a…");
        // 以换行开头 → 首行是空的，只留截断标记
        assert_eq!(one_line("\na"), "…");
        // 制表符不是换行，保持原样（str 列的合法性交给校验层管）
        assert_eq!(one_line("a\tb"), "a\tb");
    }

    #[test]
    fn one_line_output_never_contains_a_newline() {
        // 这是关键不变式：喂给固定单行行高盒子的串必须无 \n / \r，
        // 否则 slint 的 Text 会在 \n 处强制断行、溢出盒外。
        for s in ["", "x", "a\nb", "a\rb", "a\r\nb", "\n\r\n\r", "多\n行\r\n文本\n"] {
            let out = one_line(s);
            assert!(!out.contains('\n') && !out.contains('\r'), "leaked newline: {out:?}");
        }
    }
}

#[cfg(test)]
mod tsv_tests {
    use super::{tsv_escape_cell, tsv_parse};

    /// 单元格值 → 复制出 → 粘回来，必须逐字节相同（@04.3.2 剪贴板 round-trip 约定）。
    fn round_trip(value: &str) {
        let tsv = tsv_escape_cell(value);
        let parsed = tsv_parse(&tsv);
        assert_eq!(parsed.len(), 1, "value {value:?} produced {} rows", parsed.len());
        assert_eq!(parsed[0].len(), 1, "value {value:?} produced {} cols", parsed[0].len());
        assert_eq!(parsed[0][0], value, "round-trip changed value");
    }

    #[test]
    fn cell_values_survive_clipboard_round_trip() {
        round_trip("战士");
        round_trip("a\tb");           // tab（csv/excel 常见）
        round_trip("line1\nline2");   // 换行
        round_trip("line1\r\nline2"); // CRLF
        round_trip("a\rb");           // 裸 CR —— 曾经不引号，粘回来会裂成两行
        round_trip("he said \"hi\""); // 引号
        round_trip("{\"k\":\n\t\"v\"}"); // JSON 文本，tab + 换行 + 引号齐活
        // 空值不在 round-trip 断言里：空剪贴板解析出 0 行（是 no-op，不是丢数据）
        assert!(tsv_parse(&tsv_escape_cell("")).is_empty());
    }
}
