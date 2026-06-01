use std::collections::HashSet;
use eframe::egui;
use tbl_core::model::*;
use tbl_core::ops::{ProjectEngine, ProjectAction};
use crate::ui;
use crate::ui::type_selector::TypeSelectorState;
use crate::ui::ref_picker::RefPickerState;
use crate::ui::schema_dialog::{SchemaExportState, SchemaImportState, DataExportState};
use crate::ui::template_dialog::{TemplateLibraryState, NewProjectState};

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
    /// 表头 picker 单元格呼出方式：true = 单击呼出，false = 双击呼出（默认 true）
    pub picker_trigger_header_single: bool,
    /// 数据区 picker 单元格呼出方式：true = 单击呼出，false = 双击呼出（默认 false）
    pub picker_trigger_data_single: bool,
    pub tree_filter: TreeFilter,
    pub tree_filter_show_full_group: bool,
    pub tree_search: String,
    /// (project_id, group_name) → 已展开的 group。多 project 同名 group 不冲突。
    pub tree_expanded: HashSet<(String, String)>,
    /// project_id 已展开的 project 根。
    pub project_expanded: HashSet<String>,
    pub tree_context: Option<TreeContext>,
    pub type_selector: TypeSelectorState,
    pub ref_picker: RefPickerState,
    pub view_show_enum_name: bool,
    pub schema_export: SchemaExportState,
    pub schema_import: SchemaImportState,
    pub data_export: DataExportState,
    pub template_lib: TemplateLibraryState,
    pub new_project: NewProjectState,
    /// "id" / "name" / "open" / "created" / "manual"。从 [project] project_sort 读初值，UI 写时持久化。
    pub project_sort: String,
    /// sort=manual 时使用；UI 拖拽顺序持久化到 [project] project_order。
    pub project_order: Vec<String>,
    theme_applied: bool,
}

#[derive(Clone, Debug)]
pub enum TreeContext {
    Project(String),
    Group { project_id: String, name: String },
    Node { project_id: String, group: String, name: String, kind: tbl_core::ops::NodeKind },
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
    Project { project_id: String },
    Group { project_id: String, group: String },
    Table { project_id: String, group: String, name: String },
    Constant { project_id: String, group: String, name: String },
    Enum { project_id: String, group: String, name: String },
}

impl SelectedNode {
    pub fn project_id(&self) -> &str {
        match self {
            SelectedNode::Project { project_id }
            | SelectedNode::Group { project_id, .. }
            | SelectedNode::Table { project_id, .. }
            | SelectedNode::Constant { project_id, .. }
            | SelectedNode::Enum { project_id, .. } => project_id,
        }
    }
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    NewGroup { project_id: String },
    NewTable { project_id: String, group: String },
    NewConstant { project_id: String, group: String },
    NewEnum { project_id: String, group: String },
    DeleteGroup { project_id: String, group: String },
    DeleteNode { project_id: String, group: String, name: String },
    RenameGroup { project_id: String, old_name: String },
    RenameNode { project_id: String, group: String, old_name: String },
    CopyNode { project_id: String, group: String, name: String, kind: tbl_core::ops::NodeKind },
    /// 重命名 project：弹两次输入框（id / name）；这里仅记录第一阶段（id），第二阶段由 input_name 收集
    RenameProject { old_id: String, stage: RenameProjectStage },
    /// 删除 project：单步 ConfirmDialog
    DeleteProject { project_id: String },
    /// 关闭 dirty Project：单步 ConfirmDialog（确认放弃未保存改动）
    CloseDirtyProject { project_id: String },
}

#[derive(Clone, Debug)]
pub enum RenameProjectStage {
    /// 收集新 id
    EnterId,
    /// id 已确定，收集新 name
    EnterName { new_id: String },
}

