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

// ─── 业务扩展保留区间 ─────────────────────────────────────────────────────
//
// 生成的 Tpl / 常量 / 枚举里需要放一些手写业务代码（缓存读取、临时处理、枚举上的
// 业务判断函数）。做法：类型末尾放一对注释标记，重新导出时把标记**之间**的内容原样
// 搬过去。契约只有一条——标记是唯一约定：
//
//   · 区间内部  → 原样保留
//   · 区间外部  → 一律重新生成（手改会被覆盖）
//   · 标记写坏（只剩一个 / 顺序颠倒 / 重复）→ 整文件重新生成 + 警告
//   · 标记整个被删 → 当普通文件覆盖，静默（无从判断哪些是手写的）
//
// 参与的后端是「产物里有真类型」的四个：Java / Go / C++ / C#。它们的行注释都是 `//`，
// 所以标记文本完全统一；TS / Lua / GDScript 的产物是数据字面量，放不了方法，不参与。

pub const CUSTOM_REGION_BEGIN: &str = "<TBL-CUSTOM-BEGIN>";
pub const CUSTOM_REGION_END: &str = "<TBL-CUSTOM-END>";

/// 区间首次生成时写进去的提示。用户没动过它时，重新导出得到逐字节相同的结果
/// （`Unchanged` 判定因此天然命中，不会每次导出都把文件重写一遍）。
const CUSTOM_REGION_HINT: &str = "// 业务扩展代码写在这里，重新导出时原样保留";

/// 拼出保留区间（含标记）。`indent` 是落点的缩进——类型体内用四个空格，
/// Go / C++ 枚举那种文件作用域的落点用空串。
pub fn custom_region(indent: &str) -> String {
    format!(
        "{i}// {begin}\n{i}{hint}\n{i}// {end}",
        i = indent,
        begin = CUSTOM_REGION_BEGIN,
        hint = CUSTOM_REGION_HINT,
        end = CUSTOM_REGION_END,
    )
}

#[derive(Debug, PartialEq)]
pub enum RegionOutcome {
    /// 旧文件里没有任何标记（首次生成，或用户把标记整个删了）
    None,
    /// 标记不完整 / 顺序颠倒 / 重复——无法安全定位区间
    Broken,
    /// 取到的区间内容（不含两个标记行本身）
    Found(String),
}

/// 标记行：允许前导空白，因为 Java / C# 的标记是缩进在类型体里的
fn is_marker_line(line: &str, marker: &str) -> bool {
    line.trim() == format!("// {}", marker)
}

pub fn extract_custom_region(old: &str) -> RegionOutcome {
    let lines: Vec<&str> = old.split('\n').collect();
    let begins: Vec<usize> = lines.iter().enumerate()
        .filter(|(_, l)| is_marker_line(l, CUSTOM_REGION_BEGIN))
        .map(|(i, _)| i)
        .collect();
    let ends: Vec<usize> = lines.iter().enumerate()
        .filter(|(_, l)| is_marker_line(l, CUSTOM_REGION_END))
        .map(|(i, _)| i)
        .collect();

    match (begins.len(), ends.len()) {
        (0, 0) => RegionOutcome::None,
        // 恰好一对且顺序正确才算数。多一个标记就说明有东西不对，宁可报 Broken 让用户看一眼，
        // 也不猜他想保留哪一段
        (1, 1) if begins[0] < ends[0] => {
            // 按 \n 切之后每行尾部还挂着 CRLF 的 \r，统一剥掉：让取出来的内容只有 \n，
            // 换行风格由写盘时的 opts.encode 按配置统一决定，这里不掺和
            let body = lines[begins[0] + 1..ends[0]]
                .iter()
                .map(|l| l.strip_suffix('\r').unwrap_or(l))
                .collect::<Vec<_>>()
                .join("\n");
            RegionOutcome::Found(body)
        }
        _ => RegionOutcome::Broken,
    }
}

/// 把 `preserved` 塞进 `new` 的标记之间。返回 `None` 表示新内容里根本没有标记
/// （`custom_regions` 关掉了，或这个产物形态不参与）。
pub fn splice_custom_region(new: &str, preserved: &str) -> Option<String> {
    let lines: Vec<&str> = new.split('\n').collect();
    let begin = lines.iter().position(|l| is_marker_line(l, CUSTOM_REGION_BEGIN))?;
    let end = lines.iter().rposition(|l| is_marker_line(l, CUSTOM_REGION_END))?;
    if end <= begin {
        return None;
    }

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    out.extend(lines[..=begin].iter().map(|l| (*l).to_string()));
    // 区间被用户清空时 preserved 是空串，那就真的什么都不放，别塞回提示
    if !preserved.is_empty() {
        out.push(preserved.to_string());
    }
    out.extend(lines[end..].iter().map(|l| (*l).to_string()));
    Some(out.join("\n"))
}

