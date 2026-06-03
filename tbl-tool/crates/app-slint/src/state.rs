// AppState：Rust 侧的真值，所有 UI 数据从这里派生。
// slint 是 retained UI，需要 Rust 显式 push 数据；AppState 持有 engine + UI 临时态。

use std::collections::HashSet;
use tbl_core::ops::{NodeKind, ProjectEngine};
use tbl_core::project::load_workspace;
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

/// slint TreeNode 行 ↔ 真实节点的映射目标。
#[derive(Clone, Debug)]
pub enum TreeTarget {
    Project(String),
    Group { project_id: String, group: String },
    Table { project_id: String, group: String, name: String },
    Constant { project_id: String, group: String, name: String },
    Enum { project_id: String, group: String, name: String },
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
    /// 是否双击进 LineEdit 编辑或弹窗/下拉
    pub fn double_click_to_edit(&self) -> bool {
        matches!(self,
            Self::Text
            | Self::Ref { .. }
            | Self::TypeEnumCol
            | Self::ExportEnumCol)
    }
    /// 右键菜单首项的差异化文案。Text / ReadOnly 返回 None。
    pub fn picker_action_label(&self) -> Option<&'static str> {
        match self {
            Self::Ref { .. } => Some("选择引用..."),
            Self::TypeEnumCol => Some("选择类型..."),
            Self::ExportEnumCol => Some("选择导出标记..."),
            _ => None,
        }
    }
}

/// grid 当前选区。支持单格 / 矩形区域 / 整行 / 整列。
#[derive(Clone, Debug, Default, PartialEq)]
pub enum GridSelection {
    #[default]
    None,
    Cell(usize, usize),
    /// 矩形区域：(r1,c1) 是 anchor（首格），(r2,c2) 是当前 shift+click 终点。
    /// 渲染/复制/粘贴时按 min..=max 取范围。
    CellRange { r1: usize, c1: usize, r2: usize, c2: usize },
    Row(usize),
    Col(usize),
}

impl GridSelection {
    /// 返回选区覆盖的 (row_min, row_max, col_min, col_max)；None=未选择。
    /// 整行/整列时 row/col 边界返回 usize::MAX 表示"无界"——调用方按 grid 实际尺寸截断。
    pub fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        match self {
            Self::None => None,
            Self::Cell(r, c) => Some((*r, *r, *c, *c)),
            Self::CellRange { r1, c1, r2, c2 } => {
                Some((*r1.min(r2), *r1.max(r2), *c1.min(c2), *c1.max(c2)))
            }
            Self::Row(r) => Some((*r, *r, 0, usize::MAX)),
            Self::Col(c) => Some((0, usize::MAX, *c, *c)),
        }
    }
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
    /// `allow_constant_ref`：从 [ui] constant_ref_allowed 读取。Constant + 不允许时 ref tab 禁用。
    pub fn open_with(
        &mut self,
        current_type: &str,
        target: TypeEditTarget,
        group: &str,
        name: &str,
        is_table: bool,
        allow_constant_ref: bool,
    ) {
        self.open = true;
        self.target = Some(target);
        self.editing_group = group.to_string();
        self.editing_name = name.to_string();
        self.editing_source_table = is_table;
        // Constant：仅当 [ui] constant_ref_allowed = false 时禁用 ref tab
        self.ref_disabled = !is_table && !allow_constant_ref;
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
    /// 手动输入框的当前值（与 selected_id 双向同步；确认时优先以此为准）
    pub manual_value: String,
    /// 列展示策略：每次打开重置回项目配置默认；本次会话内可临时切换
    pub strategy: RefDisplayStrategy,
    /// 编辑上下文：cell 位置（仅数据格使用 Ref；表头不会是 Ref）
    pub editing_row: Option<usize>,
    pub editing_col: Option<usize>,
    pub editing_group: String,
    pub editing_name: String,
    /// true=Table 数据格 / false=Constant 数据格
    pub editing_source_table: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefDisplayStrategy {
    /// id + 最多 2 个非引用、非复合、export != "-" 的辅助列
    Auto,
    /// schema 全部字段（除 export = "-" 不导出列）
    Full,
}

impl RefDisplayStrategy {
    pub fn from_config(s: &str) -> Self {
        match s {
            "full" => Self::Full,
            _ => Self::Auto,
        }
    }
    pub fn to_index(&self) -> i32 {
        match self {
            Self::Auto => 0,
            Self::Full => 1,
        }
    }
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Full,
            _ => Self::Auto,
        }
    }
}

