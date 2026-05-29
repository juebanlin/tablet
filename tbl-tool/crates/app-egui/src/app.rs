use std::collections::HashSet;
use eframe::egui;
use tbl_core::model::*;
use tbl_core::ops::{ProjectEngine, ProjectAction};
use crate::ui;
use crate::ui::type_selector::TypeSelectorState;
use crate::ui::ref_picker::RefPickerState;
use crate::ui::schema_dialog::{SchemaExportState, SchemaImportState, DataExportState};

pub struct TblApp {
    pub engine: ProjectEngine,
    pub selected: Option<SelectedNode>,
    pub pending_action: Option<PendingAction>,
    pub input_name: String,
    pub edit_state: EditState,
    pub context_col: Option<usize>,
    pub context_row: Option<usize>,
    pub context_pos: egui::Pos2,
    pub auto_commit_on_blur: bool,
    pub realtime_validate: bool,
    pub tree_filter: TreeFilter,
    pub tree_filter_show_full_group: bool,
    pub tree_expanded: HashSet<String>,
    pub tree_context: Option<TreeContext>,
    pub type_selector: TypeSelectorState,
    pub ref_picker: RefPickerState,
    pub view_show_enum_name: bool,
    pub schema_export: SchemaExportState,
    pub schema_import: SchemaImportState,
    pub data_export: DataExportState,
    theme_applied: bool,
}

