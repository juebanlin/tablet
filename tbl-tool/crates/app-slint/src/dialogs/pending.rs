// PendingAction 闭环：input 对话框 + confirm 对话框 + revalidate + execute + tree.proj-* 分发。
//
// PendingAction 是 ctx menu / tree button 触发的"待执行操作"载体；
// 视输入需求走 input dialog 或直接 confirm dialog，最后由 execute() 落地到 ProjectAction。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::{
    AppState, GridSelection, PendingAction, RenameProjectStage, SelectedNode,
};
use crate::{ui, AppWindow};

pub fn push_input(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    match &st.pending.action {
        Some(action) if action.needs_input() => {
            ui_h.set_dlg_input_open(true);
            ui_h.set_dlg_input_title(action.input_title().into());
            ui_h.set_dlg_input_label("名称:".into());
            ui_h.set_dlg_input_buffer(st.pending.input_buffer.clone().into());
            ui_h.set_dlg_input_error(st.pending.error.clone().unwrap_or_default().into());
            let can_confirm = st.pending.error.is_none() && !st.pending.input_buffer.is_empty();
            ui_h.set_dlg_input_can_confirm(can_confirm);
        }
        _ => {
            ui_h.set_dlg_input_open(false);
            ui_h.set_dlg_input_buffer(slint::SharedString::new());
            ui_h.set_dlg_input_error(slint::SharedString::new());
            ui_h.set_dlg_input_can_confirm(false);
        }
    }
}

pub fn push_confirm(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    match &st.pending.action {
        Some(action) if action.needs_confirm() => {
            ui_h.set_dlg_confirm_open(true);
            ui_h.set_dlg_confirm_title(action.confirm_title().into());
            ui_h.set_dlg_confirm_message(action.confirm_message().into());
        }
        _ => {
            ui_h.set_dlg_confirm_open(false);
        }
    }
}

/// 根据 PendingAction 当前 input_buffer，刷新 error 字段（命名校验）。
pub(crate) fn revalidate_input(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    let action = match &st.pending.action { Some(a) => a.clone(), None => return };
    let buf = st.pending.input_buffer.clone();
    let err: Option<String> = match &action {
        PendingAction::NewGroup { .. } => st.engine.validate_group_name(&buf),
        PendingAction::RenameGroup { old_name, .. } => st.engine.validate_group_name_rename(&buf, old_name),
        PendingAction::RenameNode { old_name, .. } => st.engine.validate_node_name_rename(&buf, old_name),
        PendingAction::NewTable { .. } | PendingAction::NewConstant { .. } | PendingAction::NewEnum { .. } =>
            st.engine.validate_node_name(&buf),
        _ => None,
    };
    st.pending.error = err;
}

