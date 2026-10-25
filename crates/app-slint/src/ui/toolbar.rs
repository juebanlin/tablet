// 顶部工具栏：撤销/重做 + 当前 Project 保存 / 重新加载。
// 导入 Schema → 项目右键「合并 Schema...」；模板库 / 新建项目 → 统一「新建项目」对话框。
// 数据导出 / Schema 导出 已迁到 TreeProject 右键菜单。
// 对应 docs/12-保存与连续撤销.md §4。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::{AppState, GridSelection, PendingAction, SelectedNode};
use crate::{dialogs, refresh, ui, AppWindow};

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let weak = ui_h.as_weak();
    ui_h.on_toolbar_btn_clicked(move |id| {
        let id = id.to_string();
        let mut full_refresh = false;
        match id.as_str() {
            "undo" => {
                if s.borrow_mut().engine.undo() {
                    repair_selection_after_history(&mut s.borrow_mut());
                    full_refresh = true;
                }
            }
            "redo" => {
                if s.borrow_mut().engine.redo() {
                    repair_selection_after_history(&mut s.borrow_mut());
                    full_refresh = true;
                }
            }
            "save" => {
                // 保存当前 active Project；成功后 engine 会清掉该项目的撤销/重做。
                let pid = s.borrow().engine.active_project_id().map(str::to_string);
                if let Some(pid) = pid {
                    s.borrow_mut().engine.save_project(&pid);
                } else {
                    s.borrow_mut().engine.ui_log("没有打开的 Project，无需保存".to_string());
                }
                // save_project 内部跑 revalidate_all_projects 重算 validation_errors，
                // 必须刷新 tree（! 标记）和 grid（红框）才能让用户看到错误
                full_refresh = true;
            }
            "reload" => {
                // 有未保存改动时先确认；确认后 engine.reload() 会丢弃改动并清空全部历史。
                if s.borrow().engine.is_dirty_any() {
                    s.borrow_mut().pending.open(PendingAction::ReloadWorkspace);
                    if let Some(ui_h) = weak.upgrade() {
                        dialogs::pending::push_confirm(&ui_h, &s);
                    }
                    return;
                }
                s.borrow_mut().engine.reload();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            _ => {}
        }
        if let Some(ui_h) = weak.upgrade() {
            if full_refresh {
                refresh::push_window_title(&ui_h, &s);
                ui::toolbar::push(&ui_h, &s);
                ui::tree::push(&ui_h, &s);
                ui::grid::push(&ui_h, &s);
                dialogs::type_selector::push(&ui_h, &s);
                dialogs::ref_picker::push(&ui_h, &s);
                dialogs::context_menu::push(&ui_h, &s);
                dialogs::pending::push_input(&ui_h, &s);
                dialogs::pending::push_confirm(&ui_h, &s);
            }
            // 任何 toolbar 操作都可能产生日志（save/reload/undo/redo 全会 log）
            refresh::after_log(&ui_h, &s);
        }
    });
}

/// 把 active Project 的撤销/重做状态和「当前项目」标签推给 Toolbar。
pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let (label, dirty) = match st.engine.active() {
        Some(p) => {
            let show_id = st
                .engine
                .global_config
                .ui
                .as_ref()
                .is_some_and(|u| u.show_meta_id);
            let name = if show_id {
                p.schema.meta.id.clone()
            } else if p.schema.meta.name.is_empty() {
                p.schema.meta.id.clone()
            } else {
                p.schema.meta.name.clone()
            };
            (format!("当前项目：{}", name), p.is_dirty())
        }
        None => ("当前项目：—".to_string(), false),
    };
    ui_h.set_active_project_label(label.into());
    ui_h.set_active_project_dirty(dirty);
    ui_h.set_save_enabled(st.engine.active().is_some());
    ui_h.set_undo_enabled(st.engine.can_undo());
    ui_h.set_redo_enabled(st.engine.can_redo());
}

/// 撤销/重做可能把当前选中的节点删掉（例如撤销粘贴、撤销新建）。
/// 这里把选中回退到仍然存在的 Group / Project，避免 grid 指向不存在的节点。
fn repair_selection_after_history(st: &mut AppState) {
    let Some(sel) = st.selected.clone() else { return };
    let pid = sel.project_id().to_string();
    let Some(project) = st.engine.find_project(&pid) else {
        st.selected = None;
        st.grid_selection = GridSelection::None;
        st.editing = None;
        st.editing_header_row = -1;
        st.editing_header_col = -1;
        return;
    };

    let (group, node_alive) = match &sel {
        SelectedNode::Project { .. } => (None, true),
        SelectedNode::Group { group, .. } => (
            Some(group.clone()),
            project.groups.iter().any(|g| g.name == *group),
        ),
        SelectedNode::Table { group, name, .. } => (
            Some(group.clone()),
            project
                .groups
                .iter()
                .find(|g| g.name == *group)
                .is_some_and(|g| g.tables.iter().any(|t| t.name == *name)),
        ),
        SelectedNode::Constant { group, name, .. } => (
            Some(group.clone()),
            project
                .groups
                .iter()
                .find(|g| g.name == *group)
                .is_some_and(|g| g.constants.iter().any(|c| c.name == *name)),
        ),
        SelectedNode::Enum { group, name, .. } => (
            Some(group.clone()),
            project
                .groups
                .iter()
                .find(|g| g.name == *group)
                .is_some_and(|g| g.enums.iter().any(|e| e.name == *name)),
        ),
    };
    if node_alive {
        return;
    }

    st.grid_selection = GridSelection::None;
    st.editing = None;
    st.editing_buffer.clear();
    st.editing_in_formula = false;
    st.editing_header_row = -1;
    st.editing_header_col = -1;
    st.selected = match group {
        Some(g) if project.groups.iter().any(|x| x.name == g) => {
            Some(SelectedNode::Group { project_id: pid, group: g })
        }
        _ => Some(SelectedNode::Project { project_id: pid }),
    };
}

/// reload / generate / clear 后清掉 UI 临时态：选中节点、grid 选区、编辑 buffer。
/// 同时重新展开 active project 下所有 group + active project 根；并跑一次全 Project 验证。
pub(crate) fn reset_view_after_reload(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    // 外部可能刚切过分支；重载时顺手重新探测一次（Git 是文件读，SVN 起一次 svn info）。
    st.vcs_info = tablet_core::vcs::probe(&st.engine.workdir);
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
    st.create_project.close();
    st.clone_project.close();
    if let Some(active_id) = st.engine.active_project_id().map(str::to_string) {
        st.tree_expanded = st.engine.project().groups.iter()
            .map(|g| (active_id.clone(), g.name.clone()))
            .collect();
        st.project_expanded = std::iter::once(active_id).collect();
    } else {
        st.tree_expanded.clear();
        st.project_expanded.clear();
    }
    // 跑一遍全 Project 验证，让所有节点的红框/`!` 标记可见。
    st.engine.revalidate_all_projects();
    let n_err = st.engine.validation_errors.len();
    let n_warn = st.engine.validation_warnings.len();
    if n_err > 0 || n_warn > 0 {
        st.engine.error_log(format!("[验证] 共 {} 个错误 / {} 条警告", n_err, n_warn));
    }
}