#[derive(Clone, Debug)]
pub enum TreeContext {
    Group(String),
    Node { group: String, name: String, kind: tbl_core::ops::NodeKind },
    Blank,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum TreeFilter {
    #[default]
    All,
    New,
    Modified,
    Deleted,
    Changed,
}

#[derive(Clone, Debug, Default)]
pub struct EditState {
    pub selected: Selection,
    pub editing: Option<CellPos>,
    pub edit_buffer: String,
    pub edit_pos: Option<egui::Pos2>,
    pub drag_start: Option<(usize, usize)>,
    pub commit_pending: bool,
    pub hover_cell: Option<(usize, usize)>,
    pub formula_buffer: String,
    pub formula_committed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Selection {
    #[default]
    None,
    Cell(usize, usize),
    CellRange { start: (usize, usize), end: (usize, usize) },
    Row(usize),
    Rows(usize, usize),
    Col(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellPos {
    pub row: usize,
    pub col: usize,
    pub header_row: Option<usize>,
}

#[derive(Clone, Debug)]
pub enum SelectedNode {
    Table { group: String, name: String },
    Constant { group: String, name: String },
    Enum { group: String, name: String },
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    NewGroup,
    NewTable { group: String },
    NewConstant { group: String },
    NewEnum { group: String },
    DeleteGroup { group: String },
    DeleteNode { group: String, name: String },
    RenameGroup { old_name: String },
    RenameNode { group: String, old_name: String },
    CopyNode { group: String, name: String, kind: tbl_core::ops::NodeKind },
}

impl TblApp {
    pub fn new(project: Project) -> Self {
        let group_count = project.groups.len();
        let auto_commit = project.config.ui.as_ref().map_or(true, |u| u.auto_commit_on_blur);
        let rt_validate = project.config.ui.as_ref().map_or(false, |u| u.realtime_validate);
        let expanded: HashSet<String> = project.groups.iter().map(|g| g.name.clone()).collect();
        let mut engine = ProjectEngine::new(project);
        engine.log(format!("已加载 {} 个 Group", group_count));
        Self {
            engine,
            selected: None,
            pending_action: None,
            input_name: String::new(),
            edit_state: EditState::default(),
            context_col: None,
            context_row: None,
            context_pos: egui::Pos2::ZERO,
            auto_commit_on_blur: auto_commit,
            realtime_validate: rt_validate,
            tree_filter: TreeFilter::All,
            tree_filter_show_full_group: false,
            tree_expanded: expanded,
            tree_context: None,
            type_selector: TypeSelectorState::default(),
            ref_picker: RefPickerState::default(),
            view_show_enum_name: false,
            schema_export: SchemaExportState::default(),
            schema_import: SchemaImportState::default(),
            data_export: DataExportState::default(),
            theme_applied: false,
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::light();
        visuals.window_rounding = egui::Rounding::ZERO;
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        visuals.popup_shadow = egui::epaint::Shadow::NONE;
        visuals.widgets.noninteractive.rounding = egui::Rounding::ZERO;
        visuals.widgets.inactive.rounding = egui::Rounding::ZERO;
        visuals.widgets.hovered.rounding = egui::Rounding::ZERO;
        visuals.widgets.active.rounding = egui::Rounding::ZERO;
        visuals.widgets.open.rounding = egui::Rounding::ZERO;
        visuals.menu_rounding = egui::Rounding::ZERO;
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.button_padding = egui::vec2(4.0, 2.0);
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        ctx.set_style(style);
    }

    // --- Delegated accessors ---

    pub fn find_table(&self, group: &str, name: &str) -> Option<&Table> {
        self.engine.find_table(group, name)
    }

    pub fn find_constant(&self, group: &str, name: &str) -> Option<&Constant> {
        self.engine.find_constant(group, name)
    }

    pub fn find_enum(&self, group: &str, name: &str) -> Option<&EnumDef> {
        self.engine.find_enum(group, name)
    }

    pub fn log(&mut self, msg: String) {
        self.engine.log(msg);
    }

    // --- PLACEHOLDER_OPS ---

    pub fn mark_table_dirty(&mut self, group: &str, name: &str) {
        self.engine.mark_table_dirty(group, name);
    }

    pub fn mark_constant_dirty(&mut self, group: &str, name: &str) {
        self.engine.mark_constant_dirty(group, name);
    }

    pub fn commit_edit(&mut self, group: &str, name: &str, row: usize, col: usize) {
        let val = self.edit_state.edit_buffer.clone();
        self.engine.commit_table_cell(group, name, row, col, val);
        self.edit_state.editing = None;
    }

    pub fn commit_header_edit(&mut self, group: &str, name: &str, header_row: usize, col: usize) {
        let val = self.edit_state.edit_buffer.clone();
        self.engine.commit_header_edit(group, name, header_row, col, val);
        self.edit_state.editing = None;
    }

    pub fn insert_row(&mut self, group: &str, name: &str, at: usize) {
        match &self.selected {
            Some(SelectedNode::Enum { .. }) => self.engine.insert_enum_row(group, name, at),
            _ => self.engine.insert_row(group, name, at),
        }
    }

    pub fn delete_row(&mut self, group: &str, name: &str, at: usize) {
        match &self.selected {
            Some(SelectedNode::Enum { .. }) => self.engine.delete_enum_row(group, name, at),
            _ => self.engine.delete_row(group, name, at),
        }
    }

    pub fn insert_column(&mut self, group: &str, name: &str, at: usize) {
        self.engine.insert_column(group, name, at);
    }

    pub fn delete_column(&mut self, group: &str, name: &str, at: usize) {
        self.engine.delete_column(group, name, at);
    }

    pub fn set_cell_value(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str, source: &crate::ui::grid_model::GridSource) {
        use crate::ui::grid_model::GridSource;
        match source {
            GridSource::Table => self.engine.set_table_cell(group, name, row, col, val),
            GridSource::Constant => self.engine.set_constant_cell(group, name, row, col, val),
            GridSource::Enum => self.engine.set_enum_cell(group, name, row, col, val),
        }
        if self.realtime_validate { self.engine.revalidate(group, name); }
    }

    pub fn paste_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, text: &str, source: &crate::ui::grid_model::GridSource) {
        use crate::ui::grid_model::GridSource;
        match source {
            GridSource::Table => self.engine.paste_table_data(group, name, start_row, start_col, text),
            GridSource::Constant => self.engine.paste_constant_data(group, name, start_row, start_col, text),
            GridSource::Enum => self.engine.paste_enum_data(group, name, start_row, start_col, text),
        }
        if self.realtime_validate { self.engine.revalidate(group, name); }
    }

    pub fn copy_selection(&self, group: &str, name: &str) -> String {
        let table = match self.find_table(group, name) {
            Some(t) => t,
            None => return String::new(),
        };
        match &self.edit_state.selected {
            Selection::Cell(r, c) => {
                table.records.get(*r).and_then(|row| row.get(*c)).cloned().unwrap_or_default()
            }
            Selection::CellRange { start, end } => {
                let (r0, r1) = if start.0 <= end.0 { (start.0, end.0) } else { (end.0, start.0) };
                let (c0, c1) = if start.1 <= end.1 { (start.1, end.1) } else { (end.1, start.1) };
                let mut lines = Vec::new();
                for r in r0..=r1 {
                    let mut cells = Vec::new();
                    for c in c0..=c1 {
                        cells.push(table.records.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default());
                    }
                    lines.push(cells.join("\t"));
                }
                lines.join("\n")
            }
            Selection::Row(r) => table.records.get(*r).map(|row| row.join("\t")).unwrap_or_default(),
            Selection::Rows(s, e) => {
                let mut lines = Vec::new();
                for r in *s..=*e { if let Some(row) = table.records.get(r) { lines.push(row.join("\t")); } }
                lines.join("\n")
            }
            Selection::Col(c) => {
                table.records.iter().map(|row| row.get(*c).cloned().unwrap_or_default()).collect::<Vec<_>>().join("\n")
            }
            Selection::None => String::new(),
        }
    }

    pub fn delete_rows(&mut self, group: &str, name: &str, start: usize, end: usize) {
        match &self.selected {
            Some(SelectedNode::Enum { .. }) => self.engine.delete_enum_rows(group, name, start, end),
            _ => self.engine.delete_rows(group, name, start, end),
        }
    }

    pub fn grid_commit(&mut self, group: &str, name: &str, editing: &CellPos, source: &crate::ui::grid_model::GridSource) {
        use crate::ui::grid_model::GridSource;
        let val = self.edit_state.edit_buffer.clone();
        match source {
            GridSource::Table => {
                if let Some(hrow) = editing.header_row {
                    self.commit_header_edit(group, name, hrow, editing.col);
                } else {
                    self.commit_edit(group, name, editing.row, editing.col);
                }
            }
            GridSource::Constant => {
                if editing.header_row.is_some() {
                    self.edit_state.editing = None;
                    return;
                }
                self.engine.commit_constant_cell(group, name, editing.row, editing.col, val);
                self.edit_state.editing = None;
            }
            GridSource::Enum => {
                if editing.header_row.is_some() {
                    self.edit_state.editing = None;
                    return;
                }
                self.engine.commit_enum_cell(group, name, editing.row, editing.col, val);
                self.edit_state.editing = None;
            }
        }
    }

    pub fn delete_selected(&mut self, group: &str, name: &str, grid: &crate::ui::grid_model::GridData) {
        use crate::ui::grid_model::GridSource;
        let cells = self.selection_to_cells(grid);
        match &grid.source {
            GridSource::Table => self.engine.clear_table_cells(group, name, &cells),
            GridSource::Constant => self.engine.clear_constant_cells(group, name, &cells),
            GridSource::Enum => self.engine.clear_enum_cells(group, name, &cells),
        }
        self.engine.log("已删除选中内容".to_string());
    }

    fn selection_to_cells(&self, grid: &crate::ui::grid_model::GridData) -> Vec<(usize, usize)> {
        let mut cells = Vec::new();
        let num_rows = grid.data.len();
        let num_cols = grid.col_defs.len();
        match &self.edit_state.selected {
            Selection::Cell(r, c) => cells.push((*r, *c)),
            Selection::CellRange { start, end } => {
                let (r0, r1) = (start.0.min(end.0), start.0.max(end.0));
                let (c0, c1) = (start.1.min(end.1), start.1.max(end.1));
                for r in r0..=r1 { for c in c0..=c1 { cells.push((r, c)); } }
            }
            Selection::Row(r) => { for c in 0..num_cols { cells.push((*r, c)); } }
            Selection::Rows(s, e) => { for r in *s..=*e { for c in 0..num_cols { cells.push((r, c)); } } }
            Selection::Col(c) => { for r in 0..num_rows { cells.push((r, *c)); } }
            Selection::None => {}
        }
        cells
    }

    pub fn generate_test_config(&mut self) {
        self.engine.generate_test_config();
    }

    pub fn clear_all_config(&mut self) {
        self.engine.clear_all_config();
    }

    pub fn save_all(&mut self) {
        self.engine.save_all();
    }

    pub fn reload(&mut self) {
        self.engine.reload();
        self.selected = None;
        self.edit_state = EditState::default();
    }
}

// --- PLACEHOLDER_EFRAME ---

impl eframe::App for TblApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            self.apply_theme(ctx);
            self.theme_applied = true;
        }
        egui::SidePanel::left("tree_panel")
            .default_width(200.0)
            .min_width(80.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui::tree::render(ui, self);
            });

