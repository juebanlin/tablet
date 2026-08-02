//! Excel 同步面板 Rust 逻辑。复用 tablet_core::excel_sync 的数据类型。

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Model, VecModel};

use tablet_core::excel_sync;

use crate::state::AppState;
use crate::AppWindow;

pub use excel_sync::DiffCount;

/// 同步面板中一行的操作按钮类型。
#[derive(Clone, PartialEq)]
pub enum SyncAction {
    None,           // 无操作
    Synced,         // 已同步
    PushData,       // [→]
    PullData,       // [←]
    PushWithCols,   // [→含列]
    PullWithCols,   // [←含列]
    ForcePush,      // [强制→]
    ForcePull,      // [强制←]
    Create,         // [→创建]
    Import,         // [←导入]
    Blocked,        // ⛔
}

/// 同步面板中的一行。
pub struct SyncRow {
    pub group: String,
    pub node_name: String,
    pub checked: bool,
    pub left_exists: bool,
    pub left_diff: DiffCount,
    pub right_exists: bool,
    pub right_diff: DiffCount,
    pub right_sheet_name: String,
    pub right_blocks_reason: String,
    pub right_xlsx_name: String,
    pub headers_match: bool,
    pub headers_tail_diff: bool,
    pub headers_mismatch: bool,
    pub actions: Vec<SyncAction>,
}

/// 加载同步面板数据。
pub fn load_sync_data(state: &AppState) -> Vec<SyncRow> {
    let mut rows = Vec::new();
    let Some(pid) = state.engine.active_project_id() else { return rows; };
    let Some(project) = state.engine.find_project(pid) else { return rows; };

    let excel_dir = project.project_root.join(".excel");

    let mut xlsx_groups: std::collections::HashMap<String, std::path::PathBuf> = std::collections::HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&excel_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "xlsx") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    xlsx_groups.insert(stem.to_string(), p.clone());
                }
            }
        }
    }

    for group in &project.groups {
        let xlsx_path = xlsx_groups.get(&group.name);
        let xlsx_sheets: Vec<excel_sync::XlsxSheet> = xlsx_path
            .and_then(|p| excel_sync::read_xlsx_sheets(p).ok())
            .unwrap_or_default();

        for table in &group.tables {
            if table.deleted { continue; }
            let xlsx_sheet = xlsx_sheets.iter().find(|s| s.name == table.name);
            add_table_row(&mut rows, group, table, xlsx_sheet, xlsx_path.map(|_| group.name.as_str()));
        }
        // TODO: Constant / Enum rows

        // xlsx-only sheets
        if xlsx_path.is_some() {
            for sheet in &xlsx_sheets {
                let exists = group.tables.iter().any(|t| t.name == sheet.name);
                if !exists {
                    rows.push(SyncRow {
                        group: group.name.clone(), node_name: sheet.name.clone(), checked: false,
                        left_exists: false, left_diff: DiffCount::default(),
                        right_exists: true, right_diff: DiffCount::default(),
                        right_sheet_name: sheet.name.clone(),
                        right_blocks_reason: String::new(),
                        right_xlsx_name: group.name.clone(),
                        headers_match: false, headers_tail_diff: false, headers_mismatch: false,
                        actions: vec![SyncAction::Import],
                    });
                }
            }
        }
    }
    rows
}

fn add_table_row(
    rows: &mut Vec<SyncRow>, group: &tablet_core::model::Group,
    table: &tablet_core::model::Table, xlsx_sheet: Option<&excel_sync::XlsxSheet>,
    xlsx_name: Option<&str>,
) {
    let mut row = SyncRow {
        group: group.name.clone(), node_name: table.name.clone(), checked: true,
        left_exists: true, left_diff: DiffCount::default(),
        right_exists: xlsx_sheet.is_some(), right_diff: DiffCount::default(),
        right_sheet_name: String::new(), right_blocks_reason: String::new(),
        right_xlsx_name: xlsx_name.unwrap_or("").into(),
        headers_match: false, headers_tail_diff: false, headers_mismatch: false,
        actions: vec![],
    };

    if let Some(sheet) = xlsx_sheet {
        row.right_sheet_name = sheet.name.clone();
        match excel_sync::classify_header(&sheet.headers, &table.schema.fields) {
            excel_sync::HeaderMatch::Identical => { row.headers_match = true; row.actions = vec![SyncAction::PushData, SyncAction::PullData]; }
            excel_sync::HeaderMatch::TailDiff => { row.headers_tail_diff = true; row.actions = vec![SyncAction::PushWithCols, SyncAction::PullWithCols]; }
            excel_sync::HeaderMatch::Mismatch => { row.headers_mismatch = true; row.actions = vec![SyncAction::ForcePush, SyncAction::ForcePull]; }
            excel_sync::HeaderMatch::Invalid => { row.right_blocks_reason = "表头不符合规范".into(); row.actions = vec![SyncAction::Blocked]; }
        }
        if row.actions != [SyncAction::Blocked] {
            let d = excel_sync::diff_rows(table.records.len(), sheet.rows.len(), &table.records, &sheet.rows);
            row.left_diff = d.clone();
            row.right_diff = d;
        }
    } else {
        row.actions.push(SyncAction::Create);
    }
    rows.push(row);
}

