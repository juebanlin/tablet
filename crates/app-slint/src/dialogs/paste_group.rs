//! 「粘贴 Group」命名确认对话框。
//!
//! 只有项目/组内出现同名节点（或同名组）时才打开；默认名字来自
//! `ProjectEngine::preview_paste_group()` 的 `_copy` 预填。确认时一次性提交：
//! 组名 + 每个子项名 + 组内引用改写，全部属于同一个事务。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::state::{AppState, PasteGroupRowState};
use crate::{refresh, AppWindow, PasteGroupRow};

/// 只读检查：需要弹命名确认时把状态填好并返回 true。
/// 无冲突 / 剪贴板不是 Group 时返回 false，调用方走原来的直接粘贴。
pub fn open_if_conflict(
    state: &Rc<RefCell<AppState>>,
    target_project: &str,
    storage: tablet_core::model::GroupStorage,
) -> bool {
    let preview = {
        let st = state.borrow();
        st.engine.preview_paste_group(target_project)
    };
    let Some(preview) = preview else { return false };
    if !preview.has_conflict {
        return false;
    }

    let mut st = state.borrow_mut();
    st.paste_group.open = true;
    st.paste_group.target_project = target_project.to_string();
    st.paste_group.storage = storage;
    st.paste_group.group_name = preview.group_name;
    st.paste_group.error.clear();
    st.paste_group.rows = preview
        .nodes
        .into_iter()
        .map(|n| PasteGroupRowState {
            kind: match n.kind {
                tablet_core::model::NodeKind::Table => "Table",
                tablet_core::model::NodeKind::Constant => "Constant",
                tablet_core::model::NodeKind::Enum => "Enum",
            }
            .to_string(),
            old_name: n.old_name,
            new_name: n.new_name,
        })
        .collect();
    true
}

/// 冲突检查 + 无冲突直接粘贴的快捷入口。成功粘贴返回新组名。
/// 打开对话框时返回 None；调用方的 refresh 会负责 push 对话框状态。
pub fn request(
    state: &Rc<RefCell<AppState>>,
    target_project: &str,
    storage: tablet_core::model::GroupStorage,
) -> Option<String> {
    if open_if_conflict(state, target_project, storage) {
        return None;
    }
    state
        .borrow_mut()
        .engine
        .paste_group_as(target_project, Some(storage))
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    ui_h.set_dlg_paste_group_open(st.paste_group.open);
    if !st.paste_group.open {
        return;
    }
    ui_h.set_pg_target_project(st.paste_group.target_project.clone().into());
    ui_h.set_pg_group_name(st.paste_group.group_name.clone().into());
    ui_h.set_pg_error_text(st.paste_group.error.clone().into());
    let rows: Vec<PasteGroupRow> = st
        .paste_group
        .rows
        .iter()
        .map(|r| PasteGroupRow {
            kind: r.kind.clone().into(),
            old_name: r.old_name.clone().into(),
            new_name: r.new_name.clone().into(),
        })
        .collect();
    ui_h.set_pg_rows(ModelRc::new(VecModel::from(rows)));
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        ui_h.on_pg_set_group_name(move |name| {
            s.borrow_mut().paste_group.group_name = name.to_string();
        });
    }
    {
        let s = state.clone();
        ui_h.on_pg_set_node_name(move |index, name| {
            let mut st = s.borrow_mut();
            if let Some(row) = st.paste_group.rows.get_mut(index.max(0) as usize) {
                row.new_name = name.to_string();
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_pg_cancel(move || {
            s.borrow_mut().paste_group.close();
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_pg_confirm(move || {
            let (target, storage, group, rows) = {
                let st = s.borrow();
                let pg = &st.paste_group;
                (
                    pg.target_project.clone(),
                    pg.storage,
                    pg.group_name.clone(),
                    pg.rows.clone(),
                )
            };
            let mut names = BTreeMap::new();
            for row in rows {
                names.insert(row.old_name, row.new_name);
            }
            let result = s.borrow_mut().engine.paste_group_as_with_names(
                &target,
                Some(storage),
                &group,
                &names,
            );
            match result {
                Ok(new_group) => {
                    {
                        let mut st = s.borrow_mut();
                        st.paste_group.close();
                        st.project_expanded.insert(target.clone());
                        st.tree_expanded.insert((target.clone(), new_group.clone()));
                        st.select_pasted_group(&target, &new_group);
                    }
                    if let Some(ui_h) = weak.upgrade() {
                        refresh::after_tree_change(&ui_h, &s);
                    }
                }
                Err(e) => {
                    s.borrow_mut().paste_group.error = e;
                    if let Some(ui_h) = weak.upgrade() {
                        push(&ui_h, &s);
                    }
                }
            }
        });
    }
}
