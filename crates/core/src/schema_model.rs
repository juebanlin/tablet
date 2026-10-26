//! schema 文档的中立数据模型：模板、manifest、运行时投影共用。
//!
//! 这里只放结构和纯值校验，不放任何文件格式的解析 / 版本 / 策略。

use crate::rules::Rule;
use crate::types::SeparatorsSection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupStorage {
    /// `config/<group>/<Node>.tbl`（默认，文本、可 diff）
    #[default]
    Tbl,
    /// `config/<group>.xlsx`（config 根级文件），节点 = sheet
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



#[derive(Debug, Clone, Default, PartialEq)]
pub struct SchemaMetadata {
    pub id: String,         // 路径标识：[a-z0-9_-]{1,32}；缺省时由调用方按文件名兜底
    pub name: String,       // 显示文本（含中文）；缺省时 = id
    pub category: String,   // 分类筛选（test / slg / rpg / ...）
    pub version: String,    // semver
    /// 项目身份：创建时间。模板侧通常为空。
    pub created_at: String,
    /// 项目身份：来源模板 id。手动新建 / 模板自身为空。
    pub source_template: String,
    /// 项目身份：来源模板版本。
    pub source_template_version: String,
    /// derive 字段：任一 section.preset 非空 → true。serialize 前由调用方刷新；
    /// 反序列化阶段跟随 # @meta has_preset 行（缺省 false）。
    pub has_preset: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TblSchema {
    pub meta: SchemaMetadata,
    /// 分隔符配置（25 个叶子键，对齐 `SeparatorsSection`）。
    /// 缺省 = `SeparatorsSection::default()`；schema 中 `# @sep` 行覆写。
    /// 加载链路把它复制到 `ProjectConfig.separators`（`merge_config` 时以 schema 为准），
    /// 作为运行时唯一来源。
    pub separators: SeparatorsSection,
    pub sections: Vec<SchemaSection>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SchemaSection {
    pub group: String,
    pub name: String,
    pub mode: SchemaMode,
    /// 该组的数据后端。默认 Tbl；manifest / 模板 section 末尾写 `xlsx` 时为 Xlsx。
    /// 同一组的所有 section 必须一致（解析阶段校验）。
    pub storage: GroupStorage,
    /// Table 段：列声明；Constant / Enum 段：永远空（entries 在项目 .tbl 里，preset 例外）。
    pub fields: Vec<SchemaField>,
    /// 可选预设数据（与 .tbl `---` 之后行同形态）：
    /// - Table：每行 = 一条 record（按列序）
    /// - Constant：每行 = `name | type | value | export | desc`
    /// - Enum：每行 = `id | name | desc`
    /// 仅在「带预设」的 schema 文件 / 导出选项下出现；apply_schema_to_project 按 with_preset 决定是否灌入项目。
    pub preset: Vec<Vec<String>>,
    /// 被引用展示列（@10）：段内 `# @display: <列名>`。`None` = 显示原始 id。
    /// 段内可选行，仅 Table / Enum 段有意义；apply_schema_to_project 会回填到节点。
    pub display: Option<String>,
    /// 节点级校验规则（@01.10.7）：段内 `# @rule: <列> = <表达式> [级别]`。
    /// **行序即优先级**；apply_rules_to_groups 会回填到节点（含 original 快照）。
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SchemaMode {
    #[default]
    Table,
    Constant,
    Enum,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaField {
    pub name: String,
    pub tbl_type: String,
    pub export: String,
    pub desc: String,
}

impl SchemaField {
    pub fn export_display(&self) -> &str {
        match self.export.as_str() {
            "cs" | "" => "前后端",
            "c" => "客户端",
            "s" => "服务器",
            "-" => "不导出",
            _ => "前后端",
        }
    }

    pub fn is_server_export(&self) -> bool {
        matches!(self.export.as_str(), "cs" | "" | "s")
    }
}

/// 校验 metadata id 是否合法 ([a-z0-9_-]{1,32})。
pub fn is_valid_metadata_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

