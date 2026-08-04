// grid 操作 helpers：被 grid.rs / context_menu.rs / focus.rs 共用的 perform_* 系列
// + commit_editing + selection 计算 + popup export 写回。
//
// 本模块没有自己的 UI，全部 `pub(crate) fn`。

use std::cell::RefCell;
use std::rc::Rc;

use arboard::Clipboard;
use tablet_core::model::Export;

use crate::convert;
use crate::state::{self, AppState, GridSelection, SelectedNode};
use crate::{AppWindow, CellKind};

/// 用 slint 的 editing-buffer property 当前值，写回当前 editing cell / header cell。
/// editing buffer 是 slint LineEdit 的 text 双向绑定，用户输入实时同步在 ui 端。
/// editing_header_row >= 0 时走 commit_header_edit；否则走 set_cell。
pub(crate) fn commit_editing(ui_weak: &slint::Weak<AppWindow>, state: &Rc<RefCell<AppState>>) {
    let buf = match ui_weak.upgrade() {
        Some(ui_h) => ui_h.get_editing_buffer().to_string(),
        None => return,
    };
    let mut st = state.borrow_mut();
    if st.editing_header_row >= 0 && st.editing_header_col >= 0 {
        let hi = st.editing_header_row as usize;
        let ci = st.editing_header_col as usize;
        st.set_header_cell(hi, ci, buf);
        st.editing_header_row = -1;
        st.editing_header_col = -1;
        st.editing_buffer.clear();
        st.editing_in_formula = false;
        return;
    }
    if let Some((r, c)) = st.editing {
        st.set_cell(r, c, &buf);
        st.editing = None;
        st.editing_buffer.clear();
        st.editing_in_formula = false;
        st.editing_header_row = -1;
        st.editing_header_col = -1;
    }
}

/// 把 Rust 端 ColumnKind 映射成 slint 端 CellKind（仅用于 column-kinds 数据列指引）。
pub(crate) fn column_kind_to_slint(k: &state::ColumnKind) -> CellKind {
    match k {
        state::ColumnKind::ReadOnly => CellKind::ReadOnly,
        state::ColumnKind::Text => CellKind::Text,
        state::ColumnKind::Ref { .. } => CellKind::Ref,
        state::ColumnKind::TypeEnumCol => CellKind::TypeEnumCol,
        state::ColumnKind::ExportEnumCol => CellKind::ExportEnumCol,
    }
}

/// 算「这个 (row,col) 实际行为应该按哪种 ColumnKind 走」。
///
/// 默认就是列级 `grid_column_kinds[col]`；对 Constant 表 value 列(c=2)做单元格级覆写：
/// 如果该行 entry 的 `tbl_type` 解析后是 `Paradigm::Ref`，则把 Text 升级为 `Ref { target }`。
/// 这让 Constant 引用值单元格复用 Table 的 RefPicker 流程（双击 / 单击触发、ctx menu「选择引用...」）。
pub(crate) fn effective_column_kind_at(state: &AppState, r: usize, c: usize) -> Option<state::ColumnKind> {
    let base = state.grid_column_kinds.get(c).cloned()?;
    if c != 2 { return Some(base); }
    let (group, name) = match &state.selected {
        Some(state::SelectedNode::Constant { group, name, .. }) => (group.clone(), name.clone()),
        _ => return Some(base),
    };
    let constant = state.engine.find_constant(&group, &name)?;
    let entry = constant.entries.get(r)?;
    let t = tablet_core::types::TblType::parse(&entry.tbl_type)?;
    if t.paradigm != tablet_core::types::Paradigm::Ref { return Some(base); }
    let target = t.ref_name?;
    Some(state::ColumnKind::Ref { target })
}

