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
    wire_focus(&ui, &app_state);

    let result = ui.run().map_err(|e| anyhow::anyhow!("{}", e));
    let _ = std::fs::remove_file(&lock_path);
    result
}

/// 把 engine.logs（"HH:MM:SS msg" 格式的字符串）转成 slint LogEntry 列表。
/// level 推断：消息含 "失败" / "错误" / "[验证]" → error；含 "警告" → warn；其它 info。
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
    ui.set_logs(slint::ModelRc::new(slint::VecModel::from(entries)));
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
        ui.on_grid_cell_context_menu(move |r, c, x, y| {
            s.borrow_mut().ctx_menu.open_at(CtxMenuKind::GridCell { row: r as usize, col: c as usize }, x as f32, y as f32);
            if let Some(ui) = weak.upgrade() { push_context_menu(&ui, &s); }
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
            }
            "reload" => {
                s.borrow_mut().engine.reload();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            _ => {
                // excel / export / export-schema / import-schema：后续 step 处理
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

/// 单元格右键 → 复制/粘贴/删除内容。复制/粘贴使用系统剪贴板（TSV 单格）。
fn perform_grid_cell_action(state: &Rc<RefCell<AppState>>, r: usize, c: usize, action: &str) {
    use arboard::Clipboard;
    match action {
        "grid.cell-copy" => {
            let raw = {
                let st = state.borrow();
                convert::raw_cell_for(&st, r, c)
            };
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(raw);
            }
            state.borrow_mut().engine.log("[右键] 已复制".to_string());
        }
        "grid.cell-paste" => {
            let text = match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                Ok(t) => t,
                Err(_) => return,
            };
            // 单格粘贴：取首行首格内容
            let single = text.lines().next().and_then(|l| l.split('\t').next()).unwrap_or("").to_string();
            let mut st = state.borrow_mut();
            st.set_cell(r, c, &single);
            st.engine.log("[右键] 已粘贴".to_string());
        }
        "grid.cell-clear" => {
            let mut st = state.borrow_mut();
            st.set_cell(r, c, "");
        }
        _ => {}
    }
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
                (Some(CtxMenuKind::GridCell { row, col }), action @ ("grid.cell-copy"
                    | "grid.cell-paste" | "grid.cell-clear")) => {
                    perform_grid_cell_action(&s, row, col, action);
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
