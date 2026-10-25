// 右键菜单：列出菜单项 + 处理 dispatch（按 kind+action_id 分发）。
//
// kind 来源：tree 节点 / tree 空白 / grid 列字母 / grid 行号 / grid 数据格。
// action_id 形如 "tree.new-group" / "grid.col-insert-left"。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::{
    AppState, CtxMenuKind, GridSelection, PendingAction,
};
use crate::{dialogs, refresh, ui, AppWindow, CtxMenuItem};

/// 计算当前 ctx_menu.kind 应展示的菜单项列表。
/// 右键菜单里的全部动作。**这是菜单的唯一清单**：
/// - [`sections_for`] 决定「什么层级、什么顺序、怎么分组」；
/// - [`entry`] 决定「当前状态下这一项出不出现、显示什么文案」。
///
/// 不可用的项直接不出现（用户要求：不再展示置灰项），所以菜单项不再有 disabled。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    // 树空白
    NewProject,
    NewGroup,
    NewXlsxGroup,
    // 项目
    SaveProject,
    ExportProject,
    ExportSchema,
    ExportTemplate,
    MergeSchema,
    ImportTbl,
    ImportXlsx,
    NewGroupInProject,
    NewXlsxGroupInProject,
    CloneProject,
    ProjectSettings,
    DeleteProject,
    CloseProject,
    OpenProject,
    OpenProjectDir,
    // 组 / 节点
    NewTableV3,
    NewTableV2,
    NewConstant,
    NewEnum,
    ExcelOpenGroup,
    ReloadGroup,
    CopyGroup,
    CopyNode,
    PasteNode,
    PasteNodeV3,
    PasteNodeV2,
    PasteGroupTbl,
    PasteGroupXlsx,
    Rules,
    RenameGroup,
    RenameNode,
    DeleteGroup,
    DeleteNode,
    // 表格区
    ColInsertLeft,
    ColInsertRight,
    ColDelete,
    RowInsertAbove,
    RowInsertBelow,
    RowDelete,
    CellPick,
    CellCopy,
    CellCut,
    CellPaste,
    CellClear,
}

impl MenuAction {
    /// 分发用的稳定 id（与 `wire()` 里的 match 一一对应）。
    pub fn id(self) -> &'static str {
        use MenuAction::*;
        match self {
            NewProject => "tree.new-project",
            NewGroup => "tree.new-group",
            NewXlsxGroup => "tree.new-xlsx-group",
            SaveProject => "tree.proj-save",
            ExportProject => "tree.proj-export",
            ExportSchema => "tree.proj-export-schema",
            ExportTemplate => "tree.proj-export-template",
            MergeSchema => "tree.proj-merge-schema",
            ImportTbl => "tree.proj-import-tbl",
            ImportXlsx => "tree.proj-import-xlsx",
            NewGroupInProject => "tree.proj-new-group",
            NewXlsxGroupInProject => "tree.proj-new-xlsx-group",
            CloneProject => "tree.proj-clone",
            ProjectSettings => "tree.proj-settings",
            DeleteProject => "tree.proj-delete",
            CloseProject => "tree.proj-close",
            OpenProject => "tree.proj-open",
            OpenProjectDir => "tree.proj-open-dir",
            NewTableV3 => "tree.new-table",
            NewTableV2 => "tree.new-table-v2",
            NewConstant => "tree.new-constant",
            NewEnum => "tree.new-enum",
            ExcelOpenGroup => "tree.excel-open-group",
            ReloadGroup => "tree.reload-group",
            CopyGroup => "tree.copy-group",
            CopyNode => "tree.copy-node",
            PasteNode => "tree.paste-node",
            PasteNodeV3 => "tree.paste-node-v3",
            PasteNodeV2 => "tree.paste-node-v2",
            PasteGroupTbl => "tree.paste-group-tbl",
            PasteGroupXlsx => "tree.paste-group-xlsx",
            Rules => "tree.rules",
            RenameGroup => "tree.rename-group",
            RenameNode => "tree.rename-node",
            DeleteGroup => "tree.delete-group",
            DeleteNode => "tree.delete-node",
            ColInsertLeft => "grid.col-insert-left",
            ColInsertRight => "grid.col-insert-right",
            ColDelete => "grid.col-delete",
            RowInsertAbove => "grid.row-insert-above",
            RowInsertBelow => "grid.row-insert-below",
            RowDelete => "grid.row-delete",
            CellPick => "grid.cell-pick",
            CellCopy => "grid.cell-copy",
            CellCut => "grid.cell-cut",
            CellPaste => "grid.cell-paste",
            CellClear => "grid.cell-clear",
        }
    }
}

