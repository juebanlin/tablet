// AppState：Rust 侧的真值，所有 UI 数据从这里派生。
// slint 是 retained UI，需要 Rust 显式 push 数据；AppState 持有 engine + UI 临时态。

use std::collections::HashSet;
use tbl_core::ops::{NodeKind, ProjectEngine};
use tbl_core::project::load_project;
use tbl_core::types::{BaseType, Paradigm, TblType};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum TreeFilter {
    All,
    Changed,
    New,
    Modified,
    Deleted,
}

impl TreeFilter {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Changed,
            2 => Self::New,
            3 => Self::Modified,
            4 => Self::Deleted,
            _ => Self::All,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SelectedNode {
    Table { group: String, name: String },
    Constant { group: String, name: String },
    Enum { group: String, name: String },
}

/// slint TreeNode 行 ↔ 真实节点的映射目标。
#[derive(Clone, Debug)]
pub enum TreeTarget {
    Group(String),
    Table { group: String, name: String },
    Constant { group: String, name: String },
    Enum { group: String, name: String },
}

/// 列的可编辑性维度。决定单/双击进入哪种编辑器、公式栏是否可写。
/// 与 egui 端 `CellKind` 对应；颜色（ReadOnly 灰色等）是另一维度，不依赖此 enum。
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnKind {
    /// 不可编辑（id 列等）。颜色是否灰色由展示层独立决定。
    ReadOnly,
    /// 普通字符串列，公式栏可写、双击进入 LineEdit 编辑
    Text,
    /// @TableName / @EnumName 引用列，单击弹 RefPicker
    Ref { target: String },
    /// type 列（Constant 整列 / Table 表头 type 行单元格），单击弹 TypeSelector
    TypeEnumCol,
    /// export 列（Constant 整列 / Table 表头 export 行单元格），单击弹 Popup
    ExportEnumCol,
}

impl ColumnKind {
    /// 是否双击进 LineEdit 编辑（仅 Text）
    pub fn double_click_to_edit(&self) -> bool {
        matches!(self, Self::Text)
    }
    /// 是否单击进入弹窗/下拉
    #[allow(dead_code)]
    pub fn single_click_to_pick(&self) -> bool {
        matches!(self, Self::Ref { .. } | Self::TypeEnumCol | Self::ExportEnumCol)
    }
}

/// grid 当前选区。step 4 仅支持 Cell；CellRange / Row / Col 留给后续 step。
#[derive(Clone, Debug, Default, PartialEq)]
pub enum GridSelection {
    #[default]
    None,
    Cell(usize, usize),
    Row(usize),
    Col(usize),
}

/// 类型选择器当前编辑的 cell 位置。
/// HeaderType: Table 表头 type 行（hi=2）某列；col 即列号。
/// CellType:   Constant 数据格 type 列（c=1）；row 是数据行。
#[derive(Clone, Debug)]
pub enum TypeEditTarget {
    HeaderType { col: usize },
    CellType { row: usize, col: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum TsTab { Data, Reference }

#[derive(Clone, Debug, PartialEq)]
pub enum TsRefFilter { All, Table, Enum }

/// 类型选择器状态。Tab=Data 时由 paradigm + params 决定结果；
/// Tab=Reference 时由 ref_name 决定。constant 表禁用 Reference tab。
pub struct TypeSelectorState {
    pub open: bool,
    pub tab: TsTab,
    pub ref_disabled: bool,
    /// Data tab：当前范式
    pub paradigm: Paradigm,
    /// Data tab：每个槽的当前 BaseType
    pub params: Vec<BaseType>,
    /// Reference tab：选中的 @Name（空字符串 = 未选）
    pub ref_name: String,
    pub ref_filter: TsRefFilter,
    pub ref_search: String,
    /// 编辑上下文：保存确认时写回哪个位置
    pub target: Option<TypeEditTarget>,
    /// 编辑上下文：选中节点（确认写回时用）
    pub editing_group: String,
    pub editing_name: String,
    pub editing_source_table: bool,  // true=Table（写表头）/ false=Constant（写数据格）
}

impl TypeSelectorState {
    pub fn new() -> Self {
        Self {
            open: false,
            tab: TsTab::Data,
            ref_disabled: false,
            paradigm: Paradigm::Base,
            params: vec![BaseType::Int],
            ref_name: String::new(),
            ref_filter: TsRefFilter::All,
            ref_search: String::new(),
            target: None,
            editing_group: String::new(),
            editing_name: String::new(),
            editing_source_table: false,
        }
    }

    /// 用当前单元格的存储值预填弹窗：先 parse；解析失败按 Base/int 兜底。
    pub fn open_with(
        &mut self,
        current_type: &str,
        target: TypeEditTarget,
        group: &str,
        name: &str,
        is_table: bool,
    ) {
        self.open = true;
        self.target = Some(target);
        self.editing_group = group.to_string();
        self.editing_name = name.to_string();
        self.editing_source_table = is_table;
        // Constant 不允许引用类型
        self.ref_disabled = !is_table;
        self.ref_search.clear();
        self.ref_filter = TsRefFilter::All;
        if let Some(t) = TblType::parse(current_type) {
            if t.paradigm == Paradigm::Ref {
                if self.ref_disabled {
                    // Constant 误吃了 ref 字符串，强制回 Data tab
                    self.tab = TsTab::Data;
                    self.paradigm = Paradigm::Base;
                    self.sync_params();
                    self.ref_name.clear();
                } else {
                    self.tab = TsTab::Reference;
                    self.ref_name = t.ref_name.unwrap_or_default();
                    self.paradigm = Paradigm::Base;
                    self.sync_params();
                }
            } else {
                self.tab = TsTab::Data;
                self.paradigm = t.paradigm;
                self.params = t.params;
                self.sync_params();
                self.ref_name.clear();
            }
        } else {
            self.tab = TsTab::Data;
            self.paradigm = Paradigm::Base;
            self.params = vec![BaseType::Int];
            self.ref_name.clear();
        }
    }

    /// 把 params 长度对齐到当前 paradigm 的 slot 数。
    pub fn sync_params(&mut self) {
        let count = self.paradigm.param_slots().len();
        self.params.resize(count, BaseType::Int);
    }

    /// 当前 Data tab 选择产生的 TblType（用于 to_type_string / *_decl / example）
    pub fn data_type(&self) -> TblType {
        TblType {
            paradigm: self.paradigm.clone(),
            params: self.params.clone(),
            ref_name: None,
        }
    }

    /// 当前 Reference tab 选择产生的 TblType；ref_name 为空时返回 None
    pub fn ref_type(&self) -> Option<TblType> {
        if self.ref_name.is_empty() { None }
        else { Some(TblType::new_ref(self.ref_name.clone())) }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.target = None;
        self.editing_group.clear();
        self.editing_name.clear();
        self.ref_search.clear();
        self.ref_name.clear();
    }
}

/// 引用选择器状态。Ref 列单击呼出，列出被引用 table/enum 的所有可选条目。
/// 选中后写入 id（数据文件层永远存 id；展示按 ref_show_enum_name 决定）。
pub struct RefPickerState {
    pub open: bool,
    /// 被引用项名称（@HeroBase / @HeroType 中的 HeroBase / HeroType）
    pub ref_name: String,
    pub search: String,
    pub selected_id: String,
    /// 编辑上下文：cell 位置（仅数据格使用 Ref；表头不会是 Ref）
    pub editing_row: Option<usize>,
    pub editing_col: Option<usize>,
    pub editing_group: String,
    pub editing_name: String,
    /// true=Table 数据格 / false=Constant 数据格
    pub editing_source_table: bool,
}

impl RefPickerState {
    pub fn new() -> Self {
        Self {
            open: false,
            ref_name: String::new(),
            search: String::new(),
            selected_id: String::new(),
            editing_row: None,
            editing_col: None,
            editing_group: String::new(),
            editing_name: String::new(),
            editing_source_table: false,
        }
    }

    pub fn open_with(
        &mut self,
        ref_name: &str,
        current_value: &str,
        row: usize,
        col: usize,
        group: &str,
        name: &str,
        is_table: bool,
    ) {
        self.open = true;
        self.ref_name = ref_name.to_string();
        self.selected_id = current_value.to_string();
        self.search.clear();
        self.editing_row = Some(row);
        self.editing_col = Some(col);
        self.editing_group = group.to_string();
        self.editing_name = name.to_string();
        self.editing_source_table = is_table;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.editing_row = None;
        self.editing_col = None;
        self.editing_group.clear();
        self.editing_name.clear();
        self.search.clear();
        self.ref_name.clear();
        self.selected_id.clear();
    }
}

// ──────── 右键菜单 + 弹窗 action 系统 ────────

/// 右键菜单弹起源。决定菜单显示哪些项 + 点击 action 时怎么分发。
#[derive(Clone, Debug)]
pub enum CtxMenuKind {
    TreeBlank,
    TreeGroup { name: String },
    TreeNode { group: String, name: String, kind: NodeKind },
    GridCol { col: usize },
    GridRow { row: usize },
    GridCell { row: usize, col: usize },
}

#[derive(Default)]
pub struct ContextMenuState {
    pub open: bool,
    pub kind: Option<CtxMenuKind>,
    pub x: f32,
    pub y: f32,
}

impl ContextMenuState {
    pub fn open_at(&mut self, kind: CtxMenuKind, x: f32, y: f32) {
        self.open = true;
        self.kind = Some(kind);
        self.x = x;
        self.y = y;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.kind = None;
    }
}

/// 待执行的命名/确认型操作。需要走 InputDialog 或 ConfirmDialog 收集用户输入。
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
}

impl PendingAction {
    pub fn needs_input(&self) -> bool {
        matches!(self, PendingAction::NewGroup
            | PendingAction::NewTable { .. }
            | PendingAction::NewConstant { .. }
            | PendingAction::NewEnum { .. }
            | PendingAction::RenameGroup { .. }
            | PendingAction::RenameNode { .. })
    }
    pub fn needs_confirm(&self) -> bool {
        matches!(self, PendingAction::DeleteGroup { .. } | PendingAction::DeleteNode { .. })
    }
    pub fn input_title(&self) -> &'static str {
        match self {
            PendingAction::NewGroup => "新建 Group",
            PendingAction::NewTable { .. } => "新建 Table",
            PendingAction::NewConstant { .. } => "新建 Constant",
            PendingAction::NewEnum { .. } => "新建 Enum",
            PendingAction::RenameGroup { .. } => "重命名 Group",
            PendingAction::RenameNode { .. } => "重命名",
            _ => "",
        }
    }
    pub fn confirm_title(&self) -> &'static str {
        match self {
            PendingAction::DeleteGroup { .. } => "确认删除",
            PendingAction::DeleteNode { .. } => "确认删除",
            _ => "",
        }
    }
    pub fn confirm_message(&self) -> String {
        match self {
            PendingAction::DeleteGroup { group } =>
                format!("确定删除 Group \"{}\" 及其所有内容？", group),
            PendingAction::DeleteNode { group, name } =>
                format!("确定删除 \"{}/{}\"？", group, name),
            _ => String::new(),
        }
    }
}

