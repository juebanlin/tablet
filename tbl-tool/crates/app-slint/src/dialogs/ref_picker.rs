// 引用选择器对话框：被引用项的列表 + id 列 + 辅助列 + 手动输入。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::{AppState, RefDisplayStrategy, SelectedNode};
use crate::{refresh, AppWindow, RefHeader, RefRow};

/// 被引用项（Table 或 Enum）的可选条目集合，含表头与每行 (id, extras)。
/// extras 与 headers 等长，不含 id 列；id 始终是第一列。
struct RefTargetData {
    is_table: bool,
    /// 辅助列表头：(field, desc)；不含 id 列
    headers: Vec<(String, String)>,
    /// id 列自身的 (field, desc)
    id_header: (String, String),
    rows: Vec<(String, Vec<String>)>,
}

/// auto 策略：跳过 id / export=- / 类型以 @ 开头的引用列 / 类型含 < 的复合列；最多取 2 个辅助列。
fn pick_auto_extras(table: &tbl_core::model::Table) -> Vec<usize> {
    use tbl_core::model::Export;
    const MAX_EXTRAS: usize = 2;
    let mut picked = Vec::new();
    for (idx, f) in table.schema.fields.iter().enumerate() {
        if f.name == "id" { continue; }
        if matches!(f.export, Export::None) { continue; }
        let t = f.tbl_type.trim();
        if t.starts_with('@') { continue; }
        if t.contains('<') { continue; }
        picked.push(idx);
        if picked.len() >= MAX_EXTRAS { break; }
    }
    picked
}

/// full 策略：除 id 外所有 export != "-" 的列。
fn pick_full_extras(table: &tbl_core::model::Table) -> Vec<usize> {
    use tbl_core::model::Export;
    table.schema.fields.iter().enumerate()
        .filter(|(_, f)| f.name != "id" && !matches!(f.export, Export::None))
        .map(|(idx, _)| idx)
        .collect()
}

/// 收集被引用项 (table or enum) 的所有可选条目；返回 None 表示该 ref_name 不存在。
fn collect_ref_rows(state: &AppState, ref_name: &str, strategy: RefDisplayStrategy) -> Option<RefTargetData> {
    for g in &state.engine.project().groups {
        for t in &g.tables {
            if t.deleted || t.name != ref_name { continue; }
            let id_idx = match t.schema.fields.iter().position(|f| f.name == "id") {
                Some(i) => i,
                None => return Some(RefTargetData {
                    is_table: true,
                    headers: vec![],
                    id_header: ("id".to_string(), String::new()),
                    rows: vec![],
                }),
            };
            let id_header = (
                t.schema.fields[id_idx].name.clone(),
                t.schema.fields[id_idx].desc.clone(),
            );
            let extras_idx = match strategy {
                RefDisplayStrategy::Auto => pick_auto_extras(t),
                RefDisplayStrategy::Full => pick_full_extras(t),
            };
            let headers: Vec<(String, String)> = extras_idx.iter()
                .map(|&i| (t.schema.fields[i].name.clone(), t.schema.fields[i].desc.clone()))
                .collect();
            let rows: Vec<(String, Vec<String>)> = t.records.iter()
                .filter_map(|row| {
                    let id = row.get(id_idx).cloned().unwrap_or_default();
                    if id.is_empty() { return None; }
                    let extras: Vec<String> = extras_idx.iter()
                        .map(|&i| row.get(i).cloned().unwrap_or_default())
                        .collect();
                    Some((id, extras))
                })
                .collect();
            return Some(RefTargetData { is_table: true, headers, id_header, rows });
        }
        for e in &g.enums {
            if e.deleted || e.name != ref_name { continue; }
            let rows: Vec<(String, Vec<String>)> = e.entries.iter()
                .filter(|en| !en.id.is_empty())
                .map(|en| (en.id.clone(), vec![en.name.clone(), en.desc.clone()]))
                .collect();
            // Enum 表头固定 id|name|desc，无 schema desc，desc 列硬编码中文说明
            return Some(RefTargetData {
                is_table: false,
                headers: vec![
                    ("name".to_string(), "名称".to_string()),
                    ("desc".to_string(), "描述".to_string()),
                ],
                id_header: ("id".to_string(), "ID".to_string()),
                rows,
            });
        }
    }
    None
}

