// 模板库对话框：内置 / 本地 tab + 搜索 + 列表 + 详情 + 「使用此模板」。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tbl_core::template::{default_local_dir, BuiltinTemplates, LocalTemplates, TemplateMeta, TemplateSource};

use crate::state::{self, AppState};
use crate::{dialogs, AppWindow, TemplateItem};

/// 按当前 tab 列出模板 metas（不应用 search 过滤；过滤是 push 端的事）。
fn list_metas(tl: &state::TemplateLibraryState) -> Vec<TemplateMeta> {
    match tl.tab {
        1 => LocalTemplates::new(default_local_dir()).list(),
        _ => BuiltinTemplates::new().list(),
    }
}

fn load_sections_count(id: &str, source: &str) -> usize {
    let content = match source {
        "local" => LocalTemplates::new(default_local_dir()).load_by_id(id),
        _ => BuiltinTemplates::new().load_by_id(id),
    };
    content.map(|c| c.schema.sections.len()).unwrap_or(0)
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    ui_h.set_dlg_template_open(st.template_lib.open);
    if !st.template_lib.open {
        return;
    }

    let builtin = BuiltinTemplates::new().list();
    let local = LocalTemplates::new(default_local_dir()).list();
    ui_h.set_tpl_builtin_count(builtin.len() as i32);
    ui_h.set_tpl_local_count(local.len() as i32);
    ui_h.set_tpl_tab_index(st.template_lib.tab);
    ui_h.set_tpl_search(st.template_lib.search.clone().into());

    let active = match st.template_lib.tab {
        1 => &local,
        _ => &builtin,
    };
    let q = st.template_lib.search.to_lowercase();
    let filtered: Vec<&TemplateMeta> = active
        .iter()
        .filter(|m| {
            if q.is_empty() {
                return true;
            }
            m.id.to_lowercase().contains(&q)
                || m.name.to_lowercase().contains(&q)
                || m.category.to_lowercase().contains(&q)
        })
        .collect();

    let items: Vec<TemplateItem> = filtered
        .iter()
        .map(|m| {
            let sections = load_sections_count(&m.id, m.source);
            TemplateItem {
                id: m.id.clone().into(),
                name: (if m.name.is_empty() { m.id.clone() } else { m.name.clone() }).into(),
                category: m.category.clone().into(),
                version: m.version.clone().into(),
                source: m.source.into(),
                sections: sections as i32,
                selected: m.id == st.template_lib.selected_id,
            }
        })
        .collect();

    let detail = filtered
        .iter()
        .find(|m| m.id == st.template_lib.selected_id)
        .map(|m| {
            let sections = load_sections_count(&m.id, m.source);
            format!(
                "id: {}\nname: {}\ncategory: {} · version: {} · 来源: {}\nSections: {}",
                m.id,
                if m.name.is_empty() { m.id.as_str() } else { m.name.as_str() },
                if m.category.is_empty() { "-" } else { m.category.as_str() },
                if m.version.is_empty() { "-" } else { m.version.as_str() },
                m.source,
                sections,
            )
        })
        .unwrap_or_default();

    let can_use = !st.template_lib.selected_id.is_empty();

    ui_h.set_tpl_items(slint::ModelRc::new(slint::VecModel::from(items)));
    ui_h.set_tpl_detail(detail.into());
    ui_h.set_tpl_can_use(can_use);
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tpl_set_tab(move |i| {
            {
                let mut st = s.borrow_mut();
                st.template_lib.tab = i;
                st.template_lib.selected_id.clear();
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tpl_search_edited(move |q| {
            s.borrow_mut().template_lib.search = q.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tpl_select_item(move |idx| {
            {
                let mut st = s.borrow_mut();
                let items = list_metas(&st.template_lib);
                if let Some(m) = items.get(idx as usize) {
                    st.template_lib.selected_id = m.id.clone();
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tpl_use_template(move || {
            // 关闭模板库 → 拿选中模板 → 打开新建项目
            let opened = {
                use tbl_core::template::{default_local_dir, BuiltinTemplates, LocalTemplates, TemplateSource};
                let mut st = s.borrow_mut();
                let items = list_metas(&st.template_lib);
                let chosen = items.iter().find(|m| m.id == st.template_lib.selected_id).cloned();
                st.template_lib.open = false;
                if let Some(meta) = chosen {
                    // 提前加载一次 schema 取 has_preset，让对话框可以显示「灌入预设」开关
                    let has_preset = match meta.source {
                        "local" => LocalTemplates::new(default_local_dir()).load_by_id(&meta.id),
                        _ => BuiltinTemplates::new().load_by_id(&meta.id),
                    }.map(|c| c.schema.meta.has_preset).unwrap_or(false);
                    st.new_project.open_from_template(&meta);
                    st.new_project.set_template_preset_hint(has_preset);
                    true
                } else {
                    false
                }
            };
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                if opened { dialogs::new_project::push(&ui_h, &s); }
            }
        });
    }
}
