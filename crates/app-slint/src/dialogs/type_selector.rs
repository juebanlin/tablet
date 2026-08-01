// 类型选择器对话框：data type / reference 两个 tab + 参数槽 + 预览 + 引用列表。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tablet_core::types::{BaseType, Paradigm};

use crate::state::{
    AppState, SelectedNode, TsRefFilter, TsTab, TypeEditTarget,
};
use crate::theme::{ICON_ENUM, ICON_TABLE};
use crate::{refresh, AppWindow, RefItem, TsParamSlot};

/// Constant data cell type 列单击 → 打开 TypeSelector（编辑该数据格的 type）
pub(crate) fn open_for_cell(state: &Rc<RefCell<AppState>>, r: usize, c: usize) {
    let mut st = state.borrow_mut();
    let allow_const_ref = st.constant_ref_allowed;
    let (group, name, is_table, current) = match &st.selected {
        Some(SelectedNode::Constant { group, name, .. }) => {
            let cur = st.engine.find_constant(group, name)
                .and_then(|cst| cst.entries.get(r))
                .map(|e| e.tbl_type.clone())
                .unwrap_or_default();
            (group.clone(), name.clone(), false, cur)
        }
        Some(SelectedNode::Table { group, name, .. }) => {
            // Table 数据格不会是 TypeEnumCol，但保留兜底
            (group.clone(), name.clone(), true, String::new())
        }
        _ => return,
    };
    st.type_selector.open_with(&current, TypeEditTarget::CellType { row: r, col: c }, &group, &name, is_table, allow_const_ref);
}

/// Table 表头 type 行单击 → 打开 TypeSelector（编辑该列的 tbl_type）
pub(crate) fn open_for_header(state: &Rc<RefCell<AppState>>, col: usize) {
    let mut st = state.borrow_mut();
    let allow_const_ref = st.constant_ref_allowed;
    let (group, name, current) = match &st.selected {
        Some(SelectedNode::Table { group, name, .. }) => {
            let cur = st.engine.find_table(group, name)
                .and_then(|t| t.schema.fields.get(col))
                .map(|f| f.tbl_type.clone())
                .unwrap_or_default();
            (group.clone(), name.clone(), cur)
        }
        _ => return,
    };
    st.type_selector.open_with(&current, TypeEditTarget::HeaderType { col }, &group, &name, true, allow_const_ref);
}

