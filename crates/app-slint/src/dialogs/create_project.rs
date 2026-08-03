// 「新建项目」统一对话框：3 tab + 单页左右分栏。
//
// tab 0 = 空项目：右半填身份 → 创建一个空 project
// tab 1 = 从文件：浏览 .tblschema → sections-picker → 创建 + 可选灌入预设
// tab 2 = 从模板：内置/本地子 tab + 搜索 + 列表 → sections-picker → 创建 + 可选灌入预设
//
// 替代旧 TemplateLibraryDialog + 顶部「导入 Schema」+ NewProject Empty/FromTemplate 三个入口。
// Clone 路径（项目右键「复制(克隆)...」）独立走 NewProjectDialog（仅留 Clone 模式）。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tablet_core::tblschema::{
    is_valid_metadata_id, parse_tblschema, SchemaMode, SchemaSection, TblSchema,
};
use tablet_core::template::{
    default_local_dir, BuiltinTemplates, LocalTemplates, TemplateContent, TemplateMeta, TemplateSource,
};

use crate::state::{AppState, CreatePickerItem};
use crate::theme::{ICON_CONST, ICON_ENUM, ICON_GROUP, ICON_TABLE};
use crate::{refresh, AppWindow, SchemaItem, TemplateItem};

/// 把 schema 的 sections 扁平化为 picker_items（带组节点）。
fn build_picker_items(schema: &TblSchema) -> Vec<CreatePickerItem> {
    // 按 group 顺序聚合
    let mut grouped: Vec<(String, Vec<(String, SchemaMode)>)> = Vec::new();
    for sec in &schema.sections {
        if let Some(entry) = grouped.iter_mut().find(|(g, _)| *g == sec.group) {
            entry.1.push((sec.name.clone(), sec.mode.clone()));
        } else {
            grouped.push((sec.group.clone(), vec![(sec.name.clone(), sec.mode.clone())]));
        }
    }
    let mut items: Vec<CreatePickerItem> = Vec::new();
    for (g, secs) in &grouped {
        items.push(CreatePickerItem {
            indent: 0,
            group: g.clone(),
            name: g.clone(),
            mode: SchemaMode::Table,
        });
        for (name, mode) in secs {
            items.push(CreatePickerItem {
                indent: 1,
                group: g.clone(),
                name: name.clone(),
                mode: mode.clone(),
            });
        }
    }
    items
}

/// FromFile / FromTemplate 切换 source 时调用：重置 picker，预填 id/name/category/version。
fn on_source_loaded(st: &mut AppState, schema: TblSchema) {
    st.create_project.picker_items = build_picker_items(&schema);
    st.create_project.picker_checked = vec![true; st.create_project.picker_items.len()];
    let cps = &mut st.create_project;
    cps.project_id = schema.meta.id.clone();
    cps.project_name = if !schema.meta.name.is_empty() { schema.meta.name.clone() } else { schema.meta.id.clone() };
    cps.project_category = schema.meta.category.clone();
    cps.project_version = if !schema.meta.version.is_empty() { schema.meta.version.clone() } else { "1.0.0".into() };
    cps.with_preset = schema.meta.has_preset;
    cps.id_dirty = true;
    match cps.tab {
        1 => cps.file_schema = Some(schema),
        2 => cps.tpl_schema = Some(schema),
        _ => {}
    }
}

/// 切 tab 时清掉 picker / source 相关字段，重置右侧身份。
fn reset_source_on_tab_change(cps: &mut crate::state::CreateProjectState) {
    cps.picker_items.clear();
    cps.picker_checked.clear();
    cps.file_path.clear();
    cps.file_schema = None;
    cps.file_error.clear();
    cps.tpl_selected_id.clear();
    cps.tpl_schema = None;
    cps.tpl_meta_id.clear();
    cps.tpl_meta_version.clear();
    cps.tpl_search.clear();
    cps.with_preset = false;
    // 重置右侧身份为默认
    cps.project_id.clear();
    cps.project_name.clear();
    cps.project_category.clear();
    cps.project_version = "1.0.0".into();
    cps.id_dirty = true;  // 下一轮 push 写回 slint
}

