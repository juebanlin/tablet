// 启动壳子：与 egui 端策略对齐
// - Windows 下隐藏控制台（避免双击/run 带 console；同时抑制 vm3dgl 等 OpenGL 启动日志）
//   仅 Windows 平台需要这个属性，Linux/macOS 不存在 subsystem 概念
// - CLI workdir / lock 文件 / 文件日志 / 加载 project
// - AppState 持 ProjectEngine + UI 临时态，用 Rc<RefCell<...>> 在 callback 中共享

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod state;
mod convert;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use clap::Parser;
use log::info;
use simplelog::*;

slint::include_modules!();

use state::{AppState, CtxMenuKind, GridSelection, PendingAction, SelectedNode, TreeFilter, TreeTarget, TsRefFilter, TsTab, TypeEditTarget};

#[derive(Parser)]
#[command(name = "tbl-tool", version = "0.1.0")]
struct Cli {
    #[arg(long)]
    workdir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let workdir = match cli.workdir {
        Some(p) => std::fs::canonicalize(&p)?,
        None => {
            let exe = std::env::current_exe()?;
            exe.parent().unwrap_or(exe.as_path()).to_path_buf()
        }
    };

    let lock_path = workdir.join(tbl_core::LOCK_FILE);
    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            if is_process_alive(pid) {
                eprintln!("另一个 TBL Tool 实例正在运行 (PID: {})", pid);
                std::process::exit(1);
            }
        }
    }
    std::fs::write(&lock_path, std::process::id().to_string())?;

    let app_state = Rc::new(RefCell::new(AppState::load(&workdir)?));

    let log_level = app_state.borrow().engine.project.config.ui.as_ref()
        .and_then(|u| u.log_level.as_deref())
        .unwrap_or("debug")
        .to_string();
    let file_level = match log_level.as_str() {
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Debug,
    };

    let log_path = workdir.join(tbl_core::LOG_FILE);
    let log_file = std::fs::File::create(&log_path)?;
    CombinedLogger::init(vec![WriteLogger::new(
        file_level,
        Config::default(),
        log_file,
    )])?;
    info!("loaded {} groups", app_state.borrow().engine.project.groups.len());

    let ui = AppWindow::new()?;
    push_tree(&ui, &app_state);
    push_grid(&ui, &app_state);
    push_logs(&ui, &app_state);
    push_context_menu(&ui, &app_state);
    push_input_dialog(&ui, &app_state);
    push_confirm_dialog(&ui, &app_state);
    wire_tree(&ui, &app_state);
    wire_grid(&ui, &app_state);
    wire_toolbar(&ui, &app_state);
    wire_type_selector(&ui, &app_state);
    wire_ref_picker(&ui, &app_state);
    wire_context_menu(&ui, &app_state);
    wire_dialogs(&ui, &app_state);
    wire_focus(&ui, &app_state);

    let result = ui.run().map_err(|e| anyhow::anyhow!("{}", e));
    let _ = std::fs::remove_file(&lock_path);
    result
}

/// 把 engine.logs（"HH:MM:SS msg" 格式的字符串）转成 slint LogEntry 列表 + 拼接的多行文本。
/// level 推断：消息含 "失败" / "错误" / "[验证]" → error；含 "警告" → warn；其它 info。
/// LogPanel 主要用 logs-text（read-only 多行 TextInput，可跨行选中复制）；
/// LogEntry 列表保留以兼容旧字段 / 将来按 level 着色。
fn push_logs(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let entries: Vec<LogEntry> = st.engine.logs.iter().map(|line| {
        let (time, msg) = match line.split_once(' ') {
            Some((t, m)) => (t.to_string(), m.to_string()),
            None => (String::new(), line.clone()),
        };
        let level = if msg.contains("失败") || msg.contains("错误") || msg.contains("[验证]") {
            2
        } else if msg.contains("警告") {
            1
        } else {
            0
        };
        LogEntry { time: time.into(), msg: msg.into(), level }
    }).collect();
    let flat = st.engine.logs.join("\n");
    ui.set_logs(slint::ModelRc::new(slint::VecModel::from(entries)));
    ui.set_logs_text(flat.into());
}

/// 构建树并推送到 slint。
fn push_tree(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let nodes = convert::build_tree_nodes(&mut state.borrow_mut());
    ui.set_tree_nodes(slint::ModelRc::new(slint::VecModel::from(nodes)));
}

/// 构建当前选中节点的 GridSection 快照并推送到 slint。
/// 同时把 column_kinds / header_kinds / data_count 写回 AppState，供后续 callback 判断单元格行为。
fn push_grid(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let snap = convert::build_grid(&state.borrow());
    let (editing_r, editing_c, editing_buffer, editing_in_formula, editing_header_row, editing_header_col) = {
        let st = state.borrow();
        match st.editing {
            Some((r, c)) => (r as i32, c as i32, st.editing_buffer.clone(), st.editing_in_formula, st.editing_header_row, st.editing_header_col),
            None => (-1, -1, st.editing_buffer.clone(), false, st.editing_header_row, st.editing_header_col),
        }
    };
    let editing_export_index = compute_editing_export_index(state, editing_r, editing_c, editing_header_col);
    let slint_column_kinds: Vec<CellKind> = snap.column_kinds.iter().map(column_kind_to_slint).collect();
    {
        let mut st = state.borrow_mut();
        st.grid_column_kinds = snap.column_kinds.clone();
        st.grid_header_kinds = snap.header_kinds.clone();
        st.grid_data_count = snap.data_count;
    }
    ui.set_grid_title(snap.title.into());
    ui.set_grid_subtitle(snap.subtitle.into());
    ui.set_grid_col_count(snap.col_count);
    ui.set_grid_header_rows(slint::ModelRc::new(slint::VecModel::from(
        snap.header_rows.into_iter()
            .map(|r| slint::ModelRc::new(slint::VecModel::from(r)))
            .collect::<Vec<_>>(),
    )));
    ui.set_grid_data_rows(slint::ModelRc::new(slint::VecModel::from(snap.data_rows)));
    ui.set_grid_column_kinds(slint::ModelRc::new(slint::VecModel::from(slint_column_kinds)));
    ui.set_grid_selected_col(snap.selected_col);
    ui.set_grid_selected_row(snap.selected_row);
    ui.set_grid_selected_cell_row(snap.selected_cell_row);
    ui.set_grid_selected_cell_col(snap.selected_cell_col);
    ui.set_grid_range_row_min(snap.range_row_min);
    ui.set_grid_range_row_max(snap.range_row_max);
    ui.set_grid_range_col_min(snap.range_col_min);
    ui.set_grid_range_col_max(snap.range_col_max);
    // 单元格内 LineEdit 仅在 inline 编辑数据格（非公式栏 + 非表头编辑）时显示
    let editing_data_cell = editing_r >= 0 && !editing_in_formula && editing_header_row < 0;
    ui.set_grid_editing_row(if editing_data_cell { editing_r } else { -1 });
    ui.set_grid_editing_col(if editing_data_cell { editing_c } else { -1 });
    ui.set_grid_editing_header_row(editing_header_row);
    ui.set_grid_editing_header_col(editing_header_col);
    ui.set_grid_editing_export_index(editing_export_index);
    ui.set_editing_buffer(editing_buffer.into());
    // 公式栏 LineEdit 仅在「在公式栏编辑」时显示
    ui.set_formula_editing(editing_r >= 0 && editing_in_formula);
    ui.set_coord(snap.coord.into());
    ui.set_formula_display(snap.formula_display.into());
    ui.set_formula_editable(snap.formula_editable);
    ui.set_selection_info(snap.selection_info.into());
    ui.set_hover_info(snap.hover_info.into());
}

/// 把 Rust 端 ColumnKind 映射成 slint 端 CellKind（仅用于 column-kinds 数据列指引）。
fn column_kind_to_slint(k: &state::ColumnKind) -> CellKind {
    match k {
        state::ColumnKind::ReadOnly => CellKind::ReadOnly,
        state::ColumnKind::Text => CellKind::Text,
        state::ColumnKind::Ref { .. } => CellKind::Ref,
        state::ColumnKind::TypeEnumCol => CellKind::TypeEnumCol,
        state::ColumnKind::ExportEnumCol => CellKind::ExportEnumCol,
    }
}

