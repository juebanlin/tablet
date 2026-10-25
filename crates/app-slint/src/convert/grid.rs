// 表格区派生：把当前选中节点投影成 GridSnapshot。
//
// snapshot 包含 title/subtitle、多行表头、数据行、列与表头的 ColumnKind、
// 当前选区联动（cell 坐标、coord、公式栏、状态栏）。
// 调用方读取 snapshot 后写入 AppWindow grid-* 属性。

use slint::SharedString;

use tablet_core::types::{Paradigm, TblType, TypeParam};

use super::util::{cell_validation_message, col_letter, one_line, raw_cell_for, EXTRA_ROWS};
use crate::state::{AppState, ColumnKind, GridSelection, SelectedNode};
use crate::icons::*;
use crate::theme::*;
use crate::{CellKind, DataCell, DataRow, HeaderCell};

/// GridSection 的快照：title / subtitle / col_count / 多行表头 / 数据行 / 选区联动字段。
pub struct GridSnapshot {
    pub title: String,
    pub subtitle: String,
    pub col_count: i32,
    /// 按列顺序的稳定列名（Table = 字段名；Constant/Enum = 固定系统列）。
    /// 用户列宽按列名存，插入/删除/重排后仍能对上。
    pub column_names: Vec<String>,
    pub header_rows: Vec<Vec<HeaderCell>>,
    pub data_rows: Vec<DataRow>,
    /// 当前节点每列的可编辑性，由 main.rs 同步写回 AppState
    pub column_kinds: Vec<ColumnKind>,
    /// 当前节点表头每个 cell 的可编辑性（按 [hrow][col] 索引）
    pub header_kinds: Vec<Vec<ColumnKind>>,
    pub data_count: usize,
    pub coord: String,
    /// 非编辑态公式栏显示的展示值（已单行化：只取首行 + `…`）
    pub formula_display: String,
    /// 当前选中 cell 的存储真值原样（含换行）。展开态公式栏用它渲染多行内容。
    pub formula_raw: String,
    /// 当前选中 cell 是否允许编辑（Text 列且非占位行外限制）
    pub formula_editable: bool,
    /// 当前选中是否是**单格**。区域/整行/整列选区时公式栏强制折叠，
    /// 否则会保留上一次展开高度并显示过期的 raw 内容。
    pub formula_single: bool,
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
            column_names: Vec::new(),
            header_rows: Vec::new(),
            data_rows: Vec::new(),
            column_kinds: Vec::new(),
            header_kinds: Vec::new(),
            data_count: 0,
            coord: String::new(),
            formula_display: String::new(),
            formula_raw: String::new(),
            formula_editable: false,
            formula_single: false,
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
    // 先清掉上一个选区的公式栏内容；区域/整行/整列没有单格内容，
    // 不能让上一次的 raw 文本和展开高度留着。
    snap.formula_display.clear();
    snap.formula_raw.clear();
    snap.formula_editable = false;
    snap.formula_single = false;

