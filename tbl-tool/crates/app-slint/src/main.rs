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

use state::{AppState, GridSelection, SelectedNode, TreeFilter, TreeTarget};

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
    wire_tree(&ui, &app_state);
    wire_grid(&ui, &app_state);
    wire_toolbar(&ui, &app_state);
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
    //   TypeEnumCol   ：TODO（弹 TypeSelector，Step 7）
    //   Ref           ：TODO（弹 RefPicker，Step 8）
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
            // TypeEnumCol / Ref 走选中（dialog 暂未实现）
            // ReadOnly / Text 默认走选中
            s.borrow_mut().grid_selection = GridSelection::Cell(r as usize, c as usize);
            if let Some(ui) = weak.upgrade() {
                let need_full = was_editing || matches!(kind, Some(state::ColumnKind::ExportEnumCol));
                if need_full { push_grid(&ui, &s); } else { push_selection_only(&ui, &s); }
            }
        });
    }
    // Table 表头点击 → 按 grid_header_kinds[hi][ci] 分发：
    //   ExportEnumCol ：选中 + 同步 editing-export-index（slint popup.show 自动弹）
    //   TypeEnumCol   ：TODO（弹 TypeSelector）
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
