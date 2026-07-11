//! clap 派生：命令行参数定义。
//!
//! 仅 CLI 二进制使用。GUI 不应引用本模块。

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "tablet-cli",
    version,
    about = concat!("tablet-cli v", env!("CARGO_PKG_VERSION"), " — TBL 配置管理工具（命令行模式）")
)]
pub struct Cli {
    /// 工作目录（默认当前目录）
    #[arg(short = 'w', long, default_value = ".")]
    pub workdir: PathBuf,

    /// 指定 Project id（覆盖 last_project）。对 util 子命令无效。
    #[arg(long)]
    pub project: Option<String>,

    /// 覆盖配置项（格式: key=value，可多次）。对 util 子命令无效。
    #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
    pub overrides: Vec<String>,

    /// 输出格式。json = 结构化 JSON 输出，适用于查询类命令（project list/info, schema show, validate, export, sep show, util parse-*/stat）
    #[arg(long = "fmt", global = true, value_name = "FORMAT")]
    pub output_format: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn is_json(&self) -> bool {
        self.output_format.as_deref() == Some("json")
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 项目管理
    Project(ProjectArgs),
    /// 结构操作
    Schema(SchemaArgs),
    /// 导出数据/代码
    Export(ExportArgs),
    /// 验证 .tbl 文件（支持五级粒度过滤）
    Validate {
        /// 验证指定组
        #[arg(long)]
        group: Option<String>,
        /// 验证指定节点（依赖 --group）
        #[arg(long, requires = "group")]
        node: Option<String>,
        /// 验证指定列，从 0 开始（依赖 --node）
        #[arg(long, requires = "node")]
        col: Option<u16>,
        /// 验证指定行，从 0 开始（依赖 --col）
        #[arg(long, requires = "col")]
        row: Option<u32>,
    },
    /// Excel 桥接
    Excel(ExcelArgs),
    /// 工作区操作
    Workspace(WorkspaceArgs),
    /// 分隔符查询
    Sep(SepArgs),
    /// 底层工具（无需项目上下文）
    Util(UtilArgs),
    /// 列出可用模板
    ListTemplates,
}

// ─── project ───────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Subcommand, Debug)]
pub enum ProjectAction {
    /// 列出所有 Project
    List,
    /// 显示项目详情
    Info {
        /// 目标项目 id（默认当前活跃项目）
        #[arg(long)]
        id: Option<String>,
    },
    /// 从模板创建 Project
    New {
        /// 模板 id（来自 list-templates）
        #[arg(long)]
        template: String,
        /// Project id（[a-z0-9_-]{1,32}）
        #[arg(long)]
        id: String,
        /// Project 显示名（默认 = id）
        #[arg(long)]
        name: Option<String>,
        /// 创建后切换为 last_project
        #[arg(long, default_value_t = true)]
        switch_after: bool,
    },
    /// 重命名 Project
    Rename {
        /// 目标项目 id
        #[arg(long)]
        id: String,
        /// 新 id（会迁移目录）
        #[arg(long)]
        new_id: Option<String>,
        /// 新显示名
        #[arg(long)]
        new_name: Option<String>,
    },
    /// 删除 Project（不可逆）
    Delete {
        /// 目标项目 id
        #[arg(long)]
        id: String,
        /// 确认删除
        #[arg(long)]
        confirm: bool,
    },
    /// 克隆 Project
    Clone {
        /// 源项目 id
        #[arg(long)]
        source: String,
        /// 新项目 id
        #[arg(long)]
        id: String,
        /// 新项目显示名
        #[arg(long)]
        name: Option<String>,
    },
}

// ─── export ────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ExportArgs {
    #[command(subcommand)]
    pub action: ExportAction,
}

