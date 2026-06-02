// 右键菜单：列出菜单项 + 处理 dispatch（按 kind+action_id 分发）。
//
// kind 来源：tree 节点 / tree 空白 / grid 列字母 / grid 行号 / grid 数据格。
// action_id 形如 "tree.new-group" / "grid.col-insert-left"。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::{
    AppState, CtxMenuKind, GridSelection, PendingAction,
};
use crate::{dialogs, refresh, ui, AppWindow, CtxMenuItem};

/// 计算当前 ctx_menu.kind 应展示的菜单项列表。
pub(crate) fn items_for(kind: &CtxMenuKind, state: &AppState) -> Vec<CtxMenuItem> {
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
    let item_owned = |label: String, id: &str, disabled: bool| CtxMenuItem {
        label: label.into(),
        action_id: id.into(),
        is_separator: false,
        disabled,
    };
    let cb = state.engine.node_clipboard.as_ref();
    let cb_label = cb.map(tbl_core::ops::NodeClipboard::label).unwrap_or_default();
    let has_node_cb = cb.map_or(false, tbl_core::ops::NodeClipboard::is_node);
    let has_group_cb = cb.map_or(false, tbl_core::ops::NodeClipboard::is_group);
    let paste_node_label = if has_node_cb {
        format!("粘贴节点（{}）", cb_label)
    } else {
        "粘贴节点".to_string()
    };
    let paste_group_label = if has_group_cb {
        format!("粘贴 Group（{}）", cb_label)
    } else {
        "粘贴 Group".to_string()
    };
    match kind {
        CtxMenuKind::TreeBlank => vec![
            item("新建 Group", "tree.new-group", false),
        ],
        CtxMenuKind::TreeProject { project_id } => {
            if state.engine.is_opened(project_id) {
                vec![
                    item("保存", "tree.proj-save", false),
                    item("导出...", "tree.proj-export", false),
                    item("导出 Schema...", "tree.proj-export-schema", false),
                    item("合并 Schema...", "tree.proj-merge-schema", false),
                    sep(),
                    item("新建 Group", "tree.proj-new-group", false),
                    item("复制(克隆)...", "tree.proj-clone", false),
                    item_owned(paste_group_label.clone(), "tree.paste-group", !has_group_cb),
                    item("重命名...", "tree.proj-rename", false),
                    item("删除", "tree.proj-delete", false),
                    sep(),
                    item("关闭", "tree.proj-close", false),
                    sep(),
                    item("在文件管理器打开", "tree.proj-open-dir", false),
                ]
            } else {
                vec![
                    item("打开", "tree.proj-open", false),
                    sep(),
                    item("在文件管理器打开", "tree.proj-open-dir", false),
                    item("重命名...", "tree.proj-rename", false),
                    item("删除", "tree.proj-delete", false),
                ]
            }
        }
        CtxMenuKind::TreeGroup { .. } => vec![
            item("新建 Table", "tree.new-table", false),
            item("新建 Constant", "tree.new-constant", false),
            item("新建 Enum", "tree.new-enum", false),
            sep(),
            item("复制 Group（含全部内容）", "tree.copy-group", false),
            item_owned(paste_node_label.clone(), "tree.paste-node", !has_node_cb),
            item_owned(paste_group_label, "tree.paste-group", !has_group_cb),
            sep(),
            item("重命名", "tree.rename-group", false),
            item("删除", "tree.delete-group", false),
        ],
        CtxMenuKind::TreeNode { .. } => vec![
            item("复制", "tree.copy-node", false),
            item_owned(paste_node_label, "tree.paste-node", !has_node_cb),
            sep(),
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
        CtxMenuKind::GridCell { row, col } => {
            let mut items = Vec::new();
            // picker 类首项：差异化文案；多选状态下不显示首项（避免误以为支持批量）
            let single_cell = matches!(state.grid_selection, GridSelection::Cell(_, _));
            if single_cell {
                let kind = ui::grid_actions::effective_column_kind_at(state, *row, *col);
                if let Some(label) = kind.as_ref().and_then(|k| k.picker_action_label()) {
                    items.push(item(label, "grid.cell-pick", false));
                    items.push(sep());
                }
            }
            items.extend([
                item("复制", "grid.cell-copy", false),
                item("剪切", "grid.cell-cut", false),
                item("粘贴", "grid.cell-paste", false),
                item("删除内容", "grid.cell-clear", false),
            ]);
            items
        }
    }
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let cm = &st.ctx_menu;
    ui_h.set_ctx_menu_open(cm.open);
    if !cm.open {
        ui_h.set_ctx_menu_items(slint::ModelRc::new(slint::VecModel::from(Vec::<CtxMenuItem>::new())));
        return;
    }
    let kind = match &cm.kind { Some(k) => k, None => return };
    ui_h.set_ctx_menu_x(cm.x);
    ui_h.set_ctx_menu_y(cm.y);
    ui_h.set_ctx_menu_items(slint::ModelRc::new(slint::VecModel::from(items_for(kind, &st))));
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ctx_menu_dismiss(move || {
            s.borrow_mut().ctx_menu.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // ctx-menu-action(action_id)：根据当前 ctx_menu.kind + id 分发
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ctx_menu_action(move |id| {
            let id = id.to_string();
            // 取走当前 kind 后立即关闭菜单
            let kind = {
                let mut st = s.borrow_mut();
                let k = st.ctx_menu.kind.clone();
                st.ctx_menu.close();
                k
            };
            match (kind, id.as_str()) {
                (Some(CtxMenuKind::TreeBlank), "tree.new-group") => {
                    let project_id = {
                        let st = s.borrow();
                        st.selected.as_ref().map(|sn| sn.project_id().to_string())
                            .or_else(|| st.engine.projects.first().map(|p| p.schema.meta.id.clone()))
                            .unwrap_or_default()
                    };
                    s.borrow_mut().pending.open(PendingAction::NewGroup { project_id });
                }
                (Some(CtxMenuKind::TreeProject { project_id }), action) => {
                    dialogs::pending::handle_project_root_action(&s, &project_id, action);
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.new-table") => {
                    s.borrow_mut().pending.open(PendingAction::NewTable { project_id, group: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.new-constant") => {
                    s.borrow_mut().pending.open(PendingAction::NewConstant { project_id, group: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.new-enum") => {
                    s.borrow_mut().pending.open(PendingAction::NewEnum { project_id, group: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.rename-group") => {
                    let mut st = s.borrow_mut();
                    st.pending.open(PendingAction::RenameGroup { project_id, old_name: name.clone() });
                    st.pending.input_buffer = name;
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.delete-group") => {
                    s.borrow_mut().pending.open(PendingAction::DeleteGroup { project_id, group: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.copy-group") => {
                    s.borrow_mut().engine.clipboard_copy_group(&project_id, &name);
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.paste-node") => {
                    let mut st = s.borrow_mut();
                    if st.engine.paste_node_to(&project_id, &name).is_some() {
                        st.tree_expanded.insert((project_id, name));
                    }
                }
                (Some(CtxMenuKind::TreeGroup { project_id, .. }), "tree.paste-group") => {
                    let new_group = s.borrow_mut().engine.paste_group_to(&project_id);
                    if let Some(new_group) = new_group {
                        let mut st = s.borrow_mut();
                        st.project_expanded.insert(project_id.clone());
                        st.tree_expanded.insert((project_id, new_group));
                    }
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, name, kind }), "tree.copy-node") => {
                    s.borrow_mut().engine.clipboard_copy_node(&project_id, &group, &name, kind);
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, .. }), "tree.paste-node") => {
                    let mut st = s.borrow_mut();
                    if st.engine.paste_node_to(&project_id, &group).is_some() {
                        st.tree_expanded.insert((project_id, group));
                    }
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, name, .. }), "tree.rename-node") => {
                    let mut st = s.borrow_mut();
                    st.pending.open(PendingAction::RenameNode { project_id, group, old_name: name.clone() });
                    st.pending.input_buffer = name;
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, name, .. }), "tree.delete-node") => {
                    s.borrow_mut().pending.open(PendingAction::DeleteNode { project_id, group, name });
                }
                (Some(CtxMenuKind::GridCol { col }), action @ ("grid.col-insert-left"
                    | "grid.col-insert-right" | "grid.col-delete")) => {
                    ui::grid_actions::perform_col_action(&s, col, action);
                }
                (Some(CtxMenuKind::GridRow { row }), action @ ("grid.row-insert-above"
                    | "grid.row-insert-below" | "grid.row-delete")) => {
                    ui::grid_actions::perform_row_action(&s, row, action);
                }
                (Some(CtxMenuKind::GridCell { row, col }), "grid.cell-pick") => {
                    // 等价于双击 picker cell：弹 RefPicker / TypeSelector
                    // ExportEnumCol cell 的 popup 是 slint 端 component-internal property，
                    // 没暴露到 Rust 端，只能由用户双击/单击 cell 触发；右键菜单不接管。
                    let kind = ui::grid_actions::effective_column_kind_at(&s.borrow(), row, col);
                    match kind {
                        Some(crate::state::ColumnKind::Ref { ref target }) => {
                            dialogs::ref_picker::open_for_cell(&s, row, col, target);
                            if let Some(ui_h) = weak.upgrade() { dialogs::ref_picker::push(&ui_h, &s); }
                        }
                        Some(crate::state::ColumnKind::TypeEnumCol) => {
                            dialogs::type_selector::open_for_cell(&s, row, col);
                            if let Some(ui_h) = weak.upgrade() { dialogs::type_selector::push(&ui_h, &s); }
                        }
                        _ => {}
                    }
                }
                (Some(CtxMenuKind::GridCell { row: _, col: _ }), action @ ("grid.cell-copy"
                    | "grid.cell-cut" | "grid.cell-paste" | "grid.cell-clear")) => {
                    let tag = match action {
                        "grid.cell-copy" => "复制",
                        "grid.cell-cut" => "剪切",
                        "grid.cell-paste" => "粘贴",
                        "grid.cell-clear" => "清空",
                        _ => "操作",
                    };
                    ui::grid_actions::perform_action(&s, action, tag);
                }
                _ => {}
            }
            // pending input 需要校验首次 buffer
            dialogs::pending::revalidate_input(&s);
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_ctx_menu(&ui_h, &s);
            }
        });
    }
}
