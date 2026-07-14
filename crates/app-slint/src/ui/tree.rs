// 树面板：构建 TreeNode 列表 + 接通 9 个 callback（filter / search / 排序 /
// 单击 / 双击 / 右键 / 完整组勾选 / toggle expand / 空白右键）。
//
// `push_tree` 复用同一个 VecModel：直接替换 items 而非整张 ModelRc。
// 整张 ModelRc 替换会让 slint Repeater 销毁并重建所有子元素（含 TouchArea），
// 第一次单击触发 rebuild 后，后续点击落在新 TouchArea 实例上 → 双击永远凑不齐。

use std::cell::RefCell;
use std::rc::Rc;
use std::cell::OnceCell;
use std::str::FromStr;

use slint::ComponentHandle;

use crate::convert;
use crate::state::{
    AppState, CtxMenuKind, GridSelection, SelectedNode, TreeFilter, TreeTarget,
};
use crate::{dialogs, refresh, ui, AppWindow, TreeNode};

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let nodes = convert::build_tree_nodes(&mut state.borrow_mut());
    thread_local! {
        static TREE_MODEL: OnceCell<Rc<slint::VecModel<TreeNode>>> = OnceCell::new();
    }
    TREE_MODEL.with(|cell| {
        let model = cell.get_or_init(|| {
            let m = Rc::new(slint::VecModel::<TreeNode>::default());
            ui_h.set_tree_nodes(slint::ModelRc::from(m.clone()));
            m
        });
        sync_vec_model(model, nodes);
    });
}

/// 把 model 内容刷新成 new_items，复用尽可能多的行（in-place set_row_data），
/// 使 slint Repeater 不销毁现有 TouchArea。
fn sync_vec_model<T: Clone + 'static>(model: &Rc<slint::VecModel<T>>, new_items: Vec<T>) {
    use slint::Model;
    let old_len = model.row_count();
    let new_len = new_items.len();
    let common = old_len.min(new_len);
    for (i, item) in new_items.iter().take(common).enumerate() {
        model.set_row_data(i, item.clone());
    }
    if new_len > old_len {
        for item in new_items.into_iter().skip(common) {
            model.push(item);
        }
    } else {
        for _ in new_len..old_len {
            model.remove(new_len);
        }
    }
}

/// project_sort 字符串 ↔ slint ComboBox 索引。
/// 顺序与 tree_section.slint 的 sort-options 对齐：["ID", "名称", "已打开", "创建时间", "手动"]。
pub(crate) fn sort_to_index(s: &str) -> i32 {
    match s {
        "name" => 1,
        "open" => 2,
        "created" => 3,
        "manual" => 4,
        _ => 0,
    }
}
fn index_to_sort(i: i32) -> tablet_core::enums::ProjectSort {
    use tablet_core::enums::ProjectSort;
    match i {
        1 => ProjectSort::Name,
        2 => ProjectSort::Open,
        3 => ProjectSort::Created,
        4 => ProjectSort::Manual,
        _ => ProjectSort::Id
    }
}

/// 把当前 workspace 状态落盘到 `<workdir>/tablet.toml`；失败仅 log。
pub(crate) fn persist_workspace(state: &mut AppState) {
    let sort_enum = tablet_core::enums::ProjectSort::from_str(&state.project_sort).unwrap_or_default();
    if let Err(e) = tablet_core::project::persist_workspace_state(
        &state.engine, sort_enum, &state.project_order,
    ) {
        state.engine.error_log(format!("[workspace] 持久化失败: {}", e));
    }
}

/// 打开一个 closed project，成功后 persist。返回是否真打开了一个新的。
pub(crate) fn open_project_with_persist(state: &Rc<RefCell<AppState>>, pid: &str) -> bool {
    let result = state.borrow_mut().engine.open_project(pid);
    match result {
        Ok(true) => {
            persist_workspace(&mut *state.borrow_mut());
            true
        }
        Ok(false) => false,
        Err(e) => {
            state.borrow_mut().engine.error_log(format!("[workspace] 打开 {} 失败: {}", pid, e));
            false
        }
    }
}

