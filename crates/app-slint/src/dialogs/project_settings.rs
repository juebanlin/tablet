// 项目设置对话框：身份（id/name/category/version）+ 分隔符（25 leaves）2 tab。
//
// 所有改动只更新内存，标记 schema_dirty=true，等"保存项目"才写盘。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tablet_core::tblschema::is_valid_metadata_id;
use tablet_core::types::{SepKey, SeparatorsSection};

use crate::state::AppState;
use crate::{refresh, AppWindow};

/// 由项目右键「项目设置...」入口调用：把当前 project meta + separators 拷进 buf，开 dialog。
pub fn open_for(state: &Rc<RefCell<AppState>>, project_id: &str) {
    let mut st = state.borrow_mut();
    let Some(p) = st.engine.find_project(project_id) else {
        st.engine.ui_log(format!("[项目设置] 项目不存在: {}", project_id));
        return;
    };
    let id = p.schema.meta.id.clone();
    let name = p.schema.meta.name.clone();
    let category = p.schema.meta.category.clone();
    let version = p.schema.meta.version.clone();
    let sep = p.schema.separators.clone();
    // 仅未落盘项目（克隆 / 新建的内存项目）允许改 id；已存盘项目 id 固定。
    let id_editable = p.state.is_pending();
    let ps = &mut st.project_settings;
    ps.open = true;
    ps.tab = 0;
    ps.project_id = id.clone();
    ps.id_buf = id;
    ps.name_buf = name;
    ps.category_buf = category;
    ps.version_buf = version;
    ps.id_error.clear();
    ps.id_editable = id_editable;
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
    ui_h.set_ps_id_editable(ps.id_editable);
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

/// 「确定」逻辑。更新所有字段到内存，标记 dirty，等"保存项目"才写盘。
fn run(state: &Rc<RefCell<AppState>>) {
    // 取出 buf，先放下 borrow
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

    // 更新所有字段到内存（id / name / category / version / separators），
    // 只标记 dirty，等"保存项目"才写盘。
    let mut has_changes = false;
    {
        let mut st = state.borrow_mut();
        if let Some(p) = st.engine.find_project_mut(&old_id) {
            if p.schema.meta.id != new_id {
                p.schema.meta.id = new_id.clone();
                // 未落盘项目改 id 时需同步更新 project_root 路径
                // 子节点路径会在首次保存时由 save_project_files 自动推演
                if p.state.is_pending() {
                    p.project_root = p.workdir.join("projects").join(&new_id);
                }
                has_changes = true;
            }
            if p.schema.meta.name != name {
                p.schema.meta.name = name.clone();
                has_changes = true;
            }
            if p.schema.meta.category != category {
                p.schema.meta.category = category.clone();
                has_changes = true;
            }
            if p.schema.meta.version != version {
                p.schema.meta.version = version.clone();
                has_changes = true;
            }
            if p.schema.separators != sep {
                p.schema.separators = sep.clone();
                has_changes = true;
            }
            if has_changes {
                p.schema_dirty = true;
            }
        }
    }

    if has_changes {
        let mut st = state.borrow_mut();

        // ID 改变时，需要同步更新 engine 内部的所有索引结构
        let id_changed = old_id != new_id;
        if id_changed {
            // 更新 available_projects 列表中的 id
            if let Some(avail) = st.engine.available_projects.iter_mut().find(|a| a.id == old_id) {
                avail.id = new_id.clone();
            }
            // 更新 opened_projects 列表
            for opened_id in &mut st.engine.global_config.project_management.opened_projects {
                if opened_id == &old_id {
                    *opened_id = new_id.clone();
                }
            }
            // 更新 last_project（如果是当前项目）
            if st.engine.global_config.project_management.last_project == old_id {
                st.engine.global_config.project_management.last_project = new_id.clone();
            }
            // 更新 project_order 列表
            for order_id in &mut st.engine.global_config.project_management.project_order {
                if order_id == &old_id {
                    *order_id = new_id.clone();
                }
            }
            // 更新 validation_errors 中的 project_id
            let old_errors: Vec<_> = st.engine.validation_errors.iter()
                .filter(|(pid, _, _, _, _)| pid == &old_id)
                .cloned()
                .collect();
            for (_, g, n, r, c) in old_errors {
                st.engine.validation_errors.remove(&(old_id.clone(), g.clone(), n.clone(), r, c));
                st.engine.validation_errors.insert((new_id.clone(), g, n, r, c));
            }
        }

        // schema 改了（特别是 separators）→ 重新生成合并后的 ProjectConfig
        // 注意：ID 改变后要用新 ID 查找
        let target_id = if id_changed { &new_id } else { &old_id };
        st.engine.remerge_project_config(target_id);

        // 分隔符变了要全表重校验（active scope）：临时切到目标 project 跑 revalidate_all 再切回。
        let prev_active = st.engine.active_project_id().map(|s| s.to_string());
        if st.engine.set_active_by_id(target_id) {
            st.engine.revalidate_all();
        }
        match prev_active {
            Some(id) => { st.engine.set_active_by_id(&id); }
            None => st.engine.set_active_none(),
        }

        st.engine.ui_log(format!("[项目设置] 已应用修改: {}（需保存项目才写盘）", target_id));
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
