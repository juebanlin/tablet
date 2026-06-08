// 表格区：push grid 快照 + 接通各类 cell/header/row/col 的事件回调。
//
// 关键约束：单击千万不能对 picker cell 做 push（model 重建会销毁 TouchArea 实例，
// slint 双击事件需要同一 TouchArea 接连两次 clicked）。所以单击场景用
// `push_selection_only` 仅刷选区相关属性，不重建 model。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::convert;
use crate::state::{self, AppState, CtxMenuKind, GridSelection, SelectedNode};
use crate::{dialogs, AppWindow, CellKind};

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let snap = convert::build_grid(&state.borrow());
    let (editing_r, editing_c, editing_buffer, editing_in_formula, editing_header_row, editing_header_col) = {
        let st = state.borrow();
        match st.editing {
            Some((r, c)) => (r as i32, c as i32, st.editing_buffer.clone(), st.editing_in_formula, st.editing_header_row, st.editing_header_col),
            None => (-1, -1, st.editing_buffer.clone(), false, st.editing_header_row, st.editing_header_col),
        }
    };
    let editing_export_index = super::grid_actions::compute_editing_export_index(state, editing_r, editing_c, editing_header_col);
    let slint_column_kinds: Vec<CellKind> = snap.column_kinds.iter()
        .map(super::grid_actions::column_kind_to_slint).collect();
    {
        let mut st = state.borrow_mut();
        st.grid_column_kinds = snap.column_kinds.clone();
        st.grid_header_kinds = snap.header_kinds.clone();
        st.grid_data_count = snap.data_count;
    }
    ui_h.set_grid_title(snap.title.into());
    ui_h.set_grid_subtitle(snap.subtitle.into());
    // 「枚举显示名字」对 Table / Constant 启用：
    // - Table：列级 @ref 列；
    // - Constant：value 列单元格级 @ref 类型（视行 entry.tbl_type 而定）。
    // Enum 没有引用列，禁用。
    ui_h.set_show_enum_name_enabled(matches!(
        state.borrow().selected,
        Some(SelectedNode::Table { .. } | SelectedNode::Constant { .. })
    ));
    // 「Excel 编辑」绑定具体节点（Table / Constant / Enum 都接受）；
    // - 未选中或选 Project / Group → disabled
    // - 已有 Excel 编辑会话进行中 → disabled（@plans §2.4 全局锁）
    {
        let st = state.borrow();
        let leaf_selected = matches!(
            st.selected,
            Some(SelectedNode::Table { .. } | SelectedNode::Constant { .. } | SelectedNode::Enum { .. })
        );
        ui_h.set_excel_edit_enabled(leaf_selected && st.excel_session.is_none());
    }
    ui_h.set_grid_col_count(snap.col_count);
    ui_h.set_grid_header_rows(slint::ModelRc::new(slint::VecModel::from(
        snap.header_rows.into_iter()
            .map(|r| slint::ModelRc::new(slint::VecModel::from(r)))
            .collect::<Vec<_>>(),
    )));
    ui_h.set_grid_data_rows(slint::ModelRc::new(slint::VecModel::from(snap.data_rows)));
    ui_h.set_grid_column_kinds(slint::ModelRc::new(slint::VecModel::from(slint_column_kinds)));
    ui_h.set_grid_selected_col(snap.selected_col);
    ui_h.set_grid_selected_row(snap.selected_row);
    ui_h.set_grid_selected_cell_row(snap.selected_cell_row);
    ui_h.set_grid_selected_cell_col(snap.selected_cell_col);
    ui_h.set_grid_range_row_min(snap.range_row_min);
    ui_h.set_grid_range_row_max(snap.range_row_max);
    ui_h.set_grid_range_col_min(snap.range_col_min);
    ui_h.set_grid_range_col_max(snap.range_col_max);
    let editing_data_cell = editing_r >= 0 && !editing_in_formula && editing_header_row < 0;
    ui_h.set_grid_editing_row(if editing_data_cell { editing_r } else { -1 });
    ui_h.set_grid_editing_col(if editing_data_cell { editing_c } else { -1 });
    ui_h.set_grid_editing_header_row(editing_header_row);
    ui_h.set_grid_editing_header_col(editing_header_col);
    ui_h.set_grid_editing_export_index(editing_export_index);
    ui_h.set_editing_buffer(editing_buffer.into());
    // 公式栏 LineEdit 仅在「在公式栏编辑」时显示
    ui_h.set_formula_editing(editing_r >= 0 && editing_in_formula);
    ui_h.set_coord(snap.coord.into());
    ui_h.set_formula_display(snap.formula_display.into());
    ui_h.set_formula_editable(snap.formula_editable);
    ui_h.set_selection_info(snap.selection_info.into());
    ui_h.set_hover_info(snap.hover_info.into());
}

