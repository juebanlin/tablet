//! Excel 同步面板 Rust 逻辑。
//!
//! 这里**不做任何决策**——「这一行能做哪些操作」全部来自
//! `tablet_core::excel_sync::plan_group`。面板只把 `PairPlan` 翻译成渲染用的
//! `EsRow`，点击时把操作原样交回 `excel_sync::execute`。
//!
//! 以前不是这样：按钮的可见性写在 `.slint` 的谓词里、行为写在 `execute_action` 的
//! `match act` 里，两套规则手工对齐；而 Rust 侧算好的那份分类是**没人读的死数据**。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use slint::{ComponentHandle, VecModel};

use tablet_core::excel_sync::{self, PushScope, SyncOp};

use crate::state::AppState;
use crate::AppWindow;

/// 面板一行：一份 `PairPlan` 加上它属于哪个 group。
struct PanelRow {
    group: String,
    pair: excel_sync::PairPlan,
    /// 表头之外还有内容（区域界定表达不了的那部分）。只在日志里说一句。
    content_outside_region: bool,
}

/// 操作码 ↔ [`SyncOp`] 的**双射**。
///
/// 面板回传的是操作本身，不是它在列表里的下标——传下标等于把「按行号回查」原样
/// 搬到 ops 上：文件一变，下标就指向另一个操作。
fn op_code(op: &SyncOp) -> i32 {
    match op {
        SyncOp::Push { scope: PushScope::MatchedOnly } => 1,
        SyncOp::Pull => 2,
        SyncOp::Push { scope: PushScope::WithTabletColumns } => 3,
        SyncOp::Push { scope: PushScope::WholeTable } => 5,
        SyncOp::CreateFile => 7,
        SyncOp::ImportSheet => 8,
    }
}

fn op_from_code(code: i32) -> Option<SyncOp> {
    Some(match code {
        1 => SyncOp::Push { scope: PushScope::MatchedOnly },
        2 => SyncOp::Pull,
        3 => SyncOp::Push { scope: PushScope::WithTabletColumns },
        5 => SyncOp::Push { scope: PushScope::WholeTable },
        7 => SyncOp::CreateFile,
        8 => SyncOp::ImportSheet,
        _ => return None,
    })
}

