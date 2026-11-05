// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! Tauri 后端：把 `tablet-core` 的 ProjectEngine 投影成前端要的快照。
//!
//! 设计原则（@docs/18）：
//! - 前端只负责渲染与交互，业务规则、脏标记、校验、撤销全部走 core；
//! - 每次操作返回一份完整快照（树 + 表格 + 状态 + 日志），前端整体替换；
//!   数据量在配置表场景下可控，换来的是没有增量同步的一致性问题；
//! - 表格的行模型包含 5 行占位行（EXTRA_ROWS），编辑占位行等价于新增记录。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tablet_core::enums::{LogLevel, PickerTrigger};
use tablet_core::model::{
    ClientConfig, Constant, CppExport, DotNetExport, Export, ExportConfig, GdScriptExport,
    GoExport, GroupStorage, JavaExport, JsonExport, LuaExport, NodeKind, ServerExport,
    ServerTypeScriptExport, SeverityAction, Table, UiConfig, UnityCSharpExport, ValidateConfig,
    XmlExport, ClientTypeScriptExport, GodotCSharpExport,
};
use tablet_core::ops::{ProjectAction, ProjectEngine};
use tablet_core::project::load_workspace;
use tablet_core::rules::{
    builtin_rules, FunctionStatus, Granularity, NodeMode, ParamKind, Rule, RuleExpr, RuleScope,
    RuleSource, Severity, FUNCTIONS,
};
use tablet_core::schema_model::{SchemaMetadata, TblSchema};
use tablet_core::schema_projection::apply_schema_to_project;
use tablet_core::tblschema::parse_tblschema;
use tablet_core::template::{BuiltinTemplates, LocalTemplates, TemplateSource};
use tablet_core::types::{SepKey, SeparatorsSection};
use tablet_cli::actions::export::{run_export, ExportFormats, FormatOutcome};
use tablet_cli::actions::excel::{run_excel_export, run_excel_import, ExcelTarget};
use tablet_cli::actions::schema::{run_schema_export, SchemaExportOptions};

/// 与 Slint 版一致：表格尾部留 5 行占位，方便直接输入新记录。
const EXTRA_ROWS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelKind {
    Project,
    Group,
    Table,
    Constant,
    Enum,
}

#[derive(Clone)]
pub struct Selected {
    pub project: String,
    pub group: String,
    pub name: String,
    pub kind: SelKind,
}

pub struct Backend {
    pub workdir: PathBuf,
    pub engine: ProjectEngine,
    pub selected: Option<Selected>,
    /// 引用列是否显示目标节点的展示值（@10 的总闸，会话内状态）。
    pub show_ref_name: bool,
    /// Excel 编辑会话：导出到临时工作簿 → 用 Excel 打开 → 轮询 mtime 自动回写。
    pub excel_sessions: Vec<ExcelSession>,
}

pub struct ExcelSession {
    pub project: String,
    pub group: String,
    pub path: PathBuf,
    pub mtime: Option<std::time::SystemTime>,
}

#[derive(Serialize)]
pub struct ExcelPollItem {
    pub project: String,
    pub group: String,
    pub path: String,
}

// ── 快照结构（前端直接消费）────────────────────────────────
#[derive(Serialize)]
pub struct Snapshot {
    pub version: &'static str,
    pub target: &'static str,
    pub workdir: String,
    pub vcs: String,
    pub tree: Vec<TreeRow>,
    pub grid: Option<GridPayload>,
    pub status: Status,
    pub logs: Vec<LogRow>,
    pub issues: Vec<IssueRow>,
}

#[derive(Serialize)]
pub struct TreeRow {
    pub project: String,
    pub project_name: String,
    /// project / group / table / constant / enum
    pub kind: String,
    pub group: Option<String>,
    pub name: Option<String>,
    pub label: String,
    /// 小写名字 + 拼音首字母，供前端搜索直接 contains。
    pub search_key: String,
    pub mark: String,
    pub mark_color: String,
    pub closed: bool,
    pub active: bool,
    /// 仅 group 行有意义：该组是否是 xlsx（工作簿）组。
    pub xlsx: bool,
}

#[derive(Serialize)]
pub struct Status {
    pub active_project: Option<String>,
    pub active_project_name: Option<String>,
    pub selected_label: String,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// 当前节点与盘上不一致（Ribbon「还原」是否可用）。
    pub can_revert: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub error_count: usize,
    pub warning_count: usize,
    pub show_ref_name: bool,
    pub excel_editing: Vec<String>,
    pub has_node_clipboard: bool,
    pub has_group_clipboard: bool,
    pub node_clipboard_is_table: bool,
    pub group_clipboard_is_xlsx: bool,
}

#[derive(Serialize)]
pub struct LogRow {
    pub time: String,
    pub level: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct TemplateItem {
    pub id: String,
    pub name: String,
    pub category: String,
    pub version: String,
    pub source: String,
    pub sections: usize,
}

#[derive(Serialize)]
pub struct SectionItem {
    pub group: String,
    pub name: String,
    pub mode: String,
}

#[derive(Serialize)]
pub struct SchemaPreview {
    pub id: String,
    pub name: String,
    pub category: String,
    pub version: String,
    pub has_preset: bool,
    pub sections: Vec<SectionItem>,
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    /// empty / file / template
    pub source: String,
    pub id: String,
    pub name: String,
    pub category: String,
    pub version: String,
    pub path: Option<String>,
    pub template_id: Option<String>,
    /// "Group/Name"；空 = 全部
    pub sections: Vec<String>,
    pub with_preset: bool,
    pub open_now: bool,
}

#[derive(Serialize)]
pub struct ProjectSettings {
    pub id: String,
    pub name: String,
    pub category: String,
    pub version: String,
    /// 上次保存时的身份字段（项目设置里「已修改」高亮与还原的参照）
    pub original_name: String,
    pub original_category: String,
    pub original_version: String,
    pub separators: Vec<SepItem>,
    pub export_settings: ExportUi,
    /// 上次保存时的 [export]
    pub original_export: ExportUi,
    /// 硬编码默认 [export]（「应用模板」用）
    pub default_export: ExportUi,
    pub validate: ValidateUi,
}

/// 校验策略：每个级别一个动作（block / report / ignore）。
#[derive(Serialize, Deserialize)]
pub struct ValidateUi {
    pub error: String,
    pub warning: String,
    pub info: String,
}

#[derive(Serialize, Deserialize)]
pub struct GlobalSettingsUi {
    pub auto_commit_on_blur: bool,
    pub realtime_validate: bool,
    pub log_level: String,
    pub picker_trigger_header: String,
    pub picker_trigger_data: String,
    pub show_meta_id: bool,
    pub constant_ref_allowed: bool,
    /// 引用选择器列展示策略：auto / full
    pub ref_picker_strategy: String,
    pub separators: Vec<SepItem>,
    /// 全局 [export] 模板（新建项目 / 项目设置里留空字段时使用）
    pub export_settings: ExportUi,
    /// 硬编码默认 [export]（「使用默认值」用）
    pub default_export: ExportUi,
}

#[derive(Deserialize)]
pub struct ExportFormatsUi {
    #[serde(default)]
    pub json: bool,
    #[serde(default)]
    pub xml: bool,
    #[serde(default)]
    pub java: bool,
    #[serde(default)]
    pub go: bool,
    #[serde(default)]
    pub cpp: bool,
    #[serde(default)]
    pub csharp_dotnet: bool,
    #[serde(default)]
    pub csharp_unity: bool,
    #[serde(default)]
    pub csharp_godot: bool,
    #[serde(default)]
    pub lua: bool,
    #[serde(default)]
    pub gdscript: bool,
    #[serde(default)]
    pub typescript_client: bool,
    #[serde(default)]
    pub typescript_server: bool,
}

#[derive(Serialize)]
pub struct ExportReport {
    pub ok: bool,
    pub lines: Vec<String>,
}

#[derive(Serialize)]
pub struct IssueRow {
    pub group: String,
    pub name: String,
    pub row: usize,
    pub col: usize,
    pub message: String,
    pub severity: String,
    /// 是否可定位到单元格（表头 / 表级问题只定位到节点）。
    pub jump: bool,
}

#[derive(Serialize)]
pub struct ExcelPreview {
    pub sheets: Vec<String>,
    pub sheet: String,
    pub header_row: u32,
    pub data_start: u32,
    pub columns: Vec<String>,
    pub fields: Vec<MappedFieldUi>,
    pub sample_rows: Vec<Vec<String>>,
}

// ── 单元格校验原因（状态栏用）──────────────────────────────
#[derive(Serialize)]
pub struct CellIssue {
    /// error / warn
    pub severity: String,
    pub message: String,
}

// ── 粘贴 Group 命名确认 ────────────────────────────────────
#[derive(Serialize, Deserialize, Clone)]
pub struct PasteGroupRowUi {
    pub kind: String,
    pub old_name: String,
    pub new_name: String,
    pub conflict: bool,
}

#[derive(Serialize)]
pub struct PasteGroupPreviewUi {
    pub group_name: String,
    pub group_conflict: bool,
    pub has_conflict: bool,
    pub nodes: Vec<PasteGroupRowUi>,
}

// ── Excel 同步面板（core excel_sync 计划）──────────────────
#[derive(Serialize)]
pub struct ExcelSyncOpUi {
    pub code: u32,
    pub label: String,
}

#[derive(Serialize)]
pub struct ExcelSyncRowUi {
    pub group: String,
    pub node: String,
    /// "n行 m列"；None = 这一侧没有
    pub left: Option<String>,
    pub right: Option<String>,
    pub ops: Vec<ExcelSyncOpUi>,
    pub blocked: Option<String>,
    pub desc_diff: bool,
    pub header_edited: bool,
    /// Excel 区域外还有内容（不参与同步，但值得提示）
    pub outside: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MappedFieldUi {
    pub target_name: String,
    pub source_col: u32,
    pub tbl_type: String,
    pub export: String,
    pub desc: String,
}

#[derive(Deserialize)]
pub struct ExcelMapRequest {
    pub path: String,
    pub sheet: String,
    pub header_row: u32,
    pub data_start: u32,
    pub group_name: String,
    pub node_name: String,
    pub fields: Vec<MappedFieldUi>,
}

#[derive(Serialize, Deserialize)]
pub struct SepItem {
    pub key: String,
    pub value: String,
    /// 硬编码默认值（「应用模板」用）。
    #[serde(default)]
    pub default: String,
    /// 上次保存时的值（字段级「已修改 / 还原」的参照）。
    #[serde(default)]
    pub original: String,
}

#[derive(Deserialize)]
pub struct SepInput {
    pub key: String,
    pub value: String,
}

/// 导出设置的可编辑投影。只暴露常用字段；保存时只覆盖这些字段，
/// 其余（如 cpp.json_lib）保持项目里原值。
#[derive(Serialize, Deserialize)]
pub struct ExportUi {
    pub encoding: String,
    pub line_ending: String,
    pub json_empty_as: String,
    pub xml_empty_as: String,
    pub server_data_output: String,
    pub java_package: String,
    pub java_output: String,
    pub java_lombok: bool,
    pub java_custom_regions: bool,
    pub go_package: String,
    pub go_output: String,
    pub go_custom_regions: bool,
    pub cpp_namespace: String,
    pub cpp_output: String,
    pub cpp_custom_regions: bool,
    pub dotnet_namespace: String,
    pub dotnet_output: String,
    pub dotnet_custom_regions: bool,
    pub server_ts_output: String,
    pub lua_output: String,
    pub gdscript_output: String,
    pub client_ts_output: String,
    pub unity_namespace: String,
    pub unity_output: String,
    pub unity_custom_regions: bool,
    pub godot_namespace: String,
    pub godot_output: String,
    pub godot_custom_regions: bool,
}

// ── 校验规则：UI 表达式树 ───────────────────────────────────
//
// 前端编辑的是结构化的规则树；保存时转成 core 的表达式文本，交给
// `schema_grammar::parse_rule_body` 做唯一的语法/作用域校验，避免两套解析器。
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum ExprUi {
    Fun { name: String, args: Vec<String> },
    Group { op: String, members: Vec<ExprUi> },
    When { cond: String, inner: Box<ExprUi> },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RuleItem {
    /// 列名；表级规则为空字符串。
    pub target: String,
    /// error / warn / info
    pub level: String,
    pub expr: ExprUi,
}

#[derive(Serialize)]
pub struct SystemRule {
    pub target: String,
    pub text: String,
    pub level: String,
}

#[derive(Serialize)]
pub struct FunctionInfo {
    pub name: &'static str,
    pub scopes: Vec<&'static str>,
    pub granularity: &'static str,
    pub primary_index: Option<usize>,
    pub params: Vec<&'static str>,
    pub variadic: Option<&'static str>,
    pub summary: &'static str,
}

#[derive(Serialize)]
pub struct RulesPayload {
    pub project: String,
    pub group: String,
    pub name: String,
    pub mode: String,
    pub columns: Vec<String>,
    pub supports_table: bool,
    pub can_edit: bool,
    pub rules: Vec<RuleItem>,
    pub system: Vec<SystemRule>,
    pub functions: Vec<FunctionInfo>,
}

#[derive(Serialize)]
pub struct GridPayload {
    pub mode: String,
    pub title: String,
    pub subtitle: String,
    pub headers: Vec<Vec<String>>,
    /// 表头每格的类型：text / readonly / export / type
    pub header_kinds: Vec<Vec<String>>,
    /// 数据列类型：text / readonly / ref / export / type
    pub kinds: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// 有错误的单元格 [row, col]
    pub errors: Vec<[usize; 2]>,
    pub data_count: usize,
    pub editable_headers: bool,
    /// 当前项目里可作为引用目标的节点名（表 / 枚举）。
    pub ref_targets: Vec<String>,
    /// 当前节点被引用时的展示列（None = 原始 id）。
    pub display_field: Option<String>,
    /// 「被引用展示列」下拉的可选列。
    pub display_options: Vec<String>,
    /// 表头 picker 是否单击呼出（[ui] picker_trigger_header）。
    pub picker_header_single: bool,
    /// 数据区 picker 是否单击呼出（[ui] picker_trigger_data）。
    pub picker_data_single: bool,
    /// 每列的引用目标节点（非引用列为空）。
    pub ref_cols: Vec<Option<String>>,
    /// 每列是否是列表引用（`List<@X>` / `Set<@X>`）。
    pub ref_list: Vec<bool>,
}

#[derive(Serialize)]
pub struct RefHeaderItem {
    /// 英文字段名
    pub field: String,
    /// 中文说明（可为空）
    pub desc: String,
}#[derive(Serialize)]
pub struct RefRowItem {
    pub id: String,
    /// 与 headers 等长，不含 id 列
    pub extras: Vec<String>,
}

/// 引用选择器的完整数据：表头（字段 + 说明）、每行 (id + 辅助列)、当前默认列展示策略。
#[derive(Serialize)]
pub struct RefPickerData {
    pub target: String,
    pub is_table: bool,
    pub target_missing: bool,
    /// auto / full
    pub default_strategy: String,
    pub id_header: RefHeaderItem,
    /// 不含 id 列
    pub headers: Vec<RefHeaderItem>,
    /// 任一 desc 非空 → 表头显示两行（说明 + 字段名）
    pub has_desc: bool,
    pub rows: Vec<RefRowItem>,
}

// ── 类型选择器：范式 / 参数槽 / 预览 ────────────────────────

#[derive(Serialize)]
pub struct TypeSlotInfo {
    pub label: String,
    pub is_map_key: bool,
}

#[derive(Serialize)]
pub struct TypeParadigmInfo {
    /// 与 `Paradigm::all_data()` 同序的下标
    pub index: usize,
    pub label: String,
    pub slots: Vec<TypeSlotInfo>,
}

#[derive(Serialize)]
pub struct TypeRefTargetInfo {
    pub name: String,
    pub is_table: bool,
}

#[derive(Serialize)]
pub struct TypePickerInfo {
    pub current: String,
    pub is_constant: bool,
    /// constant 表是否允许引用类型（全局 [ui] constant_ref_allowed）
    pub allow_ref: bool,
    pub paradigms: Vec<TypeParadigmInfo>,
    pub base_all: Vec<String>,
    pub base_nestable: Vec<String>,
    pub map_keys: Vec<String>,
    pub refs: Vec<TypeRefTargetInfo>,
    /// 当前值解析出的范式下标（非引用类型）
    pub paradigm_index: Option<usize>,
    /// 当前值各参数位的选项字符串（`int` / `@HeroType`）
    pub params: Vec<String>,
    pub ref_name: Option<String>,
}

#[derive(Serialize)]
pub struct TypePreview {
    pub valid: bool,
    pub type_string: String,
    pub example: String,
    pub java: String,
    pub go: String,
    pub lua: String,
    pub error: Option<String>,
}

// ── Schema 导入 / 导出对话框 ────────────────────────────────

#[derive(Serialize)]
pub struct SchemaOutlineNode {
    pub name: String,
    /// table / constant / enum
    pub kind: String,
}

#[derive(Serialize)]
pub struct SchemaOutlineGroup {
    pub group: String,
    pub nodes: Vec<SchemaOutlineNode>,
}

#[derive(Serialize)]
pub struct SchemaPreviewNode {
    pub group: String,
    pub name: String,
    /// table / constant / enum
    pub mode: String,
    /// 当前项目里已存在同名节点（导入会覆盖）
    pub exists: bool,
}

#[derive(Serialize)]
pub struct SchemaImportPreview {
    pub path: String,
    pub has_preset: bool,
    pub meta_id: String,
    pub meta_name: String,
    pub meta_category: String,
    pub meta_version: String,
    pub sections: Vec<SchemaPreviewNode>,
}

impl TypePreview {
    fn invalid(error: impl Into<String>) -> Self {
        Self {
            valid: false,
            type_string: String::new(),
            example: String::new(),
            java: String::new(),
            go: String::new(),
            lua: String::new(),
            error: Some(error.into()),
        }
    }
}

fn parse_group_storage(storage: Option<String>) -> Option<GroupStorage> {
    storage.as_deref().and_then(|s| match s {
        "tbl" => Some(GroupStorage::Tbl),
        "xlsx" => Some(GroupStorage::Xlsx),
        _ => None,
    })
}

fn sync_summary(s: &tablet_core::excel_sync::SheetSummary) -> String {    format!("{}行 {}列", s.rows, s.cols)
}

fn sync_op_code(op: &tablet_core::excel_sync::SyncOp) -> u32 {
    use tablet_core::excel_sync::{PushScope, SyncOp};
    match op {
        SyncOp::Push { scope: PushScope::MatchedOnly } => 1,
        SyncOp::Pull => 2,
        SyncOp::Push { scope: PushScope::WithTabletColumns } => 3,
        SyncOp::Push { scope: PushScope::WholeTable } => 5,
        SyncOp::CreateFile => 7,
        SyncOp::ImportSheet => 8,
    }
}

fn sync_op_from_code(code: u32) -> Option<tablet_core::excel_sync::SyncOp> {
    use tablet_core::excel_sync::{PushScope, SyncOp};
    Some(match code {
        1 => SyncOp::Push { scope: PushScope::MatchedOnly },
        2 => SyncOp::Pull,
        3 => SyncOp::Push { scope: PushScope::WithTabletColumns },
        5 => SyncOp::Push { scope: PushScope::WholeTable },
        7 => SyncOp::CreateFile,
        8 => SyncOp::ImportSheet,
        _ => return None,
    })
}

impl Backend {
    pub fn load(workdir: &Path) -> anyhow::Result<Self> {
        let (mut engine, _fixed, _failures) = load_workspace(workdir)?;
        engine.revalidate_all_projects();
        let selected = engine.active().map(|p| Selected {
            project: p.schema.meta.id.clone(),
            group: String::new(),
            name: String::new(),
            kind: SelKind::Project,
        });
        Ok(Self {
            workdir: workdir.to_path_buf(),
            engine,
            selected,
            show_ref_name: false,
            excel_sessions: Vec::new(),
        })
    }

    // ── 快照 ────────────────────────────────────────────────
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            target: tablet_core::crash::BUILD_TARGET,
            workdir: self.workdir.display().to_string(),
            vcs: tablet_core::vcs::probe(&self.workdir).title(),
            tree: self.tree(),
            grid: self.grid(),
            status: self.status(),
            logs: self.logs(),
            issues: self.issues(),
        }
    }