impl TblApp {
    /// 从已构造好的 ProjectEngine（通常来自 `tbl_core::project::load_workspace`）创建 TblApp。
    /// engine.projects 可能为 0..N（DBeaver-style 工作空间），all-closed 时 active=None。
    pub fn from_engine(mut engine: ProjectEngine) -> Self {
        // ui config 取自 active；全关时回落 projects[0]，再回落硬编码默认
        let ui_cfg = engine
            .active()
            .or_else(|| engine.projects.first())
            .and_then(|p| p.config.ui.clone());
        let project_cfg_for_sort = engine
            .active()
            .or_else(|| engine.projects.first())
            .map(|p| p.config.project.clone());

        let auto_commit = ui_cfg.as_ref().map_or(true, |u| u.auto_commit_on_blur);
        let rt_validate = ui_cfg.as_ref().map_or(false, |u| u.realtime_validate);
        let header_single = ui_cfg.as_ref().map_or(true, |u| u.picker_trigger_header == "single");
        let data_single = ui_cfg.as_ref().map_or(false, |u| u.picker_trigger_data == "single");

        let project_sort = project_cfg_for_sort.as_ref()
            .map(|p| p.project_sort.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "id".to_string());
        let project_order = project_cfg_for_sort.as_ref()
            .map(|p| p.project_order.clone())
            .unwrap_or_default();

        // 默认展开 active project + 它的所有 group；其它 project（含 closed）折叠
        let (expanded, project_expanded): (HashSet<(String, String)>, HashSet<String>) =
            if let Some(active) = engine.active() {
                let active_id = active.instance_meta.id.clone();
                let exp = active.groups.iter()
                    .map(|g| (active_id.clone(), g.name.clone()))
                    .collect();
                let pexp = std::iter::once(active_id).collect();
                (exp, pexp)
            } else {
                (HashSet::new(), HashSet::new())
            };

        engine.revalidate_all_projects();
        let opened = engine.projects.len();
        let avail = engine.available().len();
        engine.log(format!("已加载 {} / {} 个 Project", opened, avail));

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
            picker_trigger_header_single: header_single,
            picker_trigger_data_single: data_single,
            tree_filter: TreeFilter::All,
            tree_filter_show_full_group: false,
            tree_search: String::new(),
            tree_expanded: expanded,
            project_expanded,
            tree_context: None,
            type_selector: TypeSelectorState::default(),
            ref_picker: RefPickerState::default(),
            view_show_enum_name: false,
            schema_export: SchemaExportState::default(),
            schema_import: SchemaImportState::default(),
            data_export: DataExportState::default(),
            template_lib: TemplateLibraryState::default(),
            new_project: NewProjectState::default(),
            project_sort,
            project_order,
            theme_applied: false,
        }
    }

    /// 兼容老调用（projects 全部 opened，单 active = last_id）。新代码请用 from_engine。
    pub fn new(projects: Vec<Project>, last_id: String) -> Self {
        let engine = ProjectEngine::new_multi(projects, Some(last_id.as_str()));
        Self::from_engine(engine)
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        // 切换主题：注释/取消注释下面任一行。
        // Self::apply_default_theme(ctx);
        Self::apply_flat_theme(ctx);
        // Self::apply_material_theme(ctx);
    }

    /// 当前线上主题：light + 全直角 + 无阴影 + 紧凑间距。
    /// 按钮带浅灰底（egui Visuals::light 默认行为），与右键菜单 item 高亮一致。
    fn apply_default_theme(ctx: &egui::Context) {
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

    /// 测试主题：模仿 slint Fluent 风格——按钮 4px 圆角、inactive 无底色但带 1px 浅灰边框，
    /// 仅在 hover / pressed 时才有浅灰底；边框颜色随交互态略微加深。
    /// 窗口/弹窗仍保留直角避免视觉碎片化。
    #[allow(dead_code)]
    fn apply_flat_theme(ctx: &egui::Context) {
        use egui::{Color32, Rounding, Stroke};
        let mut v = egui::Visuals::light();

        // 窗口 / 弹窗：直角 + 无阴影（与默认主题保持一致）
        v.window_rounding = Rounding::ZERO;
        v.menu_rounding = Rounding::ZERO;
        v.window_shadow = egui::epaint::Shadow::NONE;
        v.popup_shadow = egui::epaint::Shadow::NONE;

        // 按钮 / LineEdit / Checkbox 等所有交互控件
        let btn_round = Rounding::same(4.0);
        let border_idle = Stroke::new(1.0, Color32::from_gray(200));
        let border_hover = Stroke::new(1.0, Color32::from_gray(160));
        let border_active = Stroke::new(1.0, Color32::from_gray(120));

        // inactive：透明底（仅 weak_bg_fill，按钮才走这条；bg_fill 留默认给 ScrollBar thumb）
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = border_idle;
        v.widgets.inactive.rounding = btn_round;
        v.widgets.inactive.expansion = 0.0;

        // hover：浅灰底 + 略深边
        v.widgets.hovered.bg_fill = Color32::from_gray(235);
        v.widgets.hovered.weak_bg_fill = Color32::from_gray(235);
        v.widgets.hovered.bg_stroke = border_hover;
        v.widgets.hovered.rounding = btn_round;
        v.widgets.hovered.expansion = 0.0;

        // pressed / 选中：略深灰底 + 深边
        v.widgets.active.bg_fill = Color32::from_gray(218);
        v.widgets.active.weak_bg_fill = Color32::from_gray(218);
        v.widgets.active.bg_stroke = border_active;
        v.widgets.active.rounding = btn_round;
        v.widgets.active.expansion = 0.0;

        // 下拉打开态
        v.widgets.open.bg_fill = Color32::from_gray(218);
        v.widgets.open.weak_bg_fill = Color32::from_gray(218);
        v.widgets.open.bg_stroke = border_active;
        v.widgets.open.rounding = btn_round;
        v.widgets.open.expansion = 0.0;

        // noninteractive 保留 Visuals::light() 默认：
        //   bg_stroke 是 SidePanel/TopBottomPanel 拉伸分割线 + ui.separator() + 面板边界
        //   一旦覆盖（哪怕设 NONE）这些都会消失。所以这里完全不动。

        // 选择高亮蓝（cell 选中、textfield 选区）
        v.selection.bg_fill = Color32::from_rgb(180, 215, 255);
        v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(50, 120, 200));

        ctx.set_visuals(v);

        let mut s = (*ctx.style()).clone();
        s.spacing.button_padding = egui::vec2(8.0, 3.0); // Windows 按钮内边距更宽松
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        ctx.set_style(s);
    }

    /// 测试主题：Material Design 风格——按钮无底色 + Material Blue 文本，
    /// hover 加 8% 不透明度 overlay，pressed 加 12% overlay；选区/聚焦走 Material Primary 蓝。
    /// 圆角 4px（Material 规范是 4px / 文本按钮，FAB 才用大圆角）。
    #[allow(dead_code)]
    fn apply_material_theme(ctx: &egui::Context) {
        use egui::{Color32, Rounding, Stroke};
        let mut v = egui::Visuals::light();

        // Material 调色板（参考 Material 3 light scheme）
        let primary = Color32::from_rgb(0x19, 0x76, 0xD2); // Blue 700
        let surface = Color32::from_rgb(0xFA, 0xFA, 0xFA); // Grey 50
        let on_surface = Color32::from_rgb(0x21, 0x21, 0x21); // Grey 900
        let outline = Color32::from_rgb(0xE0, 0xE0, 0xE0); // Grey 300
        let hover_overlay = Color32::from_rgba_unmultiplied(0x19, 0x76, 0xD2, 20); // primary @ 8%
        let pressed_overlay = Color32::from_rgba_unmultiplied(0x19, 0x76, 0xD2, 30); // primary @ 12%

        // 全局背景
        v.window_fill = surface;
        v.panel_fill = surface;
        v.faint_bg_color = Color32::from_rgb(0xF5, 0xF5, 0xF5);
        v.extreme_bg_color = Color32::WHITE;
        v.override_text_color = Some(on_surface);

        // 圆角：按钮/控件 4px；窗口/弹窗保持 0（避免 Material 卡片浮于直角面板上的违和）
        let r = Rounding::same(4.0);
        v.window_rounding = Rounding::ZERO;
        v.menu_rounding = Rounding::ZERO;
        v.window_shadow = egui::epaint::Shadow::NONE;
        v.popup_shadow = egui::epaint::Shadow::NONE;

        // inactive：透明底 + primary 文本色（Material text button）
        v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, primary);
        v.widgets.inactive.rounding = r;
        v.widgets.inactive.expansion = 0.0;

        // hover：primary @ 8% overlay
        v.widgets.hovered.bg_fill = hover_overlay;
        v.widgets.hovered.weak_bg_fill = hover_overlay;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, primary);
        v.widgets.hovered.rounding = r;
        v.widgets.hovered.expansion = 0.0;

        // pressed：primary @ 12% overlay
        v.widgets.active.bg_fill = pressed_overlay;
        v.widgets.active.weak_bg_fill = pressed_overlay;
        v.widgets.active.bg_stroke = Stroke::NONE;
        v.widgets.active.fg_stroke = Stroke::new(1.0, primary);
        v.widgets.active.rounding = r;
        v.widgets.active.expansion = 0.0;

        // 下拉打开态
        v.widgets.open.bg_fill = pressed_overlay;
        v.widgets.open.weak_bg_fill = pressed_overlay;
        v.widgets.open.bg_stroke = Stroke::NONE;
        v.widgets.open.fg_stroke = Stroke::new(1.0, primary);
        v.widgets.open.rounding = r;

        // 不可交互（label / 分隔线）
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, outline);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, on_surface);
        v.widgets.noninteractive.rounding = Rounding::ZERO;

        // 选区：primary @ 24%
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x19, 0x76, 0xD2, 60);
        v.selection.stroke = Stroke::new(1.5, primary);

        ctx.set_visuals(v);

        let mut s = (*ctx.style()).clone();
        s.spacing.button_padding = egui::vec2(12.0, 6.0); // Material 36px 触控目标
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        // Material 字号：body-medium 14sp
        for (_, font_id) in s.text_styles.iter_mut() {
            if font_id.size < 14.0 {
                font_id.size = 14.0;
            }
        }
        ctx.set_style(s);
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
        // 工具栏「保存」= 全部脏 project 一键落盘
        self.engine.save_all_projects();
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
        egui::TopBottomPanel::top("toolbar_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // S15-X 待重新评估 —— 暂时注释掉「生成测试数据」/「清空配置」
                // if ui.button("生成测试数据").clicked() {
                //     self.generate_test_config();
                // }
                // if ui.button("清空配置").clicked() {
                //     self.clear_all_config();
                // }
                // ui.separator();
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
                // 模板库按钮已挪到 TreeSection 顶部功能区。
            });
        });

        // 注：「枚举显示名字」开关属于 GridSection 的 GridRibbon 子区域，
        // 由 ui::detail::render 在 CentralPanel 内画，宽度跟随 GridSection（不横跨 TreeSection）。
        // 见 docs/02-UI设计.md §1.2、§1.3。
        //
        // 4 大区域布局（自外而内声明，外层优先占空间）：
        //   1. TopBottomPanel::top    — Toolbar（跨全宽）
        //   2. TopBottomPanel::bottom — LogPanel（跨全宽）
        //   3. SidePanel::left        — TreeSection（仅占中间纵向带）
        //   4. CentralPanel           — GridSection（中间剩余；StatusBar 是 GridSection 子区域，不跨 TreeSection）

        egui::TopBottomPanel::bottom("log_panel")
            .default_height(120.0)
            .min_height(40.0)
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

        egui::SidePanel::left("tree_panel")
            .default_width(220.0)
            .min_width(200.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui::tree::render(ui, self);
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
        ui::template_dialog::render_library_dialog(ctx, self);
        ui::template_dialog::render_new_project_dialog(ctx, self);
    }
}