/// 算出面板要显示的每一行。决策全部发生在 core 的 `plan_group` 里。
fn load_panel_rows(state: &AppState) -> Vec<PanelRow> {
    let mut rows = Vec::new();
    let Some(pid) = state.engine.active_project_id() else { return rows; };
    let Some(project) = state.engine.find_project(pid) else { return rows; };

    let excel_dir = project.project_root.join(".excel");
    // `.excel/<group>.xlsx` 一份对一个 group，键取文件名主干
    let mut books: HashMap<String, PathBuf> = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&excel_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "xlsx") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    books.insert(stem.to_string(), p.clone());
                }
            }
        }
    }

    for group in &project.groups {
        let sheets = books
            .get(&group.name)
            .and_then(|p| excel_sync::read_xlsx_sheets(p, Some(group)).ok());
        let plan = excel_sync::plan_group(group, sheets.as_deref());
        let outside: Vec<String> = sheets
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|s| s.has_content_outside_region())
            .map(|s| s.name.clone())
            .collect();

        for pair in plan.pairs {
            rows.push(PanelRow {
                group: group.name.clone(),
                content_outside_region: outside.iter().any(|n| *n == pair.node),
                pair,
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

/// 刷新面板。
///
/// `log_notes` 只在用户主动刷新 / 刚打开时为真：越权改动与区域外内容值得吭一声，
/// 但每做一次同步都重报一遍就成噪音了。
pub fn push(ui: &AppWindow, state: &Rc<RefCell<AppState>>, log_notes: bool) {
    let mut notes: Vec<String> = Vec::new();

    let es_rows: Vec<crate::EsRow> = {
        let st = state.borrow();
        ui.set_dlg_excel_sync_open(st.excel_sync_open);
        if !st.excel_sync_open { return; }

        let rows = load_panel_rows(&st);

        if log_notes {
            for r in &rows {
                // type / export 是策划不该动的两行；改了要吭声，不能静默忽略
                if r.pair.header_edited {
                    notes.push(format!(
                        "[同步] {} / {}：type 或 export 行被改过，已忽略（只有 desc 可改）",
                        r.group, r.pair.node
                    ));
                }
                // 区域外内容不是错误、也不需要人决策，同步完全不碰它——日志一句即可
                if r.content_outside_region {
                    notes.push(format!(
                        "[同步] {} / {}：表头之外还有内容，不在同步范围内（已原样保留）",
                        r.group, r.pair.node
                    ));
                }
            }
        }

        rows.iter().map(|r| {
            let p = &r.pair;
            let ops: Vec<crate::EsOp> = p.ops.iter().map(|op| crate::EsOp {
                code: op_code(op),
                label: op.label().into(),
            }).collect();
            crate::EsRow {
                group: r.group.clone().into(),
                node: p.node.clone().into(),
                lrows: p.left.as_ref().map_or(0, |s| s.rows) as i32,
                lcols: p.left.as_ref().map_or(0, |s| s.cols) as i32,
                left_only: p.left.is_some() && p.right.is_none(),
                rrows: p.right.as_ref().map_or(0, |s| s.rows) as i32,
                rcols: p.right.as_ref().map_or(0, |s| s.cols) as i32,
                right_only: p.right.is_some() && p.left.is_none(),
                blocked: p.blocked_reason.is_some(),
                blocked_reason: p.blocked_reason.clone().unwrap_or_default().into(),
                desc_diff: p.desc_diff,
                ops: slint::ModelRc::new(VecModel::from(ops)),
            }
        }).collect()
    };

    // 日志要 &mut，借不到就先把话攒下来，出了作用域再写
    if !notes.is_empty() {
        let mut st = state.borrow_mut();
        for n in notes { st.engine.ui_log(n); }
    }

    ui.set_es_rows(slint::ModelRc::new(VecModel::from(es_rows)));
}

pub fn wire(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let ui_h = ui.as_weak();
    ui.on_es_close(move || {
        if let Some(u) = ui_h.upgrade() { close(&s); push(&u, &s, false); }
    });

    let s = state.clone();
    let ui_h = ui.as_weak();
    ui.on_es_refresh(move || {
        if let Some(u) = ui_h.upgrade() { push(&u, &s, true); }
    });

    let s = state.clone();
    let ui_h = ui.as_weak();
    ui.on_es_action(move |group, node, code| {
        let Some(u) = ui_h.upgrade() else { return };
        match execute_action(&s, &group, &node, code) {
            Ok(msg) => s.borrow_mut().engine.ui_log(format!("[同步] {}", msg)),
            Err(e) => s.borrow_mut().engine.ui_log(format!("[同步失败] {}", e)),
        }
        // 刷新主界面——同步可能修改了当前打开的表
        crate::refresh::after_grid_edit(&u, &s);
        push(&u, &s, false);
    });
}

fn execute_action(
    state: &Rc<RefCell<AppState>>,
    group_name: &str,
    node: &str,
    code: i32,
) -> Result<String, String> {
    let op = op_from_code(code).ok_or("未知操作")?;

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

    std::fs::create_dir_all(&excel_dir).map_err(|e| format!("创建 .excel 目录失败: {}", e))?;

    let mut st = state.borrow_mut();
    let project = st.engine.find_project_mut(&pid).ok_or("找不到项目")?;
    let group = project.groups.iter_mut()
        .find(|g| g.name == group_name).ok_or("找不到组")?;

    // 校验（计划是否过期）与落盘都在 core 里——dirty 的处理漏在 UI 层正是上次
    // 「回读的数据写不进 .tbl」那个 bug 的成因，别再漏出去。
    excel_sync::execute(&xlsx_path, group, node, &op).map_err(|e| e.to_string())
}