    fn issues(&self) -> Vec<IssueRow> {
        if self.engine.active().is_none() {
            return Vec::new();
        }
        self.engine
            .validate_errors()
            .into_iter()
            .map(|(group, name, e)| {
                let severity = e.severity.as_str().to_string();
                let jump = !e.is_schema() && !e.is_table_level();
                IssueRow {
                    group,
                    name,
                    row: e.row,
                    col: e.col,
                    message: e.message,
                    severity,
                    jump,
                }
            })
            .collect()
    }

    fn tree(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        let opened: Vec<String> = self.engine.opened_ids();
        let active = self.engine.active_project_id().map(str::to_string);

        // 已打开的项目（按 engine 打开顺序）
        for p in &self.engine.projects {
            let pid = p.schema.meta.id.clone();
            let pname = if p.schema.meta.name.is_empty() {
                pid.clone()
            } else {
                p.schema.meta.name.clone()
            };
            let (pm, pc) = marker(
                false,
                p.state.is_pending() || p.groups.iter().any(|g| g.is_new),
                p.is_dirty(),
                self.engine.has_project_error(&pid),
            );
            rows.push(TreeRow {
                project: pid.clone(),
                project_name: pname.clone(),
                kind: "project".into(),
                group: None,
                name: None,
                label: pname.clone(),
                search_key: tablet_core::search::search_key(&pname),
                mark: pm,
                mark_color: pc,
                closed: false,
                active: active.as_deref() == Some(pid.as_str()),
                xlsx: false,
            });

            for g in &p.groups {
                let (gm, gc) = marker(
                    false,
                    g.is_new,
                    g.is_dirty(),
                    self.engine.has_group_error(&pid, &g.name),
                );
                rows.push(TreeRow {
                    project: pid.clone(),
                    project_name: pname.clone(),
                    kind: "group".into(),
                    group: Some(g.name.clone()),
                    name: None,
                    label: g.name.clone(),
                    search_key: tablet_core::search::search_key(&g.name),
                    mark: gm,
                    mark_color: gc,
                    closed: false,
                    active: false,
                    xlsx: g.is_xlsx(),
                });

                for t in &g.tables {
                    self.push_node(&mut rows, &pid, &pname, &g.name, &t.name, SelKind::Table,
                        t.deleted, !t.saved, t.dirty);
                }
                for c in &g.constants {
                    self.push_node(&mut rows, &pid, &pname, &g.name, &c.name, SelKind::Constant,
                        c.deleted, !c.saved, c.dirty);
                }
                for e in &g.enums {
                    self.push_node(&mut rows, &pid, &pname, &g.name, &e.name, SelKind::Enum,
                        e.deleted, !e.saved, e.dirty);
                }
            }
        }

        // 未打开的项目：灰态根节点
        for ap in self.engine.available() {
            if opened.iter().any(|id| id == &ap.id) {
                continue;
            }
            rows.push(TreeRow {
                project: ap.id.clone(),
                project_name: ap.name.clone(),
                kind: "project".into(),
                group: None,
                name: None,
                label: ap.name.clone(),
                search_key: tablet_core::search::search_key(&ap.name),
                mark: String::new(),
                mark_color: String::new(),
                closed: true,
                active: false,
                xlsx: false,
            });
        }
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn push_node(
        &self,
        rows: &mut Vec<TreeRow>,
        pid: &str,
        pname: &str,
        group: &str,
        name: &str,
        kind: SelKind,
        deleted: bool,
        new: bool,
        dirty: bool,
    ) {
        let has_err = self.engine.has_node_error(pid, group, name);
        let (m, c) = marker(deleted, new, dirty, has_err);
        rows.push(TreeRow {
            project: pid.to_string(),
            project_name: pname.to_string(),
            kind: match kind {
                SelKind::Table => "table",
                SelKind::Constant => "constant",
                SelKind::Enum => "enum",
                _ => "node",
            }
            .into(),
            group: Some(group.to_string()),
            name: Some(name.to_string()),
            label: name.to_string(),
            search_key: tablet_core::search::search_key(name),
            mark: m,
            mark_color: c,
            closed: false,
            active: false,
            xlsx: false,
        });
    }

    fn status(&self) -> Status {
        let active = self.engine.active();
        let selected_label = match &self.selected {
            Some(s) if !s.name.is_empty() => format!("{} / {}", s.group, s.name),
            Some(s) if !s.group.is_empty() => format!("{}/{}", s.project, s.group),
            Some(s) => s.project.clone(),
            None => String::new(),
        };
        let cb = self.engine.node_clipboard.as_ref();
        let has_node_clipboard = cb.map_or(false, tablet_core::ops::NodeClipboard::is_node);
        let has_group_clipboard = cb.map_or(false, tablet_core::ops::NodeClipboard::is_group);
        let node_clipboard_is_table = matches!(
            cb,
            Some(tablet_core::ops::NodeClipboard::Node {
                body: tablet_core::model::NodeBody::Table(_),
                ..
            })
        );
        let group_clipboard_is_xlsx = matches!(
            cb,
            Some(tablet_core::ops::NodeClipboard::Group { snapshot, .. }) if snapshot.is_xlsx()
        );
        let can_revert = match self.selected_node() {
            Some((g, n, SelKind::Table)) => {
                self.engine.find_table(g, n).map(|t| t.revertable()).unwrap_or(false)
            }
            Some((g, n, SelKind::Constant)) => {
                self.engine.find_constant(g, n).map(|c| c.revertable()).unwrap_or(false)
            }
            Some((g, n, SelKind::Enum)) => {
                self.engine.find_enum(g, n).map(|e| e.revertable()).unwrap_or(false)
            }
            _ => false,
        };
        Status {
            active_project: self.engine.active_project_id().map(str::to_string),
            active_project_name: active.map(|p| p.schema.meta.name.clone()),
            selected_label,
            dirty: self.engine.is_dirty_any(),
            can_undo: self.engine.can_undo(),
            can_redo: self.engine.can_redo(),
            can_revert,
            undo_label: self.engine.undo_label().map(str::to_string),
            redo_label: self.engine.redo_label().map(str::to_string),
            error_count: self.engine.validation_errors.len(),
            warning_count: self.engine.validation_warnings.len(),
            show_ref_name: self.show_ref_name,
            excel_editing: self
                .excel_sessions
                .iter()
                .filter(|s| Some(s.project.as_str()) == self.engine.active_project_id())
                .map(|s| s.group.clone())
                .collect(),
            has_node_clipboard,
            has_group_clipboard,
            node_clipboard_is_table,
            group_clipboard_is_xlsx,
        }
    }

    fn logs(&self) -> Vec<LogRow> {
        self.engine
            .logs()
            .iter()
            .rev()
            .take(200)
            .map(|l| LogRow {
                time: l.time.clone(),
                level: l.level.as_str().to_string(),
                message: l.message.clone(),
            })
            .collect()
    }

    // ── 选择 ────────────────────────────────────────────────
    pub fn select(&mut self, project: &str, kind: &str, group: Option<&str>, name: Option<&str>) {
        if !self.engine.is_opened(project) {
            let _ = self.engine.open_project(project);
        }
        let _ = self.engine.set_active_by_id(project);

        self.selected = match kind {
            "project" => Some(Selected {
                project: project.into(),
                group: String::new(),
                name: String::new(),
                kind: SelKind::Project,
            }),
            "group" => Some(Selected {
                project: project.into(),
                group: group.unwrap_or_default().into(),
                name: String::new(),
                kind: SelKind::Group,
            }),
            k => {
                let sel = Selected {
                    project: project.into(),
                    group: group.unwrap_or_default().into(),
                    name: name.unwrap_or_default().into(),
                    kind: match k {
                        "constant" => SelKind::Constant,
                        "enum" => SelKind::Enum,
                        _ => SelKind::Table,
                    },
                };
                if !sel.name.is_empty() {
                    self.engine.revalidate(&sel.group, &sel.name);
                }
                Some(sel)
            }
        };
    }

    // ── 表格投影 ────────────────────────────────────────────
    fn grid(&self) -> Option<GridPayload> {
        let sel = self.selected.as_ref()?;
        match sel.kind {
            SelKind::Table => self.table_grid(&sel.group, &sel.name),
            SelKind::Constant => self.constant_grid(&sel.group, &sel.name),
            SelKind::Enum => self.enum_grid(&sel.group, &sel.name),
            _ => None,
        }
    }

    fn table_grid(&self, group: &str, name: &str) -> Option<GridPayload> {
        let table = self.engine.find_table(group, name)?;
        let fields = &table.schema.fields;
        let valid_count = valid_table_rows(table);

        let desc: Vec<String> = fields.iter().map(|f| one_line(&f.desc)).collect();
        let export: Vec<String> = fields.iter().map(|f| f.export.display().to_string()).collect();
        let types: Vec<String> = fields.iter().map(|f| one_line(&f.tbl_type)).collect();
        let names: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();

        let mut kinds = Vec::with_capacity(fields.len());
        for (i, f) in fields.iter().enumerate() {
            kinds.push(if i == 0 {
                "text".to_string() // id 列数据可编辑（输入新行），表头才是只读
            } else if is_ref_type(&f.tbl_type) {
                "ref".to_string()
            } else {
                "text".to_string()
            });
        }

        let header_kinds = vec![
            vec!["text".to_string(); fields.len()],
            kinds
                .iter()
                .enumerate()
                .map(|(i, _)| if i == 0 { "readonly" } else { "export" }.to_string())
                .collect(),
            fields
                .iter()
                .enumerate()
                .map(|(i, _)| if i == 0 { "readonly" } else { "type" }.to_string())
                .collect(),
            fields
                .iter()
                .enumerate()
                .map(|(i, _)| if i == 0 { "readonly" } else { "text" }.to_string())
                .collect(),
        ];

        let mut rows = Vec::with_capacity(valid_count + EXTRA_ROWS);
        let mut errors = Vec::new();
        let mut ref_cache = std::collections::HashMap::new();
        for r in 0..valid_count + EXTRA_ROWS {
            let mut row = Vec::with_capacity(fields.len());
            for c in 0..fields.len() {
                let text = table.records.get(r).and_then(|rec| rec.get(c)).cloned().unwrap_or_default();
                if self.engine.has_active_cell_error(group, name, r, c) {
                    errors.push([r, c]);
                }
                let display = if self.show_ref_name {
                    self.display_ref_value(&fields[c].tbl_type, &text, &mut ref_cache)
                } else {
                    text
                };
                row.push(display);
            }
            rows.push(row);
        }

        let display_options = fields
            .iter()
            .filter(|f| matches!(f.tbl_type.trim(), "str" | "int" | "txt"))
            .map(|f| f.name.clone())
            .collect();

        let mut ref_cols = Vec::with_capacity(fields.len());
        let mut ref_list = Vec::with_capacity(fields.len());
        for f in fields {
            match ref_target_of(&f.tbl_type) {
                Some((target, multi)) => {
                    ref_cols.push(Some(target));
                    ref_list.push(multi);
                }
                None => {
                    ref_cols.push(None);
                    ref_list.push(false);
                }
            }
        }

        Some(GridPayload {
            mode: "table".into(),
            title: format!("📊 {group} / {name}"),
            subtitle: format!("Table · {valid_count} 行 · {} 列", fields.len()),
            headers: vec![desc, export, types, names],
            header_kinds,
            kinds,
            rows,
            errors,
            data_count: valid_count,
            editable_headers: true,
            ref_targets: self.ref_target_names(),
            display_field: table.display_field.clone(),
            display_options,
            picker_header_single: self.picker_flags().0,
            picker_data_single: self.picker_flags().1,
            ref_cols,
            ref_list,
        })
    }

    fn constant_grid(&self, group: &str, name: &str) -> Option<GridPayload> {
        let constant = self.engine.find_constant(group, name)?;
        let valid_count = valid_constant_rows(constant);
        let mut rows = Vec::with_capacity(valid_count + EXTRA_ROWS);
        let mut errors = Vec::new();
        let mut ref_cache = std::collections::HashMap::new();
        for r in 0..valid_count + EXTRA_ROWS {
            let entry = constant.entries.get(r);
            if entry.is_some() && self.engine.has_active_cell_error(group, name, r, 2) {
                errors.push([r, 2]);
            }
            rows.push(match entry {
                Some(e) => {
                    let value_display = if self.show_ref_name {
                        self.display_ref_value(&e.tbl_type, &e.value, &mut ref_cache)
                    } else {
                        e.value.clone()
                    };
                    vec![
                        e.name.clone(),
                        e.tbl_type.clone(),
                        value_display,
                        e.export.display().to_string(),
                        e.desc.clone(),
                    ]
                }
                None => vec![String::new(); 5],
            });
        }
        Some(GridPayload {
            mode: "constant".into(),
            title: format!("📋 {group} / {name}"),
            subtitle: format!("Constant · {valid_count} 项"),
            headers: vec![
                vec!["name".into(), "type".into(), "value".into(), "export".into(), "desc".into()],
            ],
            header_kinds: vec![vec!["readonly".to_string(); 5]],
            kinds: vec!["text".into(), "type".into(), "text".into(), "export".into(), "text".into()],
            rows,
            errors,
            data_count: valid_count,
            editable_headers: false,
            ref_targets: self.ref_target_names(),
            display_field: None,
            display_options: Vec::new(),
            picker_header_single: self.picker_flags().0,
            picker_data_single: self.picker_flags().1,
            ref_cols: Vec::new(),
            ref_list: Vec::new(),
        })
    }

    fn enum_grid(&self, group: &str, name: &str) -> Option<GridPayload> {
        let e = self.engine.find_enum(group, name)?;
        let valid_count = e.entries.iter().filter(|x| !x.id.is_empty() || !x.name.is_empty()).count();
        let mut rows = Vec::with_capacity(valid_count + EXTRA_ROWS);
        let mut errors = Vec::new();
        for r in 0..valid_count + EXTRA_ROWS {
            let entry = e.entries.get(r);
            if entry.is_some() && self.engine.has_active_cell_error(group, name, r, 0) {
                errors.push([r, 0]);
            }
            rows.push(match entry {
                Some(x) => vec![x.id.clone(), x.name.clone(), x.desc.clone()],
                None => vec![String::new(); 3],
            });
        }
        Some(GridPayload {
            mode: "enum".into(),
            title: format!("🔢 {group} / {name}"),
            subtitle: format!("Enum · {valid_count} 项"),
            headers: vec![vec!["id".into(), "name".into(), "desc".into()]],
            header_kinds: vec![vec!["readonly".to_string(); 3]],
            kinds: vec!["text".into(), "text".into(), "text".into()],
            rows,
            errors,
            data_count: valid_count,
            editable_headers: false,
            ref_targets: self.ref_target_names(),
            display_field: e.display_field.clone(),
            display_options: vec!["name".into(), "desc".into()],
            picker_header_single: self.picker_flags().0,
            picker_data_single: self.picker_flags().1,
            ref_cols: Vec::new(),
            ref_list: Vec::new(),
        })
    }

    // ── 写入 ────────────────────────────────────────────────
    fn selected_node(&self) -> Option<(&str, &str, SelKind)> {
        let s = self.selected.as_ref()?;
        if s.name.is_empty() {
            return None;
        }
        Some((s.group.as_str(), s.name.as_str(), s.kind))
    }

    pub fn commit_cell(&mut self, row: usize, col: usize, value: String) {
        let Some((group, name, kind)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        match kind {
            SelKind::Table => self.engine.commit_table_cell(&group, &name, row, col, value),
            SelKind::Constant => self.engine.commit_constant_cell(&group, &name, row, col, value),
            SelKind::Enum => self.engine.commit_enum_cell(&group, &name, row, col, value),
            _ => {}
        }
        self.engine.revalidate(&group, &name);
    }

    pub fn commit_header(&mut self, header_row: usize, col: usize, value: String) {
        let Some((group, name, SelKind::Table)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        self.engine.commit_header_edit(&group, &name, header_row, col, value);
        self.engine.revalidate(&group, &name);
    }

    pub fn clear_cells(&mut self, cells: Vec<(usize, usize)>) {
        let Some((group, name, kind)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        match kind {
            SelKind::Table => self.engine.clear_table_cells(&group, &name, &cells),
            SelKind::Constant => self.engine.clear_constant_cells(&group, &name, &cells),
            SelKind::Enum => self.engine.clear_enum_cells(&group, &name, &cells),
            _ => {}
        }
        self.engine.revalidate(&group, &name);
    }

    pub fn paste_cells(&mut self, row: usize, col: usize, grid: Vec<Vec<String>>) {
        let Some((group, name, kind)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        match kind {
            SelKind::Table => self.engine.paste_table_data(&group, &name, row, col, &grid),
            SelKind::Constant => self.engine.paste_constant_data(&group, &name, row, col, &grid),
            SelKind::Enum => self.engine.paste_enum_data(&group, &name, row, col, &grid),
            _ => {}
        }
        self.engine.revalidate(&group, &name);
    }

    pub fn insert_row(&mut self, at: usize) {
        let Some((group, name, kind)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        match kind {
            SelKind::Table => self.engine.insert_row(&group, &name, at),
            SelKind::Constant => self.engine.insert_constant_row(&group, &name, at),
            SelKind::Enum => self.engine.insert_enum_row(&group, &name, at),
            _ => {}
        }
        self.engine.revalidate(&group, &name);
    }

    pub fn delete_rows(&mut self, start: usize, end: usize) {
        let Some((group, name, kind)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        match kind {
            SelKind::Table => self.engine.delete_rows(&group, &name, start, end + 1),
            SelKind::Constant => {
                for _ in start..=end {
                    self.engine.delete_constant_row(&group, &name, start);
                }
            }
            SelKind::Enum => self.engine.delete_enum_rows(&group, &name, start, end + 1),
            _ => {}
        }
        self.engine.revalidate(&group, &name);
    }

    pub fn insert_column(&mut self, at: usize) {
        let Some((group, name, SelKind::Table)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        self.engine.insert_column(&group, &name, at);
        self.engine.revalidate(&group, &name);
    }

    pub fn delete_column(&mut self, at: usize) {
        let Some((group, name, SelKind::Table)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        self.engine.delete_column(&group, &name, at);
        self.engine.revalidate(&group, &name);
    }

    // ── 全局设置 / 导出执行 ─────────────────────────────────
    pub fn global_settings(&self) -> GlobalSettingsUi {
        let ui = self.engine.global_config.ui.clone().unwrap_or_default();
        let def = SeparatorsSection::default();
        let separators = SepKey::ALL
            .iter()
            .map(|k| SepItem {
                key: k.as_directive_key().to_string(),
                value: k.get(&self.engine.global_config.separators).to_string(),
                default: k.get(&def).to_string(),
                original: k.get(&self.engine.global_config.separators).to_string(),
            })
            .collect();
        GlobalSettingsUi {
            auto_commit_on_blur: ui.auto_commit_on_blur,
            realtime_validate: ui.realtime_validate,
            log_level: ui.log_level.unwrap_or_default().to_string(),
            picker_trigger_header: trigger_str(ui.picker_trigger_header).to_string(),
            picker_trigger_data: trigger_str(ui.picker_trigger_data).to_string(),
            show_meta_id: ui.show_meta_id,
            constant_ref_allowed: ui.constant_ref_allowed,
            ref_picker_strategy: ui.ref_picker.default_strategy.as_str().to_string(),
            separators,
            export_settings: export_ui(
                self.engine
                    .global_config
                    .export
                    .as_ref()
                    .unwrap_or(&ExportConfig::default()),
            ),
            default_export: export_ui(&ExportConfig::default()),
        }
    }

    pub fn save_global_settings(&mut self, s: GlobalSettingsUi) -> Result<(), String> {
        let ui = self.engine.global_config.ui.get_or_insert_with(UiConfig::default);
        ui.auto_commit_on_blur = s.auto_commit_on_blur;
        ui.realtime_validate = s.realtime_validate;
        ui.log_level = Some(parse_log_level(&s.log_level));
        ui.picker_trigger_header = parse_trigger(&s.picker_trigger_header);
        ui.picker_trigger_data = parse_trigger(&s.picker_trigger_data);
        ui.show_meta_id = s.show_meta_id;
        ui.constant_ref_allowed = s.constant_ref_allowed;
        if let Ok(strategy) =
            s.ref_picker_strategy.parse::<tablet_core::enums::RefPickerStrategy>()
        {
            ui.ref_picker.default_strategy = strategy;
        }
        for item in &s.separators {
            if let Some(k) = SepKey::from_directive_key(&item.key) {
                k.set(&mut self.engine.global_config.separators, item.value.clone());
            }
        }
        let export = self
            .engine
            .global_config
            .export
            .get_or_insert_with(ExportConfig::default);
        apply_export(export, &s.export_settings);
        let pm = self.engine.global_config.project_management.clone();
        self.engine
            .persist_global_config(pm.project_sort, &pm.project_order)
            .map_err(|e| e.to_string())
    }

    pub fn export_project(&mut self, f: ExportFormatsUi) -> ExportReport {
        let formats = ExportFormats {
            json: f.json,
            xml: f.xml,
            java: f.java,
            go: f.go,
            cpp: f.cpp,
            csharp_dotnet: f.csharp_dotnet,
            csharp_unity: f.csharp_unity,
            csharp_godot: f.csharp_godot,
            lua: f.lua,
            gdscript: f.gdscript,
            typescript_client: f.typescript_client,
            typescript_server: f.typescript_server,
        };
        let summary = run_export(&mut self.engine, formats);
        let mut lines = Vec::new();
        let mut ok = true;
        for (name, outcome) in summary.per_format {
            match outcome {
                FormatOutcome::Ok(_) => lines.push(format!("{name}: 导出完成")),
                FormatOutcome::Err(e) => {
                    ok = false;
                    lines.push(format!("{name}: 失败 - {e}"));
                }
            }
        }
        if lines.is_empty() {
            lines.push("没有选中任何导出格式".to_string());
        }
        ExportReport { ok, lines }
    }

    // ── Schema 导入 / 导出 ──────────────────────────────────
    pub fn schema_export(
        &self,
        path: String,
        with_preset: bool,
        groups: Vec<String>,
        nodes: Vec<String>,
        meta_id: Option<String>,
        meta_name: Option<String>,
        meta_category: Option<String>,
        meta_version: Option<String>,
        as_template: bool,
    ) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let out = if as_template {
            None
        } else {
            Some(resolve_in_project(&project.project_root, &path))
        };
        let opts = SchemaExportOptions {
            out,
            as_template,
            groups: groups.into_iter().filter(|g| !g.trim().is_empty()).collect(),
            nodes: nodes.into_iter().filter(|n| !n.trim().is_empty()).collect(),
            with_preset,
            meta_id,
            meta_name,
            meta_category,
            meta_version,
        };
        let outcome = run_schema_export(&self.engine, &opts).map_err(|e| e.to_string())?;
        Ok(format!(
            "已{} {} 个段 → {}",
            if outcome.template { "保存为本地模板" } else { "导出" },
            outcome.sections,
            outcome.path
        ))
    }

    /// 当前项目的组 / 节点清单，供 Schema 对话框的勾选树使用。
    pub fn schema_outline(&self) -> Result<Vec<SchemaOutlineGroup>, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let mut out = Vec::new();
        for g in &project.groups {
            let mut nodes = Vec::new();
            for t in &g.tables {
                if !t.deleted {
                    nodes.push(SchemaOutlineNode { name: t.name.clone(), kind: "table".into() });
                }
            }
            for c in &g.constants {
                if !c.deleted {
                    nodes.push(SchemaOutlineNode { name: c.name.clone(), kind: "constant".into() });
                }
            }
            for e in &g.enums {
                if !e.deleted {
                    nodes.push(SchemaOutlineNode { name: e.name.clone(), kind: "enum".into() });
                }
            }
            if !nodes.is_empty() {
                out.push(SchemaOutlineGroup { group: g.name.clone(), nodes });
            }
        }
        Ok(out)
    }

    /// 解析待导入的 .tblschema：段清单 + 是否与当前项目冲突 + 预设信息。
    pub fn schema_import_preview(&self, path: &str) -> Result<SchemaImportPreview, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, path);
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("读取 {} 失败: {e}", file.display()))?;
        let schema = parse_tblschema(&text).map_err(|e| e.to_string())?;
        let mut existing = std::collections::HashSet::new();
        for g in &project.groups {
            for t in &g.tables {
                if !t.deleted {
                    existing.insert((g.name.clone(), t.name.clone()));
                }
            }
            for c in &g.constants {
                if !c.deleted {
                    existing.insert((g.name.clone(), c.name.clone()));
                }
            }
            for e in &g.enums {
                if !e.deleted {
                    existing.insert((g.name.clone(), e.name.clone()));
                }
            }
        }
        let sections = schema
            .sections
            .iter()
            .map(|s| SchemaPreviewNode {
                group: s.group.clone(),
                name: s.name.clone(),
                mode: match s.mode {
                    tablet_core::schema_model::SchemaMode::Table => "table",
                    tablet_core::schema_model::SchemaMode::Constant => "constant",
                    tablet_core::schema_model::SchemaMode::Enum => "enum",
                }
                .to_string(),
                exists: existing.contains(&(s.group.clone(), s.name.clone())),
            })
            .collect();
        Ok(SchemaImportPreview {
            path: path.to_string(),
            has_preset: schema.meta.has_preset,
            meta_id: schema.meta.id,
            meta_name: schema.meta.name,
            meta_category: schema.meta.category,
            meta_version: schema.meta.version,
            sections,
        })
    }

    /// sections：`Group/Name` 列表；空 = 全部。
    pub fn schema_import(
        &mut self,
        path: String,
        with_preset: bool,
        sections: Option<Vec<String>>,
    ) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, &path);
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("读取 {} 失败: {e}", file.display()))?;
        let mut schema = parse_tblschema(&text).map_err(|e| e.to_string())?;
        if let Some(wanted) = sections {
            let wanted: std::collections::HashSet<String> =
                wanted.into_iter().filter(|s| !s.trim().is_empty()).collect();
            if !wanted.is_empty() {
                schema.sections.retain(|s| wanted.contains(&format!("{}/{}", s.group, s.name)));
            }
        }
        let sections = schema.sections.clone();
        if sections.is_empty() {
            return Err("没有选择要导入的段".to_string());
        }
        let config_dir = project.data_dir();
        let result = self.engine.edit_active_project("导入 Schema", |project| {
            let r = apply_schema_to_project(&mut project.groups, &sections, &config_dir, with_preset);
            if r.is_ok() {
                project.resync_schema();
            }
            r
        });
        match result {
            Some(Ok((added, overwritten))) => {
                self.engine.revalidate_all_projects();
                Ok(format!("导入完成：{added} 新增，{overwritten} 覆盖"))
            }
            Some(Err(e)) => Err(format!("导入失败: {e}")),
            None => Err("没有活跃的 Project".to_string()),
        }
    }

    // ── Excel 导出 / 导入 ───────────────────────────────────
    pub fn excel_export(&self, group: &str, path: String) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let out = resolve_in_project(&project.project_root, &path);
        let target = ExcelTarget::Group {
            name: group.to_string(),
            include: Vec::new(),
        };
        let summary = run_excel_export(&self.engine, target, Some(&out)).map_err(|e| e.to_string())?;
        Ok(format!(
            "已导出 {}（{} 字节）",
            summary.output.display(),
            summary.bytes
        ))
    }

