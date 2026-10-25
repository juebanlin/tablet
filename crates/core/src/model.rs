use std::path::PathBuf;

use crate::enums::{
    Encoding, LineEnding, JsonEmptyAs, XmlEmptyAs, LogLevel,
    PickerTrigger, RefPickerStrategy, ProjectSort,
};
use crate::tblschema::{SchemaMetadata, TblSchema};

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

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// CLI 的 `-w` workdir：`tablet.toml` 与 `gen/` 所在目录。仓库级，全局共享。
    pub workdir: PathBuf,
    /// 当前 Project 的根目录：`project.tblschema` / `config/` 在它下面。
    /// - 多 Project 模式：`<workdir>/projects/<id>/`
    pub project_root: PathBuf,
    /// 项目配置（来自 project.toml），业务逻辑使用。
    pub config: ProjectConfig,
    /// `project.tblschema` 解析结果，承担"项目身份 + 结构骨架"双职：
    /// - `schema.meta.id / name / created_at / source_template* / category / version` = 项目身份
    /// - `schema.sections` = 结构骨架，由 ops 在 group/node 变动时增量同步
    pub schema: TblSchema,
    /// 上次落盘时的 `schema.meta` / `schema.separators` 快照。
    ///
    /// **只有这两样是项目「自己的属性」**——`schema.sections` 是 groups 的投影
    /// （`schema_from_project` 只读组名 / 节点名 / 列定义 / `# @display`），
    /// 拿它跟一份快照比等于把缓存和自己的副本对一遍。所以这里不存 `original_schema`。
    /// 见 [`Project::schema_changed`]。
    pub original_meta: SchemaMetadata,
    pub original_separators: crate::types::SeparatorsSection,
    /// 上次落盘时的 `[export]`（住在 `project.toml`，与 `project.tblschema` 是两个文件）。
    /// 语义同上：只在 load / save 两个点写。
    pub original_export: ExportConfig,
    /// 上次落盘时的 `[validate]`。语义同 `original_export`。
    pub original_validate: ValidateConfig,
    pub groups: Vec<Group>,
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

    /// 从 `groups` 重推 `schema.sections`（`meta` / `separators` 原样保留）。
    ///
    /// 单纯刷新投影缓存，**不碰任何"脏"状态**——脏是算出来的（见
    /// [`Project::schema_changed`]），没有可以写错的标。
    pub fn resync_schema(&mut self) {
        let mut s = crate::tblschema::schema_from_project(&self.groups, false);
        s.meta = self.schema.meta.clone();
        s.separators = self.schema.separators.clone();
        self.schema = s;
    }

    /// `project.tblschema` 现在写盘会和盘上不一样吗。
    ///
    /// 项目「自己的属性」只有 `meta` 与 `separators`；`schema.sections` 是 groups 的
    /// 投影，所以结构变化由**子节点自己**回答（`Group::schema_changed`），不在这里
    /// 另存一份快照去比——那等于把缓存和它自己的副本对一遍。
    ///
    /// 用途：`save_project_files` 决定要不要写 schema 文件。
    pub fn schema_changed(&self) -> bool {
        self.state.is_pending()
            || self.schema.meta != self.original_meta
            || self.schema.separators != self.original_separators
            || self.groups.iter().any(|g| g.schema_changed())
    }

    /// 这个项目现在写盘会有任何东西落地吗——**UI 树的项目行 🟡** 就是它。
    ///
    /// 与 [`schema_changed`](Self::schema_changed) 的区别：这里子层用 `is_dirty()`，
    /// 连"只改了一个格子的值"也算（那个格子确实要写进 .tbl，只是不动 schema 文件）。
    pub fn is_dirty(&self) -> bool {
        self.state.is_pending()
            || self.schema.meta != self.original_meta
            || self.schema.separators != self.original_separators
            || self.toml_changed()
            || self.groups.iter().any(|g| g.is_dirty())
    }

    /// `project.toml` 现在写盘会和盘上不一样吗。
    ///
    /// 这个文件里只有 `[export]`，与 `schema_changed()` 管的是**两个不同的文件**，
    /// 所以要分开问：改 export 不该顺手重写 `project.tblschema`，反之亦然。
    ///
    /// `project.toml` 是**可选的覆盖段**——没改过就不该有文件，让 export 继续从
    /// 工作区 `tablet.toml` 继承（@02.26.3）。
    pub fn toml_changed(&self) -> bool {
        self.config.export != self.original_export
            || self.config.validate != self.original_validate
    }

    /// 项目的「自己的属性」有没有偏离盘上——`meta` / `separators`（进 `project.tblschema`）
    /// 加 `[export]`（进 `project.toml`）。
    ///
    /// **刻意不含 `groups`**：那些是数据与结构，这个谓词喂给项目设置的「全部还原」按钮，
    /// 而那个按钮还原不了数据改动，不该为它们亮起来。
    pub fn settings_changed(&self) -> bool {
        self.schema.meta != self.original_meta
            || self.schema.separators != self.original_separators
            || self.toml_changed()
    }

    /// 把 `meta` / `separators` / `[export]` 三块整体还原成盘上的样子。
    ///
    /// 与节点上的 [`Table::revert_to_saved`] 是同一条路径的两种粒度：都是「把
    /// `original_*` 写回状态」。连续撤销已经落地后，调用方（项目设置）会把这次
    /// 写回包进一条普通历史记录，因此撤销/重做也能回到还原前后的状态；
    /// 这里仍然只负责写回内容，不碰历史栈。
    pub fn revert_settings(&mut self) {
        self.schema.meta = self.original_meta.clone();
        self.schema.separators = self.original_separators.clone();
        self.config.export = self.original_export.clone();
        self.config.validate = self.original_validate.clone();
    }
}

