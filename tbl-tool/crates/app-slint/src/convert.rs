// convert：纯函数，把 AppState 派生成 slint Model 数据。
// 不持有引用，每次调用产出新 Vec，调用方负责 push 到 AppWindow。

use std::collections::{HashMap, HashSet};
use slint::{Color, SharedString};
use crate::state::{AppState, ColumnKind, GridSelection, SelectedNode, TreeFilter, TreeTarget};
use crate::{CellKind, DataCell, DataRow, HeaderCell, TreeNode};
use tbl_core::name_matches;
use tbl_core::ops::AvailableProject;

pub const EXTRA_ROWS: usize = 5;

const ICON_PROJECT: &str = "📦";
const ICON_GROUP: &str = "📁";
const ICON_TABLE: &str = "📊";
const ICON_CONST: &str = "📋";
const ICON_ENUM: &str = "🔢";

fn color_new() -> Color { Color::from_rgb_u8(40, 180, 40) }
fn color_mod() -> Color { Color::from_rgb_u8(200, 170, 0) }
fn color_del() -> Color { Color::from_rgb_u8(220, 50, 50) }
fn color_err() -> Color { Color::from_rgb_u8(220, 50, 50) }
fn color_default() -> Color { Color::from_rgb_u8(0xe6, 0xe6, 0xe6) }

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
    let opened_snap: HashMap<String, (String, Vec<tbl_core::model::Group>)> = state.engine.projects.iter()
        .map(|p| (p.instance_meta.id.clone(),
                  (p.instance_meta.name.clone(), p.groups.clone())))
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
        let mut all_deleted_self = false;
        let mut all_deleted = true;
        let mut has_dirty = false;
        let mut has_new = false;
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
        let project_dirty = has_dirty && !project_deleted;
        let project_new = has_new && !project_dirty && !project_deleted;
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
            name: SharedString::from(if active { format!("{} ★", pname) } else { pname.clone() }),
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
    if has_err { ("!", color_err()) }
    else if deleted { ("-", color_del()) }
    else if is_new { ("+", color_new()) }
    else if dirty { ("*", color_mod()) }
    else { ("", color_default()) }
}

// ──────── GridSection ────────

fn color_text_primary() -> Color { Color::from_rgb_u8(0x1a, 0x1a, 0x1a) }
fn color_text_readonly() -> Color { Color::from_rgb_u8(0x6e, 0x6e, 0x6e) }
fn color_text_success() -> Color { Color::from_rgb_u8(0x50, 0xa0, 0x50) }
fn color_text_info() -> Color { Color::from_rgb_u8(0x50, 0x82, 0xd2) }
fn color_transparent() -> Color { Color::from_argb_u8(0, 0, 0, 0) }

/// GridSection 的快照：title / subtitle / col_count / 多行表头 / 数据行 / 选区联动字段。
pub struct GridSnapshot {
    pub title: String,
    pub subtitle: String,
    pub col_count: i32,
    pub header_rows: Vec<Vec<HeaderCell>>,
    pub data_rows: Vec<DataRow>,
    /// 当前节点每列的可编辑性，由 main.rs 同步写回 AppState
    pub column_kinds: Vec<ColumnKind>,
    /// 当前节点表头每个 cell 的可编辑性（按 [hrow][col] 索引）
    pub header_kinds: Vec<Vec<ColumnKind>>,
    pub data_count: usize,
    pub coord: String,
    /// 非编辑态公式栏显示的展示值
    pub formula_display: String,
    /// 当前选中 cell 是否允许编辑（Text 列且非占位行外限制）
    pub formula_editable: bool,
    pub selected_cell_row: i32,
    pub selected_cell_col: i32,
    pub selected_row: i32,
    pub selected_col: i32,
    /// 矩形选区边界（含端）；-1 = 非区域选区。供 slint 端按 cell (ri,ci) 判断 in-range 高亮。
    pub range_row_min: i32,
    pub range_row_max: i32,
    pub range_col_min: i32,
    pub range_col_max: i32,
    pub selection_info: String,
    pub hover_info: String,
}

