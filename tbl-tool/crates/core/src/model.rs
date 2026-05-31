use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Project {
    pub workdir: PathBuf,
    pub config: ProjectConfig,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    #[serde(default)]
    pub export: Option<ExportConfig>,
    #[serde(default)]
    pub ui: Option<UiConfig>,
    #[serde(default)]
    pub separators: crate::types::SeparatorsSection,
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
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub auto_commit_on_blur: bool,
    #[serde(default)]
    pub realtime_validate: bool,
    pub log_level: Option<String>,
    #[serde(default)]
    pub ref_picker: RefPickerConfig,
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

fn default_true() -> bool { true }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub config_dir: String,
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
}

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