    pub fn excel_import(&mut self, group: &str, path: String) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, &path);
        let summary =
            run_excel_import(&mut self.engine, group, &file).map_err(|e| e.to_string())?;
        Ok(format!(
            "导入完成：{}，修补 {} 个节点",
            summary.group, summary.nodes_patched
        ))
    }

    /// `.excel/` 下的工作簿列表（GUI 导出 Excel 的默认落点）。
    pub fn excel_files(&self) -> Vec<String> {
        let Some(project) = self.engine.active() else { return Vec::new() };
        let dir = project.project_root.join(".excel");
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut out: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension().map_or(false, |x| x.eq_ignore_ascii_case("xlsx")) {
                    Some(p.file_name()?.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();
        out.sort();
        out
    }

    pub fn excel_dir(&self) -> Option<std::path::PathBuf> {
        let project = self.engine.active()?;
        Some(project.project_root.join(".excel"))
    }

    /// 删除 `.excel/<file>`（文件浏览器「删除」）。
    pub fn excel_delete(&self, file: &str) -> Result<(), String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let name = std::path::Path::new(file)
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("非法文件名")?;
        let path = project.project_root.join(".excel").join(name);
        std::fs::remove_file(&path)
            .map_err(|e| format!("删除 {} 失败: {e}", path.display()))
    }

    /// Excel 同步面板：按 core `plan_group` 算出每一行（组/节点 + 两侧尺寸 + 可用操作）。
    pub fn excel_sync_plan(&self) -> Vec<ExcelSyncRowUi> {
        use tablet_core::excel_sync;
        let Some(project) = self.engine.active() else { return Vec::new() };
        let excel_dir = project.project_root.join(".excel");
        let mut books: std::collections::HashMap<String, std::path::PathBuf> =
            std::collections::HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&excel_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |e| e.eq_ignore_ascii_case("xlsx")) {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        books.insert(stem.to_string(), p);
                    }
                }
            }
        }
        let mut rows = Vec::new();
        for group in &project.groups {
            // xlsx 组本身就是工作簿，没有「两份拷贝要同步」这回事。
            if group.is_xlsx() {
                continue;
            }
            let sheets = books
                .get(&group.name)
                .and_then(|p| excel_sync::read_xlsx_sheets(p, Some(group)).ok());
            let plan = excel_sync::plan_group(group, sheets.as_deref());
            let outside: Vec<String> = sheets
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter(|s| s.has_content_outside_region())
                .map(|s| s.name.clone())
                .collect();
            for pair in plan.pairs {
                rows.push(ExcelSyncRowUi {
                    group: group.name.clone(),
                    node: pair.node.clone(),
                    left: pair.left.as_ref().map(sync_summary),
                    right: pair.right.as_ref().map(sync_summary),
                    ops: pair
                        .ops
                        .iter()
                        .map(|op| ExcelSyncOpUi {
                            code: sync_op_code(op),
                            label: op.label().to_string(),
                        })
                        .collect(),
                    blocked: pair.blocked_reason.clone(),
                    desc_diff: pair.desc_diff,
                    header_edited: pair.header_edited,
                    outside: outside.iter().any(|n| *n == pair.node),
                });
            }
        }
        rows
    }

    /// 执行一次同步操作。core 会重算计划校验「操作是否仍然合法」，过期直接拒绝。
    pub fn excel_sync_action(&mut self, group: &str, node: &str, code: u32) -> Result<(), String> {
        let op = sync_op_from_code(code).ok_or("未知同步操作")?;
        let pid = self.engine.active_project_id().ok_or("无活动项目")?.to_string();
        let excel_dir = self
            .engine
            .find_project(&pid)
            .ok_or("找不到项目")?
            .project_root
            .join(".excel");
        std::fs::create_dir_all(&excel_dir).map_err(|e| e.to_string())?;
        let path = excel_dir.join(format!("{group}.xlsx"));
        let msg = {
            let project = self.engine.find_project_mut(&pid).ok_or("找不到项目")?;
            let g = project
                .groups
                .iter_mut()
                .find(|g| g.name == group)
                .ok_or("找不到组")?;
            tablet_core::excel_sync::execute(&path, g, node, &op).map_err(|e| e.to_string())?
        };
        // 同步同时改了内存与工作簿，属于外部基线变化：旧撤销栈不再成立。
        self.engine.clear_history(&pid);
        self.engine.revalidate(group, node);
        self.engine.ui_log(format!("[同步] {msg}"));
        Ok(())
    }

    pub fn excel_preview(
        &self,
        path: String,
        sheet: Option<String>,
        header_row: Option<u32>,
    ) -> Result<ExcelPreview, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, &path);
        let sheets = tablet_core::excel_mapping::list_sheets(&file).map_err(|e| e.to_string())?;
        if sheets.is_empty() {
            return Err("工作簿没有 sheet".to_string());
        }
        let sheet = sheet
            .filter(|s| sheets.iter().any(|x| x == s))
            .unwrap_or_else(|| sheets[0].clone());
        let hr = header_row.unwrap_or_else(|| {
            tablet_core::excel_mapping::detect_header_row(&file, &sheet).unwrap_or(1)
        });
        let data_start = hr + 1;
        let preview =
            tablet_core::excel_mapping::read_sheet_preview(&file, &sheet, hr, data_start, 5)
                .map_err(|e| e.to_string())?;
        let fields = tablet_core::excel_mapping::suggest_fields(&preview)
            .into_iter()
            .map(|f| MappedFieldUi {
                target_name: f.target_name,
                source_col: f.source_col,
                tbl_type: f.tbl_type,
                export: f.export.display().to_string(),
                desc: f.desc,
            })
            .collect();
        Ok(ExcelPreview {
            sheets,
            sheet,
            header_row: hr,
            data_start,
            columns: preview.columns,
            fields,
            sample_rows: preview.rows,
        })
    }

    pub fn excel_import_mapped(&mut self, req: ExcelMapRequest) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let pid = project.schema.meta.id.clone();
        let file = resolve_in_project(&project.project_root, &req.path);
        let fields = req
            .fields
            .into_iter()
            .map(|f| tablet_core::excel_mapping::MappedField {
                target_name: f.target_name,
                source_col: f.source_col,
                tbl_type: f.tbl_type,
                export: tablet_core::model::Export::from_input(&f.export),
                desc: f.desc,
            })
            .collect();
        let plan = tablet_core::excel_mapping::MappingPlan {
            source: file,
            sheet: req.sheet,
            header_row: req.header_row,
            data_start: req.data_start,
            desc_row: 0,
            type_row: 0,
            export_row: 0,
            group_name: req.group_name,
            node_name: req.node_name,
            fields,
        };
        tablet_core::excel_mapping::validate_plan(&plan).map_err(|e| e.to_string())?;
        let outcome = self.engine.import_mapped_excel(&pid, &plan)?;
        self.engine.revalidate_all_projects();
        Ok(format!(
            "已导入 {}/{}（{} 个节点）",
            outcome.group, plan.node_name, outcome.nodes
        ))
    }

    /// 开始 Excel 编辑：导出临时工作簿 → 用 Excel 打开 → 记录 mtime 供轮询。
    pub fn excel_begin_edit(
        &mut self,
        project: &str,
        group: &str,
        open: bool,
    ) -> Result<String, String> {
        let root = self
            .engine
            .find_project(project)
            .map(|p| p.project_root.clone())
            .ok_or_else(|| format!("项目不存在: {project}"))?;
        let out = root.join(".excel").join(format!("{group}.xlsx"));
        let target = ExcelTarget::Group {
            name: group.to_string(),
            include: Vec::new(),
        };
        run_excel_export(&self.engine, target, Some(&out)).map_err(|e| e.to_string())?;
        let mtime = std::fs::metadata(&out).and_then(|m| m.modified()).ok();
        self.excel_sessions
            .retain(|s| !(s.project == project && s.group == group));
        self.excel_sessions.push(ExcelSession {
            project: project.to_string(),
            group: group.to_string(),
            path: out.clone(),
            mtime,
        });
        if open {
            launch(&out.to_string_lossy())?;
        }
        Ok(out.display().to_string())
    }

    /// 轮询哪些会话的工作簿被 Excel 改过（mtime 变化）。
    pub fn excel_poll(&self) -> Vec<ExcelPollItem> {
        let mut out = Vec::new();
        for s in &self.excel_sessions {
            let now = std::fs::metadata(&s.path).and_then(|m| m.modified()).ok();
            if now != s.mtime {
                out.push(ExcelPollItem {
                    project: s.project.clone(),
                    group: s.group.clone(),
                    path: s.path.display().to_string(),
                });
            }
        }
        out
    }

    /// 把 Excel 的改动导入回来，并更新 mtime（失败则保持旧 mtime，下一轮重试）。
    pub fn excel_sync(&mut self, project: &str, group: &str) -> Result<String, String> {
        let Some(idx) = self
            .excel_sessions
            .iter()
            .position(|s| s.project == project && s.group == group)
        else {
            return Err("没有该 Group 的 Excel 会话".to_string());
        };
        let path = self.excel_sessions[idx].path.clone();
        let summary =
            run_excel_import(&mut self.engine, group, &path).map_err(|e| e.to_string())?;
        self.excel_sessions[idx].mtime =
            std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        Ok(format!(
            "Excel 已回写：{}（{} 个节点）",
            summary.group, summary.nodes_patched
        ))
    }

    pub fn excel_end_edit(&mut self, project: &str, group: &str) {
        let changed = self.excel_sessions.iter().any(|s| {
            s.project == project
                && s.group == group
                && std::fs::metadata(&s.path).and_then(|m| m.modified()).ok() != s.mtime
        });
        if changed {
            let _ = self.excel_sync(project, group);
        }
        self.excel_sessions
            .retain(|s| !(s.project == project && s.group == group));
    }

    pub fn open_folder(&self, path: Option<String>) -> Result<(), String> {
        let target = match path {
            Some(p) => {
                let root = self
                    .engine
                    .active()
                    .map(|p| p.project_root.clone())
                    .unwrap_or_else(|| self.workdir.clone());
                resolve_in_project(&root, &p)
            }
            None => self.excel_dir().ok_or("没有活跃的 Project")?,
        };
        let _ = std::fs::create_dir_all(&target);
        launch(&target.to_string_lossy())
    }

    pub fn open_url(&self, url: &str) -> Result<(), String> {
        launch(url)
    }

    // ── 引用显示 / 展示列 ───────────────────────────────────
    pub fn set_show_ref_name(&mut self, on: bool) {
        self.show_ref_name = on;
    }

    pub fn set_display_field_selected(&mut self, field: Option<String>) {
        let Some(sel) = self.selected.as_ref() else { return };
        let kind = match sel.kind {
            SelKind::Table => NodeKind::Table,
            SelKind::Enum => NodeKind::Enum,
            _ => return,
        };
        if sel.name.is_empty() {
            return;
        }
        let (project, group, name) = (sel.project.clone(), sel.group.clone(), sel.name.clone());
        let _ = self.engine.set_active_by_id(&project);
        if self.engine.set_display_field(&group, &name, kind, field) {
            self.engine.revalidate(&group, &name);
        }
    }

    fn picker_flags(&self) -> (bool, bool) {
        let ui = self.engine.global_config.ui.as_ref();
        let header = ui.map_or(true, |u| u.picker_trigger_header == PickerTrigger::Single);
        let data = ui.map_or(false, |u| u.picker_trigger_data == PickerTrigger::Single);
        (header, data)
    }

    fn ref_target_names(&self) -> Vec<String> {        let Some(p) = self.engine.active() else { return Vec::new() };
        let mut out = Vec::new();
        for g in &p.groups {
            out.extend(g.tables.iter().map(|t| t.name.clone()));
            out.extend(g.enums.iter().map(|e| e.name.clone()));
        }
        out
    }

    /// 目标节点的 id → 展示值。`None` = 该节点没有设置展示列（显示原始 id）。
    fn ref_display_map(&self, target: &str) -> Option<std::collections::HashMap<String, String>> {
        let p = self.engine.active()?;
        for g in &p.groups {
            if let Some(t) = g.tables.iter().find(|t| t.name == target) {
                let field = t.display_field.clone()?;
                let col = t.schema.fields.iter().position(|f| f.name == field)?;
                let mut m = std::collections::HashMap::new();
                for r in &t.records {
                    if let Some(id) = r.first() {
                        if !id.is_empty() {
                            m.insert(id.clone(), r.get(col).cloned().unwrap_or_default());
                        }
                    }
                }
                return Some(m);
            }
            if let Some(e) = g.enums.iter().find(|e| e.name == target) {
                let field = e.display_field.clone().unwrap_or_else(|| "name".to_string());
                let col = match field.as_str() {
                    "id" => 0,
                    "desc" => 2,
                    _ => 1,
                };
                let mut m = std::collections::HashMap::new();
                for x in &e.entries {
                    if x.id.is_empty() {
                        continue;
                    }
                    let v = match col {
                        0 => x.id.clone(),
                        2 => x.desc.clone(),
                        _ => x.name.clone(),
                    };
                    m.insert(x.id.clone(), v);
                }
                return Some(m);
            }
        }
        None
    }

    /// 引用选择器的候选：目标节点的全部记录（id + 展示值）。
    /// 引用选择器数据。strategy = auto / full；不传时用全局默认（[ui.ref_picker]）。
    pub fn ref_picker_data(&self, target: &str, strategy: Option<&str>) -> Result<RefPickerData, String> {
        let p = self.engine.active().ok_or("没有活跃的 Project")?;
        let default_strategy = self
            .engine
            .global_config
            .ui
            .as_ref()
            .map(|u| u.ref_picker.default_strategy.as_str().to_string())
            .unwrap_or_else(|| "auto".to_string());
        let strat = strategy
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| s == "auto" || s == "full")
            .unwrap_or_else(|| default_strategy.clone());

        for g in &p.groups {
            if let Some(t) = g.tables.iter().find(|t| !t.deleted && t.name == target) {
                let id_idx = t.schema.fields.iter().position(|f| f.name == "id");
                let Some(id_idx) = id_idx else {
                    return Ok(RefPickerData {
                        target: target.to_string(),
                        is_table: true,
                        target_missing: false,
                        default_strategy: strat,
                        id_header: RefHeaderItem { field: "id".into(), desc: String::new() },
                        headers: Vec::new(),
                        has_desc: false,
                        rows: Vec::new(),
                    });
                };
                let id_field = &t.schema.fields[id_idx];
                let extras_idx: Vec<usize> = if strat == "full" {
                    // 全部：除 id 外所有导出列
                    t.schema.fields.iter().enumerate()
                        .filter(|(_, f)| f.name != "id" && !matches!(f.export, Export::None))
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    // 智能：跳过引用列 / 复合类型 / 不导出的列，最多取 2 个辅助列
                    let mut picked = Vec::new();
                    for (i, f) in t.schema.fields.iter().enumerate() {
                        if f.name == "id" || matches!(f.export, Export::None) { continue; }
                        let ty = f.tbl_type.trim();
                        if ty.starts_with('@') || ty.contains('<') { continue; }
                        picked.push(i);
                        if picked.len() >= 2 { break; }
                    }
                    picked
                };
                let headers: Vec<RefHeaderItem> = extras_idx
                    .iter()
                    .map(|&i| RefHeaderItem {
                        field: t.schema.fields[i].name.clone(),
                        desc: t.schema.fields[i].desc.clone(),
                    })
                    .collect();
                let has_desc = !id_field.desc.is_empty() || headers.iter().any(|h| !h.desc.is_empty());
                let rows: Vec<RefRowItem> = t
                    .records
                    .iter()
                    .filter_map(|r| {
                        let id = r.get(id_idx)?.clone();
                        if id.is_empty() { return None; }
                        Some(RefRowItem {
                            id,
                            extras: extras_idx
                                .iter()
                                .map(|&i| r.get(i).cloned().unwrap_or_default())
                                .collect(),
                        })
                    })
                    .collect();
                return Ok(RefPickerData {
                    target: target.to_string(),
                    is_table: true,
                    target_missing: false,
                    default_strategy: strat,
                    id_header: RefHeaderItem {
                        field: id_field.name.clone(),
                        desc: id_field.desc.clone(),
                    },
                    headers,
                    has_desc,
                    rows,
                });
            }
            if let Some(e) = g.enums.iter().find(|e| !e.deleted && e.name == target) {
                let rows: Vec<RefRowItem> = e
                    .entries
                    .iter()
                    .filter(|x| !x.id.is_empty())
                    .map(|x| RefRowItem {
                        id: x.id.clone(),
                        extras: vec![x.name.clone(), x.desc.clone()],
                    })
                    .collect();
                return Ok(RefPickerData {
                    target: target.to_string(),
                    is_table: false,
                    target_missing: false,
                    default_strategy: strat,
                    id_header: RefHeaderItem { field: "id".into(), desc: "ID".into() },
                    headers: vec![
                        RefHeaderItem { field: "name".into(), desc: "名称".into() },
                        RefHeaderItem { field: "desc".into(), desc: "描述".into() },
                    ],
                    has_desc: true,
                    rows,
                });
            }
        }
        // 目标不存在：不报错，选择器里显示警告并禁用确定（与 Slint 版一致）。
        Ok(RefPickerData {
            target: target.to_string(),
            is_table: true,
            target_missing: true,
            default_strategy: strat,
            id_header: RefHeaderItem { field: "id".into(), desc: String::new() },
            headers: Vec::new(),
            has_desc: false,
            rows: Vec::new(),
        })
    }

    /// 类型选择器数据：范式目录、参数位、可引用目标，以及当前类型拆解。
    pub fn type_picker_info(
        &self,
        group: &str,
        name: &str,
        current: &str,
    ) -> Result<TypePickerInfo, String> {
        use tablet_core::types::{BaseType, Paradigm, TblType, TypeParam};
        let p = self.engine.active().ok_or("没有活跃的 Project")?;
        let is_constant = p
            .groups
            .iter()
            .any(|g| g.constants.iter().any(|c| !c.deleted && c.name == name && g.name == group));
        let allow_ref = !is_constant
            || self
                .engine
                .global_config
                .ui
                .as_ref()
                .map_or(true, |u| u.constant_ref_allowed);

        let mut refs = Vec::new();
        for g in &p.groups {
            for t in &g.tables {
                if !t.deleted {
                    refs.push(TypeRefTargetInfo { name: t.name.clone(), is_table: true });
                }
            }
            for e in &g.enums {
                if !e.deleted {
                    refs.push(TypeRefTargetInfo { name: e.name.clone(), is_table: false });
                }
            }
        }
        refs.sort_by(|a, b| a.name.cmp(&b.name));

        let paradigms: Vec<TypeParadigmInfo> = Paradigm::all_data()
            .iter()
            .enumerate()
            .map(|(index, para)| TypeParadigmInfo {
                index,
                label: para.label().to_string(),
                slots: para
                    .param_slots()
                    .into_iter()
                    .map(|s| TypeSlotInfo { label: s.label.to_string(), is_map_key: s.is_map_key })
                    .collect(),
            })
            .collect();

        let parsed = TblType::parse(current);
        let mut paradigm_index = None;
        let mut params = Vec::new();
        let mut ref_name = None;
        match &parsed {
            Some(t) if t.paradigm == Paradigm::Ref => {
                ref_name = t.ref_name.clone();
            }
            Some(t) => {
                paradigm_index = Paradigm::all_data().iter().position(|x| *x == t.paradigm);
                params = t
                    .params
                    .iter()
                    .map(|param| match param {
                        TypeParam::Base(b) => b.name().to_string(),
                        TypeParam::Ref(r) => format!("@{r}"),
                    })
                    .collect();
            }
            None => {}
        }
        // 没有当前值 / 解析失败：默认基础 int。
        if paradigm_index.is_none() && ref_name.is_none() {
            paradigm_index = Paradigm::all_data()
                .iter()
                .position(|x| *x == Paradigm::Base);
            params = vec!["int".to_string()];
        }
        // 补齐参数位，保证前端每个槽都有值。
        if let Some(idx) = paradigm_index {
            let slots = Paradigm::all_data()[idx].param_slots();
            while params.len() < slots.len() {
                let i = params.len();
                params.push(if slots[i].is_map_key { "int".to_string() } else { "int".to_string() });
            }
        }

        Ok(TypePickerInfo {
            current: current.to_string(),
            is_constant,
            allow_ref,
            paradigms,
            base_all: BaseType::all().iter().map(|b| b.name().to_string()).collect(),
            base_nestable: BaseType::all_nestable().iter().map(|b| b.name().to_string()).collect(),
            map_keys: BaseType::map_key_types().iter().map(|b| b.name().to_string()).collect(),
            refs,
            paradigm_index,
            params,
            ref_name,
        })
    }

    /// 根据范式 + 参数位生成类型字符串与各语言声明。组合非法时 valid=false。
    pub fn type_preview(&self, paradigm_index: usize, params: Vec<String>) -> TypePreview {
        use tablet_core::types::{BaseType, Paradigm, TblType, TypeParam};
        let Some(paradigm) = Paradigm::all_data().get(paradigm_index).cloned() else {
            return TypePreview::invalid("未知类型范式");
        };
        let parsed: Vec<TypeParam> = params
            .iter()
            .map(|s| match s.strip_prefix('@') {
                Some(name) => TypeParam::Ref(name.to_string()),
                None => TypeParam::Base(BaseType::from_str(s).unwrap_or(BaseType::Int)),
            })
            .collect();
        let slots = paradigm.param_slots().len();
        if parsed.len() != slots {
            return TypePreview::invalid("参数位数量不匹配");
        }
        let tt = TblType { paradigm, params: parsed, ref_name: None };
        let type_string = tt.to_type_string();
        if TblType::parse(&type_string).is_none() {
            return TypePreview::invalid("该类型组合不合法（例如 txt 不能嵌套、Map 的 K 不能是 bool）");
        }
        let sep = self
            .engine
            .active()
            .map(|p| p.schema.separators.clone())
            .unwrap_or_default();
        TypePreview {
            valid: true,
            type_string,
            example: tt.example_with_sep(&sep),
            java: tt.java_decl(),
            go: tt.go_decl(),
            lua: tt.lua_decl(),
            error: None,
        }
    }

    fn display_ref_value(
        &self,
        ty: &str,
        raw: &str,
        cache: &mut std::collections::HashMap<String, Option<std::collections::HashMap<String, String>>>,
    ) -> String {        let Some((target, multi)) = ref_target_of(ty) else { return raw.to_string() };
        if raw.is_empty() {
            return String::new();
        }
        let key = target.clone();
        if !cache.contains_key(&key) {
            cache.insert(key.clone(), self.ref_display_map(&target));
        }
        let Some(map) = cache.get(&key).and_then(|m| m.as_ref()) else {
            return raw.to_string();
        };
        if !multi {
            return map.get(raw).cloned().unwrap_or_else(|| raw.to_string());
        }
        raw.split(';')
            .map(|part| map.get(part.trim()).cloned().unwrap_or_else(|| part.to_string()))
            .collect::<Vec<_>>()
            .join(";")
    }

    // ── 校验规则 ────────────────────────────────────────────
    pub fn rules_payload(
        &mut self,
        project: &str,
        group: &str,
        name: &str,
    ) -> Result<RulesPayload, String> {
        let _ = self.engine.set_active_by_id(project);

        let (mode, columns) = if let Some(t) = self.engine.find_table(group, name) {
            (
                NodeMode::Table,
                t.schema.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            )
        } else if self.engine.find_enum(group, name).is_some() {
            (
                NodeMode::Enum,
                vec!["id".to_string(), "name".to_string(), "desc".to_string()],
            )
        } else if self.engine.find_constant(group, name).is_some() {
            (NodeMode::Constant, Vec::new())
        } else {
            return Err(format!("节点不存在: {group}/{name}"));
        };

        let user = self
            .engine
            .node_rules(group, name)
            .map(|(_, rules)| rules)
            .unwrap_or_default();
        let rules = user.iter().map(rule_to_item).collect();
        let system = builtin_rules(mode)
            .iter()
            .map(|r| SystemRule {
                target: r.target.column().unwrap_or("").to_string(),
                text: r.body_string(),
                level: r.level.as_str().to_string(),
            })
            .collect();
        let functions = FUNCTIONS
            .iter()
            .filter(|f| f.status == FunctionStatus::Active)
            .map(|f| FunctionInfo {
                name: f.name,
                scopes: f
                    .scopes
                    .iter()
                    .map(|s| match s {
                        RuleScope::Column => "column",
                        RuleScope::Table => "table",
                    })
                    .collect(),
                granularity: match f.granularity {
                    Granularity::Row => "row",
                    Granularity::Table => "table",
                },
                primary_index: f.primary_column,
                params: f.params.iter().map(|p| param_str(*p)).collect(),
                variadic: f.variadic.map(param_str),
                summary: f.summary,
            })
            .collect();

        Ok(RulesPayload {
            project: project.to_string(),
            group: group.to_string(),
            name: name.to_string(),
            mode: mode_str(mode).to_string(),
            columns,
            supports_table: mode == NodeMode::Table,
            can_edit: mode != NodeMode::Constant,
            rules,
            system,
            functions,
        })
    }

    /// 保存规则：UI 树 → 表达式文本 → core 解析 → `set_node_rules`。
    /// 任何一条解析失败都整体拒绝，并指出第几条，避免半截保存。
    pub fn rules_save(
        &mut self,
        project: &str,
        group: &str,
        name: &str,
        items: Vec<RuleItem>,
    ) -> Result<(), String> {
        let _ = self.engine.set_active_by_id(project);
        let mut rules = Vec::with_capacity(items.len());
        for (idx, item) in items.iter().enumerate() {
            let text = expr_text(&item.expr);
            let body = if item.target.is_empty() {
                format!("table: {text}")
            } else {
                format!("column {}: {text}", item.target)
            };
            let mut rule = tablet_core::schema_grammar::parse_rule_body(&body)
                .map_err(|e| format!("第 {} 条规则解析失败（{body}）: {e}", idx + 1))?;
            rule.level = Severity::parse(&item.level).unwrap_or(Severity::Error);
            rule.source = RuleSource::User;
            rules.push(rule);
        }
        if !self.engine.set_node_rules(group, name, rules) {
            return Err("保存规则失败（节点不存在或项目未打开）".into());
        }
        self.engine.revalidate(group, name);
        Ok(())
    }

    // ── 新建项目 / 项目设置 ────────────────────────────────
    pub fn list_templates(&self) -> Vec<TemplateItem> {
        let mut out = Vec::new();
        let builtin = BuiltinTemplates::new();
        for m in builtin.list() {
            let sections = builtin
                .load_by_id(&m.id)
                .map(|c| c.schema.sections.len())
                .unwrap_or(0);
            out.push(TemplateItem {
                id: m.id,
                name: m.name,
                category: m.category,
                version: m.version,
                source: m.source.to_string(),
                sections,
            });
        }
        let local = LocalTemplates::new(tablet_core::template::default_local_dir());
        for m in local.list() {
            let sections = local
                .load_by_id(&m.id)
                .map(|c| c.schema.sections.len())
                .unwrap_or(0);
            out.push(TemplateItem {
                id: m.id,
                name: m.name,
                category: m.category,
                version: m.version,
                source: m.source.to_string(),
                sections,
            });
        }
        out
    }

    pub fn schema_preview(&self, path: String) -> Result<SchemaPreview, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, &path);
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("读取 {} 失败: {e}", file.display()))?;
        let schema = parse_tblschema(&text).map_err(|e| e.to_string())?;
        Ok(preview_schema(&schema))
    }

    pub fn template_preview(&self, template_id: String) -> Result<SchemaPreview, String> {
        let content = BuiltinTemplates::new()
            .load_by_id(&template_id)
            .or_else(|| {
                LocalTemplates::new(tablet_core::template::default_local_dir())
                    .load_by_id(&template_id)
            })
            .ok_or_else(|| format!("模板不存在: {template_id}"))?;
        Ok(preview_schema(&content.schema))
    }

    /// 新建项目：来源（空/文件/模板）+ 段筛选 + 身份 + 是否立即打开。
    pub fn create_project(&mut self, req: CreateProjectRequest) -> Result<String, String> {
        let mut schema = match req.source.as_str() {
            "file" => {
                let path = req.path.clone().ok_or("缺少 .tblschema 路径")?;
                let file = resolve_in_project(
                    &self
                        .engine
                        .active()
                        .map(|p| p.project_root.clone())
                        .unwrap_or_else(|| self.workdir.clone()),
                    &path,
                );
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("读取 {} 失败: {e}", file.display()))?;
                parse_tblschema(&text).map_err(|e| e.to_string())?
            }
            "template" => {
                let id = req.template_id.clone().ok_or("缺少模板 id")?;
                BuiltinTemplates::new()
                    .load_by_id(&id)
                    .or_else(|| {
                        LocalTemplates::new(tablet_core::template::default_local_dir())
                            .load_by_id(&id)
                    })
                    .ok_or_else(|| format!("模板不存在: {id}"))?
                    .schema
            }
            _ => TblSchema::default(),
        };

        if !req.sections.is_empty() {
            schema.sections.retain(|s| {
                req.sections
                    .iter()
                    .any(|k| k == &format!("{}/{}", s.group, s.name))
            });
            if schema.sections.is_empty() {
                return Err("没有选中任何段".to_string());
            }
        }
        schema.meta.id = req.id;
        schema.meta.name = req.name;
        schema.meta.category = req.category;
        schema.meta.version = req.version;
        if schema.meta.name.is_empty() {
            schema.meta.name = schema.meta.id.clone();
        }
        let with_preset =
            req.with_preset && (schema.meta.has_preset || schema.sections.iter().any(|s| !s.preset.is_empty()));
        let pid = self.engine.create_project_in_memory_with(schema, with_preset)?;
        if req.open_now {
            let _ = self.engine.set_active_by_id(&pid);
            self.selected = Some(Selected {
                project: pid.clone(),
                group: String::new(),
                name: String::new(),
                kind: SelKind::Project,
            });
        }
        Ok(pid)
    }

    /// 新建项目统一收口：覆盖 id/name → 进内存（Pending）→ 设为 active。
    /// 不立即落盘：与 Slint 版一致，用户点「保存」才写 `projects/<id>/`。
    fn apply_new_project(
        &mut self,
        mut schema: TblSchema,
        id: String,
        name: String,
    ) -> Result<String, String> {
        schema.meta.id = id;
        if !name.trim().is_empty() {
            schema.meta.name = name;
        } else if schema.meta.name.is_empty() {
            schema.meta.name = schema.meta.id.clone();
        }
        let with_preset =
            schema.meta.has_preset || schema.sections.iter().any(|s| !s.preset.is_empty());
        let pid = self.engine.create_project_in_memory_with(schema, with_preset)?;
        let _ = self.engine.set_active_by_id(&pid);
        self.selected = Some(Selected {
            project: pid.clone(),
            group: String::new(),
            name: String::new(),
            kind: SelKind::Project,
        });
        Ok(pid)
    }

    pub fn create_project_empty(&mut self, id: String, name: String) -> Result<String, String> {
        let mut schema = TblSchema::default();
        schema.meta = SchemaMetadata {
            id: id.clone(),
            name: name.clone(),
            ..Default::default()
        };
        self.apply_new_project(schema, id, name)
    }

    pub fn create_project_from_template(
        &mut self,
        id: String,
        name: String,
        template_id: String,
    ) -> Result<String, String> {
        let content = BuiltinTemplates::new()
            .load_by_id(&template_id)
            .or_else(|| {
                LocalTemplates::new(tablet_core::template::default_local_dir())
                    .load_by_id(&template_id)
            })
            .ok_or_else(|| format!("模板不存在: {template_id}"))?;
        self.apply_new_project(content.schema, id, name)
    }

    pub fn create_project_from_file(
        &mut self,
        id: String,
        name: String,
        path: String,
    ) -> Result<String, String> {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("读取 {path} 失败: {e}"))?;
        let schema =
            tablet_core::tblschema::parse_tblschema(&text).map_err(|e| format!("解析失败: {e}"))?;
        self.apply_new_project(schema, id, name)
    }

    pub fn project_settings(&self, project: &str) -> Option<ProjectSettings> {
        let p = self.engine.find_project(project)?;
        let def = SeparatorsSection::default();
        let separators = SepKey::ALL
            .iter()
            .map(|k| SepItem {
                key: k.as_directive_key().to_string(),
                value: k.get(&p.schema.separators).to_string(),
                default: k.get(&def).to_string(),
                original: k.get(&p.original_separators).to_string(),
            })
            .collect();
        Some(ProjectSettings {
            id: p.schema.meta.id.clone(),
            name: p.schema.meta.name.clone(),
            category: p.schema.meta.category.clone(),
            version: p.schema.meta.version.clone(),
            original_name: p.original_meta.name.clone(),
            original_category: p.original_meta.category.clone(),
            original_version: p.original_meta.version.clone(),
            separators,
            export_settings: export_ui(&p.config.export),
            original_export: export_ui(&p.original_export),
            default_export: export_ui(&ExportConfig::default()),
            validate: ValidateUi {
                error: p.config.validate.error.as_str().to_string(),
                warning: p.config.validate.warning.as_str().to_string(),
                info: p.config.validate.info.as_str().to_string(),
            },
        })
    }

    /// 项目身份 + 分隔符 + 导出设置。id 不可改：它是目录名与引用标识。
    pub fn save_project_settings(
        &mut self,
        project: &str,
        name: String,
        category: String,
        version: String,
        separators: Vec<SepInput>,
        export_settings: ExportUi,
        validate: ValidateUi,
    ) {
        self.engine.edit_project(project, "项目设置", |p| {
            p.schema.meta.name = name;
            p.schema.meta.category = category;
            p.schema.meta.version = version;
            for item in &separators {
                if let Some(k) = SepKey::from_directive_key(&item.key) {
                    k.set(&mut p.schema.separators, item.value.clone());
                }
            }
            apply_export(&mut p.config.export, &export_settings);
            p.config.validate = ValidateConfig {
                error: parse_action(&validate.error),
                warning: parse_action(&validate.warning),
                info: parse_action(&validate.info),
            };
        });
        // 分隔符会影响值的解析与校验结果，改完统一重算。
        self.engine.revalidate_all_projects();
    }

    // ── 节点增删改 ──────────────────────────────────────────
    pub fn new_group(&mut self, project: &str, name: String, xlsx: bool) {
        self.engine.execute_action(&ProjectAction::NewGroup {
            project_id: project.to_string(),
            name,
            storage: if xlsx { GroupStorage::Xlsx } else { GroupStorage::Tbl },
        });
    }

    pub fn new_node(&mut self, project: &str, group: &str, kind: &str, name: String, fmt: Option<String>) {
        let pid = project.to_string();
        let g = group.to_string();
        let fmt_ver = fmt
            .as_deref()
            .and_then(tablet_core::tbl::TblVersion::parse_label)
            .unwrap_or(tablet_core::tbl::DEFAULT_TBL_VERSION);
        let action = match kind {
            "constant" => ProjectAction::NewConstant { project_id: pid, group: g, name },
            "enum" => ProjectAction::NewEnum { project_id: pid, group: g, name },
            _ => ProjectAction::NewTable {
                project_id: pid,
                group: g,
                name,
                fmt_ver,
            },
        };
        self.engine.execute_action(&action);
    }

    pub fn rename_group(&mut self, project: &str, old_name: String, new_name: String) {
        self.engine.execute_action(&ProjectAction::RenameGroup {
            project_id: project.to_string(),
            old_name,
            new_name,
        });
    }

    pub fn rename_node(&mut self, project: &str, group: &str, old_name: String, new_name: String) {
        self.engine.execute_action(&ProjectAction::RenameNode {
            project_id: project.to_string(),
            group: group.to_string(),
            old_name,
            new_name,
        });
    }

    pub fn delete_group(&mut self, project: &str, group: &str) {
        let _ = self.engine.set_active_by_id(project);
        self.engine.delete_group(group);
    }

    pub fn delete_node(&mut self, project: &str, group: &str, name: &str) {
        let _ = self.engine.set_active_by_id(project);
        self.engine.delete_node(group, name);
    }

    pub fn copy_node(&mut self, project: &str, group: &str, name: &str, kind: &str) {
        let kind = match kind {
            "constant" => NodeKind::Constant,
            "enum" => NodeKind::Enum,
            _ => NodeKind::Table,
        };
        self.engine.clipboard_copy_node(project, group, name, kind);
    }

    pub fn copy_group(&mut self, project: &str, group: &str) {
        self.engine.clipboard_copy_group(project, group);
    }

    pub fn paste_node(&mut self, project: &str, group: &str, fmt: Option<String>) -> Option<String> {
        let fmt_ver = fmt
            .as_deref()
            .and_then(tablet_core::tbl::TblVersion::parse_label);
        self.engine.paste_node_to(project, group, fmt_ver)
    }

    pub fn revert_node(&mut self, project: &str, group: &str, name: &str) -> bool {
        let _ = self.engine.set_active_by_id(project);
        self.engine.revert_node(group, name)
    }

    // ── 项目级动作 ──────────────────────────────────────────
    pub fn save(&mut self) -> bool {
        self.engine.save_all()
    }

    pub fn undo(&mut self) {
        let _ = self.engine.undo();
        self.refresh_selected_validation();
    }

    pub fn redo(&mut self) {
        let _ = self.engine.redo();
        self.refresh_selected_validation();
    }

    pub fn reload(&mut self) {
        self.engine.reload();
        self.refresh_selected_validation();
    }

    pub fn validate(&mut self) {
        self.engine.revalidate_all_projects();
    }

    fn refresh_selected_validation(&mut self) {
        if let Some((group, name, kind)) = self.selected_node() {
            if kind != SelKind::Project && kind != SelKind::Group {
                let (group, name) = (group.to_string(), name.to_string());
                self.engine.revalidate(&group, &name);
            }
        }
    }

    pub fn open_project(&mut self, project: &str) {
        let _ = self.engine.open_project(project);
        if self.engine.active_project_id().is_none() {
            let _ = self.engine.set_active_by_id(project);
        }
    }

    pub fn close_project(&mut self, project: &str) {
        if self.engine.close_project(project) {
            if self.selected.as_ref().is_some_and(|s| s.project == project) {
                self.selected = None;
            }
        }
    }

    pub fn delete_project(&mut self, project: &str) -> Result<(), String> {
        if self.engine.projects.len() <= 1 {
            return Err("至少要保留一个打开的项目".to_string());
        }
        self.engine.execute_action(&ProjectAction::DeleteProject {
            project_id: project.to_string(),
        });
        if self.selected.as_ref().is_some_and(|s| s.project == project) {
            self.selected = None;
        }
        Ok(())
    }

    pub fn clone_project(
        &mut self,
        source: &str,
        new_id: String,
        new_name: String,
    ) -> Result<String, String> {
        self.engine
            .clone_project_in_memory(source, &new_id, &new_name)
            .ok_or_else(|| "复制失败（id 冲突或源项目未打开）".to_string())
    }

    /// 当前选中节点在 (row, col) 上的校验原因（状态栏「红框是为什么」）。
    /// 与 Slint 版一致：即时重算，不受「实时校验」开关影响。
    pub fn cell_issue(&self, row: usize, col: usize) -> Option<CellIssue> {
        use tablet_core::model::SeverityAction;
        use tablet_core::validate::{
            validate_constant, validate_enum, validate_table, RefIndex, RuleIndex,
        };
        let project = self.engine.active()?;
        let sel = self.selected.as_ref()?;
        if sel.name.is_empty() {
            return None;
        }
        let sep = &project.schema.separators;
        let policy = project.config.validate.clone();
        let errors = match sel.kind {
            SelKind::Table => {
                let table = self.engine.find_table(&sel.group, &sel.name)?;
                let refs = RefIndex::build(&project.groups);
                let cidx = RuleIndex::build(&project.groups);
                validate_table(table, sep, Some(&refs), Some(&cidx))
            }
            SelKind::Constant => {
                let constant = self.engine.find_constant(&sel.group, &sel.name)?;
                let refs = RefIndex::build(&project.groups);
                let allow_ref = self
                    .engine
                    .global_config
                    .ui
                    .as_ref()
                    .map_or(true, |u| u.constant_ref_allowed);
                validate_constant(constant, sep, allow_ref, Some(&refs))
            }
            SelKind::Enum => {
                let enum_def = self.engine.find_enum(&sel.group, &sel.name)?;
                let cidx = RuleIndex::build(&project.groups);
                validate_enum(enum_def, Some(&cidx))
            }
            _ => return None,
        };
        let hits: Vec<_> = errors
            .into_iter()
            .filter(|e| !e.is_schema() && e.row == row && e.col == col)
            .filter(|e| policy.action(e.severity) != SeverityAction::Ignore)
            .collect();
        if let Some(e) = hits
            .iter()
            .find(|e| policy.action(e.severity) == SeverityAction::Block)
        {
            return Some(CellIssue { severity: "error".into(), message: e.message.clone() });
        }
        hits.first().map(|e| CellIssue { severity: "warn".into(), message: e.message.clone() })
    }

    pub fn paste_group(&mut self, project: &str, storage: Option<String>) -> Option<String> {
        let storage = parse_group_storage(storage);
        self.engine.paste_group_as(project, storage)
    }

    /// 粘贴 Group 前的只读预览（有冲突才弹命名对话框）。
    pub fn paste_group_preview(&self, project: &str) -> Option<PasteGroupPreviewUi> {
        let preview = self.engine.preview_paste_group(project)?;
        Some(PasteGroupPreviewUi {
            group_name: preview.group_name,
            group_conflict: preview.group_conflict,
            has_conflict: preview.has_conflict,
            nodes: preview
                .nodes
                .into_iter()
                .map(|n| PasteGroupRowUi {
                    kind: match n.kind {
                        NodeKind::Table => "table",
                        NodeKind::Constant => "constant",
                        NodeKind::Enum => "enum",
                    }
                    .to_string(),
                    old_name: n.old_name,
                    new_name: n.new_name,
                    conflict: n.conflict,
                })
                .collect(),
        })
    }

    /// 按用户确认的组名 / 子项名粘贴；重名仍会再校验一次，失败不落数据。
    pub fn paste_group_named(
        &mut self,
        project: &str,
        storage: Option<String>,
        group_name: String,
        nodes: Vec<PasteGroupRowUi>,
    ) -> Result<String, String> {
        let names: std::collections::BTreeMap<String, String> = nodes
            .into_iter()
            .map(|n| (n.old_name, n.new_name))
            .collect();
        let name = self
            .engine
            .paste_group_as_with_names(project, parse_group_storage(storage), &group_name, &names)
            .map_err(|e| e.to_string())?;
        self.engine.revalidate_all_projects();
        Ok(name)
    }

    pub fn import_tbl(
        &mut self,
        project: &str,
        group: &str,
        paths: Vec<String>,
    ) -> Result<String, String> {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let outcome = self
            .engine
            .import_tbl_files(project, group, &paths)
            .map_err(|e| e.to_string())?;
        self.engine.revalidate_all_projects();
        Ok(format!("已导入 {} 个节点到 {}", outcome.nodes, group))
    }

    pub fn import_xlsx_group(&mut self, project: &str, path: String) -> Result<String, String> {
        let root = self
            .engine
            .find_project(project)
            .map(|p| p.project_root.clone())
            .unwrap_or_else(|| self.workdir.clone());
        let file = resolve_in_project(&root, &path);
        let outcome = self
            .engine
            .import_xlsx_group(project, &file)
            .map_err(|e| e.to_string())?;
        self.engine.revalidate_all_projects();
        Ok(format!("已导入 {} 个节点", outcome.nodes))
    }

    pub fn open_project_dir(&self, project: &str) -> Result<(), String> {
        let p = self
            .engine
            .find_project(project)
            .ok_or_else(|| format!("项目不存在: {project}"))?;
        launch(&p.project_root.to_string_lossy())
    }

    pub fn reload_group(&mut self, project: &str, group: &str) -> Result<(), String> {
        self.engine
            .reload_xlsx_group(project, group)
            .map_err(|e| e.to_string())
    }

    pub fn export_template(&self, id: String, name: String) -> Result<String, String> {
        let opts = SchemaExportOptions {
            out: None,
            as_template: true,
            groups: Vec::new(),
            nodes: Vec::new(),
            with_preset: false,
            meta_id: Some(id),
            meta_name: Some(name),
            meta_category: None,
            meta_version: None,
        };
        let outcome = run_schema_export(&self.engine, &opts).map_err(|e| e.to_string())?;
        Ok(format!("已保存模板 → {}", outcome.path))
    }
}

