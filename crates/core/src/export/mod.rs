mod json;
mod java;
mod xml;
mod lua;
mod go;
mod gdscript;
mod typescript;
mod cpp;
mod csharp;
pub(crate) mod sep_meta;

pub use json::export_all_json;
pub use json::export_json_filtered;
pub use java::export_all_java;
pub use xml::export_all_xml;
pub use xml::export_xml_filtered;
pub use lua::export_all_lua;
pub use go::export_all_go;
pub use gdscript::export_all_gdscript;
pub use typescript::{export_all_typescript, TypeScriptSide};
pub use cpp::export_all_cpp;
pub use csharp::{export_all_csharp, CSharpRuntime};

use serde_json::Value;
use crate::types::BaseType;

/// 数据导出过滤：可选限定 group 和 node。
#[derive(Debug, Clone, Default)]
pub struct DataFilter {
    pub group: Option<String>,
    pub node: Option<String>,
}

impl DataFilter {
    pub fn matches_group(&self, group_name: &str) -> bool {
        self.group.as_ref().map_or(true, |g| g == group_name)
    }
    pub fn matches_node(&self, node_name: &str) -> bool {
        self.node.as_ref().map_or(true, |n| n == node_name)
    }
}

pub enum EmptyStrategy {
    Null,
    Empty,
    Omit,
}

impl EmptyStrategy {
    pub fn from_json_config(s: &str) -> Self {
        match s {
            "omit" => Self::Omit,
            _ => Self::Null,
        }
    }

    pub fn from_xml_config(s: &str) -> Self {
        match s {
            "omit" => Self::Omit,
            _ => Self::Empty,
        }
    }
}

pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn from_config(s: &str) -> Self {
        match s {
            "crlf" => Self::Crlf,
            _ => Self::Lf,
        }
    }

    pub fn normalize(&self, content: &str) -> String {
        let lf = content.replace("\r\n", "\n");
        match self {
            Self::Lf => lf,
            Self::Crlf => lf.replace('\n', "\r\n"),
        }
    }
}

pub fn encode_content(content: &str, encoding: &str) -> Vec<u8> {
    match encoding {
        "utf-8" | "utf8" => content.as_bytes().to_vec(),
        _ => {
            let enc = encoding_rs::Encoding::for_label(encoding.as_bytes())
                .unwrap_or(encoding_rs::UTF_8);
            let (bytes, _, _) = enc.encode(content);
            bytes.into_owned()
        }
    }
}

pub struct ExportOptions {
    pub line_ending: LineEnding,
    pub encoding: String,
}

impl ExportOptions {
    pub fn encode(&self, content: &str) -> Vec<u8> {
        let normalized = self.line_ending.normalize(content);
        encode_content(&normalized, &self.encoding)
    }

    pub fn write_file(&self, path: &std::path::Path, content: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let bytes = self.encode(content);
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Unchanged,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct ExportFile {
    pub path: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone)]
pub struct ExportResult {
    pub files: Vec<ExportFile>,
}

impl ExportResult {
    pub fn added(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Added).count() }
    pub fn modified(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Modified).count() }
    pub fn unchanged(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Unchanged).count() }
    pub fn deleted(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Deleted).count() }
}

pub fn sync_export_dir(
    output_dir: &std::path::Path,
    extension: &str,
    generated: Vec<(std::path::PathBuf, Vec<u8>)>,
) -> anyhow::Result<ExportResult> {
    use std::collections::HashSet;
    use std::path::PathBuf;

    let mut existing: HashSet<PathBuf> = HashSet::new();
    if output_dir.exists() {
        collect_files_recursive(output_dir, extension, &mut existing);
    }

    let mut files = Vec::new();

    for (path, content) in &generated {
        std::fs::create_dir_all(path.parent().unwrap())?;
        existing.remove(path);
        let display = normalize_path(path);

        if path.exists() {
            let old = std::fs::read(path)?;
            if old == *content {
                files.push(ExportFile { path: display, status: FileStatus::Unchanged });
            } else {
                std::fs::write(path, content)?;
                files.push(ExportFile { path: display, status: FileStatus::Modified });
            }
        } else {
            std::fs::write(path, content)?;
            files.push(ExportFile { path: display, status: FileStatus::Added });
        }
    }

    for old_path in &existing {
        std::fs::remove_file(old_path)?;
        files.push(ExportFile { path: normalize_path(old_path), status: FileStatus::Deleted });
    }

    remove_empty_dirs(output_dir);

    Ok(ExportResult { files })
}

fn normalize_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_files_recursive(dir: &std::path::Path, ext: &str, out: &mut std::collections::HashSet<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, ext, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                out.insert(path);
            }
        }
    }
}