fn list_template_metas(subtab: i32) -> Vec<TemplateMeta> {
    match subtab {
        1 => LocalTemplates::new(default_local_dir()).list(),
        _ => BuiltinTemplates::new().list(),
    }
}

fn load_template_content(id: &str, source: &str) -> Option<TemplateContent> {
    match source {
        "local" => LocalTemplates::new(default_local_dir()).load_by_id(id),
        _ => BuiltinTemplates::new().load_by_id(id),
    }
}

fn id_validation(st: &AppState, id: &str) -> Option<String> {
    if id.is_empty() {
        return Some("Project ID 不能为空".to_string());
    }
    if !is_valid_metadata_id(id) {
        return Some("ID 仅允许小写字母 / 数字 / _ / -，长度 1..=32".to_string());
    }
    let workdir = &st.engine.workdir;
    let mut existing: Vec<String> = st.engine.available_projects.iter().map(|a| a.id.clone()).collect();
    for p in tablet_core::project::list_projects(workdir) {
        if !existing.iter().any(|e| e == &p.id) {
            existing.push(p.id);
        }
    }
    if existing.iter().any(|e| e == id) {
        return Some(format!("ID 已存在: {}", id));
    }
    None
}

/// 把 CreateProjectState 推到 slint 端。
pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    ui_h.set_dlg_create_project_open(st.create_project.open);
    if !st.create_project.open {
        return;
    }
    let cps = &st.create_project;

    ui_h.set_cp_tab_index(cps.tab);
    // tab/模板切换时强制写回身份字段，退出编辑状态
    if cps.id_dirty {
        ui_h.set_cp_project_id(cps.project_id.clone().into());
        ui_h.set_cp_project_name(cps.project_name.clone().into());
        ui_h.set_cp_project_category(cps.project_category.clone().into());
        ui_h.set_cp_project_version(cps.project_version.clone().into());
    }
    ui_h.set_cp_open_after(cps.open_after);
    ui_h.set_cp_with_preset(cps.with_preset);

    // 校验
    let id_err = id_validation(&st, &cps.project_id).unwrap_or_default();
    let name_err = if cps.project_name.trim().is_empty() {
        "项目名不能为空".to_string()
    } else {
        String::new()
    };
    ui_h.set_cp_id_error(id_err.clone().into());
    ui_h.set_cp_name_error(name_err.clone().into());

    // can_confirm：身份合法 + (空项目 / 来源已加载且至少选一个 section)
    let identity_ok = id_err.is_empty() && name_err.is_empty();
    let source_ok = match cps.tab {
        0 => true,
        1 => cps.file_schema.is_some()
            && cps.picker_checked.iter().enumerate()
                .any(|(i, &c)| c && cps.picker_items.get(i).map_or(false, |it| it.indent == 1)),
        2 => cps.tpl_schema.is_some()
            && cps.picker_checked.iter().enumerate()
                .any(|(i, &c)| c && cps.picker_items.get(i).map_or(false, |it| it.indent == 1)),
        _ => false,
    };
    ui_h.set_cp_can_confirm(identity_ok && source_ok);

    // 来源是否带 preset（决定 with-preset checkbox 可见性）
    let source_has_preset = cps.source_schema().map_or(false, |s| s.meta.has_preset);
    ui_h.set_cp_source_has_preset(source_has_preset);

    // FromFile fields
    ui_h.set_cp_file_path(cps.file_path.clone().into());
    ui_h.set_cp_file_loaded(cps.file_schema.is_some());
    ui_h.set_cp_file_error(cps.file_error.clone().into());

    // FromTemplate fields
    let builtin = BuiltinTemplates::new().list();
    let local = LocalTemplates::new(default_local_dir()).list();
    ui_h.set_cp_tpl_subtab(cps.tpl_subtab);
    ui_h.set_cp_tpl_builtin_count(builtin.len() as i32);
    ui_h.set_cp_tpl_local_count(local.len() as i32);
    ui_h.set_cp_tpl_search(cps.tpl_search.clone().into());
    let active = match cps.tpl_subtab {
        1 => &local,
        _ => &builtin,
    };
    let q = cps.tpl_search.to_lowercase();
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
    let tpl_items: Vec<TemplateItem> = filtered
        .iter()
        .map(|m| {
            let sections = load_template_content(&m.id, m.source)
                .map(|c| c.schema.sections.len())
                .unwrap_or(0);
            TemplateItem {
                id: m.id.clone().into(),
                name: (if m.name.is_empty() { m.id.clone() } else { m.name.clone() }).into(),
                category: m.category.clone().into(),
                version: m.version.clone().into(),
                source: m.source.into(),
                sections: sections as i32,
                selected: m.id == cps.tpl_selected_id,
            }
        })
        .collect();
    ui_h.set_cp_tpl_items(slint::ModelRc::new(slint::VecModel::from(tpl_items)));
    ui_h.set_cp_tpl_selected(!cps.tpl_selected_id.is_empty() && cps.tpl_schema.is_some());

    // sections-picker（按当前 tab 决定是否有内容）
    let n = cps.picker_items.len();
    // 计算 group 区段
    let mut group_ranges: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if cps.picker_items[i].indent == 0 {
            let mut j = i + 1;
            while j < n && cps.picker_items[j].indent != 0 {
                j += 1;
            }
            group_ranges.push((i, i + 1, j));
            i = j;
        } else {
            i += 1;
        }
    }
    let mut slint_items: Vec<SchemaItem> = Vec::with_capacity(n);
    for (idx, item) in cps.picker_items.iter().enumerate() {
        let (checked, tristate, icon) = if item.indent == 0 {
            let (_, start, end) = group_ranges
                .iter()
                .find(|(g, _, _)| *g == idx)
                .copied()
                .unwrap_or((idx, idx + 1, idx + 1));
            let mut all = true;
            let mut any = false;
            for k in start..end {
                if cps.picker_checked.get(k).copied().unwrap_or(false) {
                    any = true;
                } else {
                    all = false;
                }
            }
            (all && start < end, any && !all, ICON_GROUP.to_string())
        } else {
            let icon = match item.mode {
                SchemaMode::Table => ICON_TABLE,
                SchemaMode::Constant => ICON_CONST,
                SchemaMode::Enum => ICON_ENUM,
            };
            (
                cps.picker_checked.get(idx).copied().unwrap_or(false),
                false,
                icon.to_string(),
            )
        };
        slint_items.push(SchemaItem {
            indent: item.indent as i32,
            icon: icon.into(),
            name: item.name.clone().into(),
            group_name: item.group.clone().into(),
            checked,
            tristate,
            is_conflict: false,
        });
    }
    let total: i32 = cps.picker_items.iter().filter(|it| it.indent == 1).count() as i32;
    let selected: i32 = cps.picker_items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && cps.picker_checked.get(*i).copied().unwrap_or(false))
        .count() as i32;
    let all_checked = selected == total && total > 0;
    ui_h.set_cp_picker_items(slint::ModelRc::new(slint::VecModel::from(slint_items)));
    ui_h.set_cp_picker_all_checked(all_checked);
    ui_h.set_cp_picker_selected_count(selected);
    ui_h.set_cp_picker_total_count(total);
    drop(st);
    let mut st = state.borrow_mut();
    st.create_project.id_dirty = false;
}