fn separator() -> CtxMenuItem {
    CtxMenuItem {
        label: slint::SharedString::new(),
        action_id: slint::SharedString::new(),
        is_separator: true,
    }
}

/// 每个层级要展示哪些动作、按什么顺序、怎么分组（外层 = 分组，组间自动加分隔线）。
fn sections_for(kind: &CtxMenuKind, state: &AppState) -> Vec<Vec<MenuAction>> {
    use MenuAction::*;
    match kind {
        CtxMenuKind::TreeBlank => {
            if state.engine.projects.is_empty() {
                vec![vec![NewProject]]
            } else {
                vec![vec![NewGroup, NewXlsxGroup]]
            }
        }
        CtxMenuKind::TreeProject { project_id } => {
            if state.engine.is_opened(project_id) {
                vec![
                    vec![SaveProject, ExportProject, ExportSchema, ExportTemplate, MergeSchema],
                    vec![NewGroupInProject, NewXlsxGroupInProject, ImportTbl, ImportXlsx, CloneProject],
                    vec![PasteGroupTbl, PasteGroupXlsx],
                    vec![ProjectSettings, DeleteProject],
                    vec![CloseProject],
                    vec![OpenProjectDir],
                ]
            } else {
                vec![
                    vec![OpenProject],
                    vec![OpenProjectDir, ProjectSettings, DeleteProject],
                ]
            }
        }
        CtxMenuKind::TreeGroup { .. } => vec![
            vec![NewTableV3, NewTableV2, NewConstant, NewEnum],
            vec![ExcelOpenGroup],
            vec![
                CopyGroup, PasteNode, PasteNodeV3, PasteNodeV2,
                PasteGroupTbl, PasteGroupXlsx,
            ],
            vec![RenameGroup, DeleteGroup, ReloadGroup],
        ],
        CtxMenuKind::TreeNode { .. } => vec![
            vec![Rules],
            vec![CopyNode, PasteNode, PasteNodeV3, PasteNodeV2],
            vec![RenameNode, DeleteNode],
        ],
        CtxMenuKind::GridCol { .. } => vec![vec![ColInsertLeft, ColInsertRight, ColDelete]],
        CtxMenuKind::GridRow { .. } => vec![vec![RowInsertAbove, RowInsertBelow, RowDelete]],
        CtxMenuKind::GridCell { .. } => {
            vec![vec![CellPick], vec![CellCopy, CellCut, CellPaste, CellClear]]
        }
    }
}