    match state.grid_selection {
        GridSelection::Cell(r, c) if (c as i32) < snap.col_count => {
            snap.selected_cell_row = r as i32;
            snap.selected_cell_col = c as i32;
            snap.coord = format!("{}{}", col_letter(c), r + 1);
            snap.formula_single = true;
            let kind = snap.column_kinds.get(c);
            // 公式栏只允许 Text 列编辑；picker / ReadOnly 列只读 + 显示原始存储值（@04.5.3）
            snap.formula_editable = matches!(kind, Some(ColumnKind::Text));
            // 存储真值原样：展开态公式栏渲染真实换行用
            let raw = raw_cell_for(state, r, c);
            // 折叠态公式栏是单行盒子 → 一律单行化（只取首行 + …），否则多行值会溢出栏外
            snap.formula_display = one_line(&if matches!(kind, Some(ColumnKind::Text)) {
                if raw.is_empty() { String::new() } else { cell_display(state, snap, r, c, &raw) }
            } else {
                // picker / ReadOnly：显示底层 raw（"1001" / "cs"），不走 cell_display 翻译
                raw.clone()
            });
            snap.formula_raw = raw;
            // 单格选中时，若该格有验证错误，状态栏在坐标后追加错误信息（实时验证 UX）。
            snap.selection_info = match cell_validation_message(state, r, c) {
                Some(msg) => format!("{} {} {}", snap.coord, WARN, msg),
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
            // 公式栏/Name Box 用 Excel 地址风格：整行 = `1:1`；
            // 状态栏用完整单元格范围 + 维度：`A1:C1 (3列)`。
            let last = col_letter(snap.col_count.max(1) as usize - 1);
            let full = format!("A{0}:{1}{0}", r + 1, last);
            snap.coord = format!("{0}:{0}", r + 1);
            snap.selection_info = format!(
                "{} ({}列)",
                full,
                snap.col_count.max(0),
            );
        }
        GridSelection::Col(c) if (c as i32) < snap.col_count => {
            snap.selected_col = c as i32;
            // 公式栏/Name Box：整列 = `A:A`；状态栏：`A1:A3 (3行)`。
            let letter = col_letter(c);
            let rows = snap.data_count.max(1);
            let full = format!("{0}1:{0}{1}", letter, rows);
            snap.coord = format!("{0}:{0}", letter);
            snap.selection_info = format!(
                "{} ({}行)",
                full,
                snap.data_count.max(0),
            );
        }
        _ => {}
    }
}

/// 单元格的展示值（与 build_*_grid 内逻辑一致；用于公式栏只读模式）。
fn cell_display(state: &AppState, snap: &GridSnapshot, _r: usize, c: usize, raw: &str) -> String {
    if raw.is_empty() { return String::new(); }
    match snap.column_kinds.get(c) {
        Some(ColumnKind::Ref { target }) if state.view_show_ref_name => {
            let maps = build_ref_display_maps(state, std::iter::once(target.as_str()));
            display_for_table_cell(state, &maps, &Some((target.clone(), false, false)), raw)
        }
        Some(ColumnKind::RefList { target }) if state.view_show_ref_name => {
            let maps = build_ref_display_maps(state, std::iter::once(target.as_str()));
            display_for_table_cell(state, &maps, &Some((target.clone(), true, column_is_set(state, c))), raw)
        }
        Some(ColumnKind::ExportEnumCol) => {
            tablet_core::model::Export::from_input(raw).display().to_string()
        }
        _ => raw.to_string(),
    }
}

/// 判断列类型是否为 `Set<@X>`（枚举名展示需用 set 分隔符而非 list）。仅 Table 场景有效。
fn column_is_set(state: &AppState, c: usize) -> bool {
    let (group, name) = match &state.selected {
        Some(SelectedNode::Table { group, name, .. }) => (group, name),
        _ => return false,
    };
    state.engine.find_table(group, name)
        .and_then(|t| t.schema.fields.get(c))
        .and_then(|f| tablet_core::types::TblType::parse(&f.tbl_type))
        .map(|t| t.paradigm == tablet_core::types::Paradigm::Set)
        .unwrap_or(false)
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
        use tablet_core::validate::validate_table_schema_with_refs;
        let sep = &state.engine.project().schema.separators;
        let refs = tablet_core::validate::RefIndex::build(&state.engine.project().groups);
        let policy = state.engine.project().config.validate.clone();
        for err in validate_table_schema_with_refs(table, sep, Some(&refs)) {
            if !err.is_schema() { continue; }
            // 红框只表达「阻断保存」的级别；仅提示 / 忽略的走状态栏 / 日志。
            if policy.action(err.severity) != tablet_core::model::SeverityAction::Block { continue; }
            if let Some(hr) = err.header_row {
                header_errors.insert((hr.row(), err.col));
            }
        }
    }
    let h_err = |hi: usize, ci: usize| header_errors.contains(&(hi, ci));