/// Ref 列单击 → 打开 RefPicker。current_value 取自该 cell 的真实存储值。
pub(crate) fn open_for_cell(state: &Rc<RefCell<AppState>>, r: usize, c: usize, ref_target: &str) {
    let mut st = state.borrow_mut();
    let default_strategy = RefDisplayStrategy::from_config(
        st.engine.project().config.ui.as_ref()
            .map(|u| u.ref_picker.default_strategy.as_str())
            .unwrap_or("auto"));
    let (group, name, is_table, current) = match &st.selected {
        Some(SelectedNode::Table { group, name, .. }) => {
            let cur = st.engine.find_table(group, name)
                .and_then(|t| t.records.get(r).and_then(|row| row.get(c)).cloned())
                .unwrap_or_default();
            (group.clone(), name.clone(), true, cur)
        }
        Some(SelectedNode::Constant { group, name, .. }) => {
            // Constant 不允许 Ref 类型；保留兜底
            (group.clone(), name.clone(), false, String::new())
        }
        _ => return,
    };
    st.ref_picker.open_with(ref_target, &current, r, c, &group, &name, is_table, default_strategy);
}

/// 把 RefPickerState 派生成 slint 端属性并 push。
pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let rp = &st.ref_picker;
    ui_h.set_dlg_ref_open(rp.open);
    if !rp.open { return; }

    ui_h.set_rp_ref_name(rp.ref_name.clone().into());
    ui_h.set_rp_search(rp.search.clone().into());
    ui_h.set_rp_manual_value(rp.manual_value.clone().into());
    ui_h.set_rp_strategy_index(rp.strategy.to_index());

    let target = collect_ref_rows(&st, &rp.ref_name, rp.strategy);
    let (kind_label, target_missing, is_table, id_header, headers, rows): (&str, bool, bool, (String, String), Vec<(String, String)>, Vec<(String, Vec<String>)>) = match target {
        Some(t) => (
            if t.is_table { "📊 表引用" } else { "🔢 枚举引用" },
            false,
            t.is_table,
            t.id_header,
            t.headers,
            t.rows,
        ),
        None => ("⚠️ 引用不存在", true, true, ("id".to_string(), String::new()), Vec::new(), Vec::new()),
    };
    ui_h.set_rp_kind_label(kind_label.into());
    ui_h.set_rp_target_missing(target_missing);
    ui_h.set_rp_is_table(is_table);

    let has_desc = !id_header.1.is_empty() || headers.iter().any(|(_, d)| !d.is_empty());
    ui_h.set_rp_has_desc(has_desc);
    ui_h.set_rp_id_header(RefHeader {
        field: id_header.0.into(),
        desc: id_header.1.into(),
    });
    let header_items: Vec<RefHeader> = headers.iter().map(|(f, d)| RefHeader {
        field: f.clone().into(),
        desc: d.clone().into(),
    }).collect();
    ui_h.set_rp_headers(slint::ModelRc::new(slint::VecModel::from(header_items)));

    let search = rp.search.to_lowercase();
    let filtered: Vec<RefRow> = rows.iter().filter(|(id, extras)| {
        if search.is_empty() { return true; }
        if id.to_lowercase().contains(&search) { return true; }
        extras.iter().any(|e| e.to_lowercase().contains(&search))
    }).map(|(id, extras)| {
        let extras_shared: Vec<slint::SharedString> = extras.iter().map(|e| e.clone().into()).collect();
        RefRow {
            id: id.clone().into(),
            extras: slint::ModelRc::new(slint::VecModel::from(extras_shared)),
            selected: rp.selected_id == *id,
        }
    }).collect();
    ui_h.set_rp_rows(slint::ModelRc::new(slint::VecModel::from(filtered)));

    let preview = if rp.selected_id.is_empty() {
        String::from("（未选择）")
    } else {
        rows.iter().find(|(id, _)| id == &rp.selected_id)
            .map(|(id, ex)| {
                let nm = ex.first().cloned().unwrap_or_default();
                if nm.is_empty() { id.clone() } else { format!("{} ({})", id, nm) }
            })
            .unwrap_or_else(|| format!("{} ⚠️ 不存在", rp.selected_id))
    };
    ui_h.set_rp_selection_preview(preview.into());
}