/// `tablet.toml` 反序列化的顶层结构（全局配置）。
/// `[project]` 段对应 `ProjectManagementConfig`（项目管理状态）；
/// 其它段（export / ui / separators）是所有 Project 共享的工作空间默认值。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
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

/// Java 服务端导出
///
/// `deny_unknown_fields`：`[export.server.java]` 下拼错的键要直接报错，不能被 serde 静默吞掉
/// （与 ServerTypeScriptExport 同样的理由，见 @11.3.1 A3 那次事故）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct JavaExport {
    pub package: Option<String>,
    pub code_output: Option<String>,
    /// 用 Lombok 的 `@Getter` 代替逐字段手写 getter，默认开。
    ///
    /// 导出侧只写注解，不打包也不附带 lombok.jar —— 消费方自己加编译期依赖。
    /// 代价与坑（不打包 jar、`boolean` 字段变 `isXxx()`、JDK 23+ 要显式开注解处理）见 @11.3.4。
    #[serde(default)]
    pub lombok: Option<bool>,

    /// 产物里带上「业务扩展保留区间」：类型末尾一对 `// <TBL-CUSTOM-*> ` 标记，
    /// 重新导出时把标记**之间**的手写代码原样搬过去（区间外一律重新生成）。
    ///
    /// 默认 **true**——生成产物默认就带标记，不需要它的人显式关掉。
    /// 契约与边界情况见 @02.11 与 @11.3.4。
    #[serde(default)]
    pub custom_regions: Option<bool>,
}

