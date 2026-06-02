// 全局焦点 / 快捷键：commit-pending-edit / Ctrl+C/X/V / Delete / 调试键。
//
// 任何空白 / 树节点 / toolbar 点击都会触发 commit-pending-edit；
// 没有 editing 时是 no-op，所以重复 fire 也无副作用。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::AppState;
use crate::{refresh, ui, AppWindow};

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_commit_pending_edit(move || {
            let need = {
                let st = s.borrow();
                st.editing.is_some() || st.editing_header_row >= 0
            };
            if !need { return; }
            ui::grid_actions::commit_editing(&ui_for_buf, &s);
            if let Some(ui_h) = weak.upgrade() { ui::grid::push(&ui_h, &s); }
        });
    }

    // 键盘快捷键：复制/粘贴/删除/剪切。anchor 来自当前 GridSelection；无选区时 no-op。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_shortcut_copy(move || {
            if ui::grid_actions::selection_anchor(&s).is_none() { return; }
            ui::grid_actions::perform_action(&s, "grid.cell-copy", "Ctrl+C");
            if let Some(ui_h) = weak.upgrade() {
                ui::grid::push(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_shortcut_cut(move || {
            if ui::grid_actions::selection_anchor(&s).is_none() { return; }
            ui::grid_actions::perform_action(&s, "grid.cell-cut", "Ctrl+X");
            if let Some(ui_h) = weak.upgrade() {
                ui::grid::push(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_shortcut_paste(move || {
            if ui::grid_actions::selection_anchor(&s).is_none() { return; }
            ui::grid_actions::perform_action(&s, "grid.cell-paste", "Ctrl+V");
            if let Some(ui_h) = weak.upgrade() {
                ui::grid::push(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_grid_shortcut_delete(move || {
            if ui::grid_actions::selection_anchor(&s).is_none() { return; }
            ui::grid_actions::perform_action(&s, "grid.cell-clear", "Delete");
            if let Some(ui_h) = weak.upgrade() {
                ui::grid::push(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }
    // 调试键盘事件（仅落到 log 文件，不进 UI 日志框）：
    // 用于排查 Ctrl 修饰键的字符映射问题，UI 不需要。
    {
        let s = state.clone();
        ui_h.on_debug_key(move |text, ctrl| {
            // 单按可见字符（如直接按 c）不记录，避免噪音
            let is_plain_visible = !ctrl && text.chars().count() == 1
                && text.chars().next().map_or(false, |c| !c.is_control());
            if is_plain_visible { return; }
            let bytes: Vec<String> = text.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
            log::debug!("[键盘] text={:?} bytes=[{}] ctrl={}", text.as_str(), bytes.join(" "), ctrl);
            // 占位：让 borrow 链路保持非空，避免编译器警告
            let _ = s.borrow();
        });
    }
}
