//! 列宽状态与回调：节点切换时的加载、自动/手动宽度合并、拖拽、持久化。
//!
//! 持久化走独立的 [`UserData`]（`.data/user-data.json`），不碰项目 dirty / 保存闸门。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::convert;
use crate::convert::{DEFAULT_COL_W, MAX_MANUAL_W, MIN_MANUAL_W};
use crate::state::{AppState, GridResizeState, SelectedNode};
use crate::AppWindow;
use tablet_core::local_state::{NodeKey, NodeKind as UserNodeKind};

/// 当前选中叶节点 → 用户数据 key；Project/Group/未选中 → None。
pub fn selected_node_key(st: &AppState) -> Option<NodeKey> {
    match &st.selected {
        Some(SelectedNode::Table { project_id, group, name }) => Some(NodeKey {
            project: project_id.clone(),
            group: group.clone(),
            kind: UserNodeKind::Table,
            node: name.clone(),
        }),
        Some(SelectedNode::Constant { project_id, group, name }) => Some(NodeKey {
            project: project_id.clone(),
            group: group.clone(),
            kind: UserNodeKind::Constant,
            node: name.clone(),
        }),
        Some(SelectedNode::Enum { project_id, group, name }) => Some(NodeKey {
            project: project_id.clone(),
            group: group.clone(),
            kind: UserNodeKind::Enum,
            node: name.clone(),
        }),
        _ => None,
    }
}

/// 切到新节点：清运行时缓存，从 `UserData` 读取该节点的 auto/manual。
pub fn on_node_changed(st: &mut AppState) {
    st.grid_resize = None;
    st.grid_col_names.clear();
    st.grid_auto_widths.clear();
    st.grid_widths.clear();
    st.grid_widths_total = 0.0;
    st.grid_width_manual.clear();
    st.grid_width_auto = true;
    st.grid_autofit_pending = true;

    let saved = selected_node_key(st)
        .and_then(|key| st.user_data.node(&key).cloned());
    if let Some(v) = saved {
        st.grid_width_auto = v.auto;
        st.grid_width_manual = v.widths;
    }
}

/// 用当前 snapshot 重建有效列宽。只在节点切换 / 显式重算时调用。
pub fn sync_from_snapshot(st: &mut AppState, snap: &convert::GridSnapshot) {
    if snap.column_names != st.grid_col_names {
        st.grid_col_names = snap.column_names.clone();
        st.grid_autofit_pending = true;
        // 列集合变了，正在拖的那一列索引可能已经失效。
        st.grid_resize = None;
    }
    if st.grid_autofit_pending {
        st.grid_auto_widths = convert::auto_widths(snap);
        st.grid_autofit_pending = false;
    }
    // 手动数据为空时没有可展示的手动布局（例如旧文件 / 清空后）→ 保持自动，
    // 避免「勾选没勾、布局却是自动」。只判断是否为空，不比较数值：
    // 手动值恰好等于自动值时仍是手动（两套数据独立）。
    if !st.grid_width_auto && st.grid_width_manual.is_empty() {
        st.grid_width_auto = true;
    }
    let cols = snap.col_count.max(0) as usize;
    st.grid_widths = convert::merge_widths(
        &st.grid_col_names,
        &st.grid_auto_widths,
        &st.grid_width_manual,
        !st.grid_width_auto,
        cols,
    );
    st.grid_widths_total = convert::total_width(&st.grid_widths, cols);
}

/// 只更新宽度相关属性；不重建 header/data 模型。
pub fn push_width_props(ui: &AppWindow, st: &AppState) {
    ui.set_grid_column_widths(slint::ModelRc::new(slint::VecModel::from(
        st.grid_widths.clone(),
    )));
    ui.set_grid_total_col_w(st.grid_widths_total);
    ui.set_grid_auto_width(st.grid_width_auto);
    ui.set_grid_auto_width_enabled(selected_node_key(st).is_some());
}

/// 当前有效宽度（补齐到 cols），给拖动命中测试用。
pub fn effective_widths(st: &AppState, cols: usize) -> Vec<f32> {
    let mut out = st.grid_widths.clone();
    out.resize(cols, DEFAULT_COL_W);
    out
}

/// 把当前节点的模式/手动宽度写回 `UserData` 并独立保存。
pub fn save_current(st: &mut AppState) {
    let Some(key) = selected_node_key(st) else { return };
    let result = {
        let node = st.user_data.node_mut(key);
        node.auto = st.grid_width_auto;
        node.widths = st.grid_width_manual.clone();
        st.user_data.save(&st.user_data_path)
    };
    if let Err(e) = result {
        st.engine.error_log(format!("[用户数据] 保存失败: {}", e));
    }
}

/// 用当前节点重建 snapshot → 合并宽度 → 推属性（不重建模型）。
pub fn refresh_widths(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let snap = {
        let st = state.borrow();
        convert::build_grid(&st)
    };
    {
        let mut st = state.borrow_mut();
        sync_from_snapshot(&mut st, &snap);
        push_width_props(ui, &st);
    }
}

