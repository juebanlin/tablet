use std::path::PathBuf;

use crate::enums::{
    Encoding, LineEnding, JsonEmptyAs, XmlEmptyAs, LogLevel,
    PickerTrigger, RefPickerStrategy, ProjectSort,
};
use crate::tblschema::TblSchema;

/// 项目状态：区分已落盘 vs 未落盘
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectState {
    /// 从磁盘加载的已存盘项目
    Loaded,
    /// 新建的未落盘项目（待保存）
    Pending,
}

impl ProjectState {
    pub fn is_loaded(&self) -> bool {
        matches!(self, ProjectState::Loaded)
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, ProjectState::Pending)
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    /// CLI 的 `-w` workdir：`tablet.toml` 与 `gen/` 所在目录。仓库级，全局共享。
    pub workdir: PathBuf,
    /// 当前 Project 的根目录：`project.tblschema` / `config/` 在它下面。
    /// - 多 Project 模式：`<workdir>/projects/<id>/`
    pub project_root: PathBuf,
    /// 项目级原始配置（来自 project.toml），保存时写回此配置。
    pub raw_config: ProjectConfig,
    /// 合并后的最终配置（global + raw + schema），业务逻辑使用。
    pub config: ProjectConfig,
    /// `project.tblschema` 解析结果，承担"项目身份 + 结构骨架"双职：
    /// - `schema.meta.id / name / created_at / source_template* / category / version` = 项目身份
    /// - `schema.sections` = 结构骨架，由 ops 在 group/node 变动时增量同步
    pub schema: TblSchema,
    pub groups: Vec<Group>,
    /// schema 是否需要写盘：rename / 结构变动时置 true，save 时落 project.tblschema 后清零。
    pub schema_dirty: bool,
    /// 项目状态：Loaded（已落盘）或 Pending（待保存）。
    /// save 时 Pending 项目会先 create_dir_all，然后切换为 Loaded。
    pub state: ProjectState,
}

impl Project {
    /// 数据目录：当前 Project 的 .tbl 根目录 `<project_root>/config/`
    pub fn data_dir(&self) -> PathBuf {
        self.project_root.join("config")
    }

    /// 缓存目录：当前 Project 的 .tbl-cache 目录 `<project_root>/.tbl-cache/`
    pub fn cache_dir(&self) -> PathBuf {
        self.project_root.join(".tbl-cache")
    }

    /// 导出根：Project 导出目录 `<project_root>/`（每个 Project 独立 gen/，避免互相覆盖）
    pub fn export_root(&self) -> &std::path::Path {
        self.project_root.as_path()
    }
}

/// `tablet.toml` 反序列化的顶层结构（全局配置）。
/// `[project]` 段对应 `ProjectManagementConfig`（项目管理状态）；
/// 其它段（export / ui / separators）是所有 Project 共享的工作空间默认值。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GlobalConfig {
    /// 项目管理配置段：`[project]`。
    #[serde(default = "default_project_management_section", rename = "project")]
    pub project_management: ProjectManagementConfig,
    #[serde(default)]
    pub export: Option<ExportConfig>,
    #[serde(default)]
    pub ui: Option<UiConfig>,
    #[serde(default)]
    pub separators: crate::types::SeparatorsSection,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            project_management: default_project_management_section(),
            export: Some(ExportConfig::default()),
            ui: Some(UiConfig::default()),
            separators: crate::types::SeparatorsSection::default(),
        }
    }
}