/// 收集项目内可被引用的项（table + enum，排除 deleted），按名称排序。
fn collect_ref_targets(state: &AppState) -> Vec<(String, bool /* is_table */)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for g in &state.engine.project().groups {
        for t in &g.tables { if !t.deleted { out.push((t.name.clone(), true)); } }
        for e in &g.enums  { if !e.deleted { out.push((e.name.clone(), false)); } }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 把 TypeSelectorState 派生成 slint 端属性并 push。每次状态变化都重新调用。
pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let ts = &st.type_selector;
    ui_h.set_dlg_type_open(ts.open);
    if !ts.open { return; }

    ui_h.set_ts_tab(if ts.tab == TsTab::Reference { 1 } else { 0 });
    ui_h.set_ts_ref_disabled(ts.ref_disabled);

    let paradigms: Vec<slint::SharedString> = Paradigm::all_data().iter()
        .map(|p| slint::SharedString::from(p.label()))
        .collect();
    ui_h.set_ts_paradigms(slint::ModelRc::new(slint::VecModel::from(paradigms)));
    let paradigm_index = Paradigm::all_data().iter().position(|p| *p == ts.paradigm).unwrap_or(0) as i32;
    ui_h.set_ts_paradigm_index(paradigm_index);

    let slots = ts.paradigm.param_slots();
    let is_base = ts.paradigm == Paradigm::Base;
    let bt_all: Vec<slint::SharedString> = BaseType::all().iter().map(|b| slint::SharedString::from(b.name())).collect();
    let bt_nestable: Vec<slint::SharedString> = BaseType::all_nestable().iter().map(|b| slint::SharedString::from(b.name())).collect();
    let bt_keys: Vec<slint::SharedString> = BaseType::map_key_types().iter().map(|b| slint::SharedString::from(b.name())).collect();
    let mut ts_slots: Vec<TsParamSlot> = Vec::with_capacity(slots.len());
    for (i, slot) in slots.iter().enumerate() {
        let options = if slot.is_map_key { bt_keys.clone() } else if is_base { bt_all.clone() } else { bt_nestable.clone() };
        let cur = ts.params.get(i).copied().unwrap_or(BaseType::Int);
        let avail = if slot.is_map_key { BaseType::map_key_types() } else if is_base { BaseType::all() } else { BaseType::all_nestable() };
        let cur_idx = avail.iter().position(|b| *b == cur).unwrap_or(0) as i32;
        ts_slots.push(TsParamSlot {
            label: slint::SharedString::from(slot.label),
            type_options: slint::ModelRc::new(slint::VecModel::from(options)),
            selected_index: cur_idx,
        });
    }
    ui_h.set_ts_param_slots(slint::ModelRc::new(slint::VecModel::from(ts_slots)));

    let sep = st.engine.project().schema.separators.clone();
    let (preview, example, java, go, lua) = match ts.tab {
        TsTab::Data => {
            let t = ts.data_type();
            (t.to_type_string(), t.example_with_sep(&sep), t.java_decl(), t.go_decl(), t.lua_decl())
        }
        TsTab::Reference => {
            if let Some(t) = ts.ref_type() {
                (t.to_type_string(),
                 String::from("（id）"),
                 String::from("int"),
                 String::from("int32"),
                 String::from("number"))
            } else {
                (String::from("（未选择）"), String::new(), String::new(), String::new(), String::new())
            }
        }
    };
    ui_h.set_ts_result_preview(preview.into());
    ui_h.set_ts_example_value(example.into());
    ui_h.set_ts_java_decl(java.into());
    ui_h.set_ts_go_decl(go.into());
    ui_h.set_ts_lua_decl(lua.into());

    let search = ts.ref_search.to_lowercase();
    let targets = collect_ref_targets(&st);
    let items: Vec<RefItem> = targets.iter().filter(|(name, is_table)| {
        let kind_ok = match ts.ref_filter {
            TsRefFilter::All => true,
            TsRefFilter::Table => *is_table,
            TsRefFilter::Enum => !*is_table,
        };
        let search_ok = search.is_empty() || name.to_lowercase().contains(&search);
        kind_ok && search_ok
    }).map(|(name, is_table)| {
        let icon = if *is_table { ICON_TABLE } else { ICON_ENUM };
        let kind_label = if *is_table { "table" } else { "enum" };
        RefItem {
            icon: icon.into(),
            name: name.clone().into(),
            kind_label: kind_label.into(),
            selected: ts.ref_name == *name,
        }
    }).collect();
    ui_h.set_ts_ref_items(slint::ModelRc::new(slint::VecModel::from(items)));
    ui_h.set_ts_ref_search(ts.ref_search.clone().into());
    ui_h.set_ts_ref_filter(match ts.ref_filter {
        TsRefFilter::All => 0, TsRefFilter::Table => 1, TsRefFilter::Enum => 2,
    });
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // ts-set-tab：用户在 slint 端切 tab，Rust 同步并刷新预览/可见列表
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_set_tab(move |i| {
            let mut st = s.borrow_mut();
            // constant 禁用引用 tab：忽略切到 1 的请求
            if i == 1 && st.type_selector.ref_disabled { return; }
            st.type_selector.tab = if i == 1 { TsTab::Reference } else { TsTab::Data };
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_set_paradigm(move |i| {
            let mut st = s.borrow_mut();
            if let Some(p) = Paradigm::all_data().get(i as usize).cloned() {
                st.type_selector.paradigm = p;
                st.type_selector.sync_params();
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_set_param(move |slot, idx| {
            let mut st = s.borrow_mut();
            let slots = st.type_selector.paradigm.param_slots();
            if let Some(slot_def) = slots.get(slot as usize) {
                let avail = if slot_def.is_map_key {
                    BaseType::map_key_types()
                } else if st.type_selector.paradigm == Paradigm::Base {
                    BaseType::all()
                } else {
                    BaseType::all_nestable()
                };
                if let Some(bt) = avail.get(idx as usize).copied() {
                    if (slot as usize) < st.type_selector.params.len() {
                        st.type_selector.params[slot as usize] = bt;
                    }
                }
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_set_ref_search(move |t| {
            s.borrow_mut().type_selector.ref_search = t.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_set_ref_filter(move |i| {
            let f = match i { 1 => TsRefFilter::Table, 2 => TsRefFilter::Enum, _ => TsRefFilter::All };
            s.borrow_mut().type_selector.ref_filter = f;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // select-ref：用 filter+search 过滤后的索引去拿真名
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_select_ref(move |i| {
            let chosen: Option<String> = {
                let st = s.borrow();
                let ts = &st.type_selector;
                let search = ts.ref_search.to_lowercase();
                let targets = collect_ref_targets(&st);
                let filtered: Vec<&(String, bool)> = targets.iter().filter(|(name, is_table)| {
                    let kind_ok = match ts.ref_filter {
                        TsRefFilter::All => true,
                        TsRefFilter::Table => *is_table,
                        TsRefFilter::Enum => !*is_table,
                    };
                    let search_ok = search.is_empty() || name.to_lowercase().contains(&search);
                    kind_ok && search_ok
                }).collect();
                filtered.get(i as usize).map(|(n, _)| n.clone())
            };
            if let Some(name) = chosen {
                s.borrow_mut().type_selector.ref_name = name;
                if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
            }
        });
    }
    // ts-confirm：构造 type 字符串写回；slint 端会自动把 dlg-type-open 设 false
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_confirm(move || {
            let mut st = s.borrow_mut();
            let type_str = match st.type_selector.tab {
                TsTab::Data => st.type_selector.data_type().to_type_string(),
                TsTab::Reference => match st.type_selector.ref_type() {
                    Some(t) => t.to_type_string(),
                    None => { st.type_selector.close(); return; }
                }
            };
            let target = st.type_selector.target.clone();
            let group = st.type_selector.editing_group.clone();
            let name = st.type_selector.editing_name.clone();
            let is_table = st.type_selector.editing_source_table;
            match target {
                Some(TypeEditTarget::HeaderType { col }) => {
                    if is_table {
                        st.engine.commit_header_edit(&group, &name, 2, col, type_str);
                        if st.realtime_validate { st.engine.revalidate(&group, &name); }
                    }
                }
                Some(TypeEditTarget::CellType { row, col }) => {
                    if !is_table {
                        // Constant：col 应该是 1（type 列）
                        st.engine.commit_constant_cell(&group, &name, row, col, type_str);
                        if st.realtime_validate { st.engine.revalidate(&group, &name); }
                    }
                }
                None => {}
            }
            st.type_selector.close();
            drop(st);
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_grid_edit(&ui_h, &s);
                push(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ts_cancel(move || {
            s.borrow_mut().type_selector.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
