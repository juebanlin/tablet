// 项目设置对话框：身份（id/name/category/version）+ 分隔符（25 leaves）2 tab。
//
// id 改 → 复用 engine ProjectAction::RenameProject（含目录 rename + schema 写盘）；
// 其它身份字段 + 分隔符 → 直接落 schema.meta + schema.separators，schema_dirty=true，
// serialize_tblschema 写盘 + revalidate_all。
//
// 对应 plan §7（docs/plans/分隔符配置内嵌schema-与项目设置对话框.md）。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tbl_core::tblschema::is_valid_metadata_id;
use tbl_core::types::{SepKey, SeparatorsSection};

use crate::state::AppState;
use crate::{refresh, AppWindow};

/// 由项目右键「项目设置...」入口调用：把当前 project meta + separators 拷进 buf，开 dialog。
pub fn open_for(state: &Rc<RefCell<AppState>>, project_id: &str) {
    let mut st = state.borrow_mut();
    let Some(p) = st.engine.find_project(project_id) else {
        st.engine.log(format!("[项目设置] 项目不存在: {}", project_id));
        return;
    };
    let id = p.schema.meta.id.clone();
    let name = p.schema.meta.name.clone();
    let category = p.schema.meta.category.clone();
    let version = p.schema.meta.version.clone();
    let sep = p.schema.separators.clone();
    let ps = &mut st.project_settings;
    ps.open = true;
    ps.tab = 0;
    ps.project_id = id.clone();
    ps.id_buf = id;
    ps.name_buf = name;
    ps.category_buf = category;
    ps.version_buf = version;
    ps.id_error.clear();
    ps.sep = sep;
}

/// 计算 id 校验消息。空 = 合法。
fn validate_id(st: &AppState, new_id: &str, old_id: &str) -> String {
    if new_id == old_id {
        return String::new();
    }
    if new_id.is_empty() {
        return "Project ID 不能为空".to_string();
    }
    if !is_valid_metadata_id(new_id) {
        return "ID 仅允许小写字母 / 数字 / _ / -，长度 1..=32".to_string();
    }
    let in_opened = st.engine.projects.iter().any(|p| p.schema.meta.id == new_id);
    let in_available = st.engine.available_projects.iter().any(|a| a.id == new_id);
    if in_opened || in_available {
        return format!("ID 已存在: {}", new_id);
    }
    String::new()
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    // 实时校验 id（可能因 buf 改动）
    let id_err = validate_id(&st, &st.project_settings.id_buf.clone(), &st.project_settings.project_id.clone());
    st.project_settings.id_error = id_err.clone();
    let ps = &st.project_settings;

    ui_h.set_dlg_ps_open(ps.open);
    if !ps.open {
        return;
    }
    ui_h.set_ps_tab(ps.tab);
    ui_h.set_ps_id_buf(ps.id_buf.clone().into());
    ui_h.set_ps_name_buf(ps.name_buf.clone().into());
    ui_h.set_ps_category_buf(ps.category_buf.clone().into());
    ui_h.set_ps_version_buf(ps.version_buf.clone().into());
    ui_h.set_ps_id_error(id_err.clone().into());
    let can_confirm = id_err.is_empty();
    ui_h.set_ps_can_confirm(can_confirm);

    let s = &ps.sep;
    ui_h.set_ps_sep_tuple2(s.tuple2.clone().into());
    ui_h.set_ps_sep_tuple3(s.tuple3.clone().into());
    ui_h.set_ps_sep_tuple4(s.tuple4.clone().into());
    ui_h.set_ps_sep_list(s.list.clone().into());
    ui_h.set_ps_sep_set(s.set.clone().into());
    ui_h.set_ps_sep_map_kv(s.map.kv.clone().into());
    ui_h.set_ps_sep_map_entry(s.map.entry.clone().into());
    ui_h.set_ps_sep_lt2_tuple(s.list_tuple2.tuple.clone().into());
    ui_h.set_ps_sep_lt2_list(s.list_tuple2.list.clone().into());
    ui_h.set_ps_sep_lt3_tuple(s.list_tuple3.tuple.clone().into());
    ui_h.set_ps_sep_lt3_list(s.list_tuple3.list.clone().into());
    ui_h.set_ps_sep_lt4_tuple(s.list_tuple4.tuple.clone().into());
    ui_h.set_ps_sep_lt4_list(s.list_tuple4.list.clone().into());
    ui_h.set_ps_sep_mt2_kv(s.map_tuple2.kv.clone().into());
    ui_h.set_ps_sep_mt2_tuple(s.map_tuple2.tuple.clone().into());
    ui_h.set_ps_sep_mt2_entry(s.map_tuple2.entry.clone().into());
    ui_h.set_ps_sep_mt3_kv(s.map_tuple3.kv.clone().into());
    ui_h.set_ps_sep_mt3_tuple(s.map_tuple3.tuple.clone().into());
    ui_h.set_ps_sep_mt3_entry(s.map_tuple3.entry.clone().into());
    ui_h.set_ps_sep_mt4_kv(s.map_tuple4.kv.clone().into());
    ui_h.set_ps_sep_mt4_tuple(s.map_tuple4.tuple.clone().into());
    ui_h.set_ps_sep_mt4_entry(s.map_tuple4.entry.clone().into());
    ui_h.set_ps_sep_ml_kv(s.map_list.kv.clone().into());
    ui_h.set_ps_sep_ml_item(s.map_list.item.clone().into());
    ui_h.set_ps_sep_ml_entry(s.map_list.entry.clone().into());
}

