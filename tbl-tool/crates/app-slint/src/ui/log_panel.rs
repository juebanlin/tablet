// 日志面板：把 engine.logs 转成 slint LogEntry 列表 + 拼接的多行文本。
//
// LogPanel 主要用 logs-text（read-only 多行 TextInput，可跨行选中复制）；
// LogEntry 列表保留以兼容旧字段 / 将来按 level 着色。
// level 推断：消息含 "失败" / "错误" / "[验证]" → error；含 "警告" → warn；其它 info。

use std::cell::RefCell;
use std::rc::Rc;

use crate::state::AppState;
use crate::{AppWindow, LogEntry};

pub fn push(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let entries: Vec<LogEntry> = st.engine.logs.iter().map(|line| {
        let (time, msg) = match line.split_once(' ') {
            Some((t, m)) => (t.to_string(), m.to_string()),
            None => (String::new(), line.clone()),
        };
        let level = if msg.contains("失败") || msg.contains("错误") || msg.contains("[验证]") {
            2
        } else if msg.contains("警告") {
            1
        } else {
            0
        };
        LogEntry { time: time.into(), msg: msg.into(), level }
    }).collect();
    let flat = st.engine.logs.join("\n");
    ui.set_logs(slint::ModelRc::new(slint::VecModel::from(entries)));
    ui.set_logs_text(flat.into());
}

pub fn wire(_ui: &AppWindow, _state: &Rc<RefCell<AppState>>) {
    // 日志面板没有交互回调（点击全文复制由 slint 侧 TextInput 自带）。
}