fn default_project_management_section() -> ProjectManagementConfig {
    ProjectManagementConfig {
        last_project: String::new(),
        opened_projects: Vec::new(),
        project_sort: crate::enums::ProjectSort::default(),
        project_order: Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ExportConfig {
    pub json: Option<JsonExport>,
    pub xml: Option<XmlExport>,
    pub server: Option<ServerExport>,
    pub client: Option<ClientConfig>,
    #[serde(default)]
    pub encoding: Option<Encoding>,
    #[serde(default)]
    pub line_ending: Option<LineEnding>,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            json: Some(JsonExport::default()),
            xml: Some(XmlExport::default()),
            server: Some(ServerExport::default()),
            client: Some(ClientConfig::default()),
            encoding: Some(Encoding::Utf8),
            line_ending: Some(LineEnding::Lf),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct JsonExport {
    #[serde(default)]
    pub empty_as: Option<JsonEmptyAs>,
}

impl Default for JsonExport {
    fn default() -> Self {
        Self {
            empty_as: Some(JsonEmptyAs::Null),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct XmlExport {
    #[serde(default)]
    pub empty_as: Option<XmlEmptyAs>,
}

impl Default for XmlExport {
    fn default() -> Self {
        Self {
            empty_as: Some(XmlEmptyAs::Empty),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ServerExport {
    pub data_output: Option<String>,
    pub java: Option<JavaExport>,
    pub go: Option<GoExport>,
    pub cpp: Option<CppExport>,
    pub csharp_dotnet: Option<DotNetExport>,
    pub typescript: Option<ServerTypeScriptExport>,
}

impl Default for ServerExport {
    fn default() -> Self {
        Self {
            data_output: Some("gen/server/data".to_string()),
            java: Some(JavaExport::default()),
            go: Some(GoExport::default()),
            cpp: Some(CppExport::default()),
            csharp_dotnet: Some(DotNetExport::default()),
            typescript: Some(ServerTypeScriptExport::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct JavaExport {
    pub package: Option<String>,
    pub code_output: Option<String>,
}

impl Default for JavaExport {
    fn default() -> Self {
        Self {
            package: Some("com.game.config".to_string()),
            code_output: Some("gen/server/java".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GoExport {
    pub package: Option<String>,
    pub code_output: Option<String>,
}

impl Default for GoExport {
    fn default() -> Self {
        Self {
            package: Some("config".to_string()),
            code_output: Some("gen/server/go".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CppExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,
    #[serde(default)]
    pub json_lib: Option<crate::enums::CppJsonLib>,
}

impl Default for CppExport {
    fn default() -> Self {
        Self {
            namespace: Some("game::config".to_string()),
            code_output: Some("gen/server/cpp".to_string()),
            json_lib: Some(crate::enums::CppJsonLib::Nlohmann),
        }
    }
}

/// .NET 服务端导出
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DotNetExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,
}

impl Default for DotNetExport {
    fn default() -> Self {
        Self {
            namespace: Some("Game.Config.Server".to_string()),
            code_output: Some("gen/server/csharp".to_string()),
        }
    }
}

/// Unity 客户端 C# 导出
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct UnityCSharpExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,
}

impl Default for UnityCSharpExport {
    fn default() -> Self {
        Self {
            namespace: Some("Game.Config.Client".to_string()),
            code_output: Some("gen/client/csharp_unity".to_string()),
        }
    }
}

/// Godot 客户端 C# 导出
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GodotCSharpExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,
}

impl Default for GodotCSharpExport {
    fn default() -> Self {
        Self {
            namespace: Some("Game.Config.Client".to_string()),
            code_output: Some("gen/client/csharp_godot".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClientConfig {
    pub lua: Option<LuaExport>,
    pub gdscript: Option<GdScriptExport>,
    pub typescript: Option<ClientTypeScriptExport>,
    pub csharp_unity: Option<UnityCSharpExport>,
    pub csharp_godot: Option<GodotCSharpExport>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            lua: Some(LuaExport::default()),
            gdscript: Some(GdScriptExport::default()),
            typescript: Some(ClientTypeScriptExport::default()),
            csharp_unity: Some(UnityCSharpExport::default()),
            csharp_godot: Some(GodotCSharpExport::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LuaExport {
    pub output: Option<String>,
}

impl Default for LuaExport {
    fn default() -> Self {
        Self {
            output: Some("gen/client".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GdScriptExport {
    pub output: Option<String>,
}

impl Default for GdScriptExport {
    fn default() -> Self {
        Self {
            output: Some("gen/client/gdscript".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ServerTypeScriptExport {
    pub output: Option<String>,
    pub module_kind: Option<crate::enums::ModuleKind>,
}

impl Default for ServerTypeScriptExport {
    fn default() -> Self {
        Self {
            output: Some("gen/server/typescript".to_string()),
            module_kind: Some(crate::enums::ModuleKind::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClientTypeScriptExport {
    pub output: Option<String>,
    pub module_kind: Option<crate::enums::ModuleKind>,
}

impl Default for ClientTypeScriptExport {
    fn default() -> Self {
        Self {
            output: Some("gen/client/typescript".to_string()),
            module_kind: Some(crate::enums::ModuleKind::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub auto_commit_on_blur: bool,
    #[serde(default)]
    pub realtime_validate: bool,
    #[serde(default)]
    pub log_level: Option<LogLevel>,
    #[serde(default)]
    pub ref_picker: RefPickerConfig,
    /// 表头 picker 单元格（Table type/export 行）的呼出方式：Single | Double
    /// 默认 Single：表头每列一格、几乎不批量改，单击直出选择器更顺手。
    #[serde(default = "default_picker_trigger_header")]
    pub picker_trigger_header: PickerTrigger,
    /// 数据区 picker 单元格（Ref / Constant type / export 列）的呼出方式：Single | Double
    /// 默认 Double：让单击保留为"瞄准选中"，是 Ctrl+C/V 批量复制 ref id / enum 值的前提。
    #[serde(default = "default_picker_trigger_data")]
    pub picker_trigger_data: PickerTrigger,
    /// 模板/Project 列表里展示 id 还是 name；默认 false → 显示 name。
    /// 切换效果类似枚举显示 id/name。Project 实际目录始终用 id；此开关只决定**显示文本**。
    #[serde(default)]
    pub show_meta_id: bool,
    /// 是否允许 Constant 表使用 @Xxx 引用类型。
    /// 默认 true：常量值经常需要指向某条 table/enum 项（如 default_hero = @HeroType:1）。
    /// 设 false 则恢复早期行为，schema 校验会把 constant 段里的 @Xxx 报为 ConstantRefForbidden。
    #[serde(default = "default_true")]
    pub constant_ref_allowed: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            auto_commit_on_blur: true,
            realtime_validate: false,
            log_level: Some(LogLevel::Debug),
            ref_picker: RefPickerConfig::default(),
            picker_trigger_header: PickerTrigger::Single,
            picker_trigger_data: PickerTrigger::Double,
            show_meta_id: false,
            constant_ref_allowed: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RefPickerConfig {
    /// 引用选择弹窗对 Table 的列展示策略：
    /// - Auto（默认）：id + 最多 2 个 export=cs 且类型为字符串的辅助列
    /// - Full：schema 全部字段（除了 export=- 不导出列）
    #[serde(default)]
    pub default_strategy: RefPickerStrategy,
}

fn default_picker_trigger_header() -> PickerTrigger { PickerTrigger::Single }
fn default_picker_trigger_data() -> PickerTrigger { PickerTrigger::Double }

fn default_true() -> bool { true }

/// `[project]` toml 段：仓库级项目管理配置，不随 Project 切换。
///
/// 持有 Project 列表管理状态（启动 last_project / 已打开列表 / 排序）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectManagementConfig {
    /// 启动时进入的 Project id；为空 = 扫到的第一个。
    #[serde(default)]
    pub last_project: String,
    /// 启动时自动打开的 Project id 列表（DBeaver-style 多 Project 工作空间）。
    /// 为空时仅打开 `last_project`。
    #[serde(default)]
    pub opened_projects: Vec<String>,
    /// 项目排序方式：Id / Name / Open / Created / Manual
    #[serde(default)]
    pub project_sort: ProjectSort,
    /// project_sort = Manual 时使用：用户拖拽得到的 id 序列。
    #[serde(default)]
    pub project_order: Vec<String>,
}

/// 项目级配置（来自 project.toml）。
///
/// 用途：
/// - `raw_config`：原始配置，保存时写回 project.toml
/// - `config`：合并后配置（global + raw），业务逻辑使用
///
/// 注意：UI 配置仅在全局级别（GlobalConfig），项目级不支持覆盖。
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct ProjectConfig {
    #[serde(default)]
    pub export: Option<ExportConfig>,
    #[serde(default)]
    pub separators: crate::types::SeparatorsSection,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub dir: PathBuf,
    pub tables: Vec<Table>,
    pub constants: Vec<Constant>,
    pub enums: Vec<EnumDef>,
    pub is_new: bool,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub path: PathBuf,
    pub schema: TableSchema,
    pub records: Vec<Vec<String>>,
    pub dirty: bool,
    pub deleted: bool,
    pub original: String,
}

impl Table {
    pub fn update_dirty(&mut self) {
        self.dirty = crate::tbl::serialize_table(self) != self.original;
    }
}

#[derive(Debug, Clone)]
pub struct TableSchema {
    pub fields: Vec<FieldDef>,
}

impl TableSchema {
    pub fn index_col(&self) -> Option<usize> {
        self.fields.iter().position(|f| f.name == "id")
    }
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub desc: String,
    pub tbl_type: String,
    pub export: Export,
}

#[derive(Debug, Clone)]
pub struct Constant {
    pub name: String,
    pub path: PathBuf,
    pub entries: Vec<ConstEntry>,
    pub dirty: bool,
    pub deleted: bool,
    pub original: String,
}

impl Constant {
    pub fn update_dirty(&mut self) {
        self.dirty = crate::tbl::serialize_constant(self) != self.original;
    }
}

#[derive(Debug, Clone)]
pub struct ConstEntry {
    pub name: String,
    pub tbl_type: String,
    pub value: String,
    pub export: Export,
    pub desc: String,
}

/// 枚举定义：mode=enum 的 .tbl 文件
///
/// 表头固定 id|name|desc 三列，无 #desc/#type/#export/#field 头。
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub path: PathBuf,
    pub entries: Vec<EnumEntry>,
    pub dirty: bool,
    pub deleted: bool,
    pub original: String,
}

impl EnumDef {
    pub fn update_dirty(&mut self) {
        self.dirty = crate::tbl::serialize_enum(self) != self.original;
    }
}

#[derive(Debug, Clone, Default)]
pub struct EnumEntry {
    pub id: String,
    pub name: String,
    pub desc: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Export {
    Unselected,
    ClientServer,
    ClientOnly,
    ServerOnly,
    None,
}

impl Export {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "前后端" | "cs" | "" => Self::ClientServer,
            "客户端" | "c" => Self::ClientOnly,
            "服务器" | "s" => Self::ServerOnly,
            "不导出" | "-" => Self::None,
            _ => Self::Unselected,
        }
    }

    pub fn display(&self) -> &str {
        match self {
            Self::Unselected => "",
            Self::ClientServer => "前后端",
            Self::ClientOnly => "客户端",
            Self::ServerOnly => "服务器",
            Self::None => "不导出",
        }
    }

    pub fn to_tbl(&self) -> &str {
        match self {
            Self::ClientServer | Self::Unselected => "",
            Self::ClientOnly => "客户端",
            Self::ServerOnly => "服务器",
            Self::None => "不导出",
        }
    }

    /// 短码形式（schema/剪贴板用）：cs / c / s / -
    pub fn code(&self) -> &str {
        match self {
            Self::ClientServer | Self::Unselected => "cs",
            Self::ClientOnly => "c",
            Self::ServerOnly => "s",
            Self::None => "-",
        }
    }

    pub fn options() -> &'static [Export] {
        &[Export::ClientServer, Export::ClientOnly, Export::ServerOnly, Export::None]
    }
}