#[derive(Default)]
pub struct PendingActionState {
    pub action: Option<PendingAction>,
    pub input_buffer: String,
    pub error: Option<String>,
}

impl PendingActionState {
    pub fn open(&mut self, action: PendingAction) {
        self.action = Some(action);
        self.input_buffer.clear();
        self.error = None;
    }
    pub fn close(&mut self) {
        self.action = None;
        self.input_buffer.clear();
        self.error = None;
    }
}

pub struct AppState {
    pub engine: ProjectEngine,
    pub selected: Option<SelectedNode>,
    pub tree_filter: TreeFilter,
    pub tree_full_group: bool,
    pub tree_search: String,
    pub tree_expanded: HashSet<String>,
    /// slint TreeNode.id → TreeTarget；每次 rebuild 时同步刷新。
    pub tree_targets: Vec<TreeTarget>,
    /// 工具栏「枚举名」开关：开启时 @EnumName 列展示 entry.name，关闭时展示 id。
    pub view_show_enum_name: bool,
    /// 当前 grid 选区（用于公式栏 / 状态栏）。切换节点时清空。
    pub grid_selection: GridSelection,
    /// 当前节点每列的可编辑性，由 build_grid 同步刷新。供 callback 判断点击行为。
    pub grid_column_kinds: Vec<ColumnKind>,
    /// 当前节点表头每个 cell 的可编辑性（按 [hrow][col] 索引）。callback 收到 (hi,ci)
    /// 后查这里决定双击 LineEdit / 单击下拉 / 弹窗。
    pub grid_header_kinds: Vec<Vec<ColumnKind>>,
    /// 当前节点的 data_count（valid 行数），决定哪些行是"占位行"。
    pub grid_data_count: usize,
    /// 写回单元格后是否立即 revalidate。来自 [ui] realtime_validate。
    pub realtime_validate: bool,
    /// 当前正在 inline 编辑的单元格位置。仅 Text 列允许进入。
    pub editing: Option<(usize, usize)>,
    /// 编辑 buffer。inline LineEdit / 公式栏 LineEdit 共享同一份（slint 端 editing-buffer property）。
    pub editing_buffer: String,
    /// 当前编辑器在公式栏（true）还是单元格内联（false）。
    /// 用来在 slint 端控制两处 LineEdit 互斥渲染，避免同时存在争 focus。
    pub editing_in_formula: bool,
    /// 表头正在编辑的行/列（hi=0 desc / hi=3 field 才允许）；-1 = 未在编辑表头。
    /// 表头不属于 data_rows，复用 editing 不便，单独两字段。
    pub editing_header_row: i32,
    pub editing_header_col: i32,
    /// 类型选择器当前状态
    pub type_selector: TypeSelectorState,
    /// 引用选择器当前状态
    pub ref_picker: RefPickerState,
    /// 右键菜单
    pub ctx_menu: ContextMenuState,
    /// 待执行的 New/Rename/Delete 操作（Input/Confirm 对话框收集输入）
    pub pending: PendingActionState,
}

