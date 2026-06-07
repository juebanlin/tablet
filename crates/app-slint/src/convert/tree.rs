// 树面板派生：把 AppState 投影成 slint TreeNode 列表 + 同步 state.tree_targets。
//
// 三层结构（indent 0/1/2 = project / group / node）。

use std::collections::{HashMap, HashSet};

use slint::{Color, SharedString};

use crate::state::{AppState, SelectedNode, TreeFilter, TreeTarget};
use crate::theme::*;
use crate::TreeNode;
use tablet_core::name_matches;
use tablet_core::ops::AvailableProject;

/// 构建 slint TreeNode 列表，并同步 state.tree_targets。
/// 调用方负责把返回值 push 到 AppWindow.tree-nodes。
///
/// 三层结构（indent 0/1/2 = project / group / node）。
/// 过滤语义（filter ∧ search 是 AND 关系）：
/// - 子项级：passes_filter(child) ∧ name_matches(child.name, search)
/// - 组级：(组内任一子项 passes_filter) ∧ name_matches(group.name, search) → 组本身命中
/// - 组要不要展示：组本身命中 ∨ 任一子项级命中
/// - 完整组打开后：组要不要展示 = true 时，子项忽略 filter+search 全部展开（仅过滤 deleted）
/// - project 自身命中 = name_matches(project.name) ∨ name_matches(project.id)；filter 不作用 project 自身
/// - project 要不要展示：自身命中 ∨ 内部任一 group/node 命中
pub fn build_tree_nodes(state: &mut AppState) -> Vec<TreeNode> {
    state.tree_targets.clear();
    let mut nodes = Vec::new();

    let filter = state.tree_filter.clone();
    let full_group_open = state.tree_full_group;
    let search = state.tree_search.clone();

    // 已打开 project 的 (id, name, groups) snapshot；用于三层渲染（避免 borrow 冲突）
    let opened_set: HashSet<String> = state.engine.opened_ids().into_iter().collect();
    let opened_snap: HashMap<String, (String, Vec<tablet_core::model::Group>)> = state.engine.projects.iter()
        .map(|p| (p.schema.meta.id.clone(),
                  (p.schema.meta.name.clone(), p.groups.clone())))
        .collect();

    let sorted = sorted_available(
        state.engine.available(),
        &opened_set,
        &state.project_sort,
        &state.project_order,
    );

    for ap in &sorted {
        let pid = &ap.id;
        let is_open = opened_set.contains(pid);

        // ── closed project：灰态根节点；不显示三角，不参与 search 过滤 ──
        if !is_open {
            let p_selected = matches!(&state.selected, Some(SelectedNode::Project { project_id }) if project_id == pid);
            nodes.push(TreeNode {
                id: state.tree_targets.len() as i32,
                indent: 0,
                expanded: false,
                icon: SharedString::from(ICON_PROJECT),
                name: SharedString::from(ap.name.clone()),
                mark: SharedString::from(""),
                mark_color: color_default(),
                is_group: false, // closed 项目不显示三角，靠双击 / 单击 / 右键打开
                selected: p_selected,
                closed: true,
            });
            state.tree_targets.push(TreeTarget::Project(pid.clone()));
            continue;
        }

        // ── opened project：从 snapshot 拿 name + groups，按原三层渲染 ──
        let (pname, groups) = match opened_snap.get(pid) {
            Some(v) => v.clone(),
            None => continue,
        };

        // 收集每个 group 的命中情况
        let mut group_views: Vec<GroupView> = Vec::with_capacity(groups.len());
        for group in &groups {
            let table_hits: Vec<bool> = group.tables.iter().map(|t| {
                passes_filter(&filter, t.deleted, t.original.is_empty(), t.dirty)
                    && name_matches(&t.name, &search)
            }).collect();
            let const_hits: Vec<bool> = group.constants.iter().map(|c| {
                passes_filter(&filter, c.deleted, c.original.is_empty(), c.dirty)
                    && name_matches(&c.name, &search)
            }).collect();
            let enum_hits: Vec<bool> = group.enums.iter().map(|e| {
                passes_filter(&filter, e.deleted, e.original.is_empty(), e.dirty)
                    && name_matches(&e.name, &search)
            }).collect();
            let any_child_hit = table_hits.iter().any(|b| *b)
                || const_hits.iter().any(|b| *b)
                || enum_hits.iter().any(|b| *b);
            let group_filter_pass = filter == TreeFilter::All
                || group.tables.iter().any(|t| passes_filter(&filter, t.deleted, t.original.is_empty(), t.dirty))
                || group.constants.iter().any(|c| passes_filter(&filter, c.deleted, c.original.is_empty(), c.dirty))
                || group.enums.iter().any(|e| passes_filter(&filter, e.deleted, e.original.is_empty(), e.dirty));
            let group_self_hit = group_filter_pass && name_matches(&group.name, &search);
            let show = group_self_hit || any_child_hit;
            group_views.push(GroupView { table_hits, const_hits, enum_hits, show });
        }
        let project_self_hit = name_matches(&pname, &search) || name_matches(pid, &search);
        let project_show = project_self_hit || group_views.iter().any(|g| g.show);
        if !project_show { continue; }

        // 项目根节点 marker 聚合
        let project_obj = state.engine.find_project(pid);
        let root_pending = project_obj.map(|p| p.root_pending_create).unwrap_or(false);
        let schema_dirty = project_obj.map(|p| p.schema_dirty).unwrap_or(false);
        let mut all_deleted_self = false;
        let mut all_deleted = true;
        // 项目级"新建未落盘"算 has_new；项目级 schema_dirty（且 root 已落盘 → 仅 meta/structure 改）算 has_dirty。
        // 这两条让空项目（无 group）也能正确显示 🟢/🟡。
        let mut has_dirty = schema_dirty && !root_pending;
        let mut has_new = root_pending;
        for g in &groups {
            if !g.tables.is_empty() || !g.constants.is_empty() || !g.enums.is_empty() {
                all_deleted_self = true;
            }
            if g.is_new { has_new = true; }
            for t in &g.tables {
                if !t.deleted { all_deleted = false; }
                if t.dirty && !t.deleted { has_dirty = true; }
                if t.original.is_empty() && !t.deleted { has_new = true; }
            }
            for c in &g.constants {
                if !c.deleted { all_deleted = false; }
                if c.dirty && !c.deleted { has_dirty = true; }
                if c.original.is_empty() && !c.deleted { has_new = true; }
            }
            for e in &g.enums {
                if !e.deleted { all_deleted = false; }
                if e.dirty && !e.deleted { has_dirty = true; }
                if e.original.is_empty() && !e.deleted { has_new = true; }
            }
        }
        let project_deleted = all_deleted_self && all_deleted;
        let project_new = has_new && !project_deleted;
        let project_dirty = has_dirty && !project_new && !project_deleted;
        let project_has_errors = state.engine.has_project_error(pid);

        let p_expanded = state.project_expanded.contains(pid);
        let (p_mark, p_mark_color) = marker(project_deleted, project_new, project_dirty, project_has_errors);

        let active = state.engine.active_project_id() == Some(pid.as_str());
        let p_selected = matches!(&state.selected, Some(SelectedNode::Project { project_id }) if project_id == pid);
        nodes.push(TreeNode {
            id: state.tree_targets.len() as i32,
            indent: 0,
            expanded: p_expanded,
            icon: SharedString::from(ICON_PROJECT),
            name: SharedString::from(if active { format!("{} {}", pname, ACTIVE_STAR) } else { pname.clone() }),
            mark: SharedString::from(p_mark),
            mark_color: p_mark_color,
            is_group: true, // 共享组节点的展开三角 UI
            selected: p_selected,
            closed: false,
        });
        state.tree_targets.push(TreeTarget::Project(pid.clone()));

        if !p_expanded { continue; }

        for (gi, group) in groups.iter().enumerate() {
            let view = &group_views[gi];
            if !view.show { continue; }

            let all_deleted_self =
                !group.tables.is_empty() || !group.constants.is_empty() || !group.enums.is_empty();
            let all_deleted = all_deleted_self
                && group.tables.iter().all(|t| t.deleted)
                && group.constants.iter().all(|c| c.deleted)
                && group.enums.iter().all(|e| e.deleted);
            let has_dirty = group.tables.iter().any(|t| t.dirty && !t.deleted)
                || group.constants.iter().any(|c| c.dirty && !c.deleted)
                || group.enums.iter().any(|e| e.dirty && !e.deleted);
            let group_deleted = all_deleted && !group.is_new;
            let group_is_new = group.is_new;
            let group_dirty = has_dirty && !group_is_new && !group_deleted;
            let group_has_errors = state.engine.has_group_error(pid, &group.name);

            let expanded = state.tree_expanded.contains(&(pid.clone(), group.name.clone()));
            let (group_mark, group_mark_color) = marker(group_deleted, group_is_new, group_dirty, group_has_errors);
            let g_selected = matches!(&state.selected,
                Some(SelectedNode::Group { project_id, group: g }) if project_id == pid && g == &group.name);
            nodes.push(TreeNode {
                id: state.tree_targets.len() as i32,
                indent: 1,
                expanded,
                icon: SharedString::from(ICON_GROUP),
                name: SharedString::from(group.name.clone()),
                mark: SharedString::from(group_mark),
                mark_color: group_mark_color,
                is_group: true,
                selected: g_selected,
                closed: false,
            });
            state.tree_targets.push(TreeTarget::Group { project_id: pid.clone(), group: group.name.clone() });

            if !expanded { continue; }

            for (idx, t) in group.tables.iter().enumerate() {
                let show = if full_group_open { !t.deleted } else { view.table_hits[idx] };
                if !show { continue; }
                let selected = matches!(&state.selected,
                    Some(SelectedNode::Table { project_id, group: g, name: n })
                        if project_id == pid && g == &group.name && n == &t.name);
                let has_err = state.engine.has_node_error(pid, &group.name, &t.name);
                let (mark, mc) = marker(t.deleted, t.original.is_empty(), t.dirty, has_err);
                nodes.push(TreeNode {
                    id: state.tree_targets.len() as i32,
                    indent: 2,
                    expanded: false,
                    icon: SharedString::from(ICON_TABLE),
                    name: SharedString::from(t.name.clone()),
                    mark: SharedString::from(mark),
                    mark_color: mc,
                    is_group: false,
                    selected,
                    closed: false,
                });
                state.tree_targets.push(TreeTarget::Table {
                    project_id: pid.clone(), group: group.name.clone(), name: t.name.clone(),
                });
            }
            for (idx, c) in group.constants.iter().enumerate() {
                let show = if full_group_open { !c.deleted } else { view.const_hits[idx] };
                if !show { continue; }
                let selected = matches!(&state.selected,
                    Some(SelectedNode::Constant { project_id, group: g, name: n })
                        if project_id == pid && g == &group.name && n == &c.name);
                let has_err = state.engine.has_node_error(pid, &group.name, &c.name);
                let (mark, mc) = marker(c.deleted, c.original.is_empty(), c.dirty, has_err);
                nodes.push(TreeNode {
                    id: state.tree_targets.len() as i32,
                    indent: 2,
                    expanded: false,
                    icon: SharedString::from(ICON_CONST),
                    name: SharedString::from(c.name.clone()),
                    mark: SharedString::from(mark),
                    mark_color: mc,
                    is_group: false,
                    selected,
                    closed: false,
                });
                state.tree_targets.push(TreeTarget::Constant {
                    project_id: pid.clone(), group: group.name.clone(), name: c.name.clone(),
                });
            }
            for (idx, e) in group.enums.iter().enumerate() {
                let show = if full_group_open { !e.deleted } else { view.enum_hits[idx] };
                if !show { continue; }
                let selected = matches!(&state.selected,
                    Some(SelectedNode::Enum { project_id, group: g, name: n })
                        if project_id == pid && g == &group.name && n == &e.name);
                let has_err = state.engine.has_node_error(pid, &group.name, &e.name);
                let (mark, mc) = marker(e.deleted, e.original.is_empty(), e.dirty, has_err);
                nodes.push(TreeNode {
                    id: state.tree_targets.len() as i32,
                    indent: 2,
                    expanded: false,
                    icon: SharedString::from(ICON_ENUM),
                    name: SharedString::from(e.name.clone()),
                    mark: SharedString::from(mark),
                    mark_color: mc,
                    is_group: false,
                    selected,
                    closed: false,
                });
                state.tree_targets.push(TreeTarget::Enum {
                    project_id: pid.clone(), group: group.name.clone(), name: e.name.clone(),
                });
            }
        }
    }
    nodes
}

