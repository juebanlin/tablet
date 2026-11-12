// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! Tauri 后端：把 `tablet-core` 的 ProjectEngine 投影成前端要的快照。
//!
//! 设计原则（@docs/18）：
//! - 前端只负责渲染与交互，业务规则、脏标记、校验、撤销全部走 core；
//! - 每次操作返回一份完整快照（树 + 表格 + 状态 + 日志），前端整体替换；
//!   数据量在配置表场景下可控，换来的是没有增量同步的一致性问题；
//! - 表格的行模型包含 5 行占位行（EXTRA_ROWS），编辑占位行等价于新增记录。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tablet_core::enums::{LogLevel, PickerTrigger};
use tablet_core::model::{
    ClientConfig, Constant, CppExport, DotNetExport, Export, ExportConfig, GdScriptExport,
    GoExport, GroupStorage, JavaExport, JsonExport, LuaExport, NodeKind, ServerExport,
    ServerTypeScriptExport, SeverityAction, Table, UiConfig, UnityCSharpExport, ValidateConfig,
    XmlExport, ClientTypeScriptExport, GodotCSharpExport,
};
use tablet_core::ops::{format_validation_log, LogLevel as LogEntryLevel, ProjectAction, ProjectEngine};
use tablet_core::project::load_workspace;
use tablet_core::rules::{
    builtin_rules, FunctionStatus, Granularity, NodeMode, ParamKind, Rule, RuleExpr, RuleScope,
    RuleSource, Severity, FUNCTIONS,
};
use tablet_core::schema_model::{SchemaMetadata, TblSchema};
use tablet_core::schema_projection::apply_schema_to_project;
use tablet_core::tblschema::parse_tblschema;
use tablet_core::template::{BuiltinTemplates, LocalTemplates, TemplateSource};
use tablet_core::types::{Paradigm, SepKey, SeparatorsSection, TblType, TypeParam};
use tablet_core::excel::{apply_group_patch, import_xlsx_into_group};

/// 与旧版行为一致：表格尾部留 5 行占位，方便直接输入新记录。
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
    /// 本机视图偏好（列宽等），独立于项目数据；不参与 dirty / 保存闸门。
    pub user_data: tablet_core::local_state::UserData,
    pub user_data_path: PathBuf,
    /// 启动时打不开的 Project（首次快照后弹一次提示，与旧版行为一致）。
    pub load_alerts: Vec<String>,
    /// 上一次「主动验证」写进 UI 日志的 (级别, 文本)；再次验证前先按它清掉旧块，
    /// 避免连点「重新验证」时日志里堆出多份相同明细。日志文件仍按追加语义保留历史。
    pub last_validation_logs: Vec<(LogEntryLevel, String)>,
    /// Univer 窗口 label → (project, group)。一个窗口只装一个组，可同时开多个窗口。
    pub univer_windows: HashMap<String, (String, String)>,
    /// 每个已加载 Univer 标签的 Group 指纹：回写时检测主界面是否改过同一组。
    pub univer_fingerprints: HashMap<(String, String), u64>,
    /// 正在 Univer 窗口中打开的组：主界面这些组只读，避免两边同时改。
    pub univer_open_groups: HashSet<(String, String)>,
}

/// Univer 窗口的「打开某组」请求。
#[derive(Serialize, Clone)]
pub struct UniverOpenRequest {
    pub project: String,
    pub group: String,
}

/// 载入一个 Group 工作簿给 Univer 窗口。
#[derive(Serialize)]
pub struct UniverGroupPayload {
    pub project: String,
    pub group: String,
    pub title: String,
    /// `export_group_book` 的字节（base64），窗口侧用 SheetJS 解析。
    pub xlsx_b64: String,
}

#[derive(Serialize)]
pub struct UniverApplyResult {
    pub group: String,
    pub nodes_patched: usize,
}

#[derive(Serialize)]
pub struct XlsxExternalChangeUi {
    pub project: String,
    pub group: String,
    pub dirty: bool,
}

/// 单元格快速提交的轻量响应：不回 10 万行 grid，只回状态 / 树 / 错误定位。
#[derive(Serialize)]
pub struct EditResult {
    pub status: Status,
    pub tree: Vec<TreeRow>,
    pub issues: Vec<IssueRow>,
    pub errors: Vec<[usize; 2]>,
    /// 刚编辑那一格的展示值（开启「显示引用名称」时有意义；`null` = 不用更新）。
    /// 轻量提交不回整份 Snapshot，前端拿它就地刷新引用名，避免要重勾开关才看到新名字。
    pub display: Option<String>,
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
    /// 启动时失败的项目提示（前端首次快照后弹一次）。
    pub startup_alerts: Vec<String>,
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
    /// 失焦自动提交（全局设置）。
    pub auto_commit_on_blur: bool,
    /// 项目树显示元数据 ID（全局设置；树面板勾选框与设置页共用）。
    pub show_meta_id: bool,
    /// 项目排序方式（tablet.toml，跨启动保留）。
    pub project_sort: String,
    /// 正在 Univer 窗口中打开、主界面因此只读的组（当前项目）。
    pub univer_editing: Vec<String>,
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
    /// 二进制整包 `bin/config.bytes`（@20）。
    #[serde(default)]
    pub bin: bool,
    /// 二进制多文件 `bin/parts/<Node>.bytes`。
    #[serde(default)]
    pub bin_split: bool,
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
    /// 可选的语义行（0 = 未设置）。
    pub desc_row: u32,
    pub type_row: u32,
    pub export_row: u32,
    /// 语义行的原始值，供源表预览高亮 / 展示。
    pub semantic_rows: Vec<ExcelSemanticRow>,
    pub columns: Vec<String>,
    pub fields: Vec<MappedFieldUi>,
    pub sample_rows: Vec<Vec<String>>,
}

#[derive(Serialize)]
pub struct ExcelSemanticRow {
    /// desc / type / export
    pub kind: String,
    pub row: u32,
    pub values: Vec<String>,
}

/// 表头行上方可选的语义行识别结果。
#[derive(Serialize)]
pub struct ExcelSemanticRows {
    pub desc_row: u32,
    pub type_row: u32,
    pub export_row: u32,
}

/// 自动识别出的整张表布局（节点类型 + 各行号）。
#[derive(Serialize)]
pub struct ExcelDetectedLayout {
    pub kind: String,
    pub header_row: u32,
    pub data_start: u32,
    pub desc_row: u32,
    pub type_row: u32,
    pub export_row: u32,
}

/// 映射界面左栏用的 Sheet 概览。
#[derive(Serialize)]
pub struct ExcelSheetInfo {
    pub name: String,
    pub columns: u32,
    pub rows: u32,
}

/// 实时映射校验：`index` 指向请求里 fields 的下标。
#[derive(Serialize)]
pub struct ExcelFieldIssue {
    pub index: usize,
    pub message: String,
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

#[derive(Serialize, Deserialize, Clone)]
pub struct MappedFieldUi {
    pub target_name: String,
    pub source_col: u32,
    pub tbl_type: String,
    pub export: String,
    pub desc: String,
}

/// 导出对话框里选中的组 / 节点。
#[derive(Deserialize)]
pub struct ExcelExportSelection {
    pub group: String,
    pub names: Vec<String>,
}

/// 导入对话框：工作簿里每张 sheet 的识别信息。
#[derive(Serialize)]
pub struct ExcelSheetInspect {
    pub name: String,
    pub kind: String,
    pub header_row: u32,
    pub data_start: u32,
    pub desc_row: u32,
    pub type_row: u32,
    pub export_row: u32,
    pub columns: u32,
    pub rows: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 导入对话框的一行：源 sheet → 目标组 / 目标节点。
#[derive(Deserialize)]
pub struct ExcelImportRow {
    pub sheet: String,
    pub group_name: String,
    pub node_name: String,
    #[serde(default)]
    pub create_group: bool,
}

#[derive(Deserialize)]
pub struct ExcelImportRequest {
    pub path: String,
    pub rows: Vec<ExcelImportRow>,
}

#[derive(Deserialize)]
pub struct ExcelMapRequest {
    pub path: String,
    pub sheet: String,
    pub header_row: u32,
    pub data_start: u32,
    /// 目标节点类型：table / constant / enum（缺省 = table）。
    #[serde(default)]
    pub node_kind: String,
    /// 可选的语义行（0 = 不使用）。导入时 core 会按这些行再覆盖一次。
    #[serde(default)]
    pub desc_row: u32,
    #[serde(default)]
    pub type_row: u32,
    #[serde(default)]
    pub export_row: u32,
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
    /// 引用列展示值（show_ref_name 打开时）；为空表示与 rows 相同。
    pub display_rows: Vec<Vec<String>>,
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
    /// 列表引用列的多值分隔符（来自项目 `[separators]`：List 取 `list`、Set 取 `set`）。
    /// 选择器拼值、拆分现有值都必须用它，不能写死 `;`。
    pub ref_seps: Vec<Option<String>>,
    /// 该节点上次的列宽模式（`.data/user-data.json`，按节点记忆）。
    pub auto_width: bool,
    /// 该节点的手动列宽（列名 → px）；自动模式下也保留。
    pub column_widths: std::collections::BTreeMap<String, f32>,
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

/// 单格式导出结果 → 对话框文本（GUI 直接调 core，不经过 CLI action 层）。
fn push_export(
    lines: &mut Vec<String>,
    ok: &mut bool,
    name: &str,
    result: anyhow::Result<tablet_core::export::ExportResult>,
) {
    match result {
        Ok(_) => lines.push(format!("{name}: 导出完成")),
        Err(e) => {
            *ok = false;
            lines.push(format!("{name}: 失败 - {e}"));
        }
    }
}

impl Backend {
    pub fn load(workdir: &Path) -> anyhow::Result<Self> {
        let (mut engine, config_fixed, failures) = load_workspace(workdir)?;
        if config_fixed {
            engine.ui_log("启动时检测到配置文件无效值，已自动修复并保存".to_string());
        }
        let load_alerts: Vec<String> = failures
            .iter()
            .map(|f| format!("Project {} 已跳过：\n{}", f.id, f.message))
            .collect();
        for alert in &load_alerts {
            engine.ui_log(format!("[项目打不开] {alert}"));
        }
        // xlsx 组 / sheet 被策划从工作簿删掉：项目照常打开，只把原因追加到日志
        // （warn_log 同时进 UI 面板和日志文件）。
        let load_warnings: Vec<String> = engine
            .projects
            .iter()
            .flat_map(|p| {
                let id = p.schema.meta.id.clone();
                p.load_warnings.iter().map(move |w| format!("[{}] {}", id, w))
            })
            .collect();
        for warning in &load_warnings {
            engine.warn_log(warning.clone());
        }
        // 上次 Univer 回写的临时副本在这里清（正常回写结束时已删，这里兜底崩溃/强杀）。
        cleanup_univer_cache(&mut engine);
        engine.revalidate_all_projects();
        let user_data_path = tablet_core::local_state::user_data_path(workdir);
        let user_data = tablet_core::local_state::UserData::load(&user_data_path);
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
            user_data,
            user_data_path,
            load_alerts,
            last_validation_logs: Vec::new(),
            univer_windows: HashMap::new(),
            univer_fingerprints: HashMap::new(),
            univer_open_groups: HashSet::new(),
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
            startup_alerts: self.load_alerts.clone(),
        }
    }

