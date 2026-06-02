// 通用工具：列字母编号、单元格 raw 读取、单格校验消息。
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
    use tbl_core::validate::{validate_table, validate_constant, validate_enum, RefIndex};
    let sep = &state.engine.project().config.separators;
    let errors = match &state.selected {
        Some(SelectedNode::Table { group, name, .. }) => {
            let table = state.engine.find_table(group, name)?;
            let refs = RefIndex::build(&state.engine.project().groups);
            validate_table(table, sep, Some(&refs))
        }
        Some(SelectedNode::Constant { group, name, .. }) => {
            let constant = state.engine.find_constant(group, name)?;
            let refs = RefIndex::build(&state.engine.project().groups);
            let allow_ref = state.engine.project().config.ui.as_ref()
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
