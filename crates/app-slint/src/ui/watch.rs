//! 后台观察：xlsx 组的工作簿被外部改过时，自动重载（干净）或提醒（有未保存改动）。
//!
//! 为什么需要它：xlsx 组的工作簿就是真相源，策划随时可能在 Excel 里改完保存。
//! 工具这边内存里是加载时的那一份——不刷新的话，界面一直显示旧数据，而保存又会被
//! 「外部已修改」拦下（@02.26.8）。用轮询而不是文件系统事件：跨平台可控、代价只是
//! 每个 xlsx 组一次 `stat`。
//!
//! 三条原则：
//! - **有未保存改动时不自动重载**（会把人手上的东西抽走），只提醒一次；
//! - 正在编辑单元格时不重载；
//! - 重载只发生在干净组上，重载后立即刷新树/网格/日志。

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use crate::state::AppState;
use crate::AppWindow;

const POLL: Duration = Duration::from_millis(1500);

/// 启动轮询。返回的 `Timer` 必须活到窗口结束（drop 即停）。
pub fn start(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) -> slint::Timer {
    let weak = ui_h.as_weak();
    let s = state.clone();
    // 已经提醒过的 (project, group)：文件没再变之前不重复刷日志。
    let warned: Rc<RefCell<HashSet<(String, String)>>> = Rc::new(RefCell::new(HashSet::new()));

    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, POLL, move || {
        let Some(ui) = weak.upgrade() else { return; };
        tick(&ui, &s, &warned);
    });
    timer
}

fn tick(
    ui: &AppWindow,
    state: &Rc<RefCell<AppState>>,
    warned: &Rc<RefCell<HashSet<(String, String)>>>,
) {
    // 正在改格子：别把编辑中的模型换掉。
    if state.borrow().editing.is_some() {
        return;
    }

    let changes = state.borrow().engine.xlsx_external_changes();
    if changes.is_empty() {
        warned.borrow_mut().clear();
        return;
    }

    let mut reloaded = Vec::new();
    let mut conflicts = Vec::new();
    for ch in changes {
        let key = (ch.project_id.clone(), ch.group.clone());
        if ch.dirty {
            if warned.borrow_mut().insert(key) {
                conflicts.push(ch);
            }
            continue;
        }
        let result = state
            .borrow_mut()
            .engine
            .reload_xlsx_group(&ch.project_id, &ch.group);
        match result {
            Ok(()) => {
                warned.borrow_mut().remove(&key);
                reloaded.push(ch);
            }
            Err(e) => {
                if warned.borrow_mut().insert(key) {
                    state.borrow_mut().engine.warn_log(format!(
                        "[Excel] 重新加载 {} / {} 失败: {}",
                        ch.project_id, ch.group, e
                    ));
                }
            }
        }
    }

    if reloaded.is_empty() && conflicts.is_empty() {
        return;
    }
    {
        let mut st = state.borrow_mut();
        for ch in &reloaded {
            st.engine.ui_log(format!(
                "[Excel] {} / {} 的工作簿在外部改过，已重新加载",
                ch.project_id, ch.group
            ));
        }
    }
    // 冲突要让用户「看见」，不能只躺在日志里：弹一次提示框（累加语义，多个组一次看全）。
    for ch in &conflicts {
        crate::dialogs::alert::raise(
            state,
            "工作簿被外部修改",
            &format!(
                "Project {} 的组 {} 的工作簿已被外部（Excel）修改，但当前还有未保存改动。\n\
                 保存会被拒绝。请先处理未保存改动，或右键该组「从磁盘重新加载」丢弃它们。",
                ch.project_id, ch.group
            ),
        );
    }
    crate::refresh::after_tree_change(ui, state);
}