// ── 导出设置辅助 ────────────────────────────────────────────
/// `@Name` → (Name, false)；`List<@Name>` / `Set<@Name>` → (Name, true)。
fn ref_target_of(ty: &str) -> Option<(String, bool)> {
    let s = ty.trim();
    if let Some(name) = s.strip_prefix('@') {
        return Some((name.trim().to_string(), false));
    }
    for prefix in ["List<", "Set<"] {
        if let Some(inner) = s.strip_prefix(prefix).and_then(|x| x.strip_suffix('>')) {
            if let Some(name) = inner.trim().strip_prefix('@') {
                return Some((name.trim().to_string(), true));
            }
        }
    }
    None
}

fn parse_action(s: &str) -> SeverityAction {
    match s {
        "report" => SeverityAction::Report,
        "ignore" => SeverityAction::Ignore,
        _ => SeverityAction::Block,
    }
}

fn trigger_str(t: PickerTrigger) -> &'static str {
    match t {
        PickerTrigger::Single => "single",
        PickerTrigger::Double => "double",
    }
}

fn parse_trigger(s: &str) -> PickerTrigger {
    if s == "single" {
        PickerTrigger::Single
    } else {
        PickerTrigger::Double
    }
}

fn parse_log_level(s: &str) -> LogLevel {
    match s {
        "trace" => LogLevel::Trace,
        "debug" => LogLevel::Debug,
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

fn export_ui(cfg: &ExportConfig) -> ExportUi {
    let json = cfg.json.clone().unwrap_or_default();
    let xml = cfg.xml.clone().unwrap_or_default();
    let server = cfg.server.clone().unwrap_or_default();
    let client = cfg.client.clone().unwrap_or_default();
    let java = server.java.clone().unwrap_or_default();
    let go = server.go.clone().unwrap_or_default();
    let cpp = server.cpp.clone().unwrap_or_default();
    let dotnet = server.csharp_dotnet.clone().unwrap_or_default();
    let server_ts = server.typescript.clone().unwrap_or_default();
    let lua = client.lua.clone().unwrap_or_default();
    let gdscript = client.gdscript.clone().unwrap_or_default();
    let client_ts = client.typescript.clone().unwrap_or_default();
    let unity = client.csharp_unity.clone().unwrap_or_default();
    let godot = client.csharp_godot.clone().unwrap_or_default();
    ExportUi {
        encoding: cfg.encoding.unwrap_or_default().to_string(),
        line_ending: cfg.line_ending.unwrap_or_default().to_string(),
        json_empty_as: json.empty_as.unwrap_or_default().to_string(),
        xml_empty_as: xml.empty_as.unwrap_or_default().to_string(),
        server_data_output: server.data_output.clone().unwrap_or_default(),
        java_package: java.package.clone().unwrap_or_default(),
        java_output: java.code_output.clone().unwrap_or_default(),
        java_lombok: java.lombok.unwrap_or(true),
        java_custom_regions: java.custom_regions.unwrap_or(true),
        go_package: go.package.clone().unwrap_or_default(),
        go_output: go.code_output.clone().unwrap_or_default(),
        go_custom_regions: go.custom_regions.unwrap_or(true),
        cpp_namespace: cpp.namespace.clone().unwrap_or_default(),
        cpp_output: cpp.code_output.clone().unwrap_or_default(),
        cpp_custom_regions: cpp.custom_regions.unwrap_or(true),
        dotnet_namespace: dotnet.namespace.clone().unwrap_or_default(),
        dotnet_output: dotnet.code_output.clone().unwrap_or_default(),
        dotnet_custom_regions: dotnet.custom_regions.unwrap_or(true),
        server_ts_output: server_ts.output.clone().unwrap_or_default(),
        lua_output: lua.output.clone().unwrap_or_default(),
        gdscript_output: gdscript.output.clone().unwrap_or_default(),
        client_ts_output: client_ts.output.clone().unwrap_or_default(),
        unity_namespace: unity.namespace.clone().unwrap_or_default(),
        unity_output: unity.code_output.clone().unwrap_or_default(),
        unity_custom_regions: unity.custom_regions.unwrap_or(true),
        godot_namespace: godot.namespace.clone().unwrap_or_default(),
        godot_output: godot.code_output.clone().unwrap_or_default(),
        godot_custom_regions: godot.custom_regions.unwrap_or(true),
    }
}

/// 只覆盖 UI 暴露的字段；其它字段（如 `cpp.json_lib`）保留原值。
fn apply_export(cfg: &mut ExportConfig, ui: &ExportUi) {
    cfg.encoding = Some(ui.encoding.parse().unwrap_or_default());
    cfg.line_ending = Some(ui.line_ending.parse().unwrap_or_default());
    cfg.json = Some(JsonExport {
        empty_as: Some(ui.json_empty_as.parse().unwrap_or_default()),
    });
    cfg.xml = Some(XmlExport {
        empty_as: Some(ui.xml_empty_as.parse().unwrap_or_default()),
    });

    let server = cfg.server.get_or_insert_with(ServerExport::default);
    server.data_output = Some(ui.server_data_output.clone());
    let java = server.java.get_or_insert_with(JavaExport::default);
    java.package = Some(ui.java_package.clone());
    java.code_output = Some(ui.java_output.clone());
    java.lombok = Some(ui.java_lombok);
    java.custom_regions = Some(ui.java_custom_regions);
    let go = server.go.get_or_insert_with(GoExport::default);
    go.package = Some(ui.go_package.clone());
    go.code_output = Some(ui.go_output.clone());
    go.custom_regions = Some(ui.go_custom_regions);
    let cpp = server.cpp.get_or_insert_with(CppExport::default);
    cpp.namespace = Some(ui.cpp_namespace.clone());
    cpp.code_output = Some(ui.cpp_output.clone());
    cpp.custom_regions = Some(ui.cpp_custom_regions);
    let dotnet = server.csharp_dotnet.get_or_insert_with(DotNetExport::default);
    dotnet.namespace = Some(ui.dotnet_namespace.clone());
    dotnet.code_output = Some(ui.dotnet_output.clone());
    dotnet.custom_regions = Some(ui.dotnet_custom_regions);
    let server_ts = server
        .typescript
        .get_or_insert_with(ServerTypeScriptExport::default);
    server_ts.output = Some(ui.server_ts_output.clone());

    let client = cfg.client.get_or_insert_with(ClientConfig::default);
    let lua = client.lua.get_or_insert_with(LuaExport::default);
    lua.output = Some(ui.lua_output.clone());
    let gdscript = client.gdscript.get_or_insert_with(GdScriptExport::default);
    gdscript.output = Some(ui.gdscript_output.clone());
    let client_ts = client
        .typescript
        .get_or_insert_with(ClientTypeScriptExport::default);
    client_ts.output = Some(ui.client_ts_output.clone());
    let unity = client
        .csharp_unity
        .get_or_insert_with(UnityCSharpExport::default);
    unity.namespace = Some(ui.unity_namespace.clone());
    unity.code_output = Some(ui.unity_output.clone());
    unity.custom_regions = Some(ui.unity_custom_regions);
    let godot = client
        .csharp_godot
        .get_or_insert_with(GodotCSharpExport::default);
    godot.namespace = Some(ui.godot_namespace.clone());
    godot.code_output = Some(ui.godot_output.clone());
    godot.custom_regions = Some(ui.godot_custom_regions);
}

// ── 规则辅助 ────────────────────────────────────────────────
fn resolve_in_project(root: &std::path::Path, path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        root.join(p)
    }
}

fn preview_schema(schema: &TblSchema) -> SchemaPreview {
    let sections = schema
        .sections
        .iter()
        .map(|s| SectionItem {
            group: s.group.clone(),
            name: s.name.clone(),
            mode: match s.mode {
                tablet_core::schema_model::SchemaMode::Table => "table",
                tablet_core::schema_model::SchemaMode::Constant => "constant",
                tablet_core::schema_model::SchemaMode::Enum => "enum",
            }
            .to_string(),
        })
        .collect();
    SchemaPreview {
        id: schema.meta.id.clone(),
        name: schema.meta.name.clone(),
        category: schema.meta.category.clone(),
        version: schema.meta.version.clone(),
        has_preset: schema.meta.has_preset
            || schema.sections.iter().any(|s| !s.preset.is_empty()),
        sections,
    }
}

fn launch(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", target]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(target);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}
fn mode_str(mode: NodeMode) -> &'static str {
    match mode {
        NodeMode::Table => "table",
        NodeMode::Constant => "constant",
        NodeMode::Enum => "enum",
    }
}

fn param_str(p: ParamKind) -> &'static str {
    match p {
        ParamKind::Column => "column",
        ParamKind::Text => "text",
        ParamKind::Regex => "regex",
        ParamKind::Bound => "bound",
        ParamKind::CompareOp => "compare_op",
        ParamKind::NodeColumn => "node_column",
    }
}