#[derive(Subcommand, Debug)]
pub enum ExportAction {
    /// 导出数据文件（JSON/XML），支持 group/node 粒度
    Data {
        /// 导出 JSON 数据
        #[arg(long)]
        json: bool,
        /// 导出 XML 数据
        #[arg(long)]
        xml: bool,
        /// 仅导出指定组
        #[arg(long)]
        group: Option<String>,
        /// 仅导出指定节点（依赖 --group）
        #[arg(long, requires = "group")]
        node: Option<String>,
        /// 覆盖数据输出目录
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// 导出代码文件，全项目，选语言
    Code {
        /// Java
        #[arg(long)]
        java: bool,
        /// Go
        #[arg(long)]
        go: bool,
        /// Lua
        #[arg(long)]
        lua: bool,
        /// GDScript
        #[arg(long)]
        gdscript: bool,
        /// TypeScript
        #[arg(long)]
        typescript: bool,
        /// C++
        #[arg(long)]
        cpp: bool,
        /// C#（dotnet/unity/godot 三套）
        #[arg(long)]
        csharp: bool,
        /// 全部语言
        #[arg(long)]
        all: bool,
        /// 覆盖 Java/Go 的 package
        #[arg(long)]
        package: Option<String>,
        /// 覆盖 C++/C# 的 namespace
        #[arg(long)]
        namespace: Option<String>,
        /// 覆盖代码输出目录
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// 全量导出（数据 + 代码，CI 一把梭）
    All {
        /// 覆盖公共输出根目录
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
}

// ─── schema ────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct SchemaArgs {
    #[command(subcommand)]
    pub action: SchemaAction,
}

#[derive(Subcommand, Debug)]
pub enum SchemaAction {
    /// 显示项目结构树
    Show,
    /// 添加分组
    AddGroup {
        #[arg(long)]
        name: String,
    },
    /// 添加 Table
    AddTable {
        #[arg(long)]
        group: String,
        #[arg(long)]
        name: String,
    },
    /// 添加 Constant
    AddConstant {
        #[arg(long)]
        group: String,
        #[arg(long)]
        name: String,
    },
    /// 添加 Enum
    AddEnum {
        #[arg(long)]
        group: String,
        #[arg(long)]
        name: String,
    },
    /// 重命名分组
    RenameGroup {
        #[arg(long)]
        old: String,
        #[arg(long)]
        new: String,
    },
    /// 重命名节点
    RenameNode {
        #[arg(long)]
        group: String,
        #[arg(long)]
        old: String,
        #[arg(long)]
        new: String,
    },
    /// 删除分组
    DeleteGroup {
        #[arg(long)]
        name: String,
    },
    /// 删除节点
    DeleteNode {
        #[arg(long)]
        group: String,
        #[arg(long)]
        name: String,
    },
}

// ─── excel ─────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ExcelArgs {
    #[command(subcommand)]
    pub action: ExcelAction,
}

#[derive(Subcommand, Debug)]
pub enum ExcelAction {
    /// 导出分组为 xlsx
    Export {
        /// 分组名
        #[arg(long)]
        group: String,
        /// 仅导出指定节点（逗号分隔）
        #[arg(long, value_delimiter = ',')]
        include: Vec<String>,
        /// 输出路径（默认 ./{group}.xlsx）
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// 导入 xlsx 回读到 .tbl
    Import {
        /// 目标分组名
        #[arg(long)]
        group: String,
        /// xlsx 文件路径
        #[arg(long)]
        file: PathBuf,
    },
}

// ─── workspace ─────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub action: WorkspaceAction,
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceAction {
    /// 保存所有 dirty 节点到磁盘
    Save,
    /// 从磁盘重新加载
    Reload,
    /// 清空所有 .tbl 数据文件（危险）
    Clear {
        /// 确认清空
        #[arg(long)]
        confirm: bool,
    },
}

// ─── sep ───────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct SepArgs {
    #[command(subcommand)]
    pub action: SepAction,
}

#[derive(Subcommand, Debug)]
pub enum SepAction {
    /// 展示分隔符配置
    Show {
        /// 仅展示内置默认值
        #[arg(long)]
        defaults: bool,
        /// 从 tablet.toml 读取分隔符
        #[arg(long)]
        config: Option<PathBuf>,
        /// 从 .tblschema 读取分隔符
        #[arg(long)]
        schema: Option<PathBuf>,
    },
}

// ─── util ──────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct UtilArgs {
    #[command(subcommand)]
    pub action: UtilAction,
}