/// 计算 export popup 的 current-index：从当前 cell / header 读出 export code，映射到 0..3。
/// popup 列表顺序：["前后端","客户端","服务器","不导出"]。
pub(crate) fn compute_editing_export_index(
    state: &Rc<RefCell<AppState>>,
    editing_r: i32,
    editing_c: i32,
    editing_header_col: i32,
) -> i32 {
    let st = state.borrow();
    // 优先级：editing_header_col（双击编辑表头）> editing_r/c（双击编辑数据格）> 当前 GridSelection（单击选中态）。
    // 双击 ExportEnum cell 的瞬间不再走 editing，所以必须用 selection 兜底，否则 popup 显示的勾选项是上一次的旧值。
    let (sel_header_col, sel_r, sel_c): (i32, i32, i32) = match st.grid_selection {
        GridSelection::Cell(r, c) => (-1, r as i32, c as i32),
        _ => (-1, -1, -1),
    };
    let header_col = if editing_header_col >= 0 { editing_header_col } else { sel_header_col };
    let (data_r, data_c) = if editing_r >= 0 && editing_c >= 0 { (editing_r, editing_c) } else { (sel_r, sel_c) };
    let code: Option<String> = if header_col >= 0 {
        if let Some(SelectedNode::Table { group, name, .. }) = &st.selected {
            st.engine.find_table(group, name)
                .and_then(|t| t.schema.fields.get(header_col as usize))
                .map(|f| f.export.code().to_string())
        } else { None }
    } else if data_r >= 0 && data_c >= 0 {
        match &st.selected {
            Some(SelectedNode::Constant { group, name, .. }) => {
                st.engine.find_constant(group, name)
                    .and_then(|c| c.entries.get(data_r as usize))
                    .map(|e| e.export.code().to_string())
            }
            _ => None,
        }
    } else { None };
    match code.as_deref() {
        Some("cs") => 0,
        Some("c") => 1,
        Some("s") => 2,
        Some("-") => 3,
        _ => 0,
    }
}

/// 取当前 GridSelection 的 anchor (左上角)；None=无选区。
pub(crate) fn selection_anchor(state: &Rc<RefCell<AppState>>) -> Option<(usize, usize)> {
    let st = state.borrow();
    match st.grid_selection {
        GridSelection::Cell(r, c) => Some((r, c)),
        GridSelection::CellRange { r1, c1, r2, c2 } => Some((r1.min(r2), c1.min(c2))),
        GridSelection::Row(r) => Some((r, 0)),
        GridSelection::Col(c) => Some((0, c)),
        GridSelection::None => None,
    }
}

/// 取当前选中节点的 (rows, cols)，用于鼠标拖选 / 区域操作的边界裁剪。
/// rows 包含 EXTRA_ROWS 占位行：用户可以拖选 / 粘贴 / 清空到表尾的空白行；
/// engine.paste_*_data 内部会按需 push 新行。
/// 没有选中节点时返回 (0, 0)。
pub(crate) fn dims(state: &Rc<RefCell<AppState>>) -> (usize, usize) {
    let st = state.borrow();
    let rows = st.grid_data_count + convert::EXTRA_ROWS;
    let cols = match &st.selected {
        Some(SelectedNode::Table { group, name, .. }) => st.engine.find_table(group, name)
            .map(|t| t.schema.fields.len()).unwrap_or(0),
        Some(SelectedNode::Constant { .. }) => 5,
        Some(SelectedNode::Enum { .. }) => 3,
        Some(SelectedNode::Project { .. }) | Some(SelectedNode::Group { .. }) | None => 0,
    };
    (rows, cols)
}

/// Constant ExportEnumCol popup 选项被选中：写回 entries[r].export。
/// (r,c) 来自 slint cell 端的 ri/ci 闭包，不依赖 editing 状态（popup 由 cell-clicked 即时弹出）。
pub(crate) fn on_cell_export_selected(state: &Rc<RefCell<AppState>>, r: i32, _c: i32, idx: i32) {
    let opt = match idx {
        0 => Export::ClientServer,
        1 => Export::ClientOnly,
        2 => Export::ServerOnly,
        3 => Export::None,
        _ => return,
    };
    let mut st = state.borrow_mut();
    let (group, name) = match &st.selected {
        Some(SelectedNode::Constant { group, name, .. }) => (group.clone(), name.clone()),
        _ => return,
    };
    st.engine.commit_constant_cell(&group, &name, r as usize, 3, opt.code().to_string());
    if st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
}

