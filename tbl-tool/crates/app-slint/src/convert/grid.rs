// 表格区派生：把当前选中节点投影成 GridSnapshot。
//
// snapshot 包含 title/subtitle、多行表头、数据行、列与表头的 ColumnKind、
// 当前选区联动（cell 坐标、coord、公式栏、状态栏）。
// 调用方读取 snapshot 后写入 AppWindow grid-* 属性。

use slint::SharedString;

use super::util::{cell_validation_message, col_letter, raw_cell_for, EXTRA_ROWS};
use crate::state::{AppState, ColumnKind, GridSelection, SelectedNode};
use crate::theme::*;
use crate::{CellKind, DataCell, DataRow, HeaderCell};

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
                let raw = raw_cell_for(state, r, c);
                if !raw.is_empty() { cell_display(state, snap, r, c, &raw) } else { String::new() }
            } else {
                // picker / ReadOnly：显示底层 raw（"1001" / "cs"），不走 cell_display 翻译
                raw_cell_for(state, r, c)
            };
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
            snap.selection_info = format!("第{}行", r + 1);
        }
        GridSelection::Col(c) if (c as i32) < snap.col_count => {
            snap.selected_col = c as i32;
            snap.selection_info = format!("第{}列", col_letter(c));
        }
        _ => {}
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
        title: format!("{} {} / {}", ICON_TABLE, group, name),
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
        title: format!("{} {} / {}", ICON_CONST, group, name),
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
        title: format!("{} {} / {}", ICON_ENUM, group, name),
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
