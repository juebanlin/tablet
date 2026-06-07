// Schema 导出 / 导入对话框：列出 group + 子节点的 tristate 复选树。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::{self, AppState};
use crate::theme::{ICON_CONST, ICON_ENUM, ICON_GROUP, ICON_TABLE};
use crate::{refresh, ui, AppWindow, SchemaItem};

/// 把当前 project 的 groups/tables/constants/enums 扁平化为 SchemaExportItem 列表，
/// 默认勾选全部（Schema 导出对话框打开时调用）。
/// metadata 字段从当前 project schema 预填，用户可在对话框里改。
/// mode 由调用方在打开对话框前单独设置（默认 Schema）。
pub(crate) fn rebuild_export_items(st: &mut AppState) {
    let mut items: Vec<state::SchemaExportItem> = Vec::new();
    for g in &st.engine.project().groups {
        let mut sub: Vec<state::SchemaExportItem> = Vec::new();
        for t in &g.tables {
            if t.deleted { continue; }
            sub.push(state::SchemaExportItem { indent: 1, group: g.name.clone(), name: t.name.clone(), is_table: true });
        }
        for c in &g.constants {
            if c.deleted { continue; }
            sub.push(state::SchemaExportItem { indent: 1, group: g.name.clone(), name: c.name.clone(), is_table: false });
        }
        for e in &g.enums {
            if e.deleted { continue; }
            sub.push(state::SchemaExportItem { indent: 1, group: g.name.clone(), name: e.name.clone(), is_table: false });
        }
        if sub.is_empty() { continue; }
        items.push(state::SchemaExportItem { indent: 0, group: g.name.clone(), name: g.name.clone(), is_table: false });
        items.extend(sub);
    }
    st.schema_export.checked = vec![true; items.len()];
    st.schema_export.items = items;
    // 每次打开对话框默认不带预设：导出"结构骨架"是更常见路径
    st.schema_export.with_preset = false;
    // metadata 预填当前项目值，用户可改
    let meta = &st.engine.project().schema.meta;
    st.schema_export.meta_id = meta.id.clone();
    st.schema_export.meta_name = meta.name.clone();
    st.schema_export.meta_category = meta.category.clone();
    st.schema_export.meta_version = meta.version.clone();
}