/// 真正落地：按 tab 构造 schema → create_project_in_memory_with → set_active_by_id。
fn run(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    let project_id = st.create_project.project_id.clone();
    let display_name = st.create_project.project_name.clone();
    let category = st.create_project.project_category.clone();
    let version = st.create_project.project_version.clone();
    let open_after = st.create_project.open_after;
    let tab = st.create_project.tab;
    let with_preset = st.create_project.with_preset;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // 收集勾选的 (group, name)
    let selected: Vec<(String, String)> = st.create_project.picker_items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && st.create_project.picker_checked.get(*i).copied().unwrap_or(false))
        .map(|(_, it)| (it.group.clone(), it.name.clone()))
        .collect();

    let result: Result<String, String> = match tab {
        0 => {
            let mut schema = TblSchema::default();
            // 程序级默认分隔符（来自 workspace tablet.toml [separators]）拷贝到新项目；
            // 项目落盘后这份 schema.separators 即成为该项目自身的单一来源。
            schema.separators = st.engine.default_separators.clone();
            schema.meta.id = project_id.clone();
            schema.meta.name = display_name.clone();
            schema.meta.category = category.clone();
            schema.meta.version = version.clone();
            schema.meta.created_at = now.clone();
            st.engine.create_project_in_memory_with(schema, false)
        }
        1 | 2 => {
            let source = st.create_project.source_schema().cloned();
            let source = match source {
                Some(s) => s,
                None => {
                    st.engine.ui_log("[新建项目] 内部错误：source 为空".to_string());
                    return;
                }
            };
            let sections: Vec<SchemaSection> = source.sections.iter()
                .filter(|s| selected.iter().any(|(g, n)| g == &s.group && n == &s.name))
                .cloned()
                .collect();
            let (source_template, source_template_version) = if tab == 2 {
                (st.create_project.tpl_meta_id.clone(), st.create_project.tpl_meta_version.clone())
            } else {
                (String::new(), String::new())
            };
            let mut schema = TblSchema { meta: source.meta.clone(), separators: source.separators.clone(), sections };
            schema.meta.id = project_id.clone();
            schema.meta.name = display_name.clone();
            schema.meta.category = category.clone();
            schema.meta.version = version.clone();
            schema.meta.created_at = now.clone();
            schema.meta.source_template = source_template;
            schema.meta.source_template_version = source_template_version;
            // has_preset 跟随实际 sections 重算
            schema.meta.has_preset = schema.sections.iter().any(|s| !s.preset.is_empty());
            st.engine.create_project_in_memory_with(schema, with_preset)
        }
        _ => {
            st.engine.ui_log(format!("[新建项目] 未知 tab: {}", tab));
            return;
        }
    };

    let new_id = match result {
        Ok(id) => id,
        Err(e) => {
            st.engine.error_log(format!("[新建项目] 失败: {}", e));
            return;
        }
    };

    if open_after {
        st.engine.set_active_by_id(&new_id);
    }
    st.engine.ui_log(format!(
        "[新建项目] 已创建 {}（内存中，需保存才落地）",
        new_id
    ));
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // cp-new-project-clicked：树面板「新建项目」按钮 → 打开对话框（默认 Empty tab）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_new_project_clicked(move || {
            s.borrow_mut().create_project.open_empty();
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
            }
        });
    }

    // cp-set-tab：切 tab
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_set_tab(move |i| {
            {
                let mut st = s.borrow_mut();
                if st.create_project.tab != i {
                    st.create_project.tab = i;
                    reset_source_on_tab_change(&mut st.create_project);
                }
            }
            // 切换到模板 tab → 自动选中第一个模板
            if i == 2 {
                let metas = list_template_metas(0); // builtin
                if let Some(meta) = metas.first() {
                    if let Some(content) = load_template_content(&meta.id, meta.source) {
                        let mut st = s.borrow_mut();
                        st.create_project.tpl_selected_id = meta.id.clone();
                        on_source_loaded(&mut st, content.schema);
                    }
                }
            }
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
            }
        });
    }

    // 身份字段
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_id_edited(move |v| {
            {
                let mut st = s.borrow_mut();
                st.create_project.project_id = v.to_string();
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_name_edited(move |v| {
            s.borrow_mut().create_project.project_name = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_category_edited(move |v| {
            s.borrow_mut().create_project.project_category = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_version_edited(move |v| {
            s.borrow_mut().create_project.project_version = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // FromFile：浏览
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_file_browse(move || {
            let file = rfd::FileDialog::new()
                .add_filter("TblSchema", &["tblschema"])
                .pick_file();
            if let Some(path) = file {
                let path_str = path.display().to_string();
                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let mut st = s.borrow_mut();
                        st.create_project.file_path = path_str.clone();
                        st.create_project.file_error = format!("读取失败: {}", e);
                        st.create_project.file_schema = None;
                        st.create_project.picker_items.clear();
                        st.create_project.picker_checked.clear();
                        st.engine.error_log(format!("[新建项目] 读取失败: {}", e));
                        if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                        return;
                    }
                };
                let schema = match parse_tblschema(&content) {
                    Ok(sch) => sch,
                    Err(e) => {
                        let mut st = s.borrow_mut();
                        st.create_project.file_path = path_str.clone();
                        st.create_project.file_error = format!("解析失败: {}", e);
                        st.create_project.file_schema = None;
                        st.create_project.picker_items.clear();
                        st.create_project.picker_checked.clear();
                        st.engine.error_log(format!("[新建项目] 解析失败: {}", e));
                        if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                        return;
                    }
                };
                {
                    let mut st = s.borrow_mut();
                    st.create_project.file_path = path_str;
                    st.create_project.file_error.clear();
                    on_source_loaded(&mut st, schema);
                }
            }
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }

    // FromTemplate：切子 tab
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_tpl_set_subtab(move |i| {
            {
                let mut st = s.borrow_mut();
                if st.create_project.tpl_subtab != i {
                    st.create_project.tpl_subtab = i;
                    st.create_project.tpl_selected_id.clear();
                    st.create_project.tpl_schema = None;
                    st.create_project.tpl_meta_id.clear();
                    st.create_project.tpl_meta_version.clear();
                    st.create_project.picker_items.clear();
                    st.create_project.picker_checked.clear();
                    st.create_project.with_preset = false;
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // FromTemplate：搜索
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_tpl_search_edited(move |q| {
            s.borrow_mut().create_project.tpl_search = q.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // FromTemplate：选模板
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_tpl_select_item(move |idx| {
            // 过滤后的列表索引 → 找回 meta（再加载 schema）
            let chosen: Option<TemplateMeta> = {
                let st = s.borrow();
                let metas = list_template_metas(st.create_project.tpl_subtab);
                let q = st.create_project.tpl_search.to_lowercase();
                let filtered: Vec<TemplateMeta> = metas.into_iter()
                    .filter(|m| {
                        if q.is_empty() { return true; }
                        m.id.to_lowercase().contains(&q)
                            || m.name.to_lowercase().contains(&q)
                            || m.category.to_lowercase().contains(&q)
                    })
                    .collect();
                filtered.get(idx as usize).cloned()
            };
            if let Some(meta) = chosen {
                let content = load_template_content(&meta.id, meta.source);
                if let Some(content) = content {
                    let mut st = s.borrow_mut();
                    st.create_project.tpl_selected_id = meta.id.clone();
                    st.create_project.tpl_meta_id = content.meta.id.clone();
                    st.create_project.tpl_meta_version = content.meta.version.clone();
                    on_source_loaded(&mut st, content.schema);
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // sections-picker：全选 / 单项
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_picker_toggle_all(move |checked| {
            {
                let mut st = s.borrow_mut();
                let n = st.create_project.picker_items.len();
                if st.create_project.picker_checked.len() != n {
                    st.create_project.picker_checked = vec![checked; n];
                } else {
                    for i in 0..n {
                        if st.create_project.picker_items[i].indent == 1 {
                            st.create_project.picker_checked[i] = checked;
                        }
                    }
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_picker_toggle_item(move |idx| {
            {
                let mut st = s.borrow_mut();
                let i = idx as usize;
                if i >= st.create_project.picker_items.len() { return; }
                if st.create_project.picker_items[i].indent == 0 {
                    let n = st.create_project.picker_items.len();
                    let start = i + 1;
                    let mut end = n;
                    for j in (i + 1)..n {
                        if st.create_project.picker_items[j].indent == 0 { end = j; break; }
                    }
                    let any_unchecked = (start..end).any(|k|
                        !st.create_project.picker_checked.get(k).copied().unwrap_or(true));
                    let new_val = any_unchecked;
                    for k in start..end {
                        if k < st.create_project.picker_checked.len() {
                            st.create_project.picker_checked[k] = new_val;
                        }
                    }
                } else if i < st.create_project.picker_checked.len() {
                    st.create_project.picker_checked[i] = !st.create_project.picker_checked[i];
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // confirm / cancel
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_confirm(move || {
            // 同步 in-out 字段最新值
            if let Some(ui_h) = weak.upgrade() {
                let mut st = s.borrow_mut();
                st.create_project.open_after = ui_h.get_cp_open_after();
                st.create_project.with_preset = ui_h.get_cp_with_preset();
            }
            run(&s);
            s.borrow_mut().create_project.close();
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                refresh::after_tree_change(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_cp_cancel(move || {
            s.borrow_mut().create_project.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
