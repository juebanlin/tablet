// 复制(克隆)项目对话框：项目右键「复制(克隆)...」唯一入口。
//
// 与统一「新建项目」对话框不重叠：克隆是「现有 project 内存深拷贝」，没有 source 选择 / sections-picker
// 的概念，单纯改 id/name/category/version 即可。沿用旧 NewProjectDialog 的视觉。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tablet_core::tblschema::is_valid_metadata_id;

use crate::state::AppState;
use crate::{refresh, AppWindow};

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let workdir = state.borrow().engine.workdir.clone();
    let st = state.borrow();
    ui_h.set_dlg_new_project_open(st.clone_project.open);
    if !st.clone_project.open {
        return;
    }

    let title = "复制项目".to_string();
    let intro_line = format!("源项目：{}", st.clone_project.source_display);

    // 校验：现有 project id 含 opened（available_projects）+ 磁盘上未打开的
    let mut existing: Vec<String> = st.engine.available_projects.iter()
        .map(|a| a.id.clone()).collect();
    for p in tablet_core::project::list_projects(&workdir) {
        if !existing.iter().any(|id| id == &p.id) {
            existing.push(p.id);
        }
    }
    let id = &st.clone_project.project_id;
    let id_err = if id.is_empty() {
        "Project ID 不能为空".to_string()
    } else if !is_valid_metadata_id(id) {
        "ID 仅允许小写字母 / 数字 / _ / -，长度 1..=32".to_string()
    } else if existing.iter().any(|e| e == id) {
        format!("ID 已存在: {}", id)
    } else {
        String::new()
    };
    let name_err = if st.clone_project.project_name.trim().is_empty() {
        "项目名不能为空".to_string()
    } else {
        String::new()
    };

    let can_confirm = id_err.is_empty() && name_err.is_empty();

    ui_h.set_np_dialog_title(title.into());
    ui_h.set_np_intro_line(intro_line.into());
    ui_h.set_np_project_id(st.clone_project.project_id.clone().into());
    ui_h.set_np_project_name(st.clone_project.project_name.clone().into());
    ui_h.set_np_project_category(st.clone_project.project_category.clone().into());
    ui_h.set_np_project_version(st.clone_project.project_version.clone().into());
    ui_h.set_np_open_after(false);
    ui_h.set_np_allow_open_after(false);
    ui_h.set_np_id_error(id_err.into());
    ui_h.set_np_name_error(name_err.into());
    ui_h.set_np_can_confirm(can_confirm);
}

/// 内存克隆 + 改 category/version。克隆走内存模式（schema_dirty=true）。
fn run(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    let source_project_id = st.clone_project.source_project_id.clone();
    let project_id = st.clone_project.project_id.clone();
    let display_name = st.clone_project.project_name.clone();
    let category = st.clone_project.project_category.clone();
    let version = st.clone_project.project_version.clone();

    let res = st.engine
        .clone_project_in_memory(&source_project_id, &project_id, &display_name);
    let new_id = match res {
        Some(id) => id,
        None => {
            st.engine.log(format!("[复制项目] 失败：源项目未打开 {}", source_project_id));
            return;
        }
    };
    if let Some(p) = st.engine.find_project_mut(&new_id) {
        p.schema.meta.category = category;
        p.schema.meta.version = version;
        p.schema_dirty = true;
    }
    st.engine.log(format!(
        "[复制项目] 已创建 {}（内存中，需保存才落地）",
        new_id
    ));
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_id_edited(move |v| {
            {
                let mut st = s.borrow_mut();
                st.clone_project.project_id = v.to_string();
                st.clone_project.id_prefilled = true;
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_name_edited(move |v| {
            s.borrow_mut().clone_project.project_name = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_category_edited(move |v| {
            s.borrow_mut().clone_project.project_category = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_version_edited(move |v| {
            s.borrow_mut().clone_project.project_version = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_confirm(move || {
            run(&s);
            s.borrow_mut().clone_project.close();
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                refresh::after_tree_change(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_cancel(move || {
            s.borrow_mut().clone_project.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