/// 把 SchemaExportState 推到 slint 端：组节点 tristate 由其下子节点聚合。
pub fn push_export(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let sx = &st.schema_export;
    ui_h.set_dlg_schema_export_open(sx.open);
    if !sx.open { return; }

    let n = sx.items.len();
    let mut group_ranges: Vec<(usize, usize, usize)> = Vec::new(); // (group_idx, start, end)
    let mut i = 0;
    while i < n {
        if sx.items[i].indent == 0 {
            let group_idx = i;
            let mut j = i + 1;
            while j < n && sx.items[j].indent != 0 { j += 1; }
            group_ranges.push((group_idx, i + 1, j));
            i = j;
        } else { i += 1; }
    }

    let mut slint_items: Vec<SchemaItem> = Vec::with_capacity(n);
    for (idx, item) in sx.items.iter().enumerate() {
        let (checked, tristate, icon) = if item.indent == 0 {
            let (_, start, end) = group_ranges.iter().find(|(g, _, _)| *g == idx).copied().unwrap_or((idx, idx + 1, idx + 1));
            let mut all = true;
            let mut any = false;
            for k in start..end {
                if sx.checked.get(k).copied().unwrap_or(false) { any = true; } else { all = false; }
            }
            (all && start < end, any && !all, ICON_GROUP.to_string())
        } else {
            let icon = if item.is_table { ICON_TABLE } else { ICON_CONST };
            (sx.checked.get(idx).copied().unwrap_or(false), false, icon.to_string())
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

    let total: i32 = sx.items.iter().filter(|it| it.indent == 1).count() as i32;
    let selected: i32 = sx.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && sx.checked.get(*i).copied().unwrap_or(false))
        .count() as i32;
    let all_checked = selected == total && total > 0;

    ui_h.set_sx_items(slint::ModelRc::new(slint::VecModel::from(slint_items)));
    ui_h.set_sx_all_checked(all_checked);
    ui_h.set_sx_selected_count(selected);
    ui_h.set_sx_total_count(total);
    ui_h.set_sx_with_preset(sx.with_preset);
    ui_h.set_sx_mode(match sx.mode {
        state::SchemaExportMode::Schema => "schema".into(),
        state::SchemaExportMode::Template => "template".into(),
    });
    ui_h.set_sx_meta_id(sx.meta_id.clone().into());
    ui_h.set_sx_meta_name(sx.meta_name.clone().into());
    ui_h.set_sx_meta_category(sx.meta_category.clone().into());
    ui_h.set_sx_meta_version(sx.meta_version.clone().into());
}

/// 把 SchemaImportState 推到 slint 端：file_loaded / items / 冲突计数。
pub fn push_import(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    use tablet_core::tblschema::SchemaMode;
    let st = state.borrow();
    let si = &st.schema_import;
    ui_h.set_dlg_schema_import_open(si.open);
    if !si.open { return; }

    ui_h.set_si_file_path(si.file_path.clone().into());
    let file_loaded = si.schema.is_some();
    ui_h.set_si_file_loaded(file_loaded);
    if !file_loaded {
        ui_h.set_si_items(slint::ModelRc::new(slint::VecModel::from(Vec::<SchemaItem>::new())));
        ui_h.set_si_all_checked(false);
        ui_h.set_si_selected_count(0);
        ui_h.set_si_total_count(0);
        ui_h.set_si_conflict_count(0);
        return;
    }

    let n = si.items.len();
    let mut group_ranges: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if si.items[i].indent == 0 {
            let group_idx = i;
            let mut j = i + 1;
            while j < n && si.items[j].indent != 0 { j += 1; }
            group_ranges.push((group_idx, i + 1, j));
            i = j;
        } else { i += 1; }
    }

    let mut slint_items: Vec<SchemaItem> = Vec::with_capacity(n);
    for (idx, item) in si.items.iter().enumerate() {
        let (checked, tristate, icon, is_conflict) = if item.indent == 0 {
            let (_, start, end) = group_ranges.iter().find(|(g, _, _)| *g == idx).copied().unwrap_or((idx, idx + 1, idx + 1));
            let mut all = true;
            let mut any = false;
            for k in start..end {
                if si.checked.get(k).copied().unwrap_or(false) { any = true; } else { all = false; }
            }
            (all && start < end, any && !all, ICON_GROUP.to_string(), false)
        } else {
            let icon = match item.mode {
                SchemaMode::Table => ICON_TABLE,
                SchemaMode::Constant => ICON_CONST,
                SchemaMode::Enum => ICON_ENUM,
            };
            (
                si.checked.get(idx).copied().unwrap_or(false),
                false,
                icon.to_string(),
                si.conflicts.get(idx).copied().unwrap_or(false),
            )
        };
        slint_items.push(SchemaItem {
            indent: item.indent as i32,
            icon: icon.into(),
            name: item.name.clone().into(),
            group_name: item.group.clone().into(),
            checked,
            tristate,
            is_conflict,
        });
    }

    let total: i32 = si.items.iter().filter(|it| it.indent == 1).count() as i32;
    let selected: i32 = si.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && si.checked.get(*i).copied().unwrap_or(false))
        .count() as i32;
    let conflict: i32 = si.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1
            && si.checked.get(*i).copied().unwrap_or(false)
            && si.conflicts.get(*i).copied().unwrap_or(false))
        .count() as i32;
    let all_checked = selected == total && total > 0;
    let has_preset = si.schema.as_ref().map(|s| s.meta.has_preset).unwrap_or(false);

    ui_h.set_si_items(slint::ModelRc::new(slint::VecModel::from(slint_items)));
    ui_h.set_si_all_checked(all_checked);
    ui_h.set_si_selected_count(selected);
    ui_h.set_si_total_count(total);
    ui_h.set_si_conflict_count(conflict);
    ui_h.set_si_has_preset(has_preset);
    ui_h.set_si_with_preset(si.with_preset);
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // sx-toggle-all
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_sx_toggle_all(move |checked| {
            {
                let mut st = s.borrow_mut();
                let n = st.schema_export.items.len();
                if st.schema_export.checked.len() != n {
                    st.schema_export.checked = vec![checked; n];
                } else {
                    for i in 0..n {
                        if st.schema_export.items[i].indent == 1 {
                            st.schema_export.checked[i] = checked;
                        }
                    }
                }
            }
            if let Some(ui_h) = weak.upgrade() { push_export(&ui_h, &s); }
        });
    }
    // sx-toggle-item（点击单项；点击组行 flip 整组）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_sx_toggle_item(move |idx| {
            {
                let mut st = s.borrow_mut();
                let i = idx as usize;
                if i >= st.schema_export.items.len() { return; }
                if st.schema_export.items[i].indent == 0 {
                    let n = st.schema_export.items.len();
                    let mut start = i + 1;
                    let mut end = n;
                    for j in (i + 1)..n {
                        if st.schema_export.items[j].indent == 0 { end = j; break; }
                    }
                    if start > n { start = n; }
                    let any_unchecked = (start..end).any(|k|
                        !st.schema_export.checked.get(k).copied().unwrap_or(true));
                    let new_val = any_unchecked;
                    for k in start..end {
                        if k < st.schema_export.checked.len() {
                            st.schema_export.checked[k] = new_val;
                        }
                    }
                } else if i < st.schema_export.checked.len() {
                    st.schema_export.checked[i] = !st.schema_export.checked[i];
                }
            }
            if let Some(ui_h) = weak.upgrade() { push_export(&ui_h, &s); }
        });
    }
    // sx-confirm
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_sx_confirm(move || {
            // 把 slint 端的最新值同步回 Rust state，再跑 export
            if let Some(ui_h) = weak.upgrade() {
                let mut st = s.borrow_mut();
                st.schema_export.with_preset = ui_h.get_sx_with_preset();
                st.schema_export.meta_id = ui_h.get_sx_meta_id().to_string();
                st.schema_export.meta_name = ui_h.get_sx_meta_name().to_string();
                st.schema_export.meta_category = ui_h.get_sx_meta_category().to_string();
                st.schema_export.meta_version = ui_h.get_sx_meta_version().to_string();
            }
            run_export(&s);
            s.borrow_mut().schema_export.open = false;
            if let Some(ui_h) = weak.upgrade() {
                push_export(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }
    // sx-cancel：同步 Rust 端 open，否则下次刷新（ctx fan-out 等）会把对话框重新推开
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_sx_cancel(move || {
            s.borrow_mut().schema_export.open = false;
            if let Some(ui_h) = weak.upgrade() { push_export(&ui_h, &s); }
        });
    }
    // si-browse-file
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_si_browse_file(move || {
            let file = rfd::FileDialog::new()
                .add_filter("TblSchema", &["tblschema"])
                .pick_file();
            if let Some(path) = file {
                let path_str = path.display().to_string();
                load_import(&s, &path_str);
            }
            if let Some(ui_h) = weak.upgrade() {
                push_import(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }
    // si-toggle-all
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_si_toggle_all(move |checked| {
            {
                let mut st = s.borrow_mut();
                let n = st.schema_import.items.len();
                if st.schema_import.checked.len() != n {
                    st.schema_import.checked = vec![checked; n];
                } else {
                    for i in 0..n {
                        if st.schema_import.items[i].indent == 1 {
                            st.schema_import.checked[i] = checked;
                        }
                    }
                }
            }
            if let Some(ui_h) = weak.upgrade() { push_import(&ui_h, &s); }
        });
    }
    // si-toggle-item
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_si_toggle_item(move |idx| {
            {
                let mut st = s.borrow_mut();
                let i = idx as usize;
                if i >= st.schema_import.items.len() { return; }
                if st.schema_import.items[i].indent == 0 {
                    let n = st.schema_import.items.len();
                    let start = i + 1;
                    let mut end = n;
                    for j in (i + 1)..n {
                        if st.schema_import.items[j].indent == 0 { end = j; break; }
                    }
                    let any_unchecked = (start..end).any(|k|
                        !st.schema_import.checked.get(k).copied().unwrap_or(true));
                    let new_val = any_unchecked;
                    for k in start..end {
                        if k < st.schema_import.checked.len() {
                            st.schema_import.checked[k] = new_val;
                        }
                    }
                } else if i < st.schema_import.checked.len() {
                    st.schema_import.checked[i] = !st.schema_import.checked[i];
                }
            }
            if let Some(ui_h) = weak.upgrade() { push_import(&ui_h, &s); }
        });
    }
    // si-confirm
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_si_confirm(move || {
            // 把 slint 端 with-preset 的最新值同步回 Rust state，再跑 import
            if let Some(ui_h) = weak.upgrade() {
                s.borrow_mut().schema_import.with_preset = ui_h.get_si_with_preset();
            }
            run_import(&s);
            s.borrow_mut().schema_import.open = false;
            if let Some(ui_h) = weak.upgrade() {
                ui::tree::push(&ui_h, &s);
                ui::grid::push(&ui_h, &s);
                push_import(&ui_h, &s);
                refresh::after_log(&ui_h, &s);
            }
        });
    }
}

/// Schema 导出：把当前勾选项 → SchemaSection → serialize → 落盘。
/// - mode=Schema   → rfd 选路径保存（meta 留空时不写 # @meta 块）
/// - mode=Template → 写到 default_local_dir()/<id>.tblschema（id 必填，已在 UI 层校验）
fn run_export(state: &Rc<RefCell<AppState>>) {
    use tablet_core::tblschema::{
        is_valid_metadata_id, schema_from_project, serialize_tblschema,
        SchemaMetadata, TblSchema,
    };
    use tablet_core::template::default_local_dir;

    let (selected, full_schema, mode, meta) = {
        let st = state.borrow();
        let full = schema_from_project(&st.engine.project().groups, st.schema_export.with_preset);
        let selected: Vec<(String, String)> = st.schema_export.items.iter().enumerate()
            .filter(|(i, it)| it.indent == 1 && st.schema_export.checked.get(*i).copied().unwrap_or(false))
            .map(|(_, it)| (it.group.clone(), it.name.clone()))
            .collect();
        let mut meta = SchemaMetadata::default();
        meta.id = st.schema_export.meta_id.trim().to_string();
        meta.name = st.schema_export.meta_name.trim().to_string();
        meta.category = st.schema_export.meta_category.trim().to_string();
        meta.version = st.schema_export.meta_version.trim().to_string();
        // name 缺省：用 id 兜底（与 tblschema 解析侧默认行为一致）
        if meta.name.is_empty() && !meta.id.is_empty() {
            meta.name = meta.id.clone();
        }
        (selected, full, st.schema_export.mode.clone(), meta)
    };

    let mut sections = Vec::new();
    for (g, n) in &selected {
        if let Some(sec) = full_schema.sections.iter().find(|s| &s.group == g && &s.name == n) {
            sections.push(sec.clone());
        }
    }
    let schema = TblSchema {
        meta: meta.clone(),
        separators: Default::default(),
        sections,
    };
    let content = serialize_tblschema(&schema);

    match mode {
        state::SchemaExportMode::Schema => {
            let suggested = if meta.id.is_empty() {
                "export.tblschema".to_string()
            } else {
                format!("{}.tblschema", meta.id)
            };
            let file = rfd::FileDialog::new()
                .add_filter("TblSchema", &["tblschema"])
                .set_file_name(&suggested)
                .save_file();
            if let Some(path) = file {
                match std::fs::write(&path, &content) {
                    Ok(_) => state.borrow_mut().engine.log(format!("[导出Schema] 已保存到 {}", path.display())),
                    Err(e) => state.borrow_mut().engine.log(format!("[导出Schema] 写入失败: {}", e)),
                }
            }
        }
        state::SchemaExportMode::Template => {
            // id 必填且需合法（UI 层只校验非空，这里再把字符集卡一次）
            if meta.id.is_empty() {
                state.borrow_mut().engine.log("[导出为本地模板] 失败：模板 ID 为空".to_string());
                return;
            }
            if !is_valid_metadata_id(&meta.id) {
                state.borrow_mut().engine.log(format!(
                    "[导出为本地模板] 失败：模板 ID '{}' 非法（仅小写字母/数字/下划线/连字符，长度 1-32）",
                    meta.id
                ));
                return;
            }
            let dir = default_local_dir();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                state.borrow_mut().engine.log(format!(
                    "[导出为本地模板] 创建目录失败 {}: {}", dir.display(), e
                ));
                return;
            }
            let path = dir.join(format!("{}.tblschema", meta.id));
            let existed = path.exists();
            match std::fs::write(&path, &content) {
                Ok(_) => state.borrow_mut().engine.log(format!(
                    "[导出为本地模板] 已{}保存到 {}",
                    if existed { "覆盖" } else { "" },
                    path.display()
                )),
                Err(e) => state.borrow_mut().engine.log(format!(
                    "[导出为本地模板] 写入失败 {}: {}", path.display(), e
                )),
            }
        }
    }
}

