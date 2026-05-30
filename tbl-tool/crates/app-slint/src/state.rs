// AppState：Rust 侧的真值，所有 UI 数据从这里派生。
// slint 是 retained UI，需要 Rust 显式 push 数据；AppState 持有 engine + UI 临时态。

use std::collections::HashSet;
use tbl_core::ops::ProjectEngine;
use tbl_core::project::load_project;
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