/// 关闭一个 opened project，清掉相关 UI 态 + persist。
pub(crate) fn close_project_with_persist(state: &Rc<RefCell<AppState>>, pid: &str) {
    let mut st = state.borrow_mut();
    if matches!(&st.selected, Some(s) if s.project_id() == pid) {
        st.selected = None;
        st.grid_selection = GridSelection::None;
        st.editing = None;
    }
    if st.engine.close_project(pid) {
        st.tree_expanded.retain(|(p, _)| p != pid);
        st.project_expanded.remove(pid);
        persist_workspace(&mut *st);
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // 过滤切换
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_filter_changed(move |i| {
            s.borrow_mut().tree_filter = TreeFilter::from_index(i);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 完整组勾选
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_full_group_toggled(move |c| {
            s.borrow_mut().tree_full_group = c;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 搜索
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_search_edited(move |t| {
            s.borrow_mut().tree_search = t.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 展开/折叠（仅对 opened project / group 生效；closed project 三角点击不响应——靠双击/右键打开）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_node_toggle_expand(move |id| {
            let mut st = s.borrow_mut();
            match st.tree_targets.get(id as usize).cloned() {
                Some(TreeTarget::Project(pid)) => {
                    if !st.engine.is_opened(&pid) { return; }
                    if !st.project_expanded.remove(&pid) {
                        st.project_expanded.insert(pid);
                    }
                }
                Some(TreeTarget::Group { project_id, group }) => {
                    let key = (project_id, group);
                    if !st.tree_expanded.remove(&key) {
                        st.tree_expanded.insert(key);
                    }
                }
                _ => {}
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 节点点击 → 切换 selected；如有 in-progress 编辑，先 commit
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let ui_for_buf = ui_h.as_weak();
        ui_h.on_tree_node_clicked(move |id| {
            ui::grid_actions::commit_editing(&ui_for_buf, &s);
            let target = s.borrow().tree_targets.get(id as usize).cloned();
            // 单击仅选中（含切 active），不触发展开/折叠；closed project 仅选中，不触发打开。
            let mut st = s.borrow_mut();
            let mut grid_dirty = false;
            match target {
                Some(TreeTarget::Project(pid)) => {
                    let opened = st.engine.is_opened(&pid);
                    if opened {
                        st.engine.set_active_by_id(&pid);
                    }
                    st.selected = Some(SelectedNode::Project { project_id: pid });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                Some(TreeTarget::Group { project_id, group }) => {
                    st.engine.set_active_by_id(&project_id);
                    st.selected = Some(SelectedNode::Group { project_id, group });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                Some(TreeTarget::Table { project_id, group, name }) => {
                    st.engine.set_active_by_id(&project_id);
                    st.selected = Some(SelectedNode::Table { project_id, group, name });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                Some(TreeTarget::Constant { project_id, group, name }) => {
                    st.engine.set_active_by_id(&project_id);
                    st.selected = Some(SelectedNode::Constant { project_id, group, name });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                Some(TreeTarget::Enum { project_id, group, name }) => {
                    st.engine.set_active_by_id(&project_id);
                    st.selected = Some(SelectedNode::Enum { project_id, group, name });
                    st.grid_selection = GridSelection::None;
                    st.editing = None;
                    grid_dirty = true;
                }
                None => {}
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                if grid_dirty { ui::grid::push(&ui_h, &s); }
            }
        });
    }
    // 树节点右键 → 打开 ContextMenu
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_node_context_menu(move |id, x, y| {
            let kind = {
                let st = s.borrow();
                st.tree_targets.get(id as usize).cloned()
            };
            let menu_kind = match kind {
                Some(TreeTarget::Project(pid)) =>
                    Some(CtxMenuKind::TreeProject { project_id: pid }),
                Some(TreeTarget::Group { project_id, group }) =>
                    Some(CtxMenuKind::TreeGroup { project_id, name: group }),
                Some(TreeTarget::Table { project_id, group, name }) =>
                    Some(CtxMenuKind::TreeNode { project_id, group, name, kind: tablet_core::ops::NodeKind::Table }),
                Some(TreeTarget::Constant { project_id, group, name }) =>
                    Some(CtxMenuKind::TreeNode { project_id, group, name, kind: tablet_core::ops::NodeKind::Constant }),
                Some(TreeTarget::Enum { project_id, group, name }) =>
                    Some(CtxMenuKind::TreeNode { project_id, group, name, kind: tablet_core::ops::NodeKind::Enum }),
                None => None,
            };
            if let Some(k) = menu_kind {
                s.borrow_mut().ctx_menu.open_at(k, x as f32, y as f32);
                if let Some(ui_h) = weak.upgrade() { dialogs::context_menu::push(&ui_h, &s); }
            }
        });
    }
    // 树空白右键 → ContextMenu(TreeBlank)
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_blank_context_menu(move |x, y| {
            s.borrow_mut().ctx_menu.open_at(CtxMenuKind::TreeBlank, x as f32, y as f32);
            if let Some(ui_h) = weak.upgrade() { dialogs::context_menu::push(&ui_h, &s); }
        });
    }
    // 双击：closed project = 打开 + active；opened project root = 切换展开态
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_node_double_clicked(move |id| {
            let target = s.borrow().tree_targets.get(id as usize).cloned();
            match target {
                Some(TreeTarget::Project(pid)) => {
                    let opened = s.borrow().engine.is_opened(&pid);
                    if !opened {
                        if open_project_with_persist(&s, &pid) {
                            let mut st = s.borrow_mut();
                            st.engine.set_active_by_id(&pid);
                            st.selected = Some(SelectedNode::Project { project_id: pid.clone() });
                            st.project_expanded.insert(pid.clone());
                            let groups: Vec<String> = st.engine.find_project(&pid)
                                .map(|p| p.groups.iter().map(|g| g.name.clone()).collect())
                                .unwrap_or_default();
                            for g in groups {
                                st.tree_expanded.insert((pid.clone(), g));
                            }
                        }
                    } else {
                        let mut st = s.borrow_mut();
                        if !st.project_expanded.remove(&pid) {
                            st.project_expanded.insert(pid);
                        }
                    }
                }
                Some(TreeTarget::Group { project_id, group }) => {
                    let mut st = s.borrow_mut();
                    let key = (project_id, group);
                    if !st.tree_expanded.remove(&key) {
                        st.tree_expanded.insert(key);
                    }
                }
                _ => {}
            }
            if let Some(ui_h) = weak.upgrade() { refresh::after_tree_change(&ui_h, &s); }
        });
    }
    // 排序下拉切换
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_tree_sort_changed(move |i| {
            let new_sort = index_to_sort(i).as_str().to_string();
            {
                let mut st = s.borrow_mut();
                if st.project_sort != new_sort {
                    st.project_sort = new_sort;
                    persist_workspace(&mut *st);
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
