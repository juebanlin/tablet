// 日志面板。push 逻辑已迁移到 bottom_panel::push_log。
// 这里仅保留 wire 入口以兼容 main.rs 的注册。

use std::cell::RefCell;
use std::rc::Rc;

use crate::state::AppState;
use crate::AppWindow;

pub fn wire(_ui: &AppWindow, _state: &Rc<RefCell<AppState>>) {
    // 日志刷新已迁移到 bottom_panel::push_log。
}