/// 计算 export popup 的 current-index：从当前 cell / header 读出 export code，映射到 0..3。
/// popup 列表顺序：["前后端","客户端","服务器","不导出"]。
fn compute_editing_export_index(state: &Rc<RefCell<AppState>>, editing_r: i32, editing_c: i32, editing_header_col: i32) -> i32 {
    let st = state.borrow();
    let code: Option<String> = if editing_header_col >= 0 {
        // Table 表头 export 行
        if let Some(SelectedNode::Table { group, name }) = &st.selected {
            st.engine.find_table(group, name)
                .and_then(|t| t.schema.fields.get(editing_header_col as usize))
                .map(|f| f.export.code().to_string())
        } else { None }
    } else if editing_r >= 0 && editing_c >= 0 {
        // Constant 数据行的 export 列
        match &st.selected {
            Some(SelectedNode::Constant { group, name }) => {
                st.engine.find_constant(group, name)
                    .and_then(|c| c.entries.get(editing_r as usize))
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

/// 仅更新选区相关的轻量属性（不重建 header/data 模型）。
/// cell-clicked / row-num-clicked / col-letter-clicked 用，避免 model 重建
/// 而打断 slint 双击 sequence。
fn push_selection_only(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let snap = convert::build_grid(&state.borrow());
    ui.set_grid_selected_col(snap.selected_col);
    ui.set_grid_selected_row(snap.selected_row);
    ui.set_grid_selected_cell_row(snap.selected_cell_row);
    ui.set_grid_selected_cell_col(snap.selected_cell_col);
    ui.set_grid_range_row_min(snap.range_row_min);
    ui.set_grid_range_row_max(snap.range_row_max);
    ui.set_grid_range_col_min(snap.range_col_min);
    ui.set_grid_range_col_max(snap.range_col_max);
    ui.set_coord(snap.coord.into());
    ui.set_formula_display(snap.formula_display.into());
    ui.set_formula_editable(snap.formula_editable);
    ui.set_selection_info(snap.selection_info.into());
    ui.set_hover_info(snap.hover_info.into());
}

/// 用 slint 的 editing-buffer property 当前值，写回当前 editing cell / header cell。
/// editing buffer 是 slint LineEdit 的 text 双向绑定，用户输入实时同步在 ui 端。
/// editing_header_row >= 0 时走 commit_header_edit；否则走 set_cell。
fn commit_editing(ui_weak: &slint::Weak<AppWindow>, state: &Rc<RefCell<AppState>>) {
    let buf = match ui_weak.upgrade() {
        Some(ui) => ui.get_editing_buffer().to_string(),
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

/// 把 4 个 tree callback 接到 AppState。
fn wire_tree(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // 过滤切换
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_tree_filter_changed(move |i| {
            s.borrow_mut().tree_filter = TreeFilter::from_index(i);
            if let Some(ui) = weak.upgrade() { push_tree(&ui, &s); }
        });
    }
    // 完整组勾选
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_tree_full_group_toggled(move |c| {
            s.borrow_mut().tree_full_group = c;
            if let Some(ui) = weak.upgrade() { push_tree(&ui, &s); }
        });
    }
    // 搜索
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_tree_search_edited(move |t| {
            s.borrow_mut().tree_search = t.to_string();
            if let Some(ui) = weak.upgrade() { push_tree(&ui, &s); }
        });
    }
    // 展开/折叠
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_tree_node_toggle_expand(move |id| {
            let mut st = s.borrow_mut();
            if let Some(TreeTarget::Group(name)) = st.tree_targets.get(id as usize).cloned() {
                if !st.tree_expanded.remove(&name) {
                    st.tree_expanded.insert(name);
                }
            }
            drop(st);
            if let Some(ui) = weak.upgrade() { push_tree(&ui, &s); }
        });
    }
    // 节点点击 → 切换 selected；如有 in-progress 编辑，先 commit
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_tree_node_clicked(move |id| {
            // 切节点前先 commit 当前编辑（取 slint editing-buffer）
            commit_editing(&ui_for_buf, &s);
            let mut st = s.borrow_mut();
            let target = st.tree_targets.get(id as usize).cloned();
            let mut grid_dirty = false;
            match target {
                Some(TreeTarget::Group(name)) => {
                    if !st.tree_expanded.remove(&name) {
                        st.tree_expanded.insert(name);
                    }
                }
                Some(TreeTarget::Table { group, name }) => {
                    st.selected = Some(SelectedNode::Table { group, name });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                Some(TreeTarget::Constant { group, name }) => {
                    st.selected = Some(SelectedNode::Constant { group, name });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                Some(TreeTarget::Enum { group, name }) => {
                    st.selected = Some(SelectedNode::Enum { group, name });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                None => {}
            }
            drop(st);
            if let Some(ui) = weak.upgrade() {
                push_tree(&ui, &s);
                if grid_dirty { push_grid(&ui, &s); }
            }
        });
    }
    // 树节点右键 → 打开 ContextMenu
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_tree_node_context_menu(move |id, x, y| {
            let kind = {
                let st = s.borrow();
                st.tree_targets.get(id as usize).cloned()
            };
            let menu_kind = match kind {
                Some(TreeTarget::Group(name)) => Some(CtxMenuKind::TreeGroup { name }),
                Some(TreeTarget::Table { group, name }) =>
                    Some(CtxMenuKind::TreeNode { group, name, kind: tbl_core::ops::NodeKind::Table }),
                Some(TreeTarget::Constant { group, name }) =>
                    Some(CtxMenuKind::TreeNode { group, name, kind: tbl_core::ops::NodeKind::Constant }),
                Some(TreeTarget::Enum { group, name }) =>
                    Some(CtxMenuKind::TreeNode { group, name, kind: tbl_core::ops::NodeKind::Enum }),
                None => None,
            };
            if let Some(k) = menu_kind {
                s.borrow_mut().ctx_menu.open_at(k, x as f32, y as f32);
                if let Some(ui) = weak.upgrade() { push_context_menu(&ui, &s); }
            }
        });
    }
    // 树空白右键 → ContextMenu(TreeBlank)
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_tree_blank_context_menu(move |x, y| {
            s.borrow_mut().ctx_menu.open_at(CtxMenuKind::TreeBlank, x as f32, y as f32);
            if let Some(ui) = weak.upgrade() { push_context_menu(&ui, &s); }
        });
    }
}

/// 把 GridSection 相关 callback 接到 AppState。
fn wire_grid(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // 「枚举显示名字」开关 → 重建 grid 让 @EnumName 列展示切换
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_show_enum_name_toggled(move |c| {
            s.borrow_mut().view_show_enum_name = c;
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    // 单元格点击 → 行为按 ColumnKind 分发：
    //   ReadOnly      ：选中（不编辑）
    //   Text          ：选中（双击才进 inline 编辑）
    //   ExportEnumCol ：选中 + 同步 editing-export-index（slint 端 popup.show 弹自制下拉）
    //   TypeEnumCol   ：弹 TypeSelector 弹窗
    //   Ref           ：弹 RefPicker 弹窗
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_cell_clicked(move |r, c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            // 切到别的 cell：先 commit 当前编辑（用 slint 端 editing-buffer 真值）
            if was_editing { commit_editing(&ui_for_buf, &s); }
            let kind = s.borrow().grid_column_kinds.get(c as usize).cloned();
            // ExportEnumCol：先把 selection + editing_export_index 推到 slint，popup 由 slint 端 .show()
            // 弹起。需要 push_grid 才能让 editing-export-index 是最新值。
            // TypeEnumCol：弹 TypeSelector
            // Ref：弹 RefPicker
            // ReadOnly / Text 默认走选中
            s.borrow_mut().grid_selection = GridSelection::Cell(r as usize, c as usize);
            let open_type_dlg = matches!(kind, Some(state::ColumnKind::TypeEnumCol));
            let open_ref_dlg = matches!(kind, Some(state::ColumnKind::Ref { .. }));
            if open_type_dlg {
                open_type_selector_for_cell(&s, r as usize, c as usize);
            }
            if open_ref_dlg {
                if let Some(state::ColumnKind::Ref { ref target }) = kind {
                    open_ref_picker_for_cell(&s, r as usize, c as usize, target);
                }
            }
            if let Some(ui) = weak.upgrade() {
                let need_full = was_editing || matches!(kind, Some(state::ColumnKind::ExportEnumCol));
                if need_full { push_grid(&ui, &s); } else { push_selection_only(&ui, &s); }
                if open_type_dlg { push_type_selector(&ui, &s); }
                if open_ref_dlg { push_ref_picker(&ui, &s); }
            }
        });
    }
    // shift+click：扩展矩形选区到 (r,c)。anchor=当前选中单元格（无则取自身）。
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_cell_shift_clicked(move |r, c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { commit_editing(&ui_for_buf, &s); }
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
            if let Some(ui) = weak.upgrade() {
                if was_editing { push_grid(&ui, &s); } else { push_selection_only(&ui, &s); }
            }
        });
    }
    // 鼠标按下（左键，无 shift）：立即把选区收缩为单格 anchor。
    // 这是拖选 / 普通点击的统一起点：commit 编辑 + 重置选区，使后续 cell-drag 从干净状态开始。
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_cell_pressed(move |r, c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { commit_editing(&ui_for_buf, &s); }
            let r = r as usize;
            let c = c as usize;
            let changed = {
                let mut st = s.borrow_mut();
                let new_sel = GridSelection::Cell(r, c);
                let changed = st.grid_selection != new_sel;
                if changed { st.grid_selection = new_sel; }
                changed
            };
            if let Some(ui) = weak.upgrade() {
                if was_editing { push_grid(&ui, &s); }
                else if changed { push_selection_only(&ui, &s); }
            }
        });
    }
    // 鼠标拖选：anchor cell 抓住鼠标后 moved 只在它本身触发，
    // mouse-x/y 是相对 anchor cell 原点的位移。按 col-w / row-h 推算目标 cell。
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_cell_drag(move |anchor_r, anchor_c, mx, my| {
            // Theme.col-w=100px, Theme.row-h=22px（与 ui/theme.slint 一致）
            const COL_W: f32 = 100.0;
            const ROW_H: f32 = 22.0;
            let dc = (mx / COL_W).floor() as i32;
            let dr = (my / ROW_H).floor() as i32;
            let raw_r = anchor_r + dr;
            let raw_c = anchor_c + dc;
            // 计算实际行列上限做裁剪
            let (rows, cols) = grid_dims(&s);
            if rows == 0 || cols == 0 { return; }
            let r2 = raw_r.max(0).min(rows as i32 - 1) as usize;
            let c2 = raw_c.max(0).min(cols as i32 - 1) as usize;
            let r1 = anchor_r.max(0) as usize;
            let c1 = anchor_c.max(0) as usize;
            // 第一次拖动若仍在 anchor 内：保持单格选区，不进 CellRange
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
            if was_editing { commit_editing(&ui_for_buf, &s); }
            s.borrow_mut().grid_selection = new_sel;
            if let Some(ui) = weak.upgrade() {
                if was_editing { push_grid(&ui, &s); } else { push_selection_only(&ui, &s); }
            }
        });
    }
    // Table 表头点击 → 按 grid_header_kinds[hi][ci] 分发：
    //   ExportEnumCol ：选中 + 同步 editing-export-index（slint popup.show 自动弹）
    //   TypeEnumCol   ：弹 TypeSelector 弹窗
    //   Text          ：仅作用于双击（commit 当前编辑保持不变）
    //   ReadOnly      ：忽略
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_header_clicked(move |hi, ci| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { commit_editing(&ui_for_buf, &s); }
            let kind = s.borrow().grid_header_kinds
                .get(hi as usize)
                .and_then(|row| row.get(ci as usize))
                .cloned();
            match kind {
                Some(state::ColumnKind::ExportEnumCol) => {
                    // 把 editing_header_col 设为当前列，方便 push_grid 计算 editing-export-index；
                    // 不进 inline 编辑（editing_header_row 仍为 -1）
                    {
                        let mut st = s.borrow_mut();
                        st.editing_header_row = -1;
                        st.editing_header_col = ci;
                    }
                    if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
                }
                Some(state::ColumnKind::TypeEnumCol) => {
                    open_type_selector_for_header(&s, ci as usize);
                    if let Some(ui) = weak.upgrade() {
                        if was_editing { push_grid(&ui, &s); }
                        push_type_selector(&ui, &s);
                    }
                }
                _ => {
                    if was_editing {
                        if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
                    }
                }
            }
        });
    }
    // 表头双击 → desc/field 行（hi=0/3）非 ReadOnly 列进 inline LineEdit 编辑
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_header_double_clicked(move |hi, ci| {
            let allow = {
                let st = s.borrow();
                st.grid_header_kinds.get(hi as usize)
                    .and_then(|row| row.get(ci as usize))
                    .map_or(false, |k| k.double_click_to_edit())
            };
            if !allow { return; }
            // 读当前 header cell 的存储值作为初始 buffer
            let raw = {
                let st = s.borrow();
                if let Some(SelectedNode::Table { group, name }) = &st.selected {
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
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    // 单元格双击 → 进入 inline LineEdit 编辑（仅 Text 列）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_cell_double_clicked(move |r, c| {
            let allow = {
                let st = s.borrow();
                st.grid_column_kinds.get(c as usize)
                    .map_or(false, |k| k.double_click_to_edit())
            };
            if !allow { return; }
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
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    // inline / 公式栏 编辑提交（Enter / 失焦）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_cell_edit_committed(move || {
            commit_editing(&ui_for_buf, &s);
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_formula_commit(move || {
            commit_editing(&ui_for_buf, &s);
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    // inline 编辑取消（Esc — slint 暂未发出，此处保留 hook）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_cell_edit_canceled(move || {
            {
                let mut st = s.borrow_mut();
                st.editing = None;
                st.editing_buffer.clear();
                st.editing_in_formula = false;
                st.editing_header_row = -1;
                st.editing_header_col = -1;
            }
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_formula_cancel(move || {
            {
                let mut st = s.borrow_mut();
                st.editing = None;
                st.editing_buffer.clear();
                st.editing_in_formula = false;
                st.editing_header_row = -1;
                st.editing_header_col = -1;
            }
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    // 公式栏获得焦点 → 进入编辑（前提：有选中 cell + Text 列）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_formula_request_edit(move || {
            let target = {
                let st = s.borrow();
                if st.editing.is_some() { return; }
                match st.grid_selection {
                    GridSelection::Cell(r, c) => {
                        let kind = st.grid_column_kinds.get(c).cloned();
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
                if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
            }
        });
    }
    // 行号点击 → 整行选中（先 commit 当前编辑）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_row_num_clicked(move |r| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { commit_editing(&ui_for_buf, &s); }
            s.borrow_mut().grid_selection = GridSelection::Row(r as usize);
            if let Some(ui) = weak.upgrade() {
                if was_editing { push_grid(&ui, &s); } else { push_selection_only(&ui, &s); }
            }
        });
    }
    // 列字母点击 → 整列选中（先 commit 当前编辑）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_col_letter_clicked(move |c| {
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { commit_editing(&ui_for_buf, &s); }
            s.borrow_mut().grid_selection = GridSelection::Col(c as usize);
            if let Some(ui) = weak.upgrade() {
                if was_editing { push_grid(&ui, &s); } else { push_selection_only(&ui, &s); }
            }
        });
    }
    // 表格空白区点击 → 仅 commit 当前编辑（不改 selection）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_blank_clicked(move || {
            let need = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if !need { return; }
            commit_editing(&ui_for_buf, &s);
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    // Export Popup 选项被选中（Constant 数据格 / Table 表头 export 行）
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_cell_export_selected(move |r, c, idx| {
            on_cell_export_selected(&s, r, c, idx);
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_header_export_selected(move |c, idx| {
            on_header_export_selected(&s, c, idx);
            if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
        });
    }
    // grid 列字母右键
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_col_context_menu(move |c, x, y| {
            s.borrow_mut().ctx_menu.open_at(CtxMenuKind::GridCol { col: c as usize }, x as f32, y as f32);
            if let Some(ui) = weak.upgrade() { push_context_menu(&ui, &s); }
        });
    }
    // grid 行号右键
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_row_context_menu(move |r, x, y| {
            s.borrow_mut().ctx_menu.open_at(CtxMenuKind::GridRow { row: r as usize }, x as f32, y as f32);
            if let Some(ui) = weak.upgrade() { push_context_menu(&ui, &s); }
        });
    }
    // grid 数据格右键
    {
        let s = state.clone();
        let weak = ui.as_weak();
        let ui_for_buf = ui.as_weak();
        ui.on_grid_cell_context_menu(move |r, c, x, y| {
            // 先 commit 任何在编辑的 cell。
            // Excel 语义：右键命中格若已在当前选区内则保留选区，否则收缩为单格选区。
            let was_editing = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if was_editing { commit_editing(&ui_for_buf, &s); }
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
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_context_menu(&ui, &s);
            }
        });
    }
}

