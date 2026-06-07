use std::path::PathBuf;

use crate::tblschema::TblSchema;

#[derive(Debug, Clone)]
pub struct Project {
    /// CLI 的 `-w` workdir：`tbl-tool.toml` 与 `gen/` 所在目录。仓库级，全局共享。
    pub workdir: PathBuf,
    /// 当前 Project 的根目录：`project.tblschema` / `config/` 在它下面。
    /// - 多 Project 模式：`<workdir>/projects/<id>/`
    /// - 老布局（test fixture / 历史仓库）：`<workdir>` 自身
    pub project_root: PathBuf,
    pub config: WorkspaceConfig,
    /// `project.tblschema` 解析结果，承担"项目身份 + 结构骨架"双职：
    /// - `schema.meta.id / name / created_at / source_template* / category / version` = 项目身份
    /// - `schema.sections` = 结构骨架，由 ops 在 group/node 变动时增量同步
    pub schema: TblSchema,
    pub groups: Vec<Group>,
    /// schema 是否需要写盘：rename / 结构变动时置 true，save 时落 project.tblschema 后清零。
    pub schema_dirty: bool,
    /// project_root 是否还没在磁盘上创建（克隆出的内存项目 = true）。
    /// save 时先 create_dir_all 再写文件，最后清零。
    pub root_pending_create: bool,
}

impl Project {
    /// 数据目录：当前 Project 的 .tbl 根目录。
    /// - 多 Project 模式：`<project_root>/config/`
    /// - 老布局：`<project_root>/<config_dir>`（即 `<workdir>/<config_dir>`）
    pub fn data_dir(&self) -> PathBuf {
        if self.is_multi_project_layout() {
            self.project_root.join("config")
        } else {
            self.project_root.join(&self.config.project.config_dir)
        }
    }

    /// 缓存目录：当前 Project 的 .tbl-cache。
    pub fn cache_dir(&self) -> PathBuf {
        if self.is_multi_project_layout() {
            self.project_root.join(".tbl-cache")
        } else {
            self.project_root.join(&self.config.project.cache_dir)
        }
    }

    /// 是否为多 Project 布局（`<workdir>/projects/<id>/`）。
    pub fn is_multi_project_layout(&self) -> bool {
        self.project_root != self.workdir
    }

    /// 导出根：多 Project 模式 = `<project_root>`（每个 Project 独立 gen/...，避免互相覆盖），
    /// 老布局 = `<workdir>`（保持 fixture / 历史仓库行为）。
    pub fn export_root(&self) -> &std::path::Path {
        if self.is_multi_project_layout() {
            self.project_root.as_path()
        } else {
            self.workdir.as_path()
        }
    }
}

/// `tbl-tool.toml` 反序列化的顶层结构。
/// `[project]` 段对应 `ProjectConfig`（仓库展示信息 + Project 列表配置）；
/// 其它段（export / ui / separators）是 Project 共享的工作空间默认值。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct WorkspaceConfig {
    /// 仓库级配置段：`[project]`。
    #[serde(default = "default_project_section", rename = "project")]
    pub project: ProjectConfig,
    #[serde(default)]
    pub export: Option<ExportConfig>,
    #[serde(default)]
    pub ui: Option<UiConfig>,
    #[serde(default)]
    pub separators: crate::types::SeparatorsSection,
}

fn default_project_section() -> ProjectConfig {
    ProjectConfig {
        last_project: String::new(),
        opened_projects: Vec::new(),
        project_sort: String::new(),
        project_order: Vec::new(),
        config_dir: default_config_dir(),
        cache_dir: default_cache_dir(),
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExportConfig {
    pub json: Option<JsonExport>,
    pub xml: Option<XmlExport>,
    pub server: Option<ServerExport>,
    pub client: Option<ClientConfig>,
    pub encoding: Option<String>,
    pub line_ending: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct JsonExport {
    pub empty_as: Option<String>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct XmlExport {
    pub empty_as: Option<String>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ServerExport {
    pub data_output: Option<String>,
    pub java: Option<JavaExport>,
    pub go: Option<GoExport>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct JavaExport {
    pub package: Option<String>,
    pub code_output: Option<String>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GoExport {
    pub package: Option<String>,
    pub code_output: Option<String>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClientConfig {
    pub lua: Option<LuaExport>,
    pub gdscript: Option<GdScriptExport>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LuaExport {
    pub output: Option<String>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GdScriptExport {
    pub output: Option<String>,
    pub line_ending: Option<String>,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub auto_commit_on_blur: bool,
    #[serde(default)]
    pub realtime_validate: bool,
    pub log_level: Option<String>,
    #[serde(default)]
    pub ref_picker: RefPickerConfig,
    /// 表头 picker 单元格（Table type/export 行）的呼出方式："single" | "double"
    /// 默认 single：表头每列一格、几乎不批量改，单击直出选择器更顺手。
    #[serde(default = "default_picker_trigger_header")]
    pub picker_trigger_header: String,
    /// 数据区 picker 单元格（Ref / Constant type / export 列）的呼出方式："single" | "double"
    /// 默认 double：让单击保留为"瞄准选中"，是 Ctrl+C/V 批量复制 ref id / enum 值的前提。
    #[serde(default = "default_picker_trigger_data")]
    pub picker_trigger_data: String,
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

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct RefPickerConfig {
    /// 引用选择弹窗对 Table 的列展示策略：
    /// - "auto"（默认）：id + 最多 2 个 export=cs 且类型为字符串的辅助列
    /// - "full"：schema 全部字段（除了 export=- 不导出列）
    #[serde(default = "default_ref_strategy")]
    pub default_strategy: String,
}

fn default_ref_strategy() -> String { "auto".to_string() }

fn default_picker_trigger_header() -> String { "single".to_string() }
fn default_picker_trigger_data() -> String { "double".to_string() }

fn default_true() -> bool { true }

/// `[project]` toml 段：仓库级配置，不随 Project 切换。
///
/// 持有 Project 列表管理状态（启动 last_project / 已打开列表 / 排序）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectConfig {
    /// 启动时进入的 Project id；为空 = 扫到的第一个。
    #[serde(default)]
    pub last_project: String,
    /// 启动时自动打开的 Project id 列表（DBeaver-style 多 Project 工作空间）。
    /// 为空时仅打开 `last_project`。
    #[serde(default)]
    pub opened_projects: Vec<String>,
    /// 项目排序方式：id / name / open / created / manual。空字符串=id。
    #[serde(default)]
    pub project_sort: String,
    /// project_sort = "manual" 时使用：用户拖拽得到的 id 序列。
    #[serde(default)]
    pub project_order: Vec<String>,
    /// 历史字段：老布局 `<workdir>/<config_dir>/`。S15-D 之后 Project 用 `<project_root>/config/`，
    /// 该字段仅在迁移期 / 老 fixture 使用，新版本写出时不再带它。
    #[serde(default = "default_config_dir")]
    pub config_dir: String,
    /// 历史字段：同 config_dir 一样保留兼容；新版本走 `<project_root>/.tbl-cache/`。
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
}

fn default_config_dir() -> String { "config".to_string() }

fn default_cache_dir() -> String {
    ".tbl-cache".to_string()
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