pub fn open(state: &Rc<RefCell<AppState>>) {
    state.borrow_mut().excel_sync_open = true;
}

pub fn close(state: &Rc<RefCell<AppState>>) {
    state.borrow_mut().excel_sync_open = false;
}

pub fn push(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    ui.set_dlg_excel_sync_open(st.excel_sync_open);
    if !st.excel_sync_open { return; }

    let rows = load_sync_data(&st);
    let es_rows: Vec<crate::EsRow> = rows.iter().map(|r| crate::EsRow {
        group: r.group.clone().into(),
        node: r.node_name.clone().into(),
        checked: r.checked,
        la: r.left_diff.added as i32,
        lr: r.left_diff.removed as i32,
        lm: r.left_diff.modified as i32,
        left_only: !r.right_exists && r.left_exists,
        ra: r.right_diff.added as i32,
        rr: r.right_diff.removed as i32,
        rm: r.right_diff.modified as i32,
        right_sheet: r.right_sheet_name.clone().into(),
        right_xlsx: r.right_xlsx_name.clone().into(),
        right_only: !r.left_exists && r.right_exists,
        blocked: !r.right_blocks_reason.is_empty(),
        blocked_reason: r.right_blocks_reason.clone().into(),
        hdr_match: r.headers_match,
        hdr_tail: r.headers_tail_diff,
        hdr_mismatch: r.headers_mismatch,
    }).collect();
    ui.set_es_rows(slint::ModelRc::new(VecModel::from(es_rows)));
}

pub fn wire(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let ui_h = ui.as_weak();
    ui.on_es_close(move || {
        if let Some(u) = ui_h.upgrade() { close(&s); push(&u, &s); }
    });

    let s = state.clone();
    let ui_h = ui.as_weak();
    ui.on_es_refresh(move || {
        if let Some(u) = ui_h.upgrade() { push(&u, &s); }
    });

    let s = state.clone();
    let ui_h = ui.as_weak();
    ui.on_es_action(move |row_idx, act| {
        let Some(u) = ui_h.upgrade() else { return };
        let result = execute_action(&s, row_idx, act);
        match &result {
            Ok(msg) => { s.borrow_mut().engine.ui_log(format!("[同步] {}", msg)); }
            Err(e) => { s.borrow_mut().engine.ui_log(format!("[同步失败] {}", e)); }
        }
        // 刷新主界面——同步可能修改了当前打开的表
        crate::refresh::after_grid_edit(&u, &s);
        push(&u, &s);
    });
}

fn execute_action(state: &Rc<RefCell<AppState>>, row_idx: i32, act: i32) -> Result<String, String> {
    let rows = {
        let st = state.borrow();
        load_sync_data(&st)
    };
    let row = rows.get(row_idx as usize).ok_or("行索引无效")?;
    let group_name = &row.group;
    let node_name = &row.node_name;

    let pid = {
        let st = state.borrow();
        st.engine.active_project_id().ok_or("无活动项目")?.to_string()
    };
    let excel_dir = {
        let st = state.borrow();
        let project = st.engine.find_project(&pid).ok_or("找不到项目")?;
        project.project_root.join(".excel")
    };
    let xlsx_path = excel_dir.join(format!("{}.xlsx", group_name));

    match act {
        // 1=PushData (data only), 3=PushWithCols, 7=Create (full overwrite)
        1 | 3 | 7 => {
            let mode = match act { 1 => excel_sync::SyncMode::DataOnly, 3 => excel_sync::SyncMode::WithColumns, _ => excel_sync::SyncMode::Full };
            std::fs::create_dir_all(&excel_dir).map_err(|e| format!("创建 .excel 目录失败: {}", e))?;
            let mut st = state.borrow_mut();
            let project = st.engine.find_project_mut(&pid).ok_or("找不到项目")?;
            let group = project.groups.iter()
                .find(|g| g.name == *group_name).ok_or("找不到组")?;
            excel_sync::sync_group_to_xlsx(&xlsx_path, group, mode)
                .map_err(|e| format!("写入 xlsx 失败: {}", e))?;
            let mode_label = match mode { excel_sync::SyncMode::DataOnly => "(仅数据)", excel_sync::SyncMode::WithColumns => "(含列)", _ => "" };
            Ok(format!("已同步 {} → {} {}", group_name, xlsx_path.display(), mode_label))
        }
        // 2=PullData or 8=Import: xlsx → tablet
        2 | 8 => {
            let mut st = state.borrow_mut();
            let patches = {
                let project = st.engine.find_project(&pid).ok_or("找不到项目")?;
                let group = project.groups.iter()
                    .find(|g| g.name == *group_name).ok_or("找不到组")?;
                excel_sync::read_group_from_xlsx(&xlsx_path, group)
                    .map_err(|e| format!("读取 xlsx 失败: {}", e))?
            };
            let project = st.engine.find_project_mut(&pid).ok_or("找不到项目")?;
            let group = project.groups.iter_mut()
                .find(|g| g.name == *group_name).ok_or("找不到组")?;
            for (tname, records) in &patches.tables {
                if let Some(table) = group.tables.iter_mut().find(|t| &t.name == tname) {
                    table.records = records.clone();
                    table.update_dirty();
                }
            }
            Ok(format!("已同步 {} ← {}", group_name, xlsx_path.display()))
        }
        _ => Err("该操作待实现".into()),
    }
}