pub fn resize_start(state: &Rc<RefCell<AppState>>, col: usize, abs_x: f32) {
    let mut st = state.borrow_mut();
    let start_w = st.grid_widths.get(col).copied().unwrap_or(DEFAULT_COL_W);
    st.grid_resize = Some(GridResizeState {
        col,
        start_abs_x: abs_x,
        start_w,
        changed: false,
    });
}

pub fn resize_move(ui: &AppWindow, state: &Rc<RefCell<AppState>>, col: usize, abs_x: f32) {
    let mut st = state.borrow_mut();
    let Some(rs) = st.grid_resize.clone() else { return };
    if rs.col != col {
        return;
    }
    let delta = abs_x - rs.start_abs_x;
    if delta.abs() < 2.0 && !rs.changed {
        return;
    }
    let new_w = (rs.start_w + delta).clamp(MIN_MANUAL_W, MAX_MANUAL_W).round();
    if st.grid_widths.get(col).copied() == Some(new_w) {
        return;
    }

    if st.grid_width_auto {
        // 首次真实拖动：切手动，并把当前有效宽度播种给所有列，避免未拖的列跳变。
        st.grid_width_auto = false;
        let seed: Vec<(String, f32)> = st
            .grid_col_names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                (name.clone(), st.grid_widths.get(i).copied().unwrap_or(DEFAULT_COL_W))
            })
            .collect();
        for (name, w) in seed {
            st.grid_width_manual.entry(name).or_insert(w);
        }
    }
    if let Some(name) = st.grid_col_names.get(col).cloned() {
        st.grid_width_manual.insert(name, new_w);
    }
    if let Some(w) = st.grid_widths.get_mut(col) {
        *w = new_w;
    }
    st.grid_widths_total = convert::total_width(&st.grid_widths, st.grid_widths.len());
    if let Some(rs) = st.grid_resize.as_mut() {
        rs.changed = true;
    }
    push_width_props(ui, &st);
}

pub fn resize_end(state: &Rc<RefCell<AppState>>) {
    let changed = state
        .borrow()
        .grid_resize
        .as_ref()
        .map(|r| r.changed)
        .unwrap_or(false);
    state.borrow_mut().grid_resize = None;
    if changed {
        let mut st = state.borrow_mut();
        save_current(&mut st);
    }
}

/// 双击边界的纯状态部分：把该列的内容宽度写进**手动**数据。
/// 自动宽度（`grid_auto_widths`）只读不写；自动模式下切手动并播种其它列。
pub(crate) fn apply_autofit(st: &mut AppState, snap: &convert::GridSnapshot, col: usize) -> bool {
    let fitted = convert::auto_widths(snap).get(col).copied();
    let Some(w) = fitted else { return false };
    if st.grid_width_auto {
        st.grid_width_auto = false;
        let seed: Vec<(String, f32)> = st
            .grid_col_names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                (name.clone(), st.grid_widths.get(i).copied().unwrap_or(DEFAULT_COL_W))
            })
            .collect();
        for (name, old) in seed {
            st.grid_width_manual.entry(name).or_insert(old);
        }
    }
    if let Some(name) = st.grid_col_names.get(col).cloned() {
        st.grid_width_manual.insert(name, w);
    }
    if let Some(slot) = st.grid_widths.get_mut(col) {
        *slot = w;
    }
    st.grid_widths_total = convert::total_width(&st.grid_widths, st.grid_widths.len());
    true
}

/// 双击边界：**手动调整的快捷操作**，只改该列的手动宽度，不重算/修改自动宽度。
pub fn autofit_col(ui: &AppWindow, state: &Rc<RefCell<AppState>>, col: usize) {
    let snap = {
        let st = state.borrow();
        convert::build_grid(&st)
    };
    {
        let mut st = state.borrow_mut();
        if !apply_autofit(&mut st, &snap, col) {
            return;
        }
        push_width_props(ui, &st);
    }
    let mut st = state.borrow_mut();
    save_current(&mut st);
}

pub fn set_auto_mode(ui: &AppWindow, state: &Rc<RefCell<AppState>>, auto: bool) {
    {
        let mut st = state.borrow_mut();
        if st.grid_width_auto == auto {
            return;
        }
        if !auto && st.grid_width_manual.is_empty() {
            // 还没有任何手动宽度可回退：保持自动，把 Slint 的勾选状态推回 true。
            st.grid_width_auto = true;
            drop(st);
            refresh_widths(ui, state);
            let mut st = state.borrow_mut();
            save_current(&mut st);
            return;
        }
        if auto {
            st.grid_autofit_pending = true;
        }
        st.grid_width_auto = auto;
    }
    refresh_widths(ui, state);
    let mut st = state.borrow_mut();
    save_current(&mut st);
}