/// 判断一个动作在当前层级 / 状态下是否可用，并给出文案（不可用返回 None）。
fn entry(action: MenuAction, kind: &CtxMenuKind, state: &AppState) -> Option<CtxMenuItem> {
    use tablet_core::ops::{NodeBody, NodeClipboard, NodeKind};

    let cb = state.engine.node_clipboard.as_ref();
    let has_node_cb = cb.map_or(false, NodeClipboard::is_node);
    let has_group_cb = cb.map_or(false, NodeClipboard::is_group);
    let cb_is_table = matches!(
        cb,
        Some(NodeClipboard::Node { body: NodeBody::Table(_), .. })
    );

    let group = match kind {
        CtxMenuKind::TreeGroup { project_id, name } => state.engine
            .find_project(project_id)
            .and_then(|p| p.groups.iter().find(|g| &g.name == name)),
        _ => None,
    };
    let is_xlsx = group.map_or(false, |g| g.is_xlsx());
    let node_is_constant = matches!(
        kind,
        CtxMenuKind::TreeNode { kind: NodeKind::Constant, .. }
    );

    // 可用性：数据相关的条件集中在这里
    let available = match action {
        MenuAction::PasteNode => has_node_cb,
        MenuAction::PasteNodeV3 | MenuAction::PasteNodeV2 => cb_is_table,
        MenuAction::PasteGroupTbl | MenuAction::PasteGroupXlsx => has_group_cb,
        // Tbl 组走临时本会话，有 Excel 会话进行中时不可用；Xlsx 组开原文件，不受限。
        MenuAction::ExcelOpenGroup => is_xlsx || state.excel_session.is_none(),
        MenuAction::ReloadGroup => is_xlsx,
        MenuAction::Rules => !node_is_constant,
        MenuAction::ColInsertLeft | MenuAction::ColDelete =>
            matches!(kind, CtxMenuKind::GridCol { col } if *col != 0),
        _ => true,
    };
    if !available {
        return None;
    }

    // 文案：只有少数动作依赖上下文
    let label: String = match action {
        MenuAction::NewProject => "新建项目".into(),
        MenuAction::NewGroup | MenuAction::NewGroupInProject => "新建 Group".into(),
        MenuAction::NewXlsxGroup | MenuAction::NewXlsxGroupInProject => "新建 Excel Group".into(),
        MenuAction::SaveProject => "保存".into(),
        MenuAction::ExportProject => "导出...".into(),
        MenuAction::ExportSchema => "导出 Schema...".into(),
        MenuAction::ExportTemplate => "另存为模板...".into(),
        MenuAction::MergeSchema => "合并 Schema...".into(),
        MenuAction::ImportTbl => "导入 .tbl 文件...".into(),
        MenuAction::ImportXlsx => "导入 Excel Group...".into(),
        MenuAction::CloneProject => "克隆项目...".into(),
        MenuAction::ProjectSettings => "项目设置...".into(),
        MenuAction::DeleteProject => "删除".into(),
        MenuAction::CloseProject => "关闭".into(),
        MenuAction::OpenProject => "打开".into(),
        MenuAction::OpenProjectDir => "打开所在目录".into(),
        MenuAction::NewTableV3 => "新建 Table（v3）".into(),
        MenuAction::NewTableV2 => "新建 Table（v2）".into(),
        MenuAction::NewConstant => "新建 Constant".into(),
        MenuAction::NewEnum => "新建 Enum".into(),
        MenuAction::ExcelOpenGroup => if is_xlsx {
            "用 Excel 打开（原文件）".into()
        } else {
            "用 Excel 打开（整组）".into()
        },
        MenuAction::ReloadGroup => "重新加载".into(),
        MenuAction::CopyGroup => "复制 Group".into(),
        MenuAction::CopyNode => "复制".into(),
        MenuAction::PasteNode => "粘贴节点".into(),
        MenuAction::PasteNodeV3 => "粘贴为 v3".into(),
        MenuAction::PasteNodeV2 => "粘贴为 v2".into(),
        MenuAction::PasteGroupTbl => "粘贴为 .tbl Group".into(),
        MenuAction::PasteGroupXlsx => "粘贴为 Excel Group".into(),
        MenuAction::Rules => "校验规则...".into(),
        MenuAction::RenameGroup | MenuAction::RenameNode => "重命名".into(),
        MenuAction::DeleteGroup | MenuAction::DeleteNode => "删除".into(),
        MenuAction::ColInsertLeft => "左侧插入列".into(),
        MenuAction::ColInsertRight => "右侧插入列".into(),
        MenuAction::ColDelete => "删除列".into(),
        MenuAction::RowInsertAbove => "上方插入行".into(),
        MenuAction::RowInsertBelow => "下方插入行".into(),
        MenuAction::RowDelete => "删除行".into(),
        MenuAction::CellPick => {
            let CtxMenuKind::GridCell { row, col } = kind else { return None };
            if !matches!(state.grid_selection, GridSelection::Cell(_, _)) {
                return None;
            }
            ui::grid_actions::effective_column_kind_at(state, *row, *col)
                .as_ref()
                .and_then(|k| k.picker_action_label())
                .map(|s| s.to_string())?
        }
        MenuAction::CellCopy => "复制".into(),
        MenuAction::CellCut => "剪切".into(),
        MenuAction::CellPaste => "粘贴".into(),
        MenuAction::CellClear => "清空内容".into(),
    };

    Some(CtxMenuItem {
        label: label.into(),
        action_id: action.id().into(),
        is_separator: false,
    })
}

