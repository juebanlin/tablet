// 底部面板：日志 Tab + Excel 文件 Tab。

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Model};

use crate::state::AppState;
use crate::AppWindow;

pub fn push_log(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let flat = st.engine.logs.join("\n");
    ui.set_logs_text(flat.into());
}

pub fn push_excel_files(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let mut files: Vec<slint::SharedString> = Vec::new();

    if let Some(pid) = st.engine.active_project_id() {
        if let Some(project) = st.engine.find_project(pid) {
            let excel_dir = project.project_root.join(".excel");
            if let Ok(entries) = std::fs::read_dir(&excel_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |e| e == "xlsx") {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            files.push(name.into());
                        }
                    }
                }
            }
        }
    }
    files.sort();

    let len = files.len();
    let selected: Vec<bool> = vec![false; len];
    ui.set_excel_files(slint::ModelRc::new(slint::VecModel::from(files)));
    ui.set_excel_selected(slint::ModelRc::new(slint::VecModel::from(selected)));
    ui.set_excel_has_selection(false);
}

fn delete_selected_files(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let files_model = ui.get_excel_files();
    let sel_model = ui.get_excel_selected();
    let n = files_model.row_count();
    let mut to_delete: Vec<String> = Vec::new();
    for i in 0..n {
        if sel_model.row_data(i).unwrap_or(false) {
            if let Some(name) = files_model.row_data(i) {
                to_delete.push(name.to_string());
            }
        }
    }
    if to_delete.is_empty() { return; }

    let pid = {
        let st = state.borrow();
        st.engine.active_project_id().map(str::to_string)
    };
    if let Some(pid) = pid {
        let mut st = state.borrow_mut();
        if let Some(project) = st.engine.find_project_mut(&pid) {
            let excel_dir = project.project_root.join(".excel");
            for name in &to_delete {
                let _ = std::fs::remove_file(excel_dir.join(name));
            }
            st.engine.ui_log(format!("已删除 {} 个 Excel 文件", to_delete.len()));
        }
    }
}

pub fn wire(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let _ui_h = ui.as_weak();
    ui.on_bottom_tab_changed(move |_i| {
        let _ = s.borrow();
        // tab switch — no state change
    });

    let s = state.clone();
    let ui_h2 = ui.as_weak();
    ui.on_clear_log_clicked(move || {
        s.borrow_mut().engine.logs.clear();
        if let Some(u) = ui_h2.upgrade() {
            push_log(&u, &s);
        }
    });

    let s = state.clone();
    let ui_h2 = ui.as_weak();
    ui.on_excel_delete_clicked(move || {
        if let Some(u) = ui_h2.upgrade() {
            delete_selected_files(&u, &s);
            push_excel_files(&u, &s);
        }
    });

    let s = state.clone();
    let ui_h2 = ui.as_weak();
    ui.on_excel_sync_clicked(move || {
        let st = s.borrow();
        let _ = st; // sync panel will be implemented in Phase 2
        // TODO: open Excel Sync dialog
    });

    let s = state.clone();
    let ui_h2 = ui.as_weak();
    ui.on_excel_refresh_clicked(move || {
        if let Some(u) = ui_h2.upgrade() {
            push_excel_files(&u, &s);
        }
    });

    let s = state.clone();
    let ui_h2 = ui.as_weak();
    ui.on_excel_file_opened(move |idx| {
        let u = if let Some(u) = ui_h2.upgrade() { u } else { return; };
        let files_model = u.get_excel_files();
        let name = if let Some(n) = files_model.row_data(idx as usize) { n.to_string() } else { return; };
        let st = s.borrow();
        let Some(pid) = st.engine.active_project_id() else { return };
        let Some(project) = st.engine.find_project(pid) else { return };
        let path = project.project_root.join(".excel").join(&name);
        let _ = open::that(&path);
    });
}