impl Default for JavaExport {
    fn default() -> Self {
        Self {
            package: Some("com.game.config".to_string()),
            code_output: Some("gen/server/java".to_string()),
            lombok: Some(true),
            custom_regions: Some(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GoExport {
    pub package: Option<String>,
    pub code_output: Option<String>,

    /// 产物里带上「业务扩展保留区间」：类型末尾一对 `// <TBL-CUSTOM-*> ` 标记，
    /// 重新导出时把标记**之间**的手写代码原样搬过去（区间外一律重新生成）。
    ///
    /// 默认 **true**——生成产物默认就带标记，不需要它的人显式关掉。
    /// 契约与边界情况见 @02.11 与 @11.3.4。
    #[serde(default)]
    pub custom_regions: Option<bool>,
}

impl Default for GoExport {
    fn default() -> Self {
        Self {
            package: Some("config".to_string()),
            code_output: Some("gen/server/go".to_string()),
            custom_regions: Some(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CppExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,
    #[serde(default)]
    pub json_lib: Option<crate::enums::CppJsonLib>,

    /// 产物里带上「业务扩展保留区间」：类型末尾一对 `// <TBL-CUSTOM-*> ` 标记，
    /// 重新导出时把标记**之间**的手写代码原样搬过去（区间外一律重新生成）。
    ///
    /// 默认 **true**——生成产物默认就带标记，不需要它的人显式关掉。
    /// 契约与边界情况见 @02.11 与 @11.3.4。
    #[serde(default)]
    pub custom_regions: Option<bool>,
}

impl Default for CppExport {
    fn default() -> Self {
        Self {
            namespace: Some("game::config".to_string()),
            code_output: Some("gen/server/cpp".to_string()),
            json_lib: Some(crate::enums::CppJsonLib::Nlohmann),
            custom_regions: Some(true),
        }
    }
}

/// .NET 服务端导出
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DotNetExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,

    /// 产物里带上「业务扩展保留区间」：类型末尾一对 `// <TBL-CUSTOM-*> ` 标记，
    /// 重新导出时把标记**之间**的手写代码原样搬过去（区间外一律重新生成）。
    ///
    /// 默认 **true**——生成产物默认就带标记，不需要它的人显式关掉。
    /// 契约与边界情况见 @02.11 与 @11.3.4。
    #[serde(default)]
    pub custom_regions: Option<bool>,
}

impl Default for DotNetExport {
    fn default() -> Self {
        Self {
            namespace: Some("Game.Config.Server".to_string()),
            code_output: Some("gen/server/csharp".to_string()),
            custom_regions: Some(true),
        }
    }
}

/// Unity 客户端 C# 导出
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct UnityCSharpExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,

    /// 产物里带上「业务扩展保留区间」：类型末尾一对 `// <TBL-CUSTOM-*> ` 标记，
    /// 重新导出时把标记**之间**的手写代码原样搬过去（区间外一律重新生成）。
    ///
    /// 默认 **true**——生成产物默认就带标记，不需要它的人显式关掉。
    /// 契约与边界情况见 @02.11 与 @11.3.4。
    #[serde(default)]
    pub custom_regions: Option<bool>,
}

impl Default for UnityCSharpExport {
    fn default() -> Self {
        Self {
            namespace: Some("Game.Config.Client".to_string()),
            code_output: Some("gen/client/csharp_unity".to_string()),
            custom_regions: Some(true),
        }
    }
}

/// Godot 客户端 C# 导出
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GodotCSharpExport {
    pub namespace: Option<String>,
    pub code_output: Option<String>,

    /// 产物里带上「业务扩展保留区间」：类型末尾一对 `// <TBL-CUSTOM-*> ` 标记，
    /// 重新导出时把标记**之间**的手写代码原样搬过去（区间外一律重新生成）。
    ///
    /// 默认 **true**——生成产物默认就带标记，不需要它的人显式关掉。
    /// 契约与边界情况见 @02.11 与 @11.3.4。
    #[serde(default)]
    pub custom_regions: Option<bool>,
}

impl Default for GodotCSharpExport {
    fn default() -> Self {
        Self {
            namespace: Some("Game.Config.Client".to_string()),
            code_output: Some("gen/client/csharp_godot".to_string()),
            custom_regions: Some(true),
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

// 注意：两个 TS 导出段都开了 deny_unknown_fields。产物统一为 ESM 源码后，
// 老配置里残留的 `module_kind = "..."` 会在加载时直接报错，而不是被静默忽略
// ——见 docs/11 §3.1 A3。
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerTypeScriptExport {
    pub output: Option<String>,
}

impl Default for ServerTypeScriptExport {
    fn default() -> Self {
        Self {
            output: Some("gen/server/typescript".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientTypeScriptExport {
    pub output: Option<String>,
}

impl Default for ClientTypeScriptExport {
    fn default() -> Self {
        Self {
            output: Some("gen/client/typescript".to_string()),
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
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
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

/// 校验级别的处理方式（项目级「验证设置」，@01.10.7）。
///
/// 照 EDA 的 DRC 思路：级别本身只表达「多严重」，**要不要拦保存是策略**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityAction {
    /// 阻断保存；同时进红框 / 树上 `!`。
    #[default]
    Block,
    /// 只提示：进日志、状态栏、工具栏计数，不拦保存。
    Report,
    /// 完全忽略：不产生任何结果。
    Ignore,
}

impl SeverityAction {
    pub fn as_str(self) -> &'static str {
        match self {
            SeverityAction::Block => "block",
            SeverityAction::Report => "report",
            SeverityAction::Ignore => "ignore",
        }
    }

    /// UI 下拉用的中文标签。
    pub fn label(self) -> &'static str {
        match self {
            SeverityAction::Block => "阻断保存",
            SeverityAction::Report => "仅提示",
            SeverityAction::Ignore => "忽略",
        }
    }

    pub fn from_index(i: i32) -> Self {
        match i {
            1 => SeverityAction::Report,
            2 => SeverityAction::Ignore,
            _ => SeverityAction::Block,
        }
    }

    pub fn to_index(self) -> i32 {
        match self {
            SeverityAction::Block => 0,
            SeverityAction::Report => 1,
            SeverityAction::Ignore => 2,
        }
    }
}

/// 项目级校验策略：每个级别一个动作。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValidateConfig {
    pub error: SeverityAction,
    pub warning: SeverityAction,
    pub info: SeverityAction,
}

impl Default for ValidateConfig {
    fn default() -> Self {
        Self {
            error: SeverityAction::Block,
            warning: SeverityAction::Report,
            info: SeverityAction::Ignore,
        }
    }
}

impl ValidateConfig {
    pub fn action(&self, severity: Severity) -> SeverityAction {
        match severity {
            Severity::Error => self.error,
            Severity::Warning => self.warning,
            Severity::Info => self.info,
        }
    }

    /// 是否与默认策略一致（序列化时默认不输出）。
    pub fn is_default(&self) -> bool { self == &ValidateConfig::default() }

    /// 给 UI 用的三项（级别, 动作）。
    pub fn entries(&self) -> [(Severity, SeverityAction); 3] {
        [
            (Severity::Error, self.error),
            (Severity::Warning, self.warning),
            (Severity::Info, self.info),
        ]
    }
}

/// 项目级配置（来自 project.toml）。
/// 包含项目的导出配置，新建项目时从全局配置拷贝初始值，之后独立维护。
///
/// 注意：
/// - UI 配置仅在全局级别（GlobalConfig），项目级不支持覆盖。
/// - 分隔符配置存储在 TblSchema 中，不在此处。
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize, Default)]
pub struct ProjectConfig {
    #[serde(default)]
    pub export: ExportConfig,
    /// 校验策略（@01.10.7）：每个级别是阻断保存 / 仅提示 / 忽略。
    #[serde(default)]
    pub validate: ValidateConfig,
}

/// 一个组的数据落在什么容器里。**一组一种后端，不混存**（@02.27）。
///
/// | 后端 | 结构真相源 | 数据真相源 |
/// |---|---|---|
/// | `Tbl`  | 各 `.tbl` 自己（自描述） | 各 `.tbl` |
/// | `Xlsx` | `project.tblschema` 的 section | `<group>.xlsx` 的 sheet |
///
/// 节点（`Table` / `Constant` / `EnumDef`）在内存里两种后端完全同型，
/// 所以差别只在「加载时从哪读、保存时往哪写」，不在模型里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupStorage {
    /// `config/<group>/<Node>.tbl`（默认，文本、可 diff）
    #[default]
    Tbl,
    /// `config/<group>/<group>.xlsx`，节点 = sheet
    Xlsx,
}

impl GroupStorage {
    pub fn as_str(self) -> &'static str {
        match self {
            GroupStorage::Tbl => "tbl",
            GroupStorage::Xlsx => "xlsx",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    /// 上次落盘时的组名。改名不在改名那一刻动盘，只记在这里，save 时才搬目录。
    /// 语义同 `Table::original_name`：**只在 load / save 两个点写**。
    pub original_name: String,
    pub dir: PathBuf,
    pub tables: Vec<Table>,
    pub constants: Vec<Constant>,
    pub enums: Vec<EnumDef>,
    pub is_new: bool,
    /// 存储后端（@02.27）。Tbl 组用不到 workbook 路径，Xlsx 组由 [`Group::book_path`] 推导。
    pub storage: GroupStorage,
    /// 加载时 workbook 的 `(mtime, size)`；保存前用来发现「外部改过」。
    /// Tbl 组恒为 `None`。
    pub book_stamp: Option<(std::time::SystemTime, u64)>,
}

impl Group {
    /// Xlsx 组的工作簿路径。**推导**而不是存字段——组改名后不会留下旧路径。
    pub fn book_path(&self) -> PathBuf {
        self.dir.join(format!("{}.xlsx", self.name))
    }

    pub fn is_xlsx(&self) -> bool {
        self.storage == GroupStorage::Xlsx
    }

    /// 影响 `project.tblschema` 的改动：自己的名字 / 新增，或**任何子节点**。
    /// 「A.B.C 里只有 C 改了」在这里体现为 C 说 true，A 和 B 只是把它聚合上来。
    pub fn schema_changed(&self) -> bool {
        self.is_new
            || self.original_name != self.name
            || self.tables.iter().any(|t| node_pending(t.deleted, t.saved, t.schema_changed()))
            || self.constants.iter().any(|c| node_pending(c.deleted, c.saved, c.schema_changed()))
            || self.enums.iter().any(|e| node_pending(e.deleted, e.saved, e.schema_changed()))
    }

    /// UI 树投影用：连「只改了数据」也算（那个格子确实要写进 .tbl）。
    pub fn is_dirty(&self) -> bool {
        self.is_new
            || self.original_name != self.name
            || self.tables.iter().any(|t| node_pending(t.deleted, t.saved, t.dirty))
            || self.constants.iter().any(|c| node_pending(c.deleted, c.saved, c.dirty))
            || self.enums.iter().any(|e| node_pending(e.deleted, e.saved, e.dirty))
    }
}

/// 一个叶子节点「save 会不会有动作」。
///
/// 软删的节点不再看它自己的 `dirty`：删掉一个**落过盘**的节点 save 要 remove 文件
/// （算有动作），而「新建了又没保存就删掉」的节点盘上从来没有过、save 什么也不做。
fn node_pending(deleted: bool, saved: bool, changed: bool) -> bool {
    if deleted { saved } else { changed }
}

/// 校验级别（@01.10.7）：错误拦保存，警告只提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Severity {
    /// 阻断保存（现有所有校验码默认都是这一级）。
    #[default]
    Error,
    /// 只提示，不阻断保存；出现在日志与状态栏，不进红框 / 树 `!`。
    Warning,
    /// 信息级：默认不显示（由项目级「验证设置」决定是忽略 / 提示 / 阻断）。
    Info,
}

impl Severity {
    /// 序列化用短码：`error` / `warn`。
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warn",
            Severity::Info => "info",
        }
    }

    /// 严格解析：只认 `error` / `warn`（大小写不敏感）。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Severity::Error),
            "warn" | "warning" => Some(Severity::Warning),
            "info" => Some(Severity::Info),
            _ => None,
        }
    }
}

/// 节点模式。规则去重与列访问都要按模式区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeMode {
    Table,
    Constant,
    Enum,
}

/// 一条节点级校验规则（@01.10.7）。**列表顺序即优先级**：报告与展示按此顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// 本节点的列名：Table 是字段名；Enum 是 `id`/`name`/`desc`。
    pub column: String,
    pub kind: RuleKind,
    pub level: Severity,
    /// 来源。内置规则由 [`builtin_rules`] 生成：不落盘、不可编辑、不可删除。
    pub source: RuleSource,
}

/// 规则来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSource {
    /// 用户写在 `project.tblschema` 里的。
    User,
    /// 系统内置（`builtin_rules`），只读。
    Builtin,
}

impl Rule {
    /// 表达式形态（磁盘与 UI 摘要共用）：`unique()` / `ref_exists(Lang.key)`。
    pub fn expr_string(&self) -> String {
        self.kind.expr_string()
    }