/// 计算当前 ctx_menu.kind 应展示的菜单项列表：分组 → 过滤不可用 → 组间加分隔线。
pub(crate) fn items_for(kind: &CtxMenuKind, state: &AppState) -> Vec<CtxMenuItem> {
    let mut out: Vec<CtxMenuItem> = Vec::new();
    for section in sections_for(kind, state) {
        let mut part: Vec<CtxMenuItem> = section
            .into_iter()
            .filter_map(|a| entry(a, kind, state))
            .collect();
        if part.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(separator());
        }
        out.append(&mut part);
    }
    out
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let cm = &st.ctx_menu;
    ui_h.set_ctx_menu_open(cm.open);
    if !cm.open {
        ui_h.set_ctx_menu_items(slint::ModelRc::new(slint::VecModel::from(Vec::<CtxMenuItem>::new())));
        return;
    }
    let kind = match &cm.kind { Some(k) => k, None => return };
    let items = items_for(kind, &st);
    // 全被隐藏（例如剪贴板为空时的粘贴项）→ 不弹空菜单
    if items.is_empty() {
        drop(st);
        state.borrow_mut().ctx_menu.open = false;
        ui_h.set_ctx_menu_open(false);
        ui_h.set_ctx_menu_items(slint::ModelRc::new(slint::VecModel::from(Vec::<CtxMenuItem>::new())));
        return;
    }
    ui_h.set_ctx_menu_x(cm.x);
    ui_h.set_ctx_menu_y(cm.y);
    ui_h.set_ctx_menu_items(slint::ModelRc::new(slint::VecModel::from(items)));
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ctx_menu_dismiss(move || {
            s.borrow_mut().ctx_menu.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // ctx-menu-action(action_id)：根据当前 ctx_menu.kind + id 分发
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ctx_menu_action(move |id| {
            let id = id.to_string();
            // 取走当前 kind 后立即关闭菜单
            let kind = {
                let mut st = s.borrow_mut();
                let k = st.ctx_menu.kind.clone();
                st.ctx_menu.close();
                k
            };
            match (kind, id.as_str()) {
                (Some(CtxMenuKind::TreeBlank), "tree.new-project") => {
                    s.borrow_mut().create_project.open_empty();
                    if let Some(ui_h) = weak.upgrade() {
                        dialogs::create_project::push(&ui_h, &s);
                    }
                }
                (Some(CtxMenuKind::TreeBlank), act @ ("tree.new-group" | "tree.new-xlsx-group")) => {
                    let project_id = {
                        let st = s.borrow();
                        st.selected.as_ref().map(|sn| sn.project_id().to_string())
                            .or_else(|| st.engine.projects.first().map(|p| p.schema.meta.id.clone()))
                            .unwrap_or_default()
                    };
                    let storage = if act == "tree.new-xlsx-group" {
                        tablet_core::model::GroupStorage::Xlsx
                    } else {
                        tablet_core::model::GroupStorage::Tbl
                    };
                    s.borrow_mut().pending.open(PendingAction::NewGroup { project_id, storage });
                }
                (Some(CtxMenuKind::TreeProject { project_id }), "tree.proj-import-tbl") => {
                    if let Some(files) = rfd::FileDialog::new()
                        .add_filter("TBL", &["tbl"])
                        .pick_files()
                    {
                        let parent = files.first().and_then(|p| p.parent());
                        let group = parent.and_then(|p| p.file_name())
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let same_parent = files.iter().all(|p| p.parent() == parent);
                        if !same_parent {
                            dialogs::alert::raise(&s, "导入失败",
                                "一次导入的 .tbl 必须来自同一个目录（目录名 = 组名）");
                        } else {
                            match s.borrow_mut().engine.import_tbl_files(&project_id, &group, &files) {
                                Ok(out) => {
                                    let mut st = s.borrow_mut();
                                    st.project_expanded.insert(project_id.clone());
                                    st.tree_expanded.insert((project_id.clone(), out.group.clone()));
                                    st.select_pasted_group(&project_id, &out.group);
                                }
                                Err(e) => dialogs::alert::raise(&s, "导入 .tbl 失败", &e),
                            }
                        }
                    }
                }
                (Some(CtxMenuKind::TreeProject { project_id }), "tree.proj-import-xlsx") => {
                    if let Some(file) = rfd::FileDialog::new()
                        .add_filter("Excel", &["xlsx"])
                        .pick_file()
                    {
                        match s.borrow_mut().engine.import_xlsx_group(&project_id, &file) {
                            Ok(out) => {
                                let mut st = s.borrow_mut();
                                st.project_expanded.insert(project_id.clone());
                                st.tree_expanded.insert((project_id.clone(), out.group.clone()));
                                st.select_pasted_group(&project_id, &out.group);
                            }
                            Err(e) => dialogs::alert::raise(&s, "导入 Excel Group 失败", &e),
                        }
                    }
                }
                (Some(CtxMenuKind::TreeProject { project_id }), action) => {
                    dialogs::pending::handle_project_root_action(&s, &project_id, action);
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.new-table") => {
                    s.borrow_mut().pending.open(PendingAction::NewTable {
                        project_id, group: name,
                        fmt_ver: tablet_core::tbl::TblVersion::V3,
                    });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.new-table-v2") => {
                    s.borrow_mut().pending.open(PendingAction::NewTable {
                        project_id, group: name,
                        fmt_ver: tablet_core::tbl::TblVersion::V2,
                    });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.new-constant") => {
                    s.borrow_mut().pending.open(PendingAction::NewConstant { project_id, group: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.new-enum") => {
                    s.borrow_mut().pending.open(PendingAction::NewEnum { project_id, group: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.rename-group") => {
                    // 预填由 PendingActionState::open 统一做（按 old_name）
                    s.borrow_mut().pending.open(PendingAction::RenameGroup { project_id, old_name: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.delete-group") => {
                    s.borrow_mut().pending.open(PendingAction::DeleteGroup { project_id, group: name });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.excel-open-group") => {
                    let is_xlsx = s.borrow().engine.find_project(&project_id)
                        .and_then(|p| p.groups.iter().find(|g| g.name == name).map(|g| g.is_xlsx()))
                        .unwrap_or(false);
                    if is_xlsx {
                        match crate::excel_bridge::open_native_book(&s, &project_id, &name) {
                            Ok(()) => s.borrow_mut().engine.ui_log(
                                format!("[Excel] 已打开组 {} 的工作簿", name)),
                            Err(e) => s.borrow_mut().engine.error_log(
                                format!("[Excel] 打开失败: {}", e)),
                        }
                    } else {
                        // 整组 Excel 编辑：include 空 vec → core 侧 None（@plans §5.1.2）
                        let weak_ui = weak.clone();
                        let result = crate::excel_bridge::launch_excel_edit(
                            &s, weak_ui, &project_id, &name, vec![],
                        );
                        if let Err(e) = result {
                            s.borrow_mut().engine.ui_log(format!("[Excel] 调起失败: {}", e));
                        }
                        if let Some(ui_h) = weak.upgrade() {
                            crate::excel_bridge::push(&ui_h, &s);
                        }
                    }
                    if let Some(ui_h) = weak.upgrade() {
                        crate::refresh::after_log(&ui_h, &s);
                    }
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.reload-group") => {
                    s.borrow_mut().pending.open(PendingAction::ReloadXlsxGroup {
                        project_id, group: name,
                    });
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), "tree.copy-group") => {
                    s.borrow_mut().engine.clipboard_copy_group(&project_id, &name);
                }
                (Some(CtxMenuKind::TreeGroup { project_id, name }), action @ ("tree.paste-node" | "tree.paste-node-v3" | "tree.paste-node-v2")) => {
                    let fmt_ver = match action {
                        "tree.paste-node-v3" => Some(tablet_core::tbl::TblVersion::V3),
                        "tree.paste-node-v2" => Some(tablet_core::tbl::TblVersion::V2),
                        _ => None,
                    };
                    let mut st = s.borrow_mut();
                    if let Some(new_name) = st.engine.paste_node_to(&project_id, &name, fmt_ver) {
                        st.tree_expanded.insert((project_id.clone(), name.clone()));
                        st.select_pasted_node(&project_id, &name, &new_name);
                    }
                }
                (Some(CtxMenuKind::TreeGroup { project_id, .. }),
                 act @ ("tree.paste-group-tbl" | "tree.paste-group-xlsx")) => {
                    let storage = if act == "tree.paste-group-xlsx" {
                        tablet_core::model::GroupStorage::Xlsx
                    } else {
                        tablet_core::model::GroupStorage::Tbl
                    };
                    let new_group = crate::dialogs::paste_group::request(&s, &project_id, storage);
                    if let Some(new_group) = new_group {
                        let mut st = s.borrow_mut();
                        st.project_expanded.insert(project_id.clone());
                        st.tree_expanded.insert((project_id.clone(), new_group.clone()));
                        st.select_pasted_group(&project_id, &new_group);
                    }
                }
                (Some(CtxMenuKind::TreeNode { group, name, .. }), "tree.rules") => {
                    dialogs::rules_panel::open_for_node(&s, &group, &name);
                    if let Some(ui_h) = weak.upgrade() { dialogs::rules_panel::push(&ui_h, &s); }
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, name, kind }), "tree.copy-node") => {
                    s.borrow_mut().engine.clipboard_copy_node(&project_id, &group, &name, kind);
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, .. }), action @ ("tree.paste-node" | "tree.paste-node-v3" | "tree.paste-node-v2")) => {
                    let fmt_ver = match action {
                        "tree.paste-node-v3" => Some(tablet_core::tbl::TblVersion::V3),
                        "tree.paste-node-v2" => Some(tablet_core::tbl::TblVersion::V2),
                        _ => None,
                    };
                    let mut st = s.borrow_mut();
                    if let Some(new_name) = st.engine.paste_node_to(&project_id, &group, fmt_ver) {
                        st.tree_expanded.insert((project_id.clone(), group.clone()));
                        st.select_pasted_node(&project_id, &group, &new_name);
                    }
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, name, .. }), "tree.rename-node") => {
                    // 预填由 PendingActionState::open 统一做（按 old_name）
                    s.borrow_mut().pending.open(PendingAction::RenameNode { project_id, group, old_name: name });
                }
                (Some(CtxMenuKind::TreeNode { project_id, group, name, .. }), "tree.delete-node") => {
                    s.borrow_mut().pending.open(PendingAction::DeleteNode { project_id, group, name });
                }
                (Some(CtxMenuKind::GridCol { col }), action @ ("grid.col-insert-left"
                    | "grid.col-insert-right" | "grid.col-delete")) => {
                    ui::grid_actions::perform_col_action(&s, col, action);
                }
                (Some(CtxMenuKind::GridRow { row }), action @ ("grid.row-insert-above"
                    | "grid.row-insert-below" | "grid.row-delete")) => {
                    ui::grid_actions::perform_row_action(&s, row, action);
                }
                (Some(CtxMenuKind::GridCell { row, col }), "grid.cell-pick") => {
                    // 等价于双击 picker cell：弹 RefPicker / TypeSelector
                    // ExportEnumCol cell 的 popup 是 slint 端 component-internal property，
                    // 没暴露到 Rust 端，只能由用户双击/单击 cell 触发；右键菜单不接管。
                    let kind = ui::grid_actions::effective_column_kind_at(&s.borrow(), row, col);
                    match kind {
                        Some(crate::state::ColumnKind::Ref { ref target }) => {
                            dialogs::ref_picker::open_for_cell(&s, row, col, target, false);
                            if let Some(ui_h) = weak.upgrade() { dialogs::ref_picker::push(&ui_h, &s); }
                        }
                        Some(crate::state::ColumnKind::RefList { ref target }) => {
                            dialogs::ref_picker::open_for_cell(&s, row, col, target, true);
                            if let Some(ui_h) = weak.upgrade() { dialogs::ref_picker::push(&ui_h, &s); }
                        }
                        Some(crate::state::ColumnKind::TypeEnumCol) => {
                            dialogs::type_selector::open_for_cell(&s, row, col);
                            if let Some(ui_h) = weak.upgrade() { dialogs::type_selector::push(&ui_h, &s); }
                        }
                        _ => {}
                    }
                }
                (Some(CtxMenuKind::GridCell { row: _, col: _ }), action @ ("grid.cell-copy"
                    | "grid.cell-cut" | "grid.cell-paste" | "grid.cell-clear")) => {
                    let tag = match action {
                        "grid.cell-copy" => "复制",
                        "grid.cell-cut" => "剪切",
                        "grid.cell-paste" => "粘贴",
                        "grid.cell-clear" => "清空",
                        _ => "操作",
                    };
                    ui::grid_actions::perform_action(&s, action, tag);
                }
                _ => {}
            }
            // pending input 需要校验首次 buffer
            dialogs::pending::revalidate_input(&s);
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_ctx_menu(&ui_h, &s);
            }
        });
    }
}