    /// 单元格提交后的轻量结果（不构建 grid.rows，避免 10 万行全量 IPC）。
    pub fn edit_result(&self) -> EditResult {
        let errors = match (&self.selected, self.engine.active_project_id()) {
            (Some(sel), Some(pid)) if !sel.name.is_empty() => self
                .engine
                .validation_errors
                .iter()
                .filter(|(p, g, n, _, _)| {
                    p == pid && g == &sel.group && n == &sel.name
                })
                .map(|(_, _, _, r, c)| [*r, *c])
                .collect(),
            _ => Vec::new(),
        };
        EditResult {
            status: self.status(),
            tree: self.tree(),
            issues: self.issues(),
            errors,
            display: None,
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
                label: if self.engine.global_config.ui.as_ref().is_some_and(|u| u.show_meta_id) {
                    pid.clone()
                } else {
                    pname.clone()
                },
                search_key: format!(
                    "{}\u{0}{}",
                    tablet_core::search::search_key(&pname),
                    tablet_core::search::search_key(&pid)
                ),
                mark: pm,
                mark_color: pc,
                closed: false,
                active: active.as_deref() == Some(pid.as_str()),
                xlsx: false,
            });

            for g in &p.groups {
                let (mut gm, mut gc) = marker(
                    false,
                    g.is_new,
                    g.is_dirty(),
                    self.engine.has_group_error(&pid, &g.name),
                );
                if gm.is_empty() && g.tables.iter().any(|t| t.schema.fields.iter().any(|f| f.is_aux())) {
                    gm = "普".into();
                    gc = "#b45309".into();
                }
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
                    let has_aux = t.schema.fields.iter().any(|f| f.is_aux());
                    self.push_node(&mut rows, &pid, &pname, &g.name, &t.name, SelKind::Table,
                        t.deleted, !t.saved, t.dirty, has_aux);
                }
                for c in &g.constants {
                    self.push_node(&mut rows, &pid, &pname, &g.name, &c.name, SelKind::Constant,
                        c.deleted, !c.saved, c.dirty, false);
                }
                for e in &g.enums {
                    self.push_node(&mut rows, &pid, &pname, &g.name, &e.name, SelKind::Enum,
                        e.deleted, !e.saved, e.dirty, false);
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
                label: if self.engine.global_config.ui.as_ref().is_some_and(|u| u.show_meta_id) {
                    ap.id.clone()
                } else {
                    ap.name.clone()
                },
                search_key: format!(
                    "{}\u{0}{}",
                    tablet_core::search::search_key(&ap.name),
                    tablet_core::search::search_key(&ap.id)
                ),
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
        has_aux: bool,
    ) {
        let has_err = self.engine.has_node_error(pid, group, name);
        let (mut m, mut c) = marker(deleted, new, dirty, has_err);
        if m.is_empty() && has_aux {
            m = "普".into();
            c = "#b45309".into();
        }
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
            // 保存只作用于 active project，脏标记也必须只反映它（与旧版行为一致）。
            dirty: self.engine.active().is_some_and(|p| p.is_dirty()),
            can_undo: self.engine.can_undo(),
            can_redo: self.engine.can_redo(),
            can_revert,
            undo_label: self.engine.undo_label().map(str::to_string),
            redo_label: self.engine.redo_label().map(str::to_string),
            error_count: self.engine.validation_errors.len(),
            warning_count: self.engine.validation_warnings.len(),
            show_ref_name: self.show_ref_name,
            auto_commit_on_blur: self.engine.global_config.ui.as_ref().map(|u| u.auto_commit_on_blur).unwrap_or(true),
            show_meta_id: self
                .engine
                .global_config
                .ui
                .as_ref()
                .map(|u| u.show_meta_id)
                .unwrap_or(false),
            project_sort: self
                .engine
                .global_config
                .project_management
                .project_sort
                .as_str()
                .to_string(),
            univer_editing: self
                .univer_open_groups
                .iter()
                .filter(|(p, _)| Some(p.as_str()) == self.engine.active_project_id())
                .map(|(_, g)| g.clone())
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
        let was_opened = self.engine.is_opened(project);
        if !was_opened {
            let _ = self.engine.open_project(project);
        }
        let active_before = self.engine.active_project_id().map(str::to_string);
        let _ = self.engine.set_active_by_id(project);
        // 首次打开 / 切换 active 项目时写回项目集合与 last_project。节点间切换不写，
        // 避免每点一个格子/节点就重写 tablet.toml。
        if !was_opened || active_before.as_deref() != Some(project) {
            self.persist_project_layout();
        }

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

    pub fn set_project_sort(&mut self, sort: &str) -> Result<(), String> {
        use tablet_core::enums::ProjectSort;
        let value = match sort {
            "id" => ProjectSort::Id,
            "name" => ProjectSort::Name,
            "open" => ProjectSort::Open,
            "created" => ProjectSort::Created,
            "manual" => ProjectSort::Manual,
            other => return Err(format!("未知排序方式: {other}")),
        };
        self.engine.global_config.project_management.project_sort = value;
        let pm = self.engine.global_config.project_management.clone();
        self.engine
            .persist_global_config(pm.project_sort, &pm.project_order)
            .map_err(|e| e.to_string())
    }

    // ── 视图偏好（列宽）─────────────────────────────────────
    fn view_state(
        &self,
        group: &str,
        kind: &str,
        name: &str,
    ) -> (bool, std::collections::BTreeMap<String, f32>) {
        let Some(project) = self.engine.active_project_id() else {
            return (true, std::collections::BTreeMap::new());
        };
        let kind = match kind {
            "constant" => tablet_core::local_state::NodeKind::Constant,
            "enum" => tablet_core::local_state::NodeKind::Enum,
            _ => tablet_core::local_state::NodeKind::Table,
        };
        let key = tablet_core::local_state::NodeKey {
            project: project.to_string(),
            group: group.to_string(),
            kind,
            node: name.to_string(),
        };
        self.user_data
            .ui
            .grid
            .nodes
            .get(&key)
            .map(|v| (v.auto, v.widths.clone()))
            .unwrap_or((true, std::collections::BTreeMap::new()))
    }

    /// 主界面列宽变化：写入 `.data/user-data.json`，不参与项目 dirty。
    pub fn save_grid_view(
        &mut self,
        project: &str,
        group: &str,
        kind: &str,
        name: &str,
        auto: bool,
        widths: std::collections::BTreeMap<String, f32>,
    ) -> Result<(), String> {
        let kind = match kind {
            "constant" => tablet_core::local_state::NodeKind::Constant,
            "enum" => tablet_core::local_state::NodeKind::Enum,
            "table" => tablet_core::local_state::NodeKind::Table,
            other => return Err(format!("未知节点类型: {other}")),
        };
        let widths = widths
            .into_iter()
            .map(|(k, w)| {
                (
                    k,
                    w.clamp(
                        tablet_core::local_state::MIN_MANUAL_WIDTH,
                        tablet_core::local_state::MAX_MANUAL_WIDTH,
                    ),
                )
            })
            .collect();
        let key = tablet_core::local_state::NodeKey {
            project: project.to_string(),
            group: group.to_string(),
            kind,
            node: name.to_string(),
        };
        let node = self.user_data.node_mut(key);
        node.auto = auto;
        node.widths = widths;
        self.user_data
            .save(&self.user_data_path)
            .map_err(|e| format!("保存列宽失败: {e}"))
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
        let export: Vec<String> = fields
            .iter()
            .map(|f| {
                f.aux
                    .as_ref()
                    .map(|m| one_line(&m.raw_export))
                    .unwrap_or_else(|| f.export.display().to_string())
            })
            .collect();
        let types: Vec<String> = fields.iter().map(|f| one_line(&f.tbl_type)).collect();
        let names: Vec<String> = fields.iter().map(|f| f.label().to_string()).collect();

        let mut kinds = Vec::with_capacity(fields.len());
        for (i, f) in fields.iter().enumerate() {
            kinds.push(if f.is_aux() {
                "aux".to_string()
            } else if i == 0 {
                "text".to_string() // id 列数据可编辑（输入新行），表头才是只读
            } else if is_ref_type(&f.tbl_type) {
                "ref".to_string()
            } else {
                "text".to_string()
            });
        }

        // id 列的表头锁定只针对真正的 tablet id；辅助列（含不合规的 id）允许编辑修复。
        let id_locked = |i: usize| fields.get(i).is_some_and(|f| !f.is_aux() && f.name == "id");
        let header_kinds = vec![
            vec!["text".to_string(); fields.len()],
            fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    if id_locked(i) { "readonly" }
                    else if f.is_aux() { "text" }
                    else { "export" }
                }.to_string())
                .collect(),
            fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    if id_locked(i) { "readonly" }
                    else if f.is_aux() { "text" }
                    else { "type" }
                }.to_string())
                .collect(),
            fields
                .iter()
                .enumerate()
                .map(|(i, _)| if id_locked(i) { "readonly" } else { "text" }.to_string())
                .collect(),
        ];