impl GridSnapshot {
    pub fn empty() -> Self {
        Self {
            title: "未选中".into(),
            subtitle: String::new(),
            col_count: 0,
            header_rows: Vec::new(),
            data_rows: Vec::new(),
            column_kinds: Vec::new(),
            header_kinds: Vec::new(),
            data_count: 0,
            coord: String::new(),
            formula_display: String::new(),
            formula_editable: false,
            selected_cell_row: -1,
            selected_cell_col: -1,
            selected_row: -1,
            selected_col: -1,
            range_row_min: -1,
            range_row_max: -1,
            range_col_min: -1,
            range_col_max: -1,
            selection_info: String::new(),
            hover_info: String::new(),
        }
    }
}

pub fn build_grid(state: &AppState) -> GridSnapshot {
    let mut snap = match state.selected.clone() {
        Some(SelectedNode::Table { group, name, .. }) => build_table_grid(state, &group, &name),
        Some(SelectedNode::Constant { group, name, .. }) => build_constant_grid(state, &group, &name),
        Some(SelectedNode::Enum { group, name, .. }) => build_enum_grid(state, &group, &name),
        Some(SelectedNode::Project { .. }) | Some(SelectedNode::Group { .. }) | None => GridSnapshot::empty(),
    };
    enrich_selection(&mut snap, state);
    snap
}

/// 把 AppState.grid_selection 投影到 snap 的 coord/formula/selection_info 等字段。
fn enrich_selection(snap: &mut GridSnapshot, state: &AppState) {
    match state.grid_selection {
        GridSelection::Cell(r, c) if (c as i32) < snap.col_count => {
            snap.selected_cell_row = r as i32;
            snap.selected_cell_col = c as i32;
            snap.coord = format!("{}{}", col_letter(c), r + 1);
            let kind = snap.column_kinds.get(c);
            // 公式栏只允许 Text 列编辑；picker / ReadOnly 列只读 + 显示原始存储值（@04.5.3）
            snap.formula_editable = matches!(kind, Some(ColumnKind::Text));
            snap.formula_display = if matches!(kind, Some(ColumnKind::Text)) {
                let raw = raw_cell(state, r, c);
                if !raw.is_empty() { cell_display(state, snap, r, c, &raw) } else { String::new() }
            } else {
                // picker / ReadOnly：显示底层 raw（"1001" / "cs"），不走 cell_display 翻译
                raw_cell(state, r, c)
            };
            // 单格选中时，若该格有验证错误，状态栏在坐标后追加错误信息（实时验证 UX）。
            snap.selection_info = match cell_validation_message(state, r, c) {
                Some(msg) => format!("{} ⚠ {}", snap.coord, msg),
                None => snap.coord.clone(),
            };
        }
        GridSelection::CellRange { r1, c1, r2, c2 } => {
            let rmin = r1.min(r2);
            let rmax = r1.max(r2);
            let cmin = c1.min(c2).min((snap.col_count.max(1) as usize) - 1);
            let cmax = c1.max(c2).min((snap.col_count.max(1) as usize) - 1);
            snap.range_row_min = rmin as i32;
            snap.range_row_max = rmax as i32;
            snap.range_col_min = cmin as i32;
            snap.range_col_max = cmax as i32;
            // anchor 仍当成"主选中"，让公式栏和 coord 跟着 anchor
            snap.selected_cell_row = r1 as i32;
            snap.selected_cell_col = c1.min((snap.col_count.max(1) as usize) - 1) as i32;
            snap.coord = format!("{}{}:{}{}", col_letter(cmin), rmin + 1, col_letter(cmax), rmax + 1);
            snap.formula_editable = false;
            snap.formula_display = String::new();
            snap.selection_info = format!(
                "{}{}:{}{} ({}行×{}列, 共{}格)",
                col_letter(cmin), rmin + 1,
                col_letter(cmax), rmax + 1,
                rmax - rmin + 1, cmax - cmin + 1,
                (rmax - rmin + 1) * (cmax - cmin + 1),
            );
        }
        GridSelection::Row(r) => {
            snap.selected_row = r as i32;
            snap.selection_info = format!("第{}行", r + 1);
        }
        GridSelection::Col(c) if (c as i32) < snap.col_count => {
            snap.selected_col = c as i32;
            snap.selection_info = format!("第{}列", col_letter(c));
        }
        _ => {}
    }
}