/// 把旧的字节内容解回文本，好从中取区间。编码解不出来时按 UTF-8 有损解，
/// 最坏情况是区间里的非 UTF-8 字符被打成替换符——比整块丢掉强。
fn decode_content(bytes: &[u8], encoding: &str) -> String {
    match encoding {
        "utf-8" | "utf8" => String::from_utf8_lossy(bytes).into_owned(),
        _ => {
            let enc = encoding_rs::Encoding::for_label(encoding.as_bytes())
                .unwrap_or(encoding_rs::UTF_8);
            let (s, _, _) = enc.decode(bytes);
            s.into_owned()
        }
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

#[derive(Debug, Clone, Default)]
pub struct ExportResult {
    pub files: Vec<ExportFile>,
    /// 保留了内容但没能如期搬过去、以及标记写坏这类「代码可能丢了」的情况。
    /// 调用方必须显示出来——只收集不显示等于没有。
    pub warnings: Vec<String>,
}

impl ExportResult {
    pub fn added(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Added).count() }
    pub fn modified(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Modified).count() }
    pub fn unchanged(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Unchanged).count() }
    pub fn deleted(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Deleted).count() }
}

/// 把生成内容同步到输出目录。
///
/// 收的是**未编码的源码**（`String`），因为保留区间要在这里拼接：旧文件内容只有在这个
/// 函数里才拿得到。编码（换行符归一 + 字符编码）也一并收进来，拼接完再编码。
///
/// 传 `Vec<u8>` 的老签名做不到这件事——那样字符串边界在各后端自己的 collect 闭包里，
/// 到了这儿已经是字节，再解回来就是有损的。
pub fn sync_export_dir(
    output_dir: &std::path::Path,
    extension: &str,
    generated: Vec<(std::path::PathBuf, String)>,
    opts: &ExportOptions,
) -> anyhow::Result<ExportResult> {
    use std::collections::HashSet;
    use std::path::PathBuf;

    let mut existing: HashSet<PathBuf> = HashSet::new();
    if output_dir.exists() {
        collect_files_recursive(output_dir, extension, &mut existing);
    }

    let mut files = Vec::new();
    let mut warnings = Vec::new();

    for (path, content) in &generated {
        std::fs::create_dir_all(path.parent().unwrap())?;
        existing.remove(path);
        let display = normalize_path(path);

        // 保留区间：只有旧文件存在时才需要读它
        let mut spliced: Option<String> = None;
        if path.exists() {
            let old_bytes = std::fs::read(path)?;
            let old = decode_content(&old_bytes, &opts.encoding);
            match extract_custom_region(&old) {
                RegionOutcome::Found(preserved) => {
                    match splice_custom_region(content, &preserved) {
                        Some(s) => spliced = Some(s),
                        None => {
                            // 新内容里没有标记。只可能是 custom_regions 被关掉了
                            // （或这个产物形态本来就不参与）。区间里只有默认提示时不算数，
                            // 否则每次关掉开关都会刷一堆没意义的警告。
                            let meaningful = !preserved.trim().is_empty()
                                && preserved.trim() != CUSTOM_REGION_HINT.trim();
                            if meaningful {
                                warnings.push(format!(
                                    "{}: 新产物里没有保留区间（custom_regions 关了？），\
                                     区间内的手写内容未被保留",
                                    display
                                ));
                            }
                        }
                    }
                }
                RegionOutcome::Broken => {
                    warnings.push(format!(
                        "{}: 保留区间标记不完整（只剩一个 / 顺序颠倒 / 重复），\
                         已整文件重新生成，区间内的手写内容可能已丢失",
                        display
                    ));
                }
                RegionOutcome::None => {}
            }
        }

        let final_src = spliced.as_deref().unwrap_or(content);
        let bytes = opts.encode(final_src);

        if path.exists() {
            let old = std::fs::read(path)?;
            if old == bytes {
                files.push(ExportFile { path: display, status: FileStatus::Unchanged });
            } else {
                std::fs::write(path, &bytes)?;
                files.push(ExportFile { path: display, status: FileStatus::Modified });
            }
        } else {
            std::fs::write(path, &bytes)?;
            files.push(ExportFile { path: display, status: FileStatus::Added });
        }
    }

    for old_path in &existing {
        std::fs::remove_file(old_path)?;
        files.push(ExportFile { path: normalize_path(old_path), status: FileStatus::Deleted });
    }

    remove_empty_dirs(output_dir);

    Ok(ExportResult { files, warnings })
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

    // ─── 业务扩展保留区间 ───────────────────────────────────────────────
    //
    // 这套逻辑写错的后果是**静默吃掉用户手写的代码**，所以边界要逐个钉住。

    fn region_block(body: &str, indent: &str) -> String {
        format!(
            "{i}// {b}\n{body}\n{i}// {e}\n",
            i = indent,
            b = CUSTOM_REGION_BEGIN,
            e = CUSTOM_REGION_END,
            body = body,
        )
    }

    #[test]
    fn extract_finds_region_between_markers() {
        let old = format!("head\n{}tail", region_block("    public int biz() { return 42; }", "    "));
        match extract_custom_region(&old) {
            RegionOutcome::Found(p) => assert_eq!(p, "    public int biz() { return 42; }"),
            other => panic!("应取到区间，实际 {:?}", other),
        }
    }

    #[test]
    fn extract_treats_no_markers_as_none() {
        // 首次生成、或用户把标记整个删了——都无从判断哪些是手写的，静默当普通文件
        assert_eq!(extract_custom_region("public class A {\n}\n"), RegionOutcome::None);
    }

    #[test]
    fn extract_reports_broken_markers() {
        let begin = format!("// {}", CUSTOM_REGION_BEGIN);
        let end = format!("// {}", CUSTOM_REGION_END);
        assert_eq!(extract_custom_region(&format!("a\n{begin}\nbiz()\n")), RegionOutcome::Broken);
        assert_eq!(extract_custom_region(&format!("a\nbiz()\n{end}\n")), RegionOutcome::Broken);
        assert_eq!(extract_custom_region(&format!("{end}\nbiz()\n{begin}\n")), RegionOutcome::Broken);
        // 重复：多一个标记就说明有东西不对，宁可报 Broken 让用户看一眼，也不猜他想保留哪段
        let dup = format!("{begin}\n{begin}\nbiz()\n{end}\n");
        assert_eq!(extract_custom_region(&dup), RegionOutcome::Broken);
    }

    #[test]
    fn extract_handles_crlf_and_indent() {
        // CRLF 文件里标记行尾部带 \r，靠 trim 吃掉；标记允许前导空白（Java 是缩进在类体里的）
        let old = region_block("    // 注释\n    public int biz() { return 1; }", "    ")
            .replace('\n', "\r\n");
        match extract_custom_region(&old) {
            RegionOutcome::Found(p) => {
                assert!(p.contains("public int biz()"), "got {:?}", p);
                assert!(!p.contains('\r'), "取出的内容不该带 \\r：{:?}", p);
            }
            other => panic!("应取到区间，实际 {:?}", other),
        }
    }

    #[test]
    fn extract_keeps_multiline_and_blank_lines() {
        let old = region_block("\n    // 一段\n\n    // 两段", "    ");
        match extract_custom_region(&old) {
            RegionOutcome::Found(p) => assert_eq!(p, "\n    // 一段\n\n    // 两段"),
            other => panic!("实际 {:?}", other),
        }
    }

    #[test]
    fn splice_replaces_hint_with_preserved() {
        // 注意 {} 后面要跟换行：真实生成器用 writeln!，标记行才不会被下一句粘住
        let new = format!("class A {{\n{}\n}}\n", custom_region("    "));
        let out = splice_custom_region(&new, "    public int biz() { return 42; }").unwrap();
        assert!(out.contains("public int biz()"), "got:\n{}", out);
        assert!(!out.contains(CUSTOM_REGION_HINT), "提示行应被替换掉，got:\n{}", out);
        // 标记本身要留着，下次才还能定位
        assert_eq!(out.matches(CUSTOM_REGION_BEGIN).count(), 1);
        assert_eq!(out.matches(CUSTOM_REGION_END).count(), 1);
    }

    #[test]
    fn splice_with_empty_preserved_drops_the_hint() {
        // 用户主动清空区间：那就真的空着，别把提示塞回去
        let new = format!("{}\n\n", custom_region("    "));
        let out = splice_custom_region(&new, "").unwrap();
        assert!(!out.contains(CUSTOM_REGION_HINT), "got:\n{}", out);
    }

    #[test]
    fn splice_returns_none_when_new_has_no_markers() {
        assert!(splice_custom_region("class A {\n}\n", "biz()").is_none());
    }

    #[test]
    fn region_roundtrip_is_byte_identical() {
        // 另一条底线：用户没动过区间时，重新生成 → 拼接 → 结果必须与上次产物完全相同，
        // 否则每次导出都在重写文件（「不变」统计从此失真）
        let gen = || format!("class A {{\n    int x;\n\n{}\n}}\n", custom_region("    "));
        let first = gen();
        let preserved = match extract_custom_region(&first) {
            RegionOutcome::Found(p) => p,
            other => panic!("实际 {:?}", other),
        };
        assert_eq!(splice_custom_region(&gen(), &preserved).unwrap(), first);
    }

    // ─── sync_export_dir ────────────────────────────────────────────────

    fn tmp_dir(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("tbl_region_{}_{}_{}", label, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn lf_opts() -> ExportOptions {
        ExportOptions { line_ending: LineEnding::Lf, encoding: "utf-8".to_string() }
    }

    fn gen_src(field: &str) -> String {
        format!("class A {{\n    int {field};\n\n{}\n}}\n", custom_region("    "))
    }

    const USER_CODE: &str = "    public int biz() { return 42; }";

    fn inject_user_code(file: &std::path::Path) {
        let s = std::fs::read_to_string(file).unwrap()
            .replace(&format!("    {}\n", CUSTOM_REGION_HINT), &format!("{USER_CODE}\n"));
        std::fs::write(file, s).unwrap();
    }

    #[test]
    fn sync_preserves_user_code_in_region() {
        let dir = tmp_dir("preserve");
        let file = dir.join("A.java");
        let opts = lf_opts();

        sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("x"))], &opts).unwrap();
        inject_user_code(&file);

        // 重新生成：生成部分变了，区间内容要原样留下
        let r = sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("y"))], &opts).unwrap();
        let out = std::fs::read_to_string(&file).unwrap();
        assert!(out.contains(USER_CODE), "手写代码被吃掉了:\n{}", out);
        assert!(out.contains("int y;"), "生成部分应照常更新:\n{}", out);
        assert!(!out.contains("int x;"), "旧字段应被换掉:\n{}", out);
        assert_eq!(r.modified(), 1);
        assert!(r.warnings.is_empty(), "不该有警告: {:?}", r.warnings);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_reports_unchanged_when_nothing_moved() {
        // 用户没动过区间时重复导出必须报「不变」，否则每次导出都在重写文件
        let dir = tmp_dir("unchanged");
        let file = dir.join("A.java");
        let opts = lf_opts();

        sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("x"))], &opts).unwrap();
        let r = sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("x"))], &opts).unwrap();
        assert_eq!(r.unchanged(), 1, "应报不变: {:?}", r.files);
        assert!(r.warnings.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_warns_when_region_cannot_be_placed() {
        // 关掉 custom_regions 后新产物里没有标记 —— 手写内容会丢，必须警告
        let dir = tmp_dir("warn_nowhere");
        let file = dir.join("A.java");
        let opts = lf_opts();

        sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("x"))], &opts).unwrap();
        inject_user_code(&file);

        let r = sync_export_dir(&dir, "java",
            vec![(file.clone(), "class A {\n    int x;\n}\n".to_string())], &opts).unwrap();
        assert_eq!(r.warnings.len(), 1, "应有一条警告: {:?}", r.warnings);
        assert!(r.warnings[0].contains("未被保留"), "got {:?}", r.warnings);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_does_not_warn_when_region_holds_only_the_hint() {
        // 区间里只有默认提示就没丢东西，别每次关开关都刷警告
        let dir = tmp_dir("no_noise");
        let file = dir.join("A.java");
        let opts = lf_opts();
        sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("x"))], &opts).unwrap();

        let r = sync_export_dir(&dir, "java",
            vec![(file.clone(), "class A {\n    int x;\n}\n".to_string())], &opts).unwrap();
        assert!(r.warnings.is_empty(), "不该有噪音警告: {:?}", r.warnings);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_warns_on_broken_markers_and_regenerates() {
        let dir = tmp_dir("broken");
        let file = dir.join("A.java");
        let opts = lf_opts();
        sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("x"))], &opts).unwrap();
        inject_user_code(&file);

        // 手滑删掉 END
        let broken = std::fs::read_to_string(&file).unwrap()
            .replace(&format!("    // {}\n", CUSTOM_REGION_END), "");
        std::fs::write(&file, broken).unwrap();

        let r = sync_export_dir(&dir, "java", vec![(file.clone(), gen_src("x"))], &opts).unwrap();
        assert_eq!(r.warnings.len(), 1, "应有一条警告: {:?}", r.warnings);
        assert!(r.warnings[0].contains("标记不完整"), "got {:?}", r.warnings);
        // 整文件重新生成 → 标记回来了
        let out = std::fs::read_to_string(&file).unwrap();
        assert!(out.contains(CUSTOM_REGION_END), "标记应被重新生成:\n{}", out);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
