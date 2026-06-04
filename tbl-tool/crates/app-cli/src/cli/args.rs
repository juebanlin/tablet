//! clap 派生：命令行参数定义。
//!
//! 仅 CLI 二进制使用。GUI 不应引用本模块。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "tbl-cli", version, about = "TBL 配置管理工具 - 命令行模式")]
pub struct Cli {
    /// 工作目录（默认当前目录）
    #[arg(short = 'w', long, default_value = ".")]
    pub workdir: PathBuf,

    /// 显式指定 Project id（覆盖 [app] last_project）
    #[arg(long)]
    pub project: Option<String>,

    /// 覆盖配置项（格式: key=value，可多次使用）
    #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
    pub overrides: Vec<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 导出数据文件和代码
    Export {
        /// 只导出 JSON 数据文件
        #[arg(long)]
        json: bool,
        /// 只导出 XML 数据文件
        #[arg(long)]
        xml: bool,
        /// 只导出 Java 模板类
        #[arg(long)]
        java: bool,
        /// 只导出 Go 模板代码
        #[arg(long)]
        go: bool,
        /// 只导出 Lua 前端文件
        #[arg(long)]
        lua: bool,
    },
    /// 验证所有 .tbl 文件
    Validate,
    /// 生成测试配置数据
    GenerateTest {
        /// 使用空值测试 schema
        #[arg(long)]
        empty: bool,
        /// 指定外部 .tblschema 文件
        #[arg(long)]
        schema: Option<PathBuf>,
        /// 数据行数（0 表示使用默认固定数据）
        #[arg(long, default_value_t = 0)]
        rows: usize,
        /// 随机种子（0 表示使用固定数据，非 0 启用随机生成）
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// 数据格式（json 或 xml），影响 TestMain.java 的初始化方式
        #[arg(long, default_value = "json")]
        format: String,
        /// 测试语言（java / go / none），none 时不生成 TestMain
        #[arg(long, default_value = "java")]
        lang: String,
    },
    /// 列出所有 Project（@02 Project）
    ListProjects,
    /// 列出可用模板（@02 项目模板）
    ListTemplates,
    /// 把根目录 config/ 迁移到 projects/default/
    MigrateLegacy,
    /// 用模板新建 Project
    NewProject {
        /// 模板 id（来自 list-templates）
        #[arg(long)]
        template: String,
        /// Project id（[a-z0-9_-]{1,32}）
        #[arg(long)]
        id: String,
        /// Project 显示名（默认 = id）
        #[arg(long)]
        name: Option<String>,
        /// 创建后切换为 last_project（默认开启）
        #[arg(long, default_value_t = true)]
        switch_after: bool,
    },
}
