// convert：纯函数，把 AppState 派生成 slint Model 数据。
// 不持有引用，每次调用产出新 Vec，调用方负责 push 到 AppWindow。

use slint::{Color, SharedString};
use crate::state::{AppState, ColumnKind, GridSelection, SelectedNode, TreeFilter, TreeTarget};
use crate::{CellKind, DataCell, DataRow, HeaderCell, TreeNode};

pub const EXTRA_ROWS: usize = 5;

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
pub fn build_tree_nodes(state: &mut AppState) -> Vec<TreeNode> {
    state.tree_targets.clear();
    let mut nodes = Vec::new();

    let filter = state.tree_filter.clone();
    let show_full = state.tree_full_group;
    let search = state.tree_search.to_lowercase();

    let groups = state.engine.project.groups.clone();
    for group in &groups {
        let group_has_filter_match = filter == TreeFilter::All
            || group.tables.iter().any(|t| passes_filter(&filter, t.deleted, t.original.is_empty(), t.dirty))
            || group.constants.iter().any(|c| passes_filter(&filter, c.deleted, c.original.is_empty(), c.dirty))
            || group.enums.iter().any(|e| passes_filter(&filter, e.deleted, e.original.is_empty(), e.dirty));
        if !group_has_filter_match { continue; }

        let group_match_search = search.is_empty()
            || group.name.to_lowercase().contains(&search)
            || group.tables.iter().any(|t| t.name.to_lowercase().contains(&search))
            || group.constants.iter().any(|c| c.name.to_lowercase().contains(&search))
            || group.enums.iter().any(|e| e.name.to_lowercase().contains(&search));
        if !group_match_search { continue; }

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
        let group_has_errors = state.engine.validation_errors.iter().any(|(g, _, _, _)| g == &group.name);

        let expanded = state.tree_expanded.contains(&group.name);
        let group_selected = matches!(&state.selected,
            Some(SelectedNode::Table { group: g, .. } | SelectedNode::Constant { group: g, .. } | SelectedNode::Enum { group: g, .. })
            if g == &group.name && false // 组本身不显示选中态，仅子节点
        );

        let (group_mark, group_mark_color) = marker(group_deleted, group_is_new, group_dirty, group_has_errors);
        nodes.push(TreeNode {
            id: state.tree_targets.len() as i32,
            indent: 0,
            expanded,
            icon: SharedString::from(ICON_GROUP),
            name: SharedString::from(group.name.clone()),
            mark: SharedString::from(group_mark),
            mark_color: group_mark_color,
            is_group: true,
            selected: group_selected,
        });
        state.tree_targets.push(TreeTarget::Group(group.name.clone()));

        if !expanded { continue; }

        for t in &group.tables {
            if !show_full && !passes_filter(&filter, t.deleted, t.original.is_empty(), t.dirty) { continue; }
            if !search.is_empty() && !t.name.to_lowercase().contains(&search)
                && !group.name.to_lowercase().contains(&search) { continue; }
            let selected = matches!(&state.selected,
                Some(SelectedNode::Table { group: g, name: n }) if g == &group.name && n == &t.name);
            let has_err = state.engine.validation_errors.iter()
                .any(|(g, n, _, _)| g == &group.name && n == &t.name);
            let (mark, mc) = marker(t.deleted, t.original.is_empty(), t.dirty, has_err);
            nodes.push(TreeNode {
                id: state.tree_targets.len() as i32,
                indent: 1,
                expanded: false,
                icon: SharedString::from(ICON_TABLE),
                name: SharedString::from(t.name.clone()),
                mark: SharedString::from(mark),
                mark_color: mc,
                is_group: false,
                selected,
            });
            state.tree_targets.push(TreeTarget::Table { group: group.name.clone(), name: t.name.clone() });
        }
        for c in &group.constants {
            if !show_full && !passes_filter(&filter, c.deleted, c.original.is_empty(), c.dirty) { continue; }
            if !search.is_empty() && !c.name.to_lowercase().contains(&search)
                && !group.name.to_lowercase().contains(&search) { continue; }
            let selected = matches!(&state.selected,
                Some(SelectedNode::Constant { group: g, name: n }) if g == &group.name && n == &c.name);
            let has_err = state.engine.validation_errors.iter()
                .any(|(g, n, _, _)| g == &group.name && n == &c.name);
            let (mark, mc) = marker(c.deleted, c.original.is_empty(), c.dirty, has_err);
            nodes.push(TreeNode {
                id: state.tree_targets.len() as i32,
                indent: 1,
                expanded: false,
                icon: SharedString::from(ICON_CONST),
                name: SharedString::from(c.name.clone()),
                mark: SharedString::from(mark),
                mark_color: mc,
                is_group: false,
                selected,
            });
            state.tree_targets.push(TreeTarget::Constant { group: group.name.clone(), name: c.name.clone() });
        }
        for e in &group.enums {
            if !show_full && !passes_filter(&filter, e.deleted, e.original.is_empty(), e.dirty) { continue; }
            if !search.is_empty() && !e.name.to_lowercase().contains(&search)
                && !group.name.to_lowercase().contains(&search) { continue; }
            let selected = matches!(&state.selected,
                Some(SelectedNode::Enum { group: g, name: n }) if g == &group.name && n == &e.name);
            let has_err = state.engine.validation_errors.iter()
                .any(|(g, n, _, _)| g == &group.name && n == &e.name);
            let (mark, mc) = marker(e.deleted, e.original.is_empty(), e.dirty, has_err);
            nodes.push(TreeNode {
                id: state.tree_targets.len() as i32,
                indent: 1,
                expanded: false,
                icon: SharedString::from(ICON_ENUM),
                name: SharedString::from(e.name.clone()),
                mark: SharedString::from(mark),
                mark_color: mc,
                is_group: false,
                selected,
            });
            state.tree_targets.push(TreeTarget::Enum { group: group.name.clone(), name: e.name.clone() });
        }
    }
    nodes
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
        Some(SelectedNode::Table { group, name }) => build_table_grid(state, &group, &name),
        Some(SelectedNode::Constant { group, name }) => build_constant_grid(state, &group, &name),
        Some(SelectedNode::Enum { group, name }) => build_enum_grid(state, &group, &name),
        None => GridSnapshot::empty(),
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
            snap.formula_editable = matches!(kind, Some(ColumnKind::Text));
            // 公式栏只展示 Text 列内容；Export/Type/Ref/ReadOnly 列以下拉/弹窗操作，公式栏留空。
            snap.formula_display = if matches!(kind, Some(ColumnKind::Text)) {
                let raw = raw_cell(state, r, c);
                if !raw.is_empty() { cell_display(state, snap, r, c, &raw) } else { String::new() }
            } else {
                String::new()
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
        Some(SelectedNode::Table { group, name }) => state.engine.find_table(group, name)
            .and_then(|t| t.records.get(r).and_then(|row| row.get(c)).cloned())
            .unwrap_or_default(),
        Some(SelectedNode::Constant { group, name }) => state.engine.find_constant(group, name)
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
        Some(SelectedNode::Enum { group, name }) => state.engine.find_enum(group, name)
            .and_then(|en| en.entries.get(r))
            .map(|e| match c {
                0 => e.id.clone(),
                1 => e.name.clone(),
                2 => e.desc.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        None => String::new(),
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
    use tbl_core::validate::{
        validate_table_cell_with_refs, validate_constant_cell, validate_enum_cell, RefIndex,
    };
    let sep = &state.engine.project.config.separators;
    match &state.selected {
        Some(SelectedNode::Table { group, name }) => {
            let table = state.engine.find_table(group, name)?;
            let refs = RefIndex::build(&state.engine.project.groups);
            if let Some(msg) = validate_table_cell_with_refs(table, r, c, sep, Some(&refs)) {
                return Some(msg);
            }
            // 主键 id 重复检测
            if let Some(idx) = table.schema.fields.iter().position(|f| f.name == "id") {
                if c == idx {
                    let id = table.records.get(r).and_then(|row| row.get(idx))
                        .map(|s| s.as_str()).unwrap_or("");
                    if !id.is_empty() {
                        let mut count = 0;
                        for row in &table.records {
                            if row.get(idx).map_or(false, |v| v == id) { count += 1; }
                        }
                        if count > 1 { return Some(format!("ID \"{}\" 重复", id)); }
                    }
                }
            }
            None
        }
        Some(SelectedNode::Constant { group, name }) => {
            let constant = state.engine.find_constant(group, name)?;
            let entry = constant.entries.get(r)?;
            if let Some(msg) = validate_constant_cell(entry, c, sep) { return Some(msg); }
            // name 重复
            if c == 0 && !entry.name.is_empty() {
                let count = constant.entries.iter().filter(|e| e.name == entry.name).count();
                if count > 1 { return Some(format!("name \"{}\" 重复", entry.name)); }
            }
            // name 已填但 value 为空
            if c == 2 && !entry.name.is_empty() && entry.value.is_empty() {
                return Some("name已填但value为空".to_string());
            }
            None
        }
        Some(SelectedNode::Enum { group, name }) => {
            let enum_def = state.engine.find_enum(group, name)?;
            let entry = enum_def.entries.get(r)?;
            if let Some(msg) = validate_enum_cell(entry, c) { return Some(msg); }
            // id / name 重复
            if c == 0 && !entry.id.is_empty() {
                let count = enum_def.entries.iter().filter(|e| e.id == entry.id).count();
                if count > 1 { return Some(format!("id \"{}\" 重复", entry.id)); }
            }
            if c == 1 && !entry.name.is_empty() {
                let count = enum_def.entries.iter().filter(|e| e.name == entry.name).count();
                if count > 1 { return Some(format!("name \"{}\" 重复", entry.name)); }
            }
            None
        }
        None => None,
    }
}

fn build_table_grid(state: &AppState, group: &str, name: &str) -> GridSnapshot {
    let table = match state.engine.find_table(group, name) {
        Some(t) => t,
        None => return GridSnapshot::empty(),
    };
    let fields = &table.schema.fields;

    // 把 schema 错误按"问题归属哪一行表头"分散成 (hi,ci) 的红框集合：
    //   类型不合法 / 引用不存在 → type 行（hi=2）
    //   字段名非法 / 关键字 / 重复 / 第一列必须是 id → field 行（hi=3）
    //   主键值重复 → 不是表头错误（已在 row 级 cell.has_error 处理）
    let mut header_errors: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    {
        use tbl_core::validate::validate_table_schema_with_refs;
        let sep = &state.engine.project.config.separators;
        let refs = tbl_core::validate::RefIndex::build(&state.engine.project.groups);
        for sch in validate_table_schema_with_refs(table, sep, Some(&refs)) {
            // SchemaError 没有 col 字段，按 field name 反查 col；找不到 (空 / 主键消息) 时 col=0
            let ci = fields.iter().position(|f| f.name == sch.field).unwrap_or(0);
            let hi = if sch.message.contains("类型") || sch.message.contains("引用") {
                2  // type 行
            } else if sch.message.contains("主键值") {
                continue;  // 这条已经由 row 级 dup-id 标记覆盖
            } else {
                3  // field 行
            };
            header_errors.insert((hi, ci));
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
            let has_error = state.engine.validation_errors
                .contains(&(group.to_string(), name.to_string(), r, c));
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
    let has_error = state.engine.validation_errors
        .contains(&(group.to_string(), name.to_string(), r, c));
    DataCell {
        text: SharedString::new(),
        has_error,
        is_empty_ref: false,
    }
}

fn plain_cell(state: &AppState, group: &str, name: &str, r: usize, c: usize, text: &str) -> DataCell {
    let has_error = state.engine.validation_errors
        .contains(&(group.to_string(), name.to_string(), r, c));
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
    for g in &state.engine.project.groups {
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