fn rule_to_item(r: &Rule) -> RuleItem {
    RuleItem {
        target: r.target.column().unwrap_or("").to_string(),
        level: r.level.as_str().to_string(),
        expr: expr_to_ui(&r.expr, r.target.scope()),
    }
}

fn expr_to_ui(e: &RuleExpr, scope: RuleScope) -> ExprUi {
    match e {
        RuleExpr::Validator(k) => {
            let (name, args) = split_call(&k.expr_string(scope == RuleScope::Column));
            ExprUi::Fun { name, args }
        }
        RuleExpr::When { condition, inner } => ExprUi::When {
            cond: condition.expr_string(),
            inner: Box::new(expr_to_ui(inner, scope)),
        },
        RuleExpr::And(cs) => ExprUi::Group {
            op: "and".into(),
            members: cs.iter().map(|c| expr_to_ui(c, scope)).collect(),
        },
        RuleExpr::Or(cs) => ExprUi::Group {
            op: "or".into(),
            members: cs.iter().map(|c| expr_to_ui(c, scope)).collect(),
        },
    }
}

fn expr_text(e: &ExprUi) -> String {
    match e {
        ExprUi::Fun { name, args } => format!("{}({})", name, args.join(", ")),
        ExprUi::Group { op, members } => format!(
            "{}({})",
            op,
            members.iter().map(expr_text).collect::<Vec<_>>().join(", ")
        ),
        ExprUi::When { cond, inner } => format!("when({}, {})", cond, expr_text(inner)),
    }
}