/// `# @sep` 行 key → 写到 SeparatorsSection 的对应字段。
fn apply_sep_kv(sep: &mut SeparatorsSection, key: &str, value: &str) {
    if let Some(k) = SepKey::from_directive_key(key) {
        k.set(sep, value.to_string());
    }
}

/// 「确定」逻辑。聚合 id rename + meta + separators，一次写盘。
fn run(state: &Rc<RefCell<AppState>>) {
    use tbl_core::ops::ProjectAction;

    // 取出 buf，先放下 borrow（execute_action 内部要 borrow_mut）
    let (old_id, new_id, name, category, version, sep) = {
        let st = state.borrow();
        let ps = &st.project_settings;
        (
            ps.project_id.clone(),
            ps.id_buf.clone(),
            ps.name_buf.clone(),
            ps.category_buf.clone(),
            ps.version_buf.clone(),
            ps.sep.clone(),
        )
    };

    // id 或 name 改了 → 走 RenameProject（同时承担目录 rename 与 schema 写盘）
    let id_changed = old_id != new_id;
    let name_changed = {
        let st = state.borrow();
        st.engine.find_project(&old_id).map(|p| p.schema.meta.name != name).unwrap_or(false)
    };
    if id_changed || name_changed {
        let mut st = state.borrow_mut();
        st.engine.execute_action(&ProjectAction::RenameProject {
            old_id: old_id.clone(),
            new_id: new_id.clone(),
            new_name: name.clone(),
        });
        // RenameProject 失败时（id 重名 / 目录 rename 失败）会保留旧 id；
        // 后续仍按 new_id 找项目可能拿不到，统一用 find_by_id 兜底。
    }

    // 此时 project 真正的 id（可能 rename 失败，故重新查一次）
    let effective_id = {
        let st = state.borrow();
        if st.engine.find_project(&new_id).is_some() {
            new_id.clone()
        } else {
            old_id.clone()
        }
    };

    // 改 category / version / separators，并写盘
    let mut wrote = false;
    let mut write_err: Option<String> = None;
    {
        let mut st = state.borrow_mut();
        if let Some(p) = st.engine.find_project_mut(&effective_id) {
            let mut dirty = false;
            if p.schema.meta.category != category {
                p.schema.meta.category = category.clone();
                dirty = true;
            }
            if p.schema.meta.version != version {
                p.schema.meta.version = version.clone();
                dirty = true;
            }
            if p.schema.separators != sep {
                p.schema.separators = sep.clone();
                p.config.separators = sep.clone();
                dirty = true;
            }
            if dirty {
                p.schema_dirty = true;
                let schema_path = p.project_root.join(tbl_core::project::PROJECT_SCHEMA_FILE);
                let txt = tbl_core::tblschema::serialize_tblschema(&p.schema);
                match std::fs::write(&schema_path, txt) {
                    Ok(_) => {
                        p.schema_dirty = false;
                        wrote = true;
                    }
                    Err(e) => {
                        write_err = Some(format!("[项目设置] 写 project.tblschema 失败: {}", e));
                    }
                }
            }
        }
    }
    if let Some(msg) = write_err {
        state.borrow_mut().engine.log(msg);
    }

    if wrote {
        let mut st = state.borrow_mut();
        // 分隔符变了要全表重校验（active scope）：临时切到目标 project 跑 revalidate_all 再切回。
        let prev_active = st.engine.active_project_id().map(|s| s.to_string());
        if st.engine.set_active_by_id(&effective_id) {
            st.engine.revalidate_all();
        }
        match prev_active {
            Some(id) => { st.engine.set_active_by_id(&id); }
            None => st.engine.set_active_none(),
        }
        st.engine.log(format!("[项目设置] 已保存 {}", effective_id));
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // tab 切换
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_tab_changed(move |i| {
            s.borrow_mut().project_settings.tab = i;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 身份字段 edited
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_id_edited(move |v| {
            s.borrow_mut().project_settings.id_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_name_edited(move |v| {
            s.borrow_mut().project_settings.name_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_category_edited(move |v| {
            s.borrow_mut().project_settings.category_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_version_edited(move |v| {
            s.borrow_mut().project_settings.version_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 分隔符任一字段 edited（slint 端统一通过 sep-edited(key, value) 上来）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_sep_edited(move |key, value| {
            apply_sep_kv(&mut s.borrow_mut().project_settings.sep, &key, &value);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 恢复默认（分隔符 tab）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_reset_sep(move || {
            s.borrow_mut().project_settings.sep = SeparatorsSection::default();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 确定 / 取消
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_confirm(move || {
            // 防止 id 校验未通过时被 enter 触发
            let ok = {
                let st = s.borrow();
                st.project_settings.id_error.is_empty()
            };
            if !ok {
                if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                return;
            }
            run(&s);
            s.borrow_mut().project_settings.close();
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                refresh::after_tree_change(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_cancel(move || {
            s.borrow_mut().project_settings.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