#[derive(Subcommand, Debug)]
pub enum UtilAction {
    /// 解析 .tbl 文件
    ParseTbl {
        /// .tbl 文件路径
        file: PathBuf,
    },
    /// 解析 .tblschema 文件
    ParseSchema {
        /// .tblschema 文件路径
        file: PathBuf,
    },
    /// 合并多个 .tblschema
    MergeSchema {
        /// .tblschema 文件路径（至少两个）
        #[arg(required = true, num_args = 2..)]
        files: Vec<PathBuf>,
    },
    /// 验证单个 .tbl 文件
    ValidateTbl {
        /// .tbl 文件路径
        file: PathBuf,
        #[command(flatten)]
        sep_opts: SepOpts,
    },
    /// 验证值是否匹配类型
    ValidateType {
        /// 类型表达式（如 "List<int>"）
        r#type: String,
        /// 待验证的值
        value: String,
        #[command(flatten)]
        sep_opts: SepOpts,
    },
    /// 单 .tbl 转 xlsx
    TblToXlsx {
        /// .tbl 文件路径
        file: PathBuf,
        /// 输出 xlsx 路径
        #[arg(short = 'o', long)]
        output: PathBuf,
    },
    /// xlsx 转 .tbl
    XlsxToTbl {
        /// xlsx 文件路径
        file: PathBuf,
        /// schema 文件（做 header 校验）
        #[arg(long)]
        schema: PathBuf,
        /// 输出目录
        #[arg(short = 'o', long)]
        output: PathBuf,
    },
    /// 从 schema 生成项目骨架
    Scaffold {
        /// .tblschema 文件路径
        file: PathBuf,
        /// 输出目录
        #[arg(short = 'o', long)]
        output: PathBuf,
    },
    /// 对比两个 .tbl 文件差异
    Diff {
        /// 第一个 .tbl 文件
        a: PathBuf,
        /// 第二个 .tbl 文件
        b: PathBuf,
    },
    /// 格式化 .tbl 文件
    Fmt {
        /// .tbl 文件或目录路径
        path: PathBuf,
        /// 原地修改
        #[arg(short = 'i', long = "in-place")]
        in_place: bool,
    },
    /// 统计 .tbl 信息
    Stat {
        /// .tbl 文件或目录路径
        path: PathBuf,
    },
    /// 生成测试运行代码（TestMain.java / main.go）
    GenTest {
        /// 测试语言（java / go）
        #[arg(long)]
        lang: String,
        /// 数据格式（json / xml），决定 TestMain 的初始化方式
        #[arg(long, default_value = "json")]
        format: String,
        /// schema 文件路径
        #[arg(long)]
        schema: PathBuf,
        /// 输出目录
        #[arg(short = 'o', long)]
        output: PathBuf,
        /// Java package 名（--lang java 时使用）
        #[arg(long, default_value = "com.game.config")]
        package: String,
        /// Go code_output 路径（--lang go 时用于构造 import path）
        #[arg(long, default_value = "gen/server/go")]
        code_output: String,
    },
}

/// util 子命令的分隔符配置选项
#[derive(Args, Debug)]
pub struct SepOpts {
    /// 从 tablet.toml 读取分隔符
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// 从 .tblschema 读取分隔符（优先级高于 config）
    #[arg(long)]
    pub schema: Option<PathBuf>,
    /// 手动指定分隔符（KEY=VALUE，最高优先级，可多次）
    #[arg(long = "sep", value_name = "KEY=VALUE")]
    pub sep_overrides: Vec<String>,
}