impl TblApp {
    fn show_type_selector(&mut self, ctx: &egui::Context) {
        let Some(active) = self.engine.active() else { return; };
        let sep = active.config.separators.clone();
        let groups = active.groups.clone();
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
        let Some(active) = self.engine.active() else { return; };
        let groups = active.groups.clone();
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
            PendingAction::DeleteGroup { project_id, group } => {
                let (pid, group) = (project_id.clone(), group.clone());
                let mut open = true;
                egui::Window::new("确认删除")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("确定删除 [{}] Group \"{}\" 及其所有内容？", pid, group));
                        ui::modal::dialog_buttons(ui, |ui| {
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                            if ui.button("确定").clicked() {
                                self.engine.with_active(&pid, |e| e.delete_group(&group));
                                self.selected = None;
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::DeleteNode { project_id, group, name } => {
                let (pid, group, name) = (project_id.clone(), group.clone(), name.clone());
                let mut open = true;
                egui::Window::new("确认删除")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("确定删除 [{}] \"{}/{}\"？", pid, group, name));
                        ui::modal::dialog_buttons(ui, |ui| {
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                            if ui.button("确定").clicked() {
                                self.engine.with_active(&pid, |e| e.delete_node(&group, &name));
                                self.selected = None;
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::DeleteProject { project_id } => {
                let pid = project_id.clone();
                let mut open = true;
                egui::Window::new("确认删除 Project")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("此操作不可逆，将永久删除 projects/{}/ 目录及其全部数据。", pid));
                        ui.label("是否继续？");
                        ui::modal::dialog_buttons(ui, |ui| {
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                            if ui.button("确定删除").clicked() {
                                let core = ProjectAction::DeleteProject { project_id: pid.clone() };
                                self.engine.execute_action(&core);
                                if matches!(&self.selected, Some(s) if s.project_id() == pid) {
                                    self.selected = None;
                                }
                                self.tree_expanded.retain(|(p, _)| p != &pid);
                                self.project_expanded.remove(&pid);
                                self.persist_workspace();
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::CloseDirtyProject { project_id } => {
                let pid = project_id.clone();
                let mut open = true;
                egui::Window::new("未保存的修改")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("Project \"{}\" 有未保存的修改，关闭后将丢失这些改动。", pid));
                        ui.label("继续关闭？");
                        ui::modal::dialog_buttons(ui, |ui| {
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                            if ui.button("放弃修改并关闭").clicked() {
                                if matches!(&self.selected, Some(s) if s.project_id() == pid) {
                                    self.selected = None;
                                }
                                if self.engine.close_project(&pid) {
                                    self.tree_expanded.retain(|(p, _)| p != &pid);
                                    self.project_expanded.remove(&pid);
                                    self.persist_workspace();
                                }
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::CopyNode { project_id, group, name, kind } => {
                let pid = project_id.clone();
                let (group, name, kind) = (group.clone(), name.clone(), kind.clone());
                self.engine.with_active(&pid, |e| e.copy_node(&group, &name, kind));
                self.pending_action = None;
                return;
            }
            _ => {}
        }

        let title = match &action {
            PendingAction::NewGroup { .. } => "新建 Group",
            PendingAction::NewTable { .. } => "新建 Table",
            PendingAction::NewConstant { .. } => "新建 Constant",
            PendingAction::NewEnum { .. } => "新建 Enum",
            PendingAction::RenameGroup { .. } => "重命名 Group",
            PendingAction::RenameNode { .. } => "重命名",
            PendingAction::RenameProject { stage, .. } => match stage {
                RenameProjectStage::EnterId => "重命名 Project（新 id）",
                RenameProjectStage::EnterName { .. } => "重命名 Project（新显示名）",
            },
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
                let input = self.input_name.clone();
                let err = match &action {
                    PendingAction::NewGroup { project_id } => {
                        self.engine.with_active(project_id, |e| e.validate_group_name(&input)).flatten()
                    }
                    PendingAction::RenameGroup { project_id, old_name } => {
                        self.engine.with_active(project_id, |e| e.validate_group_name_rename(&input, old_name)).flatten()
                    }
                    PendingAction::RenameNode { project_id, old_name, .. } => {
                        self.engine.with_active(project_id, |e| e.validate_node_name_rename(&input, old_name)).flatten()
                    }
                    PendingAction::NewTable { project_id, .. }
                    | PendingAction::NewConstant { project_id, .. }
                    | PendingAction::NewEnum { project_id, .. } => {
                        self.engine.with_active(project_id, |e| e.validate_node_name(&input)).flatten()
                    }
                    PendingAction::RenameProject { stage, old_id } => match stage {
                        RenameProjectStage::EnterId => self.engine.validate_project_id_rename(&input, old_id),
                        RenameProjectStage::EnterName { .. } => {
                            if input.trim().is_empty() { Some("名称不能为空".to_string()) } else { None }
                        }
                    },
                    _ => None,
                };
                if let Some(ref msg) = err {
                    ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(220, 50, 50)).size(11.0));
                }
                ui::modal::dialog_buttons(ui, |ui| {
                    let can_confirm = err.is_none() && !self.input_name.is_empty();
                    if ui.button("取消").clicked() {
                        self.pending_action = None;
                        self.input_name.clear();
                    }
                    if ui.add_enabled(can_confirm, egui::Button::new("确定")).clicked() {
                        self.execute_action(&action);
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
            PendingAction::NewGroup { project_id } => ProjectAction::NewGroup {
                project_id: project_id.clone(), name: self.input_name.clone(),
            },
            PendingAction::NewTable { project_id, group } => ProjectAction::NewTable {
                project_id: project_id.clone(), group: group.clone(), name: self.input_name.clone(),
            },
            PendingAction::NewConstant { project_id, group } => ProjectAction::NewConstant {
                project_id: project_id.clone(), group: group.clone(), name: self.input_name.clone(),
            },
            PendingAction::NewEnum { project_id, group } => ProjectAction::NewEnum {
                project_id: project_id.clone(), group: group.clone(), name: self.input_name.clone(),
            },
            PendingAction::RenameGroup { project_id, old_name } => ProjectAction::RenameGroup {
                project_id: project_id.clone(), old_name: old_name.clone(), new_name: self.input_name.clone(),
            },
            PendingAction::RenameNode { project_id, group, old_name } => ProjectAction::RenameNode {
                project_id: project_id.clone(), group: group.clone(), old_name: old_name.clone(), new_name: self.input_name.clone(),
            },
            PendingAction::RenameProject { stage, old_id } => match stage {
                RenameProjectStage::EnterId => {
                    let new_id = self.input_name.clone();
                    let display = self.engine.find_project(old_id)
                        .map(|p| p.instance_meta.name.clone())
                        .unwrap_or_default();
                    self.pending_action = Some(PendingAction::RenameProject {
                        old_id: old_id.clone(),
                        stage: RenameProjectStage::EnterName { new_id },
                    });
                    self.input_name = display;
                    return;
                }
                RenameProjectStage::EnterName { new_id } => ProjectAction::RenameProject {
                    old_id: old_id.clone(),
                    new_id: new_id.clone(),
                    new_name: self.input_name.clone(),
                },
            },
            _ => return,
        };
        if let PendingAction::NewGroup { project_id } = action {
            self.tree_expanded.insert((project_id.clone(), self.input_name.clone()));
            self.project_expanded.insert(project_id.clone());
        }
        // RenameProject 可能改 id；记下 active 跟随
        let old_active = self.engine.active_project_id().unwrap_or("").to_string();
        let track_rename = matches!(action, PendingAction::RenameProject { .. });
        self.engine.execute_action(&core_action);
        if track_rename {
            if let ProjectAction::RenameProject { old_id, new_id, .. } = &core_action {
                if old_id != new_id {
                    // 更新 selected / tree_expanded / project_expanded
                    if matches!(&self.selected, Some(s) if s.project_id() == old_id) {
                        // selected 节点的 project_id 同步迁移
                        let mut new_sel = self.selected.clone().unwrap();
                        match &mut new_sel {
                            SelectedNode::Project { project_id }
                            | SelectedNode::Group { project_id, .. }
                            | SelectedNode::Table { project_id, .. }
                            | SelectedNode::Constant { project_id, .. }
                            | SelectedNode::Enum { project_id, .. } => *project_id = new_id.clone(),
                        }
                        self.selected = Some(new_sel);
                    }
                    let migrated_groups: Vec<_> = self.tree_expanded.iter()
                        .filter(|(p, _)| p == old_id)
                        .map(|(_, g)| g.clone())
                        .collect();
                    self.tree_expanded.retain(|(p, _)| p != old_id);
                    for g in migrated_groups {
                        self.tree_expanded.insert((new_id.clone(), g));
                    }
                    if self.project_expanded.remove(old_id) {
                        self.project_expanded.insert(new_id.clone());
                    }
                    if old_active == *old_id {
                        let _ = self.engine.set_active_by_id(new_id);
                    }
                    // rename 改了 id：opened_projects / last_project 持久化
                    self.persist_workspace();
                }
            }
        }
        self.pending_action = None;
        self.input_name.clear();
    }

    /// 把当前 workspace 状态落盘到 `<workdir>/tbl-tool.toml`；失败仅 log。
    pub fn persist_workspace(&mut self) {
        if let Err(e) = tbl_core::project::persist_workspace_state(
            &self.engine, &self.project_sort, &self.project_order,
        ) {
            self.log(format!("[workspace] 持久化失败: {}", e));
        }
    }
}
