// 新建项目对话框（统一 3 模式：空 / 模板 / 克隆）。
//
// 由模板库的「使用此模板」按钮、tree 面板的「新建项目」按钮、tree 项目右键的「复制(克隆)」三个入口共享。
// run() 按 mode 走不同落地路径：Empty/FromTemplate 写盘 + reload；Clone 仅内存复制 + set_active。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::{AppState, NewProjectMode};
use crate::{refresh, AppWindow};

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    use tbl_core::tblschema::is_valid_metadata_id;
    let workdir = state.borrow().engine.workdir.clone();
    let mut st = state.borrow_mut();
    ui_h.set_dlg_new_project_open(st.new_project.open);
    if !st.new_project.open {
        return;
    }

    // 按 mode 取标题 / 介绍行 / id-name 默认值 / allow-open-after
    let (title, intro_line, default_id, default_name, allow_open_after) = match &st.new_project.mode {
        NewProjectMode::Empty => (
            "新建空项目".to_string(),
            String::new(),
            String::new(),
            String::new(),
            true,
        ),
        NewProjectMode::FromTemplate { template_id, template_source, template_display } => (
            "从模板新建项目".to_string(),
            format!("基于模板：{} ({})", template_display, template_source),
            template_id.clone(),
            template_display.clone(),
            true,
        ),
        NewProjectMode::Clone { source_project_id, source_display } => (
            "复制项目".to_string(),
            format!("源项目：{}", source_display),
            format!("{}_copy", source_project_id),
            format!("{}_copy", source_display),
            false,
        ),
    };

    if !st.new_project.id_prefilled && st.new_project.project_id.is_empty() {
        st.new_project.project_id = default_id;
        st.new_project.id_prefilled = true;
    }
    if st.new_project.project_name.is_empty() {
        st.new_project.project_name = default_name;
    }

    // 校验：现有 project id 含 opened（available_projects）+ 磁盘上未打开的
    let mut existing: Vec<String> = st.engine.available_projects.iter()
        .map(|a| a.id.clone()).collect();
    for p in tbl_core::project::list_projects(&workdir) {
        if !existing.iter().any(|id| id == &p.id) {
            existing.push(p.id);
        }
    }
    let id = &st.new_project.project_id;
    let id_err = if id.is_empty() {
        "Project ID 不能为空".to_string()
    } else if !is_valid_metadata_id(id) {
        "ID 仅允许小写字母 / 数字 / _ / -，长度 1..=32".to_string()
    } else if existing.iter().any(|e| e == id) {
        format!("ID 已存在: {}", id)
    } else {
        String::new()
    };
    let name_err = if st.new_project.project_name.trim().is_empty() {
        "项目名不能为空".to_string()
    } else {
        String::new()
    };

    let can_confirm = id_err.is_empty() && name_err.is_empty();

    ui_h.set_np_dialog_title(title.into());
    ui_h.set_np_intro_line(intro_line.into());
    ui_h.set_np_project_id(st.new_project.project_id.clone().into());
    ui_h.set_np_project_name(st.new_project.project_name.clone().into());
    ui_h.set_np_project_category(st.new_project.project_category.clone().into());
    ui_h.set_np_project_version(st.new_project.project_version.clone().into());
    ui_h.set_np_open_after(st.new_project.open_after);
    ui_h.set_np_allow_open_after(allow_open_after);
    ui_h.set_np_id_error(id_err.into());
    ui_h.set_np_name_error(name_err.into());
    ui_h.set_np_can_confirm(can_confirm);
    ui_h.set_np_template_has_preset(st.new_project.template_has_preset);
    ui_h.set_np_with_preset(st.new_project.with_preset);
}

/// 真正落地新项目；返回是否需要"大刷新"（reload + reset view）。
/// 三种模式都走内存模式：项目仅在内存里，schema_dirty + root_pending_create=true，
/// 树面板自动显示 +；用户点保存才落盘。所以一律返回 false。
fn run(state: &Rc<RefCell<AppState>>) -> bool {
    use tbl_core::tblschema::TblSchema;
    use tbl_core::template::{default_local_dir, BuiltinTemplates, LocalTemplates, TemplateSource};

    let mut st = state.borrow_mut();
    let mode = st.new_project.mode.clone();
    let project_id = st.new_project.project_id.clone();
    let display_name = st.new_project.project_name.clone();
    let category = st.new_project.project_category.clone();
    let version = st.new_project.project_version.clone();
    let open_after = st.new_project.open_after;
    let with_preset = st.new_project.with_preset;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result: Result<String, String> = match &mode {
        NewProjectMode::Empty => {
            let mut schema = TblSchema::default();
            schema.meta.id = project_id.clone();
            schema.meta.name = display_name.clone();
            schema.meta.category = category.clone();
            schema.meta.version = version.clone();
            schema.meta.created_at = now.clone();
            st.engine.create_project_in_memory_with(schema, false)
        }
        NewProjectMode::FromTemplate { template_id, template_source, .. } => {
            let content = match template_source.as_str() {
                "local" => LocalTemplates::new(default_local_dir()).load_by_id(template_id),
                _ => BuiltinTemplates::new().load_by_id(template_id),
            };
            let content = match content {
                Some(c) => c,
                None => {
                    st.engine.log(format!("[新建项目] 模板未找到: {}", template_id));
                    return false;
                }
            };
            let mut schema = content.schema;
            schema.meta.id = project_id.clone();
            schema.meta.name = display_name.clone();
            schema.meta.category = category.clone();
            schema.meta.version = version.clone();
            schema.meta.created_at = now.clone();
            schema.meta.source_template = content.meta.id.clone();
            schema.meta.source_template_version = content.meta.version.clone();
            st.engine.create_project_in_memory_with(schema, with_preset)
        }
        NewProjectMode::Clone { source_project_id, .. } => {
            let res = st.engine
                .clone_project_in_memory(source_project_id, &project_id, &display_name);
            if let Some(ref new_id) = res {
                if let Some(p) = st.engine.find_project_mut(new_id) {
                    p.schema.meta.category = category.clone();
                    p.schema.meta.version = version.clone();
                    p.schema_dirty = true;
                }
            }
            res.ok_or_else(|| format!("克隆失败: 源项目未打开 {}", source_project_id))
        }
    };

    let new_id = match result {
        Ok(id) => id,
        Err(e) => {
            st.engine.log(format!("[新建项目] 失败: {}", e));
            return false;
        }
    };

    if open_after {
        st.engine.set_active_by_id(&new_id);
    }
    st.engine.log(format!(
        "[新建项目] 已创建 {}（内存中，需保存才落地）",
        new_id
    ));
    false
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_id_edited(move |v| {
            {
                let mut st = s.borrow_mut();
                st.new_project.project_id = v.to_string();
                st.new_project.id_prefilled = true;
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_name_edited(move |v| {
            s.borrow_mut().new_project.project_name = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_category_edited(move |v| {
            s.borrow_mut().new_project.project_category = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_version_edited(move |v| {
            s.borrow_mut().new_project.project_version = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_confirm(move || {
            // 同步 in-out checkbox 当前值
            if let Some(ui_h) = weak.upgrade() {
                let mut st = s.borrow_mut();
                st.new_project.open_after = ui_h.get_np_open_after();
                st.new_project.with_preset = ui_h.get_np_with_preset();
            }
            run(&s);
            s.borrow_mut().new_project.close();
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
            s.borrow_mut().new_project.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        // 树面板「新建项目」按钮 → 打开 NewProject 对话框（Empty 模式）
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_np_new_project_clicked(move || {
            s.borrow_mut().new_project.open_empty();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