    /// 是否与系统硬编码的校验重复（重复时规则不生效，只报一条非阻断警告）。
    pub fn conflicts_with_builtin(&self, mode: NodeMode) -> bool {
        self.kind.conflicts_with_builtin(mode, &self.column)
    }
}

/// 规则种类。v1 只有两个内置声明式验证器（S25 第 2 层）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    /// 本列非空值在节点内不重复。
    Unique,
    /// 本列每个非空值都必须出现在目标节点的目标列里。
    RefExists { target_node: String, target_column: String },
    /// 数值区间 `range(min, max)`；空串表示该侧不限。
    Range { min: String, max: String },
    /// 必填：非空。
    NonEmpty,
    /// 文本长度区间 `len(min, max)`（按字符数）；空串表示该侧不限。
    Len { min: String, max: String },
    /// 值必须是给定集合之一 `one_of(a, b, c)`。
    OneOf { values: Vec<String> },
    /// 文本必须匹配正则 `matches("...")`。
    Matches { pattern: String },
    /// 条件必填：当本行 `column` 的值等于 `value` 时，本列必须有值。
    RequiredIf { column: String, value: String },
    /// 跨列比较：本列与 `column` 按 `op` 比较（如 `min_level <= max_level`）。
    Compare { column: String, op: CompareOp },
    // ── 以下两种只由 builtin_rules 生成，schema 里写不出来 ──
    /// 必须是正整数（> 0）。
    PositiveInt,
    /// 必须是 UPPER_SNAKE_CASE。
    UpperSnake,
}