        egui::TopBottomPanel::top("toolbar_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("生成测试数据").clicked() {
                    self.generate_test_config();
                }
                if ui.button("清空配置").clicked() {
                    self.clear_all_config();
                }
                ui.separator();
                if ui.button("用Excel打开").clicked() {
                    self.log("Excel 编辑功能待实现".to_string());
                }
                if ui.button("保存").clicked() {
                    self.save_all();
                }
                if ui.button("重新加载").clicked() {
                    self.reload();
                }
                ui.separator();
                if ui.button("导出").clicked() {
                    self.data_export.open = true;
                }
                if ui.button("导出Schema").clicked() {
                    self.schema_export.open = true;
                    self.schema_export.checked.clear();
                }
                if ui.button("导入Schema").clicked() {
                    self.schema_import.open = true;
                    self.schema_import.file_path.clear();
                    self.schema_import.schema = None;
                    self.schema_import.checked.clear();
                    self.schema_import.conflicts.clear();
                }
            });
        });

        egui::TopBottomPanel::top("view_bar_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.view_show_enum_name, "枚举显示名字")
                    .on_hover_text("引用枚举（@EnumName）的单元格只读渲染时显示 name 而非 id\n（编辑时仍是 id，表引用不受此开关影响）");
            });
        });

        egui::TopBottomPanel::bottom("log_panel")
            .default_height(120.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("日志");
                ui.separator();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in &self.engine.logs {
                            ui.label(line);
                        }
                    });
            });

        egui::TopBottomPanel::bottom("status_panel")
            .exact_height(20.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let text = ui::grid::format_selection(&self.edit_state.selected, self.edit_state.hover_cell);
                    ui.label(egui::RichText::new(text).size(11.0));
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui::detail::render(ui, self);
        });

        self.show_type_selector(ctx);
        self.show_ref_picker(ctx);
        self.show_input_dialog(ctx);
        ui::schema_dialog::render_export_dialog(ctx, self);
        ui::schema_dialog::render_import_dialog(ctx, self);
        ui::schema_dialog::render_data_export_dialog(ctx, self);
    }
}