/// Schema 导入：读 file_path → parse → 填充 items/checked/conflicts。
fn load_import(state: &Rc<RefCell<AppState>>, file_path: &str) {
    use tablet_core::tblschema::{parse_tblschema, SchemaMode};
    let mut st = state.borrow_mut();
    st.schema_import.file_path = file_path.to_string();
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            st.engine.log(format!("[导入Schema] 读取失败: {}", e));
            st.schema_import.schema = None;
            st.schema_import.items.clear();
            st.schema_import.checked.clear();
            st.schema_import.conflicts.clear();
            return;
        }
    };
    let schema = match parse_tblschema(&content) {
        Ok(s) => s,
        Err(e) => {
            st.engine.log(format!("[导入Schema] 解析失败: {}", e));
            st.schema_import.schema = None;
            st.schema_import.items.clear();
            st.schema_import.checked.clear();
            st.schema_import.conflicts.clear();
            return;
        }
    };

    // 按 group 分段，并计算 conflict（已存在）
    let mut grouped: Vec<(String, Vec<(String, SchemaMode)>)> = Vec::new();
    for sec in &schema.sections {
        if let Some(entry) = grouped.iter_mut().find(|(g, _)| *g == sec.group) {
            entry.1.push((sec.name.clone(), sec.mode.clone()));
        } else {
            grouped.push((sec.group.clone(), vec![(sec.name.clone(), sec.mode.clone())]));
        }
    }

    let mut items: Vec<state::SchemaImportItem> = Vec::new();
    let mut checked: Vec<bool> = Vec::new();
    let mut conflicts: Vec<bool> = Vec::new();
    let groups = &st.engine.project().groups;
    for (g, secs) in &grouped {
        items.push(state::SchemaImportItem { indent: 0, group: g.clone(), name: g.clone(), mode: SchemaMode::Table });
        checked.push(true);
        conflicts.push(false);
        for (name, mode) in secs {
            let exists = if let Some(grp) = groups.iter().find(|gr| &gr.name == g) {
                match mode {
                    SchemaMode::Table => grp.tables.iter().any(|t| &t.name == name && !t.deleted),
                    SchemaMode::Constant => grp.constants.iter().any(|c| &c.name == name && !c.deleted),
                    SchemaMode::Enum => grp.enums.iter().any(|e| &e.name == name && !e.deleted),
                }
            } else { false };
            items.push(state::SchemaImportItem { indent: 1, group: g.clone(), name: name.clone(), mode: mode.clone() });
            checked.push(true);
            conflicts.push(exists);
        }
    }
    st.schema_import.items = items;
    st.schema_import.checked = checked;
    st.schema_import.conflicts = conflicts;
    // schema 自带 preset 时默认勾选「灌入预设」；不带则关掉
    st.schema_import.with_preset = schema.meta.has_preset;
    st.schema_import.schema = Some(schema);
}

/// Schema 导入：把当前选中的 sections 应用到 project。
fn run_import(state: &Rc<RefCell<AppState>>) {
    use tablet_core::tblschema::apply_schema_to_project;
    let mut st = state.borrow_mut();
    let schema = match st.schema_import.schema.clone() { Some(s) => s, None => return };
    let selected: Vec<(String, String)> = st.schema_import.items.iter().enumerate()
        .filter(|(i, it)| it.indent == 1 && st.schema_import.checked.get(*i).copied().unwrap_or(false))
        .map(|(_, it)| (it.group.clone(), it.name.clone()))
        .collect();
    let sections: Vec<_> = schema.sections.iter()
        .filter(|s| selected.iter().any(|(g, n)| g == &s.group && n == &s.name))
        .cloned().collect();
    let config_dir = st.engine.project().data_dir();
    let with_preset = st.schema_import.with_preset;
    let (added, overwritten) = apply_schema_to_project(
        &mut st.engine.project_mut().groups,
        &sections,
        &config_dir,
        with_preset,
    );
    st.engine.log(format!("[导入Schema] 完成: {} 新增, {} 覆盖", added, overwritten));
}