/// 把 available_projects 按当前 sort 排序；closed 与 opened 一同进入。
/// "id"     ：字典序（默认）
/// "name"   ：display name 字典序
/// "open"   ：已打开优先 → id 字典序
/// "created"：created_at 字典序
/// "manual" ：跟随 project_order；缺席的排到末尾保持 id 序
pub fn sorted_available(
    available: &[AvailableProject],
    opened: &HashSet<String>,
    sort: &str,
    manual: &[String],
) -> Vec<AvailableProject> {
    let mut v: Vec<AvailableProject> = available.to_vec();
    match sort {
        "name" => v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id))),
        "open" => v.sort_by(|a, b| {
            let ao = opened.contains(&a.id);
            let bo = opened.contains(&b.id);
            bo.cmp(&ao).then(a.id.cmp(&b.id))
        }),
        "created" => v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id))),
        "manual" => {
            let pos = |id: &str| manual.iter().position(|x| x == id).unwrap_or(usize::MAX);
            v.sort_by(|a, b| pos(&a.id).cmp(&pos(&b.id)).then(a.id.cmp(&b.id)));
        }
        _ => v.sort_by(|a, b| a.id.cmp(&b.id)),
    }
    v
}

struct GroupView {
    table_hits: Vec<bool>,
    const_hits: Vec<bool>,
    enum_hits: Vec<bool>,
    show: bool,
}

fn passes_filter(filter: &TreeFilter, deleted: bool, is_new: bool, dirty: bool) -> bool {
    match filter {
        TreeFilter::All => true,
        TreeFilter::New => is_new && !deleted,
        TreeFilter::Modified => dirty && !is_new && !deleted,
        TreeFilter::Deleted => deleted,
        TreeFilter::Changed => deleted || dirty || is_new,
    }
}

fn marker(deleted: bool, is_new: bool, dirty: bool, has_err: bool) -> (&'static str, Color) {
    if has_err { (MARK_ERR, color_err()) }
    else if deleted { (MARK_DEL, color_del()) }
    else if is_new { (MARK_NEW, color_new()) }
    else if dirty { (MARK_MOD, color_mod()) }
    else { (MARK_NONE, color_default()) }
}