impl TblApp {
    fn show_type_selector(&mut self, ctx: &egui::Context) {
        let sep = self.engine.project.config.separators.clone();
        let groups = self.engine.project.groups.clone();
        if let Some(type_str) = ui::type_selector::render_type_selector(ctx, &mut self.type_selector, &sep, &groups) {
            if let Some(cell) = self.type_selector.editing_cell.take() {
                let group = self.type_selector.editing_group.clone();
                let name = self.type_selector.editing_name.clone();
                let source = self.type_selector.editing_source.clone();
                self.edit_state.edit_buffer = type_str;
                self.grid_commit(&group, &name, &cell, &source);
            }
        }
    }

    fn show_ref_picker(&mut self, ctx: &egui::Context) {
        let groups = self.engine.project.groups.clone();
        if let Some(value) = ui::ref_picker::render_ref_picker(ctx, &mut self.ref_picker, &groups) {
            if let Some(cell) = self.ref_picker.editing_cell.take() {
                let group = self.ref_picker.editing_group.clone();
                let name = self.ref_picker.editing_name.clone();
                let source = self.ref_picker.editing_source.clone();
                self.edit_state.edit_buffer = value;
                self.grid_commit(&group, &name, &cell, &source);
            }
        }
    }

    fn show_input_dialog(&mut self, ctx: &egui::Context) {
        let action = match &self.pending_action {
            Some(a) => a.clone(),
            None => return,
        };

        match &action {
            PendingAction::DeleteGroup { group } => {
                let group = group.clone();
                let mut open = true;
                egui::Window::new("确认删除")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("确定删除 Group \"{}\" 及其所有内容？", group));
                        ui.horizontal(|ui| {
                            if ui.button("确定").clicked() {
                                self.engine.delete_group(&group);
                                self.selected = None;
                                self.pending_action = None;
                            }
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::DeleteNode { group, name } => {
                let (group, name) = (group.clone(), name.clone());
                let mut open = true;
                egui::Window::new("确认删除")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("确定删除 \"{}/{}\"？", group, name));
                        ui.horizontal(|ui| {
                            if ui.button("确定").clicked() {
                                self.engine.delete_node(&group, &name);
                                self.selected = None;
                                self.pending_action = None;
                            }
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::CopyNode { group, name, kind } => {
                let (group, name, kind) = (group.clone(), name.clone(), kind.clone());
                self.engine.copy_node(&group, &name, kind);
                self.pending_action = None;
                return;
            }
            _ => {}
        }

        let title = match &action {
            PendingAction::NewGroup => "新建 Group",
            PendingAction::NewTable { .. } => "新建 Table",
            PendingAction::NewConstant { .. } => "新建 Constant",
            PendingAction::NewEnum { .. } => "新建 Enum",
            PendingAction::RenameGroup { .. } => "重命名 Group",
            PendingAction::RenameNode { .. } => "重命名",
            _ => return,
        };

        let mut open = true;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("名称:");
                    ui.text_edit_singleline(&mut self.input_name);
                });
                let err = match &action {
                    PendingAction::NewGroup => self.engine.validate_group_name(&self.input_name),
                    PendingAction::RenameGroup { old_name } => self.engine.validate_group_name_rename(&self.input_name, old_name),
                    PendingAction::RenameNode { old_name, .. } => self.engine.validate_node_name_rename(&self.input_name, old_name),
                    _ => self.engine.validate_node_name(&self.input_name),
                };
                if let Some(ref msg) = err {
                    ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(220, 50, 50)).size(11.0));
                }
                ui.horizontal(|ui| {
                    let can_confirm = err.is_none() && !self.input_name.is_empty();
                    if ui.add_enabled(can_confirm, egui::Button::new("确定")).clicked() {
                        self.execute_action(&action);
                        self.pending_action = None;
                        self.input_name.clear();
                    }
                    if ui.button("取消").clicked() {
                        self.pending_action = None;
                        self.input_name.clear();
                    }
                });
            });

        if !open {
            self.pending_action = None;
            self.input_name.clear();
        }
    }

    fn execute_action(&mut self, action: &PendingAction) {
        let core_action = match action {
            PendingAction::NewGroup => ProjectAction::NewGroup { name: self.input_name.clone() },
            PendingAction::NewTable { group } => ProjectAction::NewTable { group: group.clone(), name: self.input_name.clone() },
            PendingAction::NewConstant { group } => ProjectAction::NewConstant { group: group.clone(), name: self.input_name.clone() },
            PendingAction::NewEnum { group } => ProjectAction::NewEnum { group: group.clone(), name: self.input_name.clone() },
            PendingAction::RenameGroup { old_name } => ProjectAction::RenameGroup { old_name: old_name.clone(), new_name: self.input_name.clone() },
            PendingAction::RenameNode { group, old_name } => ProjectAction::RenameNode { group: group.clone(), old_name: old_name.clone(), new_name: self.input_name.clone() },
            _ => return,
        };
        if matches!(action, PendingAction::NewGroup) {
            self.tree_expanded.insert(self.input_name.clone());
        }
        self.engine.execute_action(&core_action);
    }
}