/// 跨列比较运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CompareOp {
    pub fn as_str(self) -> &'static str {
        match self {
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "<" => Some(CompareOp::Lt),
            "<=" => Some(CompareOp::Le),
            ">" => Some(CompareOp::Gt),
            ">=" => Some(CompareOp::Ge),
            "==" | "=" => Some(CompareOp::Eq),
            "!=" | "<>" => Some(CompareOp::Ne),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CompareOp::Lt => "小于",
            CompareOp::Le => "小于等于",
            CompareOp::Gt => "大于",
            CompareOp::Ge => "大于等于",
            CompareOp::Eq => "等于",
            CompareOp::Ne => "不等于",
        }
    }
}

/// 把参数值写成表达式里的形态：含逗号 / 引号 / 括号或首尾空格时加引号。
fn quote_arg(s: &str) -> String {
    let needs = s.contains(',') || s.contains('"') || s.contains('\\') || s.contains('(')
        || s.contains(')') || s.trim() != s;
    if needs {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

impl RuleKind {
    pub fn expr_string(&self) -> String {
        match self {
            RuleKind::Unique => "unique()".to_string(),
            RuleKind::RefExists { target_node, target_column } =>
                format!("ref_exists({}.{})", target_node, target_column),
            RuleKind::Range { min, max } => format!("range({}, {})", min, max),
            RuleKind::NonEmpty => "non_empty()".to_string(),
            RuleKind::Len { min, max } => format!("len({}, {})", min, max),
            RuleKind::OneOf { values } =>
                format!("one_of({})", values.iter().map(|v| quote_arg(v)).collect::<Vec<_>>().join(", ")),
            RuleKind::Matches { pattern } => format!("matches({})", quote_arg(pattern)),
            RuleKind::RequiredIf { column, value } =>
                format!("required_if({}, {})", column, quote_arg(value)),
            RuleKind::Compare { column, op } =>
                format!("compare({}, {})", column, op.as_str()),
            RuleKind::PositiveInt => "positive_int()".to_string(),
            RuleKind::UpperSnake => "upper_snake()".to_string(),
        }
    }

    /// 这条规则是不是只由系统生成（schema 里不允许出现）。
    pub fn is_builtin_only(&self) -> bool {
        matches!(self, RuleKind::PositiveInt | RuleKind::UpperSnake)
    }

    /// 这条用户规则是否与系统内置规则重复（重复时不执行，只报 `RuleRedundant`）。
    pub fn conflicts_with_builtin(&self, mode: NodeMode, column: &str) -> bool {
        builtin_rules(mode).iter().any(|b| {
            b.column == column && match (&b.kind, self) {
                (RuleKind::Unique, RuleKind::Unique) => true,
                (RuleKind::NonEmpty, RuleKind::NonEmpty) => true,
                // 枚举 id 已保证 > 0：只加下限且下限 ≤ 1 时，整条规则不带来新约束。
                (RuleKind::PositiveInt, RuleKind::Range { min, max }) =>
                    max.trim().is_empty()
                        && min.trim().parse::<f64>().map_or(false, |v| v <= 1.0),
                _ => false,
            }
        })
    }
}

/// 系统内置规则（只读）：由内置校验生成，**不落盘、不可编辑、不可删除**。
///
/// **这是「哪些列已被系统保证」的唯一真相源**：规则面板的只读行、
/// CLI 的有效规则输出、`conflicts_with_builtin` 的冗余判断都读它。
/// 内置校验本身目前仍在 validate.rs 里硬编码（第二步再迁到规则引擎）；
/// 两边必须保持一致。
pub fn builtin_rules(mode: NodeMode) -> Vec<Rule> {
    let mk = |column: &str, kind: RuleKind| Rule {
        column: column.to_string(),
        kind,
        level: Severity::Error,
        source: RuleSource::Builtin,
    };
    match mode {
        NodeMode::Table => vec![
            mk("id", RuleKind::Unique),
            mk("id", RuleKind::NonEmpty),
            mk("id", RuleKind::PositiveInt),
        ],
        NodeMode::Enum => vec![
            mk("id", RuleKind::Unique),
            mk("id", RuleKind::NonEmpty),
            mk("id", RuleKind::PositiveInt),
            mk("name", RuleKind::Unique),
            mk("name", RuleKind::NonEmpty),
            mk("name", RuleKind::UpperSnake),
        ],
        NodeMode::Constant => Vec::new(),
    }
}

/// 有效规则 = 系统内置（只读，排在前）+ 用户规则。
pub fn effective_rules(mode: NodeMode, user: &[Rule]) -> Vec<Rule> {
    let mut out = builtin_rules(mode);
    out.extend(user.iter().cloned());
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    /// 上次落盘时的表名，`.tbl` 在盘上叫什么。
    ///
    /// **只在 load / save 两个点写，改名时不动**——这正是连续改名 A→B→C 仍然
    /// 找得到 `A.tbl` 的原因；若记成「上一次改名前叫什么」，第二次改名就把 A
    /// 覆盖掉，save 时会去找一个不存在的 `B.tbl`。
    /// 与 `original_records` / `original_fields` 同源：都是「上次落盘时的样子」。
    pub original_name: String,
    pub path: PathBuf,
    pub schema: TableSchema,
    pub records: Vec<Vec<String>>,
    pub dirty: bool,
    pub deleted: bool,
    /// 上次落盘时的 records 快照，用于 update_dirty() 比对。
    pub original_records: Vec<Vec<String>>,
    /// 上次落盘时的 schema.fields 快照，用于检测表头变更。
    pub original_fields: Vec<FieldDef>,
    pub saved: bool,
    /// 文件格式版本。load 时检测，save 时按原版本序列化。
    /// 新建表默认 V3。
    pub fmt_ver: crate::tbl::TblVersion,
    /// 被引用展示列（@10）：被 `@本表` 引用时展示哪一列。`None` = 显示原始 id；
    /// `Some(列名)` = 展示该列（须为 str/int/txt 叶子类型）。
    /// 持久化在 project.tblschema 段内的 `# @display` 行；本字段是运行时真相源。
    pub display_field: Option<String>,
    /// 上次落盘时的 `display_field`。`# @display` 写在 project.tblschema 里，
    /// 所以要进 `schema_changed()`（@02.26.2）。
    pub original_display_field: Option<String>,
    /// 节点级校验规则（@01.10.7）。顺序即优先级；只存 project.tblschema，不写 `.tbl`。
    pub rules: Vec<Rule>,
    /// 上次落盘时的 `rules` 快照。语义同 `original_display_field`。
    pub original_rules: Vec<Rule>,
}

impl Table {
    /// 影响 `project.tblschema` 的改动。
    ///
    /// **不含 `records`**——改一个格子的值会让本表要写盘，但 schema 文件一个字都不用动。
    /// 父层的聚合必须用这个而不是 `dirty`，否则每次单元格编辑都会把 schema 标脏。
    pub fn schema_changed(&self) -> bool {
        // 改名也是一次未落盘的改动：盘上还是旧文件名，save 时才搬。
        self.original_name != self.name
            || self.schema.fields != self.original_fields
            || self.original_display_field != self.display_field
            || self.rules != self.original_rules
    }

    /// 这个 `.tbl` 现在写盘会和盘上不一样吗。UI 树节点行 🟡 就是它。
    pub fn update_dirty(&mut self) {
        self.dirty = self.records != self.original_records || self.schema_changed();
    }

    /// 把盘上的样子写回来（内容 + 结构 + 身份四样）。
    ///
    /// **不含「存在性」**：新建未保存的节点压根没有盘上的样子（见 [`revertable`]），
    /// 软删的节点还原了内容也仍然是删着的——那两件事归右键的新建/删除管，不混进「还原」。
    ///
    /// 改名**不需要任何文件系统动作**：改名本来就不在改名那一刻动盘（C2，@02.26.1），
    /// 名字改回 `original_name` 等价于从没改过，save 时也不会去搬文件。
    pub fn revert_to_saved(&mut self) {
        self.records = self.original_records.clone();
        self.schema.fields = self.original_fields.clone();
        self.name = self.original_name.clone();
        self.display_field = self.original_display_field.clone();
        self.rules = self.original_rules.clone();
        self.update_dirty();
    }

    /// 「还原」按钮该不该亮：盘上得有它，而且它现在和盘上不一样。
    pub fn revertable(&self) -> bool {
        self.saved && self.dirty
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableSchema {
    pub fields: Vec<FieldDef>,
}

impl TableSchema {
    pub fn index_col(&self) -> Option<usize> {
        self.fields.iter().position(|f| f.name == "id")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub desc: String,
    pub tbl_type: String,
    pub export: Export,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    pub name: String,
    /// 上次落盘时的常量表名。语义同 `Table::original_name`。
    pub original_name: String,
    pub path: PathBuf,
    pub entries: Vec<ConstEntry>,
    pub dirty: bool,
    pub deleted: bool,
    /// 上次落盘时的 entries 快照。
    pub original_entries: Vec<ConstEntry>,
    pub saved: bool,
}

impl Constant {
    /// 影响 `project.tblschema` 的改动：只有名字（常量段在 schema 里只剩段头）。
    ///
    /// 常量**不支持** `# @rule`：`value` 列每行的类型可以不同，列级规则无法定义
    /// 统一的比较口径；`name` 唯一这类约束也已经由内置校验覆盖（@01.8.8）。
    pub fn schema_changed(&self) -> bool {
        self.original_name != self.name
    }

    pub fn update_dirty(&mut self) {
        self.dirty = self.entries != self.original_entries || self.schema_changed();
    }

    /// 把盘上的样子写回来。语义与 [`Table::revert_to_saved`] 一致（含「不含存在性」那条）。
    pub fn revert_to_saved(&mut self) {
        self.entries = self.original_entries.clone();
        self.name = self.original_name.clone();
        self.update_dirty();
    }

    /// 「还原」按钮该不该亮。
    pub fn revertable(&self) -> bool {
        self.saved && self.dirty
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstEntry {
    pub name: String,
    pub tbl_type: String,
    pub value: String,
    pub export: Export,
    pub desc: String,
}

/// 枚举未显式设 `display_field` 时，被引用时默认展示的列（@10）。
/// 枚举固定 id|name|desc，`name` 是天然标签；表无此默认（默认显原始 id）。
pub const ENUM_DEFAULT_DISPLAY_FIELD: &str = "name";

/// 枚举定义：mode=enum 的 .tbl 文件
///
/// 表头固定 id|name|desc 三列，无 #desc/#type/#export/#field 头。
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    /// 上次落盘时的枚举名。语义同 `Table::original_name`。
    pub original_name: String,
    pub path: PathBuf,
    pub entries: Vec<EnumEntry>,
    pub dirty: bool,
    pub deleted: bool,
    /// 上次落盘时的 entries 快照。
    pub original_entries: Vec<EnumEntry>,
    pub saved: bool,
    /// 被引用展示列（@10）：被 `@本枚举` 引用时展示 `name` 还是 `desc`。`None` = 显示原始 id。
    /// 持久化在 project.tblschema 段内的 `# @display` 行；本字段是运行时真相源。
    pub display_field: Option<String>,
    /// 上次落盘时的 `display_field`。语义同 `Table::original_display_field`。
    pub original_display_field: Option<String>,
    /// 节点级校验规则（@01.10.7）。顺序即优先级；只存 project.tblschema。
    pub rules: Vec<Rule>,
    /// 上次落盘时的 `rules` 快照。语义同 `Table::original_rules`。
    pub original_rules: Vec<Rule>,
}

impl EnumDef {
    /// 影响 `project.tblschema` 的改动：名字 + `# @display` + 规则（**不含 entries**）。
    pub fn schema_changed(&self) -> bool {
        self.original_name != self.name
            || self.original_display_field != self.display_field
            || self.rules != self.original_rules
    }

    pub fn update_dirty(&mut self) {
        self.dirty = self.entries != self.original_entries || self.schema_changed();
    }

    /// 把盘上的样子写回来。语义与 [`Table::revert_to_saved`] 一致（含「不含存在性」那条）。
    pub fn revert_to_saved(&mut self) {
        self.entries = self.original_entries.clone();
        self.name = self.original_name.clone();
        self.display_field = self.original_display_field.clone();
        self.rules = self.original_rules.clone();
        self.update_dirty();
    }

    /// 「还原」按钮该不该亮。
    pub fn revertable(&self) -> bool {
        self.saved && self.dirty
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnumEntry {
    pub id: String,
    pub name: String,
    pub desc: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Export {
    ClientServer,
    ClientOnly,
    ServerOnly,
    None,
}

impl Export {
    /// **严格**解析：只认短码 `cs` / `c` / `s` / `-`；空串按 @01.10 的约定算默认 `cs`。
    ///
    /// 读 `.tbl` 与 `.tblschema` 一律走这里——**不合法就是 `None`，由调用方报错**，
    /// 不静默兜底。中文展示值是给人和 Excel 用的，不是磁盘格式的一部分。
    pub fn parse_code(s: &str) -> Option<Self> {
        match s.trim() {
            "" | "cs" => Some(Self::ClientServer),
            "c" => Some(Self::ClientOnly),
            "s" => Some(Self::ServerOnly),
            "-" => Some(Self::None),
            _ => None,
        }
    }

    /// **宽松**解析：短码 + 中文展示值。**只给「人给的输入」用**——
    /// 表头单元格编辑、剪贴板粘贴、以及 Excel 同步回读（策划在 Excel 里看到的就是中文）。
    ///
    /// 认不出来时返回双端导出（UI 侧输入框本来就有下拉，落到这里说明不是手打就是脏数据）。
    pub fn from_input(s: &str) -> Self {
        Self::parse_code(s).unwrap_or(match s.trim() {
            "前后端" => Self::ClientServer,
            "客户端" => Self::ClientOnly,
            "服务器" => Self::ServerOnly,
            "不导出" => Self::None,
            _ => Self::ClientServer,
        })
    }

    /// 展示值（界面用）：前后端 / 客户端 / 服务器 / 不导出
    pub fn display(&self) -> &str {
        match self {
            Self::ClientServer => "前后端",
            Self::ClientOnly => "客户端",
            Self::ServerOnly => "服务器",
            Self::None => "不导出",
        }
    }

    /// 短码形式（tbl/schema/剪贴板用）：cs / c / s / -
    pub fn to_tbl(&self) -> &str {
        self.code()
    }

    /// 短码形式（tbl/schema/剪贴板用）：cs / c / s / -
    pub fn code(&self) -> &str {
        match self {
            Self::ClientServer => "cs",
            Self::ClientOnly => "c",
            Self::ServerOnly => "s",
            Self::None => "-",
        }
    }

    pub fn options() -> &'static [Export] {
        &[Export::ClientServer, Export::ClientOnly, Export::ServerOnly, Export::None]
    }
}

#[cfg(test)]
mod export_parse_tests {
    use super::*;

    #[test]
    fn parse_code_is_strict_and_from_input_is_not() {
        // 短码：两个入口都认
        for (raw, want) in [
            ("", Export::ClientServer), ("cs", Export::ClientServer), (" cs ", Export::ClientServer),
            ("c", Export::ClientOnly), ("s", Export::ServerOnly), ("-", Export::None),
        ] {
            assert_eq!(Export::parse_code(raw), Some(want.clone()), "raw={:?}", raw);
            assert_eq!(Export::from_input(raw), want, "raw={:?}", raw);
        }
        // 中文：只有宽松入口认（UI 展示 / Excel 回读）
        for raw in ["前后端", "客户端", "服务器", "不导出"] {
            assert_eq!(Export::parse_code(raw), None, "严格解析不该接受中文: {}", raw);
        }
        assert_eq!(Export::from_input("前后端"), Export::ClientServer);
        assert_eq!(Export::from_input("不导出"), Export::None);
        // 垃圾值：严格拒绝；宽松兜底成双端（UI 输入框本来就有下拉）
        assert_eq!(Export::parse_code("xyz"), None);
        assert_eq!(Export::from_input("xyz"), Export::ClientServer);
    }
}