        let mut rows = Vec::with_capacity(valid_count + EXTRA_ROWS);
        let mut display_rows = Vec::new();
        let mut errors = Vec::new();
        let mut ref_cache = std::collections::HashMap::new();
        for r in 0..valid_count + EXTRA_ROWS {
            let mut row = Vec::with_capacity(fields.len());
            let mut display_row = Vec::with_capacity(fields.len());
            for c in 0..fields.len() {
                let text = table.records.get(r).and_then(|rec| rec.get(c)).cloned().unwrap_or_default();
                if self.engine.has_active_cell_error(group, name, r, c) {
                    errors.push([r, c]);
                }
                // rows 始终是磁盘原值（引用写 id）；展示值单独给前端渲染。
                // 复制/粘贴/公式栏必须用原值，否则会把显示名写回引用格。
                let display = if self.show_ref_name && !fields[c].is_aux() {
                    self.display_ref_value(&fields[c].tbl_type, &text, &mut ref_cache)
                } else {
                    text.clone()
                };
                row.push(text);
                display_row.push(display);
            }
            rows.push(row);
            if self.show_ref_name {
                display_rows.push(display_row);
            }
        }

        // 「被引用展示列」下拉自带一个 ID（空值 = 原始 id），不能再把 id 列列一遍，
        // 否则会出现 `ID id ...` 两项。
        let display_options = fields
            .iter()
            .filter(|f| !f.is_aux() && f.name != "id" && matches!(f.tbl_type.trim(), "str" | "int" | "txt"))
            .map(|f| f.name.clone())
            .collect();

        let seps = self
            .engine
            .active()
            .map(|p| p.schema.separators.clone())
            .unwrap_or_default();
        let mut ref_cols = Vec::with_capacity(fields.len());
        let mut ref_list = Vec::with_capacity(fields.len());
        let mut ref_seps = Vec::with_capacity(fields.len());
        for f in fields {
            match ref_target_of(&f.tbl_type) {
                Some((target, multi)) if !f.is_aux() => {
                    ref_cols.push(Some(target));
                    ref_list.push(multi);
                    ref_seps.push(if multi {
                        Some(ref_multi_sep(&f.tbl_type, &seps).unwrap_or(";").to_string())
                    } else {
                        None
                    });
                }
                _ => {
                    ref_cols.push(None);
                    ref_list.push(false);
                    ref_seps.push(None);
                }
            }
        }