/// 注册列宽相关回调。启动后调用一次。
pub fn wire(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        ui.on_grid_col_resize_start(move |col, x| {
            resize_start(&s, col.max(0) as usize, x);
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_col_resize_move(move |col, x| {
            if let Some(ui_h) = weak.upgrade() {
                resize_move(&ui_h, &s, col.max(0) as usize, x);
            }
        });
    }
    {
        let s = state.clone();
        ui.on_grid_col_resize_end(move |_col| {
            resize_end(&s);
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_col_autofit(move |col| {
            if let Some(ui_h) = weak.upgrade() {
                autofit_col(&ui_h, &s, col.max(0) as usize);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui.as_weak();
        ui.on_grid_auto_width_toggled(move |on| {
            if let Some(ui_h) = weak.upgrade() {
                set_auto_mode(&ui_h, &s, on);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_workspace(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tablet_grid_layout_{}_{}_{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let proj = dir.join("projects").join("demo");
        std::fs::create_dir_all(proj.join("config").join("enums")).unwrap();
        std::fs::write(
            proj.join("project.tblmanifest"),
            "#!tblmanifest v1\n# @meta id: demo\n\n[enums/HeroType] enum\n",
        )
        .unwrap();
        std::fs::write(
            proj.join("config").join("enums").join("HeroType.tbl"),
            "#!tbl v2\n#mode enum\n---\n1|WARRIOR|战士\n2|MAGE|法师\n",
        )
        .unwrap();
        dir
    }

    fn select_enum(st: &mut AppState) {
        st.selected = Some(SelectedNode::Enum {
            project_id: "demo".into(),
            group: "enums".into(),
            name: "HeroType".into(),
        });
    }

    #[test]
    fn manual_widths_survive_node_switch_and_auto_data_is_independent() {
        let dir = tmp_workspace("switch");
        let (mut st, _, _) = AppState::load(&dir).unwrap();
        select_enum(&mut st);
        on_node_changed(&mut st);
        let snap = convert::build_grid(&st);
        sync_from_snapshot(&mut st, &snap);
        assert!(st.grid_width_auto);

        let auto_name = st.grid_auto_widths[1];
        let auto_id = st.grid_auto_widths[0];
        let auto_desc = st.grid_auto_widths[2];

        // 手动拖过 name：切走再切回，必须仍是手动宽度。
        st.grid_width_manual.insert("name".into(), 115.0);
        st.grid_width_auto = false;
        sync_from_snapshot(&mut st, &snap);
        assert!(!st.grid_width_auto);
        assert_eq!(st.grid_widths[1], 115.0);
        save_current(&mut st);

        st.selected = Some(SelectedNode::Project { project_id: "demo".into() });
        on_node_changed(&mut st);
        select_enum(&mut st);
        on_node_changed(&mut st);
        let snap = convert::build_grid(&st);
        sync_from_snapshot(&mut st, &snap);
        assert!(!st.grid_width_auto, "回来后仍应是手动模式");
        assert_eq!(st.grid_widths[1], 115.0, "手动宽度不能丢");

        // 自动数据和手动数据独立：手动值恰好等于自动值时，仍是手动模式。
        st.grid_width_manual.clear();
        st.grid_width_manual.insert("id".into(), auto_id);
        st.grid_width_manual.insert("name".into(), auto_name);
        st.grid_width_manual.insert("desc".into(), auto_desc);
        st.grid_width_auto = false;
        sync_from_snapshot(&mut st, &snap);
        assert!(!st.grid_width_auto, "手动数据与自动值相同也仍是手动");
        assert_eq!(st.grid_widths[1], auto_name);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn double_click_autofit_writes_manual_widths_only() {
        let dir = tmp_workspace("autofit");
        let (mut st, _, _) = AppState::load(&dir).unwrap();
        select_enum(&mut st);
        on_node_changed(&mut st);
        let snap = convert::build_grid(&st);
        sync_from_snapshot(&mut st, &snap);

        let auto_before = st.grid_auto_widths.clone();
        assert!(apply_autofit(&mut st, &snap, 1), "name 列应能算出内容宽度");
        assert!(!st.grid_width_auto, "双击是手动操作，应切到手动");
        assert_eq!(st.grid_auto_widths, auto_before, "自动宽度数据不能被手动操作修改");
        assert_eq!(
            st.grid_width_manual.get("name"),
            st.grid_auto_widths.get(1),
            "双击结果写入手动数据（独立于自动数据）"
        );
        assert!(st.grid_width_manual.contains_key("id"), "其它列应播种当前宽度");
        assert!(st.grid_width_manual.contains_key("desc"));

        // 手动模式下双击另一列：只改该列手动值，自动数据仍不变。
        let auto_before2 = st.grid_auto_widths.clone();
        assert!(apply_autofit(&mut st, &snap, 0));
        assert_eq!(st.grid_auto_widths, auto_before2);
        assert_eq!(
            st.grid_width_manual.get("id"),
            st.grid_auto_widths.get(0)
        );

        // 手动数据被清空后没有可展示的手动布局 → 回自动（只判断空，不比较数值）。
        st.grid_width_manual.clear();
        st.grid_width_auto = false;
        sync_from_snapshot(&mut st, &snap);
        assert!(st.grid_width_auto, "手动数据为空时应回自动");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