fn raw_cell(state: &AppState, r: usize, c: usize) -> String {
    raw_cell_for(state, r, c)
}

/// 读取真实存储值（不经过 display 翻译）。供 inline 编辑等取初始文本。
pub fn raw_cell_for(state: &AppState, r: usize, c: usize) -> String {
    match &state.selected {
        Some(SelectedNode::Table { group, name, .. }) => state.engine.find_table(group, name)
            .and_then(|t| t.records.get(r).and_then(|row| row.get(c)).cloned())
            .unwrap_or_default(),
        Some(SelectedNode::Constant { group, name, .. }) => state.engine.find_constant(group, name)
            .and_then(|cst| cst.entries.get(r))
            .map(|e| match c {
                0 => e.name.clone(),
                1 => e.tbl_type.clone(),
                2 => e.value.clone(),
                3 => e.export.code().to_string(),
                4 => e.desc.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        Some(SelectedNode::Enum { group, name, .. }) => state.engine.find_enum(group, name)
            .and_then(|en| en.entries.get(r))
            .map(|e| match c {
                0 => e.id.clone(),
                1 => e.name.clone(),
                2 => e.desc.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        Some(SelectedNode::Project { .. }) | Some(SelectedNode::Group { .. }) | None => String::new(),
    }
}

/// 单元格的展示值（与 build_*_grid 内逻辑一致；用于公式栏只读模式）。
fn cell_display(state: &AppState, snap: &GridSnapshot, _r: usize, c: usize, raw: &str) -> String {
    if raw.is_empty() { return String::new(); }
    match snap.column_kinds.get(c) {
        Some(ColumnKind::Ref { target }) if state.view_show_enum_name => {
            display_for_table_cell(state, &Some(target.clone()), raw)
        }
        Some(ColumnKind::ExportEnumCol) => {
            tbl_core::model::Export::from_str(raw).display().to_string()
        }
        _ => raw.to_string(),
    }
}

pub fn col_letter(idx: usize) -> String {
    let mut s = String::new();
    let mut n = idx;
    loop { s.insert(0, (b'A' + (n % 26) as u8) as char); if n < 26 { break; } n = n / 26 - 1; }
    s
}

/// 取 (r,c) 单元格的具体验证错误信息；当前节点不存在 / 该格无错误时返回 None。
/// 用于 StatusBar 在选中单格时把"红框是因为什么"展示给用户，对齐文档 §5.4 实时验证 UX。
fn cell_validation_message(state: &AppState, r: usize, c: usize) -> Option<String> {
    use tbl_core::validate::{validate_table, validate_constant, validate_enum, RefIndex};
    let sep = &state.engine.project().config.separators;
    let errors = match &state.selected {
        Some(SelectedNode::Table { group, name, .. }) => {
            let table = state.engine.find_table(group, name)?;
            let refs = RefIndex::build(&state.engine.project().groups);
            validate_table(table, sep, Some(&refs))
        }
        Some(SelectedNode::Constant { group, name, .. }) => {
            let constant = state.engine.find_constant(group, name)?;
            validate_constant(constant, sep)
        }
        Some(SelectedNode::Enum { group, name, .. }) => {
            let enum_def = state.engine.find_enum(group, name)?;
            validate_enum(enum_def)
        }
        Some(SelectedNode::Project { .. }) | Some(SelectedNode::Group { .. }) | None => return None,
    };
    errors.into_iter()
        .find(|e| !e.is_schema() && e.row == r && e.col == c)
        .map(|e| e.message)
}

fn build_table_grid(state: &AppState, group: &str, name: &str) -> GridSnapshot {
    let table = match state.engine.find_table(group, name) {
        Some(t) => t,
        None => return GridSnapshot::empty(),
    };
    let fields = &table.schema.fields;

    // schema 错误按 ValidationError.header_row 直接归到 (hi,ci)：
    // hi 取自 TableHeaderRow.row()（0-based UI 行号）。
    // 主键值重复（is_schema()=false）走数据行红框，不在此处理。
    let mut header_errors: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    {
        use tbl_core::validate::validate_table_schema_with_refs;
        let sep = &state.engine.project().config.separators;
        let refs = tbl_core::validate::RefIndex::build(&state.engine.project().groups);
        for err in validate_table_schema_with_refs(table, sep, Some(&refs)) {
            if !err.is_schema() { continue; }
            if let Some(hr) = err.header_row {
                header_errors.insert((hr.row(), err.col));
            }
        }
    }
    let h_err = |hi: usize, ci: usize| header_errors.contains(&(hi, ci));

    let desc_row: Vec<HeaderCell> = fields.iter().enumerate().map(|(ci, f)| HeaderCell {
        text: SharedString::from(f.desc.clone()),
        kind: CellKind::Text,
        color: color_text_primary(),
        has_error: h_err(0, ci),
    }).collect();
    let export_row: Vec<HeaderCell> = fields.iter().enumerate().map(|(ci, f)| {
        let (kind, color) = if f.name == "id" {
            (CellKind::ReadOnly, color_text_readonly())
        } else {
            (CellKind::ExportEnum, color_text_success())
        };
        HeaderCell {
            text: SharedString::from(f.export.display().to_string()),
            kind,
            color,
            has_error: h_err(1, ci),
        }
    }).collect();
    let type_row: Vec<HeaderCell> = fields.iter().enumerate().map(|(ci, f)| {
        let kind = if f.name == "id" { CellKind::ReadOnly } else { CellKind::TypeEnum };
        let color = if f.name == "id" { color_text_readonly() } else { color_text_info() };
        HeaderCell {
            text: SharedString::from(f.tbl_type.clone()),
            kind,
            color,
            has_error: h_err(2, ci),
        }
    }).collect();
    let field_row: Vec<HeaderCell> = fields.iter().enumerate().map(|(ci, f)| {
        let kind = if f.name == "id" { CellKind::ReadOnly } else { CellKind::Text };
        let color = if f.name == "id" { color_text_readonly() } else { color_text_primary() };
        HeaderCell {
            text: SharedString::from(f.name.clone()),
            kind,
            color,
            has_error: h_err(3, ci),
        }
    }).collect();

    let valid_count = table.records.iter()
        .filter(|r| r.first().map_or(false, |id| !id.is_empty()))
        .count();

    // 识别引用列（@TableName / @EnumName）
    let ref_targets: Vec<Option<String>> = fields.iter()
        .map(|f| f.tbl_type.strip_prefix('@').map(|s| s.trim().to_string()))
        .collect();
    let column_kinds: Vec<ColumnKind> = ref_targets.iter().enumerate().map(|(i, t)| {
        if i == 0 { return ColumnKind::Text; } // id 列：数据行可编辑（与 egui 一致）；表头才是 ReadOnly
        match t {
            Some(name) => ColumnKind::Ref { target: name.clone() },
            None => ColumnKind::Text,
        }
    }).collect();

    // 表头四行的 ColumnKind：与 egui 对齐——desc 行整行 Text；export/type/field 三行 id 列固定 ReadOnly。
    let mk_header_kinds_id_ro = |non_id: ColumnKind| -> Vec<ColumnKind> {
        fields.iter().enumerate().map(|(i, _)| {
            if i == 0 { ColumnKind::ReadOnly } else { non_id.clone() }
        }).collect()
    };
    let header_kinds: Vec<Vec<ColumnKind>> = vec![
        fields.iter().map(|_| ColumnKind::Text).collect(),  // desc：整行可编辑
        mk_header_kinds_id_ro(ColumnKind::ExportEnumCol),    // export
        mk_header_kinds_id_ro(ColumnKind::TypeEnumCol),      // type
        mk_header_kinds_id_ro(ColumnKind::Text),             // field
    ];

    let display_rows = valid_count + EXTRA_ROWS;
    let mut data_rows = Vec::with_capacity(display_rows);
    for r in 0..display_rows {
        let row_data = table.records.get(r);
        let cells: Vec<DataCell> = (0..fields.len()).map(|c| {
            let raw = row_data.and_then(|row| row.get(c)).cloned().unwrap_or_default();
            let has_error = state.engine.has_active_cell_error(group, name, r, c);
            let is_empty_ref = ref_targets[c].is_some() && raw.is_empty();
            let display = display_for_table_cell(state, &ref_targets[c], &raw);
            DataCell {
                text: SharedString::from(display),
                has_error,
                is_empty_ref,
            }
        }).collect();
        data_rows.push(DataRow {
            cells: slint::ModelRc::new(slint::VecModel::from(cells)),
            row_color: color_transparent(),
            selected: false,
        });
    }

    GridSnapshot {
        title: format!("📊 {} / {}", group, name),
        subtitle: format!("Table · {} 行 · {} 列", valid_count, fields.len()),
        col_count: fields.len() as i32,
        header_rows: vec![desc_row, export_row, type_row, field_row],
        data_rows,
        column_kinds,
        header_kinds,
        data_count: valid_count,
        ..GridSnapshot::empty()
    }
}

fn build_constant_grid(state: &AppState, group: &str, name: &str) -> GridSnapshot {
    let constant = match state.engine.find_constant(group, name) {
        Some(c) => c,
        None => return GridSnapshot::empty(),
    };

    let header_row = vec![
        HeaderCell { text: "name".into(),   kind: CellKind::ReadOnly, color: color_text_readonly(), has_error: false },
        HeaderCell { text: "type".into(),   kind: CellKind::ReadOnly, color: color_text_info(), has_error: false },
        HeaderCell { text: "value".into(),  kind: CellKind::ReadOnly, color: color_text_readonly(), has_error: false },
        HeaderCell { text: "export".into(), kind: CellKind::ReadOnly, color: color_text_success(), has_error: false },
        HeaderCell { text: "desc".into(),   kind: CellKind::ReadOnly, color: color_text_readonly(), has_error: false },
    ];

    let valid_count = constant.entries.iter().filter(|e| !e.name.is_empty()).count();

    let display_rows = valid_count + EXTRA_ROWS;
    let mut data_rows = Vec::with_capacity(display_rows);
    for r in 0..display_rows {
        let entry = constant.entries.get(r);
        let cells = match entry {
            Some(e) => {
                let export_disp = e.export.display().to_string();
                vec![
                    plain_cell(state, group, name, r, 0, &e.name),
                    plain_cell(state, group, name, r, 1, &e.tbl_type),
                    plain_cell(state, group, name, r, 2, &e.value),
                    plain_cell(state, group, name, r, 3, &export_disp),
                    plain_cell(state, group, name, r, 4, &e.desc),
                ]
            }
            None => (0..5).map(|c| empty_cell(state, group, name, r, c)).collect(),
        };
        data_rows.push(DataRow {
            cells: slint::ModelRc::new(slint::VecModel::from(cells)),
            row_color: color_transparent(),
            selected: false,
        });
    }

    GridSnapshot {
        title: format!("📋 {} / {}", group, name),
        subtitle: format!("Constant · {} 项", valid_count),
        col_count: 5,
        header_rows: vec![header_row],
        data_rows,
        column_kinds: vec![
            ColumnKind::Text,
            ColumnKind::TypeEnumCol,
            ColumnKind::Text,
            ColumnKind::ExportEnumCol,
            ColumnKind::Text,
        ],
        header_kinds: vec![vec![ColumnKind::ReadOnly; 5]],
        data_count: valid_count,
        ..GridSnapshot::empty()
    }
}

fn build_enum_grid(state: &AppState, group: &str, name: &str) -> GridSnapshot {
    let enum_def = match state.engine.find_enum(group, name) {
        Some(e) => e,
        None => return GridSnapshot::empty(),
    };

    let header_row = vec![
        HeaderCell { text: "id".into(),   kind: CellKind::ReadOnly, color: color_text_readonly(), has_error: false },
        HeaderCell { text: "name".into(), kind: CellKind::ReadOnly, color: color_text_readonly(), has_error: false },
        HeaderCell { text: "desc".into(), kind: CellKind::ReadOnly, color: color_text_readonly(), has_error: false },
    ];

    let valid_count = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() || !e.name.is_empty())
        .count();

    let display_rows = valid_count + EXTRA_ROWS;
    let mut data_rows = Vec::with_capacity(display_rows);
    for r in 0..display_rows {
        let entry = enum_def.entries.get(r);
        let cells = match entry {
            Some(e) => vec![
                plain_cell(state, group, name, r, 0, &e.id),
                plain_cell(state, group, name, r, 1, &e.name),
                plain_cell(state, group, name, r, 2, &e.desc),
            ],
            None => (0..3).map(|c| empty_cell(state, group, name, r, c)).collect(),
        };
        data_rows.push(DataRow {
            cells: slint::ModelRc::new(slint::VecModel::from(cells)),
            row_color: color_transparent(),
            selected: false,
        });
    }

    GridSnapshot {
        title: format!("🔢 {} / {}", group, name),
        subtitle: format!("Enum · {} 项", valid_count),
        col_count: 3,
        header_rows: vec![header_row],
        data_rows,
        column_kinds: vec![ColumnKind::Text, ColumnKind::Text, ColumnKind::Text],
        header_kinds: vec![vec![ColumnKind::ReadOnly; 3]],
        data_count: valid_count,
        ..GridSnapshot::empty()
    }
}

// ──────── 单元格构造辅助 ────────

fn empty_cell(state: &AppState, group: &str, name: &str, r: usize, c: usize) -> DataCell {
    let has_error = state.engine.has_active_cell_error(group, name, r, c);
    DataCell {
        text: SharedString::new(),
        has_error,
        is_empty_ref: false,
    }
}

fn plain_cell(state: &AppState, group: &str, name: &str, r: usize, c: usize, text: &str) -> DataCell {
    let has_error = state.engine.has_active_cell_error(group, name, r, c);
    DataCell {
        text: SharedString::from(text),
        has_error,
        is_empty_ref: false,
    }
}

/// Table 单元格的 display 转换：处理 @EnumName 引用列的 id → entry.name 映射。
fn display_for_table_cell(state: &AppState, ref_target: &Option<String>, raw: &str) -> String {
    if raw.is_empty() { return String::new(); }
    if !state.view_show_enum_name { return raw.to_string(); }
    let ref_name = match ref_target { Some(n) => n, None => return raw.to_string() };
    for g in &state.engine.project().groups {
        for e in &g.enums {
            if e.deleted || e.name != *ref_name { continue; }
            if let Some(entry) = e.entries.iter().find(|en| en.id == raw && !en.name.is_empty()) {
                return entry.name.clone();
            }
            return raw.to_string();
        }
    }
    raw.to_string()
}