pub(crate) fn execute(state: &Rc<RefCell<AppState>>) {
    use tbl_core::ops::ProjectAction;
    let mut st = state.borrow_mut();
    let action = match st.pending.action.clone() { Some(a) => a, None => return };
    let buf = st.pending.input_buffer.clone();
    let core_action = match &action {
        PendingAction::NewGroup { project_id } =>
            ProjectAction::NewGroup { project_id: project_id.clone(), name: buf.clone() },
        PendingAction::NewTable { project_id, group } =>
            ProjectAction::NewTable { project_id: project_id.clone(), group: group.clone(), name: buf.clone() },
        PendingAction::NewConstant { project_id, group } =>
            ProjectAction::NewConstant { project_id: project_id.clone(), group: group.clone(), name: buf.clone() },
        PendingAction::NewEnum { project_id, group } =>
            ProjectAction::NewEnum { project_id: project_id.clone(), group: group.clone(), name: buf.clone() },
        PendingAction::RenameGroup { project_id, old_name } =>
            ProjectAction::RenameGroup { project_id: project_id.clone(), old_name: old_name.clone(), new_name: buf.clone() },
        PendingAction::RenameNode { project_id, group, old_name } =>
            ProjectAction::RenameNode { project_id: project_id.clone(), group: group.clone(), old_name: old_name.clone(), new_name: buf.clone() },
        PendingAction::DeleteGroup { project_id: _, group } => {
            st.engine.delete_group(group);
            if let Some(SelectedNode::Table { group: g, .. }
                | SelectedNode::Constant { group: g, .. }
                | SelectedNode::Enum { group: g, .. }) = &st.selected
            {
                if g == group { st.selected = None; st.grid_selection = GridSelection::None; }
            }
            st.pending.close();
            return;
        }
        PendingAction::DeleteNode { project_id: _, group, name } => {
            st.engine.delete_node(group, name);
            if let Some(SelectedNode::Table { group: g, name: n, .. }
                | SelectedNode::Constant { group: g, name: n, .. }
                | SelectedNode::Enum { group: g, name: n, .. }) = &st.selected
            {
                if g == group && n == name { st.selected = None; st.grid_selection = GridSelection::None; }
            }
            st.pending.close();
            return;
        }
        PendingAction::RenameProject { old_id, stage } => match stage {
            RenameProjectStage::EnterId => {
                // 第一步：收新 id；不真正落地，跳到第二步
                let new_id = buf.clone();
                st.pending.action = Some(PendingAction::RenameProject {
                    old_id: old_id.clone(),
                    stage: RenameProjectStage::EnterName { new_id },
                });
                st.pending.input_buffer.clear();
                st.pending.error = None;
                return;
            }
            RenameProjectStage::EnterName { new_id } => {
                ProjectAction::RenameProject {
                    old_id: old_id.clone(),
                    new_id: new_id.clone(),
                    new_name: buf.clone(),
                }
            }
        },
        PendingAction::DeleteProject { project_id } => {
            ProjectAction::DeleteProject { project_id: project_id.clone() }
        }
        PendingAction::CloseDirtyProject { project_id } => {
            // 用户已确认放弃未保存改动 → 直接 close。
            // drop 当前 borrow 让 close_project_with_persist 重新 borrow。
            let pid = project_id.clone();
            st.pending.close();
            drop(st);
            ui::tree::close_project_with_persist(state, &pid);
            return;
        }
    };
    if let PendingAction::NewGroup { project_id } = &action {
        st.tree_expanded.insert((project_id.clone(), buf.clone()));
    }
    // RenameProject 可能改 id —— 记下 active 在 execute 之前
    let old_active = st.engine.active_project_id().map(str::to_string);
    let track_rename = matches!(action, PendingAction::RenameProject { .. });
    let track_delete = matches!(action, PendingAction::DeleteProject { .. });
    st.engine.execute_action(&core_action);
    if track_rename {
        if let ProjectAction::RenameProject { old_id, new_id, .. } = &core_action {
            if old_id != new_id {
                if matches!(&st.selected, Some(s) if s.project_id() == old_id) {
                    if let Some(sel) = st.selected.as_mut() {
                        match sel {
                            SelectedNode::Project { project_id }
                            | SelectedNode::Group { project_id, .. }
                            | SelectedNode::Table { project_id, .. }
                            | SelectedNode::Constant { project_id, .. }
                            | SelectedNode::Enum { project_id, .. } => *project_id = new_id.clone(),
                        }
                    }
                }
                let migrated_groups: Vec<String> = st.tree_expanded.iter()
                    .filter(|(p, _)| p == old_id)
                    .map(|(_, g)| g.clone())
                    .collect();
                st.tree_expanded.retain(|(p, _)| p != old_id);
                for g in migrated_groups {
                    st.tree_expanded.insert((new_id.clone(), g));
                }
                if st.project_expanded.remove(old_id) {
                    st.project_expanded.insert(new_id.clone());
                }
                if old_active.as_deref() == Some(old_id.as_str()) {
                    let _ = st.engine.set_active_by_id(new_id);
                }
            }
            ui::tree::persist_workspace(&mut *st);
        }
    } else if track_delete {
        if let ProjectAction::DeleteProject { project_id } = &core_action {
            if matches!(&st.selected, Some(s) if s.project_id() == project_id) {
                st.selected = None;
                st.grid_selection = GridSelection::None;
                st.editing = None;
            }
            st.tree_expanded.retain(|(p, _)| p != project_id);
            st.project_expanded.remove(project_id);
            ui::tree::persist_workspace(&mut *st);
        }
    }
    st.pending.close();
}