        let (auto_width, column_widths) = self.view_state(group, "table", name);
        Some(GridPayload {
            mode: "table".into(),
            title: format!("📊 {group} / {name}"),
            subtitle: format!("Table · {valid_count} 行 · {} 列", fields.len()),
            headers: vec![desc, export, types, names],
            header_kinds,
            kinds,
            rows,
            display_rows,
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
            ref_seps,
            auto_width,
            column_widths,
        })
    }

    fn constant_grid(&self, group: &str, name: &str) -> Option<GridPayload> {
        let constant = self.engine.find_constant(group, name)?;
        let valid_count = valid_constant_rows(constant);
        let mut rows = Vec::with_capacity(valid_count + EXTRA_ROWS);
        let mut display_rows = Vec::new();
        let mut errors = Vec::new();
        let mut ref_cache = std::collections::HashMap::new();
        for r in 0..valid_count + EXTRA_ROWS {
            let entry = constant.entries.get(r);
            if entry.is_some() && self.engine.has_active_cell_error(group, name, r, 2) {
                errors.push([r, 2]);
            }
            rows.push(match entry {
                Some(e) => vec![
                    e.name.clone(),
                    e.tbl_type.clone(),
                    e.value.clone(),
                    e.export.display().to_string(),
                    e.desc.clone(),
                ],
                None => vec![String::new(); 5],
            });
            if self.show_ref_name {
                display_rows.push(match entry {
                    Some(e) => vec![
                        e.name.clone(),
                        e.tbl_type.clone(),
                        self.display_ref_value(&e.tbl_type, &e.value, &mut ref_cache),
                        e.export.display().to_string(),
                        e.desc.clone(),
                    ],
                    None => vec![String::new(); 5],
                });
            }
        }
        let (auto_width, column_widths) = self.view_state(group, "constant", name);
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
            display_rows,
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
            ref_seps: Vec::new(),
            auto_width,
            column_widths,
        })
    }

    fn enum_grid(&self, group: &str, name: &str) -> Option<GridPayload> {
        let e = self.engine.find_enum(group, name)?;
        let valid_count = e.entries.iter().filter(|x| !x.id.is_empty() || !x.name.is_empty()).count();
        let mut rows = Vec::with_capacity(valid_count + EXTRA_ROWS);
        let display_rows = Vec::new();
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
        let (auto_width, column_widths) = self.view_state(group, "enum", name);
        Some(GridPayload {
            mode: "enum".into(),
            title: format!("🔢 {group} / {name}"),
            subtitle: format!("Enum · {valid_count} 项"),
            headers: vec![vec!["id".into(), "name".into(), "desc".into()]],
            header_kinds: vec![vec!["readonly".to_string(); 3]],
            kinds: vec!["text".into(), "text".into(), "text".into()],
            rows,
            display_rows,
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
            ref_seps: Vec::new(),
            auto_width,
            column_widths,
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

    /// 编辑后是否立即重算校验，跟全局设置 `[ui] realtime_validate` 走。
    fn revalidate_live(&mut self, group: &str, name: &str) {
        if self.engine.global_config.ui.as_ref().is_some_and(|u| u.realtime_validate) {
            self.engine.revalidate(group, name);
        }
    }

    /// 提交一个格子；返回该格的最新展示值（开启「显示引用名称」时用于就地刷新）。
    pub fn commit_cell(&mut self, row: usize, col: usize, value: String) -> Option<String> {
        let Some((group, name, kind)) = self.selected_node() else { return None };
        let (group, name) = (group.to_string(), name.to_string());
        match kind {
            SelKind::Table => self.engine.commit_table_cell(&group, &name, row, col, value),
            SelKind::Constant => self.engine.commit_constant_cell(&group, &name, row, col, value),
            SelKind::Enum => self.engine.commit_enum_cell(&group, &name, row, col, value),
            _ => {}
        }
        self.revalidate_live(&group, &name);
        self.cell_display_value(&group, &name, kind, row, col)
    }

    /// 单格的展示文本（引用列 = 目标表的展示列）；未开启「显示引用名称」时为 None。
    fn cell_display_value(
        &self,
        group: &str,
        name: &str,
        kind: SelKind,
        row: usize,
        col: usize,
    ) -> Option<String> {
        if !self.show_ref_name {
            return None;
        }
        let mut cache = std::collections::HashMap::new();
        match kind {
            SelKind::Table => {
                let table = self.engine.find_table(group, name)?;
                let ty = table.schema.fields.get(col)?.tbl_type.clone();
                let raw = table.records.get(row).and_then(|r| r.get(col)).cloned().unwrap_or_default();
                Some(self.display_ref_value(&ty, &raw, &mut cache))
            }
            SelKind::Constant => {
                // Constant 只有 value 列（索引 2）可能带类型引用。
                if col != 2 {
                    return None;
                }
                let constant = self.engine.find_constant(group, name)?;
                let entry = constant.entries.get(row)?;
                Some(self.display_ref_value(&entry.tbl_type, &entry.value, &mut cache))
            }
            // Enum 没有引用列。
            _ => None,
        }
    }

    /// 手动把一列降级为辅助列 / 升级为普通列。
    pub fn set_column_aux(&mut self, col: usize, aux: bool) -> Result<bool, String> {
        let Some((group, name, SelKind::Table)) = self.selected_node() else {
            return Err("请先选中一张表".to_string());
        };
        let (group, name) = (group.to_string(), name.to_string());
        let did = self.engine.set_table_column_aux(&group, &name, col, aux)?;
        if did {
            self.revalidate_live(&group, &name);
        }
        Ok(did)
    }

    pub fn commit_header(&mut self, header_row: usize, col: usize, value: String) {
        let Some((group, name, SelKind::Table)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        self.engine.commit_header_edit(&group, &name, header_row, col, value);
        self.revalidate_live(&group, &name);
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
        self.revalidate_live(&group, &name);
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
        self.revalidate_live(&group, &name);
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
        self.revalidate_live(&group, &name);
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
        self.revalidate_live(&group, &name);
    }

    pub fn insert_column(&mut self, at: usize) {
        let Some((group, name, SelKind::Table)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        self.engine.insert_column(&group, &name, at);
        self.revalidate_live(&group, &name);
    }

    pub fn delete_column(&mut self, at: usize) {
        let Some((group, name, SelKind::Table)) = self.selected_node() else { return };
        let (group, name) = (group.to_string(), name.to_string());
        self.engine.delete_column(&group, &name, at);
        self.revalidate_live(&group, &name);
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

    /// 树面板「显示 ID」勾选框：与全局设置里的同一项同步，立即写回 tablet.toml。
    pub fn set_show_meta_id(&mut self, on: bool) -> Result<(), String> {
        let ui = self.engine.global_config.ui.get_or_insert_with(UiConfig::default);
        ui.show_meta_id = on;
        let pm = self.engine.global_config.project_management.clone();
        self.engine
            .persist_global_config(pm.project_sort, &pm.project_order)
            .map_err(|e| e.to_string())
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
        let mut lines = Vec::new();
        let mut ok = true;
        if f.json {
            push_export(&mut lines, &mut ok, "JSON", self.engine.export_json());
        }
        if f.xml {
            push_export(&mut lines, &mut ok, "XML", self.engine.export_xml());
        }
        if f.bin {
            let filter = tablet_core::export::DataFilter::default();
            push_export(&mut lines, &mut ok, "BIN", self.engine.export_bin_filtered(&filter, true));
        }
        if f.bin_split {
            let filter = tablet_core::export::DataFilter::default();
            push_export(&mut lines, &mut ok, "BIN (split)", self.engine.export_bin_filtered(&filter, false));
        }
        if f.java {
            push_export(&mut lines, &mut ok, "Java", self.engine.export_java());
        }
        if f.go {
            push_export(&mut lines, &mut ok, "Go", self.engine.export_go());
        }
        if f.cpp {
            push_export(&mut lines, &mut ok, "C++", self.engine.export_cpp());
        }
        if f.csharp_dotnet {
            push_export(&mut lines, &mut ok, "C# (.NET)", self.engine.export_csharp_dotnet());
        }
        if f.typescript_server {
            push_export(
                &mut lines,
                &mut ok,
                "TypeScript (Node.js)",
                self.engine.export_typescript_server(),
            );
        }
        if f.lua {
            push_export(&mut lines, &mut ok, "Lua", self.engine.export_lua());
        }
        if f.gdscript {
            push_export(&mut lines, &mut ok, "GDScript", self.engine.export_gdscript());
        }
        if f.typescript_client {
            push_export(
                &mut lines,
                &mut ok,
                "TypeScript (前端)",
                self.engine.export_typescript_client(),
            );
        }
        if f.csharp_unity {
            push_export(&mut lines, &mut ok, "C# (Unity)", self.engine.export_csharp_unity());
        }
        if f.csharp_godot {
            push_export(&mut lines, &mut ok, "C# (Godot)", self.engine.export_csharp_godot());
        }
        if lines.is_empty() {
            ok = false;
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
        let groups: Vec<String> = groups
            .into_iter()
            .filter(|g| !g.trim().is_empty())
            .collect();
        let nodes: Vec<String> = nodes
            .into_iter()
            .filter(|n| !n.trim().is_empty())
            .collect();

        let full = tablet_core::schema_projection::schema_from_project(&project.groups, with_preset);
        for g in &groups {
            if !full.sections.iter().any(|s| &s.group == g) {
                return Err(format!("分组不存在或没有可导出的节点: {g}"));
            }
        }
        let mut wanted: Vec<(String, String)> = Vec::new();
        for spec in &nodes {
            let Some((g, name)) = spec.split_once('/') else {
                return Err(format!("节点参数需要 Group/Name，收到 {spec:?}"));
            };
            if !full
                .sections
                .iter()
                .any(|s| s.group == g && s.name == name)
            {
                return Err(format!("节点不存在: {spec}"));
            }
            wanted.push((g.to_string(), name.to_string()));
        }
        let all = groups.is_empty() && wanted.is_empty();
        let sections: Vec<_> = full
            .sections
            .into_iter()
            .filter(|s| {
                all
                    || groups.iter().any(|g| g == &s.group)
                    || wanted.iter().any(|(g, n)| g == &s.group && n == &s.name)
            })
            .collect();
        if sections.is_empty() {
            return Err("没有匹配的节点可导出".to_string());
        }

        let mut meta = project.schema.meta.clone();
        if let Some(v) = meta_id {
            meta.id = v.trim().to_string();
        }
        if let Some(v) = meta_name {
            meta.name = v.trim().to_string();
        }
        if let Some(v) = meta_category {
            meta.category = v.trim().to_string();
        }
        if let Some(v) = meta_version {
            meta.version = v.trim().to_string();
        }
        if !meta.id.is_empty()
            && !tablet_core::schema_model::is_valid_metadata_id(&meta.id)
        {
            return Err(format!("非法 meta id: {}", meta.id));
        }
        if meta.name.is_empty() && !meta.id.is_empty() {
            meta.name = meta.id.clone();
        }
        let schema = tablet_core::schema_model::TblSchema {
            meta,
            separators: Default::default(),
            sections,
        };
        let content = tablet_core::tblschema::serialize_tblschema(&schema);

        let (target, template) = if as_template {
            let id = schema.meta.id.clone();
            if id.is_empty() {
                return Err("保存为模板需要 meta id".to_string());
            }
            let dir = tablet_core::template::default_local_dir();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            (dir.join(format!("{id}.tblschema")), true)
        } else {
            (resolve_in_project(&project.project_root, &path), false)
        };
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        std::fs::write(&target, content).map_err(|e| e.to_string())?;
        Ok(format!(
            "已{} {} 个段 → {}",
            if template { "保存为本地模板" } else { "导出" },
            schema.sections.len(),
            target.to_string_lossy().replace('\\', "/")
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

    // ── Excel 导出 / 导入（GUI 直接走 core，不依赖 CLI action 层）────
    /// 把 xlsx 解析成 GroupPatch 并只应用到内存；返回修补的节点数。
    fn apply_workbook_in_memory(
        &mut self,
        project: &str,
        group: &str,
        file: &Path,
        label: &str,
    ) -> Result<usize, String> {
        let patch = {
            let g = self
                .engine
                .find_project(project)
                .and_then(|p| p.groups.iter().find(|g| g.name == group))
                .ok_or_else(|| format!("找不到分组: {project}::{group}"))?;
            import_xlsx_into_group(file, g).map_err(|e| e.to_string())?
        };
        let nodes = patch.patches.len();
        let applied = self.engine.edit_project(project, label, |p| {
            let Some(g) = p.groups.iter_mut().find(|g| g.name == group) else {
                return false;
            };
            apply_group_patch(g, &patch);
            p.resync_schema();
            true
        });
        if applied != Some(true) {
            return Err("回写失败：Project 或分组已不存在".to_string());
        }
        self.engine.revalidate_all_projects();
        Ok(nodes)
    }

    /// 导出对话框：选中多个组 / 节点，生成一本 xlsx。
    pub fn excel_export_selected(
        &self,
        path: String,
        selections: Vec<ExcelExportSelection>,
    ) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let out = resolve_in_project(&project.project_root, &path);
        let picks: Vec<(String, Vec<String>)> = selections
            .into_iter()
            .map(|s| (s.group, s.names))
            .collect();
        let bytes = tablet_core::excel::export_selected_book(project, &picks)
            .map_err(|e| format!("{e:#}"))?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&out, &bytes).map_err(|e| e.to_string())?;
        Ok(format!("已导出 {}（{} 字节）", out.display(), bytes.len()))
    }

    /// 导入对话框：打开工作簿，识别每张 sheet 的类型与规模。
    pub fn excel_inspect_workbook(&self, path: &str) -> Result<Vec<ExcelSheetInspect>, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, path);
        let infos = tablet_core::excel_mapping::sheet_infos(&file)
            .map_err(|e| format!("{e:#}"))?;
        let mut out = Vec::with_capacity(infos.len());
        for info in infos {
            match tablet_core::excel_mapping::detect_layout(&file, &info.name) {
                Ok(d) => out.push(ExcelSheetInspect {
                    name: info.name,
                    kind: d.kind.as_str().to_string(),
                    header_row: d.header_row,
                    data_start: d.data_start,
                    desc_row: d.desc_row,
                    type_row: d.type_row,
                    export_row: d.export_row,
                    columns: info.columns,
                    rows: info.rows,
                    error: None,
                }),
                Err(e) => out.push(ExcelSheetInspect {
                    name: info.name,
                    kind: "unknown".to_string(),
                    header_row: 0,
                    data_start: 0,
                    desc_row: 0,
                    type_row: 0,
                    export_row: 0,
                    columns: info.columns,
                    rows: info.rows,
                    error: Some(format!("{e:#}")),
                }),
            }
        }
        Ok(out)
    }

    /// 导入对话框：一张工作簿的多个 sheet 写到多个组 / 节点。
    ///
    /// 解析 / 校验全部在 core 的单个事务里完成；整个导入只产生一个撤销步骤。
    pub fn excel_import_multi(&mut self, req: ExcelImportRequest) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let pid = project.schema.meta.id.clone();
        let file = resolve_in_project(&project.project_root, &req.path);
        if req.rows.is_empty() {
            return Err("没有选中要导入的 sheet".to_string());
        }

        let mut specs = Vec::with_capacity(req.rows.len());
        for row in &req.rows {
            let d = tablet_core::excel_mapping::detect_layout(&file, &row.sheet)
                .map_err(|e| format!("sheet {}: {e:#}", row.sheet))?;
            let preview = tablet_core::excel_mapping::read_sheet_preview(
                &file,
                &row.sheet,
                d.header_row,
                d.data_start,
                60,
            )
            .map_err(|e| format!("sheet {}: {e:#}", row.sheet))?;
            let fields = if d.kind == tablet_core::excel_mapping::MappedNodeKind::Table {
                tablet_core::excel_mapping::suggest_fields(&preview)
            } else {
                tablet_core::excel_mapping::suggest_entry_fields(&preview.columns, d.kind)
            };
            specs.push(tablet_core::ops::ExcelNodeSpec {
                sheet: row.sheet.clone(),
                group_name: row.group_name.clone(),
                node_name: row.node_name.clone(),
                create_group: row.create_group,
                kind: d.kind,
                header_row: d.header_row,
                data_start: d.data_start,
                desc_row: d.desc_row,
                type_row: d.type_row,
                export_row: d.export_row,
                fields,
            });
        }

        // 编辑中的组不能导入。
        for spec in &specs {
            if self
                .univer_open_groups
                .iter()
                .any(|(p, g)| p == &pid && g.eq_ignore_ascii_case(&spec.group_name))
            {
                return Err(format!(
                    "分组「{}」正在 Univer 窗口中编辑，主界面暂时只读",
                    spec.group_name
                ));
            }
        }

        let n = self.engine.import_excel_nodes(&pid, &file, &specs)?;
        Ok(format!("已导入 {n} 个 sheet；保存后落盘"))
    }

    pub fn excel_preview(
        &self,
        path: String,
        sheet: Option<String>,
        header_row: Option<u32>,
        data_start: Option<u32>,
        desc_row: Option<u32>,
        type_row: Option<u32>,
        export_row: Option<u32>,
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
        // 数据起始行可以手动改；低于表头后一行的值直接拒绝，别让预览静默错位。
        let data_start = data_start.unwrap_or(hr + 1).max(hr + 1);
        let preview =
            tablet_core::excel_mapping::read_sheet_preview(&file, &sheet, hr, data_start, 10)
                .map_err(|e| e.to_string())?;
        let desc_row = desc_row.unwrap_or(0);
        let type_row = type_row.unwrap_or(0);
        let export_row = export_row.unwrap_or(0);
        let mut wanted: Vec<u32> = Vec::new();
        for r in [desc_row, type_row, export_row] {
            if r > 0 && !wanted.contains(&r) {
                wanted.push(r);
            }
        }
        let mut by_row: std::collections::HashMap<u32, Vec<String>> =
            std::collections::HashMap::new();
        if !wanted.is_empty() {
            let rows = tablet_core::excel_mapping::read_rows(&file, &sheet, &wanted)
                .map_err(|e| e.to_string())?;
            for (row, values) in wanted.into_iter().zip(rows) {
                by_row.insert(row, values);
            }
        }
        let cell = |row: u32, col: u32| -> String {
            by_row
                .get(&row)
                .and_then(|r| r.get((col as usize).saturating_sub(1)))
                .map(|v| v.trim().to_string())
                .unwrap_or_default()
        };
        let mut fields = tablet_core::excel_mapping::suggest_fields(&preview);
        for f in &mut fields {
            if desc_row > 0 {
                let v = cell(desc_row, f.source_col);
                if !v.is_empty() {
                    f.desc = v;
                }
            }
            if type_row > 0 {
                let v = cell(type_row, f.source_col);
                if !v.is_empty() {
                    // 原样展示；合不合法交给 field_issues / validate_plan。
                    f.tbl_type = v;
                }
            }
            if export_row > 0 {
                let v = cell(export_row, f.source_col);
                if let Some(e) = tablet_core::excel_mapping::parse_export_value(&v) {
                    f.export = e;
                }
            }
        }
        let mut semantic_rows: Vec<ExcelSemanticRow> = Vec::new();
        for (kind, row) in [("desc", desc_row), ("type", type_row), ("export", export_row)] {
            if row == 0 {
                continue;
            }
            semantic_rows.push(ExcelSemanticRow {
                kind: kind.to_string(),
                row,
                values: by_row.get(&row).cloned().unwrap_or_default(),
            });
        }
        semantic_rows.sort_by_key(|r| r.row);
        let fields = fields
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
            desc_row,
            type_row,
            export_row,
            semantic_rows,
            columns: preview.columns,
            fields,
            sample_rows: preview.rows,
        })
    }

    /// 自动识别节点类型（表 / 常量 / 枚举）与字段行、语义行。
    pub fn excel_detect_layout(
        &self,
        path: String,
        sheet: String,
    ) -> Result<ExcelDetectedLayout, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, &path);
        let d = tablet_core::excel_mapping::detect_layout(&file, &sheet)
            .map_err(|e| e.to_string())?;
        Ok(ExcelDetectedLayout {
            kind: d.kind.as_str().to_string(),
            header_row: d.header_row,
            data_start: d.data_start,
            desc_row: d.desc_row,
            type_row: d.type_row,
            export_row: d.export_row,
        })
    }

    /// 尝试识别表头行上方的说明 / 类型 / 导出量行。
    pub fn excel_semantic_rows(
        &self,
        path: String,
        sheet: String,
        header_row: u32,
    ) -> Result<ExcelSemanticRows, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, &path);
        let (desc_row, type_row, export_row) =
            tablet_core::excel_mapping::detect_semantic_rows(&file, &sheet, header_row)
                .map_err(|e| e.to_string())?;
        Ok(ExcelSemanticRows { desc_row, type_row, export_row })
    }

    /// 工作簿里每张 sheet 的名字与行/列数（只读一次文件，供左栏概览）。
    pub fn excel_sheet_info(&self, path: String) -> Result<Vec<ExcelSheetInfo>, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let file = resolve_in_project(&project.project_root, &path);
        tablet_core::excel_mapping::sheet_infos(&file)
            .map_err(|e| e.to_string())
            .map(|infos| {
                infos
                    .into_iter()
                    .map(|i| ExcelSheetInfo { name: i.name, columns: i.columns, rows: i.rows })
                    .collect()
            })
    }

    /// 实时校验当前映射（字段名 / 关键字 / 重复 / id 类型等）。
    ///
    /// 只校验传进来的 fields（前端只传启用列），返回的 index 就是该数组的下标。
    /// 真正的导入仍会走 `validate_plan`；这里只是把同一份 core 规则提前展示给界面。
    pub fn excel_check_mapping(&self, req: ExcelMapRequest) -> Vec<ExcelFieldIssue> {
        let kind = tablet_core::excel_mapping::MappedNodeKind::parse(&req.node_kind)
            .unwrap_or(tablet_core::excel_mapping::MappedNodeKind::Table);
        let fields: Vec<tablet_core::excel_mapping::MappedField> = req
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
        if kind != tablet_core::excel_mapping::MappedNodeKind::Table {
            return tablet_core::excel_mapping::entry_field_issues(kind, &fields)
                .into_iter()
                .map(|i| ExcelFieldIssue { index: i.index, message: i.message })
                .collect();
        }
        let mut issues = tablet_core::excel_mapping::field_issues(&fields, 0)
            .into_iter()
            .map(|i| ExcelFieldIssue { index: i.index, message: i.message })
            .collect::<Vec<_>>();
        if !fields.iter().any(|f| f.target_name.eq_ignore_ascii_case("id")) {
            issues.push(ExcelFieldIssue {
                index: 0,
                message: "缺少 id 列：至少有一列的目标字段要命名为 id".to_string(),
            });
        }
        issues
    }

    pub fn excel_import_mapped(&mut self, req: ExcelMapRequest) -> Result<String, String> {
        let project = self.engine.active().ok_or("没有活跃的 Project")?;
        let pid = project.schema.meta.id.clone();
        if self
            .univer_open_groups
            .contains(&(pid.clone(), req.group_name.clone()))
        {
            return Err(format!(
                "分组「{}」正在 Univer 窗口中编辑，主界面暂时只读",
                req.group_name
            ));
        }
        let file = resolve_in_project(&project.project_root, &req.path);
        let kind = tablet_core::excel_mapping::MappedNodeKind::parse(&req.node_kind)
            .unwrap_or(tablet_core::excel_mapping::MappedNodeKind::Table);
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
            .collect::<Vec<_>>();
        if kind != tablet_core::excel_mapping::MappedNodeKind::Table {
            let plan = tablet_core::excel_mapping::EntryMappingPlan {
                source: file,
                sheet: req.sheet,
                header_row: req.header_row,
                data_start: req.data_start,
                group_name: req.group_name,
                node_name: req.node_name,
                kind,
                fields,
            };
            tablet_core::excel_mapping::validate_entry_plan(&plan).map_err(|e| e.to_string())?;
            let outcome = self.engine.import_mapped_entries(&pid, &plan)?;
            self.engine.revalidate_all_projects();
            return Ok(format!(
                "已导入 {}/{}（{}）",
                outcome.group,
                plan.node_name,
                plan.kind.label()
            ));
        }
        let plan = tablet_core::excel_mapping::MappingPlan {
            source: file,
            sheet: req.sheet,
            header_row: req.header_row,
            data_start: req.data_start,
            desc_row: req.desc_row,
            type_row: req.type_row,
            export_row: req.export_row,
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

    // ── Univer 编辑窗口 ─────────────────────────────────────
    /// 窗口 label：同一个组固定同一个 label，重复点按钮只会聚焦已有窗口。
    pub fn univer_window_label(project: &str, group: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        project.hash(&mut h);
        group.hash(&mut h);
        format!("univer-{:016x}", h.finish())
    }

    /// 建窗口前登记「这个 label 对应哪个组」；窗口加载后凭自己的 label 取。
    pub fn prepare_univer_window(
        &mut self,
        label: &str,
        project: &str,
        group: &str,
    ) -> Result<(), String> {
        let exists = self
            .engine
            .find_project(project)
            .is_some_and(|p| p.groups.iter().any(|g| g.name == group));
        if !exists {
            return Err(format!("找不到分组: {project}::{group}"));
        }
        self.univer_windows.insert(
            label.to_string(),
            (project.to_string(), group.to_string()),
        );
        self.univer_open_groups
            .insert((project.to_string(), group.to_string()));
        Ok(())
    }

    pub fn univer_window_context(&self, label: &str) -> Option<UniverOpenRequest> {
        self.univer_windows
            .get(label)
            .map(|(project, group)| UniverOpenRequest {
                project: project.clone(),
                group: group.clone(),
            })
    }

    pub fn release_univer_window(&mut self, label: &str) {
        if let Some((project, group)) = self.univer_windows.remove(label) {
            self.univer_fingerprints.remove(&(project.clone(), group.clone()));
            self.univer_open_groups.remove(&(project, group));
        }
    }

    /// 导出 Group 工作簿（与「Excel 编辑」同一布局）交给 Univer 窗口。
    /// 同时记录 Group 指纹：回写时用它检测主界面是否在编辑期间改过同一组。
    pub fn univer_load_group(
        &mut self,
        project: &str,
        group: &str,
    ) -> Result<UniverGroupPayload, String> {
        let xlsx_b64 = {
            let p = self
                .engine
                .find_project(project)
                .ok_or_else(|| format!("项目不存在: {project}"))?;
            let g = p
                .groups
                .iter()
                .find(|g| g.name == group)
                .ok_or_else(|| format!("找不到分组: {group}"))?;
            let bytes = tablet_core::excel::export_group_book(g, None)
                .map_err(|e| format!("导出工作簿失败: {e}"))?;
            let fp = group_fingerprint(g);
            self.univer_fingerprints
                .insert((project.to_string(), group.to_string()), fp);
            self.univer_open_groups
                .insert((project.to_string(), group.to_string()));
            base64::engine::general_purpose::STANDARD.encode(bytes)
        };
        Ok(UniverGroupPayload {
            project: project.to_string(),
            group: group.to_string(),
            title: format!("{project} / {group}"),
            xlsx_b64,
        })
    }

    /// 接收 Univer 写回的 xlsx，**只回写到内存**；主界面保存后才落盘。
    ///
    /// 先比对 Group 指纹：加载之后主界面改过同一组，直接拒绝，避免静默覆盖。
    pub fn univer_apply_group(
        &mut self,
        project: &str,
        group: &str,
        xlsx_b64: &str,
    ) -> Result<UniverApplyResult, String> {
        let Some(&baseline) = self
            .univer_fingerprints
            .get(&(project.to_string(), group.to_string()))
        else {
            return Err("该标签没有加载记录，请重新加载".to_string());
        };
        let current = self
            .engine
            .find_project(project)
            .ok_or_else(|| format!("项目不存在: {project}"))?
            .groups
            .iter()
            .find(|g| g.name == group)
            .ok_or_else(|| format!("找不到分组: {group}"))?;
        if group_fingerprint(current) != baseline {
            return Err("主界面已修改该组，Univer 标签数据已过期；请重新加载后再回写".to_string());
        }

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(xlsx_b64)
            .map_err(|e| format!("回写数据损坏: {e}"))?;
        let root = self
            .engine
            .find_project(project)
            .map(|p| p.project_root.clone())
            .ok_or_else(|| format!("项目不存在: {project}"))?;
        let path = root.join(".tbl-cache").join(format!(
            "univer-{group}-{}.xlsx",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;

        let summary = self.apply_workbook_in_memory(project, group, &path, "Univer 回写");
        let _ = std::fs::remove_file(&path);
        let nodes_patched = summary?;

        // 回写后的内存状态是新基线：下一次回写应相对它做冲突检测。
        if let Some(g) = self
            .engine
            .find_project(project)
            .and_then(|p| p.groups.iter().find(|g| g.name == group))
        {
            let fp = group_fingerprint(g);
            self.univer_fingerprints
                .insert((project.to_string(), group.to_string()), fp);
        }
        Ok(UniverApplyResult {
            group: group.to_string(),
            nodes_patched,
        })
    }

    pub fn univer_close_group(&mut self, project: &str, group: &str) {
        self.univer_fingerprints
            .remove(&(project.to_string(), group.to_string()));
        self.univer_open_groups
            .remove(&(project.to_string(), group.to_string()));
    }

    // ── xlsx 组外部修改轮询 ────────────────────────────────
    pub fn xlsx_poll(&self) -> Vec<XlsxExternalChangeUi> {
        self.engine
            .xlsx_external_changes()
            .into_iter()
            .filter(|c| {
                // Univer 正在编辑的组不自动重载，避免覆盖编辑中的内容。
                !self
                    .univer_open_groups
                    .contains(&(c.project_id.clone(), c.group.clone()))
            })
            .map(|c| XlsxExternalChangeUi {
                project: c.project_id,
                group: c.group,
                dirty: c.dirty,
            })
            .collect()
    }

    /// 重新从磁盘读干净的 xlsx 组（外部修改检测用）。
    pub fn reload_xlsx_external(&mut self, project: &str, group: &str) -> Result<(), String> {
        self.engine
            .reload_xlsx_group(project, group)
            .map_err(|e| e.to_string())?;
        self.engine.revalidate_all_projects();
        Ok(())
    }

    /// 主界面当前选中的组是否被 Univer 窗口占用（用于拦截编辑命令）。
    fn locked_group(&self) -> Option<(String, String)> {
        let sel = self.selected.as_ref()?;
        let project = sel.project.clone();
        let group = sel.group.clone();
        let locked = self
            .univer_open_groups
            .contains(&(project.clone(), group.clone()));
        locked.then_some((project, group))
    }

    pub fn ensure_unlocked(&self) -> Result<(), String> {
        if let Some((_, group)) = self.locked_group() {
            Err(format!(
                "分组「{group}」正在 Univer 中编辑，主界面暂时只读"
            ))
        } else {
            Ok(())
        }
    }

    pub fn ensure_group_unlocked(&self, project: &str, group: &str) -> Result<(), String> {
        if self
            .univer_open_groups
            .contains(&(project.to_string(), group.to_string()))
        {
            Err(format!(
                "分组「{group}」正在 Univer 窗口中编辑，主界面暂时只读"
            ))
        } else {
            Ok(())
        }
    }

    fn project_locked(&self, project: &str) -> Option<String> {
        self.univer_open_groups
            .iter()
            .find(|(p, _)| p == project)
            .map(|(_, g)| g.clone())
    }

    pub fn ensure_project_unlocked(&self, project: &str) -> Result<(), String> {
        if let Some(group) = self.project_locked(project) {
            Err(format!(
                "分组「{group}」正在 Univer 窗口中编辑，暂不能执行项目级操作"
            ))
        } else {
            Ok(())
        }
    }

    pub fn ensure_active_project_unlocked(&self) -> Result<(), String> {
        match self.engine.active_project_id() {
            Some(pid) => self.ensure_project_unlocked(pid),
            None => Ok(()),
        }
    }

    /// 会丢弃/重建整个工作区的操作：只要有 Univer 窗口开着就拒绝。
    pub fn ensure_no_univer_windows(&self) -> Result<(), String> {
        if let Some((_, group)) = self.univer_open_groups.iter().next() {
            Err(format!(
                "分组「{group}」正在 Univer 窗口中编辑，先关闭 Univer 窗口再重新加载"
            ))
        } else {
            Ok(())
        }
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
                let col = t.schema.fields.iter().position(|f| !f.is_aux() && f.name == field)?;
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
                let id_idx = t.schema.fields.iter().position(|f| !f.is_aux() && f.name == "id");
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
                        .filter(|(_, f)| !f.is_aux() && f.name != "id" && !matches!(f.export, Export::None))
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    // 智能：跳过引用列 / 复合类型 / 不导出的列，最多取 2 个辅助列
                    let mut picked = Vec::new();
                    for (i, f) in t.schema.fields.iter().enumerate() {
                        if f.is_aux() || f.name == "id" || matches!(f.export, Export::None) { continue; }
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
        // 目标不存在：不报错，选择器里显示警告并禁用确定（与旧版行为一致）。
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
    ) -> String {
        if raw.is_empty() {
            return String::new();
        }
        let Some(tt) = TblType::parse(ty) else { return raw.to_string() };
        let targets = tt.ref_targets();
        if targets.is_empty() {
            return raw.to_string();
        }
        // 一次把用到的目标映射都取出来（Map 的键 / 值可能引用不同节点）。
        let mut maps: HashMap<String, HashMap<String, String>> = HashMap::new();
        for target in targets {
            if !cache.contains_key(target) {
                cache.insert(target.to_string(), self.ref_display_map(target));
            }
            if let Some(Some(m)) = cache.get(target) {
                maps.insert(target.to_string(), m.clone());
            }
        }
        if maps.is_empty() {
            return raw.to_string();
        }
        let seps = self
            .engine
            .active()
            .map(|p| p.schema.separators.clone())
            .unwrap_or_default();
        format_ref_display(&tt, raw, &seps, &mut |target, id| {
            maps.get(target)
                .and_then(|m| m.get(id.trim()))
                .cloned()
                .unwrap_or_else(|| id.to_string())
        })
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
                t.schema.fields.iter().filter(|f| !f.is_aux()).map(|f| f.name.clone()).collect::<Vec<_>>(),
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
    /// 不立即落盘：与旧版行为一致，用户点「保存」才写 `projects/<id>/`。
    fn apply_new_project(
        &mut self,
        mut schema: TblSchema,
        id: String,
        name: String,
    ) -> Result<String, String> {
        schema.meta.id = id;
        // 新建项目继承全局分隔符模板（与旧版行为一致）。
        schema.separators = self.engine.global_config.separators.clone();
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
        let template = &self.engine.global_config.separators;
        let separators = SepKey::ALL
            .iter()
            .map(|k| SepItem {
                key: k.as_directive_key().to_string(),
                value: k.get(&p.schema.separators).to_string(),
                default: k.get(template).to_string(),
                original: k.get(&p.original_separators).to_string(),
            })
            .collect();
        let template_export = self
            .engine
            .global_config
            .export
            .clone()
            .unwrap_or_default();
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
            default_export: export_ui(&template_export),
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
    /// 树面板输入框的实时命名校验（新建 / 重命名）。规则完全来自 core 的 `validate_action`，
    /// UI 只负责显示错误，不自己实现命名规则。
    pub fn validate_name(
        &self,
        project: &str,
        group: Option<&str>,
        kind: &str,
        name: &str,
        old_name: Option<&str>,
    ) -> Option<String> {
        let group = group.unwrap_or("").to_string();
        let action = match (kind, old_name) {
            ("group", Some(old)) => ProjectAction::RenameGroup {
                project_id: project.to_string(),
                old_name: old.to_string(),
                new_name: name.to_string(),
            },
            ("group", None) => ProjectAction::NewGroup {
                project_id: project.to_string(),
                name: name.to_string(),
                storage: GroupStorage::Tbl,
            },
            (_, Some(old)) => ProjectAction::RenameNode {
                project_id: project.to_string(),
                group,
                old_name: old.to_string(),
                new_name: name.to_string(),
            },
            ("constant", None) => ProjectAction::NewConstant {
                project_id: project.to_string(),
                group,
                name: name.to_string(),
            },
            ("enum", None) => ProjectAction::NewEnum {
                project_id: project.to_string(),
                group,
                name: name.to_string(),
            },
            _ => ProjectAction::NewTable {
                project_id: project.to_string(),
                group,
                name: name.to_string(),
                fmt_ver: tablet_core::tbl::DEFAULT_TBL_VERSION,
            },
        };
        self.engine.validate_action(&action).err()
    }

    pub fn new_group(&mut self, project: &str, name: String, xlsx: bool) -> Result<(), String> {
        self.engine.execute_action(&ProjectAction::NewGroup {
            project_id: project.to_string(),
            name,
            storage: if xlsx { GroupStorage::Xlsx } else { GroupStorage::Tbl },
        })
    }

    pub fn new_node(&mut self, project: &str, group: &str, kind: &str, name: String, fmt: Option<String>) -> Result<(), String> {
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
        self.engine.execute_action(&action)
    }

    pub fn rename_group(&mut self, project: &str, old_name: String, new_name: String) -> Result<(), String> {
        self.engine.execute_action(&ProjectAction::RenameGroup {
            project_id: project.to_string(),
            old_name,
            new_name,
        })
    }

    pub fn rename_node(&mut self, project: &str, group: &str, old_name: String, new_name: String) -> Result<(), String> {
        self.engine.execute_action(&ProjectAction::RenameNode {
            project_id: project.to_string(),
            group: group.to_string(),
            old_name,
            new_name,
        })
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

    /// 清空引擎日志（否则下一次快照会把旧日志重新合并回来）。
    pub fn clear_logs(&mut self) {
        self.engine.logs.clear();
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

    /// 主动验证（工具栏「验证」/ 问题面板「重新验证」）：重算全部已打开 Project，
    /// 并把明细写进 UI 日志与日志文件。实时校验（编辑时）不走这里，避免刷屏。
    ///
    /// 每次验证前先清掉上一次的验证日志块（UI 面板），连点「重新验证」不会堆出
    /// 多份相同明细；日志文件仍按追加语义保留历史。
    pub fn validate(&mut self) {
        self.clear_previous_validation_logs();
        self.engine.revalidate_all_projects();
        let pids = self.engine.opened_ids();
        let mut total_errors = 0usize;
        let mut total_warnings = 0usize;
        for pid in &pids {
            let issues = self.engine.validate_project_issues(pid);
            let errors = issues
                .iter()
                .filter(|(_, _, e)| e.severity == Severity::Error)
                .count();
            let warnings = issues
                .iter()
                .filter(|(_, _, e)| e.severity == Severity::Warning)
                .count();
            total_errors += errors;
            total_warnings += warnings;
            if errors + warnings > 0 {
                self.log_validation(
                    LogEntryLevel::Info,
                    format!("[验证] 项目 {pid}：{errors} 错误 / {warnings} 警告"),
                );
            }
            for (group, node, err) in issues {
                let level = match err.severity {
                    Severity::Error => LogEntryLevel::Error,
                    Severity::Warning => LogEntryLevel::Warning,
                    Severity::Info => LogEntryLevel::Info,
                };
                self.log_validation(level, format_validation_log(&group, &node, &err));
            }
        }
        if total_errors == 0 && total_warnings == 0 {
            self.log_validation(LogEntryLevel::Info, "验证通过：无错误 / 无警告".to_string());
        } else {
            self.log_validation(
                LogEntryLevel::Info,
                format!("验证完成：{total_errors} 错误 / {total_warnings} 警告"),
            );
        }
    }

    /// 写一条验证日志并记下它，供下一次验证前整体移除。
    fn log_validation(&mut self, level: LogEntryLevel, msg: String) {
        match level {
            LogEntryLevel::Info => self.engine.ui_log(msg.clone()),
            LogEntryLevel::Warning => self.engine.warn_log(msg.clone()),
            LogEntryLevel::Error => self.engine.error_log(msg.clone()),
        }
        self.last_validation_logs.push((level, msg));
    }

    /// 从 UI 日志里移除上一次验证写入的条目（按级别 + 文本从后往前匹配）。
    fn clear_previous_validation_logs(&mut self) {
        let old = std::mem::take(&mut self.last_validation_logs);
        for (level, msg) in old.into_iter().rev() {
            if let Some(i) = self
                .engine
                .logs
                .iter()
                .rposition(|l| l.level == level && l.message == msg)
            {
                self.engine.logs.remove(i);
            }
        }
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
        let opened = self.engine.open_project(project).unwrap_or(false);
        if self.engine.active_project_id().is_none() {
            let _ = self.engine.set_active_by_id(project);
        }
        if opened {
            self.persist_project_layout();
        }
    }

    pub fn close_project(&mut self, project: &str) {
        if self.engine.close_project(project) {
            if self.selected.as_ref().is_some_and(|s| s.project == project) {
                self.selected = None;
            }
            self.persist_project_layout();
        }
    }

    /// 打开 / 关闭项目后把运行时项目集合写回 `tablet.toml`。否则「重新加载」
    /// 读到的是旧的 `opened_projects`，会把其它已打开项目静默关掉。
    fn persist_project_layout(&mut self) {
        let pm = self.engine.global_config.project_management.clone();
        if let Err(e) = self
            .engine
            .persist_global_config(pm.project_sort, &pm.project_order)
        {
            self.engine
                .error_log(format!("保存工作区项目列表失败: {e}"));
        }
    }

    pub fn delete_project(&mut self, project: &str) -> Result<(), String> {
        // 只限制「删除最后一个已打开的项目」；删除关闭项目不受影响。
        if self.engine.find_project(project).is_some() && self.engine.projects.len() <= 1 {
            return Err("至少要保留一个打开的项目".to_string());
        }
        let _ = self.engine.execute_action(&ProjectAction::DeleteProject {
            project_id: project.to_string(),
        });
        if self.selected.as_ref().is_some_and(|s| s.project == project) {
            self.selected = None;
        }
        self.persist_project_layout();
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
    /// 与旧版行为一致：即时重算，不受「实时校验」开关影响。
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

    /// 打开组所在目录（Tbl 组 = config/<group>，Xlsx 组 = config 根）。
    /// 节点右键也复用它：节点文件 / sheet 都落在组目录或其工作簿里。
    pub fn open_group_dir(&self, project: &str, group: &str) -> Result<(), String> {
        let p = self
            .engine
            .find_project(project)
            .ok_or_else(|| format!("项目不存在: {project}"))?;
        let g = p
            .groups
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(group))
            .ok_or_else(|| format!("分组不存在: {group}"))?;
        launch(&g.dir.to_string_lossy())
    }

    pub fn reload_group(&mut self, project: &str, group: &str) -> Result<(), String> {
        self.engine
            .reload_xlsx_group(project, group)
            .map_err(|e| e.to_string())
    }

    pub fn export_template(&self, id: String, name: String) -> Result<String, String> {
        self.schema_export(
            String::new(),
            false,
            Vec::new(),
            Vec::new(),
            Some(id),
            Some(name),
            None,
            None,
            true,
        )?;
        Ok("已保存模板".to_string())
    }
}

// ── 导出设置辅助 ────────────────────────────────────────────
/// 按列类型的结构逐段翻译引用 id，保留原有的分隔符与嵌套形状。
///
/// 与 `TblType::ref_elements` 用同一套分隔符：Map 的键 / 值、Map 的 tuple/list 变体
/// 出现 `@X` 时也都能把 id 换成展示名。非引用部分原样保留。
fn format_ref_display(
    tt: &TblType,
    value: &str,
    sep: &SeparatorsSection,
    lookup: &mut dyn FnMut(&str, &str) -> String,
) -> String {
    let p = &tt.params;
    match &tt.paradigm {
        Paradigm::Ref => {
            let t = value.trim();
            if t.is_empty() {
                String::new()
            } else {
                lookup(tt.ref_name.as_deref().unwrap_or(""), t)
            }
        }
        Paradigm::List => join_parts(value.split(&sep.list), &sep.list, |s| {
            p.first().map(|x| ref_part(x, s, lookup)).unwrap_or_else(|| s.to_string())
        }),
        Paradigm::Set => join_parts(value.split(&sep.set), &sep.set, |s| {
            p.first().map(|x| ref_part(x, s, lookup)).unwrap_or_else(|| s.to_string())
        }),
        Paradigm::Tuple2 => fmt_tuple(value, &sep.tuple2, p, lookup),
        Paradigm::Tuple3 => fmt_tuple(value, &sep.tuple3, p, lookup),
        Paradigm::Tuple4 => fmt_tuple(value, &sep.tuple4, p, lookup),
        Paradigm::Map => fmt_map(value, &sep.map.entry, &sep.map.kv, &p[0], &p[1], lookup),
        Paradigm::ListTuple2 => join_parts(value.split(&sep.list_tuple2.list), &sep.list_tuple2.list, |s| {
            fmt_tuple(s, &sep.list_tuple2.tuple, p, lookup)
        }),
        Paradigm::ListTuple3 => join_parts(value.split(&sep.list_tuple3.list), &sep.list_tuple3.list, |s| {
            fmt_tuple(s, &sep.list_tuple3.tuple, p, lookup)
        }),
        Paradigm::ListTuple4 => join_parts(value.split(&sep.list_tuple4.list), &sep.list_tuple4.list, |s| {
            fmt_tuple(s, &sep.list_tuple4.tuple, p, lookup)
        }),
        Paradigm::MapTuple2 => fmt_map_tuple(value, &sep.map_tuple2.entry, &sep.map_tuple2.kv, &sep.map_tuple2.tuple, &p[0], &p[1..3], lookup),
        Paradigm::MapTuple3 => fmt_map_tuple(value, &sep.map_tuple3.entry, &sep.map_tuple3.kv, &sep.map_tuple3.tuple, &p[0], &p[1..4], lookup),
        Paradigm::MapTuple4 => fmt_map_tuple(value, &sep.map_tuple4.entry, &sep.map_tuple4.kv, &sep.map_tuple4.tuple, &p[0], &p[1..5], lookup),
        Paradigm::ListList => join_parts(value.split(&sep.list_list.list), &sep.list_list.list, |outer| {
            join_parts(outer.split(&sep.list_list.item), &sep.list_list.item, |s| {
                p.first().map(|x| ref_part(x, s, lookup)).unwrap_or_else(|| s.to_string())
            })
        }),
        Paradigm::ListMap => join_parts(value.split(&sep.list_map.list), &sep.list_map.list, |map| {
            join_parts(map.split(&sep.list_map.entry), &sep.list_map.entry, |entry| {
                let kv: Vec<&str> = entry.splitn(2, &sep.list_map.kv).collect();
                if kv.len() == 2 {
                    format!("{}{}{}", ref_part(&p[0], kv[0], lookup), sep.list_map.kv, ref_part(&p[1], kv[1], lookup))
                } else {
                    entry.to_string()
                }
            })
        }),
        Paradigm::MapList => fmt_map_list(value, &sep.map_list.entry, &sep.map_list.kv, &sep.map_list.item, &p[0], &p[1], lookup),
        Paradigm::MapMap => join_parts(value.split(&sep.map_map.entry), &sep.map_map.entry, |entry| {
            let kv: Vec<&str> = entry.splitn(2, &sep.map_map.kv).collect();
            if kv.len() != 2 {
                return entry.to_string();
            }
            let inner = join_parts(kv[1].split(&sep.map_map.inner_entry), &sep.map_map.inner_entry, |item| {
                let ikv: Vec<&str> = item.splitn(2, &sep.map_map.inner_kv).collect();
                if ikv.len() == 2 {
                    format!("{}{}{}", ref_part(&p[1], ikv[0], lookup), sep.map_map.inner_kv, ref_part(&p[2], ikv[1], lookup))
                } else {
                    item.to_string()
                }
            });
            format!("{}{}{}", ref_part(&p[0], kv[0], lookup), sep.map_map.kv, inner)
        }),
        Paradigm::Base => value.to_string(),
    }
}

fn ref_part(param: &TypeParam, raw: &str, lookup: &mut dyn FnMut(&str, &str) -> String) -> String {
    match param {
        TypeParam::Ref(target) => {
            let t = raw.trim();
            if t.is_empty() {
                String::new()
            } else {
                lookup(target, t)
            }
        }
        TypeParam::Base(_) => raw.to_string(),
    }
}

fn join_parts<'a, I>(
    parts: I,
    sep: &str,
    mut f: impl FnMut(&'a str) -> String,
) -> String
where
    I: Iterator<Item = &'a str>,
{
    parts.map(|s| f(s)).collect::<Vec<_>>().join(sep)
}

fn fmt_tuple(
    value: &str,
    tuple_sep: &str,
    params: &[TypeParam],
    lookup: &mut dyn FnMut(&str, &str) -> String,
) -> String {
    value
        .split(tuple_sep)
        .enumerate()
        .map(|(i, s)| {
            params
                .get(i)
                .map(|p| ref_part(p, s, lookup))
                .unwrap_or_else(|| s.to_string())
        })
        .collect::<Vec<_>>()
        .join(tuple_sep)
}

fn fmt_map(
    value: &str,
    entry_sep: &str,
    kv_sep: &str,
    key: &TypeParam,
    val: &TypeParam,
    lookup: &mut dyn FnMut(&str, &str) -> String,
) -> String {
    join_parts(value.split(entry_sep), entry_sep, |entry| {
        let kv: Vec<&str> = entry.splitn(2, kv_sep).collect();
        if kv.len() == 2 {
            format!("{}{}{}", ref_part(key, kv[0], lookup), kv_sep, ref_part(val, kv[1], lookup))
        } else {
            entry.to_string()
        }
    })
}

fn fmt_map_tuple(
    value: &str,
    entry_sep: &str,
    kv_sep: &str,
    tuple_sep: &str,
    key: &TypeParam,
    tuple_params: &[TypeParam],
    lookup: &mut dyn FnMut(&str, &str) -> String,
) -> String {
    join_parts(value.split(entry_sep), entry_sep, |entry| {
        let kv: Vec<&str> = entry.splitn(2, kv_sep).collect();
        if kv.len() == 2 {
            format!(
                "{}{}{}",
                ref_part(key, kv[0], lookup),
                kv_sep,
                fmt_tuple(kv[1], tuple_sep, tuple_params, lookup)
            )
        } else {
            entry.to_string()
        }
    })
}

fn fmt_map_list(
    value: &str,
    entry_sep: &str,
    kv_sep: &str,
    item_sep: &str,
    key: &TypeParam,
    item: &TypeParam,
    lookup: &mut dyn FnMut(&str, &str) -> String,
) -> String {
    join_parts(value.split(entry_sep), entry_sep, |entry| {
        let kv: Vec<&str> = entry.splitn(2, kv_sep).collect();
        if kv.len() == 2 {
            let list = join_parts(kv[1].split(item_sep), item_sep, |s| ref_part(item, s, lookup));
            format!("{}{}{}", ref_part(key, kv[0], lookup), kv_sep, list)
        } else {
            entry.to_string()
        }
    })
}

/// 多值引用列的取值分隔符：`List<@X>` 取项目 `[separators].list`，`Set<@X>` 取 `.set`。
/// 单值引用 / 非引用返回 None。
fn ref_multi_sep<'a>(ty: &str, sep: &'a SeparatorsSection) -> Option<&'a str> {
    let s = ty.trim();
    if s.starts_with("Set<") {
        Some(sep.set.as_str())
    } else if s.starts_with("List<") {
        Some(sep.list.as_str())
    } else {
        None
    }
}

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

/// Group 的内容指纹（结构 + 数据，不含规则 / 展示列等不影响工作簿的设置）。
///
/// 用于 Univer 回写前的并发检测：值不同 = 主界面在标签加载后改过这组。
fn group_fingerprint(group: &tablet_core::model::Group) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    group.name.hash(&mut h);
    for t in &group.tables {
        t.name.hash(&mut h);
        for f in &t.schema.fields {
            f.name.hash(&mut h);
            f.desc.hash(&mut h);
            f.tbl_type.hash(&mut h);
            f.export.code().hash(&mut h);
        }
        for row in &t.records {
            row.hash(&mut h);
        }
    }
    for c in &group.constants {
        c.name.hash(&mut h);
        for e in &c.entries {
            e.name.hash(&mut h);
            e.tbl_type.hash(&mut h);
            e.value.hash(&mut h);
            e.export.code().hash(&mut h);
            e.desc.hash(&mut h);
        }
    }
    for e in &group.enums {
        e.name.hash(&mut h);
        for x in &e.entries {
            x.id.hash(&mut h);
            x.name.hash(&mut h);
            x.desc.hash(&mut h);
        }
    }
    h.finish()
}

/// 启动时清掉上次 Univer 回写留下的临时工作簿（`.tbl-cache/univer-*.xlsx`）。
///
/// 回写正常结束时会立即删除；这里兜底崩溃 / 强杀。占用中的文件静默跳过，
/// 下次启动再清。
fn cleanup_univer_cache(engine: &mut ProjectEngine) {
    let mut removed = 0usize;
    for project in &engine.projects {
        let Ok(entries) = std::fs::read_dir(project.cache_dir()) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let is_univer_temp = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("univer-"))
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"));
            if is_univer_temp && std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        engine.ui_log(format!("[Univer] 已清理 {removed} 个上次回写残留的临时工作簿"));
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
    // 与旧版行为一致：校验错误优先展示（`!`），不被 * / + / - 盖住。
    if error {
        ("!".into(), "#dc3232".into())
    } else if deleted {
        ("-".into(), "#dc3232".into())
    } else if new {
        ("+".into(), "#50a050".into())
    } else if dirty {
        ("*".into(), "#d8a020".into())
    } else {
        (String::new(), String::new())
    }
}

/// 给 vcs 探测留的薄封装：`VcsInfo` 的 title 形态与 旧版标题栏一致。
trait VcsTitle {
    fn title(&self) -> String;
}

impl VcsTitle for tablet_core::vcs::VcsInfo {
    fn title(&self) -> String {
        self.display().unwrap_or_else(|| "无版本库".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用固定前缀模拟 id→展示名映射，`target` 也带上以验证 Map 两侧引用各自取对目标。
    fn tr(ty: &str, value: &str, sep: &SeparatorsSection) -> String {
        let tt = TblType::parse(ty).expect("类型应可解析");
        let mut lookup = |target: &str, id: &str| format!("{target}#{id}");
        format_ref_display(&tt, value, sep, &mut lookup)
    }

    #[test]
    fn map_ref_display_translates_key_and_value() {
        let sep = SeparatorsSection::default();
        // 键是引用：id 换成展示名，值保持原样
        assert_eq!(tr("Map<@Equip,int>", "2001:1;2002:2", &sep), "Equip#2001:1;Equip#2002:2");
        // 值是引用
        assert_eq!(tr("Map<int,@Equip>", "1:2001;2:2002", &sep), "1:Equip#2001;2:Equip#2002");
        // 键值都是不同引用目标
        assert_eq!(tr("Map<@A,@B>", "1:2", &sep), "A#1:B#2");
    }

    #[test]
    fn map_ref_display_follows_project_separators() {
        let mut sep = SeparatorsSection::default();
        sep.map.entry = "#".into();
        assert_eq!(tr("Map<@Equip,int>", "2001:1#2002:2", &sep), "Equip#2001:1#Equip#2002:2");
    }

    #[test]
    fn list_set_and_tuple_ref_display_still_work() {
        let sep = SeparatorsSection::default();
        assert_eq!(tr("@Equip", "2001", &sep), "Equip#2001");
        assert_eq!(tr("List<@Equip>", "2001;2002", &sep), "Equip#2001;Equip#2002");
        assert_eq!(tr("Set<@Equip>", "2001;2002", &sep), "Equip#2001;Equip#2002");
        assert_eq!(tr("Tuple2<@Equip,int>", "2001,7", &sep), "Equip#2001,7");
        assert_eq!(tr("List<Tuple2<@Equip,int>>", "2001,1;2002,2", &sep), "Equip#2001,1;Equip#2002,2");
    }

    /// 所有含引用的范式都要能翻译，避免只修 Map 又漏掉别的复合类型。
    #[test]
    fn every_ref_paradigm_translates() {
        let sep = SeparatorsSection::default();
        // 元组
        assert_eq!(tr("Tuple3<@A,int,@B>", "1,2,3", &sep), "A#1,2,B#3");
        // Map 的 tuple / list 变体
        assert_eq!(
            tr("Map<@A,Tuple2<int,@B>>", "1:2,3;4:5,6", &sep),
            "A#1:2,B#3;A#4:5,B#6"
        );
        assert_eq!(tr("Map<@A,List<@B>>", "1:2,3;4:5", &sep), "A#1:B#2,B#3;A#4:B#5");
        // 嵌套列表 / 列表内 Map / Map 内 Map
        assert_eq!(tr("List<List<@A>>", "1,2;3", &sep), "A#1,A#2;A#3");
        assert_eq!(tr("List<Map<@A,@B>>", "1:2,3:4;5:6", &sep), "A#1:B#2,A#3:B#4;A#5:B#6");
        assert_eq!(
            tr("Map<@A,Map<@B,@C>>", "1:2=3,4=5;6:7=8", &sep),
            "A#1:B#2=C#3,B#4=C#5;A#6:B#7=C#8"
        );
    }

    #[test]
    fn non_ref_parts_are_kept_verbatim() {
        let sep = SeparatorsSection::default();
        // Map 里只有值引用时，键原样保留
        assert_eq!(tr("Map<int,@Equip>", "x:2001", &sep), "x:Equip#2001");
        // 缺 kv 分隔的脏值不丢，原样返回
        assert_eq!(tr("Map<@Equip,int>", "2001", &sep), "2001");
    }
}
