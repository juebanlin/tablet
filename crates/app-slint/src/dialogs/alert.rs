// 「提示框」：只有一个「确定」。用于「项目打不开」这类**必须让用户看见、但没有可取消
// 动作**的事（@02.26）。
//
// 与 pending.rs 的 confirm 分开，是因为那个的开关由 `pending.action` 驱动 —— 任何一次
// `refresh::after_ctx_menu` 都会按 pending 的状态把它关掉，借用它弹提示会被立刻顶掉。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::AppState;
use crate::AppWindow;

/// 弹一条。**累加**语义：已经弹着的那条后面追加，一次把原因看全
/// （一次加载可能有多个项目打不开，分多次点是折磨）。
///
/// 同时落日志 —— 用户点掉之后，日志里还留得住。
pub fn raise(state: &Rc<RefCell<AppState>>, title: &str, message: &str) {
    let mut st = state.borrow_mut();
    match &mut st.alert {
        Some((_, prev)) => {
            prev.push_str("\n\n");
            prev.push_str(message);
        }
        None => st.alert = Some((title.to_string(), message.to_string())),
    }
    // 日志里压成一行，免得日志面板被一条多行消息撑开
    st.engine.error_log(format!("[{}] {}", title, message.replace('\n', " ")));
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    match &state.borrow().alert {
        Some((title, message)) => {
            ui_h.set_dlg_alert_title(title.clone().into());
            ui_h.set_dlg_alert_message(message.clone().into());
            ui_h.set_dlg_alert_open(true);
        }
        None => ui_h.set_dlg_alert_open(false),
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let weak = ui_h.as_weak();
    ui_h.on_dlg_alert_closed(move || {
        s.borrow_mut().alert = None;
        if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
    });
}