/// 仅更新选区相关的轻量属性（不重建 header/data 模型）。
/// cell-clicked / row-num-clicked / col-letter-clicked 用，避免 model 重建
/// 而打断 slint 双击 sequence。
fn push_selection_only(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let snap = convert::build_grid(&state.borrow());
    ui_h.set_grid_selected_col(snap.selected_col);
    ui_h.set_grid_selected_row(snap.selected_row);
    ui_h.set_grid_selected_cell_row(snap.selected_cell_row);
    ui_h.set_grid_selected_cell_col(snap.selected_cell_col);
    ui_h.set_grid_range_row_min(snap.range_row_min);
    ui_h.set_grid_range_row_max(snap.range_row_max);
    ui_h.set_grid_range_col_min(snap.range_col_min);
    ui_h.set_grid_range_col_max(snap.range_col_max);
    // editing-export-index 跟随选区变化（Constant ExportEnumCol 单击切换 row 时，popup 当前勾选项要换）
    ui_h.set_grid_editing_export_index(super::grid_actions::compute_editing_export_index(state, -1, -1, -1));
    ui_h.set_coord(snap.coord.into());
    ui_h.set_formula_display(snap.formula_display.into());
    ui_h.set_formula_editable(snap.formula_editable);
    ui_h.set_selection_info(snap.selection_info.into());
    ui_h.set_hover_info(snap.hover_info.into());
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    use super::grid_actions;

    // 「枚举显示名字」开关 → 重建 grid 让 @EnumName 列展示切换
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_show_enum_name_toggled(move |c| {
            s.borrow_mut().view_show_enum_name = c;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 「Excel 编辑」按钮：选中叶节点（Table/Constant/Enum）时调起单 sheet xlsx
    // 编辑流程（@docs/06-Excel桥接.md §1）。等价于
    // `ExcelTarget::Group { name: 节点所属 group, include: vec![当前节点 name] }`。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_excel_edit_clicked(move || {
            let target = {
                let st = s.borrow();
                match &st.selected {
                    Some(SelectedNode::Table { project_id, group, name }
                        | SelectedNode::Constant { project_id, group, name }
                        | SelectedNode::Enum { project_id, group, name }) => {
                        Some((project_id.clone(), group.clone(), name.clone()))
                    }
                    _ => None,
                }
            };
            let (pid, group, name) = match target {
                Some(t) => t,
                None => {
                    s.borrow_mut().engine.log("[Excel] 未选中可编辑节点".to_string());
                    if let Some(ui_h) = weak.upgrade() { crate::refresh::after_log(&ui_h, &s); }
                    return;
                }
            };
            let result = crate::excel_bridge::launch_excel_edit(
                &s, weak.clone(), &pid, &group, vec![name],
            );
            if let Err(e) = result {
                s.borrow_mut().engine.log(format!("[Excel] 调起失败: {}", e));
            }
            if let Some(ui_h) = weak.upgrade() {
                crate::excel_bridge::push(&ui_h, &s);
                push(&ui_h, &s);
                crate::refresh::after_log(&ui_h, &s);
            }
        });
    }
    // 单元格点击 → 行为按 ColumnKind + picker_trigger_data 配置分发：
    //   ReadOnly / Text          ：选中（双击才进 inline 编辑）
    //   ExportEnumCol            ：popup 由 slint 端 TouchArea 直接控制（按 picker_trigger_data 决定单击/双击），
    //                             Rust 这边只更新选区 + editing-export-index
    //   Ref / TypeEnumCol        ：picker_trigger_data = "single" 时单击弹；否则只选中（双击才弹）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_cell_clicked(move |r, c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { grid_actions::commit_editing(&ui_for_buf, &s); }
            // 关键：单击千万不能对 picker cell push grid，否则 data/header model 重建
            // 会销毁 TouchArea 实例，slint 双击事件需要同一个 TouchArea 接连两次 clicked，
            // 第二次落到新实例上只会再次触发 clicked、永远不会变成 double-clicked。
            let (kind, data_single) = {
                let st = s.borrow();
                (grid_actions::effective_column_kind_at(&st, r as usize, c as usize),
                 st.picker_trigger_data_single)
            };
            s.borrow_mut().grid_selection = GridSelection::Cell(r as usize, c as usize);
            // single 模式下：Ref / TypeEnumCol 单击直接弹（ExportEnumCol 由 slint 端 TouchArea 自己处理 popup）
            if data_single {
                if matches!(kind, Some(state::ColumnKind::TypeEnumCol)) {
                    dialogs::type_selector::open_for_cell(&s, r as usize, c as usize);
                    if let Some(ui_h) = weak.upgrade() {
                        push_selection_only(&ui_h, &s);
                        dialogs::type_selector::push(&ui_h, &s);
                    }
                    return;
                }
                if let Some(state::ColumnKind::Ref { ref target }) = kind {
                    dialogs::ref_picker::open_for_cell(&s, r as usize, c as usize, target);
                    if let Some(ui_h) = weak.upgrade() {
                        push_selection_only(&ui_h, &s);
                        dialogs::ref_picker::push(&ui_h, &s);
                    }
                    return;
                }
            }
            if let Some(ui_h) = weak.upgrade() {
                if was_editing { push(&ui_h, &s); } else { push_selection_only(&ui_h, &s); }
            }
        });
    }
    // shift+click：扩展矩形选区到 (r,c)。anchor=当前选中单元格（无则取自身）。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_cell_shift_clicked(move |r, c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { grid_actions::commit_editing(&ui_for_buf, &s); }
            {
                let mut st = s.borrow_mut();
                let r = r as usize;
                let c = c as usize;
                let anchor = match st.grid_selection {
                    GridSelection::Cell(ar, ac) => Some((ar, ac)),
                    GridSelection::CellRange { r1, c1, .. } => Some((r1, c1)),
                    GridSelection::Row(ar) => Some((ar, 0)),
                    GridSelection::Col(ac) => Some((0, ac)),
                    GridSelection::None => None,
                };
                let (r1, c1) = anchor.unwrap_or((r, c));
                st.grid_selection = if r1 == r && c1 == c {
                    GridSelection::Cell(r, c)
                } else {
                    GridSelection::CellRange { r1, c1, r2: r, c2: c }
                };
            }
            if let Some(ui_h) = weak.upgrade() {
                if was_editing { push(&ui_h, &s); } else { push_selection_only(&ui_h, &s); }
            }
        });
    }
    // 鼠标按下（左键，无 shift）：立即把选区收缩为单格 anchor。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_cell_pressed(move |r, c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { grid_actions::commit_editing(&ui_for_buf, &s); }
            let r = r as usize;
            let c = c as usize;
            let changed = {
                let mut st = s.borrow_mut();
                let new_sel = GridSelection::Cell(r, c);
                let changed = st.grid_selection != new_sel;
                if changed { st.grid_selection = new_sel; }
                changed
            };
            if let Some(ui_h) = weak.upgrade() {
                if was_editing { push(&ui_h, &s); }
                else if changed { push_selection_only(&ui_h, &s); }
            }
        });
    }
    // 鼠标拖选：anchor cell 抓住鼠标后 moved 只在它本身触发。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_cell_drag(move |anchor_r, anchor_c, mx, my| {
            // Theme.col-w=100px, Theme.row-h=22px（与 ui/theme.slint 一致）
            const COL_W: f32 = 100.0;
            const ROW_H: f32 = 22.0;
            let dc = (mx / COL_W).floor() as i32;
            let dr = (my / ROW_H).floor() as i32;
            let raw_r = anchor_r + dr;
            let raw_c = anchor_c + dc;
            let (rows, cols) = grid_actions::dims(&s);
            if rows == 0 || cols == 0 { return; }
            let r2 = raw_r.max(0).min(rows as i32 - 1) as usize;
            let c2 = raw_c.max(0).min(cols as i32 - 1) as usize;
            let r1 = anchor_r.max(0) as usize;
            let c1 = anchor_c.max(0) as usize;
            let new_sel = if r1 == r2 && c1 == c2 {
                GridSelection::Cell(r1, c1)
            } else {
                GridSelection::CellRange { r1, c1, r2, c2 }
            };
            let changed = {
                let st = s.borrow();
                st.grid_selection != new_sel
            };
            if !changed { return; }
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { grid_actions::commit_editing(&ui_for_buf, &s); }
            s.borrow_mut().grid_selection = new_sel;
            if let Some(ui_h) = weak.upgrade() {
                if was_editing { push(&ui_h, &s); } else { push_selection_only(&ui_h, &s); }
            }
        });
    }
    // Table 表头单击：commit 上次编辑；picker_trigger_header = "single" 时 picker 类弹窗
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_header_clicked(move |hi, ci| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing {
                grid_actions::commit_editing(&ui_for_buf, &s);
                if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
            }
            let (kind, header_single) = {
                let st = s.borrow();
                let kind = st.grid_header_kinds
                    .get(hi as usize)
                    .and_then(|row| row.get(ci as usize))
                    .cloned();
                (kind, st.picker_trigger_header_single)
            };
            if header_single && matches!(kind, Some(state::ColumnKind::TypeEnumCol)) {
                dialogs::type_selector::open_for_header(&s, ci as usize);
                if let Some(ui_h) = weak.upgrade() {
                    push(&ui_h, &s);
                    dialogs::type_selector::push(&ui_h, &s);
                }
            }
        });
    }
    // 表头双击 → desc/field 行进 inline LineEdit；picker 类弹 TypeSelector / popup
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_header_double_clicked(move |hi, ci| {
            let (kind, header_single) = {
                let st = s.borrow();
                let kind = st.grid_header_kinds.get(hi as usize)
                    .and_then(|row| row.get(ci as usize))
                    .cloned();
                (kind, st.picker_trigger_header_single)
            };
            let allow = kind.as_ref().map_or(false, |k| k.double_click_to_edit());
            if !allow { return; }
            // picker 类（TypeEnumCol）：仅在 picker_trigger_header = "double" 时双击弹
            if matches!(kind, Some(state::ColumnKind::TypeEnumCol)) {
                if !header_single {
                    dialogs::type_selector::open_for_header(&s, ci as usize);
                    if let Some(ui_h) = weak.upgrade() {
                        push(&ui_h, &s);
                        dialogs::type_selector::push(&ui_h, &s);
                    }
                }
                return;
            }
            if matches!(kind, Some(state::ColumnKind::ExportEnumCol)) {
                // popup 已在 slint 端打开（TouchArea 按 picker_trigger_header 决定单击/双击）。
                // Rust 这里只同步 editing-export-index 让 popup 的 current-index 是当前 cell 的值。
                if let Some(ui_h) = weak.upgrade() {
                    ui_h.set_grid_editing_export_index(grid_actions::compute_editing_export_index(&s, -1, -1, ci));
                }
                return;
            }
            // Text 类（desc / field）：进 inline LineEdit
            let raw = {
                let st = s.borrow();
                if let Some(SelectedNode::Table { group, name, .. }) = &st.selected {
                    st.engine.find_table(group, name)
                        .and_then(|t| t.schema.fields.get(ci as usize))
                        .map(|f| match hi {
                            0 => f.desc.clone(),
                            2 => f.tbl_type.clone(),
                            3 => f.name.clone(),
                            _ => String::new(),
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            };
            {
                let mut st = s.borrow_mut();
                st.editing = None;
                st.editing_buffer = raw;
                st.editing_in_formula = false;
                st.editing_header_row = hi;
                st.editing_header_col = ci;
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 单元格双击 → 进入 inline LineEdit 编辑 / 弹 picker
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_cell_double_clicked(move |r, c| {
            let (kind, data_single) = {
                let st = s.borrow();
                (grid_actions::effective_column_kind_at(&st, r as usize, c as usize),
                 st.picker_trigger_data_single)
            };
            let allow = kind.as_ref().map_or(false, |k| k.double_click_to_edit());
            if !allow { return; }
            // ExportEnumCol：popup 由 slint 端 TouchArea 直接控制
            if matches!(kind, Some(state::ColumnKind::ExportEnumCol)) {
                {
                    let mut st = s.borrow_mut();
                    st.grid_selection = GridSelection::Cell(r as usize, c as usize);
                }
                if let Some(ui_h) = weak.upgrade() {
                    ui_h.set_grid_editing_export_index(grid_actions::compute_editing_export_index(&s, -1, -1, -1));
                    push_selection_only(&ui_h, &s);
                }
                return;
            }
            // picker 类（Ref / TypeEnumCol）：仅在 picker_trigger_data = "double" 时双击弹
            let open_type_dlg = matches!(kind, Some(state::ColumnKind::TypeEnumCol));
            let open_ref_dlg = matches!(kind, Some(state::ColumnKind::Ref { .. }));
            if open_type_dlg && !data_single {
                dialogs::type_selector::open_for_cell(&s, r as usize, c as usize);
                if let Some(ui_h) = weak.upgrade() {
                    dialogs::type_selector::push(&ui_h, &s);
                }
                return;
            }
            if open_ref_dlg && !data_single {
                if let Some(state::ColumnKind::Ref { ref target }) = kind {
                    dialogs::ref_picker::open_for_cell(&s, r as usize, c as usize, target);
                }
                if let Some(ui_h) = weak.upgrade() {
                    dialogs::ref_picker::push(&ui_h, &s);
                }
                return;
            }
            if open_type_dlg || open_ref_dlg { return; }
            // Text：进 inline LineEdit
            let raw = convert::raw_cell_for(&s.borrow(), r as usize, c as usize);
            {
                let mut st = s.borrow_mut();
                st.editing = Some((r as usize, c as usize));
                st.editing_buffer = raw;
                st.editing_in_formula = false;
                st.editing_header_row = -1;
                st.editing_header_col = -1;
                st.grid_selection = GridSelection::Cell(r as usize, c as usize);
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // inline / 公式栏 编辑提交（Enter / 失焦）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_cell_edit_committed(move || {
            grid_actions::commit_editing(&ui_for_buf, &s);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_formula_commit(move || {
            grid_actions::commit_editing(&ui_for_buf, &s);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // inline 编辑取消（Esc — slint 暂未发出，此处保留 hook）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_cell_edit_canceled(move || {
            {
                let mut st = s.borrow_mut();
                st.editing = None;
                st.editing_buffer.clear();
                st.editing_in_formula = false;
                st.editing_header_row = -1;
                st.editing_header_col = -1;
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_formula_cancel(move || {
            {
                let mut st = s.borrow_mut();
                st.editing = None;
                st.editing_buffer.clear();
                st.editing_in_formula = false;
                st.editing_header_row = -1;
                st.editing_header_col = -1;
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 公式栏获得焦点 → 进入编辑（前提：有选中 cell + Text 列）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_formula_request_edit(move || {
            let target = {
                let st = s.borrow();
                if st.editing.is_some() { return; }
                match st.grid_selection {
                    GridSelection::Cell(r, c) => {
                        let kind = grid_actions::effective_column_kind_at(&st, r, c);
                        if matches!(kind, Some(state::ColumnKind::Text)) {
                            Some((r, c, convert::raw_cell_for(&st, r, c)))
                        } else { None }
                    }
                    _ => None,
                }
            };
            if let Some((r, c, raw)) = target {
                {
                    let mut st = s.borrow_mut();
                    st.editing = Some((r, c));
                    st.editing_buffer = raw;
                    st.editing_in_formula = true;
                }
                if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
            }
        });
    }
    // 行号点击 → 整行选中（先 commit 当前编辑）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_row_num_clicked(move |r| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { grid_actions::commit_editing(&ui_for_buf, &s); }
            s.borrow_mut().grid_selection = GridSelection::Row(r as usize);
            if let Some(ui_h) = weak.upgrade() {
                if was_editing { push(&ui_h, &s); } else { push_selection_only(&ui_h, &s); }
            }
        });
    }
    // 列字母点击 → 整列选中（先 commit 当前编辑）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_col_letter_clicked(move |c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { grid_actions::commit_editing(&ui_for_buf, &s); }
            s.borrow_mut().grid_selection = GridSelection::Col(c as usize);
            if let Some(ui_h) = weak.upgrade() {
                if was_editing { push(&ui_h, &s); } else { push_selection_only(&ui_h, &s); }
            }
        });
    }
    // 表格空白区点击 → 仅 commit 当前编辑（不改 selection）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_blank_clicked(move || {
            let need = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if !need { return; }
            grid_actions::commit_editing(&ui_for_buf, &s);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // Export Popup 选项被选中（Constant 数据格 / Table 表头 export 行）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_cell_export_selected(move |r, c, idx| {
            grid_actions::on_cell_export_selected(&s, r, c, idx);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_header_export_selected(move |c, idx| {
            grid_actions::on_header_export_selected(&s, c, idx);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // grid 列字母右键
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_col_context_menu(move |c, x, y| {
            s.borrow_mut().ctx_menu.open_at(CtxMenuKind::GridCol { col: c as usize }, x as f32, y as f32);
            if let Some(ui_h) = weak.upgrade() { dialogs::context_menu::push(&ui_h, &s); }
        });
    }
    // grid 行号右键
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_row_context_menu(move |r, x, y| {
            s.borrow_mut().ctx_menu.open_at(CtxMenuKind::GridRow { row: r as usize }, x as f32, y as f32);
            if let Some(ui_h) = weak.upgrade() { dialogs::context_menu::push(&ui_h, &s); }
        });
    }
    // grid 数据格右键
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_grid_cell_context_menu(move |r, c, x, y| {
            // 先 commit 任何在编辑的 cell。
            // Excel 语义：右键命中格若已在当前选区内则保留选区，否则收缩为单格选区。
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { grid_actions::commit_editing(&ui_for_buf, &s); }
            {
                let mut st = s.borrow_mut();
                let r = r as usize;
                let c = c as usize;
                let inside = match st.grid_selection.bounds() {
                    Some((rmin, rmax, cmin, cmax)) => {
                        let rmax = rmax.min(usize::MAX - 1);
                        let cmax = cmax.min(usize::MAX - 1);
                        r >= rmin && r <= rmax && c >= cmin && c <= cmax
                    }
                    None => false,
                };
                if !inside {
                    st.grid_selection = GridSelection::Cell(r, c);
                }
                st.ctx_menu.open_at(CtxMenuKind::GridCell { row: r, col: c }, x as f32, y as f32);
            }
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                dialogs::context_menu::push(&ui_h, &s);
            }
        });
    }
}