/// Table 表头 export 行的 popup 选项被选中：写回 schema.fields[col].export。
pub(crate) fn on_header_export_selected(state: &Rc<RefCell<AppState>>, col: i32, idx: i32) {
    let opt = match idx {
        0 => Export::ClientServer,
        1 => Export::ClientOnly,
        2 => Export::ServerOnly,
        3 => Export::None,
        _ => return,
    };
    let mut st = state.borrow_mut();
    let (group, name) = match &st.selected {
        Some(SelectedNode::Table { group, name, .. }) => (group.clone(), name.clone()),
        _ => return,
    };
    st.engine.commit_header_edit(&group, &name, 1, col as usize, opt.code().to_string());
    if st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
}

/// 列右键操作（依赖当前选中 Table）。
pub(crate) fn perform_col_action(state: &Rc<RefCell<AppState>>, col: usize, action: &str) {
    let mut st = state.borrow_mut();
    let (group, name) = match &st.selected {
        Some(SelectedNode::Table { group, name, .. }) => (group.clone(), name.clone()),
        _ => return,
    };
    match action {
        "grid.col-insert-left" => st.engine.insert_column(&group, &name, col),
        "grid.col-insert-right" => st.engine.insert_column(&group, &name, col + 1),
        "grid.col-delete" => st.engine.delete_column(&group, &name, col),
        _ => {}
    }
    if st.realtime_validate { st.engine.revalidate(&group, &name); }
}

/// 行右键操作（Table 走 insert/delete_row；Constant/Enum 直接增删 entries）。
pub(crate) fn perform_row_action(state: &Rc<RefCell<AppState>>, row: usize, action: &str) {
    let mut st = state.borrow_mut();
    let (group, name, is_table, is_constant, is_enum) = match &st.selected {
        Some(SelectedNode::Table { group, name, .. }) => (group.clone(), name.clone(), true, false, false),
        Some(SelectedNode::Constant { group, name, .. }) => (group.clone(), name.clone(), false, true, false),
        Some(SelectedNode::Enum { group, name, .. }) => (group.clone(), name.clone(), false, false, true),
        _ => return,
    };
    if is_table {
        match action {
            "grid.row-insert-above" => st.engine.insert_row(&group, &name, row),
            "grid.row-insert-below" => st.engine.insert_row(&group, &name, row + 1),
            "grid.row-delete" => st.engine.delete_row(&group, &name, row),
            _ => {}
        }
    } else if is_constant {
        if let Some(g) = st.engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                use tablet_core::model::ConstEntry;
                match action {
                    "grid.row-insert-above" | "grid.row-insert-below" => {
                        let at = if action == "grid.row-insert-above" { row } else { row + 1 };
                        let at = at.min(c.entries.len());
                        c.entries.insert(at, ConstEntry {
                            name: String::new(), tbl_type: "str".to_string(),
                            value: String::new(), export: Export::ClientServer, desc: String::new(),
                        });
                        c.update_dirty();
                    }
                    "grid.row-delete" => {
                        if row < c.entries.len() { c.entries.remove(row); c.update_dirty(); }
                    }
                    _ => {}
                }
            }
        }
    } else if is_enum {
        if let Some(g) = st.engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(e) = g.enums.iter_mut().find(|e| e.name == name) {
                use tablet_core::model::EnumEntry;
                match action {
                    "grid.row-insert-above" | "grid.row-insert-below" => {
                        let at = if action == "grid.row-insert-above" { row } else { row + 1 };
                        let at = at.min(e.entries.len());
                        e.entries.insert(at, EnumEntry { id: String::new(), name: String::new(), desc: String::new() });
                        e.update_dirty();
                    }
                    "grid.row-delete" => {
                        if row < e.entries.len() { e.entries.remove(row); e.update_dirty(); }
                    }
                    _ => {}
                }
            }
        }
    }
    if st.realtime_validate { st.engine.revalidate(&group, &name); }
}

