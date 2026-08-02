//! Excel 同步面板 Rust 逻辑（Phase 2 骨架）。
//!
//! SyncNode 表示同步面板中的一行，包含两边节点的状态和可用的操作按钮。

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Model, VecModel};

use crate::state::AppState;
use crate::AppWindow;

/// 同步面板中一行的操作按钮类型。
#[derive(Clone, PartialEq)]
pub enum SyncAction {
    None,           // 无操作
    Synced,         // 已同步
    PushData,       // [→] 同名列数据覆写
    PullData,       // [←] 同名列数据覆写
    PushWithCols,   // [→含列] 含新增列
    PullWithCols,   // [←含列] 含新增列
    ForcePush,      // [强制→] 列映射
    ForcePull,      // [强制←] 列映射
    Create,         // [→创建] 新建 xlsx
    Import,         // [←导入] 导入到 tablet
    Blocked,        // ⛔ 不可操作
}

/// 差异计数。
#[derive(Default, Clone)]
pub struct DiffCount {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

/// 同步面板中的一行。
pub struct SyncRow {
    pub group: String,
    pub node_name: String,
    pub checked: bool,

    // 左侧 (tablet)
    pub left_exists: bool,
    pub left_diff: DiffCount,

    // 右侧 (xlsx)
    pub right_exists: bool,
    pub right_diff: DiffCount,
    pub right_sheet_name: String,
    pub right_blocks_reason: String, // 非空表示 ⛔
    pub right_xlsx_name: String,

    // 表头状态
    pub headers_match: bool,    // S1
    pub headers_tail_diff: bool, // S2: 末尾差异
    pub headers_mismatch: bool,  // S3: 不一致

    // 可用操作
    pub actions: Vec<SyncAction>,
}

/// 加载同步面板数据。
pub fn load_sync_data(state: &AppState) -> Vec<SyncRow> {
    let mut rows = Vec::new();
    if state.engine.active_project_id().is_none() { return rows; }

    let project = state.engine.project();

    // 扫描 .excel/ 下的所有 xlsx，建立 sheet→xlsx 的索引
    let excel_dir = project.project_root.join(".excel");
    // ... xlsx reading will be done in Phase 3 when umya-spreadsheet is added
    let _ = excel_dir;

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            rows.push(SyncRow {
                group: group.name.clone(),
                node_name: table.name.clone(),
                checked: true,
                left_exists: true,
                left_diff: DiffCount::default(),
                right_exists: false,
                right_diff: DiffCount::default(),
                right_sheet_name: String::new(),
                right_blocks_reason: String::new(),
                right_xlsx_name: String::new(),
                headers_match: false,
                headers_tail_diff: false,
                headers_mismatch: false,
                actions: vec![SyncAction::Create],
            });
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            rows.push(SyncRow {
                group: group.name.clone(),
                node_name: constant.name.clone(),
                checked: true,
                left_exists: true,
                left_diff: DiffCount::default(),
                right_exists: false,
                right_diff: DiffCount::default(),
                right_sheet_name: String::new(),
                right_blocks_reason: String::new(),
                right_xlsx_name: String::new(),
                headers_match: false,
                headers_tail_diff: false,
                headers_mismatch: false,
                actions: vec![SyncAction::Create],
            });
        }
        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            rows.push(SyncRow {
                group: group.name.clone(),
                node_name: enum_def.name.clone(),
                checked: true,
                left_exists: true,
                left_diff: DiffCount::default(),
                right_exists: false,
                right_diff: DiffCount::default(),
                right_sheet_name: String::new(),
                right_blocks_reason: String::new(),
                right_xlsx_name: String::new(),
                headers_match: false,
                headers_tail_diff: false,
                headers_mismatch: false,
                actions: vec![SyncAction::Create],
            });
        }
    }
    rows
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
}
