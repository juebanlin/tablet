// 数据导出对话框：5 个格式勾选框 + 确认。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::AppState;
use crate::{refresh, AppWindow};

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let de = &st.data_export;
    ui_h.set_dlg_export_open(de.open);
    ui_h.set_ex_json(de.json);
    ui_h.set_ex_xml(de.xml);
    ui_h.set_ex_java(de.java);
    ui_h.set_ex_go(de.go);
    ui_h.set_ex_lua(de.lua);
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let weak = ui_h.as_weak();
    ui_h.on_ex_confirm(move || {
        let (json, xml, java, go, lua) = match weak.upgrade() {
            Some(ui_h) => (
                ui_h.get_ex_json(), ui_h.get_ex_xml(), ui_h.get_ex_java(),
                ui_h.get_ex_go(), ui_h.get_ex_lua(),
            ),
            None => return,
        };
        {
            let mut st = s.borrow_mut();
            st.data_export.json = json;
            st.data_export.xml = xml;
            st.data_export.java = java;
            st.data_export.go = go;
            st.data_export.lua = lua;
            st.data_export.open = false;
        }
        run(&s);
        if let Some(ui_h) = weak.upgrade() {
            push(&ui_h, &s);
            refresh::after_log(&ui_h, &s);
        }
    });
}

/// 执行数据导出（按 data_export 选项）。
fn run(state: &Rc<RefCell<AppState>>) {
    let opts = state.borrow().data_export.clone();
    let mut st = state.borrow_mut();
    if opts.json {
        match st.engine.export_json() {
            Ok(r) => log_result(&mut st, "JSON", &r),
            Err(e) => st.engine.log(format!("[JSON] 错误: {}", e)),
        }
    }
    if opts.xml {
        match st.engine.export_xml() {
            Ok(r) => log_result(&mut st, "XML", &r),
            Err(e) => st.engine.log(format!("[XML] 错误: {}", e)),
        }
    }
    if opts.java {
        match st.engine.export_java() {
            Ok(r) => log_result(&mut st, "Java", &r),
            Err(e) => st.engine.log(format!("[Java] 错误: {}", e)),
        }
    }
    if opts.go {
        match st.engine.export_go() {
            Ok(r) => log_result(&mut st, "Go", &r),
            Err(e) => st.engine.log(format!("[Go] 错误: {}", e)),
        }
    }
    if opts.lua {
        match st.engine.export_lua() {
            Ok(r) => log_result(&mut st, "Lua", &r),
            Err(e) => st.engine.log(format!("[Lua] 错误: {}", e)),
        }
    }
}

fn log_result(st: &mut AppState, label: &str, result: &tbl_core::export::ExportResult) {
    use tbl_core::export::FileStatus;
    st.engine.log(format!("[{}] {} 新增, {} 修改, {} 删除, {} 不变",
        label, result.added(), result.modified(), result.deleted(), result.unchanged()));
    for f in &result.files {
        match f.status {
            FileStatus::Added => st.engine.log(format!("  [新增] {}", f.path)),
            FileStatus::Modified => st.engine.log(format!("  [修改] {}", f.path)),
            FileStatus::Deleted => st.engine.log(format!("  [删除] {}", f.path)),
            FileStatus::Unchanged => {}
        }
    }
}