/// 复制/粘贴/清空：单元格或矩形区域。
/// 选区由 GridSelection 决定（单格 / 区域 / 整行 / 整列）；由调用方确保命中合法选区。
/// `tag` 决定日志前缀：右键传中文动词（复制/粘贴/清空），键盘传按键组合（Ctrl+C/Ctrl+V/Delete）。
///
/// 范围语义对齐 Excel：
/// - Copy: TSV 拼接区域，UI 日志只打范围（如 `[Ctrl+C] B2:D5 (4行×3列)`），不打内容
/// - Paste: clipboard TSV 从锚点展开覆盖
/// - Clear: 清空区域所有 cell
pub(crate) fn perform_action(state: &Rc<RefCell<AppState>>, action: &str, tag: &str) {
    let (r1, c1, r2, c2) = match resolve_selection_rect(state) {
        Some(v) => v,
        None => return,
    };
    let coord_label = if r1 == r2 && c1 == c2 {
        format!("{}{}", convert::col_letter(c1), r1 + 1)
    } else {
        format!(
            "{}{}:{}{} ({}行×{}列)",
            convert::col_letter(c1), r1 + 1,
            convert::col_letter(c2), r2 + 1,
            r2 - r1 + 1, c2 - c1 + 1,
        )
    };
    let is_single = r1 == r2 && c1 == c2;
    match action {
        "grid.cell-copy" => {
            let tsv = {
                let st = state.borrow();
                convert::build_tsv(&st, r1, c1, r2, c2)
            };
            match Clipboard::new().and_then(|mut cb| cb.set_text(tsv.clone())) {
                Ok(()) => {
                    let msg = if is_single {
                        format!("[{}] {} = \"{}\"", tag, coord_label, tsv)
                    } else {
                        format!("[{}] {}", tag, coord_label)
                    };
                    state.borrow_mut().engine.ui_log(msg);
                }
                Err(e) => state.borrow_mut().engine.error_log(format!("[{}] {} 失败: {}", tag, coord_label, e)),
            }
        }
        "grid.cell-paste" => {
            let text = match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                Ok(t) => t,
                Err(e) => {
                    state.borrow_mut().engine.error_log(format!("[{}] {} 读剪贴板失败: {}", tag, coord_label, e));
                    return;
                }
            };
            let grid = convert::tsv_parse(&text);
            let row_n = grid.len();
            let col_n = grid.first().map_or(0, |r| r.len());
            if row_n == 0 || col_n == 0 { return; }
            let clip_is_single = row_n == 1 && col_n == 1;
            if clip_is_single && is_single {
                let single = grid[0][0].clone();
                let before = {
                    let st = state.borrow();
                    convert::raw_cell_for(&st, r1, c1)
                };
                let mut st = state.borrow_mut();
                st.set_cell(r1, c1, &single);
                st.engine.ui_log(format!("[{}] {} \"{}\" → \"{}\"", tag, coord_label, before, single));
            } else {
                paste_region(state, r1, c1, &grid);
                let dst = format!(
                    "{}{}:{}{}",
                    convert::col_letter(c1), r1 + 1,
                    convert::col_letter(c1 + col_n - 1), r1 + row_n,
                );
                state.borrow_mut().engine.ui_log(format!("[{}] {} → {} ({}行×{}列)", tag, coord_label, dst, row_n, col_n));
            }
        }
        "grid.cell-clear" => {
            if is_single {
                let before = {
                    let st = state.borrow();
                    convert::raw_cell_for(&st, r1, c1)
                };
                let mut st = state.borrow_mut();
                st.set_cell(r1, c1, "");
                st.engine.ui_log(format!("[{}] {} \"{}\" → \"\"", tag, coord_label, before));
            } else {
                clear_region(state, r1, c1, r2, c2);
                state.borrow_mut().engine.ui_log(format!("[{}] {}", tag, coord_label));
            }
        }
        "grid.cell-cut" => {
            let tsv = {
                let st = state.borrow();
                (r1..=r2).map(|r| {
                    (c1..=c2).map(|c| convert::raw_cell_for(&st, r, c))
                        .collect::<Vec<_>>().join("\t")
                }).collect::<Vec<_>>().join("\n")
            };
            match Clipboard::new().and_then(|mut cb| cb.set_text(tsv.clone())) {
                Ok(()) => {
                    if is_single {
                        let mut st = state.borrow_mut();
                        st.set_cell(r1, c1, "");
                        st.engine.ui_log(format!("[{}] {} \"{}\" → \"\"", tag, coord_label, tsv));
                    } else {
                        clear_region(state, r1, c1, r2, c2);
                        state.borrow_mut().engine.ui_log(format!("[{}] {}", tag, coord_label));
                    }
                }
                Err(e) => state.borrow_mut().engine.error_log(format!("[{}] {} 失败: {}", tag, coord_label, e)),
            }
        }
        _ => {}
    }
}

