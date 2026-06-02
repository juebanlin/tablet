// 跨模块刷新 fan-out。
//
// 调用方根据「我刚做了什么」选 helper，而不是「我现在要刷哪些 view」。
// 新增刷新场景就在这里加一个 helper，全局唯一来源。
//
// 注意：`refresh::*` 单向调用 ui/dialogs 的 `push`。模块之间互相只通过
// 各自的 `pub(crate) fn`（如 `type_selector::open_for_cell`），不通过本文件互调。

use std::cell::RefCell;
use std::rc::Rc;

use crate::state::AppState;
use crate::AppWindow;
use crate::{dialogs, ui};

/// 树结构变了（New/Rename/Delete Group/Node、克隆、reload）。
pub fn after_tree_change(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    ui::tree::push(ui_h, state);
    ui::grid::push(ui_h, state);
    ui::log_panel::push(ui_h, state);
}

/// grid 单元格写入或表头改了。dirty 标记会冒到树面板，所以也刷树。
pub fn after_grid_edit(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    ui::grid::push(ui_h, state);
    ui::tree::push(ui_h, state);
    ui::log_panel::push(ui_h, state);
}

/// PendingAction（input / confirm 对话框）走完。
pub fn after_pending_action(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    dialogs::pending::push_input(ui_h, state);
    dialogs::pending::push_confirm(ui_h, state);
    ui::tree::push(ui_h, state);
    ui::grid::push(ui_h, state);
    ui::log_panel::push(ui_h, state);
}

/// 右键菜单 action 走完后的标准 fan-out（含可能被打开的 input/confirm/new-project）。
pub fn after_ctx_menu(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    dialogs::context_menu::push(ui_h, state);
    dialogs::pending::push_input(ui_h, state);
    dialogs::pending::push_confirm(ui_h, state);
    dialogs::new_project::push(ui_h, state);
    ui::tree::push(ui_h, state);
    ui::grid::push(ui_h, state);
    ui::log_panel::push(ui_h, state);
}

/// 工程级 reload（reload 按钮 / 切 active project / template-create open-after）。
pub fn after_workspace_reload(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    ui::toolbar::reset_view_after_reload(state);
    ui::tree::push(ui_h, state);
    ui::grid::push(ui_h, state);
    ui::log_panel::push(ui_h, state);
}

/// 仅日志（罕见：单独打 log 不动业务）。
pub fn after_log(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    ui::log_panel::push(ui_h, state);
}

/// 启动时的初次 push（不调用 reset_view_after_reload，避免覆盖 AppState::load 已计算好的展开集）。
pub fn initial(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    ui::tree::push(ui_h, state);
    ui::grid::push(ui_h, state);
    ui::log_panel::push(ui_h, state);
    dialogs::context_menu::push(ui_h, state);
    dialogs::pending::push_input(ui_h, state);
    dialogs::pending::push_confirm(ui_h, state);
}