impl RefPickerState {
    pub fn new() -> Self {
        Self {
            open: false,
            ref_name: String::new(),
            search: String::new(),
            selected_id: String::new(),
            manual_value: String::new(),
            strategy: RefDisplayStrategy::Auto,
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
        default_strategy: RefDisplayStrategy,
    ) {
        self.open = true;
        self.ref_name = ref_name.to_string();
        self.selected_id = current_value.to_string();
        self.manual_value = current_value.to_string();
        self.strategy = default_strategy;
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
        self.manual_value.clear();
    }
}

// ──────── 右键菜单 + 弹窗 action 系统 ────────

/// 右键菜单弹起源。决定菜单显示哪些项 + 点击 action 时怎么分发。
#[derive(Clone, Debug)]
pub enum CtxMenuKind {
    TreeBlank,
    TreeProject { project_id: String },
    TreeGroup { project_id: String, name: String },
    TreeNode { project_id: String, group: String, name: String, kind: NodeKind },
    GridCol { col: usize },
    GridRow { row: usize },
    GridCell {
        #[allow(dead_code)] row: usize,
        #[allow(dead_code)] col: usize,
    },
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
    NewGroup { project_id: String },
    NewTable { project_id: String, group: String },
    NewConstant { project_id: String, group: String },
    NewEnum { project_id: String, group: String },
    DeleteGroup { project_id: String, group: String },
    DeleteNode { project_id: String, group: String, name: String },
    RenameGroup { project_id: String, old_name: String },
    RenameNode { project_id: String, group: String, old_name: String },
    /// 删除 Project：单步 ConfirmDialog
    DeleteProject { project_id: String },
    /// 关闭 dirty Project：单步 ConfirmDialog（确认放弃未保存改动）
    CloseDirtyProject { project_id: String },
}

impl PendingAction {
    pub fn needs_input(&self) -> bool {
        matches!(self, PendingAction::NewGroup { .. }
            | PendingAction::NewTable { .. }
            | PendingAction::NewConstant { .. }
            | PendingAction::NewEnum { .. }
            | PendingAction::RenameGroup { .. }
            | PendingAction::RenameNode { .. })
    }
    pub fn needs_confirm(&self) -> bool {
        matches!(self, PendingAction::DeleteGroup { .. }
            | PendingAction::DeleteNode { .. }
            | PendingAction::DeleteProject { .. }
            | PendingAction::CloseDirtyProject { .. })
    }
    pub fn input_title(&self) -> &'static str {
        match self {
            PendingAction::NewGroup { .. } => "新建 Group",
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
            PendingAction::DeleteProject { .. } => "确认删除 Project",
            PendingAction::CloseDirtyProject { .. } => "未保存的修改",
            _ => "",
        }
    }
    pub fn confirm_message(&self) -> String {
        match self {
            PendingAction::DeleteGroup { project_id, group } =>
                format!("确定删除 [{}] Group \"{}\" 及其所有内容？", project_id, group),
            PendingAction::DeleteNode { project_id, group, name } =>
                format!("确定删除 [{}] \"{}/{}\"？", project_id, group, name),
            PendingAction::DeleteProject { project_id } =>
                format!("此操作不可逆，将永久删除 projects/{}/ 目录及其全部数据。是否继续？", project_id),
            PendingAction::CloseDirtyProject { project_id } =>
                format!("Project \"{}\" 有未保存的修改，关闭后将丢失这些改动。继续关闭？", project_id),
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

// ──────── 数据导出 / Schema 导出 / Schema 导入 ────────

/// 数据导出对话框选项
#[derive(Clone, Debug)]
pub struct DataExportState {
    pub open: bool,
    pub json: bool,
    pub xml: bool,
    pub java: bool,
    pub go: bool,
    pub lua: bool,
}

impl Default for DataExportState {
    fn default() -> Self {
        Self { open: false, json: true, xml: true, java: true, go: false, lua: true }
    }
}

/// Schema 导出对话框：列出当前项目内 Table/Constant，按勾选写 .tblschema。
/// items 与 checked 一一对应；items 是扁平化（组节点 + 子节点）后的视图，
/// 只有 indent=1 的项参与勾选；组节点的勾选靠子节点聚合判断。
#[derive(Default)]
pub struct SchemaExportState {
    pub open: bool,
    /// 扁平 items：每项 (group_name, name_opt, is_table)。组节点 name_opt = None。
    pub items: Vec<SchemaExportItem>,
    /// items 同步长度的 checked。组节点 checked 由子节点聚合，不直接读写。
    pub checked: Vec<bool>,
    /// 是否把当前 records / entries 作为 `# @preset` 块写入导出文件。
    /// 默认 false：导出"结构骨架"用于复用；true 用于"结构 + 数据"打包迁移。
    pub with_preset: bool,
}

#[derive(Clone, Debug)]
pub struct SchemaExportItem {
    pub indent: u8,           // 0=group / 1=node
    pub group: String,
    pub name: String,         // 组名 或 子节点名
    pub is_table: bool,       // indent=1 时有效
}

/// Schema 导入对话框：读 .tblschema 后扁平展示并允许勾选。
/// items 与 checked 长度一致；conflicts 与 items 长度一致（仅 indent=1 有效）。
#[derive(Default)]
pub struct SchemaImportState {
    pub open: bool,
    pub file_path: String,
    pub schema: Option<tbl_core::tblschema::TblSchema>,
    pub items: Vec<SchemaImportItem>,
    pub checked: Vec<bool>,
    pub conflicts: Vec<bool>,
    /// 是否把 schema 里的 `# @preset` 数据一同灌进项目（ND+tbl 语义：apply 后即解耦）。
    /// 默认 true：有预设就直接拿；用户可以在导入对话框里关掉。
    pub with_preset: bool,
}

#[derive(Clone, Debug)]
pub struct SchemaImportItem {
    pub indent: u8,           // 0=group / 1=section
    pub group: String,
    pub name: String,
    pub mode: tbl_core::tblschema::SchemaMode,
}

// ──────── 新建项目 / 克隆项目 ────────

/// 统一「新建项目」对话框：3 tab（空 / 从文件 / 从模板）+ 单页左右分栏。
/// 替代旧 TemplateLibrary + 顶部「导入 Schema」+ NewProject Empty/FromTemplate 三个入口。
pub struct CreateProjectState {
    pub open: bool,
    /// 0 = 空项目 / 1 = 从文件 / 2 = 从模板
    pub tab: i32,

    // —— 共享身份字段 ——
    pub project_id: String,
    pub project_name: String,
    pub project_category: String,
    pub project_version: String,
    /// 「立即打开新项目」勾选项；默认 true
    pub open_after: bool,
    /// 标记是否已经预填过 project id（避免文件/模板改变时覆盖用户手输）
    pub id_prefilled: bool,
    /// 「灌入预设数据」勾选项；仅 FromFile / FromTemplate tab 且来源带 preset 时显示
    pub with_preset: bool,

    // —— FromFile tab 专用 ——
    pub file_path: String,
    pub file_schema: Option<tbl_core::tblschema::TblSchema>,
    /// parse 失败时显示的错误（占位用，目前只记 log）
    pub file_error: String,

    // —— FromTemplate tab 专用 ——
    /// 0 = 内置 / 1 = 本地
    pub tpl_subtab: i32,
    pub tpl_search: String,
    pub tpl_selected_id: String,
    /// FromTemplate 当前选中模板的 schema 缓存（按 tpl_selected_id 加载）
    pub tpl_schema: Option<tbl_core::tblschema::TblSchema>,
    pub tpl_meta_id: String,
    pub tpl_meta_version: String,

    // —— 共享 sections-picker（FromFile + FromTemplate 共用）——
    pub picker_items: Vec<CreatePickerItem>,
    pub picker_checked: Vec<bool>,
}

impl Default for CreateProjectState {
    fn default() -> Self {
        Self {
            open: false,
            tab: 0,
            project_id: String::new(),
            project_name: String::new(),
            project_category: String::new(),
            project_version: "1.0.0".to_string(),
            open_after: true,
            id_prefilled: false,
            with_preset: false,
            file_path: String::new(),
            file_schema: None,
            file_error: String::new(),
            tpl_subtab: 0,
            tpl_search: String::new(),
            tpl_selected_id: String::new(),
            tpl_schema: None,
            tpl_meta_id: String::new(),
            tpl_meta_version: String::new(),
            picker_items: Vec::new(),
            picker_checked: Vec::new(),
        }
    }
}

/// 扁平化的 sections-picker 行：组节点 indent=0，子节点 indent=1。
#[derive(Clone, Debug)]
pub struct CreatePickerItem {
    pub indent: u8,
    pub group: String,
    pub name: String,
    pub mode: tbl_core::tblschema::SchemaMode,
}

impl CreateProjectState {
    pub fn open_empty(&mut self) {
        *self = Self::default();
        self.open = true;
        self.tab = 0;
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    /// 当前 tab 是否需要 sections-picker（仅 FromFile / FromTemplate）
    pub fn needs_picker(&self) -> bool {
        self.tab == 1 || self.tab == 2
    }

    /// 当前 tab 决定的 source schema（FromFile / FromTemplate 共享 picker 与 with_preset）
    pub fn source_schema(&self) -> Option<&tbl_core::tblschema::TblSchema> {
        match self.tab {
            1 => self.file_schema.as_ref(),
            2 => self.tpl_schema.as_ref(),
            _ => None,
        }
    }
}

/// 克隆项目对话框（项目右键「复制(克隆)...」专用）。
/// 与旧 NewProjectState 的 Clone 模式一致：仅做内存深拷贝 + 改 id/name/category/version。
#[derive(Default)]
pub struct CloneProjectState {
    pub open: bool,
    pub source_project_id: String,
    pub source_display: String,
    pub project_id: String,
    pub project_name: String,
    pub project_category: String,
    pub project_version: String,
    /// 标记 id 是否已被用户改过（cancel 后再次打开沿用 *_copy 的默认值）
    pub id_prefilled: bool,
}

impl CloneProjectState {
    pub fn open_clone(
        &mut self,
        source_id: &str,
        source_display: &str,
        source_category: &str,
        source_version: &str,
    ) {
        self.open = true;
        self.source_project_id = source_id.to_string();
        self.source_display = source_display.to_string();
        self.project_id = format!("{}_copy", source_id);
        self.project_name = format!("{}_copy", source_display);
        self.project_category = source_category.to_string();
        self.project_version = if source_version.is_empty() {
            "1.0.0".to_string()
        } else {
            source_version.to_string()
        };
        self.id_prefilled = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.source_project_id.clear();
        self.source_display.clear();
        self.project_id.clear();
        self.project_name.clear();
        self.project_category.clear();
        self.project_version.clear();
        self.id_prefilled = false;
    }
}

/// 项目设置对话框（项目右键「项目设置...」入口）。
///
/// 一锁式编辑全部 project meta + 分隔符；旧两段式 RenameProject 流程已被替代。
/// id 改动 → 复用 engine `RenameProject` 动作处理目录 rename；
/// name/category/version/separators → 直接落 schema.meta + schema.separators，
/// schema_dirty=true，写盘 + revalidate_all。
#[derive(Default)]
pub struct ProjectSettingsState {
    pub open: bool,
    /// 当前编辑的 project id（也是 dialog 打开时的 old_id）
    pub project_id: String,
    /// 0=身份  1=分隔符
    pub tab: i32,
    pub id_buf: String,
    pub name_buf: String,
    pub category_buf: String,
    pub version_buf: String,
    /// id 实时校验消息（空 = 合法）；不合法时禁用「确定」
    pub id_error: String,
    /// 分隔符编辑缓冲
    pub sep: tbl_core::types::SeparatorsSection,
}

impl ProjectSettingsState {
    pub fn close(&mut self) {
        self.open = false;
        self.project_id.clear();
        self.tab = 0;
        self.id_buf.clear();
        self.name_buf.clear();
        self.category_buf.clear();
        self.version_buf.clear();
        self.id_error.clear();
        self.sep = tbl_core::types::SeparatorsSection::default();
    }
}

pub struct AppState {
    pub engine: ProjectEngine,
    pub selected: Option<SelectedNode>,
    pub tree_filter: TreeFilter,
    pub tree_full_group: bool,
    pub tree_search: String,
    /// (project_id, group_name) → 已展开的 group 行
    pub tree_expanded: HashSet<(String, String)>,
    /// project_id → 已展开的 project 根
    pub project_expanded: HashSet<String>,
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
    /// 表头 picker 单元格呼出方式：true = 单击呼出，false = 双击呼出（[ui] picker_trigger_header）
    pub picker_trigger_header_single: bool,
    /// 数据区 picker 单元格呼出方式：true = 单击呼出，false = 双击呼出（[ui] picker_trigger_data）
    pub picker_trigger_data_single: bool,
    /// 是否允许 Constant 表使用 @Xxx 引用类型（[ui] constant_ref_allowed）。
    /// 控制 TypeSelector 的 Reference tab 是否对 Constant 开放。
    pub constant_ref_allowed: bool,
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
    /// 数据导出对话框
    pub data_export: DataExportState,
    /// Schema 导出对话框
    pub schema_export: SchemaExportState,
    /// Schema 导入对话框
    pub schema_import: SchemaImportState,
    /// 「新建项目」统一对话框（3 tab：空 / 文件 / 模板）
    pub create_project: CreateProjectState,
    /// 「复制(克隆)项目」对话框（项目右键专用）
    pub clone_project: CloneProjectState,
    /// 「项目设置」对话框（id / name / category / version / 分隔符）
    pub project_settings: ProjectSettingsState,
    /// "id" / "name" / "open" / "created" / "manual"。从 [project] project_sort 读初值，UI 写时持久化。
    pub project_sort: String,
    /// sort=manual 时使用；UI 拖拽顺序持久化到 [project] project_order。
    pub project_order: Vec<String>,
}

impl AppState {
    pub fn load(workdir: &Path) -> anyhow::Result<Self> {
        let mut engine = load_workspace(workdir)?;
        // active 缺失时回落到首个 opened；都没有则取 default 配置。
        let cfg_src = engine.active().or_else(|| engine.projects.first());
        let rt_validate = cfg_src
            .and_then(|p| p.config.ui.as_ref())
            .map_or(false, |u| u.realtime_validate);
        let header_single = cfg_src
            .and_then(|p| p.config.ui.as_ref())
            .map_or(true, |u| u.picker_trigger_header == "single");
        let data_single = cfg_src
            .and_then(|p| p.config.ui.as_ref())
            .map_or(false, |u| u.picker_trigger_data == "single");
        let constant_ref_allowed = cfg_src
            .and_then(|p| p.config.ui.as_ref())
            .map_or(true, |u| u.constant_ref_allowed);
        let project_sort = cfg_src
            .map(|p| p.config.project.project_sort.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "id".to_string());
        let project_order = cfg_src
            .map(|p| p.config.project.project_order.clone())
            .unwrap_or_default();
        let (expanded, project_expanded) = if let Some(active) = engine.active() {
            let aid = active.schema.meta.id.clone();
            let exp: HashSet<(String, String)> = active.groups.iter()
                .map(|g| (aid.clone(), g.name.clone()))
                .collect();
            let proj_exp: HashSet<String> = std::iter::once(aid).collect();
            (exp, proj_exp)
        } else {
            (HashSet::new(), HashSet::new())
        };
        let group_count = engine.active().map(|p| p.groups.len()).unwrap_or(0);
        engine.log(format!("已加载 {} 个 Group", group_count));
        // 加载即跑一遍全 Project 验证：让所有 project 的红框/`!` 标记打开就可见。
        engine.revalidate_all_projects();
        if !engine.validation_errors.is_empty() {
            engine.log(format!("[验证] 加载后发现 {} 个错误", engine.validation_errors.len()));
        }
        Ok(Self {
            engine,
            selected: None,
            tree_filter: TreeFilter::All,
            tree_full_group: false,
            tree_search: String::new(),
            tree_expanded: expanded,
            project_expanded,
            tree_targets: Vec::new(),
            view_show_enum_name: false,
            grid_selection: GridSelection::None,
            grid_column_kinds: Vec::new(),
            grid_header_kinds: Vec::new(),
            grid_data_count: 0,
            realtime_validate: rt_validate,
            picker_trigger_header_single: header_single,
            picker_trigger_data_single: data_single,
            constant_ref_allowed,
            editing: None,
            editing_buffer: String::new(),
            editing_in_formula: false,
            editing_header_row: -1,
            editing_header_col: -1,
            type_selector: TypeSelectorState::new(),
            ref_picker: RefPickerState::new(),
            ctx_menu: ContextMenuState::default(),
            pending: PendingActionState::default(),
            data_export: DataExportState::default(),
            schema_export: SchemaExportState::default(),
            schema_import: SchemaImportState::default(),
            create_project: CreateProjectState::default(),
            clone_project: CloneProjectState::default(),
            project_settings: ProjectSettingsState::default(),
            project_sort,
            project_order,
        })
    }

    /// 写一个单元格的真实存储值；按当前 selected 节点类型分发到 engine。
    /// realtime_validate 开启时立即重算该节点的 validation_errors。
    pub fn set_cell(&mut self, r: usize, c: usize, val: &str) {
        let (group, name, is_table, is_constant) = match &self.selected {
            Some(SelectedNode::Table { group, name, .. }) => (group.clone(), name.clone(), true, false),
            Some(SelectedNode::Constant { group, name, .. }) => (group.clone(), name.clone(), false, true),
            Some(SelectedNode::Enum { group, name, .. }) => (group.clone(), name.clone(), false, false),
            _ => return,
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
            Some(SelectedNode::Table { group, name, .. }) => (group.clone(), name.clone()),
            _ => return,
        };
        self.engine.commit_header_edit(&group, &name, hi, ci, val);
        if self.realtime_validate {
            self.engine.revalidate(&group, &name);
        }
    }
}