/// 写回当前 RefPicker 的选中 id 到 cell（空字符串 = 清空）。
fn commit(state: &Rc<RefCell<AppState>>, value: String) {
    let mut st = state.borrow_mut();
    let r = match st.ref_picker.editing_row { Some(v) => v, None => return };
    let c = match st.ref_picker.editing_col { Some(v) => v, None => return };
    let group = st.ref_picker.editing_group.clone();
    let name = st.ref_picker.editing_name.clone();
    let is_table = st.ref_picker.editing_source_table;
    if is_table {
        st.engine.commit_table_cell(&group, &name, r, c, value);
    } else {
        st.engine.commit_constant_cell(&group, &name, r, c, value);
    }
    if st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
    st.ref_picker.close();
}

/// 把 slint 端 rp-search 文本同步回 Rust state，方便后续 collect/filter 用。
fn sync_search(state: &Rc<RefCell<AppState>>, ui_weak: &slint::Weak<AppWindow>) {
    if let Some(ui_h) = ui_weak.upgrade() {
        let s = ui_h.get_rp_search().to_string();
        state.borrow_mut().ref_picker.search = s;
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_select_row(move |i| {
            sync_search(&s, &weak);
            let chosen: Option<String> = {
                let st = s.borrow();
                let rp = &st.ref_picker;
                let target = collect_ref_rows(&st, &rp.ref_name, rp.strategy);
                let rows: Vec<(String, Vec<String>)> = match target {
                    Some(t) => t.rows,
                    None => Vec::new(),
                };
                let search = rp.search.to_lowercase();
                let filtered: Vec<&(String, Vec<String>)> = rows.iter().filter(|(id, extras)| {
                    if search.is_empty() { return true; }
                    if id.to_lowercase().contains(&search) { return true; }
                    extras.iter().any(|e| e.to_lowercase().contains(&search))
                }).collect();
                filtered.get(i as usize).map(|(id, _)| id.clone())
            };
            if let Some(id) = chosen {
                {
                    let mut st = s.borrow_mut();
                    st.ref_picker.selected_id = id.clone();
                    st.ref_picker.manual_value = id;
                }
                if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_row_double_clicked(move |i| {
            sync_search(&s, &weak);
            let chosen: Option<String> = {
                let st = s.borrow();
                let rp = &st.ref_picker;
                let target = collect_ref_rows(&st, &rp.ref_name, rp.strategy);
                let rows: Vec<(String, Vec<String>)> = match target {
                    Some(t) => t.rows,
                    None => Vec::new(),
                };
                let search = rp.search.to_lowercase();
                let filtered: Vec<&(String, Vec<String>)> = rows.iter().filter(|(id, extras)| {
                    if search.is_empty() { return true; }
                    if id.to_lowercase().contains(&search) { return true; }
                    extras.iter().any(|e| e.to_lowercase().contains(&search))
                }).collect();
                filtered.get(i as usize).map(|(id, _)| id.clone())
            };
            if let Some(id) = chosen {
                {
                    let mut st = s.borrow_mut();
                    st.ref_picker.selected_id = id.clone();
                    st.ref_picker.manual_value = id.clone();
                }
                commit(&s, id);
                if let Some(ui_h) = weak.upgrade() {
                    refresh::after_grid_edit(&ui_h, &s);
                    push(&ui_h, &s);
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_confirm(move || {
            sync_search(&s, &weak);
            let val = {
                let st = s.borrow();
                if !st.ref_picker.manual_value.is_empty() {
                    st.ref_picker.manual_value.clone()
                } else {
                    st.ref_picker.selected_id.clone()
                }
            };
            commit(&s, val);
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_grid_edit(&ui_h, &s);
                push(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_clear(move || {
            commit(&s, String::new());
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_grid_edit(&ui_h, &s);
                push(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_cancel(move || {
            s.borrow_mut().ref_picker.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        // 搜索框编辑：把文本写入 state 并立即重 push，否则列表过滤永远不刷新。
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_search_edited(move |q| {
            s.borrow_mut().ref_picker.search = q.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        // 列展示策略临时切换：写入 state 并重 push 让 headers/extras 跟着变。
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_strategy_changed(move |i| {
            s.borrow_mut().ref_picker.strategy = RefDisplayStrategy::from_index(i);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        // 手动输入：与列表选中同步；命中列表项时一并更新 selected_id 触发高亮。
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_rp_manual_edited(move |q| {
            {
                let mut st = s.borrow_mut();
                st.ref_picker.manual_value = q.to_string();
                st.ref_picker.selected_id = q.to_string();
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