impl AppState {
    pub fn load(workdir: &Path) -> anyhow::Result<Self> {
        let project = load_project(workdir)?;
        let group_count = project.groups.len();
        let rt_validate = project.config.ui.as_ref().map_or(false, |u| u.realtime_validate);
        let expanded: HashSet<String> = project.groups.iter().map(|g| g.name.clone()).collect();
        let mut engine = ProjectEngine::new(project);
        engine.log(format!("已加载 {} 个 Group", group_count));
        Ok(Self {
            engine,
            selected: None,
            tree_filter: TreeFilter::All,
            tree_full_group: false,
            tree_search: String::new(),
            tree_expanded: expanded,
            tree_targets: Vec::new(),
            view_show_enum_name: false,
            grid_selection: GridSelection::None,
            grid_column_kinds: Vec::new(),
            grid_header_kinds: Vec::new(),
            grid_data_count: 0,
            realtime_validate: rt_validate,
            editing: None,
            editing_buffer: String::new(),
            editing_in_formula: false,
            editing_header_row: -1,
            editing_header_col: -1,
            type_selector: TypeSelectorState::new(),
            ref_picker: RefPickerState::new(),
            ctx_menu: ContextMenuState::default(),
            pending: PendingActionState::default(),
        })
    }

    /// 写一个单元格的真实存储值；按当前 selected 节点类型分发到 engine。
    /// realtime_validate 开启时立即重算该节点的 validation_errors。
    pub fn set_cell(&mut self, r: usize, c: usize, val: &str) {
        let (group, name, is_table, is_constant) = match &self.selected {
            Some(SelectedNode::Table { group, name }) => (group.clone(), name.clone(), true, false),
            Some(SelectedNode::Constant { group, name }) => (group.clone(), name.clone(), false, true),
            Some(SelectedNode::Enum { group, name }) => (group.clone(), name.clone(), false, false),
            None => return,
        };
        if is_table {
            self.engine.set_table_cell(&group, &name, r, c, val);
        } else if is_constant {
            self.engine.set_constant_cell(&group, &name, r, c, val);
        } else {
            self.engine.set_enum_cell(&group, &name, r, c, val);
        }
        if self.realtime_validate {
            self.engine.revalidate(&group, &name);
        }
    }

    /// 写表头单元格（仅 Table）；hi: 0=desc / 1=export / 2=type / 3=field。
    pub fn set_header_cell(&mut self, hi: usize, ci: usize, val: String) {
        let (group, name) = match &self.selected {
            Some(SelectedNode::Table { group, name }) => (group.clone(), name.clone()),
            _ => return,
        };
        self.engine.commit_header_edit(&group, &name, hi, ci, val);
        if self.realtime_validate {
            self.engine.revalidate(&group, &name);
        }
    }
}