/// 顶部工具栏按钮 → ProjectEngine 操作。
/// 按 egui 端语义：generate-test/clear/save/reload 立即生效，其余 step 再接入。
fn wire_toolbar(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let weak = ui.as_weak();
    ui.on_toolbar_btn_clicked(move |id| {
        let id = id.to_string();
        let mut full_refresh = false;
        let mut export_dlg = false;
        let mut schema_export_dlg = false;
        let mut schema_import_dlg = false;
        match id.as_str() {
            "generate-test" => {
                s.borrow_mut().engine.generate_test_config();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            "clear" => {
                s.borrow_mut().engine.clear_all_config();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            "save" => {
                s.borrow_mut().engine.save_all();
                // save_all 内部会跑 revalidate_all 重算 validation_errors，
                // 必须刷新 tree（! 标记）和 grid（红框）才能让用户看到错误
                full_refresh = true;
            }
            "reload" => {
                s.borrow_mut().engine.reload();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            "export" => {
                s.borrow_mut().data_export.open = true;
                export_dlg = true;
            }
            "export-schema" => {
                {
                    let mut st = s.borrow_mut();
                    st.schema_export.open = true;
                    rebuild_schema_export_items(&mut st);
                }
                schema_export_dlg = true;
            }
            "import-schema" => {
                {
                    let mut st = s.borrow_mut();
                    st.schema_import = state::SchemaImportState::default();
                    st.schema_import.open = true;
                }
                schema_import_dlg = true;
            }
            _ => {
                // excel 等：后续 step
            }
        }
        if let Some(ui) = weak.upgrade() {
            if full_refresh {
                push_tree(&ui, &s);
                push_grid(&ui, &s);
                push_type_selector(&ui, &s);
                push_ref_picker(&ui, &s);
                push_context_menu(&ui, &s);
                push_input_dialog(&ui, &s);
                push_confirm_dialog(&ui, &s);
            }
            if export_dlg { push_data_export(&ui, &s); }
            if schema_export_dlg { push_schema_export(&ui, &s); }
            if schema_import_dlg { push_schema_import(&ui, &s); }
            // 任何 toolbar 操作都可能产生日志（save/reload/generate/clear 全会 log）
            push_logs(&ui, &s);
        }
    });
}

/// reload / generate / clear 后清掉 UI 临时态：选中节点、grid 选区、编辑 buffer。
fn reset_view_after_reload(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    st.selected = None;
    st.grid_selection = GridSelection::None;
    st.editing = None;
    st.editing_buffer.clear();
    st.editing_in_formula = false;
    st.editing_header_row = -1;
    st.editing_header_col = -1;
    st.type_selector.close();
    st.ref_picker.close();
    st.ctx_menu.close();
    st.pending.close();
    // 重新展开所有 group（与 AppState::load 一致的初始态）
    st.tree_expanded = st.engine.project.groups.iter().map(|g| g.name.clone()).collect();
    // 同步 AppState::load：reload / generate-test / clear 后跑一遍全表验证。
    st.engine.revalidate_all();
    if !st.engine.validation_errors.is_empty() {
        let n = st.engine.validation_errors.len();
        st.engine.log(format!("[验证] 共 {} 个错误", n));
    }
}

/// 监听 slint 端 drop-focus → commit-pending-edit：
/// 任何空白 / 树节点 / toolbar 点击都会触发，把 editing-buffer 写回 cell 并刷新表格。
/// 没有 editing 时是 no-op，所以在每条点击路径上都重复 fire 也无副作用。
fn wire_focus(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let weak = ui.as_weak();
    let ui_for_buf = ui.as_weak();
    ui.on_commit_pending_edit(move || {
        let need = {
            let st = s.borrow();
            st.editing.is_some() || st.editing_header_row >= 0
        };
        if !need { return; }
        commit_editing(&ui_for_buf, &s);
        if let Some(ui) = weak.upgrade() { push_grid(&ui, &s); }
    });

    // 键盘快捷键：复制/粘贴/删除。anchor 来自当前 GridSelection；无选区时 no-op。
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_shortcut_copy(move || {
            if grid_selection_anchor(&s).is_none() { return; }
            perform_grid_action(&s, "grid.cell-copy", "Ctrl+C");
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_shortcut_cut(move || {
            if grid_selection_anchor(&s).is_none() { return; }
            perform_grid_action(&s, "grid.cell-cut", "Ctrl+X");
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_shortcut_paste(move || {
            if grid_selection_anchor(&s).is_none() { return; }
            perform_grid_action(&s, "grid.cell-paste", "Ctrl+V");
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_shortcut_delete(move || {
            if grid_selection_anchor(&s).is_none() { return; }
            perform_grid_action(&s, "grid.cell-clear", "Delete");
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    // 调试键盘事件（仅落到 log 文件，不进 UI 日志框）：
    // 用于排查 Ctrl 修饰键的字符映射问题，UI 不需要。
    {
        let s = state.clone();
        ui.on_debug_key(move |text, ctrl| {
            // 单按可见字符（如直接按 c）不记录，避免噪音
            let is_plain_visible = !ctrl && text.chars().count() == 1
                && text.chars().next().map_or(false, |c| !c.is_control());
            if is_plain_visible { return; }
            let bytes: Vec<String> = text.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
            log::debug!("[键盘] text={:?} bytes=[{}] ctrl={}", text.as_str(), bytes.join(" "), ctrl);
            // 占位：让 borrow 链路保持非空，避免编译器警告
            let _ = s.borrow();
        });
    }
}

/// 取当前 GridSelection 的 anchor (左上角)；None=无选区。
fn grid_selection_anchor(state: &Rc<RefCell<AppState>>) -> Option<(usize, usize)> {
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
/// rows 包含 EXTRA_ROWS 占位行：用户可以拖选 / 粘贴 / 清空到表尾的空白行，
/// 跟 egui 端 display_rows 行为一致；engine.paste_*_data 内部会按需 push 新行。
/// 没有选中节点时返回 (0, 0)。
fn grid_dims(state: &Rc<RefCell<AppState>>) -> (usize, usize) {
    let st = state.borrow();
    let rows = st.grid_data_count + convert::EXTRA_ROWS;
    let cols = match &st.selected {
        Some(SelectedNode::Table { group, name }) => st.engine.find_table(group, name)
            .map(|t| t.schema.fields.len()).unwrap_or(0),
        Some(SelectedNode::Constant { .. }) => 5,
        Some(SelectedNode::Enum { .. }) => 3,
        None => 0,
    };
    (rows, cols)
}

/// Constant ExportEnumCol popup 选项被选中：写回 entries[r].export。
/// (r,c) 来自 slint cell 端的 ri/ci 闭包，不依赖 editing 状态（popup 由 cell-clicked 即时弹出）。
fn on_cell_export_selected(state: &Rc<RefCell<AppState>>, r: i32, _c: i32, idx: i32) {
    use tbl_core::model::Export;
    let opt = match idx {
        0 => Export::ClientServer,
        1 => Export::ClientOnly,
        2 => Export::ServerOnly,
        3 => Export::None,
        _ => return,
    };
    let mut st = state.borrow_mut();
    let (group, name) = match &st.selected {
        Some(SelectedNode::Constant { group, name }) => (group.clone(), name.clone()),
        _ => return,
    };
    st.engine.commit_constant_cell(&group, &name, r as usize, 3, opt.code().to_string());
    if st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
}

/// Table 表头 export 行的 popup 选项被选中：写回 schema.fields[col].export。
fn on_header_export_selected(state: &Rc<RefCell<AppState>>, col: i32, idx: i32) {
    use tbl_core::model::Export;
    let opt = match idx {
        0 => Export::ClientServer,
        1 => Export::ClientOnly,
        2 => Export::ServerOnly,
        3 => Export::None,
        _ => return,
    };
    let mut st = state.borrow_mut();
    let (group, name) = match &st.selected {
        Some(SelectedNode::Table { group, name }) => (group.clone(), name.clone()),
        _ => return,
    };
    st.engine.commit_header_edit(&group, &name, 1, col as usize, opt.code().to_string());
    if st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
}

// ──────── TypeSelector ────────

/// Constant data cell type 列单击 → 打开 TypeSelector（编辑该数据格的 type）
fn open_type_selector_for_cell(state: &Rc<RefCell<AppState>>, r: usize, c: usize) {
    let mut st = state.borrow_mut();
    let (group, name, is_table, current) = match &st.selected {
        Some(SelectedNode::Constant { group, name }) => {
            let cur = st.engine.find_constant(group, name)
                .and_then(|cst| cst.entries.get(r))
                .map(|e| e.tbl_type.clone())
                .unwrap_or_default();
            (group.clone(), name.clone(), false, cur)
        }
        Some(SelectedNode::Table { group, name }) => {
            // Table 数据格不会是 TypeEnumCol，但保留兜底
            (group.clone(), name.clone(), true, String::new())
        }
        _ => return,
    };
    st.type_selector.open_with(&current, TypeEditTarget::CellType { row: r, col: c }, &group, &name, is_table);
}

/// Table 表头 type 行单击 → 打开 TypeSelector（编辑该列的 tbl_type）
fn open_type_selector_for_header(state: &Rc<RefCell<AppState>>, col: usize) {
    let mut st = state.borrow_mut();
    let (group, name, current) = match &st.selected {
        Some(SelectedNode::Table { group, name }) => {
            let cur = st.engine.find_table(group, name)
                .and_then(|t| t.schema.fields.get(col))
                .map(|f| f.tbl_type.clone())
                .unwrap_or_default();
            (group.clone(), name.clone(), cur)
        }
        _ => return,
    };
    st.type_selector.open_with(&current, TypeEditTarget::HeaderType { col }, &group, &name, true);
}

/// 收集项目内可被引用的项（table + enum，排除 deleted），按名称排序。
fn collect_ref_targets(state: &AppState) -> Vec<(String, bool /* is_table */)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for g in &state.engine.project.groups {
        for t in &g.tables { if !t.deleted { out.push((t.name.clone(), true)); } }
        for e in &g.enums  { if !e.deleted { out.push((e.name.clone(), false)); } }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 把 TypeSelectorState 派生成 slint 端属性并 push。每次状态变化都重新调用。
fn push_type_selector(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    use tbl_core::types::{BaseType, Paradigm};
    let st = state.borrow();
    let ts = &st.type_selector;
    ui.set_dlg_type_open(ts.open);
    if !ts.open { return; }

    ui.set_ts_tab(if ts.tab == TsTab::Reference { 1 } else { 0 });
    ui.set_ts_ref_disabled(ts.ref_disabled);

    // paradigms 列表（仅 Data 范式，14 项）
    let paradigms: Vec<slint::SharedString> = Paradigm::all_data().iter()
        .map(|p| slint::SharedString::from(p.label()))
        .collect();
    ui.set_ts_paradigms(slint::ModelRc::new(slint::VecModel::from(paradigms)));
    let paradigm_index = Paradigm::all_data().iter().position(|p| *p == ts.paradigm).unwrap_or(0) as i32;
    ui.set_ts_paradigm_index(paradigm_index);

    // param-slots：每个槽给 label / 该槽可选基础类型 / 当前选中下标
    let slots = ts.paradigm.param_slots();
    let bt_all: Vec<slint::SharedString> = BaseType::all().iter().map(|b| slint::SharedString::from(b.name())).collect();
    let bt_keys: Vec<slint::SharedString> = BaseType::map_key_types().iter().map(|b| slint::SharedString::from(b.name())).collect();
    let mut ts_slots: Vec<TsParamSlot> = Vec::with_capacity(slots.len());
    for (i, slot) in slots.iter().enumerate() {
        let options = if slot.is_map_key { bt_keys.clone() } else { bt_all.clone() };
        let cur = ts.params.get(i).copied().unwrap_or(BaseType::Int);
        let avail = if slot.is_map_key { BaseType::map_key_types() } else { BaseType::all() };
        let cur_idx = avail.iter().position(|b| *b == cur).unwrap_or(0) as i32;
        ts_slots.push(TsParamSlot {
            label: slint::SharedString::from(slot.label),
            type_options: slint::ModelRc::new(slint::VecModel::from(options)),
            selected_index: cur_idx,
        });
    }
    ui.set_ts_param_slots(slint::ModelRc::new(slint::VecModel::from(ts_slots)));

    // 预览：依据当前 tab 选择 data_type / ref_type
    let sep = st.engine.project.config.separators.clone();
    let (preview, example, java, go, lua) = match ts.tab {
        TsTab::Data => {
            let t = ts.data_type();
            (t.to_type_string(), t.example_with_sep(&sep), t.java_decl(), t.go_decl(), t.lua_decl())
        }
        TsTab::Reference => {
            if let Some(t) = ts.ref_type() {
                (t.to_type_string(),
                 String::from("（id）"),
                 String::from("int"),
                 String::from("int32"),
                 String::from("number"))
            } else {
                (String::from("（未选择）"), String::new(), String::new(), String::new(), String::new())
            }
        }
    };
    ui.set_ts_result_preview(preview.into());
    ui.set_ts_example_value(example.into());
    ui.set_ts_java_decl(java.into());
    ui.set_ts_go_decl(go.into());
    ui.set_ts_lua_decl(lua.into());

    // ref-items：按 filter + search 过滤，并标 selected
    let search = ts.ref_search.to_lowercase();
    let targets = collect_ref_targets(&st);
    let items: Vec<RefItem> = targets.iter().filter(|(name, is_table)| {
        let kind_ok = match ts.ref_filter {
            TsRefFilter::All => true,
            TsRefFilter::Table => *is_table,
            TsRefFilter::Enum => !*is_table,
        };
        let search_ok = search.is_empty() || name.to_lowercase().contains(&search);
        kind_ok && search_ok
    }).map(|(name, is_table)| {
        let icon = if *is_table { "📊" } else { "🔢" };
        let kind_label = if *is_table { "table" } else { "enum" };
        RefItem {
            icon: icon.into(),
            name: name.clone().into(),
            kind_label: kind_label.into(),
            selected: ts.ref_name == *name,
        }
    }).collect();
    ui.set_ts_ref_items(slint::ModelRc::new(slint::VecModel::from(items)));
    ui.set_ts_ref_search(ts.ref_search.clone().into());
    ui.set_ts_ref_filter(match ts.ref_filter {
        TsRefFilter::All => 0, TsRefFilter::Table => 1, TsRefFilter::Enum => 2,
    });
}

/// 接通 TypeSelector 的所有回调：set-paradigm / set-param / set-ref-* / select-ref / confirm / cancel。
fn wire_type_selector(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    use tbl_core::types::{BaseType, Paradigm};

    // tab 切换：slint 端 ts-tab 是 in-out，由 slint 自己写。每次切换需要重新 push 让预览刷新。
    // 这里通过 ts-set-paradigm 等所有 set-* 同时刷新预览即可，不单独 hook tab。
    // 但 tab 切换后预览会改，所以也接一个 setter：在 ChangeEvents 里重 push（slint 没有
    // property-changed 回调暴露给 Rust），workaround：监听 ts-set-paradigm/set-param/set-ref-* 都重 push。
    // tab 自身的切换在 slint 端就改了 ts-tab，渲染随之刷新；预览文本由 push_type_selector 算。
    // 用户切 tab 后没改 paradigm 的话预览看似旧——为简单起见，confirm 时再用 Rust 的 ts.tab 拿真值；
    // 视觉上 tab 切到 Reference 不显示参数行 / 切到 Data 不显示 ref 列表，问题不大。

    // ts-set-tab：用户在 slint 端切 tab，Rust 同步并刷新预览/可见列表
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_set_tab(move |i| {
            let mut st = s.borrow_mut();
            // constant 禁用引用 tab：忽略切到 1 的请求
            if i == 1 && st.type_selector.ref_disabled { return; }
            st.type_selector.tab = if i == 1 { TsTab::Reference } else { TsTab::Data };
            drop(st);
            if let Some(ui) = weak.upgrade() { push_type_selector(&ui, &s); }
        });
    }
    // ts-set-paradigm
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_set_paradigm(move |i| {
            let mut st = s.borrow_mut();
            if let Some(p) = Paradigm::all_data().get(i as usize).cloned() {
                st.type_selector.paradigm = p;
                st.type_selector.sync_params();
            }
            drop(st);
            if let Some(ui) = weak.upgrade() { push_type_selector(&ui, &s); }
        });
    }
    // ts-set-param(slot, type-idx)
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_set_param(move |slot, idx| {
            let mut st = s.borrow_mut();
            let slots = st.type_selector.paradigm.param_slots();
            if let Some(slot_def) = slots.get(slot as usize) {
                let avail = if slot_def.is_map_key { BaseType::map_key_types() } else { BaseType::all() };
                if let Some(bt) = avail.get(idx as usize).copied() {
                    if (slot as usize) < st.type_selector.params.len() {
                        st.type_selector.params[slot as usize] = bt;
                    }
                }
            }
            drop(st);
            if let Some(ui) = weak.upgrade() { push_type_selector(&ui, &s); }
        });
    }
    // ts-set-ref-search
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_set_ref_search(move |t| {
            s.borrow_mut().type_selector.ref_search = t.to_string();
            if let Some(ui) = weak.upgrade() { push_type_selector(&ui, &s); }
        });
    }
    // ts-set-ref-filter
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_set_ref_filter(move |i| {
            let f = match i { 1 => TsRefFilter::Table, 2 => TsRefFilter::Enum, _ => TsRefFilter::All };
            s.borrow_mut().type_selector.ref_filter = f;
            if let Some(ui) = weak.upgrade() { push_type_selector(&ui, &s); }
        });
    }
    // select-ref：用 filter+search 过滤后的索引去拿真名
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_select_ref(move |i| {
            let chosen: Option<String> = {
                let st = s.borrow();
                let ts = &st.type_selector;
                let search = ts.ref_search.to_lowercase();
                let targets = collect_ref_targets(&st);
                let filtered: Vec<&(String, bool)> = targets.iter().filter(|(name, is_table)| {
                    let kind_ok = match ts.ref_filter {
                        TsRefFilter::All => true,
                        TsRefFilter::Table => *is_table,
                        TsRefFilter::Enum => !*is_table,
                    };
                    let search_ok = search.is_empty() || name.to_lowercase().contains(&search);
                    kind_ok && search_ok
                }).collect();
                filtered.get(i as usize).map(|(n, _)| n.clone())
            };
            if let Some(name) = chosen {
                s.borrow_mut().type_selector.ref_name = name;
                if let Some(ui) = weak.upgrade() { push_type_selector(&ui, &s); }
            }
        });
    }
    // ts-confirm：构造 type 字符串写回；slint 端会自动把 dlg-type-open 设 false
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_confirm(move || {
            let mut st = s.borrow_mut();
            // 计算 type 字符串
            let type_str = match st.type_selector.tab {
                TsTab::Data => st.type_selector.data_type().to_type_string(),
                TsTab::Reference => match st.type_selector.ref_type() {
                    Some(t) => t.to_type_string(),
                    None => { st.type_selector.close(); return; }
                }
            };
            // 写回 target
            let target = st.type_selector.target.clone();
            let group = st.type_selector.editing_group.clone();
            let name = st.type_selector.editing_name.clone();
            let is_table = st.type_selector.editing_source_table;
            match target {
                Some(TypeEditTarget::HeaderType { col }) => {
                    if is_table {
                        st.engine.commit_header_edit(&group, &name, 2, col, type_str);
                        if st.realtime_validate { st.engine.revalidate(&group, &name); }
                    }
                }
                Some(TypeEditTarget::CellType { row, col }) => {
                    if !is_table {
                        // Constant：col 应该是 1（type 列）
                        st.engine.commit_constant_cell(&group, &name, row, col, type_str);
                        if st.realtime_validate { st.engine.revalidate(&group, &name); }
                    }
                }
                None => {}
            }
            st.type_selector.close();
            drop(st);
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_tree(&ui, &s);
                push_type_selector(&ui, &s);
            }
        });
    }
    // ts-cancel
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ts_cancel(move || {
            s.borrow_mut().type_selector.close();
            if let Some(ui) = weak.upgrade() { push_type_selector(&ui, &s); }
        });
    }
}

// ──────── RefPicker ────────

/// 收集被引用项 (table or enum) 的所有可选条目，返回 (is_table, [(id, name, desc)])。
/// None = 未找到该 ref_name 对应的项。
fn collect_ref_rows(state: &AppState, ref_name: &str) -> Option<(bool, Vec<(String, String, String)>)> {
    for g in &state.engine.project.groups {
        for t in &g.tables {
            if t.deleted || t.name != ref_name { continue; }
            let id_idx = t.schema.fields.iter().position(|f| f.name == "id");
            let name_idx = t.schema.fields.iter().position(|f| f.name == "name");
            let mut rows = Vec::new();
            if let Some(idx) = id_idx {
                for row in &t.records {
                    let id = row.get(idx).cloned().unwrap_or_default();
                    if id.is_empty() { continue; }
                    let nm = name_idx.and_then(|i| row.get(i).cloned()).unwrap_or_default();
                    rows.push((id, nm, String::new()));
                }
            }
            return Some((true, rows));
        }
        for e in &g.enums {
            if e.deleted || e.name != ref_name { continue; }
            let rows: Vec<(String, String, String)> = e.entries.iter()
                .filter(|en| !en.id.is_empty())
                .map(|en| (en.id.clone(), en.name.clone(), en.desc.clone()))
                .collect();
            return Some((false, rows));
        }
    }
    None
}

/// Ref 列单击 → 打开 RefPicker。current_value 取自该 cell 的真实存储值。
fn open_ref_picker_for_cell(state: &Rc<RefCell<AppState>>, r: usize, c: usize, ref_target: &str) {
    let mut st = state.borrow_mut();
    let (group, name, is_table, current) = match &st.selected {
        Some(SelectedNode::Table { group, name }) => {
            let cur = st.engine.find_table(group, name)
                .and_then(|t| t.records.get(r).and_then(|row| row.get(c)).cloned())
                .unwrap_or_default();
            (group.clone(), name.clone(), true, cur)
        }
        Some(SelectedNode::Constant { group, name }) => {
            // Constant 不允许 Ref 类型；保留兜底
            (group.clone(), name.clone(), false, String::new())
        }
        _ => return,
    };
    st.ref_picker.open_with(ref_target, &current, r, c, &group, &name, is_table);
}

/// 把 RefPickerState 派生成 slint 端属性并 push。
fn push_ref_picker(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let rp = &st.ref_picker;
    ui.set_dlg_ref_open(rp.open);
    if !rp.open { return; }

    ui.set_rp_ref_name(rp.ref_name.clone().into());
    ui.set_rp_search(rp.search.clone().into());

    let target = collect_ref_rows(&st, &rp.ref_name);
    let (kind_label, target_missing, show_desc, rows): (&str, bool, bool, Vec<(String, String, String)>) = match target {
        Some((true, rows))  => ("📊 表引用", false, false, rows),
        Some((false, rows)) => ("🔢 枚举引用", false, true, rows),
        None => ("⚠️ 引用不存在", true, true, Vec::new()),
    };
    ui.set_rp_kind_label(kind_label.into());
    ui.set_rp_target_missing(target_missing);
    ui.set_rp_show_desc(show_desc);

    let search = rp.search.to_lowercase();
    let filtered: Vec<RefRow> = rows.iter().filter(|(id, name, desc)| {
        if search.is_empty() { return true; }
        id.to_lowercase().contains(&search)
            || name.to_lowercase().contains(&search)
            || desc.to_lowercase().contains(&search)
    }).map(|(id, name, desc)| RefRow {
        id: id.clone().into(),
        name: name.clone().into(),
        desc: if show_desc { desc.clone().into() } else { slint::SharedString::new() },
        selected: rp.selected_id == *id,
    }).collect();
    ui.set_rp_rows(slint::ModelRc::new(slint::VecModel::from(filtered)));

    let preview = if rp.selected_id.is_empty() {
        String::from("（未选择）")
    } else {
        rows.iter().find(|(id, _, _)| id == &rp.selected_id)
            .map(|(id, n, _)| if n.is_empty() { id.clone() } else { format!("{} ({})", id, n) })
            .unwrap_or_else(|| format!("{} ⚠️ 不存在", rp.selected_id))
    };
    ui.set_rp_selection_preview(preview.into());
}

/// 写回当前 RefPicker 的选中 id 到 cell（空字符串 = 清空）。
fn commit_ref_picker(state: &Rc<RefCell<AppState>>, value: String) {
    let mut st = state.borrow_mut();
    let r = match st.ref_picker.editing_row { Some(v) => v, None => return };
    let c = match st.ref_picker.editing_col { Some(v) => v, None => return };
    let group = st.ref_picker.editing_group.clone();
    let name = st.ref_picker.editing_name.clone();
    let is_table = st.ref_picker.editing_source_table;
    if is_table {
        st.engine.commit_table_cell(&group, &name, r, c, value);
    } else {
        st.engine.commit_constant_cell(&group, &name, r, c, value);
    }
    if st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
    st.ref_picker.close();
}

fn wire_ref_picker(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // rp-set-search → 已在 app.slint 内：set-search(s) => { root.rp-search = s; }
    // 但 rp-search 改了不会自动触发 push；hook 一个外部 callback 让 Rust 重 push。
    // 当前 app.slint 用 in-out + LineEdit edited(s) => set-search(s) → root.rp-search = s。
    // 没有专门的 callback。简化：直接用 in-out 让 slint 自己保持 text；过滤逻辑由 push 时取
    // ui.get_rp_search() 兜底。但 RefPickerState.search 在 Rust 侧是真值，
    // 必须监听到 rp-search 变化。最简洁：让 select-row 时把 ui.get_rp_search() 同步过来。
    // 同样地，我们也给搜索建一个 callback。先在 set-search 里调用一个 Rust 侧 hook。

    // 当前 app.slint 的 RefPicker.set-search(s) 处理方式只把 rp-search 写回 root，没调用 Rust。
    // 这里通过 ui.on_<...> 监听 rp callbacks（select/double/confirm/clear/cancel）拿到状态：
    // - select-row(i)：以 push 之后过滤后的索引 i 拿到 id，写入 selected_id 并重 push
    // - row-double-clicked(i)：select + 直接 confirm
    // - confirm：写回 selected_id
    // - clear：写回空字符串
    // - cancel：仅关闭

    // 取 rp-search 的快照：每次回调用 ui.get_rp_search() 同步到 Rust state.search
    // 这样不必为搜索单独建 callback。
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_rp_select_row(move |i| {
            sync_rp_search(&s, &weak);
            let chosen: Option<String> = {
                let st = s.borrow();
                let rp = &st.ref_picker;
                let target = collect_ref_rows(&st, &rp.ref_name);
                let rows: Vec<(String, String, String)> = match target {
                    Some((_, rows)) => rows,
                    None => Vec::new(),
                };
                let search = rp.search.to_lowercase();
                let filtered: Vec<&(String, String, String)> = rows.iter().filter(|(id, name, desc)| {
                    if search.is_empty() { return true; }
                    id.to_lowercase().contains(&search)
                        || name.to_lowercase().contains(&search)
                        || desc.to_lowercase().contains(&search)
                }).collect();
                filtered.get(i as usize).map(|(id, _, _)| id.clone())
            };
            if let Some(id) = chosen {
                s.borrow_mut().ref_picker.selected_id = id;
                if let Some(ui) = weak.upgrade() { push_ref_picker(&ui, &s); }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_rp_row_double_clicked(move |i| {
            sync_rp_search(&s, &weak);
            let chosen: Option<String> = {
                let st = s.borrow();
                let rp = &st.ref_picker;
                let target = collect_ref_rows(&st, &rp.ref_name);
                let rows: Vec<(String, String, String)> = match target {
                    Some((_, rows)) => rows,
                    None => Vec::new(),
                };
                let search = rp.search.to_lowercase();
                let filtered: Vec<&(String, String, String)> = rows.iter().filter(|(id, name, desc)| {
                    if search.is_empty() { return true; }
                    id.to_lowercase().contains(&search)
                        || name.to_lowercase().contains(&search)
                        || desc.to_lowercase().contains(&search)
                }).collect();
                filtered.get(i as usize).map(|(id, _, _)| id.clone())
            };
            if let Some(id) = chosen {
                s.borrow_mut().ref_picker.selected_id = id.clone();
                commit_ref_picker(&s, id);
                if let Some(ui) = weak.upgrade() {
                    push_grid(&ui, &s);
                    push_tree(&ui, &s);
                    push_ref_picker(&ui, &s);
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_rp_confirm(move || {
            sync_rp_search(&s, &weak);
            let id = s.borrow().ref_picker.selected_id.clone();
            commit_ref_picker(&s, id);
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_tree(&ui, &s);
                push_ref_picker(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_rp_clear(move || {
            commit_ref_picker(&s, String::new());
            if let Some(ui) = weak.upgrade() {
                push_grid(&ui, &s);
                push_tree(&ui, &s);
                push_ref_picker(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_rp_cancel(move || {
            s.borrow_mut().ref_picker.close();
            if let Some(ui) = weak.upgrade() { push_ref_picker(&ui, &s); }
        });
    }
}

/// 把 slint 端 rp-search 文本同步回 Rust state，方便后续 collect/filter 用。
fn sync_rp_search(state: &Rc<RefCell<AppState>>, ui_weak: &slint::Weak<AppWindow>) {
    if let Some(ui) = ui_weak.upgrade() {
        let s = ui.get_rp_search().to_string();
        state.borrow_mut().ref_picker.search = s;
    }
}

// ──────── ContextMenu / InputDialog / ConfirmDialog ────────

/// 计算当前 ctx_menu.kind 应展示的菜单项列表。
/// action-id 形如 "tree.new-group" / "grid.col-insert-left" 等，由 wire_context_menu 分发。
fn ctx_menu_items_for(kind: &CtxMenuKind) -> Vec<CtxMenuItem> {
    let sep = || CtxMenuItem {
        label: slint::SharedString::new(),
        action_id: slint::SharedString::new(),
        is_separator: true,
        disabled: false,
    };
    let item = |label: &str, id: &str, disabled: bool| CtxMenuItem {
        label: label.into(),
        action_id: id.into(),
        is_separator: false,
        disabled,
    };
    match kind {
        CtxMenuKind::TreeBlank => vec![
            item("新建 Group", "tree.new-group", false),
        ],
        CtxMenuKind::TreeGroup { .. } => vec![
            item("新建 Table", "tree.new-table", false),
            item("新建 Constant", "tree.new-constant", false),
            item("新建 Enum", "tree.new-enum", false),
            sep(),
            item("重命名", "tree.rename-group", false),
            item("删除", "tree.delete-group", false),
        ],
        CtxMenuKind::TreeNode { .. } => vec![
            item("复制", "tree.copy-node", false),
            item("重命名", "tree.rename-node", false),
            item("删除", "tree.delete-node", false),
        ],
        CtxMenuKind::GridCol { .. } => vec![
            item("左侧插入列", "grid.col-insert-left", false),
            item("右侧插入列", "grid.col-insert-right", false),
            item("删除列", "grid.col-delete", false),
        ],
        CtxMenuKind::GridRow { .. } => vec![
            item("上方插入行", "grid.row-insert-above", false),
            item("下方插入行", "grid.row-insert-below", false),
            item("删除行", "grid.row-delete", false),
        ],
        CtxMenuKind::GridCell { .. } => vec![
            item("复制", "grid.cell-copy", false),
            item("剪切", "grid.cell-cut", false),
            item("粘贴", "grid.cell-paste", false),
            item("删除内容", "grid.cell-clear", false),
        ],
    }
}

fn push_context_menu(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let cm = &st.ctx_menu;
    ui.set_ctx_menu_open(cm.open);
    if !cm.open {
        ui.set_ctx_menu_items(slint::ModelRc::new(slint::VecModel::from(Vec::<CtxMenuItem>::new())));
        return;
    }
    let kind = match &cm.kind { Some(k) => k, None => return };
    // slint length 属性的 setter 接受 Coord（f32）
    ui.set_ctx_menu_x(cm.x);
    ui.set_ctx_menu_y(cm.y);
    ui.set_ctx_menu_items(slint::ModelRc::new(slint::VecModel::from(ctx_menu_items_for(kind))));
}

fn push_input_dialog(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    match &st.pending.action {
        Some(action) if action.needs_input() => {
            ui.set_dlg_input_open(true);
            ui.set_dlg_input_title(action.input_title().into());
            ui.set_dlg_input_label("名称:".into());
            ui.set_dlg_input_buffer(st.pending.input_buffer.clone().into());
            ui.set_dlg_input_error(st.pending.error.clone().unwrap_or_default().into());
            let can_confirm = st.pending.error.is_none() && !st.pending.input_buffer.is_empty();
            ui.set_dlg_input_can_confirm(can_confirm);
        }
        _ => {
            ui.set_dlg_input_open(false);
            ui.set_dlg_input_buffer(slint::SharedString::new());
            ui.set_dlg_input_error(slint::SharedString::new());
            ui.set_dlg_input_can_confirm(false);
        }
    }
}

fn push_confirm_dialog(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    match &st.pending.action {
        Some(action) if action.needs_confirm() => {
            ui.set_dlg_confirm_open(true);
            ui.set_dlg_confirm_title(action.confirm_title().into());
            ui.set_dlg_confirm_message(action.confirm_message().into());
        }
        _ => {
            ui.set_dlg_confirm_open(false);
        }
    }
}

/// 根据 PendingAction 当前 input_buffer，刷新 error 字段（命名校验）。
fn revalidate_pending_input(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    let action = match &st.pending.action { Some(a) => a.clone(), None => return };
    let buf = st.pending.input_buffer.clone();
    let err: Option<String> = match &action {
        PendingAction::NewGroup => st.engine.validate_group_name(&buf),
        PendingAction::RenameGroup { old_name } => st.engine.validate_group_name_rename(&buf, old_name),
        PendingAction::RenameNode { old_name, .. } => st.engine.validate_node_name_rename(&buf, old_name),
        PendingAction::NewTable { .. } | PendingAction::NewConstant { .. } | PendingAction::NewEnum { .. } =>
            st.engine.validate_node_name(&buf),
        _ => None,
    };
    st.pending.error = err;
}

fn execute_pending_action(state: &Rc<RefCell<AppState>>) {
    use tbl_core::ops::ProjectAction;
    let mut st = state.borrow_mut();
    let action = match st.pending.action.clone() { Some(a) => a, None => return };
    let buf = st.pending.input_buffer.clone();
    let core_action = match &action {
        PendingAction::NewGroup => ProjectAction::NewGroup { name: buf.clone() },
        PendingAction::NewTable { group } => ProjectAction::NewTable { group: group.clone(), name: buf.clone() },
        PendingAction::NewConstant { group } => ProjectAction::NewConstant { group: group.clone(), name: buf.clone() },
        PendingAction::NewEnum { group } => ProjectAction::NewEnum { group: group.clone(), name: buf.clone() },
        PendingAction::RenameGroup { old_name } => ProjectAction::RenameGroup { old_name: old_name.clone(), new_name: buf.clone() },
        PendingAction::RenameNode { group, old_name } => ProjectAction::RenameNode { group: group.clone(), old_name: old_name.clone(), new_name: buf.clone() },
        PendingAction::DeleteGroup { group } => {
            st.engine.delete_group(group);
            // 如果当前选中的节点在被删除的 group 下，清空选中
            if let Some(SelectedNode::Table { group: g, .. }
                | SelectedNode::Constant { group: g, .. }
                | SelectedNode::Enum { group: g, .. }) = &st.selected
            {
                if g == group { st.selected = None; st.grid_selection = GridSelection::None; }
            }
            st.pending.close();
            return;
        }
        PendingAction::DeleteNode { group, name } => {
            st.engine.delete_node(group, name);
            if let Some(SelectedNode::Table { group: g, name: n }
                | SelectedNode::Constant { group: g, name: n }
                | SelectedNode::Enum { group: g, name: n }) = &st.selected
            {
                if g == group && n == name { st.selected = None; st.grid_selection = GridSelection::None; }
            }
            st.pending.close();
            return;
        }
    };
    if matches!(action, PendingAction::NewGroup) {
        st.tree_expanded.insert(buf.clone());
    }
    st.engine.execute_action(&core_action);
    st.pending.close();
}

/// 列右键操作（依赖当前选中 Table）
fn perform_grid_col_action(state: &Rc<RefCell<AppState>>, col: usize, action: &str) {
    let mut st = state.borrow_mut();
    let (group, name) = match &st.selected {
        Some(SelectedNode::Table { group, name }) => (group.clone(), name.clone()),
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

/// 行右键操作（Table 走 insert/delete_row；Constant/Enum 暂只支持当前节点的 row 删除占位）
fn perform_grid_row_action(state: &Rc<RefCell<AppState>>, row: usize, action: &str) {
    let mut st = state.borrow_mut();
    let (group, name, is_table, is_constant, is_enum) = match &st.selected {
        Some(SelectedNode::Table { group, name }) => (group.clone(), name.clone(), true, false, false),
        Some(SelectedNode::Constant { group, name }) => (group.clone(), name.clone(), false, true, false),
        Some(SelectedNode::Enum { group, name }) => (group.clone(), name.clone(), false, false, true),
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
        // Constant 没有专门的 insert/delete API；用 entries 直接增删
        if let Some(g) = st.engine.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                use tbl_core::model::{ConstEntry, Export};
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
        if let Some(g) = st.engine.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(e) = g.enums.iter_mut().find(|e| e.name == name) {
                use tbl_core::model::EnumEntry;
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
/// 范围语义对齐 Excel + egui：
/// - Copy: TSV 拼接区域，UI 日志只打范围（如 `[Ctrl+C] B2:D5 (4行×3列)`），不打内容
/// - Paste: clipboard TSV 从锚点展开覆盖
/// - Clear: 清空区域所有 cell
fn perform_grid_action(state: &Rc<RefCell<AppState>>, action: &str, tag: &str) {
    use arboard::Clipboard;
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
            // TSV：行内 \t、行间 \n
            let tsv = {
                let st = state.borrow();
                (r1..=r2).map(|r| {
                    (c1..=c2).map(|c| convert::raw_cell_for(&st, r, c))
                        .collect::<Vec<_>>().join("\t")
                }).collect::<Vec<_>>().join("\n")
            };
            match Clipboard::new().and_then(|mut cb| cb.set_text(tsv.clone())) {
                Ok(()) => {
                    let msg = if is_single {
                        format!("[{}] {} = \"{}\"", tag, coord_label, tsv)
                    } else {
                        format!("[{}] {}", tag, coord_label)
                    };
                    state.borrow_mut().engine.log(msg);
                }
                Err(e) => state.borrow_mut().engine.log(format!("[{}] {} 失败: {}", tag, coord_label, e)),
            }
        }
        "grid.cell-paste" => {
            let text = match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                Ok(t) => t,
                Err(e) => {
                    state.borrow_mut().engine.log(format!("[{}] {} 读剪贴板失败: {}", tag, coord_label, e));
                    return;
                }
            };
            // Excel 规则：剪贴板矩形从锚点 (r1,c1) 向右下展开，跟目标选区大小无关。
            // 单格目标 + 多格剪贴板 ⇒ 自动扩展；多格目标 + 单格剪贴板 ⇒ 只填左上角。
            let lines: Vec<&str> = text.lines().collect();
            let row_n = lines.len().max(1);
            let col_n = lines.iter().map(|l| l.split('\t').count()).max().unwrap_or(1);
            let clip_is_single = row_n == 1 && col_n == 1;
            if clip_is_single && is_single {
                // 都是单格：保留 before → after 详细日志
                let single = lines.first().map(|s| s.to_string()).unwrap_or_default();
                let before = {
                    let st = state.borrow();
                    convert::raw_cell_for(&st, r1, c1)
                };
                let mut st = state.borrow_mut();
                st.set_cell(r1, c1, &single);
                st.engine.log(format!("[{}] {} \"{}\" → \"{}\"", tag, coord_label, before, single));
            } else {
                paste_region(state, r1, c1, &text);
                let dst = format!(
                    "{}{}:{}{}",
                    convert::col_letter(c1), r1 + 1,
                    convert::col_letter(c1 + col_n - 1), r1 + row_n,
                );
                state.borrow_mut().engine.log(format!("[{}] {} → {} ({}行×{}列)", tag, coord_label, dst, row_n, col_n));
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
                st.engine.log(format!("[{}] {} \"{}\" → \"\"", tag, coord_label, before));
            } else {
                clear_region(state, r1, c1, r2, c2);
                state.borrow_mut().engine.log(format!("[{}] {}", tag, coord_label));
            }
        }
        "grid.cell-cut" => {
            // 剪切 = 复制到剪贴板 + 清空原区域
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
                        st.engine.log(format!("[{}] {} \"{}\" → \"\"", tag, coord_label, tsv));
                    } else {
                        clear_region(state, r1, c1, r2, c2);
                        state.borrow_mut().engine.log(format!("[{}] {}", tag, coord_label));
                    }
                }
                Err(e) => state.borrow_mut().engine.log(format!("[{}] {} 失败: {}", tag, coord_label, e)),
            }
        }
        _ => {}
    }
}

/// 把当前 GridSelection 解析成裁剪到表实际尺寸的矩形 (r1,c1,r2,c2)。
/// 整行/整列时按 grid_dims 截断。无选区返回 None。
fn resolve_selection_rect(state: &Rc<RefCell<AppState>>) -> Option<(usize, usize, usize, usize)> {
    let (rows, cols) = grid_dims(state);
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
fn paste_region(state: &Rc<RefCell<AppState>>, r1: usize, c1: usize, text: &str) {
    let (group, name, kind) = match state.borrow().selected.clone() {
        Some(SelectedNode::Table { group, name }) => (group, name, "table"),
        Some(SelectedNode::Constant { group, name }) => (group, name, "constant"),
        Some(SelectedNode::Enum { group, name }) => (group, name, "enum"),
        None => return,
    };
    let mut st = state.borrow_mut();
    match kind {
        "table" => st.engine.paste_table_data(&group, &name, r1, c1, text),
        "constant" => st.engine.paste_constant_data(&group, &name, r1, c1, text),
        _ => st.engine.paste_enum_data(&group, &name, r1, c1, text),
    }
    if st.realtime_validate { st.engine.revalidate(&group, &name); }
}

/// 区域清空：按当前选中节点类型走 engine.clear_*_cells。
fn clear_region(state: &Rc<RefCell<AppState>>, r1: usize, c1: usize, r2: usize, c2: usize) {
    let (group, name, kind) = match state.borrow().selected.clone() {
        Some(SelectedNode::Table { group, name }) => (group, name, "table"),
        Some(SelectedNode::Constant { group, name }) => (group, name, "constant"),
        Some(SelectedNode::Enum { group, name }) => (group, name, "enum"),
        None => return,
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

fn wire_context_menu(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // ctx-menu-dismiss
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ctx_menu_dismiss(move || {
            s.borrow_mut().ctx_menu.close();
            if let Some(ui) = weak.upgrade() { push_context_menu(&ui, &s); }
        });
    }
    // ctx-menu-action(action_id)：根据当前 ctx_menu.kind + id 分发
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ctx_menu_action(move |id| {
            let id = id.to_string();
            // 取走当前 kind 后立即关闭菜单
            let kind = {
                let mut st = s.borrow_mut();
                let k = st.ctx_menu.kind.clone();
                st.ctx_menu.close();
                k
            };
            match (kind, id.as_str()) {
                // ── 树空白 ──
                (Some(CtxMenuKind::TreeBlank), "tree.new-group") => {
                    s.borrow_mut().pending.open(PendingAction::NewGroup);
                }
                // ── 树 Group ──
                (Some(CtxMenuKind::TreeGroup { name }), "tree.new-table") => {
                    s.borrow_mut().pending.open(PendingAction::NewTable { group: name });
                }
                (Some(CtxMenuKind::TreeGroup { name }), "tree.new-constant") => {
                    s.borrow_mut().pending.open(PendingAction::NewConstant { group: name });
                }
                (Some(CtxMenuKind::TreeGroup { name }), "tree.new-enum") => {
                    s.borrow_mut().pending.open(PendingAction::NewEnum { group: name });
                }
                (Some(CtxMenuKind::TreeGroup { name }), "tree.rename-group") => {
                    let mut st = s.borrow_mut();
                    st.pending.open(PendingAction::RenameGroup { old_name: name.clone() });
                    st.pending.input_buffer = name;
                }
                (Some(CtxMenuKind::TreeGroup { name }), "tree.delete-group") => {
                    s.borrow_mut().pending.open(PendingAction::DeleteGroup { group: name });
                }
                // ── 树节点 ──
                (Some(CtxMenuKind::TreeNode { group, name, kind }), "tree.copy-node") => {
                    let mut st = s.borrow_mut();
                    st.engine.copy_node(&group, &name, kind);
                }
                (Some(CtxMenuKind::TreeNode { group, name, .. }), "tree.rename-node") => {
                    let mut st = s.borrow_mut();
                    st.pending.open(PendingAction::RenameNode { group, old_name: name.clone() });
                    st.pending.input_buffer = name;
                }
                (Some(CtxMenuKind::TreeNode { group, name, .. }), "tree.delete-node") => {
                    s.borrow_mut().pending.open(PendingAction::DeleteNode { group, name });
                }
                // ── grid 列 ──
                (Some(CtxMenuKind::GridCol { col }), action @ ("grid.col-insert-left"
                    | "grid.col-insert-right" | "grid.col-delete")) => {
                    perform_grid_col_action(&s, col, action);
                }
                // ── grid 行 ──
                (Some(CtxMenuKind::GridRow { row }), action @ ("grid.row-insert-above"
                    | "grid.row-insert-below" | "grid.row-delete")) => {
                    perform_grid_row_action(&s, row, action);
                }
                // ── grid 单元格 ──
                (Some(CtxMenuKind::GridCell { row: _, col: _ }), action @ ("grid.cell-copy"
                    | "grid.cell-cut" | "grid.cell-paste" | "grid.cell-clear")) => {
                    let tag = match action {
                        "grid.cell-copy" => "复制",
                        "grid.cell-cut" => "剪切",
                        "grid.cell-paste" => "粘贴",
                        "grid.cell-clear" => "清空",
                        _ => "操作",
                    };
                    perform_grid_action(&s, action, tag);
                }
                _ => {}
            }
            // pending input 需要校验首次 buffer
            revalidate_pending_input(&s);
            if let Some(ui) = weak.upgrade() {
                push_context_menu(&ui, &s);
                push_input_dialog(&ui, &s);
                push_confirm_dialog(&ui, &s);
                push_tree(&ui, &s);
                push_grid(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    // 输入对话框：set-input / confirm / cancel
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_inp_set_input(move |t| {
            s.borrow_mut().pending.input_buffer = t.to_string();
            revalidate_pending_input(&s);
            if let Some(ui) = weak.upgrade() { push_input_dialog(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_inp_confirm(move || {
            // 再校验一次
            revalidate_pending_input(&s);
            let ok = {
                let st = s.borrow();
                st.pending.error.is_none() && !st.pending.input_buffer.is_empty()
            };
            if !ok {
                if let Some(ui) = weak.upgrade() { push_input_dialog(&ui, &s); }
                return;
            }
            execute_pending_action(&s);
            if let Some(ui) = weak.upgrade() {
                push_input_dialog(&ui, &s);
                push_tree(&ui, &s);
                push_grid(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_inp_cancel(move || {
            s.borrow_mut().pending.close();
            if let Some(ui) = weak.upgrade() { push_input_dialog(&ui, &s); }
        });
    }
    // 确认对话框：confirm / cancel
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_cf_confirm(move || {
            execute_pending_action(&s);
            if let Some(ui) = weak.upgrade() {
                push_confirm_dialog(&ui, &s);
                push_tree(&ui, &s);
                push_grid(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_cf_cancel(move || {
            s.borrow_mut().pending.close();
            if let Some(ui) = weak.upgrade() { push_confirm_dialog(&ui, &s); }
        });
    }
}

// ──────── 数据导出 / Schema 导出 / Schema 导入 ────────

/// 把当前 project 的 groups/tables/constants/enums 扁平化为 SchemaExportItem 列表，
/// 默认勾选全部（Schema 导出对话框打开时调用）。
fn rebuild_schema_export_items(st: &mut AppState) {
    let mut items: Vec<state::SchemaExportItem> = Vec::new();
    for g in &st.engine.project.groups {
        let mut sub: Vec<state::SchemaExportItem> = Vec::new();
        for t in &g.tables {
            if t.deleted { continue; }
            sub.push(state::SchemaExportItem { indent: 1, group: g.name.clone(), name: t.name.clone(), is_table: true });
        }
        for c in &g.constants {
            if c.deleted { continue; }
            sub.push(state::SchemaExportItem { indent: 1, group: g.name.clone(), name: c.name.clone(), is_table: false });
        }
        // schema_from_project 同样跳过 enum 段，这里和 egui 端一致
        if sub.is_empty() { continue; }
        items.push(state::SchemaExportItem { indent: 0, group: g.name.clone(), name: g.name.clone(), is_table: false });
        items.extend(sub);
    }
    st.schema_export.checked = vec![true; items.len()];
    st.schema_export.items = items;
}
/// 把 DataExportState 推到 slint 端 dialog 属性。
fn push_data_export(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let de = &st.data_export;
    ui.set_dlg_export_open(de.open);
    ui.set_ex_json(de.json);
    ui.set_ex_xml(de.xml);
    ui.set_ex_java(de.java);
    ui.set_ex_go(de.go);
    ui.set_ex_lua(de.lua);
}
/// 把 SchemaExportState 推到 slint 端：组节点 tristate 由其下子节点聚合。
fn push_schema_export(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let sx = &st.schema_export;
    ui.set_dlg_schema_export_open(sx.open);
    if !sx.open { return; }

    // 计算每个组节点位置 → (子项 [start..end))
    let n = sx.items.len();
    let mut group_ranges: Vec<(usize, usize, usize)> = Vec::new(); // (group_idx, start, end)
    let mut i = 0;
    while i < n {
        if sx.items[i].indent == 0 {
            let group_idx = i;
            let mut j = i + 1;
            while j < n && sx.items[j].indent != 0 { j += 1; }
            group_ranges.push((group_idx, i + 1, j));
            i = j;
        } else { i += 1; }
    }

    let mut slint_items: Vec<SchemaItem> = Vec::with_capacity(n);
    for (idx, item) in sx.items.iter().enumerate() {
        let (checked, tristate, icon) = if item.indent == 0 {
            // 组节点：聚合
            let (_, start, end) = group_ranges.iter().find(|(g, _, _)| *g == idx).copied().unwrap_or((idx, idx + 1, idx + 1));
            let mut all = true;
            let mut any = false;
            for k in start..end {
                if sx.checked.get(k).copied().unwrap_or(false) { any = true; } else { all = false; }
            }
            (all && start < end, any && !all, "📁".to_string())
        } else {
            let icon = if item.is_table { "📊" } else { "📋" };
            (sx.checked.get(idx).copied().unwrap_or(false), false, icon.to_string())
        };
        slint_items.push(SchemaItem {
            indent: item.indent as i32,
            icon: icon.into(),
            name: item.name.clone().into(),
            group_name: item.group.clone().into(),
            checked,
            tristate,
            is_conflict: false,
        });
    }

    // 总数 / 已选数（仅子节点参与）
    let total: i32 = sx.items.iter().filter(|it| it.indent == 1).count() as i32;
    let selected: i32 = sx.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && sx.checked.get(*i).copied().unwrap_or(false))
        .count() as i32;
    let all_checked = selected == total && total > 0;

    ui.set_sx_items(slint::ModelRc::new(slint::VecModel::from(slint_items)));
    ui.set_sx_all_checked(all_checked);
    ui.set_sx_selected_count(selected);
    ui.set_sx_total_count(total);
}
/// 把 SchemaImportState 推到 slint 端：file_loaded / items / 冲突计数。
fn push_schema_import(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    use tbl_core::tblschema::SchemaMode;
    let st = state.borrow();
    let si = &st.schema_import;
    ui.set_dlg_schema_import_open(si.open);
    if !si.open { return; }

    ui.set_si_file_path(si.file_path.clone().into());
    let file_loaded = si.schema.is_some();
    ui.set_si_file_loaded(file_loaded);
    if !file_loaded {
        ui.set_si_items(slint::ModelRc::new(slint::VecModel::from(Vec::<SchemaItem>::new())));
        ui.set_si_all_checked(false);
        ui.set_si_selected_count(0);
        ui.set_si_total_count(0);
        ui.set_si_conflict_count(0);
        return;
    }

    let n = si.items.len();
    // 同样的 group 分段
    let mut group_ranges: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if si.items[i].indent == 0 {
            let group_idx = i;
            let mut j = i + 1;
            while j < n && si.items[j].indent != 0 { j += 1; }
            group_ranges.push((group_idx, i + 1, j));
            i = j;
        } else { i += 1; }
    }

    let mut slint_items: Vec<SchemaItem> = Vec::with_capacity(n);
    for (idx, item) in si.items.iter().enumerate() {
        let (checked, tristate, icon, is_conflict) = if item.indent == 0 {
            let (_, start, end) = group_ranges.iter().find(|(g, _, _)| *g == idx).copied().unwrap_or((idx, idx + 1, idx + 1));
            let mut all = true;
            let mut any = false;
            for k in start..end {
                if si.checked.get(k).copied().unwrap_or(false) { any = true; } else { all = false; }
            }
            (all && start < end, any && !all, "📁".to_string(), false)
        } else {
            let icon = match item.mode {
                SchemaMode::Table => "📊",
                SchemaMode::Constant => "📋",
                SchemaMode::Enum => "🔢",
            };
            (
                si.checked.get(idx).copied().unwrap_or(false),
                false,
                icon.to_string(),
                si.conflicts.get(idx).copied().unwrap_or(false),
            )
        };
        slint_items.push(SchemaItem {
            indent: item.indent as i32,
            icon: icon.into(),
            name: item.name.clone().into(),
            group_name: item.group.clone().into(),
            checked,
            tristate,
            is_conflict,
        });
    }

    let total: i32 = si.items.iter().filter(|it| it.indent == 1).count() as i32;
    let selected: i32 = si.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && si.checked.get(*i).copied().unwrap_or(false))
        .count() as i32;
    let conflict: i32 = si.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1
            && si.checked.get(*i).copied().unwrap_or(false)
            && si.conflicts.get(*i).copied().unwrap_or(false))
        .count() as i32;
    let all_checked = selected == total && total > 0;

    ui.set_si_items(slint::ModelRc::new(slint::VecModel::from(slint_items)));
    ui.set_si_all_checked(all_checked);
    ui.set_si_selected_count(selected);
    ui.set_si_total_count(total);
    ui.set_si_conflict_count(conflict);
}
/// 接通三个对话框（数据导出 / Schema 导出 / Schema 导入）的回调。
fn wire_dialogs(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // ── 数据导出：ex-confirm ──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_ex_confirm(move || {
            // slint 端 ex-* 是 in-out，先把当前 UI 值同步回 Rust state
            let (json, xml, java, go, lua) = match weak.upgrade() {
                Some(ui) => (
                    ui.get_ex_json(), ui.get_ex_xml(), ui.get_ex_java(),
                    ui.get_ex_go(), ui.get_ex_lua(),
                ),
                None => return,
            };
            {
                let mut st = s.borrow_mut();
                st.data_export.json = json;
                st.data_export.xml = xml;
                st.data_export.java = java;
                st.data_export.go = go;
                st.data_export.lua = lua;
                st.data_export.open = false;
            }
            run_data_export(&s);
            if let Some(ui) = weak.upgrade() {
                push_data_export(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    // ── Schema 导出：sx-toggle-all ──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_sx_toggle_all(move |checked| {
            {
                let mut st = s.borrow_mut();
                let n = st.schema_export.items.len();
                if st.schema_export.checked.len() != n {
                    st.schema_export.checked = vec![checked; n];
                } else {
                    for i in 0..n {
                        if st.schema_export.items[i].indent == 1 {
                            st.schema_export.checked[i] = checked;
                        }
                    }
                }
            }
            if let Some(ui) = weak.upgrade() { push_schema_export(&ui, &s); }
        });
    }
    // ── Schema 导出：sx-toggle-item（点击单项；点击组行 flip 整组）──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_sx_toggle_item(move |idx| {
            {
                let mut st = s.borrow_mut();
                let i = idx as usize;
                if i >= st.schema_export.items.len() { return; }
                if st.schema_export.items[i].indent == 0 {
                    // 组节点 → 切换整组
                    let n = st.schema_export.items.len();
                    let mut start = i + 1;
                    let mut end = n;
                    for j in (i + 1)..n {
                        if st.schema_export.items[j].indent == 0 { end = j; break; }
                    }
                    if start > n { start = n; }
                    let any_unchecked = (start..end).any(|k|
                        !st.schema_export.checked.get(k).copied().unwrap_or(true));
                    let new_val = any_unchecked;
                    for k in start..end {
                        if k < st.schema_export.checked.len() {
                            st.schema_export.checked[k] = new_val;
                        }
                    }
                } else if i < st.schema_export.checked.len() {
                    st.schema_export.checked[i] = !st.schema_export.checked[i];
                }
            }
            if let Some(ui) = weak.upgrade() { push_schema_export(&ui, &s); }
        });
    }
    // ── Schema 导出：sx-confirm ──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_sx_confirm(move || {
            run_schema_export(&s);
            // 关闭对话框
            s.borrow_mut().schema_export.open = false;
            if let Some(ui) = weak.upgrade() {
                push_schema_export(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    // ── Schema 导入：si-browse-file ──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_si_browse_file(move || {
            let file = rfd::FileDialog::new()
                .add_filter("TblSchema", &["tblschema"])
                .pick_file();
            if let Some(path) = file {
                let path_str = path.display().to_string();
                load_schema_import(&s, &path_str);
            }
            if let Some(ui) = weak.upgrade() {
                push_schema_import(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
    // ── Schema 导入：si-toggle-all ──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_si_toggle_all(move |checked| {
            {
                let mut st = s.borrow_mut();
                let n = st.schema_import.items.len();
                if st.schema_import.checked.len() != n {
                    st.schema_import.checked = vec![checked; n];
                } else {
                    for i in 0..n {
                        if st.schema_import.items[i].indent == 1 {
                            st.schema_import.checked[i] = checked;
                        }
                    }
                }
            }
            if let Some(ui) = weak.upgrade() { push_schema_import(&ui, &s); }
        });
    }
    // ── Schema 导入：si-toggle-item ──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_si_toggle_item(move |idx| {
            {
                let mut st = s.borrow_mut();
                let i = idx as usize;
                if i >= st.schema_import.items.len() { return; }
                if st.schema_import.items[i].indent == 0 {
                    let n = st.schema_import.items.len();
                    let start = i + 1;
                    let mut end = n;
                    for j in (i + 1)..n {
                        if st.schema_import.items[j].indent == 0 { end = j; break; }
                    }
                    let any_unchecked = (start..end).any(|k|
                        !st.schema_import.checked.get(k).copied().unwrap_or(true));
                    let new_val = any_unchecked;
                    for k in start..end {
                        if k < st.schema_import.checked.len() {
                            st.schema_import.checked[k] = new_val;
                        }
                    }
                } else if i < st.schema_import.checked.len() {
                    st.schema_import.checked[i] = !st.schema_import.checked[i];
                }
            }
            if let Some(ui) = weak.upgrade() { push_schema_import(&ui, &s); }
        });
    }
    // ── Schema 导入：si-confirm ──
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_si_confirm(move || {
            run_schema_import(&s);
            s.borrow_mut().schema_import.open = false;
            if let Some(ui) = weak.upgrade() {
                push_tree(&ui, &s);
                push_grid(&ui, &s);
                push_schema_import(&ui, &s);
                push_logs(&ui, &s);
            }
        });
    }
}

/// 执行数据导出（按 data_export 选项）。
fn run_data_export(state: &Rc<RefCell<AppState>>) {
    let opts = state.borrow().data_export.clone();
    let mut st = state.borrow_mut();
    if opts.json {
        match st.engine.export_json() {
            Ok(r) => log_export_result(&mut st, "JSON", &r),
            Err(e) => st.engine.log(format!("[JSON] 错误: {}", e)),
        }
    }
    if opts.xml {
        match st.engine.export_xml() {
            Ok(r) => log_export_result(&mut st, "XML", &r),
            Err(e) => st.engine.log(format!("[XML] 错误: {}", e)),
        }
    }
    if opts.java {
        match st.engine.export_java() {
            Ok(r) => log_export_result(&mut st, "Java", &r),
            Err(e) => st.engine.log(format!("[Java] 错误: {}", e)),
        }
    }
    if opts.go {
        match st.engine.export_go() {
            Ok(r) => log_export_result(&mut st, "Go", &r),
            Err(e) => st.engine.log(format!("[Go] 错误: {}", e)),
        }
    }
    if opts.lua {
        match st.engine.export_lua() {
            Ok(r) => log_export_result(&mut st, "Lua", &r),
            Err(e) => st.engine.log(format!("[Lua] 错误: {}", e)),
        }
    }
}

fn log_export_result(st: &mut AppState, label: &str, result: &tbl_core::export::ExportResult) {
    use tbl_core::export::FileStatus;
    st.engine.log(format!("[{}] {} 新增, {} 修改, {} 删除, {} 不变",
        label, result.added(), result.modified(), result.deleted(), result.unchanged()));
    for f in &result.files {
        match f.status {
            FileStatus::Added => st.engine.log(format!("  [新增] {}", f.path)),
            FileStatus::Modified => st.engine.log(format!("  [修改] {}", f.path)),
            FileStatus::Deleted => st.engine.log(format!("  [删除] {}", f.path)),
            FileStatus::Unchanged => {}
        }
    }
}

/// Schema 导出：把当前勾选项 → SchemaSection → serialize → rfd save。
fn run_schema_export(state: &Rc<RefCell<AppState>>) {
    use tbl_core::tblschema::{TblSchema, schema_from_project, serialize_tblschema};
    let (selected, full_schema) = {
        let st = state.borrow();
        let full = schema_from_project(&st.engine.project.groups);
        let selected: Vec<(String, String)> = st.schema_export.items.iter().enumerate()
            .filter(|(i, it)| it.indent == 1 && st.schema_export.checked.get(*i).copied().unwrap_or(false))
            .map(|(_, it)| (it.group.clone(), it.name.clone()))
            .collect();
        (selected, full)
    };
    let mut sections = Vec::new();
    for (g, n) in &selected {
        if let Some(sec) = full_schema.sections.iter().find(|s| &s.group == g && &s.name == n) {
            sections.push(sec.clone());
        }
    }
    let schema = TblSchema { sections };
    let content = serialize_tblschema(&schema);
    let file = rfd::FileDialog::new()
        .add_filter("TblSchema", &["tblschema"])
        .set_file_name("export.tblschema")
        .save_file();
    if let Some(path) = file {
        match std::fs::write(&path, &content) {
            Ok(_) => state.borrow_mut().engine.log(format!("[导出Schema] 已保存到 {}", path.display())),
            Err(e) => state.borrow_mut().engine.log(format!("[导出Schema] 写入失败: {}", e)),
        }
    }
}

/// Schema 导入：读 file_path → parse → 填充 items/checked/conflicts。
fn load_schema_import(state: &Rc<RefCell<AppState>>, file_path: &str) {
    use tbl_core::tblschema::{parse_tblschema, SchemaMode};
    let mut st = state.borrow_mut();
    st.schema_import.file_path = file_path.to_string();
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            st.engine.log(format!("[导入Schema] 读取失败: {}", e));
            st.schema_import.schema = None;
            st.schema_import.items.clear();
            st.schema_import.checked.clear();
            st.schema_import.conflicts.clear();
            return;
        }
    };
    let schema = match parse_tblschema(&content) {
        Ok(s) => s,
        Err(e) => {
            st.engine.log(format!("[导入Schema] 解析失败: {}", e));
            st.schema_import.schema = None;
            st.schema_import.items.clear();
            st.schema_import.checked.clear();
            st.schema_import.conflicts.clear();
            return;
        }
    };

    // 按 group 分段，并计算 conflict（已存在）
    let mut grouped: Vec<(String, Vec<(String, SchemaMode)>)> = Vec::new();
    for sec in &schema.sections {
        if let Some(entry) = grouped.iter_mut().find(|(g, _)| *g == sec.group) {
            entry.1.push((sec.name.clone(), sec.mode.clone()));
        } else {
            grouped.push((sec.group.clone(), vec![(sec.name.clone(), sec.mode.clone())]));
        }
    }

    let mut items: Vec<state::SchemaImportItem> = Vec::new();
    let mut checked: Vec<bool> = Vec::new();
    let mut conflicts: Vec<bool> = Vec::new();
    let groups = &st.engine.project.groups;
    for (g, secs) in &grouped {
        items.push(state::SchemaImportItem { indent: 0, group: g.clone(), name: g.clone(), mode: SchemaMode::Table });
        checked.push(true);
        conflicts.push(false);
        for (name, mode) in secs {
            let exists = if let Some(grp) = groups.iter().find(|gr| &gr.name == g) {
                match mode {
                    SchemaMode::Table => grp.tables.iter().any(|t| &t.name == name && !t.deleted),
                    SchemaMode::Constant => grp.constants.iter().any(|c| &c.name == name && !c.deleted),
                    SchemaMode::Enum => grp.enums.iter().any(|e| &e.name == name && !e.deleted),
                }
            } else { false };
            items.push(state::SchemaImportItem { indent: 1, group: g.clone(), name: name.clone(), mode: mode.clone() });
            checked.push(true);
            conflicts.push(exists);
        }
    }
    st.schema_import.items = items;
    st.schema_import.checked = checked;
    st.schema_import.conflicts = conflicts;
    st.schema_import.schema = Some(schema);
}

/// Schema 导入：把当前选中的 sections 应用到 project。
fn run_schema_import(state: &Rc<RefCell<AppState>>) {
    use tbl_core::tblschema::apply_schema_to_project;
    let mut st = state.borrow_mut();
    let schema = match st.schema_import.schema.clone() { Some(s) => s, None => return };
    let selected: Vec<(String, String)> = st.schema_import.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && st.schema_import.checked.get(*i).copied().unwrap_or(false))
        .map(|(_, it)| (it.group.clone(), it.name.clone()))
        .collect();
    let sections: Vec<_> = schema.sections.iter()
        .filter(|s| selected.iter().any(|(g, n)| g == &s.group && n == &s.name))
        .cloned().collect();
    let config_dir = st.engine.project.workdir
        .join(&st.engine.project.config.project.config_dir);
    let (added, overwritten) = apply_schema_to_project(
        &mut st.engine.project.groups,
        &sections,
        &config_dir,
    );
    st.engine.log(format!("[导入Schema] 完成: {} 新增, {} 覆盖", added, overwritten));
}


fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Unix-like：kill 0 不发送信号，仅检查目标进程是否存在
        use std::process::Command;
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