/// 树面板 Project 根右键菜单分发：保存 / 导出 / 新建 Group / 克隆 / 打开 / 关闭 / 重命名 / 删除 / 在文件管理器打开。
pub(crate) fn handle_project_root_action(state: &Rc<RefCell<AppState>>, project_id: &str, action: &str) {
    match action {
        "tree.proj-save" => {
            state.borrow_mut().engine.save_project(project_id);
        }
        "tree.proj-export" => {
            // 数据导出对话框走 active project；切到当前选中 project 再开
            let mut st = state.borrow_mut();
            st.engine.set_active_by_id(project_id);
            st.data_export.open = true;
        }
        "tree.proj-export-schema" => {
            // Schema 导出对话框同样走 active project
            let mut st = state.borrow_mut();
            st.engine.set_active_by_id(project_id);
            st.schema_export.open = true;
            crate::dialogs::schema_io::rebuild_export_items(&mut st);
        }
        "tree.proj-new-group" => {
            state.borrow_mut().pending.open(PendingAction::NewGroup {
                project_id: project_id.to_string(),
            });
        }
        "tree.proj-clone" => {
            let (display, category, version) = {
                let st = state.borrow();
                st.engine.find_project(project_id)
                    .map(|p| (
                        p.schema.meta.name.clone(),
                        p.schema.meta.category.clone(),
                        p.schema.meta.version.clone(),
                    ))
                    .unwrap_or_else(|| (project_id.to_string(), String::new(), String::new()))
            };
            state.borrow_mut().new_project.open_clone(project_id, &display, &category, &version);
        }
        "tree.proj-open" => {
            // 右键 closed project → 打开 + 设 active + 默认展开
            if ui::tree::open_project_with_persist(state, project_id) {
                let mut st = state.borrow_mut();
                st.engine.set_active_by_id(project_id);
                st.selected = Some(SelectedNode::Project { project_id: project_id.to_string() });
                st.project_expanded.insert(project_id.to_string());
                let groups: Vec<String> = st.engine.find_project(project_id)
                    .map(|p| p.groups.iter().map(|g| g.name.clone()).collect())
                    .unwrap_or_default();
                for g in groups {
                    st.tree_expanded.insert((project_id.to_string(), g));
                }
            }
        }
        "tree.proj-close" => {
            // 有未保存改动 → 弹 ConfirmDialog 二次确认；干净状态直接 close。
            let dirty = state.borrow().engine.is_project_dirty(project_id);
            if dirty {
                state.borrow_mut().pending.open(PendingAction::CloseDirtyProject {
                    project_id: project_id.to_string(),
                });
            } else {
                ui::tree::close_project_with_persist(state, project_id);
            }
        }
        "tree.proj-rename" => {
            // 重命名 closed project：先打开（重命名流程操作的是已打开 project 的 root + 文件）
            let need_open = !state.borrow().engine.is_opened(project_id);
            if need_open {
                ui::tree::open_project_with_persist(state, project_id);
                state.borrow_mut().engine.set_active_by_id(project_id);
                state.borrow_mut().project_expanded.insert(project_id.to_string());
            }
            state.borrow_mut().pending.open(PendingAction::RenameProject {
                old_id: project_id.to_string(),
                stage: RenameProjectStage::EnterId,
            });
        }
        "tree.proj-delete" => {
            state.borrow_mut().pending.open(PendingAction::DeleteProject {
                project_id: project_id.to_string(),
            });
        }
        "tree.proj-open-dir" => {
            let st = state.borrow();
            if let Some(p) = st.engine.find_project(project_id) {
                let _ = open::that(&p.project_root);
            } else if let Some(ap) = st.engine.available().iter().find(|a| a.id == project_id) {
                let _ = open::that(&ap.root);
            }
        }
        "tree.paste-group" => {
            let new_group = state.borrow_mut().engine.paste_group_to(project_id);
            if let Some(new_group) = new_group {
                let mut st = state.borrow_mut();
                st.project_expanded.insert(project_id.to_string());
                st.tree_expanded.insert((project_id.to_string(), new_group));
            }
        }
        _ => {}
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    use crate::refresh;

    // 输入对话框：set-input / confirm / cancel
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_inp_set_input(move |t| {
            s.borrow_mut().pending.input_buffer = t.to_string();
            revalidate_input(&s);
            if let Some(ui_h) = weak.upgrade() { push_input(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_inp_confirm(move || {
            revalidate_input(&s);
            let ok = {
                let st = s.borrow();
                st.pending.error.is_none() && !st.pending.input_buffer.is_empty()
            };
            if !ok {
                if let Some(ui_h) = weak.upgrade() { push_input(&ui_h, &s); }
                return;
            }
            execute(&s);
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_pending_action(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_inp_cancel(move || {
            s.borrow_mut().pending.close();
            if let Some(ui_h) = weak.upgrade() { push_input(&ui_h, &s); }
        });
    }
    // 确认对话框：confirm / cancel
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cf_confirm(move || {
            execute(&s);
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_pending_action(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cf_cancel(move || {
            s.borrow_mut().pending.close();
            if let Some(ui_h) = weak.upgrade() { push_confirm(&ui_h, &s); }
        });
    }
}
