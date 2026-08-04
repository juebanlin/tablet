//! Excel 同步面板 Rust 逻辑。复用 tablet_core::excel_sync 的数据类型。

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, VecModel};

use tablet_core::excel_sync;

use crate::state::AppState;
use crate::AppWindow;

pub use excel_sync::DiffCount;

/// 同步面板中一行的操作按钮类型。
#[derive(Clone, PartialEq)]
pub enum SyncAction {
    PushData,       // [→]
    PullData,       // [←]
    PushWithCols,   // [补列→]
    ForcePush,      // [强制→]
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
    pub left_rows: usize,      // tablet 行数
    pub left_cols: usize,      // tablet 列数
    pub left_diff: DiffCount,
    pub right_exists: bool,
    pub right_rows: usize,     // xlsx 行数
    pub right_cols: usize,     // xlsx 列数
    pub right_diff: DiffCount,
    pub right_sheet_name: String,
    pub right_blocks_reason: String,
    pub right_xlsx_name: String,
    pub headers_match: bool,
    pub matched_cols: usize,    // 匹配的连续前缀列数
    pub tbl_more_cols: bool,
    pub xlsx_more_cols: bool,
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
                        left_exists: false, left_rows: 0, left_cols: 0, left_diff: DiffCount::default(),
                        right_exists: true, right_rows: 0, right_cols: 0, right_diff: DiffCount::default(),
                        right_sheet_name: sheet.name.clone(),
                        right_blocks_reason: String::new(),
                        right_xlsx_name: group.name.clone(),
                        headers_match: false, matched_cols: 0, tbl_more_cols: false, xlsx_more_cols: false, headers_mismatch: false,
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
        left_exists: true, left_rows: table.records.len(), left_cols: table.schema.fields.len(), left_diff: DiffCount::default(),
        right_exists: xlsx_sheet.is_some(), right_rows: 0, right_cols: 0, right_diff: DiffCount::default(),
        right_sheet_name: String::new(), right_blocks_reason: String::new(),
        right_xlsx_name: xlsx_name.unwrap_or("").into(),
        headers_match: false, matched_cols: 0, tbl_more_cols: false, xlsx_more_cols: false, headers_mismatch: false,
        actions: vec![],
    };

    if let Some(sheet) = xlsx_sheet {
        row.right_sheet_name = sheet.name.clone();
        row.right_rows = sheet.rows.len();
        row.right_cols = sheet.headers.len();
        let cm = excel_sync::compute_column_match(&table.schema.fields, &sheet.headers);
        row.matched_cols = cm.matched_prefix;
        let tbl_has_more = cm.tablet_only > 0;

        if cm.matched_prefix == 0 { // first col doesn't match → invalid
            row.right_blocks_reason = "表头不符合规范".into();
            row.actions = vec![SyncAction::Blocked];
        } else if cm.tablet_only == 0 && cm.xlsx_only == 0 {
            row.headers_match = true;
            row.actions = vec![SyncAction::PushData, SyncAction::PullData];
        } else if cm.tablet_only > 0 && cm.xlsx_only > 0 {
            // Both sides have extra columns → mismatch → force sync
            row.headers_mismatch = true;
            row.actions = vec![SyncAction::ForcePush];
        } else if tbl_has_more {
            row.tbl_more_cols = true;
            row.actions = vec![SyncAction::PushData, SyncAction::PullData, SyncAction::PushWithCols];
        } else { // xlsx_has_more — 策划公式列 or 删列残留，结构不同，仅强制同步
            row.xlsx_more_cols = true;
            row.actions = vec![SyncAction::ForcePush];
        }

        if !row.right_blocks_reason.is_empty() {
            // blocked — no diff
        } else {
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
        lrows: r.left_rows as i32, lcols: r.left_cols as i32,
        left_only: !r.right_exists && r.left_exists,
        rrows: r.right_rows as i32, rcols: r.right_cols as i32,
        right_sheet: r.right_sheet_name.clone().into(),
        right_xlsx: r.right_xlsx_name.clone().into(),
        right_only: !r.left_exists && r.right_exists,
        blocked: !r.right_blocks_reason.is_empty(),
        blocked_reason: r.right_blocks_reason.clone().into(),
        hdr_match: r.headers_match,
        matched: r.matched_cols as i32,
        tbl_more_cols: r.tbl_more_cols,
        xlsx_more_cols: r.xlsx_more_cols,
        hdr_mismatch: r.headers_mismatch,
        has_diff: r.left_diff.added > 0 || r.left_diff.modified > 0 || r.left_diff.removed > 0
            || r.right_diff.added > 0 || r.right_diff.modified > 0 || r.right_diff.removed > 0,
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

    // Guard: don't push/pull if there's no actual diff
    if matches!(act, 1 | 2) {
        let has_diff = row.left_diff.added > 0 || row.left_diff.modified > 0
            || row.left_diff.removed > 0 || row.right_diff.removed > 0;
        if !has_diff {
            return Err("数据已同步，无需操作".into());
        }
    }

    let group_name = &row.group;

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
        // 1=PushData 3=PushWithCols 5=ForcePush 7=Create: tablet → xlsx
        1 | 3 | 5 | 7 => {
            let mode = match act { 1 => excel_sync::SyncMode::DataOnly, 3 => excel_sync::SyncMode::WithColumns, _ => excel_sync::SyncMode::Full };
            let mode_label = match mode { excel_sync::SyncMode::DataOnly => "(仅数据)", excel_sync::SyncMode::WithColumns => "(含列)", excel_sync::SyncMode::Full => "(强制覆写)" };
            std::fs::create_dir_all(&excel_dir).map_err(|e| format!("创建 .excel 目录失败: {}", e))?;
            let mut st = state.borrow_mut();
            let project = st.engine.find_project_mut(&pid).ok_or("找不到项目")?;
            let group = project.groups.iter().find(|g| g.name == *group_name).ok_or("找不到组")?;
            excel_sync::sync_group_to_xlsx(&xlsx_path, group, mode).map_err(|e| format!("写入 xlsx 失败: {}", e))?;
            Ok(format!("已同步 {} → {} {}", group_name, xlsx_path.display(), mode_label))
        }
        // 2=PullData or 6=ForcePull or 8=Import: xlsx → tablet
        2 | 8 => {
            let mut st = state.borrow_mut();
            let patches = {
                let project = st.engine.find_project(&pid).ok_or("找不到项目")?;
                let group = project.groups.iter()
                    .find(|g| g.name == *group_name).ok_or("找不到组")?;
                excel_sync::read_group_from_xlsx(&xlsx_path, group, false)
                    .map_err(|e| format!("读取 xlsx 失败: {}", e))?
            };
            let project = st.engine.find_project_mut(&pid).ok_or("找不到项目")?;
            let group = project.groups.iter_mut()
                .find(|g| g.name == *group_name).ok_or("找不到组")?;
            // Merge: only overwrite columns that xlsx has. Tablet-only columns are preserved.
            for (tname, xlsx_records) in &patches.tables {
                if let Some(table) = group.tables.iter_mut().find(|t| &t.name == tname) {
                    for (ri, xlsx_row) in xlsx_records.iter().enumerate() {
                        while table.records.len() <= ri { table.records.push(vec![String::new(); table.schema.fields.len()]); }
                        for (ci, val) in xlsx_row.iter().enumerate() {
                            if ci < table.schema.fields.len() && !val.is_empty() {
                                while table.records[ri].len() <= ci { table.records[ri].push(String::new()); }
                                table.records[ri][ci] = val.clone();
                            }
                        }
                    }
                    // Sync completes → update snapshot so dirty flag reflects reality
                    table.original_records = table.records.clone();
                    table.original_fields = table.schema.fields.clone();
                    table.dirty = false;
                }
            }
            Ok(format!("已同步 {} ← {}", group_name, xlsx_path.display()))
        }
        _ => Err("该操作待实现".into()),
    }
}