fn remove_empty_dirs(dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                remove_empty_dirs(&path);
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
}

/// 单行注释文本：把 desc 折叠成一行。
///
/// desc 来自 .tbl（将来还可能来自 Excel 导入），不能假设它不含换行——换行会直接破坏
/// 生成文件（`//` 注释断行后，后半截变成代码）。所有把 desc 写进注释位的导出器都要过这层。
pub fn comment_line(desc: &str) -> String {
    desc.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// `/** ... */` 块注释文本：在 [`comment_line`] 基础上拆掉 `*/`，否则 desc 会提前闭合注释。
pub fn comment_block(desc: &str) -> String {
    comment_line(desc).replace("*/", "* /")
}

/// 字段行尾注释的文本：优先用 .tbl 里的 desc；没有 desc 时退回 TblFieldType 串，
/// 至少还能看出源类型（Go / C++ 这类注释位只能放一行）。
pub fn field_comment(desc: &str, tbl_type: &str) -> String {
    let d = desc.trim();
    if d.is_empty() { tbl_type.to_string() } else { comment_line(d) }
}

/// 「转换后撞名」的统一文案。导出期报错（[`check_key_collision`]）与验证层
/// （`validate::validate_table_schema` / `validate_constant_schema`）共用，
/// 免得同一个规则在两处写成两种说法。`noun` 传「字段名」或「常量名」。
pub fn key_collision_message(noun: &str, a: &str, b: &str, key: &str) -> String {
    format!(
        "{} \"{}\" 和 \"{}\" 转换后都是 \"{}\"，生成的 Java/Go/C++/C#/TypeScript 代码会出现重复成员；\
         源名不能以 `_` 结尾，也不能含连续 `_` 或 `_` 后接数字",
        noun, a, b, key
    )
}

/// 单个表 / 常量集内的「转换后键名撞车」检测。
///
/// `.tbl` 只允许 `[a-z][a-z0-9_]*` 的字段名（`validate::is_valid_identifier`），它**没有**
/// 禁止结尾下划线和连续下划线，所以 `stack_` / `stack__max` 都是合法源名；但做大小写转换时
/// `to_camel_case` 会把「`_` 后的字符变大写、`_` 后的空位吞掉」，于是 `stack_`→`stack`、
/// `stack__max`→`stackMax`，和 `stack` / `stack_max` 撞成同一个键，生成出来的
/// struct / interface 会出现两个同名字段。
///
/// camelCase 撞名 ⟺ PascalCase 撞名（源名必以小写字母开头，两者只差首字母大小写），
/// 所以**一次检查覆盖所有做转换的后端**（Java / Go / C++ / C# / TypeScript）。
/// Lua / GDScript 原样透传源名，而源名在同表内本就要求不重复，无需检查。
pub fn check_key_collision<'a>(
    node: &str,
    names: impl Iterator<Item = &'a str>,
) -> anyhow::Result<()> {
    let mut seen: std::collections::HashMap<String, &'a str> = std::collections::HashMap::new();
    for name in names {
        let key = to_camel_case(name);
        if let Some(prev) = seen.insert(key.clone(), name) {
            anyhow::bail!("{}: {}", node, key_collision_message("字段名", prev, name, &key));
        }
    }
    Ok(())
}

/// 全项目扫一遍撞名（表用字段名，常量集用条目名）。
/// 五个做大小写转换的导出器都在入口调它一次。
pub fn check_project_key_collisions(project: &crate::model::Project) -> anyhow::Result<()> {
    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            check_key_collision(&table.name, table.schema.fields.iter().map(|f| f.name.as_str()))?;
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            check_key_collision(&constant.name, constant.entries.iter().map(|e| e.name.as_str()))?;
        }
    }
    Ok(())
}