/// 把 `name(a, b, "c, d")` 拆成 (name, args)。尊重引号、转义与嵌套括号。
fn split_call(text: &str) -> (String, Vec<String>) {
    let open = text.find('(').unwrap_or(text.len());
    let name = text[..open].trim().to_string();
    if open >= text.len() {
        return (name, Vec::new());
    }
    let close = text.rfind(')').unwrap_or(text.len());
    let inner = &text[open + 1..close.max(open + 1)];
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for ch in inner.chars() {
        if esc {
            cur.push(ch);
            esc = false;
            continue;
        }
        match ch {
            '\\' if in_str => {
                cur.push(ch);
                esc = true;
            }
            '"' => {
                in_str = !in_str;
                cur.push(ch);
            }
            '(' if !in_str => {
                depth += 1;
                cur.push(ch);
            }
            ')' if !in_str => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if !in_str && depth == 0 => {
                args.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() || !args.is_empty() {
        args.push(cur.trim().to_string());
    }
    (name, args)
}

// ── 辅助 ────────────────────────────────────────────────────
fn valid_table_rows(t: &Table) -> usize {
    t.records.iter().filter(|r| r.first().map_or(false, |id| !id.is_empty())).count()
}

fn valid_constant_rows(c: &Constant) -> usize {
    c.entries.iter().filter(|e| !e.name.is_empty()).count()
}

fn is_ref_type(ty: &str) -> bool {
    let s = ty.trim();
    s.starts_with('@') || s.contains("@")
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// 单个节点的状态标记：删除 / 新增 / 修改 / 错误，同一时刻只显示一个。
fn marker(deleted: bool, new: bool, dirty: bool, error: bool) -> (String, String) {
    if deleted {
        ("-".into(), "#dc3232".into())
    } else if new {
        ("+".into(), "#50a050".into())
    } else if dirty {
        ("*".into(), "#d8a020".into())
    } else if error {
        ("!".into(), "#dc3232".into())
    } else {
        (String::new(), String::new())
    }
}

/// 给 vcs 探测留的薄封装：`VcsInfo` 的 title 形态与 Slint 版标题栏一致。
trait VcsTitle {
    fn title(&self) -> String;
}

impl VcsTitle for tablet_core::vcs::VcsInfo {
    fn title(&self) -> String {
        self.display().unwrap_or_else(|| "无版本库".to_string())
    }
}