/// 把当前 GridSelection 解析成裁剪到表实际尺寸的矩形 (r1,c1,r2,c2)。
/// 整行/整列时按 dims 截断。无选区返回 None。
fn resolve_selection_rect(state: &Rc<RefCell<AppState>>) -> Option<(usize, usize, usize, usize)> {
    let (rows, cols) = dims(state);
    if rows == 0 || cols == 0 { return None; }
    let st = state.borrow();
    let (rmin, rmax, cmin, cmax) = st.grid_selection.bounds()?;
    let r1 = rmin.min(rows - 1);
    let r2 = rmax.min(rows - 1);
    let c1 = cmin.min(cols - 1);
    let c2 = cmax.min(cols - 1);
    Some((r1, c1, r2, c2))
}

/// 区域粘贴：按当前选中节点类型走 engine.paste_*_data。
fn paste_region(state: &Rc<RefCell<AppState>>, r1: usize, c1: usize, grid: &[Vec<String>]) {
    let (group, name, kind) = match state.borrow().selected.clone() {
        Some(SelectedNode::Table { group, name, .. }) => (group, name, "table"),
        Some(SelectedNode::Constant { group, name, .. }) => (group, name, "constant"),
        Some(SelectedNode::Enum { group, name, .. }) => (group, name, "enum"),
        _ => return,
    };
    let mut st = state.borrow_mut();
    match kind {
        "table" => st.engine.paste_table_data(&group, &name, r1, c1, grid),
        "constant" => st.engine.paste_constant_data(&group, &name, r1, c1, grid),
        _ => st.engine.paste_enum_data(&group, &name, r1, c1, grid),
    }
    if st.realtime_validate { st.engine.revalidate(&group, &name); }
}

/// 区域清空：按当前选中节点类型走 engine.clear_*_cells。
fn clear_region(state: &Rc<RefCell<AppState>>, r1: usize, c1: usize, r2: usize, c2: usize) {
    let (group, name, kind) = match state.borrow().selected.clone() {
        Some(SelectedNode::Table { group, name, .. }) => (group, name, "table"),
        Some(SelectedNode::Constant { group, name, .. }) => (group, name, "constant"),
        Some(SelectedNode::Enum { group, name, .. }) => (group, name, "enum"),
        _ => return,
    };
    let cells: Vec<(usize, usize)> = (r1..=r2).flat_map(|r| (c1..=c2).map(move |c| (r, c))).collect();
    let mut st = state.borrow_mut();
    match kind {
        "table" => st.engine.clear_table_cells(&group, &name, &cells),
        "constant" => st.engine.clear_constant_cells(&group, &name, &cells),
        _ => st.engine.clear_enum_cells(&group, &name, &cells),
    }
    if st.realtime_validate { st.engine.revalidate(&group, &name); }
}