/// 拼「字段名=说明」一行图例，无说明的字段跳过。
/// 给 Lua / GDScript 用——它们导出的是纯数据字面量，没有可供注释的字段声明位，
/// 只能在文件头把字段含义列出来。
pub fn field_legend(entries: &[(String, String)]) -> String {
    entries.iter()
        .filter(|(_, d)| !d.trim().is_empty())
        .map(|(n, d)| format!("{}={}", n, comment_line(d.trim())))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            result.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

pub fn to_pascal_case(s: &str) -> String {
    let camel = to_camel_case(s);
    let mut chars = camel.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

pub fn parse_base_value(raw: &str, bt: &BaseType) -> Value {
    match bt {
        BaseType::Int | BaseType::Long => {
            raw.parse::<i64>().map(Value::from).unwrap_or(Value::from(0))
        }
        BaseType::Float | BaseType::Double => {
            raw.parse::<f64>().map(Value::from).unwrap_or(Value::from(0.0))
        }
        BaseType::Bool => Value::from(raw == "true" || raw == "1"),
        BaseType::Str => Value::from(raw),
        BaseType::Txt => Value::from(raw),
    }
}

/// 各导出器单元测试共用的最小 fixture：只造表头/常量项，不造数据行。
#[cfg(test)]
pub(crate) mod test_fixtures {
    use crate::model::{ConstEntry, Constant, Export, FieldDef, Table, TableSchema};
    use std::path::PathBuf;

    pub fn field(name: &str, desc: &str, tbl_type: &str) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            desc: desc.to_string(),
            tbl_type: tbl_type.to_string(),
            export: Export::ClientServer,
        }
    }

    pub fn entry(name: &str, desc: &str, tbl_type: &str) -> ConstEntry {
        ConstEntry {
            name: name.to_string(),
            tbl_type: tbl_type.to_string(),
            value: "1".to_string(),
            export: Export::ClientServer,
            desc: desc.to_string(),
        }
    }

    pub fn table(name: &str, fields: Vec<FieldDef>) -> Table {
        Table {
            name: name.to_string(),
            path: PathBuf::new(),
            schema: TableSchema { fields: fields.clone() },
            records: Vec::new(),
            dirty: false,
            deleted: false,
            original_records: Vec::new(),
            original_fields: fields,
            saved: true,
            fmt_ver: crate::tbl::TblVersion::V3,
            display_field: None,
        }
    }

    pub fn constant(name: &str, entries: Vec<ConstEntry>) -> Constant {
        Constant {
            name: name.to_string(),
            path: PathBuf::new(),
            entries: entries.clone(),
            dirty: false,
            deleted: false,
            original_entries: entries,
            saved: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_comment_prefers_desc_and_trims() {
        assert_eq!(field_comment("  堆叠上限  ", "int"), "堆叠上限");
        // 没 desc 时退回 TblFieldType 串，至少能看出源类型
        assert_eq!(field_comment("", "List<@Item>"), "List<@Item>");
        assert_eq!(field_comment("   ", "int"), "int");
    }

    #[test]
    fn comment_line_folds_newlines() {
        assert_eq!(comment_line("第一行\n第二行"), "第一行 第二行");
        assert_eq!(comment_line("a\r\nb"), "a b");
        assert_eq!(comment_line("  原样  "), "  原样  ");
    }

    #[test]
    fn comment_block_neutralizes_terminator() {
        // desc 里出现 */ 会提前闭合 /** */ 注释
        assert_eq!(comment_block("结束 */ 之后"), "结束 * / 之后");
        assert_eq!(comment_block("多行\n含 */ 的说明"), "多行 含 * / 的说明");
    }

    #[test]
    fn key_collision_is_caught() {
        // 正常：互不相同的源名
        assert!(check_key_collision("T", ["id", "stack_max", "stack"].into_iter()).is_ok());
        // `stack_` 转出来就是 `stack`，和已有的 `stack` 撞
        let err = check_key_collision("T", ["stack", "stack_"].into_iter()).unwrap_err().to_string();
        assert!(err.contains("stack") && err.contains("T"), "got: {}", err);
        // 连续下划线同理
        assert!(check_key_collision("T", ["stack_max", "stack__max"].into_iter()).is_err());
        // 名字里带下划线但转换后仍不同 → 不算撞
        assert!(check_key_collision("T", ["max_level", "max_exp"].into_iter()).is_ok());

        // `_` 在转换中「不可见」的三种情形，光禁「_ 结尾」只挡住第一种：
        for (a, b) in [("a_", "a"), ("a__b", "a_b"), ("a_1", "a1"), ("stage_1", "stage1")] {
            assert!(check_key_collision("T", [a, b].into_iter()).is_err(),
                "{} 与 {} 应当被判为撞名", a, b);
        }
        // 但单独存在时都合法——问题只出在「同表内两个名字撞了」
        for solo in ["a_", "a__b", "stage_1", "a_1"] {
            assert!(check_key_collision("T", [solo].into_iter()).is_ok(), "{} 单独不该报", solo);
        }
    }

    #[test]
    fn field_legend_joins_and_skips_blank() {
        let entries = vec![
            ("id".to_string(), "物品 ID".to_string()),
            ("raw".to_string(), String::new()),
            ("desc".to_string(), "描述".to_string()),
        ];
        assert_eq!(field_legend(&entries), "id=物品 ID, desc=描述");
        assert_eq!(field_legend(&[]), "");
    }
}