    // 表头 / 数据格都是固定 22px 单行盒子，展示串统一单行化（@convert::util::one_line）
    let desc_row: Vec<HeaderCell> = fields.iter().enumerate().map(|(ci, f)| HeaderCell {
        text: SharedString::from(one_line(&f.desc)),
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
            text: SharedString::from(one_line(&f.tbl_type)),
            kind,
            color,
            has_error: h_err(2, ci),
        }
    }).collect();
    let field_row: Vec<HeaderCell> = fields.iter().enumerate().map(|(ci, f)| {
        let kind = if f.name == "id" { CellKind::ReadOnly } else { CellKind::Text };
        let color = if f.name == "id" { color_text_readonly() } else { color_text_primary() };
        HeaderCell {
            text: SharedString::from(one_line(&f.name)),
            kind,
            color,
            has_error: h_err(3, ci),
        }
    }).collect();

    let valid_count = table.records.iter()
        .filter(|r| r.first().map_or(false, |id| !id.is_empty()))
        .count();

    // 识别引用列：@TableName / @EnumName → 单选 Ref；List<@X> / Set<@X> → 多选 RefList
    let ref_targets: Vec<Option<(String, bool, bool)>> = fields.iter().map(|f| {
        let s = f.tbl_type.trim();
        if let Some(name) = s.strip_prefix('@') {
            return Some((name.trim().to_string(), false, false));
        }
        if let Some(t) = TblType::parse(s) {
            if matches!(t.paradigm, Paradigm::List | Paradigm::Set) {
                if let Some(TypeParam::Ref(name)) = t.params.first() {
                    return Some((name.clone(), true, t.paradigm == Paradigm::Set));
                }
            }
        }
        None
    }).collect();
    let column_kinds: Vec<ColumnKind> = ref_targets.iter().enumerate().map(|(i, t)| {
        if i == 0 { return ColumnKind::Text; } // id 列：数据行可编辑；表头才是 ReadOnly
        match t {
            Some((name, false, _)) => ColumnKind::Ref { target: name.clone() },
            Some((name, true, _)) => ColumnKind::RefList { target: name.clone() },
            None => ColumnKind::Text,
        }
    }).collect();

    // 表头四行的 ColumnKind：desc 行整行 Text；export/type/field 三行 id 列固定 ReadOnly。
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

    // 被引用展示列（@10）：按本网格用到的目标预建 id→显示值 映射（一次构建，单元格 O(1) 查）。
    let ref_maps = if state.view_show_ref_name {
        build_ref_display_maps(state, ref_targets.iter().filter_map(|t| t.as_ref().map(|(n, _, _)| n.as_str())))
    } else {
        RefDisplayMaps::new()
    };

    let display_rows = valid_count + EXTRA_ROWS;
    let mut data_rows = Vec::with_capacity(display_rows);
    for r in 0..display_rows {
        let row_data = table.records.get(r);
        let cells: Vec<DataCell> = (0..fields.len()).map(|c| {
            let raw = row_data.and_then(|row| row.get(c)).cloned().unwrap_or_default();
            let has_error = state.engine.has_active_cell_error(group, name, r, c);
            let is_empty_ref = ref_targets[c].is_some() && raw.is_empty();
            let display = display_for_table_cell(state, &ref_maps, &ref_targets[c], &raw);
            DataCell {
                text: SharedString::from(one_line(&display)),
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
        title: format!("{} {} / {}", ICON_TABLE, group, name),
        subtitle: format!("Table · {} 行 · {} 列", valid_count, fields.len()),
        col_count: fields.len() as i32,
        column_names: fields.iter().map(|f| f.name.clone()).collect(),
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

    // 被引用展示列（@10）：预建本 Constant 各引用目标的 id→显示值 映射。
    let const_ref_targets: Vec<Option<(String, bool, bool)>> =
        constant.entries.iter().map(const_value_ref_target).collect();
    let ref_maps = if state.view_show_ref_name {
        build_ref_display_maps(state, const_ref_targets.iter().filter_map(|t| t.as_ref().map(|(n, _, _)| n.as_str())))
    } else {
        RefDisplayMaps::new()
    };

    let display_rows = valid_count + EXTRA_ROWS;
    let mut data_rows = Vec::with_capacity(display_rows);
    for r in 0..display_rows {
        let entry = constant.entries.get(r);
        let cells = match entry {
            Some(e) => {
                let export_disp = e.export.display().to_string();
                // value 列：若 tbl_type 是 @Xxx 引用（或 List<@X>），按总闸走 id→显示值 翻译
                let value_ref_target = const_ref_targets.get(r).cloned().flatten();
                let value_disp = display_for_table_cell(state, &ref_maps, &value_ref_target, &e.value);
                let value_has_error = state.engine.has_active_cell_error(group, name, r, 2);
                let value_is_empty_ref = value_ref_target.is_some() && e.value.is_empty();
                vec![
                    plain_cell(state, group, name, r, 0, &e.name),
                    plain_cell(state, group, name, r, 1, &e.tbl_type),
                    DataCell {
                        text: SharedString::from(one_line(&value_disp)),
                        has_error: value_has_error,
                        is_empty_ref: value_is_empty_ref,
                    },
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
        title: format!("{} {} / {}", ICON_CONST, group, name),
        subtitle: format!("Constant · {} 项", valid_count),
        col_count: 5,
        column_names: vec!["name".into(), "type".into(), "value".into(), "export".into(), "desc".into()],
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
        title: format!("{} {} / {}", ICON_ENUM, group, name),
        subtitle: format!("Enum · {} 项", valid_count),
        col_count: 3,
        column_names: vec!["id".into(), "name".into(), "desc".into()],
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
        text: SharedString::from(one_line(text)),
        has_error,
        is_empty_ref: false,
    }
}

// ──────── 被引用展示列（@10）────────

/// 目标名 → (id → 显示值)。`None` = 该目标不翻译（无 display_field / 列不合格 / 不是表或枚举）。
/// 每次 build_grid 按当前网格用到的目标构建一次，单元格查表 O(1)——避免逐格扫全表。
type RefDisplayMaps = std::collections::HashMap<String, Option<std::collections::HashMap<String, String>>>;

/// 为若干目标名构建 id→显示值 映射。
fn build_ref_display_maps<'a>(
    state: &AppState,
    targets: impl Iterator<Item = &'a str>,
) -> RefDisplayMaps {
    let mut maps = RefDisplayMaps::new();
    for t in targets {
        if maps.contains_key(t) { continue; }
        maps.insert(t.to_string(), build_one_target_map(state, t));
    }
    maps
}

/// 单个目标（表 / 枚举）的 id→显示值 映射。无 display_field 或列不合格 → None。
fn build_one_target_map(state: &AppState, target: &str) -> Option<std::collections::HashMap<String, String>> {
    use std::collections::HashMap;
    let groups = &state.engine.project().groups;
    // 枚举：display_field ∈ {name, desc}；未显式设 → 默认 name（@10）
    for g in groups {
        for e in &g.enums {
            if e.deleted || e.name != target { continue; }
            let field = e.display_field.as_deref().unwrap_or(tablet_core::model::ENUM_DEFAULT_DISPLAY_FIELD);
            let mut m = HashMap::new();
            for en in &e.entries {
                if en.id.is_empty() { continue; }
                let v = if field == "desc" { &en.desc } else { &en.name };
                if !v.is_empty() { m.insert(en.id.clone(), v.clone()); }
            }
            return Some(m);
        }
    }
    // 表：display_field 必须是 str/int/txt 叶子列
    for g in groups {
        for t in &g.tables {
            if t.deleted || t.name != target { continue; }
            let field = t.display_field.as_deref()?;
            let col = t.schema.fields.iter().position(|f| f.name == field
                && tablet_core::types::TblType::parse(&f.tbl_type).map_or(false, |ty| ty.is_displayable()))?;
            let idc = t.schema.index_col()?;
            let mut m = HashMap::new();
            for row in &t.records {
                let id = row.get(idc).map(|s| s.trim()).unwrap_or("");
                let v = row.get(col).map(|s| s.as_str()).unwrap_or("");
                if !id.is_empty() && !v.is_empty() { m.insert(id.to_string(), v.to_string()); }
            }
            return Some(m);
        }
    }
    None
}

/// Constant value 列的引用目标：`@X` / `List<@X>` / `Set<@X>` → (目标名, multi, is_set)。
fn const_value_ref_target(e: &tablet_core::model::ConstEntry) -> Option<(String, bool, bool)> {
    tablet_core::types::TblType::parse(&e.tbl_type).and_then(|t| match t.paradigm {
        tablet_core::types::Paradigm::Ref => t.ref_name.map(|n| (n, false, false)),
        tablet_core::types::Paradigm::List | tablet_core::types::Paradigm::Set => {
            match t.params.first() {
                Some(tablet_core::types::TypeParam::Ref(n)) => Some((n.clone(), true, t.paradigm == tablet_core::types::Paradigm::Set)),
                _ => None,
            }
        }
        _ => None,
    })
}

/// 引用单元格的 display：按目标节点 `display_field` 把 id 翻成显示值（@10）。
/// `(name, multi, is_set)`：multi 时按 List/Set 分隔符切分逐元素翻译。
fn display_for_table_cell(
    state: &AppState,
    maps: &RefDisplayMaps,
    ref_target: &Option<(String, bool, bool)>,
    raw: &str,
) -> String {
    if raw.is_empty() { return String::new(); }
    if !state.view_show_ref_name { return raw.to_string(); }
    let (ref_name, multi, is_set) = match ref_target { Some(t) => t, None => return raw.to_string() };
    let map = match maps.get(ref_name).and_then(|m| m.as_ref()) {
        Some(m) => m,
        None => return raw.to_string(),
    };
    let lookup = |id: &str| -> String {
        map.get(id.trim()).cloned().unwrap_or_else(|| id.to_string())
    };
    if *multi {
        let seps = &state.engine.project().schema.separators;
        let sep = if *is_set { &seps.set } else { &seps.list };
        return raw.split(sep).map(|s| lookup(s)).collect::<Vec<_>>().join(sep);
    }
    lookup(raw)
}

#[cfg(test)]
mod tests {
    use super::build_grid;
    use crate::state::{AppState, GridSelection, SelectedNode};

    /// 建一个临时仓库，里面有一张表，desc（txt 列）带真实换行 / 制表符。
    /// 返回 (临时目录, AppState)。目录保留到调用方 drop 后手动清理。
    fn workspace_with_multiline_cell() -> (std::path::PathBuf, AppState) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "tablet-oneline-test-{}-{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let proj = dir.join("projects").join("demo");
        std::fs::create_dir_all(proj.join("config").join("enums")).unwrap();
        std::fs::create_dir_all(proj.join("config").join("hero")).unwrap();

        std::fs::write(proj.join("project.tblmanifest"), "\
#!tblmanifest v1
# @meta id: demo

[enums/HeroType] enum
[hero/HeroBase] table
").unwrap();
        std::fs::write(proj.join("config").join("enums").join("HeroType.tbl"), "\
#!tbl v2
#mode enum
---
1|WARRIOR|战士
").unwrap();
        // desc 写成转义形式：\n = 换行，\t = 制表符（@tbl_str）
        std::fs::write(proj.join("config").join("hero").join("HeroBase.tbl"), "\
#!tbl v3
#mode table
---

@field id
  desc:英雄 ID
  export:cs
  type:int

@field name
  desc:名称
  export:cs
  type:str

@field desc
  desc:描述
  export:cs
  type:txt

[1001]
  name:阿尔托利亚
  desc:第一行\\n第二行\\t尾段
").unwrap();

        let (mut state, _, _) = AppState::load(&dir).unwrap();
        state.selected = Some(SelectedNode::Table {
            project_id: "demo".into(),
            group: "hero".into(),
            name: "HeroBase".into(),
        });
        (dir, state)
    }

    #[test]
    fn single_cell_selection_shows_column_and_row() {
        let (dir, mut state) = workspace_with_multiline_cell();
        state.grid_selection = GridSelection::Cell(0, 0);
        let snap = build_grid(&state);
        assert_eq!(snap.coord, "A1");
        assert_eq!(snap.selection_info, "A1");
        assert!(snap.formula_single);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn whole_row_and_column_use_excel_name_box_style() {
        let (dir, mut state) = workspace_with_multiline_cell();
        state.grid_selection = GridSelection::Row(0);
        let snap = build_grid(&state);
        assert_eq!(snap.coord, "1:1", "公式栏/Name Box 的整行地址");
        assert_eq!(snap.selection_info, "A1:C1 (3列)", "状态栏应显示完整单元格范围");
        assert!(!snap.formula_single);

        state.grid_selection = GridSelection::Col(1);
        let snap = build_grid(&state);
        assert_eq!(snap.coord, "B:B", "公式栏/Name Box 的整列地址");
        assert_eq!(snap.selection_info, "B1:B1 (1行)", "状态栏应显示完整单元格范围");
        assert!(!snap.formula_single);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn range_selection_reports_the_full_range() {
        let (dir, mut state) = workspace_with_multiline_cell();
        state.grid_selection = GridSelection::CellRange { r1: 0, c1: 0, r2: 0, c2: 2 };
        let snap = build_grid(&state);
        assert_eq!(snap.coord, "A1:C1", "公式栏左边应显示完整范围");
        assert_eq!(
            snap.selection_info,
            "A1:C1 (1行×3列, 共3格)",
            "状态栏应显示完整范围，而不是 anchor A1"
        );
        assert!(snap.formula_display.is_empty(), "区域选区没有单格内容");
        assert!(snap.formula_raw.is_empty(), "区域选区不能留下上一格的 raw");
        assert!(!snap.formula_editable, "区域选区不可编辑");
        assert!(!snap.formula_single, "区域选区必须强制折叠公式栏");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiline_cell_is_flattened_for_display_but_kept_raw() {
        let (dir, mut state) = workspace_with_multiline_cell();

        // 选中 desc 列那一格（id/name/desc → 第 2 列）
        state.grid_selection = GridSelection::Cell(0, 2);
        let snap = build_grid(&state);
        assert!(snap.formula_single, "单格选区允许展开公式栏");

        let cell = slint::Model::row_data(&snap.data_rows[0].cells, 2).unwrap().text.to_string();
        assert!(!cell.contains('\n') && !cell.contains('\r'), "数据格展示串残留换行: {cell:?}");
        assert_eq!(cell, "第一行…", "数据格应只取首行 + …");

        // 折叠公式栏：同样只取首行
        assert!(!snap.formula_display.contains('\n'), "公式栏展示串残留换行");
        assert_eq!(snap.formula_display, "第一行…");

        // 展开公式栏 / 编辑缓冲读的 raw：换行必须原样保留（不能丢精度）
        assert_eq!(snap.formula_raw, "第一行\n第二行\t尾段", "raw 值被改动了");

        // 表头 desc 行也走同一条路
        let header = snap.header_rows[0][2].text.to_string();
        assert!(!header.contains('\n'));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
